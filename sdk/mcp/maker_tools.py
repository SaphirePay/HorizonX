#!/usr/bin/env python3
"""The tools for people making plugins and mods, beside the porting line in `plugin_mcp.py`.

A plugin here is a Rust DLL on horizon-bukkit, Bukkit's names in snake_case. A mod is a Rust DLL on
horizon-mod, the runtime's own events. Neither needs an engine build. An engine hook does, and the last
group here is for that.

Every jar or URL handed in is read as data: nothing here runs it. `plugin_try` runs a plugin's DLL on
the test server, which is native code with the whole PC in reach, so only try what you wrote or read.
"""
import workspace  # noqa: E402
import difflib
import json
try:
    import msvcrt  # Windows file locks
except ImportError:  # Linux and macOS
    msvcrt = None
    import fcntl
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time
import urllib.parse
import urllib.request
import zipfile

HORIZON = workspace.WORKSPACE
TOOLS = HORIZON / 'plugin-tools'
FABRIC = HORIZON / 'fabric-port'
# The SDK converted plugins are compiled against (jport.py's SDK): bindings come from its functions. The Fabric-era
# copy under mod-sdk drifted after the Paper migration, so additions to this one were never bound.
_REPO_SDK = pathlib.Path(__file__).resolve().parent.parent / 'horizon-bukkit'
SDK = _REPO_SDK if _REPO_SDK.exists() else HORIZON / 'paper-engine' / 'sdk' / 'horizon-bukkit'
SDK_MOD = FABRIC / 'mod-sdk' / 'horizon-mod'
PORTS = HORIZON / 'plugin-ports'
MODS = HORIZON / 'mod-ports'
IMPORTS = HORIZON / 'plugin-imports'
TARGET = PORTS / 'target'
CHECK_TARGET = PORTS / 'target-check'
SCOUT = TOOLS / 'target' / 'release' / 'plugin-scout.exe'
JAVA = workspace.JAVA
LOCK = TOOLS / '.check.lock'
GUIDE = TOOLS / 'GUIDE.md'
JOB = TOOLS / '.job.log'
COMMIT = 'cbfae59314dfa74d54cb8a379cae609c6f5ec91d'
HOOKS_FILE = str(workspace.at('main', 'native-hooks.json'))
CARGO_ENV = {**os.environ, 'CARGO_TERM_COLOR': 'never'}
sys.path[:0] = [str(TOOLS / 'tools'), str(FABRIC / 'tools')]


def identifier(name):
    text = re.sub(r'[^a-z0-9]+', '_', name.lower()).strip('_')
    return text if text and not text[0].isdigit() else f'p_{text}'


def words(value):
    """A list from a list, or from a string of names."""
    if not value:
        return []
    return list(value) if isinstance(value, list) else [word for word in re.split(r'[,\s]+', value) if word]


def folder_of(id):
    """A plugin's or a mod's crate."""
    for root in (PORTS, MODS):
        if (root / id / 'Cargo.toml').is_file():
            return root / id
    return None


def dll_of(id):
    return (TARGET if (PORTS / id).is_dir() else MODS / 'target') / 'release' / f'{id}.dll'


# ---- The SDK as Rust: what the Java-keyed index can't say

ITEM = re.compile(r'^\s*pub\s+(?:const\s+|unsafe\s+|async\s+)*(fn|struct|enum|type|trait)\s+([A-Za-z_]\w*)')
# `impl crate::PlayerInventory` / `impl blocks::Sign`: the type is the last name of the path, not `crate`.
IMPL = re.compile(r'^impl(?:<[^>]*>)?\s+(?:[\w:<>,\' ]+\s+for\s+)?(?:\w+::)*([A-Za-z_]\w*)')
TRAIT = re.compile(r'^pub trait\s+([A-Za-z_]\w*)')
TRAIT_FN = re.compile(r'^\s+(fn)\s+([a-z_]\w*)')
TAG = re.compile(r'^\s*///\s*bukkit(~?):\s*([^|]+?)(?:\s*\|\s*(.*))?$')
_sdk = (0.0, [])


def macro_rows(path, lines):
    """Functions a macro makes from a table: a `/// bukkit:` line over a row, with no `pub fn` to see."""
    out = []
    types = {'int': 'i32', 'flag': 'bool', 'text': 'String'}
    for number, line in enumerate(lines):
        row = line.strip()
        if not lines[number - 1].lstrip().startswith('///') or not TAG.match(lines[number - 1]):
            continue
        tag = TAG.match(lines[number - 1])
        java = [name.strip() for name in tag.group(2).split(',')]
        common = {'kind': 'fn', 'place': f'{path.name}:{number + 1}', 'java': java, 'partial': tag.group(1) == '~', 'note': tag.group(3) or ''}
        if (found := re.match(r'([a-z_]\w*),\s*(int|flag|text)\b', row)):
            out.append({'rust': found.group(1), 'signature': f'pub fn {found.group(1)}() -> {types[found.group(2)]}', **common})
        elif path.name == 'config2.rs' and (found := re.match(r'(get_\w+_list),\s*(\w+),', row)):
            out.append({'rust': f'YamlConfiguration::{found.group(1)}', 'signature': f'pub fn {found.group(1)}(&self, path: &str) -> Vec<{found.group(2)}>', **common})
        elif (found := re.match(r'(\w+Event),\s*\w+,\s*(\w+),\s*(\w+),', row)):
            out.append({'rust': f'{found.group(1)}::{found.group(3)}', 'signature': f'pub fn {found.group(3)}(&self) -> {found.group(2)}', **common})
    return out


