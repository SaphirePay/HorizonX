#!/usr/bin/env python3
"""A Java plugin's jar to the start of its Rust rewrite on horizon-kit, in seconds: a crate that builds and loads as
it stands, with everything a rewrite otherwise spends its first half hour on already done.

    new_rewrite.py JAR|SOURCE_DIR|SOURCE_ZIP OUT_DIR [--id NAME]
    new_rewrite.py --fetch NAME OUT_DIR [--id NAME]     find it on Modrinth / Hangar / Spiget / GitHub first

  SOURCE_DIR/ZIP: a plugin's source (a GitHub zip): plugin.yml and configs from src/main/resources, the .java/.kt
  files copied to java/ (no decompile). --fetch falls back to a GitHub source zip when no jar can be downloaded.

What it makes:
  plugin.yml              the jar's (commands, aliases, permissions as they were)
  data/                   the jar's config.yml / messages / lang files, written out on first start (include_str!)
  java/                   the decompiled source (Vineflower), to read beside the Rust
  src/lib.rs              settings + messages loaded, a kit Router per command with a Sub for every sub-command
                          found in the source (name, permission, usage where the source says them), /<cmd> reload,
                          the events and timers the plugin uses as notes where they go, a worker when it has timers
  BRIEF.md                classes by size, commands and their subs, events, timers, config and message keys,
                          what each part maps to in the kit and SDK
  tests/check.py          a live check: every command and sub on the test server, with what each should answer
Then: fill each sub from java/, build (cargo build --release), drop the dll in, run tests/check.py.
"""
import json
import pathlib
import re
import shutil
import subprocess
import sys
import zipfile

JAVA = r'C:\Program Files\Java\jdk-25\bin\java.exe'
VINEFLOWER = r'D:\Horizon\plugin-tools\tools\vineflower-java.jar'
KIT = pathlib.Path(__file__).resolve().parent / 'horizon-kit'
SDK = pathlib.Path(r'D:\Horizon\paper-engine\sdk\horizon-bukkit')
DATA_EXT = ('.yml', '.yaml', '.json', '.txt', '.properties', '.conf')
LANG_DIRS = ('lang/', 'langs/', 'language/', 'languages/', 'locale/', 'locales/', 'translations/', 'messages/')
DEFAULT_LANG = re.compile(r'(^|[/_.-])(en|en[_-]us|en[_-]gb|english|default)([_.-]|$)', re.I)


DOWNLOADS = pathlib.Path(r'D:\Horizon\plugin-downloads\rw')
API_TSV = SDK / 'API_INDEX.tsv'
UA = {'User-Agent': 'horizon-new-rewrite/2 (plugin port tooling)'}


def http_json(url):
    import urllib.request  # noqa: PLC0415
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=30) as r:
        return json.loads(r.read().decode('utf-8', 'replace'))


def http_get(url, target):
    import urllib.request  # noqa: PLC0415
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=180) as r:
        data = r.read()
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)
    return target


def q(text):
    import urllib.parse  # noqa: PLC0415
    return urllib.parse.quote(text)


