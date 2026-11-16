#!/usr/bin/env python3
"""The plugin and mod line as an MCP server: pick a plugin, read what it needs, start it, build it,
try it on a test server, and prove it against the Java original. `maker_tools.py` holds the second half.

Register it with: claude mcp add --scope user plugin-port -- python -I sdk/mcp/plugin_mcp.py

Jars are read as data here, and decompiled by Vineflower. Only `plugin_parity` runs one, on Paper.
"""
import json
import shlex
import os
import pathlib
import re
import shutil
import subprocess
import sys
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import workspace  # noqa: E402
import maker_tools  # noqa: E402
import rewrite_tools  # noqa: E402
import uui_tools  # noqa: E402
import mod_tools  # noqa: E402
import jl_tools  # noqa: E402
import port_one  # noqa: E402
import sdk_tools  # noqa: E402
import rig_tools  # noqa: E402

HORIZON = workspace.WORKSPACE
TOOLS = HORIZON / 'plugin-tools'
REPORT = HORIZON / 'plugin-report-sdk'
CORPUS = HORIZON / 'plugin-corpus'
PORTS = HORIZON / 'plugin-ports'
SDK = sdk_tools.SDK_SRC.parent  # this repo's sdk/horizon-bukkit (the old fabric-port copy is stale)
SCOUT = TOOLS / 'target' / 'release' / 'plugin-scout.exe'
JAVA = workspace.JAVA
TARGET = PORTS / 'target'


def plugins():
    return [json.loads(line) for line in (REPORT / 'plugins.jsonl').read_text(encoding='utf-8').splitlines() if line.strip()] + maker_tools.import_rows()


def mapping():
    return json.loads((TOOLS / 'mapping-sdk.json').read_text(encoding='utf-8'))


def find(name):
    wanted = name.lower()
    rows = plugins()
    return next((r for r in rows if (r.get('name') or '').lower() == wanted), None) or next((r for r in rows if wanted in (r.get('name') or '').lower()), None)


def identifier(name):
    text = re.sub(r'[^a-z0-9]+', '_', name.lower()).strip('_')
    return text if text and not text[0].isdigit() else f'p_{text}'


def rust_of(calls, java):
    """What the SDK has for a Java call, by the call or by its class."""
    return calls.get(java) or calls.get(java.split('.')[0])


def plugin_queue(limit=20, tier='AB'):
    """The next plugins to port: easiest first, most-wanted API last."""
    done = {p.name for p in PORTS.iterdir() if p.is_dir()} if PORTS.is_dir() else set()
    rows = [r for r in plugins() if r.get('tier', 'E') in tier]
    rows.sort(key=lambda r: (r.get('tier', 'E'), len(r.get('unmapped_calls', [])), r.get('classes', 0)))
    return [{
        'name': r['name'], 'version': r.get('version'), 'tier': r['tier'], 'classes': r.get('classes'),
        'unmapped_calls': len(r.get('unmapped_calls', [])), 'license': r.get('license'),
        'ported': identifier(r['name']) in done, 'why': r.get('why', [])[:3],
    } for r in rows[:limit]]


def decompile(row):
    out = PORTS / '_java' / identifier(row['name'])
    if not out.is_dir():
        out.mkdir(parents=True)
        subprocess.run([JAVA, '-jar', str(TOOLS / 'tools' / 'vineflower.jar'), '--silent', str(CORPUS / row['file']), str(out)], check=False, capture_output=True, timeout=600)
    return out


def plugin_brief(name):
    """All one needs to port a plugin: its files, its events, each call's Rust, and what is missing."""
    row = find(name)
    if not row:
        return {'error': f'no plugin like {name}'}
    source = decompile(row)
    calls = mapping()['calls']
    java_files = sorted(str(p.relative_to(source)) for p in source.rglob('*.java'))
    lines = sum(len((source / f).read_text(encoding='utf-8', errors='replace').split('\n')) for f in java_files)
    used = sorted({call for handler in row.get('handlers', []) for call in handler.get('calls', [])} | set(row.get('unmapped_calls', [])))
    return {
        'name': row['name'], 'version': row.get('version'), 'tier': row['tier'], 'why': row.get('why'),
        'license': row.get('license'),
        'depend': row.get('depend'), 'softdepend': row.get('softdepend'),
        'java_source': str(source), 'java_files': java_files, 'java_lines': lines,
        'commands': row.get('commands'), 'permissions': row.get('permissions'),
        'handlers': [{'event': h.get('event'), 'method': f"{h.get('class')}.{h.get('method')}"} for h in row.get('handlers', [])],
        # A big plugin's whole table would fill a reply: `java_hints` gives it file by file.
        'api_calls': len(used),
        'api': {call: rust_of(calls, call) or 'MISSING: add it to horizon-bukkit' for call in used if len(used) <= 200 or not rust_of(calls, call)},
    }