def sdk_items():
    """Every public item of horizon-bukkit: owner, name, signature, place, Java names, and any note."""
    global _sdk
    files = sorted((SDK / 'src').glob('*.rs'))
    stamp = max(path.stat().st_mtime for path in files)
    if stamp == _sdk[0]:
        return _sdk[1]
    items = []
    for path in files:
        lines = path.read_text(encoding='utf-8', errors='replace').split('\n')
        owner, in_trait = '', False
        for number, line in enumerate(lines):
            if not line.startswith(' ') and (found := IMPL.match(line)):
                owner = found.group(1)
            elif line.startswith(('}', 'pub ')) and not line.startswith('pub fn'):
                owner = ''
            if line.startswith('}'):
                in_trait = False
            elif (trait := TRAIT.match(line)):
                owner, in_trait = trait.group(1), True
            found = ITEM.match(line)
            if not found and in_trait:
                found = TRAIT_FN.match(line)
            if not found:
                continue
            kind, name = found.groups()
            head = line.strip()
            for more in lines[number + 1:number + 8]:
                if '{' in head or head.endswith((';', ')')):
                    break
                head += ' ' + more.strip()
            java, partial, notes = [], False, []
            above = number - 1
            while above >= 0 and lines[above].lstrip().startswith(('///', '#[')):
                if tag := TAG.match(lines[above]):
                    java += [name_.strip() for name_ in tag.group(2).split(',')]
                    partial = partial or tag.group(1) == '~'
                    if tag.group(3):
                        notes.append(tag.group(3))
                above -= 1
            items.append({'rust': f'{owner}::{name}' if owner and kind == 'fn' else name, 'kind': kind, 'signature': head.split('{')[0].strip().rstrip(' ;'), 'place': f'{path.name}:{number + 1}', 'java': java, 'partial': partial, 'note': ' '.join(notes)})
        items += macro_rows(path, lines)
    _sdk = (stamp, items)
    return items


def sdk_search(query, limit=25):
    """Find Rust in horizon-bukkit by words: `player teleport`, `getHealth`, `set_flying`."""
    wanted = [word.lower().replace('_', '') for word in query.split()]
    hits = []
    for item in sdk_items():
        text = ' '.join([item['rust'], item['signature'], *item['java']]).lower().replace('_', '')
        if all(word in text for word in wanted):
            rank = 0 if wanted[-1] == item['rust'].rsplit('::', 1)[-1].lower().replace('_', '') else 1
            hits.append((rank, len(item['rust']), item))
    hits.sort(key=lambda hit: hit[:2])
    return {'matches': len(hits), 'found': [{key: value for key, value in hit[2].items() if value} for hit in hits[:limit]]}


def java_hints(file):
    """A Java file line by line against the SDK: each API call it makes, and the Rust for it."""
    path = pathlib.Path(file)
    if not path.is_file():
        return {'error': f'no file {file}'}
    text = path.read_text(encoding='utf-8', errors='replace')
    imports = {line.rsplit('.', 1)[-1] for line in re.findall(r'^import\s+(?:static\s+)?([\w.]+);', text, re.M)}
    by_method = {}
    for item in sdk_items():
        for java in item['java']:
            if '.' in java:
                owner, method = java.rsplit('/', 1)[-1].rsplit('.', 1)
                by_method.setdefault(method, []).append((owner.split('$')[-1], item))
    found = {}
    lines = text.split('\n')
    for number, line in enumerate(lines, 1):
        for method in re.findall(r'\.(\w+)\(', line):
            if method in by_method:
                found.setdefault(method, []).append(number)
    calls = []
    for method, where in sorted(found.items(), key=lambda row: row[1][0]):
        rows = by_method[method]
        rows = sorted(rows, key=lambda row: row[0] not in imports)
        seen, rust = set(), []
        for owner, item in rows:
            if item['rust'] not in seen:
                seen.add(item['rust'])
                rust.append({'rust': item['rust'], 'java_owner': owner, **({'note': item['note']} if item['note'] else {})})
        calls.append({'java': method, 'lines': where[:8], 'rust': rust[:3]})
    known = set(found)
    unknown = sorted({method for line in lines for method in re.findall(r'\.(\w+)\(', line)} - known)
    events = re.findall(r'@EventHandler[^\n]*\n\s*public\s+void\s+(\w+)\s*\(\s*(?:final\s+)?(\w+)', text)
    return {'file': str(path), 'lines': len(lines), 'listeners': [{'method': method, 'event': event, 'rust': f'plugin.register_event(|event: &mut {event}| ...)'} for method, event in events], 'calls': calls, 'not_api_or_not_in_the_sdk': unknown[:80]}


def event_info(query='', limit=30):
    """A Paper event: how to listen, what it can read and change, and what's still not exact. Empty: the whole picture."""
    mapping = json.loads((TOOLS / 'mapping-sdk.json').read_text(encoding='utf-8'))['events']
    api = json.loads((HORIZON / 'paper-api' / 'api.json').read_text(encoding='utf-8'))
    probe = TOOLS / 'probe-last.json'
    fired = json.loads(probe.read_text(encoding='utf-8')) if probe.is_file() else {}
    classes = sorted({key.split('.')[0] for key in api if '/event/' in key and key.endswith('.getHandlers')})
    hits = [name for name in classes if query.lower() in name.rsplit('/', 1)[-1].lower()]
    if not query:
        return {'events': len(classes), 'listenable': sum(name in mapping for name in classes), 'not_yet': [name.rsplit('/', 1)[-1] for name in classes if name not in mapping][:120], 'seen_firing_in_the_last_probe': sorted(fired)}
    if len(hits) != 1 and (exact := [name for name in hits if name.rsplit('/', 1)[-1].lower() == query.lower()]):
        hits = exact
    if len(hits) != 1:
        return {'matches': len(hits), 'events': [{'event': name.rsplit('/', 1)[-1], 'listenable': name in mapping} for name in hits[:limit]]}
    name = hits[0]
    short = name.rsplit('/', 1)[-1]
    methods = {key.split('.')[-1]: entry for key, entry in api.items() if key.split('.')[0] == name}
    state = lambda entry: 'exact' if entry['exact'] else 'partial' if entry['partial'] else 'missing'
    hook = None
    for path in (SDK / 'src').glob('*.rs'):
        text = path.read_text(encoding='utf-8', errors='replace')
        if found := re.search(rf'impl Event for {short} \{{.*?const HOOK: &\'static str = "(\w+)"', text, re.S):
            hook = found.group(1)
            break
    return {
        'event': name, 'listen': mapping.get(name, {}).get('listener') or 'NOT LISTENABLE YET', 'hook': hook,
        'note': mapping.get(name, {}).get('note', ''), 'seen_firing_in_the_last_probe': short in fired,
        'methods': {method: {'state': state(entry), 'rust': entry.get('rust', '')} for method, entry in sorted(methods.items()) if method not in ('getHandlers', 'getHandlerList')},
    }