def fetch(name):
    """The plugin's latest Paper/Spigot jar, else its GitHub source zip, into its own folder under
    plugin-downloads/rw. NAME is a plugin name (exact matches only, Modrinth > Hangar > Spiget > GitHub) or an
    explicit source: `github:owner/repo`, `modrinth:slug`, `hangar:owner/slug`, `spiget:id`, or a jar/zip URL."""
    kind, _, ref = name.partition(':') if re.match(r'^(github|modrinth|hangar|spiget):', name) else ('', '', name)
    folder = DOWNLOADS / ident(ref.split('/')[-1])
    folder.mkdir(parents=True, exist_ok=True)
    if name.startswith(('http://', 'https://')):
        target = http_get(name, folder / name.rsplit('/', 1)[-1].split('?')[0])
        return target, name
    tried, near = [], []
    exact = lambda *names: any(ident(n or '') == ident(ref) for n in names)  # noqa: E731

    if kind in ('', 'modrinth'):
        try:
            if kind:
                slugs = [ref]
            else:
                hits = http_json('https://api.modrinth.com/v2/search?limit=8&facets=' + q('[["project_type:plugin"]]') + '&query=' + q(ref))['hits']
                near += [f'modrinth:{h["slug"]}' for h in hits[:4]]
                slugs = [h['slug'] for h in hits if exact(h['slug'], h['title'])]
            for slug in slugs[:1]:
                for v in http_json(f'https://api.modrinth.com/v2/project/{slug}/version'):
                    if set(v.get('loaders', [])) & {'paper', 'spigot', 'bukkit', 'purpur', 'folia'}:
                        f = next((f for f in v['files'] if f.get('primary')), v['files'][0])
                        return http_get(f['url'], folder / f['filename']), f'modrinth:{slug} {v["version_number"]}'
            tried.append('modrinth: no exact match with a paper build')
        except Exception as e:  # noqa: BLE001
            tried.append(f'modrinth: {e}')
    if kind in ('', 'hangar'):
        try:
            if kind:
                owner, slug = ref.split('/', 1)
                found = [(owner, slug)]
            else:
                res = http_json('https://hangar.papermc.io/api/v1/projects?limit=8&q=' + q(ref))['result']
                near += [f'hangar:{r["namespace"]["owner"]}/{r["namespace"]["slug"]}' for r in res[:4]]
                found = [(r['namespace']['owner'], r['namespace']['slug']) for r in res if exact(r['name'], r['namespace']['slug'])]
            for owner, slug in found[:1]:
                v = http_json(f'https://hangar.papermc.io/api/v1/projects/{owner}/{slug}/versions?limit=1')['result'][0]
                d = v['downloads'].get('PAPER') or next(iter(v['downloads'].values()))
                url = d.get('downloadUrl') or d.get('externalUrl')
                if url:
                    target = http_get(url, folder / f'{slug}-{v["name"]}.jar')
                    if zipfile.is_zipfile(target):
                        return target, f'hangar:{owner}/{slug} {v["name"]}'
                    target.unlink(missing_ok=True)
            tried.append('hangar: no exact match')
        except Exception as e:  # noqa: BLE001
            tried.append(f'hangar: {e}')
    if kind in ('', 'spiget'):
        try:
            if kind:
                ids = [(ref, ref)]
            else:
                res = http_json('https://api.spiget.org/v2/search/resources/' + q(ref) + '?size=8&fields=id,name,premium,external')
                near += [f'spiget:{r["id"]} ({r["name"]})' for r in res[:4]]
                ids = [(r['id'], r['name']) for r in res if not r.get('premium') and not r.get('external') and exact(re.split(r'[\s|\-\[(]', r['name'].strip())[0], r['name'])]
            for rid, rname in ids[:1]:
                target = http_get(f'https://api.spiget.org/v2/resources/{rid}/download', folder / f'{ident(str(rname))}.jar')
                if zipfile.is_zipfile(target):
                    return target, f'spiget:{rid}'
                target.unlink(missing_ok=True)
            tried.append('spiget: no exact free match')
        except Exception as e:  # noqa: BLE001
            tried.append(f'spiget: {e}')
    if kind in ('', 'github'):
        try:
            if kind:
                full = ref
                branch = http_json(f'https://api.github.com/repos/{full}').get('default_branch', 'master')
            else:
                repos = []
                for lang in ('java', 'kotlin'):
                    repos += http_json('https://api.github.com/search/repositories?per_page=5&q=' + q(ref) + '+language:' + lang)['items']
                near += [f'github:{r["full_name"]}' for r in repos[:4]]
                hit = next((r for r in repos if exact(r['name'])), None)
                if not hit:
                    raise RuntimeError('no exact repo name')
                full, branch = hit['full_name'], hit['default_branch']
            try:
                rel = http_json(f'https://api.github.com/repos/{full}/releases/latest')
                jar = next((a for a in rel.get('assets', []) if a['name'].endswith('.jar') and not re.search(r'sources|javadoc|api', a['name'], re.I)), None)
                if jar:
                    return http_get(jar['browser_download_url'], folder / jar['name']), f'github release {full} {rel.get("tag_name")}'
            except Exception:  # noqa: BLE001
                pass
            return http_get(f'https://github.com/{full}/archive/refs/heads/{branch}.zip', folder / 'source.zip'), f'github source {full}'
        except Exception as e:  # noqa: BLE001
            tried.append(f'github: {e}')
    sys.exit(f'could not fetch {name}: ' + '; '.join(tried) + ('\nclosest: ' + ', '.join(dict.fromkeys(near)) + '\nrun again with one of them' if near else ''))


def is_source(path):
    """A source tree (folder, or a zip holding src/main) rather than a built jar."""
    if path.is_dir():
        return True
    if zipfile.is_zipfile(path):
        with zipfile.ZipFile(path) as z:
            names = z.namelist()
            return not any(n.endswith('.class') for n in names[:4000]) and any('src/main/' in n for n in names)
    return False


def source_tree(path, out):
    if path.is_dir():
        return path
    target = out / '.source'
    if not target.exists():
        with zipfile.ZipFile(path) as z:
            z.extractall(target)
    return target


def source_plugin_yml(root):
    for name in ('paper-plugin.yml', 'plugin.yml'):
        hits = sorted((h for h in root.rglob(name) if '/build/' not in h.as_posix()), key=lambda p: ('src/main/resources' not in p.as_posix(), len(p.parts)))
        if hits:
            return name, hits[0].read_text(encoding='utf-8', errors='replace'), hits[0].parent
    return None, '', None


def copy_sources(root, out):
    java = out / 'java'
    if java.exists():
        return java
    for f in root.rglob('*'):
        p = f.as_posix()
        if f.suffix in ('.java', '.kt') and '/src/main/' in p and '/test/' not in p:
            rel = p.split('/src/main/', 1)[1].split('/', 1)[-1]
            target = java / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(f, target)
    return java


def semver(raw):
    """`2.11.0-b339`, `5.3.0-SNAPSHOT`, `${version}` -> a version cargo takes (`2.11.0`, `0.1.0`)."""
    m = re.match(r'\s*v?(\d+(?:\.\d+)*)', str(raw or ''))
    parts = [str(int(p)) for p in (m.group(1).split('.') if m else ['0', '1', '0'])][:3]
    while len(parts) < 3:
        parts.append('0')
    return '.'.join(parts)


def api_rows():
    rows = {}
    if not API_TSV.exists():
        return rows
    for line in API_TSV.read_text(encoding='utf-8', errors='replace').splitlines()[1:]:
        cols = line.split('\t')
        if len(cols) >= 4 and '.' in cols[0]:
            simple = cols[0].rsplit('.', 1)[-1].split('$')[-1]
            rows.setdefault(simple, {}).setdefault(cols[1], (cols[2], cols[3], cols[4] if len(cols) > 4 else ''))
    return rows