def api_index():
    return json.loads((HORIZON / 'paper-api' / 'api.json').read_text(encoding='utf-8'))


def api_lookup(java):
    """A Java class or method: its signature, Javadoc, how many plugins want it, and the Rust for it."""
    needle = java.replace('#', '.').replace('.java', '').lower()
    hits = {k: v for k, v in api_index().items() if needle in k.lower()}
    ranked = sorted(hits.items(), key=lambda item: -item[1]['wanted'])[:25]
    return {'matches': len(hits), 'found': dict(ranked)}


def api_gaps(limit=40):
    """What to add to horizon-bukkit next: missing methods, by how many sampled plugins call them."""
    index = api_index()
    missing = [(e['wanted'], k, e) for k, e in index.items() if e['wanted'] and not (e['exact'] or e['partial'])]
    missing.sort(key=lambda item: -item[0])
    head = (HORIZON / 'paper-api' / 'COVERAGE.md').read_text(encoding='utf-8').split('## Most wanted', 1)[0]
    return {'coverage': head, 'next': [{'java': k, 'plugins': n, 'signature': e['signature'], 'doc': e['doc']} for n, k, e in missing[:limit]]}


HORIZON_SOURCE = HORIZON / 'fabric-port' / 'target' / 'source' / 'cbfae59314dfa74d54cb8a379cae609c6f5ec91d' / 'crates'
JAVA_ROOT = PORTS / '_java'


def grep(root, suffix, pattern, limit):
    """`path:line: text`, for a regular expression over a tree of text files."""
    needle = re.compile(pattern)
    found = []
    for path in root.rglob(f'*{suffix}'):
        try:
            text = path.read_text(encoding='utf-8', errors='replace')
        except OSError:
            continue
        if not needle.search(text):
            continue
        for number, line in enumerate(text.split('\n'), 1):
            if needle.search(line):
                found.append(f'{path.relative_to(root)}:{number}: {line.strip()[:160]}')
                if len(found) >= limit:
                    return found
    return found


def search_plugins(pattern, limit=40):
    """Search the decompiled code of every sampled plugin, with a regular expression."""
    return grep(JAVA_ROOT, '.java', pattern, limit)


def search_horizon(pattern, limit=40):
    """Search Horizon's engine source, with a regular expression."""
    return grep(HORIZON_SOURCE, '.rs', pattern, limit)


def used_by(java, limit=15):
    """Real code that calls a Paper method: `Player.isSneaking`. Shows how plugins really use it."""
    method = java.replace('#', '.').rsplit('.', 1)[-1]
    index = api_index()
    wanted = max((e['wanted'] for k, e in index.items() if k.lower().endswith(java.replace('#', '.').lower())), default=0)
    return {'plugins_calling_it': wanted, 'examples': search_plugins(rf'\.{re.escape(method)}\(', limit)}


def snake(name):
    return re.sub(r'(?<!^)([A-Z])', r'_\1', name).lower()


def api_kit(java):
    """Everything to implement one Paper method: signature, Javadoc, demand, the Rust so far, real
    uses, and the engine code that looks related."""
    needle = java.replace('#', '.').lower()
    hits = sorted(((k, e) for k, e in api_index().items() if needle in k.lower()), key=lambda item: -item[1]['wanted'])
    if not hits:
        return {'error': f'no API method like {java}'}
    key, entry = hits[0]
    method = key.rsplit('.', 1)[-1]
    name = snake(re.sub(r'^(get|set|is|has)(?=[A-Z])', '', method)) or snake(method)
    return {
        'java': key,
        'others_with_this_name': [k for k, _ in hits[1:8]],
        **entry,
        'examples': search_plugins(rf'\.{re.escape(method)}\(', 6),
        'horizon_functions': search_horizon(rf'fn \w*{re.escape(name)}\w*\(', 12),
        'native_api': search_horizon(rf'fn native_\w*{re.escape(name)}\w*\(', 6),
    }