# ---- Any plugin, not only the sampled 200

def fetch(source, folder):
    """A jar from a path or a URL into `folder`, which holds nothing else."""
    if not re.match(r'https?://', source):
        path = pathlib.Path(source)
        if not path.is_file():
            raise FileNotFoundError(source)
        return pathlib.Path(shutil.copy(path, folder))
    request = urllib.request.Request(source, headers={'User-Agent': 'Mozilla/5.0'})
    with urllib.request.urlopen(request, timeout=120) as answer:
        name = pathlib.PurePosixPath(urllib.parse.unquote(urllib.parse.urlparse(answer.url).path)).name
        out = folder / (name if name.endswith('.jar') else 'plugin.jar')
        out.write_bytes(answer.read())
    if out.read_bytes()[:2] != b'PK':
        out.unlink()
        raise ValueError('that URL gave a web page, not a jar: give the direct download link')
    return out


def import_rows():
    rows = []
    for path in IMPORTS.glob('*/report/plugins.jsonl') if IMPORTS.is_dir() else []:
        for line in path.read_text(encoding='utf-8').splitlines():
            if line.strip():
                row = json.loads(line)
                row['file'] = str(path.parent.parent / 'jar' / row['file'])
                row['imported'] = True
                rows.append(row)
    return rows


def plugin_import(source):
    """Bring in any Bukkit, Spigot or Paper jar, by path or URL: decompile it and say what a port takes."""
    IMPORTS.mkdir(exist_ok=True)
    stage = IMPORTS / f'_new_{os.getpid()}'
    shutil.rmtree(stage, ignore_errors=True)
    (stage / 'jar').mkdir(parents=True)
    jar = fetch(source, stage / 'jar')
    scan = subprocess.run([str(SCOUT), 'scan', str(stage / 'jar'), '--out', str(stage / 'report'), '--mapping', str(TOOLS / 'mapping-sdk.json')], capture_output=True, text=True, timeout=300)
    rows = stage / 'report' / 'plugins.jsonl'
    if scan.returncode or not rows.is_file() or not rows.read_text(encoding='utf-8').strip():
        shutil.rmtree(stage, ignore_errors=True)
        return {'error': 'the scanner could not read it', 'detail': scan.stderr[-400:]}
    row = json.loads(rows.read_text(encoding='utf-8').splitlines()[0])
    id_ = identifier(row.get('name') or jar.stem)
    home = IMPORTS / id_
    shutil.rmtree(home, ignore_errors=True)
    stage.rename(home)
    java = PORTS / '_java' / id_
    if not java.is_dir():
        java.mkdir(parents=True)
        subprocess.run([JAVA, '-jar', str(TOOLS / 'tools' / 'vineflower.jar'), '--silent', str(home / 'jar' / jar.name), str(java)], capture_output=True, timeout=900)
        # Obfuscated constants (AES strings, scrambled numbers) back to plain Java, as the converter does.
        try:
            sys.path.insert(0, str(TOOLS / 'jport'))
            import deobfuscate
            deobfuscate.decode_tree(java)
        except Exception:
            pass
    files = list(java.rglob('*.java'))
    lines = sum(len(path.read_text(encoding='utf-8', errors='replace').split('\n')) for path in files)
    with zipfile.ZipFile(home / 'jar' / jar.name) as archive:
        licenses = [name for name in archive.namelist() if re.search(r'licen[sc]e|copying', name, re.I) and not name.endswith('/')][:6]
    api = json.loads((HORIZON / 'paper-api' / 'api.json').read_text(encoding='utf-8'))
    used = sorted({call for handler in row.get('handlers', []) for call in handler.get('calls', [])} | set(row.get('unmapped_calls', [])))
    state = {'exact': 0, 'partial': 0, 'missing': 0, 'not_paper': 0}
    missing = []
    for call in used:
        entry = api.get(call)
        if entry is None:
            state['not_paper'] += 1
        elif entry['exact']:
            state['exact'] += 1
        elif entry['partial']:
            state['partial'] += 1
        else:
            state['missing'] += 1
            missing.append(call)
    return {
        'id': id_, 'name': row.get('name'), 'version': row.get('version'), 'tier': row.get('tier'), 'why': row.get('why', [])[:6],
        'license_guess': row.get('license'), 'license_files_in_the_jar': licenses, 'license_warning': "The scanner reads whatever license text it meets first, bundled libraries included: confirm it from the project's own repository.",
        'depend': row.get('depend'), 'softdepend': row.get('softdepend'), 'commands': len(row.get('commands') or []), 'listeners': len(row.get('handlers') or []), 'classes': row.get('classes'),
        'java_source': str(java), 'java_files': len(files), 'java_lines': lines, 'calls_seen_in_listeners': state, 'missing': missing[:40],
        'next': 'plugin_brief and plugin_scaffold take this name now',
    }


CARGO_PLUGIN = '''[package]
name = "{id}"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
horizon-bukkit = {{ path = "{sdk}" }}
serde_json = "1"

[profile.release]
panic = "unwind"

[workspace]
'''

CARGO_MOD = '''[package]
name = "{id}"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
horizon-mod = {{ path = "{sdk}" }}

[profile.release]
panic = "unwind"

[workspace]
'''