IMPORT = re.compile(r'^import\s+(?:static\s+)?((?:org\.bukkit|io\.papermc|com\.destroystokyo|net\.kyori\.adventure)[\w.]*)\.(\w+)\s*;?\s*$', re.M)
CALL = re.compile(r'\.(\w+)\(')
IMPORT_ANY = re.compile(r'^import\s+[\w.]+\.(\w+)\s*;?\s*$', re.M)
# Where the kit does better than the raw SDK call: said beside it in the SDK map.
KIT_FOR = {
    'MiniMessage': 'kit: mini::parse / parse_mixed (gradients, hover, click)', 'Component': 'kit: mini::parse for MiniMessage text',
    'Inventory': 'kit: gui::Menu (buttons, pages, click routing)', 'InventoryClickEvent': 'kit: gui::install routes menu clicks',
    'BossBar': 'kit: wire::boss (packet bars, not /bossbar)', 'BarColor': 'kit: wire::boss::colour_of', 'BarStyle': 'kit: wire::boss::overlay_of',
    'ConfigurationSection': 'kit: yaml::{string,list_or_one,int,is_section,keys}', 'FileConfiguration': 'kit: Config / yaml',
    'YamlConfiguration': 'kit: Config::load / yaml', 'ItemStack': 'kit: item::Spec (from config), item::to_base64',
    'Sound': 'kit: sound::key / sound::play', 'Objective': 'kit: wire::set_score (number formats)', 'Scoreboard': 'kit: wire::set_score + packets',
    'BukkitScheduler': 'kit: sync::on_main, worker, app::later', 'BukkitRunnable': 'kit: sync::on_main, worker',
    'Economy': 'kit: hooks::economy', 'Chat': 'kit: hooks::chat', 'PlaceholderAPI': 'kit: hooks::placeholders',
    'PlaceholderExpansion': 'kit: hooks::provide_placeholders',
}


def sdk_map(files):
    """Every Bukkit/Paper/Adventure call the plugin's own code makes, with the SDK's Rust for it (methods matched by
    name against the classes each file imports): the lookups a rewrite otherwise does one at a time."""
    rows = api_rows()
    used, missing = {}, {}
    for f in files:
        text = f.read_text(encoding='utf-8', errors='replace')
        classes = {m.group(2) for m in IMPORT.finditer(text)}
        calls = set(CALL.findall(text))
        for c in classes:
            methods = rows.get(c)
            if not methods:
                missing[c] = missing.get(c, 0) + 1
                continue
            for m in calls & set(methods):
                used.setdefault(f'{c}.{m}', [methods[m], 0])[1] += 1
    lines = [f'- `{k}` -> `{v[0][0]}` `{v[0][1]}`' + (' ~' if v[0][2] else '') + (f'  ({KIT_FOR[k.split(".")[0]]})' if k.split('.')[0] in KIT_FOR else '') for k, v in sorted(used.items(), key=lambda kv: (-kv[1][1], kv[0]))]
    kit_classes = sorted({c for f in files for c in IMPORT_ANY.findall(f.read_text(encoding='utf-8', errors='replace')) if c in KIT_FOR})
    if kit_classes:
        lines = [f'- **kit first:** ' + '; '.join(f'`{c}` -> {KIT_FOR[c][5:]}' for c in kit_classes)] + lines
    gaps = sorted(c for c in missing if c[:1].isupper() and not c.endswith(('Event', 'Exception', 'Handler', 'Priority', 'Listener')))
    return lines, gaps


DEFAULT_CALL = re.compile(r'(?:addDefault|getConfig\(\)\.get(?:String|Int|Boolean|Double|Long))\(\s*"([\w.-]+)"\s*,\s*([^()]{1,80}?)\)')


def code_defaults(files):
    """`config.addDefault("a.b", 5)` / `getConfig().getInt("a.b", 5)`: the config a jar doesn't ship, from code."""
    found = {}
    for f in files:
        for k, v in DEFAULT_CALL.findall(f.read_text(encoding='utf-8', errors='replace')):
            v = v.strip()
            if re.fullmatch(r'-?\d+(\.\d+)?[dDfFlL]?', v):
                val = float(v.rstrip('dDfFlL')) if '.' in v else int(v.rstrip('lL'))
            elif v in ('true', 'false'):
                val = v == 'true'
            elif len(v) >= 2 and v.startswith('"') and v.endswith('"'):
                val = v[1:-1]
            else:
                continue
            found.setdefault(k, val)
    return found


def nested(flat):
    root = {}
    for k, v in sorted(flat.items()):
        cur = root
        parts = k.split('.')
        ok = True
        for p in parts[:-1]:
            nxt = cur.setdefault(p, {})
            if not isinstance(nxt, dict):
                ok = False
                break
            cur = nxt
        if ok and not isinstance(cur.get(parts[-1]), dict):
            cur.setdefault(parts[-1], v)
    return root


def ident(name):
    s = re.sub(r'[^0-9a-zA-Z]+', '_', name).strip('_').lower()
    return ('p_' + s) if s[:1].isdigit() else (s or 'plugin')


def yaml_load(text):
    try:
        import yaml  # noqa: PLC0415
    except ImportError:
        sys.exit('needs PyYAML: run it with the system python (pip install pyyaml)')
    try:
        return yaml.safe_load(text) or {}
    except Exception:
        return {}


def plugin_yml(jar):
    with zipfile.ZipFile(jar) as z:
        for name in ('paper-plugin.yml', 'plugin.yml'):
            if name in z.namelist():
                return name, z.read(name).decode('utf-8', 'replace')
    return None, ''