def sdk_refresh():
    """Reread the SDK's `bukkit:` lines and rerate every plugin. Run after adding to the SDK."""
    subprocess.run([sys.executable, '-I', str(TOOLS / 'tools' / 'sdk_map.py')], check=True, capture_output=True)
    subprocess.run([str(SCOUT), 'scan', str(CORPUS), '--out', str(REPORT), '--mapping', str(TOOLS / 'mapping-sdk.json')], check=True, capture_output=True)
    subprocess.run([sys.executable, '-I', str(TOOLS / 'tools' / 'api_index.py')], check=True, capture_output=True)
    return (HORIZON / 'paper-api' / 'COVERAGE.md').read_text(encoding='utf-8').split('## Most wanted', 1)[0] + (REPORT / 'REPORT.md').read_text(encoding='utf-8').split('## One hook', 1)[0]


CARGO = '''[package]
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


def plugin_scaffold(name):
    """Starts a port: plugin.yml and its resources from the jar, a crate, a stub for each command and
    event, and a parity script to fill in."""
    row = find(name)
    if not row:
        return {'error': f'no plugin like {name}'}
    id_ = identifier(row['name'])
    out = PORTS / id_
    if out.exists():
        return {'error': f'{out} is there already'}
    (out / 'src').mkdir(parents=True)
    with zipfile.ZipFile(CORPUS / row['file']) as jar:
        for entry in jar.namelist():
            if '/' not in entry and entry.rsplit('.', 1)[-1] in ('yml', 'yaml', 'json', 'txt', 'properties'):
                (out / entry).write_bytes(jar.read(entry))
    (out / 'Cargo.toml').write_text(CARGO.format(id=id_, sdk=SDK.as_posix()), encoding='utf-8')
    (out / 'fabric.mod.json').write_text(json.dumps({'schemaVersion': 1, 'id': id_, 'version': str(row.get('version') or '0'), 'name': row['name'], 'description': f'{row["name"]}, rewritten in Rust.', 'depends': {'fabric_api': '>=0.1.0'}}, indent=2), encoding='utf-8')
    commands = row.get('commands') or []
    names = [c if isinstance(c, str) else c.get('name', '') for c in commands]
    code = [f'// {row["name"]} {row.get("version")}, rewritten class by class.',
            'use horizon_bukkit::{CommandSender, Plugin, bukkit_plugin};', '']
    for command in names:
        code += [f'fn {identifier(command)}_command(sender: &CommandSender, label: &str, args: &[&str]) -> bool {{', f'    todo!("port /{command}: {{}} {{}} {{}}", sender.get_name(), label, args.len())', '}', '']
    code += ['fn on_enable(plugin: Plugin) {']
    code += [f'    plugin.set_executor("{c}", {identifier(c)}_command);' for c in names]
    for handler in row.get('handlers', []):
        code.append(f'    // {handler.get("event")}: {handler.get("class")}.{handler.get("method")}')
    code += ['}', '', 'fn on_disable(_: Plugin) {}', '', 'bukkit_plugin!(on_enable, on_disable);', '']
    (out / 'src' / 'lib.rs').write_text('\n'.join(code), encoding='utf-8')
    parity = TOOLS / 'parity' / f'{id_}.json'
    if not parity.exists():
        parity.write_text(json.dumps({'plugin': row['name'], 'files': [], 'ignore': [], 'setup': ['/gamemode creative', '/gamerule send_command_feedback false'], 'steps': [f'/{c}' for c in names]}, indent=2), encoding='utf-8')
    return {'port': str(out), 'parity_script': str(parity), 'next': 'plugin_brief, then rewrite each Java class in src/lib.rs'}


def plugin_rewrite(source, id=''):
    """A plugin to the start of its native Rust rewrite on horizon-kit (kit/KIT.md): a crate that builds and loads,
    with config/messages, a command router with every sub-command found, and BRIEF.md (commands, events, config
    defaults, an SDK map of every Bukkit call it makes with the Rust for it, kit-first hints) + tests/check.py.
    `source`: a jar path, a name in the corpus, a source folder/zip, or anything `--fetch` takes (a plugin name;
    `github:owner/repo`, `modrinth:slug`, `hangar:owner/slug`, `spiget:id`, a URL)."""
    path = pathlib.Path(source)
    args = None
    if not path.exists():
        found = [p for root in (CORPUS, HORIZON / 'plugin-sweep' / 'jars', HORIZON / 'plugin-imports', HORIZON / 'plugin-downloads') for p in root.rglob('*.jar') if source.lower().replace(' ', '') in p.name.lower()]
        if found and ':' not in source:
            path = found[0]
        else:
            name = source.split(':', 1)[-1].rstrip('/').split('/')[-1]
            args = ['--fetch', source, str(PORTS / (id or identifier(name) + '_rs'))]
    if args is None:
        stem = re.sub(r'[-_.]?\d.*$', '', path.stem).split('-')[-1] if path.is_file() else path.name
        args = [str(path), str(PORTS / (id or identifier(stem) + '_rs'))]
    done = subprocess.run(['python', str(TOOLS / 'kit' / 'new_rewrite.py'), *args] + (['--id', id] if id else []), capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=900)
    if done.returncode:
        return {'error': (done.stderr or done.stdout)[-1200:]}
    out = json.loads(done.stdout[done.stdout.index('{'):])
    out.pop('data', None)
    return {**out, 'brief': str(pathlib.Path(out['crate']) / 'BRIEF.md'), 'next': 'read BRIEF.md (SDK map + kit map), rm src/lib.rs and write the plugin with `use horizon_kit::prelude::*`, then rewrite_build'}


REWRITE_TARGET = HORIZON / 'hx-plugin-build' / 't_rw'


def rewrite_build(crate, release=True):
    """Builds a rewrite crate (a folder under plugin-ports, or a path) with the shared target dir, and answers
    in a few lines: ok, the DLL, and each error as `file:line: message` with its `help:` line (warnings counted,
    the first few shown). Token-cheap: no cargo noise."""
    folder = pathlib.Path(crate)
    if not folder.is_dir():
        folder = PORTS / crate
        if not folder.is_dir():
            matches = [d for d in PORTS.iterdir() if d.is_dir() and crate.lower() in d.name.lower() and (d / 'Cargo.toml').exists()]
            if not matches:
                return {'error': f'no crate {crate}'}
            folder = matches[0]
    env = {**os.environ, 'CARGO_TARGET_DIR': str(REWRITE_TARGET), 'CARGO_TERM_COLOR': 'never', 'CARGO_BUILD_JOBS': os.environ.get('CARGO_BUILD_JOBS', '8')}
    command = ['cargo', 'build', '--message-format=short'] + (['--release'] if release else [])
    result = subprocess.run(command, cwd=folder, capture_output=True, text=True, encoding='utf-8', errors='replace', env=env, timeout=900)
    errors, warnings = [], []
    for line in result.stderr.splitlines():
        if ': error' in line or line.startswith('error'):
            if 'could not compile' not in line and 'aborting due to' not in line:
                errors.append(line.strip())
        elif ': warning' in line and 'generated' not in line:
            warnings.append(line.strip())
    name = re.search(r'name\s*=\s*"([^"]+)"', (folder / 'Cargo.toml').read_text(encoding='utf-8')).group(1).replace('-', '_')
    dll = REWRITE_TARGET / ('release' if release else 'debug') / f'{name}.dll'
    reply = {'ok': result.returncode == 0, 'dll': str(dll) if dll.exists() else None, 'errors': errors[:25], 'warnings': len(warnings)}
    if warnings:
        reply['first_warnings'] = warnings[:5]
    if not errors and result.returncode:
        reply['tail'] = result.stderr[-800:]
    return reply


def kit_doc(topic=''):
    """horizon-kit in brief: no topic = every module's one-line summary and the recipe (KIT.md); a module name
    (`gui`, `item`, `store`, `hooks`, `mini`, `yaml`, `wire`, `sync`, `time`...) = that module's doc and its
    public functions with signatures. Cheaper than reading the source."""
    # This repository's own kit first (sdk/horizon-kit), a workspace checkout's otherwise.
    here = pathlib.Path(__file__).resolve().parent.parent / 'horizon-kit'
    kit = here / 'src' if (here / 'src').is_dir() else TOOLS / 'kit' / 'horizon-kit' / 'src'
    guide = here / 'KIT.md' if (here / 'KIT.md').is_file() else TOOLS / 'kit' / 'KIT.md'
    if not topic:
        lib = (kit / 'lib.rs').read_text(encoding='utf-8')
        docs = '\n'.join(l[4:] for l in lib.splitlines() if l.startswith('//!'))
        return docs + '\n\n' + guide.read_text(encoding='utf-8')
    f = kit / f'{topic.strip().lower()}.rs'
    if not f.exists():
        return {'error': f'no module {topic}', 'modules': sorted(x.stem for x in kit.glob('*.rs'))}
    text = f.read_text(encoding='utf-8')
    head = '\n'.join(l[4:] for l in text.splitlines() if l.startswith('//!'))
    sigs = []
    lines = text.splitlines()
    for i, l in enumerate(lines):
        t = l.strip()
        if t.startswith(('pub fn ', 'pub struct ', 'pub enum ', 'pub const ', 'pub static ', 'pub type ', 'pub trait ')) and '#[cfg(test)]' not in text[:text.find(l)][-200:]:
            doc = []
            j = i - 1
            while j >= 0 and lines[j].strip().startswith('///'):
                doc.insert(0, lines[j].strip()[4:])
                j -= 1
            sig = t.split('{')[0].rstrip()
            sigs.append(sig + (f'  // {" ".join(doc)[:160]}' if doc else ''))
    return head + '\n\n' + '\n'.join(sigs)


def plugin_build(id, spread=False):
    """Builds a port into its DLL: seconds, with no engine build."""
    folder = maker_tools.folder_of(id)
    if not folder:
        return {'error': f'no crate {id}'}
    # Spread: rustc's analysis on 4 threads, for this crate alone. Converted plugins are one huge file.
    command = ['cargo', '+1.99.1', 'rustc', '--lib', '--release', '--', '-Zthreads=4'] if spread else ['cargo', '+1.99.1', 'build', '--release']
    extra = {'RUSTC_BOOTSTRAP': '1'} if spread else {}
    result = subprocess.run(command, cwd=folder, capture_output=True, text=True, encoding='utf-8', errors='replace', env={**__import__('os').environ, **extra, 'CARGO_TARGET_DIR': str(maker_tools.dll_of(id).parent.parent), 'CARGO_TERM_COLOR': 'never'}, timeout=590)
    rows = result.stderr.splitlines()
    errors = [line for number, line in enumerate(rows) if line.startswith('error') or (line.lstrip().startswith('-->') and rows[number - 1].startswith('error'))]
    return {'ok': result.returncode == 0, 'dll': str(maker_tools.dll_of(id)), 'errors': errors[:40]}


def plugin_convert(jar, id=''):
    """A plugin's jar to Rust that builds, with no hand work: see jport/. What it couldn't do is in the reply."""
    path = pathlib.Path(jar)
    if not path.is_file():
        found = [p for root in (CORPUS, HORIZON / 'plugin-imports', HORIZON / 'plugin-random') for p in root.rglob('*.jar') if jar.lower() in p.name.lower()]
        if not found:
            return {'error': f'no jar {jar}'}
        path = found[0]
    id_ = id or identifier(re.sub(r'[-_.]?\d.*$', '', path.stem).split('-')[-1]) + '_auto'
    jport = TOOLS / 'jport'
    done = subprocess.run([str(jport / '.venv' / 'Scripts' / 'python.exe'), str(jport / 'jport.py'), 'jar', str(path), str(PORTS / id_), '--id', id_], capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=590)
    if done.returncode:
        return {'error': done.stderr[-800:] or done.stdout[-800:]}
    report = json.loads(done.stdout[done.stdout.index('{'):])
    report.pop('by_area', None)
    report['id'] = id_
    report['build'] = plugin_build(id_, spread=True)
    return report


def plugin_parity(id):
    """Runs the port's script on Paper with the Java plugin and on Horizon with the port, and compares."""
    row = next((r for r in plugins() if identifier(r['name']) == id), None)
    if not row:
        return {'error': f'no plugin {id}'}
    result = subprocess.run([sys.executable, '-I', str(TOOLS / 'tools' / 'parity.py'), str(CORPUS / row['file']), str(TARGET / 'release' / f'{id}.dll'), str(TOOLS / 'parity' / f'{id}.json')], capture_output=True, text=True, timeout=900)
    lines = [line for line in result.stdout.splitlines() if line.startswith(('same', 'DIFF', '  ', '      ')) or ' differences in ' in line]
    return {'identical': result.returncode == 0, 'report': lines[-120:], 'errors': result.stderr[-600:]}


def game_ctl(home, *words):
    """A line to a running game's control socket. `home` is its HORIZON_HOME, when it isn't the usual one."""
    versions = pathlib.Path(os.environ['APPDATA']) / 'horizon-launcher' / 'versions'
    ctl = max(versions.glob('*/horizon-ctl.exe'), key=lambda path: path.stat().st_mtime)
    env = dict(os.environ, **({'HORIZON_HOME': home} if home else {}))
    done = subprocess.run([str(ctl), '--timeout', '120', *map(str, words)], capture_output=True, text=True, env=env, timeout=140)
    return (done.stdout or done.stderr).strip()


def video_record(path, fps=60, home=''):
    return game_ctl(home, 'record', path, '--fps', fps)


def video_stop(home=''):
    return game_ctl(home, 'record-stop')


def video_edit(command):
    words = shlex.split(command, posix=False)
    words = [word.strip('"') for word in words]
    done = subprocess.run([sys.executable, '-I', str(TOOLS / 'tools' / 'video.py'), *words], capture_output=True, text=True, timeout=590)
    return (done.stdout + done.stderr).strip()


TOOLS_LIST = {
    'video_record': (video_record, 'Start recording video and sound in a running Horizon client, windowed or --offscreen (not --headless). Ends with video_stop.', {'path': 'string', 'fps': 'integer', 'home': 'string'}),
    'video_stop': (video_stop, 'Finish the recording and join its sound. Gives the path, frames, seconds and whether it has sound.', {'home': 'string'}),
    'video_edit': (video_edit, 'Edit a video. A line of tools/video.py: info IN, trim IN OUT --from S --to S, join OUT IN IN [--fade S], speed IN OUT FACTOR, title IN OUT TEXT [--at S --for S], sheet IN OUT [--stills N], still IN OUT --at S, compress IN OUT [--crf N].', {'command': 'string'}),
    'plugin_queue': (plugin_queue, 'The next plugins to port, easiest first.', {'limit': 'integer', 'tier': 'string'}),
    'plugin_brief': (plugin_brief, 'Everything needed to port a plugin: decompiled source, events, and the Rust for each Java call.', {'name': 'string'}),
    'api_lookup': (api_lookup, 'The horizon-bukkit Rust for a Java class or method.', {'java': 'string'}),
    'api_gaps': (api_gaps, 'What to add to horizon-bukkit next, by how many plugins want it.', {'limit': 'integer'}),
    'api_kit': (api_kit, 'Everything to implement one Paper method: signature, Javadoc, demand, real uses, likely engine code.', {'java': 'string'}),
    'used_by': (used_by, 'Real plugin code that calls a Paper method.', {'java': 'string', 'limit': 'integer'}),
    'search_plugins': (search_plugins, 'Regex search of every decompiled plugin.', {'pattern': 'string', 'limit': 'integer'}),
    'search_horizon': (search_horizon, "Regex search of Horizon's engine source.", {'pattern': 'string', 'limit': 'integer'}),
    'sdk_refresh': (sdk_refresh, 'Reread the SDK and rerate every plugin.', {}),
    'plugin_rewrite': (plugin_rewrite, 'THE START OF A NATIVE REWRITE ("rewrite X in Rust"): a jar path, corpus name, source folder/zip, or a plugin to fetch (name, github:owner/repo, modrinth:slug, hangar:owner/slug, spiget:id, URL) to a horizon-kit crate that builds and loads in ~30 s; BRIEF.md has an SDK map of every Bukkit call it makes: config + messages, a router with every sub-command found (permissions, usage), events/timers noted, BRIEF.md and tests/check.py. See plugin-tools/kit/KIT.md.', {'source': 'string', 'id': 'string'}),
    'rewrite_build': (rewrite_build, 'Build a native rewrite crate (plugin-ports/<name> or a path) with the shared target dir; a few lines back: ok, the DLL, `file:line: error` with help lines, warning count.', {'crate': 'string', 'release': 'boolean'}),
    'kit_doc': (kit_doc, 'horizon-kit reference without reading source: no topic = module list + KIT.md recipe; a module (gui, item, store, hooks, mini, yaml, wire, sync, time, sound, hocon, packets, cmd, config...) = its doc and public signatures.', {'topic': 'string'}),
    'plugin_scaffold': (plugin_scaffold, 'Start a port: crate, plugin.yml, resources, stubs and a parity script.', {'name': 'string'}),
    'plugin_port': (port_one.plugin_port, 'THE ONE TO USE when someone asks to convert, port or install a Java plugin (Bukkit, Spigot, Paper) for Horizon. One call does it all: finds the jar (a Modrinth name like "LuckPerms", a download link, or a path), converts it to Rust, builds it, starts it once on a scratch server to see it enable, and with `server` (a launcher server name like "test") installs it there, backing up and restarting the server; it refuses while players are on unless force. Takes minutes for a big plugin. The reply gives each step and an honest note of what may not work.', {'source': 'string', 'server': 'string', 'version': 'string', 'id': 'string', 'force': 'boolean'}),
    'plugin_port_many': (lambda sources, server='', force=False, workers=3: port_one.plugin_port_many([s.strip() for s in sources.replace('\n', ',').split(',') if s.strip()], server, force, workers),
                         'Like plugin_port for any number of plugins at once: `sources` is a comma-separated list of Modrinth names, links or paths. They convert side by side (`workers` at a time, 3 by default), then every one that enabled cleanly goes onto `server` together with one backup and one restart.',
                         {'sources': 'string', 'server': 'string', 'force': 'boolean', 'workers': 'integer'}),
    'plugin_convert': (plugin_convert, 'Convert a plugin jar to Rust automatically, and build it: seconds, not hours. Gives what share of its methods run and what is missing. Takes a path or part of a jar name.', {'jar': 'string', 'id': 'string'}),
    'plugin_build': (plugin_build, 'Build a port into its DLL.', {'id': 'string'}),
    'plugin_parity': (plugin_parity, 'Prove a port 1:1: same script on the Java plugin on Paper and the port on Horizon.', {'id': 'string'}),
    **maker_tools.TOOLS_MORE,
    **sdk_tools.TOOLS,
    **rig_tools.TOOLS,
    **rewrite_tools.TOOLS,
    **uui_tools.TOOLS,
    **jl_tools.TOOLS,
    # Last: its mod_new (an .hmod with the code inside, on the mod engine) replaces the old engine's.
    **mod_tools.TOOLS,
}


# The replies' own copy of stdout: the tools run servers, and whatever those print goes to stderr.
REPLIES = open(os.dup(1), 'w', encoding='utf-8', newline='\n', buffering=1)
os.dup2(2, 1)


def send(message):
    REPLIES.write(json.dumps(message) + '\n')
    REPLIES.flush()


def reply(request, result):
    send({'jsonrpc': '2.0', 'id': request.get('id'), 'result': result})


def main():
    for line in sys.stdin:
        try:
            request = json.loads(line)
        except json.JSONDecodeError:
            continue
        method = request.get('method')
        if method == 'initialize':
            reply(request, {'protocolVersion': request['params'].get('protocolVersion', '2024-11-05'), 'capabilities': {'tools': {}}, 'serverInfo': {'name': 'plugin-port', 'version': '0.2.0'}})
        elif method == 'tools/list':
            tools = [{'name': name, 'description': description, 'inputSchema': {'type': 'object', 'properties': {key: {'type': kind} for key, kind in fields.items()}}} for name, (_, description, fields) in TOOLS_LIST.items()]
            reply(request, {'tools': tools})
        elif method == 'tools/call':
            name, arguments = request['params']['name'], request['params'].get('arguments') or {}
            try:
                result = TOOLS_LIST[name][0](**arguments)
                reply(request, {'content': [{'type': 'text', 'text': result if isinstance(result, str) else json.dumps(result, indent=1)}]})
            except Exception as error:
                reply(request, {'content': [{'type': 'text', 'text': f'{type(error).__name__}: {error}'}], 'isError': True})
        elif method == 'ping':
            reply(request, {})
        elif 'id' in request:
            send({'jsonrpc': '2.0', 'id': request['id'], 'error': {'code': -32601, 'message': f'no method {method}'}})


if __name__ == '__main__':
    main()