def plugin_new(name, commands='', description=''):
    """Start a new plugin from nothing: a crate that builds and loads as it stands."""
    id_ = identifier(name)
    out = PORTS / id_
    if out.exists():
        return {'error': f'{out} is there already'}
    (out / 'src').mkdir(parents=True)
    names = words(commands)
    (out / 'Cargo.toml').write_text(CARGO_PLUGIN.format(id=id_, sdk=SDK.as_posix()), encoding='utf-8')
    (out / 'fabric.mod.json').write_text(json.dumps({'schemaVersion': 1, 'id': id_, 'version': '0.1.0', 'name': name, 'description': description, 'depends': {'fabric_api': '>=0.1.0'}}, indent=2), encoding='utf-8')
    yaml = [f'name: {name}', f'main: horizon.{id_}', 'version: 0.1.0', "api-version: '1.21'"]
    if names:
        yaml.append('commands:')
        for command in names:
            yaml += [f'  {command}:', f'    description: /{command}', f'    usage: /{command}']
    (out / 'plugin.yml').write_text('\n'.join(yaml) + '\n', encoding='utf-8')
    code = [f'//! {name}.', 'use horizon_bukkit::event::PlayerJoinEvent;', 'use horizon_bukkit::{CommandSender, Plugin, bukkit_plugin};', '']
    for command in names:
        code += [f'fn {identifier(command)}(sender: &CommandSender, _label: &str, _args: &[&str]) -> bool {{', f'    sender.send_message("/{command} works");', '    true', '}', '']
    code += ['fn on_enable(plugin: Plugin) {']
    code += [f'    plugin.set_executor("{command}", {identifier(command)});' for command in names]
    code += ['    plugin.register_event(|event: &mut PlayerJoinEvent| {', f'        event.get_player().send_message("{name} is running");', '    });', '}', '', 'fn on_disable(_plugin: Plugin) {}', '', 'bukkit_plugin!(on_enable, on_disable);', '']
    (out / 'src' / 'lib.rs').write_text('\n'.join(code), encoding='utf-8')
    return {'id': id_, 'crate': str(out), 'next': f'plugin_check {id_} while writing, then plugin_try {id_} with the commands to run'}


def mod_new(name, commands='', events='join', description=''):
    """Start a new mod from nothing, on horizon-mod's events: a crate that builds and loads as it stands."""
    id_ = identifier(name)
    out = MODS / id_
    if out.exists():
        return {'error': f'{out} is there already'}
    (out / 'src').mkdir(parents=True)
    (out / 'Cargo.toml').write_text(CARGO_MOD.format(id=id_, sdk=SDK_MOD.as_posix()), encoding='utf-8')
    (out / 'fabric.mod.json').write_text(json.dumps({'schemaVersion': 1, 'id': id_, 'version': '0.1.0', 'name': name, 'description': description, 'depends': {'fabric_api': '>=0.1.0'}}, indent=2), encoding='utf-8')
    names, hears = words(commands), words(events)
    code = [f'//! {name}. Build it, drop the DLL in `mods/`, and `/fabricapi reload` plays the change.', 'use horizon_mod::{Answer, Host, Value, feedback, log};', '', 'fn start(host: Host) {', f'    log("{id_} started");']
    code += [f'    host.command("{command}", 0);' for command in names]
    if names:
        code += ['    host.on("command", |event: &Value| {', '        let line = event["line"].as_str().unwrap_or_default();', '        match line.split_whitespace().next().unwrap_or_default() {']
        code += [f'            "{command}" => feedback("/{command} works"),' for command in names]
        code += ['            _ => {}', '        }', '        Answer::Pass', '    });']
    for event in hears:
        code += [f'    host.on("{event}", |event: &Value| {{', f'        log(&format!("{event}: {{event}}"));', '        Answer::Pass', '    });']
    code += ['}', '', 'horizon_mod::mod_entry!(start, include_str!("../fabric.mod.json"));', '']
    (out / 'src' / 'lib.rs').write_text('\n'.join(code), encoding='utf-8')
    return {'id': id_, 'crate': str(out), 'next': f'mod_events for what to listen to, plugin_check {id_}, then plugin_try {id_}'}


def plugin_check(id):
    """Compile a plugin or mod without building it, with a fix suggested for each method the SDK doesn't have."""
    folder = folder_of(id)
    if not folder:
        return {'error': f'no crate {id} in {PORTS} or {MODS}'}
    begin = time.time()
    result = subprocess.run(['cargo', '+1.99.1', 'check', '--message-format=short'], cwd=folder, capture_output=True, text=True, encoding='utf-8', errors='replace', env={**CARGO_ENV, 'CARGO_TARGET_DIR': str(CHECK_TARGET)}, timeout=590)
    errors = [line for line in result.stderr.splitlines() if ': error' in line or line.startswith('error:')]
    warnings = [line for line in result.stderr.splitlines() if ': warning' in line and str(folder.name) in line.replace('\\', '/') or (': warning' in line and line.startswith('src'))]
    names = sorted({item['rust'].rsplit('::', 1)[-1] for item in sdk_items()})
    fixes = {}
    for line in errors:
        if found := re.search(r"no (?:method|function|associated item) named `(\w+)`", line):
            fixes[found.group(1)] = difflib.get_close_matches(found.group(1), names, 4, 0.6)
    return {'ok': result.returncode == 0, 'seconds': round(time.time() - begin, 1), 'errors': errors[:40], 'more_errors': max(0, len(errors) - 40), 'close_names_in_the_sdk': fixes, 'warnings': warnings[:15]}