def decompile(jar, out):
    java = out / 'java'
    if java.exists():
        return java
    classes = out / '.classes'
    with zipfile.ZipFile(jar) as z:
        for entry in z.namelist():
            if entry.endswith('.class') and not entry.startswith(('META-INF/', 'kotlin/')):
                target = classes / entry
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(z.read(entry))
    java.mkdir(parents=True, exist_ok=True)
    subprocess.run([JAVA, '-jar', VINEFLOWER, '--silent', str(classes), str(java)], capture_output=True, timeout=900)
    shutil.rmtree(classes, ignore_errors=True)
    # An obfuscator's encrypted strings and scrambled numbers back to plain constants (jport's deobfuscate.py), so
    # the Java read beside the Rust says what it does (UltimateUI: all 14,365 strings were AES-encrypted).
    try:
        sys.path.insert(0, r'D:\Horizon\plugin-tools\jport')
        import deobfuscate
        decoded = deobfuscate.decode_tree(java)
        if decoded['decoders']:
            print(f"deobfuscated: {decoded['strings']} strings, {decoded['numbers']} numbers ({decoded['decoders']} decoder classes)")
    except Exception as error:  # reading the Java never stops a rewrite's start
        print(f'deobfuscate skipped: {error}')
    return java


SHADED = ('/libs/', '/lib/', '/shaded/', '/shade/', '/metrics/', '/bstats/', '/thirdparty/', '/dependencies/', '/relocated/')


def own_sources(java, main_class):
    """The plugin's own classes: everything under its group (the main class's first three package names, or fewer),
    without shaded libraries."""
    parts = main_class.split('.')[:-1]
    root = java
    for depth in (3, 2, 1):
        if len(parts) >= depth and (java / '/'.join(parts[:depth])).is_dir():
            root = java / '/'.join(parts[:depth])
            break
    return [f for f in sorted([*root.rglob('*.java'), *root.rglob('*.kt')]) if not any(x in f.as_posix().lower() for x in SHADED)]


ANNOTATION = re.compile(r'@(CommandInfo|CommandAlias|Subcommand|CommandPermission|Description|Syntax|Command|SubCommand|SubCommandInfo)\((.*?)\)\s*\n', re.S)
FIELD_STR = re.compile(r'(\w+)\s*=\s*"((?:[^"\\]|\\.)*)"')
FIELD_LIST = re.compile(r'(\w+)\s*=\s*\{([^}]*)\}')
FIELD_INT = re.compile(r'(\w+)\s*=\s*(\d+)')


def annotated_commands(files, base):
    """Command frameworks' annotations (DecentHolograms' @CommandInfo, ACF's @CommandAlias/@Subcommand, ...): each
    with its usage, description, permissions, minimum arguments and aliases, and where it is."""
    found = []
    for f in files:
        text = f.read_text(encoding='utf-8', errors='replace')
        for m in ANNOTATION.finditer(text):
            body = m.group(2)
            info = {'kind': m.group(1), 'where': f'{f.relative_to(base).as_posix()}:{text.count(chr(10), 0, m.start()) + 1}'}
            if body.strip().startswith('"'):
                info['value'] = body.strip().strip('"')
            for k, v in FIELD_STR.findall(body):
                info[k] = v
            for k, v in FIELD_LIST.findall(body):
                info[k] = re.findall(r'"([^"]*)"', v)
            for k, v in FIELD_INT.findall(body):
                info.setdefault(k, int(v))
            found.append(info)
    return found


def literal_path(usage):
    """`/dh d attribute <name>` -> ['d', 'attribute'] (the words before the first argument, without the command)."""
    words = usage.strip().lstrip('/').split()
    out = []
    for w in words[1:]:
        if w[:1] in '<[(' or '|' in w and w[:1] in '<[':
            break
        out.append(w.split('|')[0])
    return out


SUB_PATTERNS = [
    re.compile(r'case\s+"([A-Za-z][\w-]{0,30})"\s*(?:->|:)'),
    re.compile(r'args\[0\]\.equalsIgnoreCase\("([A-Za-z][\w-]{0,30})"\)'),
    re.compile(r'args\[0\]\.equals\("([A-Za-z][\w-]{0,30})"\)'),
    re.compile(r'@(?:Subcommand|CommandAlias|Command)\("([A-Za-z][\w|-]{0,40})"\)'),
    re.compile(r'(?:literal|subcommand|withSubcommand|new\s+\w*SubCommand\w*)\(\s*"([A-Za-z][\w-]{0,30})"'),
]
PERM = re.compile(r'hasPermission\("([\w.*-]+)"\)')
EVENT = re.compile(r'@EventHandler[^\n]*\n\s*(?:public|private|protected)?\s*(?:final\s+)?void\s+(\w+)\s*\(\s*(?:final\s+)?(\w+)\s+\w+')
TIMER = re.compile(r'(runTask(?:Timer|Later)(?:Asynchronously)?)\([^;]*?,\s*(\d+)L?(?:\s*,\s*(\d+)L?)?\s*\)')
MESSAGE_KEY = re.compile(r'"((?:messages?|lang)\.[\w.-]+)"|getString\("([\w.-]+)"')