def plugin_audit(id):
    """What keeps a plugin or mod from being finished: stubs, commands with no code, SDK calls that aren't exact."""
    folder = folder_of(id)
    if not folder:
        return {'error': f'no crate {id} in {PORTS} or {MODS}'}
    stubs, used = [], set()
    source = ''
    for path in (folder / 'src').rglob('*.rs'):
        text = path.read_text(encoding='utf-8', errors='replace')
        source += text
        for number, line in enumerate(text.split('\n'), 1):
            if re.search(r'\b(todo|unimplemented)!\(|\bTODO\b|\bFIXME\b', line):
                stubs.append(f'{path.relative_to(folder)}:{number}: {line.strip()[:120]}')
        used |= set(re.findall(r'[.:]([a-z_][a-z0-9_]*)\(', text))
    partial = {}
    for item in sdk_items():
        name = item['rust'].rsplit('::', 1)[-1]
        owner = item['rust'].split('::')[0]
        # By name alone every `get_player` would count: the type must be in the code too.
        if item['partial'] and name in used and item['kind'] == 'fn' and '::' in item['rust'] and re.search(rf'\b{owner}\b', source):
            partial[item['rust']] = item['note']
    missing = []
    yml = folder / 'plugin.yml'
    if yml.is_file() and not (folder / 'build.rs').is_file():
        commands = re.findall(r'^  ([\w-]+):\s*$', yml.read_text(encoding='utf-8', errors='replace').split('commands:', 1)[-1].split('\npermissions:', 1)[0], re.M)
        missing = [command for command in commands if f'"{command}"' not in source]
    return {'stubs': stubs[:60], 'stub_count': len(stubs), 'commands_with_no_executor': missing, 'sdk_calls_not_exact': dict(sorted(partial.items())[:60]), 'sdk_calls_not_exact_count': len(partial), 'lines_of_rust': source.count('\n')}


# ---- The test server, in one call