def scan(files, base):
    subs, perms, events, timers, keys = {}, set(), [], [], set()
    for f in files:
        text = f.read_text(encoding='utf-8', errors='replace')
        rel = f.relative_to(base).as_posix()
        is_command = 'onCommand' in text or 'CommandExecutor' in text or 'TabCompleter' in text or 'Subcommand' in text or 'SubCommand' in text
        if is_command:
            for pat in SUB_PATTERNS:
                for m in pat.finditer(text):
                    for name in m.group(1).split('|'):
                        line = text.count('\n', 0, m.start()) + 1
                        subs.setdefault(name.lower(), f'{rel}:{line}')
        perms.update(PERM.findall(text))
        for m in EVENT.finditer(text):
            events.append((m.group(2), m.group(1), rel))
        for m in TIMER.finditer(text):
            timers.append((m.group(1), m.group(2), m.group(3) or '', rel))
        for m in MESSAGE_KEY.finditer(text):
            keys.add(m.group(1) or m.group(2))
    # Words that are Java/Bukkit, not sub-commands.
    for noise in ('help', 'true', 'false', 'null', 'default'):
        subs.pop(noise, None)
    return subs, sorted(perms), events, timers, sorted(keys)


def sdk_events():
    names = set()
    for f in (SDK / 'src').glob('*.rs'):
        names.update(re.findall(r'pub struct (\w+Event)\b', f.read_text(encoding='utf-8', errors='replace')))
    return names


def guess_perm(sub, perms, command):
    for p in perms:
        if p.lower().endswith('.' + sub) or p.lower().endswith('.' + sub + '.use') or f'.{sub}.' in p.lower():
            return p
    return None


CARGO = '''[package]
name = "{id}"
version = "{version}"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
horizon-bukkit = {{ path = "{sdk}" }}
horizon-kit = {{ path = "{kit}" }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
serde_yaml = "0.9"

[profile.release]
panic = "unwind"
opt-level = 3
codegen-units = 4

[workspace]
'''


def rust_str(s):
    return json.dumps(s, ensure_ascii=False)


def main():
    args = sys.argv[1:]
    if len(args) < 2:
        sys.exit(__doc__)
    id_override = args[args.index('--id') + 1] if '--id' in args else None
    origin = ''
    if args[0] == '--fetch':
        jar, origin = fetch(args[1])
        out = pathlib.Path(args[2])
        print(f'fetched {jar} ({origin})', flush=True)
    else:
        jar, out = pathlib.Path(args[0]), pathlib.Path(args[1])
    out.mkdir(parents=True, exist_ok=True)
    source = is_source(jar)
    res_dir = None
    if source:
        tree = source_tree(jar, out)
        yml_name, yml_text, res_dir = source_plugin_yml(tree)
    else:
        yml_name, yml_text = plugin_yml(jar)
    meta = yaml_load(yml_text)
    name = str(meta.get('name') or jar.stem)
    id_ = id_override or ident(name)
    version = semver(meta.get('version'))
    main_class = str(meta.get('main') or '')
    commands = meta.get('commands') or {}
    if isinstance(commands, list):
        commands = {c: {} for c in commands}

    (out / 'src').mkdir(exist_ok=True)
    (out / 'data').mkdir(exist_ok=True)
    (out / 'tests').mkdir(exist_ok=True)
    (out / 'plugin.yml').write_text(yml_text.replace('paper-plugin.yml', 'plugin.yml') if yml_text else f'name: {name}\nversion: {version}\nmain: {id_}\n', encoding='utf-8')
    data_files, other_langs = [], []
    if source:
        entries = {f.relative_to(res_dir).as_posix(): f.read_bytes() for f in res_dir.rglob('*') if f.is_file()} if res_dir else {}
    else:
        with zipfile.ZipFile(jar) as z:
            entries = {e: z.read(e) for e in z.namelist() if not e.endswith('/') and not e.endswith('.class') and z.getinfo(e).file_size < 4_000_000}
    for entry, data in entries.items():
        low = entry.lower()
        if entry.startswith('META-INF/') or entry in ('plugin.yml', 'paper-plugin.yml', 'bungee.yml', 'velocity-plugin.json', 'fabric.mod.json'):
            continue
        wanted = (low.endswith(DATA_EXT) and '/' not in entry.strip('/')) or (low.endswith(DATA_EXT) and entry.count('/') <= 2 and not low.startswith(('org/', 'com/', 'net/', 'io/', 'me/', 'de/', 'kotlin/', 'assets/')))
        if not wanted:
            continue
        if any(low.startswith(d) or f'/{d}' in low for d in LANG_DIRS) and not DEFAULT_LANG.search(low.rsplit('/', 1)[-1]):
            other_langs.append(entry)
            continue
        target = out / 'data' / entry
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        data_files.append(entry)
    (out / 'Cargo.toml').write_text(CARGO.format(id=id_, version=version, sdk=SDK.as_posix(), kit=KIT.as_posix()), encoding='utf-8')

    java = copy_sources(tree, out) if source else decompile(jar, out)
    files = own_sources(java, main_class)
    subs, perms, events, timers, keys = scan(files, java)
    annotated = annotated_commands(files, java)
    sub_info = {}
    for a in annotated:
        usage = a.get('usage') or a.get('syntax') or ''
        path = literal_path(usage) if usage else []
        if path:
            sub = path[0].lower()
            info = sub_info.setdefault(sub, {'usage': ' '.join(usage.split()[1:]), 'about': a.get('description', ''), 'perm': (a.get('permissions') or [a.get('permission')] or [None])[0], 'min': a.get('minArgs', 0), 'where': a['where'], 'aliases': a.get('aliases') or [], 'nested': []})
            if len(path) > 1:
                info['nested'].append(' '.join(path[1:]))
            subs.setdefault(sub, a['where'])
        elif a.get('aliases') and not commands:
            # The main command of a framework that registers its own (no commands in plugin.yml).
            main = name.lower().replace(' ', '')
            commands[main] = {'aliases': a.get('aliases'), 'description': a.get('description', ''), 'permission': (a.get('permissions') or [None])[0]}
    if sub_info:
        # Annotations are the plugin's own list: the `case "x"` guesses go (they catch argument words too), and a
        # short form (`/dh d attribute` where `d` is an alias of `displays`) folds into the sub it names.
        alias_of = {}
        for sub, info in sub_info.items():
            for a in info.get('aliases', []):
                if a.lower() != sub:
                    alias_of.setdefault(a.lower(), sub)
        for short in [s_ for s_ in list(sub_info) if s_ in alias_of and alias_of[s_] in sub_info]:
            target = sub_info[alias_of[short]]
            target['nested'] += [sub_info[short]['usage'].split(' ', 1)[-1].split('<')[0].strip()] + sub_info[short]['nested']
            del sub_info[short]
        subs = {k: v['where'] for k, v in sub_info.items()}
    if not commands and subs:
        commands[name.lower().replace(' ', '')] = {'description': f'{name} (registered in its code; added here)'}
    if commands and not re.search(r'(?m)^commands:', yml_text):
        extra = ['commands:']
        for c, spec in commands.items():
            extra.append(f'  {c}:')
            if spec.get('description'):
                extra.append(f'    description: {json.dumps(spec["description"])}')
            if spec.get('aliases'):
                extra.append(f'    aliases: {json.dumps(spec["aliases"])}')
        (out / 'plugin.yml').write_text(yml_text.rstrip() + '\n' + '\n'.join(extra) + '\n', encoding='utf-8')
    known_events = sdk_events()
    defaults = code_defaults(files)
    if defaults and not any(f.lower() == 'config.yml' for f in data_files):
        import yaml  # noqa: PLC0415
        (out / 'data' / 'config.yml').write_text('# Built by new_rewrite.py from the defaults in the Java code (the jar ships no config.yml): check it.\n' + yaml.safe_dump(nested(defaults), sort_keys=False, allow_unicode=True), encoding='utf-8')
        data_files.append('config.yml')
    api_lines, api_gaps = sdk_map(files)
    perm_nodes = sorted(set(perms) | set((meta.get('permissions') or {}).keys()))
    config_file = next((f for f in data_files if f.lower() == 'config.yml'), None)
    lang_file = next((f for f in data_files if re.search(r'(messages|lang|language|locale)[^/]*\.ya?ml$', f.lower()) and not f.lower().startswith('config')), None)
    admin = next((p for p in perm_nodes if p.lower().endswith(('.admin', '.*'))), None)

    # src/lib.rs
    L = []
    L.append(f'//! {name} {meta.get("version", "")} rewritten in Rust for Horizon on horizon-kit. The Java it follows is in java/;')
    L.append('//! BRIEF.md says what each part is. Started by new_rewrite.py: every sub-command answers "not yet" until written.')
    L.append('//! This file is a stub that builds: replace it whole (delete it first, `rm src/lib.rs`, then write yours).')
    L.append('')
    L.append('use horizon_bukkit::{Plugin, bukkit_plugin};')
    L.append('#![allow(unused_imports)]') if False else None
    L.append('use horizon_kit::{Config, Ctx, Lang, Res, Router, Sub};')
    L.append('use std::sync::{LazyLock, Mutex};')
    L.append('')
    L.append('static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);')
    L.append('static SETTINGS: Mutex<Option<(Config, Lang)>> = Mutex::new(None);')
    if timers:
        L.append('static WORKER: horizon_kit::worker::Worker = horizon_kit::worker::Worker::new();')
    L.append('')
    if config_file:
        L.append(f'const DEFAULT_CONFIG: &str = include_str!("../data/{config_file}");')
    if lang_file:
        L.append(f'const DEFAULT_LANG: &str = include_str!("../data/{lang_file}");')
    L.append('')
    L.append('/// config.yml and the messages, read again (`/<command> reload`, start).')
    L.append('pub fn reload() {')
    L.append('    let Some(plugin) = *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) else { return };')
    L.append('    let folder = plugin.get_data_folder();')
    L.append('    let config = ' + (f'Config::load(&folder, "config.yml", DEFAULT_CONFIG);' if config_file else 'Config::default();'))
    L.append('    let lang = ' + (f'Lang::load(&folder, {rust_str(lang_file)}, DEFAULT_LANG, "prefix");' if lang_file else 'Lang::of(&config, "prefix");'))
    L.append('    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some((config, lang));')
    L.append('}')
    L.append('')
    L.append('/// A message of the messages file, filled in.')
    L.append('pub fn msg(key: &str, args: &[&str]) -> String {')
    L.append('    SETTINGS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or_else(|| key.to_owned(), |(_, l)| l.get(key, args))')
    L.append('}')
    L.append('')
    L.append('/// A setting of config.yml.')
    L.append('pub fn config<R>(f: impl FnOnce(&Config) -> R) -> R {')
    L.append('    let held = SETTINGS.lock().unwrap_or_else(|e| e.into_inner());')
    L.append('    f(held.as_ref().map(|(c, _)| c).unwrap_or(&Config::default()))')
    L.append('}')
    L.append('')
    L.append('fn not_yet(c: &Ctx) -> Res {')
    L.append(f'    Err(format!("&e/{{}} {{}}: not written yet in the Rust {name}.", c.label, c.sub))')
    L.append('}')
    L.append('')
    L.append('fn help_sub(c: &Ctx) -> Res {')
    L.append(f'    {ident(next(iter(commands))).upper()}.help(c.sender, c.label);' if commands else '    let _ = c;')
    L.append('    Ok(())')
    L.append('}')
    L.append('')
    L.append('fn reload_sub(c: &Ctx) -> Res {')
    L.append('    reload();')
    L.append(f'    c.tell("&a{name} reloaded.");')
    L.append('    Ok(())')
    L.append('}')
    L.append('')
    routers = []
    sub_names = sorted(subs)
    for cmd, spec in commands.items():
        spec = spec or {}
        rid = ident(cmd).upper()
        routers.append((cmd, rid))
        L.append(f'/// /{cmd}' + (f' ({", ".join(spec.get("aliases"))})' if isinstance(spec.get('aliases'), list) else '') + (f': {spec.get("description")}' if spec.get('description') else ''))
        L.append(f'static {rid}: LazyLock<Router> = LazyLock::new(|| {{')
        L.append(f'    Router::new({rust_str(cmd)})')
        if admin:
            L.append(f'        .admin({rust_str(admin)})')
        # Only the first command gets the discovered subs (they're usually one command's); the rest run bare.
        if (cmd, rid) == routers[0]:
            if 'reload' not in subs:
                perm = guess_perm('reload', perm_nodes, cmd) or admin
                L.append(f'        .sub(Sub::new("reload", reload_sub)' + (f'.perm({rust_str(perm)})' if perm else '') + '.about("Read the config and messages again"))')
            for sub in sub_names:
                info = sub_info.get(sub, {})
                perm = info.get('perm') or guess_perm(sub, perm_nodes, cmd)
                handler = {'reload': 'reload_sub', 'help': 'help_sub'}.get(sub, 'not_yet')
                L.append(f'        // {info.get("where") or subs[sub]}' + (f'  (and under it: {", ".join(list(dict.fromkeys(n for n in info["nested"] if n))[:40])})' if info.get('nested') else ''))
                line = f'        .sub(Sub::new({rust_str(sub)}, {handler})'
                for alias in [a for a in info.get('aliases', []) if a.lower() != sub][:4]:
                    line += f'.alias({rust_str(alias)})'
                if perm:
                    line += f'.perm({rust_str(perm)})'
                if info.get('usage'):
                    line += f'.usage({rust_str(info["usage"])})'
                if info.get('about'):
                    line += f'.about({rust_str(info["about"])})'
                if info.get('min'):
                    line += f'.args({int(info["min"]) - 1 if int(info["min"]) > 0 else 0})'
                L.append(line + ')')
        else:
            L.append('        .bare(not_yet)')
        L.append('});')
        L.append('')
    L.append('fn on_enable(plugin: Plugin) {')
    L.append('    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);')
    L.append('    reload();')
    for cmd, rid in routers:
        L.append(f'    plugin.set_executor({rust_str(cmd)}, |s, l, a| {rid}.run(s, l, a));')
        L.append(f'    plugin.set_tab_completer({rust_str(cmd)}, |s, _, a| {rid}.complete(s, a));')
    if events:
        L.append('    // The events the Java plugin listens to (Java handler: file). Write each as')
        L.append('    // plugin.register_event(|event: &mut horizon_bukkit::...::XEvent| { ... });')
        for ev, method, rel in sorted(set(events)):
            mark = '' if ev in known_events else '   <- no SDK event of this name: api_lookup / sdk_search'
            L.append(f'    //   {ev}: {method} in {rel}{mark}')
    if timers:
        L.append('    // The Java plugin\'s timers (kind, delay, period, file). Per-tick work runs on the worker thread:')
        for kind, delay, period, rel in timers:
            L.append(f'    //   {kind} {delay} {period} in {rel}')
        L.append(f'    WORKER.start({rust_str(name)}, tick);')
        L.append('    plugin.run_task_timer(1, 1, || {')
        L.append('        // Read what the tick needs from the game here (server thread), then:')
        L.append('        horizon_kit::packets::flush();')
        L.append('        WORKER.wake();')
        L.append('    });')
    L.append(f'    horizon_bukkit::horizon_mod::log({rust_str(f"[{name}] enabled (Rust)")});')
    L.append('}')
    L.append('')
    if timers:
        L.append('/// One tick on the worker thread (no game queries here: see horizon_kit::worker).')
        L.append('fn tick() {}')
        L.append('')
    L.append('fn on_disable(_plugin: Plugin) {')
    if timers:
        L.append('    WORKER.stop();')
    L.append('    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;')
    L.append('}')
    L.append('')
    L.append('bukkit_plugin!(on_enable, on_disable);')
    (out / 'src' / 'lib.rs').write_text('\n'.join(L) + '\n', encoding='utf-8')

    # BRIEF.md
    sizes = sorted(((f.stat().st_size, f.relative_to(java).as_posix()) for f in files), reverse=True)
    B = [f'# {name} {meta.get("version", "")}: rewrite brief', '', f'Main class `{main_class}`. {len(files)} own classes, {sum(s for s, _ in sizes) // 1024} KB of {"source" if source else "Java"} (java/).',
         f'From: {origin or jar}. Crate version {version}.', '']
    B += ['## Commands', '']
    for cmd, spec in commands.items():
        spec = spec or {}
        B.append(f'- `/{cmd}`' + (f' aliases {spec.get("aliases")}' if spec.get('aliases') else '') + (f', permission `{spec.get("permission")}`' if spec.get('permission') else '') + (f': {spec.get("description")}' if spec.get('description') else ''))
    B += ['', '## Sub-commands found in the source', '']
    B += [f'- `{s}` ({subs[s]})' for s in sub_names] or ['- none found: read the command classes']
    B += ['', '## Permissions', ''] + [f'- `{p}`' for p in perm_nodes]
    B += ['', '## Events', '']
    for ev, method, rel in sorted(set(events)):
        B.append(f'- {ev} -> {method} ({rel})' + ('' if ev in known_events else ' **no SDK event of this name**'))
    B += ['', '## Timers', ''] + [f'- {k} delay {d} period {p} ({r})' for k, d, p, r in timers]
    B += ['', '## Files', ''] + [f'- data/{f}' for f in data_files]
    if other_langs:
        B.append(f'- (and {len(other_langs)} other language files, not copied: {", ".join(sorted({o.rsplit("/", 1)[-1] for o in other_langs})[:12])}...)')
    if defaults:
        B += ['', '## Config defaults found in the code', ''] + [f'- `{k}`: {json.dumps(v)}' for k, v in list(defaults.items())[:120]]
    B += ['', f'## SDK map ({len(api_lines)} calls the plugin makes, most used first: Java -> Rust)', ''] + (api_lines[:220] or ['- none matched'])
    if api_gaps:
        B += ['', '## Bukkit classes it imports that the SDK has no rows for (look these up, or work around)', '', '- ' + ', '.join(api_gaps)]
    B += ['', '## Keys read from config / messages', ''] + [f'- `{k}`' for k in keys[:200]]
    B += ['', '## Biggest classes (write these first, they are most of the work)', ''] + [f'- {s // 1024} KB {r}' for s, r in sizes[:25]]
    B += ['', '## Kit map (`use horizon_kit::prelude::*;` brings all of it; recipes in KIT.md)', '',
          '- messages: `msg("key", &[args])` (Lang: {prefix}, %s, {0}); settings: `config(|c| c.int("path", default))`; any YAML node: `yaml::{string, list_or_one, int, is_section, keys}`',
          '- commands: the Router above; a sub = `fn(&Ctx) -> Res` (`c.player()?`, `c.int(0, "amount")?`, `c.online(0)?`, `c.tell(..)`)',
          '- menus: `gui::install(plugin)` once, then `gui::Menu::new(rows, title).button(slot, item, |c| ..).open(&p)`; lists: `gui::Pages`; `gui::slots("9-17")`',
          '- items from config: `item::Spec::from_yaml(&node).build(&fill)`; saved items: `item::to_base64` / `from_base64`; heads: `item::head`',
          '- player data: `store::Docs<T>` (one JSON per uuid, load at pre-login, batched off-thread saves); one file: `store::File<T>`',
          '- money / ranks / placeholders: `hooks::economy`, `hooks::chat`, `hooks::placeholders(text, Some(&p))`, offer yours: `hooks::provide_placeholders`',
          '- text: `mini::parse_mixed` (MiniMessage + & codes) -> component; `text::colorize` -> legacy; HOCON: `hocon::read`',
          '- durations `time::parse("10m")`, `sync::{on_main, drain, hold, Cooldowns}`, sounds `sound::play(&p, "ENTITY_X:1:1")`',
          '- client-only entities, scores, boss bars: `packets`, `wire` (`wire::set_score`, `wire::boss`); per-tick work: `worker`',
          '- a `/x profiler`: `stats::{set_running, report, timed}`; anything else: the SDK map above, then `api_lookup <Class.method>`', '']
    (out / 'BRIEF.md').write_text('\n'.join(B), encoding='utf-8')

    # tests/check.py
    first = next(iter(commands), id_)
    T = ['"""Live check of the Rust ' + name + ' on the test server (hwe_live.py up): each command and sub answers.', '', '    python tests/check.py', '"""',
         'import pathlib, sys, time', "sys.path.insert(0, r'D:\\Horizon\\plugin-sweep\\audit')", "sys.path.insert(0, r'D:\\Horizon\\fabric-port\\tools')",
         'import proof  # noqa: E402', "proof.RUN = pathlib.Path(r'D:\\Horizon\\parity\\hwelive')", 'proof.PORT = 25781', 'import hwe_verify as v  # noqa: E402', '',
         'def has(lines, word):', '    return any(word.lower() in l.lower() for l in lines)', '',
         'v.stand(0, 125, 0)', '# Fill each expectation with what the Java plugin says (its messages file).',
         f"lines = v.say('/{first} help')", f"v.check('/{first} help', has(lines, '{first}'), lines)"]
    for sub in ['reload'] + [s for s in sub_names if s != 'reload']:
        T.append(f"lines = v.say('/{first} {sub}')")
        T.append(f"v.check('/{first} {sub}', bool(lines) and not has(lines, 'not written yet') and not has(lines, 'Unknown'), lines)")
    for cmd in list(commands)[1:]:
        T.append(f"lines = v.say('/{cmd}')")
        T.append(f"v.check('/{cmd}', bool(lines) and not has(lines, 'not written yet') and not has(lines, 'Unknown'), lines)")
    T.append("print(f'{v.PASSED}/{v.PASSED + v.FAILED} checks passed' if hasattr(v, 'PASSED') else 'done')")
    (out / 'tests' / 'check.py').write_text('\n'.join(T) + '\n', encoding='utf-8')
    (out / '.gitignore').write_text('target/\njava/\n', encoding='utf-8')
    print(json.dumps({'crate': str(out), 'id': id_, 'commands': list(commands), 'subs': len(subs), 'events': len(events), 'timers': len(timers), 'data': data_files, 'classes': len(files)}, indent=1))


if __name__ == '__main__':
    main()