class Lock:
    """The compile lock every game run takes: two runs at once share a control socket."""

    def __enter__(self):
        self.file = LOCK.open('a+b')
        deadline = time.time() + 300
        while True:
            try:
                self.file.seek(0)
                if msvcrt:
                    msvcrt.locking(self.file.fileno(), msvcrt.LK_NBLCK, 1)
                else:
                    fcntl.flock(self.file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                return self
            except OSError:
                if time.time() > deadline:
                    self.file.close()
                    raise TimeoutError('another check holds the lock')
                time.sleep(2)

    def __exit__(self, *_):
        self.file.seek(0)
        if msvcrt:
            msvcrt.locking(self.file.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            fcntl.flock(self.file.fileno(), fcntl.LOCK_UN)
        self.file.close()


def rig():
    import paper_server
    import proof
    return paper_server, proof


def log_of(paper_server):
    return paper_server.ROOT / 'server' / 'logs' / 'latest.log'


def read(path):
    return path.read_text(encoding='utf-8', errors='replace') if path.is_file() else ''


def players_on(paper_server):
    """Who the log says is on, leaving out the test client."""
    on = set()
    for line in read(log_of(paper_server)).splitlines():
        if found := re.search(r'\]: (\S+?)\[/[^\]]*\] logged in', line):
            on.add(found.group(1))
        elif found := re.search(r'\]: (\S+) lost connection', line):
            on.discard(found.group(1))
    return sorted(on - {paper_server.NAME})


def restart(paper_server, proof):
    paper_server.down()
    time.sleep(2)
    proof.server('start')
    time.sleep(3)
    log = log_of(paper_server)
    return proof.wait(lambda: 'server started' in read(log), 120, 0.25)


def problems(paper_server):
    return [line[:220] for line in read(log_of(paper_server)).splitlines() if re.search(r'panic|listener panicked|ERROR|failed to load|could not load|not JSON|unknown action|no such query', line)][-30:]


def server_status():
    """The test server: is it up, who is on, which plugins and mods it loads, and any trouble in its log."""
    paper_server, _ = rig()
    mods = sorted(path.name for path in (paper_server.ROOT / 'server' / 'mods').glob('*.dll'))
    return {'folder': str(paper_server.ROOT / 'server'), 'port': paper_server.PORT, 'status': paper_server.proof.server('status')[:200], 'players': players_on(paper_server), 'mods': mods, 'problems': problems(paper_server)}


def server_log(filter='', lines=60):
    """The test server's log, last lines first filtered by a regular expression."""
    paper_server, _ = rig()
    rows = read(log_of(paper_server)).splitlines()
    if filter:
        needle = re.compile(filter, re.I)
        rows = [row for row in rows if needle.search(row)]
    return [row[:240] for row in rows[-lines:]]


def plugin_deploy(id, force=False):
    """Put a plugin's or mod's newest DLL on the test server and restart it: refuses while someone is playing."""
    dll = dll_of(id)
    if not dll.is_file():
        return {'error': f'no {dll}: run plugin_build first'}
    paper_server, proof = rig()
    if (on := players_on(paper_server)) and not force:
        return {'error': f'{", ".join(on)} on the server: pass force to restart anyway'}
    with Lock():
        paper_server.down()
        time.sleep(2)
        shutil.copy(dll, paper_server.ROOT / 'server' / 'mods')
        started = restart(paper_server, proof)
    return {'started': started, 'mods': sorted(path.name for path in (paper_server.ROOT / 'server' / 'mods').glob('*.dll')), 'problems': problems(paper_server), 'loaded': [row[:160] for row in read(log_of(paper_server)).splitlines() if f'[{id}' in row.lower() or id in row.lower()][-8:]}


def plugin_remove(id, force=False):
    """Take a plugin's or mod's DLL off the test server and restart it. The crate stays."""
    paper_server, proof = rig()
    dll = paper_server.ROOT / 'server' / 'mods' / f'{id}.dll'
    if not dll.is_file():
        return {'error': f'{id} is not on the test server'}
    if (on := players_on(paper_server)) and not force:
        return {'error': f'{", ".join(on)} on the server: pass force to restart anyway'}
    with Lock():
        paper_server.down()
        time.sleep(2)
        dll.unlink()
        started = restart(paper_server, proof)
    return {'started': started, 'mods': sorted(path.name for path in dll.parent.glob('*.dll'))}


def plugin_try(id='', commands='', seconds=1.0, screenshot=True, force=False, new_engine=False, survival=False):
    """Build-to-chat in one call: put the DLL on the test server, join with a test client, run the commands, return the chat, the server's trouble and a screenshot."""
    paper_server, proof = rig()
    if (on := players_on(paper_server)) and not force:
        return {'error': f'{", ".join(on)} on the server: pass force to restart anyway'}
    lines = commands if isinstance(commands, list) else [line.strip() for line in re.split(r'[\n;]+', commands) if line.strip()]
    begin = time.time()
    with Lock():
        paper_server.down()
        time.sleep(2)
        if new_engine:
            shutil.copy2(paper_server.PARITY, paper_server.ENGINE)
        if id:
            dll = dll_of(id)
            if not dll.is_file():
                return {'error': f'no {dll}: run plugin_build first'}
            shutil.copy(dll, paper_server.ROOT / 'server' / 'mods')
        if not restart(paper_server, proof):
            return {'error': 'the server did not start', 'log': read(log_of(paper_server))[-1200:]}
        client = paper_server.ROOT / 'client'
        proof.captured([str(part) for part in (proof.CTL, '--timeout', 60, 'launch', '--game', paper_server.ENGINE, '--offscreen', '--width', 1280, '--height', 720, '--game-dir', client, '--resources', proof.RESOURCES, '--name', paper_server.NAME)], 75, paper_server.environment())
        paper_server.ctl('connect', f'localhost:{paper_server.PORT}', paper_server.NAME, timeout=60)
        paper_server.ctl('wait-ready', timeout=90)
        paper_server.ctl('respawn', timeout=5)
        # A test client that quit dead comes back dead, and mobs reach it at night.
        if not survival:
            paper_server.ctl('chat', '/gamemode creative')
        time.sleep(1.5)
        for line in lines:
            paper_server.ctl('chat', line)
            time.sleep(seconds)
        time.sleep(0.5)
        picture = ''
        if screenshot:
            picture = str(paper_server.ROOT / 'last-try.png')
            paper_server.ctl('screenshot', picture, timeout=15)
        paper_server.ctl('quit', timeout=5)
        chat = [row.split('[CHAT] ', 1)[1].rstrip() for row in read(client / 'logs' / 'latest.log').splitlines() if '[CHAT] ' in row]
    return {'seconds': round(time.time() - begin, 1), 'chat': chat[-80:], 'problems': problems(paper_server), 'mod_log': [row[:200] for row in read(log_of(paper_server)).splitlines() if '[mod]' in row or re.search(r'INFO\]: \[[\w .-]+\] ', row)][-30:], 'screenshot': picture}


# ---- Mods and the engine's seams

def mod_events(query='', limit=60):
    """What a mod or plugin can hear with `host.on`: each event's name, its payload, and whether a refusal works."""
    bridge = (FABRIC / 'mixins' / 'fabric_api' / 'fabric_bridge.rs').read_text(encoding='utf-8', errors='replace')
    rows = {}
    for name, payload in re.findall(r'fire_in\(game, "(\w+)", json!\(\{\s*(.*?)\}\)\)', bridge):
        rows.setdefault(name, {'event': name, 'payload': re.findall(r'"(\w+)":', payload), 'refusable': False, 'from': 'fabric_bridge.rs'})
    for name in re.findall(r'fire_in\(game, "(\w+)", Value::Null', bridge):
        rows.setdefault(name, {'event': name, 'payload': [], 'refusable': False, 'from': 'fabric_bridge.rs'})
    for name in re.findall(r'fire_in\(game, "(\w+)".*== REFUSED', bridge):
        rows.setdefault(name, {'event': name, 'payload': [], 'refusable': False, 'from': 'fabric_bridge.rs'})['refusable'] = True
    import event_hooks
    for raw in event_hooks.raws():
        name = raw['id'].removeprefix('ev_')
        rows.setdefault(name, {'event': name, 'payload': re.findall(r'"(\w+)" ?:', raw['code']), 'refusable': bool(raw.get('cancellable')), 'from': raw['target'].replace('horizon_client::game::', '').replace("Game<'_>::", '')})
    found = [row for name, row in sorted(rows.items()) if query.lower() in name.lower() or query.lower() in row['from'].lower()]
    return {'matches': len(found), 'events': found[:limit]}


def fabric_status():
    """Fabric API's events by their tracked status, from COVERAGE.md."""
    rows = {}
    for line in read(FABRIC / 'COVERAGE.md').splitlines():
        cells = [cell.strip() for cell in line.strip('|').split('|')]
        if len(cells) == 3 and '.' in cells[1]:
            rows[cells[1]] = (cells[0], cells[2])
    return rows


def fabric_event(query, limit=25):
    """A Fabric API event: is it ported, in which module, and where its Java is."""
    hits = [(name, row) for name, row in fabric_status().items() if query.lower() in name.lower()]
    out = {'matches': len(hits), 'events': [{'event': name, 'module': row[0], 'status': row[1]} for name, row in hits[:limit]]}
    if 0 < len(hits) <= 3:
        source = HORIZON / 'fabric-api-src'
        if source.is_dir():
            word = query.split('.')[-1]
            run = subprocess.run([sys.executable, '-I', str(FABRIC / 'tools' / 'event_sites.py'), str(source), str(FABRIC / 'EVENT_SITES.md'), word], capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=60, cwd=FABRIC)
            out['java_sites'] = run.stdout[-2500:]
    return out


def mod_scan(source):
    """What a Fabric mod jar needs, by path or URL: Fabric API events ported or not, and its Mixin count, which has no Rust counterpart."""
    from fabric_mod_scan import scan
    IMPORTS.mkdir(exist_ok=True)
    stage = IMPORTS / f'_mod_{os.getpid()}'
    shutil.rmtree(stage, ignore_errors=True)
    stage.mkdir()
    try:
        jar = fetch(source, stage)
        seen = {'classes': 0, 'mixins': 0, 'minecraft': 0, 'refs': set()}
        with zipfile.ZipFile(jar) as archive:
            scan(archive, seen)
            names = archive.namelist()
            info = json.loads(archive.read('fabric.mod.json')) if 'fabric.mod.json' in names else {}
    finally:
        shutil.rmtree(stage, ignore_errors=True)
    events = json.loads(read(FABRIC / 'events.json'))
    holders = {event['holder'].split('.')[0] for event in events}
    status = fabric_status()
    used = []
    for holder in sorted(seen['refs'] & holders):
        states = [row[1].split(':')[0] for name, row in status.items() if name.split('.')[0] == holder]
        used.append({'holder': holder, 'ported': states.count('Ported'), 'partly': states.count('Partly'), 'no': states.count('No')})
    other = sorted(seen['refs'] - holders)
    return {
        'id': info.get('id'), 'name': info.get('name'), 'version': info.get('version'), 'license': info.get('license'), 'depends': info.get('depends'),
        'classes': seen['classes'], 'mixins': seen['mixins'], 'minecraft_references': seen['minecraft'], 'fabric_events': used, 'other_fabric_api': other[:40],
        'verdict': 'no Mixin: Fabric API and Horizon hooks may be enough' if not seen['mixins'] else f"{seen['mixins']} Mixin classes: each needs an engine hook of its own",
    }


def hook_search(query, limit=30):
    """The engine hooks that exist already: their ids, targets and what they pass."""
    import event_hooks
    found = []
    for raw in event_hooks.raws():
        if query.lower() in f"{raw['id']} {raw['target']}".lower():
            found.append({'id': raw['id'], 'target': raw['target'], 'kind': raw.get('kind'), 'cancellable': bool(raw.get('cancellable')), 'params': [row[0] for row in raw.get('params', [])]})
    return {'matches': len(found), 'hooks': found[:limit], 'where': 'tools/event_hooks.py EXTRA and ROWS, plugin-tools/tools/family/*.py, tools/gen_bundle.py'}


def hook_seam(function, file=''):
    """Which injection kinds an engine function takes, before writing a hook: `tick_blocks`, `game/remote.rs`."""
    run = subprocess.run([sys.executable, '-I', str(FABRIC / 'tools' / 'seam.py'), function, *([file] if file else [])], capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=60)
    return run.stdout[-3000:] or run.stderr[-800:]


def engine_check():
    """Regenerate the bundle and type-check the engine with every mixin in: about a minute. Takes the lock."""
    begin = time.time()
    with Lock():
        run = subprocess.run([sys.executable, '-I', 'tools/cycle.py', '--check'], cwd=FABRIC, capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=590)
    text = re.sub(r'\x1b\[[0-9;]*m', '', run.stdout + run.stderr)
    log = FABRIC / 'target' / 'build-test.log'
    built = re.sub(r'\x1b\[[0-9;]*m', '', read(log))
    rows = built.splitlines()
    errors = [line[:220] for number, line in enumerate(rows) if line.startswith('error') or (line.lstrip().startswith('-->') and rows[number - 1].startswith('error'))]
    refused = [line.strip()[:220] for line in (text + built).splitlines() if '"code"' in line or '"message"' in line or '"injection_id"' in line]
    return {'ok': run.returncode == 0, 'seconds': round(time.time() - begin), 'errors': errors[:40], 'refused_by_the_mixin_planner': refused[:12], 'tail': text[-400:] if run.returncode else ''}


def check_start(kind='full'):
    """Start a long check in the background: `full` (conformance, event probe, parity, proof), `conformance`, `probe`, `parity` or `proof`. Read it with check_status."""
    scripts = {'events': ('event_parity.py', TOOLS / 'tools'), 'fast': ('full_check.py', TOOLS / 'tools'), 'full': ('full_check.py', TOOLS / 'tools'), 'conformance': ('conformance.py', TOOLS / 'tools'), 'probe': ('event_probe.py', TOOLS / 'tools'), 'parity': ('parity_all.py', TOOLS / 'tools'), 'proof': ('tools/cycle.py', FABRIC)}
    if kind not in scripts:
        return {'error': f'one of {sorted(scripts)}'}
    script, cwd = scripts[kind]
    log = JOB.open('w', encoding='utf-8')
    log.write(f'== {kind} from {time.strftime("%H:%M:%S")}\n')
    log.flush()
    flags = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP
    process = subprocess.Popen([sys.executable, '-I', script, *(['--fast'] if kind == 'fast' else [])], cwd=cwd, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, creationflags=flags, close_fds=True)
    (TOOLS / '.job.pid').write_text(str(process.pid), encoding='utf-8')
    return {'started': kind, 'pid': process.pid, 'next': 'check_status'}


def event_results(name=''):
    """What the last event comparison found: events on Paper only, on Horizon only, and counts far apart. `name` narrows it."""
    path = TOOLS / 'event-parity.json'
    if not path.is_file():
        return {'error': 'no comparison yet: check_start events'}
    data = json.loads(path.read_text(encoding='utf-8'))
    paper, horizon = data['paper'], data['horizon']
    noise = {'ServerTickStartEvent', 'ServerTickEndEvent', 'PlayerMoveEvent', 'EntityMoveEvent', 'ChunkLoadEvent', 'ChunkUnloadEvent', 'BlockPhysicsEvent'}
    keep = lambda event: name.lower() in event.lower() and event not in noise
    return {
        'when': time.strftime('%H:%M:%S', time.localtime(path.stat().st_mtime)),
        'fired': {'paper': len(paper), 'horizon': len(horizon), 'both': len(set(paper) & set(horizon))},
        'paper_only': {event: paper[event] for event in sorted(paper) if event not in horizon and keep(event)},
        'horizon_only': {event: horizon[event] for event in sorted(horizon) if event not in paper and keep(event)},
        'far_apart': {event: [paper[event], horizon[event]] for event in sorted(set(paper) & set(horizon)) if keep(event) and max(paper[event], horizon[event]) > 8 and (paper[event] > 4 * horizon[event] or horizon[event] > 4 * paper[event])},
    }


def thread_stacks(match='', everything=False):
    """Where a running engine's threads are, with no debugger: why a server is frozen. `match` picks by command line (`paper-server`, `rig`)."""
    command = [sys.executable, '-I', str(TOOLS / 'tools' / 'stacks.py'), *(['--all'] if everything else []), *(['--match', match] if match else [])]
    result = subprocess.run(command, capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=120)
    return {'stacks': (result.stdout + result.stderr)[-12000:]}


def lock_scan():
    """Engine code that holds a static lock across a call that could want it again: the shape of the freeze on the first mob hurt."""
    result = subprocess.run([sys.executable, '-I', str(TOOLS / 'tools' / 'lockscan.py')], capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=60)
    return {'held': result.stdout.splitlines()}


def check_status(lines=40):
    """How the check from check_start stands: still running or done, with its last lines."""
    pid = read(TOOLS / '.job.pid').strip()
    listed = subprocess.run(['tasklist', '/FI', f'PID eq {pid}', '/NH'], capture_output=True, text=True).stdout if pid else ''
    return {'running': bool(pid) and pid in listed, 'output': read(JOB).splitlines()[-lines:]}


def maker_guide(topic=''):
    """How to make a plugin, a mod or an engine hook here, with the traps. Topics: plugin, mod, hook, test, traps."""
    text = read(GUIDE)
    if not topic:
        return text
    parts = re.split(r'(?m)^## ', text)
    hits = [part for part in parts if part.lower().startswith(topic.lower())]
    return '## ' + hits[0] if hits else f'no topic {topic}: ' + ', '.join(part.split('\n')[0] for part in parts[1:])


TOOLS_MORE = {
    'sdk_search': (sdk_search, 'Find Rust in horizon-bukkit by words, with signatures, file and line, and any note on what is not exact.', {'query': 'string', 'limit': 'integer'}),
    'java_hints': (java_hints, 'A Java file against the SDK: each listener and API call it has, the Rust for it, and what has none.', {'file': 'string'}),
    'event_info': (event_info, 'A Paper event: how to listen, what it reads and changes, what is not exact, and if the last probe saw it fire. Empty for the whole picture.', {'query': 'string', 'limit': 'integer'}),
    'plugin_import': (plugin_import, 'Bring in any Bukkit, Spigot or Paper jar by path or URL: decompile it, count it, and say what a port takes. plugin_brief and plugin_scaffold then take it.', {'source': 'string'}),
    'plugin_new': (plugin_new, 'Start a new plugin from nothing: a crate that builds and loads as it stands.', {'name': 'string', 'commands': 'string', 'description': 'string'}),
    'mod_new': (mod_new, 'Start a new mod from nothing on horizon-mod: a crate that builds and loads as it stands.', {'name': 'string', 'commands': 'string', 'events': 'string', 'description': 'string'}),
    'plugin_check': (plugin_check, 'Compile a plugin or mod without building it, suggesting close SDK names for what does not exist.', {'id': 'string'}),
    'plugin_audit': (plugin_audit, 'What keeps a plugin or mod from being finished: stubs, commands with no code, SDK calls that are not exact.', {'id': 'string'}),
    'plugin_deploy': (plugin_deploy, 'Put a plugin or mod DLL on the test server and restart it. Refuses while someone plays unless forced.', {'id': 'string', 'force': 'boolean'}),
    'plugin_remove': (plugin_remove, 'Take a plugin or mod DLL off the test server and restart it.', {'id': 'string', 'force': 'boolean'}),
    'plugin_try': (plugin_try, 'One call from DLL to chat: deploy, join with a test client, run commands, return chat, trouble and a screenshot. Commands are split on ; or newlines. new_engine puts the newest engine build on first; the client plays in creative unless survival is set.', {'id': 'string', 'commands': 'string', 'seconds': 'number', 'screenshot': 'boolean', 'force': 'boolean', 'new_engine': 'boolean', 'survival': 'boolean'}),
    'server_status': (server_status, 'The test server: up or not, who is on, what it loads, and trouble in its log.', {}),
    'server_log': (server_log, "The test server's log, filtered by a regular expression.", {'filter': 'string', 'lines': 'integer'}),
    'mod_events': (mod_events, 'What host.on can hear: each event, its payload, and if a refusal works.', {'query': 'string', 'limit': 'integer'}),
    'fabric_event': (fabric_event, 'A Fabric API event: its status here, and where its Java is.', {'query': 'string', 'limit': 'integer'}),
    'mod_scan': (mod_scan, 'What a Fabric mod jar needs, by path or URL: events ported or not, and its Mixin count.', {'source': 'string'}),
    'hook_search': (hook_search, 'The engine hooks that exist: ids, targets, what they pass.', {'query': 'string', 'limit': 'integer'}),
    'hook_seam': (hook_seam, 'Which injection kinds an engine function takes, before writing a hook.', {'function': 'string', 'file': 'string'}),
    'engine_check': (engine_check, 'Regenerate the bundle and type-check the engine with every mixin in, giving errors and planner refusals.', {}),
    'check_start': (check_start, 'Start a check in the background: fast (conformance and the event comparison, about 4 minutes), full (those with parity and the proof, about 8 minutes), events, conformance, probe, parity or proof.', {'kind': 'string'}),
    'check_status': (check_status, 'How the check from check_start stands.', {'lines': 'integer'}),
    'event_results': (event_results, 'What the last event comparison found: events firing on Paper only, on Horizon only, and counts far apart. Run one with check_start kind=events: every event class listened to on real Paper and on Horizon, the same real actions played on both, a minute and a half with the rig up.', {'name': 'string'}),
    'thread_stacks': (thread_stacks, 'Where the threads of a running engine are, for a frozen or slow server: no debugger needed.', {'match': 'string', 'everything': 'boolean'}),
    'lock_scan': (lock_scan, 'Engine code holding a static lock across a call that could want it again.', {}),
    'maker_guide': (maker_guide, 'How to make a plugin, a mod or an engine hook here, and the traps. Topics: plugin, mod, hook, test, traps.', {'topic': 'string'}),
}
