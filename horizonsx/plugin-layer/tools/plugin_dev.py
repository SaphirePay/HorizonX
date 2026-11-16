#!/usr/bin/env python3
"""Tools for writing Horizon plugins in Rust (horizon-bukkit), made for AIs as much as people: look the Rust up from the
Bukkit/Paper name you know, start a plugin, build it (with the fix for the usual mistakes), test it on a real server,
deploy it to a running one. The same tools as a command line and as an MCP server.

  python -I tools/plugin_dev.py lookup Player.setHealth      the Rust for a Java class, method or Class.method
  python -I tools/plugin_dev.py search "teleport spawn"       SDK functions whose name, Java name or docs hold the words
  python -I tools/plugin_dev.py event PlayerJoinEvent         what an event has, how to listen to it
  python -I tools/plugin_dev.py example [topic]              the tested examples, or one's code
  python -I tools/plugin_dev.py new NAME DIR [--command c,d]  a plugin crate ready to build
  python -I tools/plugin_dev.py build DIR                     build it: the DLL, or the errors with their usual fix
  python -I tools/plugin_dev.py test DIR [--command c,d]      start it on a scratch server with a player, run its commands
  python -I tools/plugin_dev.py deploy DIR SERVER_DIR         copy the DLL into a server's plugins/: hot reload swaps it
  python -I tools/plugin_dev.py --mcp                         the same tools over MCP (stdio)
"""
import csv
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SDK = ROOT / 'sdk' / 'horizon-bukkit'
INDEX = SDK / 'API_INDEX.tsv'
EXAMPLES = ROOT / 'sdk' / 'examples'
TARGET = pathlib.Path(os.environ.get('HORIZON_PLUGIN_TARGET', r'D:\Horizon\sdk-target'))
ENGINE = pathlib.Path(os.environ.get('HORIZON_PLUGIN_ENGINE', str(ROOT / 'out' / 'release-next' / 'horizon-client.exe')))
CHECK = pathlib.Path(r'D:\Horizon\plugin-tools\tools\plugin_check.py')
TOOLCHAIN = os.environ.get('HORIZON_PLUGIN_TOOLCHAIN', '+1.99.1')


def snake(name):
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()


_rows = None


def rows():
    global _rows
    if _rows is None:
        if not INDEX.is_file() or any(p.stat().st_mtime > INDEX.stat().st_mtime for p in (SDK / 'src').glob('*.rs')):
            subprocess.run([sys.executable, '-I', str(ROOT / 'tools' / 'sdk_index.py')], capture_output=True)
        with INDEX.open(encoding='utf-8') as f:
            _rows = list(csv.DictReader(f, delimiter='\t'))
    return _rows


def line(row):
    approx = ' (works, not exact: read its doc comment)' if row['approx'] else ''
    short = row['java_class'].rsplit('.', 1)[-1]
    return f"{short}.{row['java_method']} -> {row['rust_path']}  ::  {row['signature']}{approx}"


def lookup(java, limit=12):
    """The Rust for `java`: `Player.setHealth`, `setHealth`, `Player`, `getConfig`, `InventoryClickEvent`."""
    java = java.strip().replace('#', '.').replace('()', '')
    if '.' in java:
        cls, method = java.rsplit('.', 1)
        cls = cls.rsplit('.', 1)[-1]
    elif java[:1].isupper():
        cls, method = java, ''
    else:
        cls, method = '', java
    found = []
    for row in rows():
        short = row['java_class'].rsplit('.', 1)[-1]
        if cls and short.lower() != cls.lower():
            continue
        if method and row['java_method'].lower() != method.lower():
            continue
        found.append(row)
    if not found and method:
        # The Rust name, or a near one: `setHealth` -> `set_health`.
        want = snake(method)
        found = [r for r in rows() if r['rust_path'].endswith('::' + want) and (not cls or cls.lower() in r['java_class'].lower())]
    if not found:
        return f'Nothing for {java!r}. Try `search` with words, or the class alone to list its methods.'
    # The plain types first: Player before fam_core::Something.
    found.sort(key=lambda r: (r['approx'] != '', r['rust_path'].count('::'), r['rust_path']))
    seen, out = set(), []
    for row in found:
        key = (row['java_class'], row['java_method'], row['rust_path'])
        if key not in seen:
            seen.add(key)
            out.append(line(row))
    more = f'\n... {len(out) - limit} more: ask with Class.method' if len(out) > limit else ''
    return '\n'.join(out[:limit]) + more


def search(words, limit=20):
    """SDK functions whose Java name, Rust name or signature hold every word."""
    terms = [w.lower() for w in words.split()]
    hits = []
    for row in rows():
        text = ' '.join((row['java_class'], row['java_method'], row['rust_path'], row['signature'])).lower()
        if all(t in text for t in terms):
            hits.append(row)
    hits.sort(key=lambda r: (r['rust_path'].count('::'), len(r['signature'])))
    return '\n'.join(line(r) for r in hits[:limit]) or f'Nothing has all of {words!r}.'


def event(name):
    """An event: where it lives, how to listen, what it has."""
    name = name.strip()
    have = [r for r in rows() if r['java_class'].rsplit('.', 1)[-1].lower() == name.lower()]
    if not have:
        return f'No event {name!r}. `search {name[:-5] if name.endswith("Event") else name}` lists near ones.'
    short = have[0]['java_class'].rsplit('.', 1)[-1]
    methods = sorted({line(r) for r in have})
    cancel = any(r['java_method'] == 'setCancelled' for r in have)
    prelude = (SDK / 'src' / 'prelude.rs').read_text(encoding='utf-8') if (SDK / 'src' / 'prelude.rs').is_file() else ''
    import_line = (f'use horizon_bukkit::prelude::*;  // brings {short}' if re.search(rf'\b{short}\b', prelude)
                   else f"use {have[0]['rust_path'].rsplit('::', 1)[0]};  // not in the prelude")
    head = [
        import_line,
        f'plugin.register_event(|event: &mut {short}| {{ ... }});',
        'plugin.register_event_with(enums::event_priority::HIGH, true, |event: &mut ' + short + '| { ... });  // priority, ignoreCancelled',
        f'Cancellable: {"yes, event.set_cancelled(true)" if cancel else "no"}',
        '',
    ]
    return '\n'.join(head + methods)


def example(topic=''):
    """The tested examples; with a topic, the code of the best match."""
    if not EXAMPLES.is_dir():
        return 'No examples folder yet.'
    found = sorted(p for p in EXAMPLES.iterdir() if (p / 'src' / 'lib.rs').is_file())
    if not topic:
        lines = []
        for p in found:
            first = (p / 'src' / 'lib.rs').read_text(encoding='utf-8').splitlines()[0].lstrip('/! ').strip()
            lines.append(f'{p.name}: {first}')
        return '\n'.join(lines)
    terms = [t.lower() for t in topic.split()]

    def score(p):
        text = (p.name + ' ' + (p / 'src' / 'lib.rs').read_text(encoding='utf-8')).lower()
        return sum(text.count(t) for t in terms) + 50 * sum(t in p.name.lower() for t in terms)

    best = max(found, key=score)
    files = [best / 'Cargo.toml', best / 'plugin.yml', *sorted((best / 'src').glob('*.rs')), *sorted(best.glob('*.yml'))]
    out = []
    for f in dict.fromkeys(files):
        if f.is_file():
            out.append(f'--- {f.relative_to(EXAMPLES)}\n{f.read_text(encoding="utf-8")}')
    return '\n'.join(out)


CARGO = '''[package]
name = "{crate}"
version = "1.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
horizon-bukkit = {{ path = "{sdk}" }}

[profile.release]
panic = "unwind"

[workspace]
'''

LIB = '''//! {name}: what it does, in one line.
use horizon_bukkit::prelude::*;

fn on_enable(plugin: Plugin) {{
{body}}}

fn on_disable(_plugin: Plugin) {{}}

bukkit_plugin!(on_enable, on_disable);
'''


def new(name, folder, command=''):
    """A plugin crate: Cargo.toml, plugin.yml, src/lib.rs with a command executor per command."""
    folder = pathlib.Path(folder)
    if (folder / 'Cargo.toml').exists():
        return f'{folder} already has a Cargo.toml.'
    (folder / 'src').mkdir(parents=True, exist_ok=True)
    crate = re.sub(r'[^a-z0-9_]', '_', folder.name.lower())
    (folder / 'Cargo.toml').write_text(CARGO.format(crate=crate, sdk=SDK.as_posix()), encoding='utf-8')
    commands = [c.strip().lstrip('/') for c in command.split(',') if c.strip()]
    yml = [f'name: {name}', 'version: 1.0.0', f'main: {crate}', 'api-version: "1.21"']
    if commands:
        yml.append('commands:')
        for c in commands:
            yml += [f'  {c}:', f'    description: /{c}', f'    usage: /{c}']
    (folder / 'plugin.yml').write_text('\n'.join(yml) + '\n', encoding='utf-8')
    body = ''
    for c in commands:
        body += f'    plugin.set_executor("{c}", |sender: &CommandSender, _label: &str, args: &[&str]| {{\n        sender.send_message(&format!("/{c} {{}}", args.join(" ")));\n        true\n    }});\n'
    (folder / 'src' / 'lib.rs').write_text(LIB.format(name=name, body=body), encoding='utf-8')
    return f'Made {folder}: Cargo.toml, plugin.yml, src/lib.rs. Build it with `build {folder}`.'


# The usual mistakes and their fix, by a piece of the compiler's message (filled from what AIs got wrong).
HINTS = [
    (r'save_default_config', 'save_default_config takes the default YAML as text: plugin.save_default_config(include_str!("../config.yml")) with config.yml beside plugin.yml.'),
    (r'cannot find (type|function|macro) `(\w+)` in', 'Use `use horizon_bukkit::prelude::*;` (every event and the common types), or `lookup` the Java name to find its module.'),
    (r'expected `&str`, found `String`', 'Pass a String as &str with &name (or name.as_str()).'),
    (r'expected `String`, found `&str`', 'Make an owned String with .to_string() or .to_owned().'),
    (r'closure may outlive|borrowed value does not live long enough|`plugin` does not live long enough', 'Plugin is Copy: move it into the closure (`move |...|`); clone other values before the closure and move the clone in.'),
    (r'cannot borrow .* as mutable', 'Events arrive as `&mut E`: write `|event: &mut E|`. Shared state across calls goes in a static Mutex (`static COUNTS: Mutex<...> = Mutex::new(...)`) or an Arc<Mutex<...>> moved into the closures.'),
    (r'the trait bound .*Send.* is not satisfied|cannot be sent between threads', 'Listeners, executors and tasks must be Send + Sync: hold shared data in Arc<Mutex<..>>/static Mutex, not Rc/RefCell.'),
    (r'expected `bool`, found `\(\)`', 'A command executor returns bool: end it with `true` (false prints its usage).'),
    (r'no method named `(\w+)` found', 'That method has another name or lives elsewhere: `lookup` its Java name (`lookup Player.getHealth`).'),
]


def build(folder):
    """Builds the plugin into its DLL with the shared target (the SDK is built once for every plugin)."""
    folder = pathlib.Path(folder)
    env = dict(os.environ, CARGO_TARGET_DIR=str(TARGET), CARGO_TERM_COLOR='never')
    done = subprocess.run(['cargo', TOOLCHAIN, 'build', '--release', '--message-format', 'short'], cwd=folder, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace')
    crate = re.search(r'^name\s*=\s*"([^"]+)"', (folder / 'Cargo.toml').read_text(encoding='utf-8'), re.M).group(1).replace('-', '_')
    dll = TARGET / 'release' / f'{crate}.dll'
    errors = [l for l in done.stderr.splitlines() if ': error' in l or l.startswith('error')]
    if done.returncode == 0:
        warnings = len([l for l in done.stderr.splitlines() if ': warning' in l])
        return f'OK: {dll} ({warnings} warnings)'
    out = ['FAILED: ' + str(len(errors)) + ' errors'] + errors[:15]
    hints = []
    for pattern, hint in HINTS:
        if re.search(pattern, done.stderr) and hint not in hints:
            hints.append(hint)
    if hints:
        out += ['', 'Usual fixes:'] + [f'- {h}' for h in hints]
    return '\n'.join(out)


def test(folder, command=''):
    """Starts the plugin on a scratch server with a player and runs `/cmd` and `/cmd help` for each command."""
    folder = pathlib.Path(folder)
    built = build(folder)
    if not built.startswith('OK'):
        return built
    dll = built.split(': ', 1)[1].split(' (', 1)[0]
    yml = (folder / 'plugin.yml').read_text(encoding='utf-8')
    name = re.search(r'^name:\s*["\']?([^\s"\']+)', yml, re.M).group(1)
    commands = [c.strip().lstrip('/') for c in command.split(',') if c.strip()] or re.findall(r'^\s{2}([\w-]+):\s*$', yml.split('commands:', 1)[1], re.M) if 'commands:' in yml else []
    listing = TARGET / f'test-{name}.json'
    listing.write_text(json.dumps([{'name': name, 'dll': dll, 'commands': commands[:6], 'dlls': []}]), encoding='utf-8')
    env = dict(os.environ, PARITY_RUN=f'dev-{name.lower()}', PARITY_PORT=os.environ.get('HORIZON_PLUGIN_TEST_PORT', '25731'))
    done = subprocess.run([sys.executable, '-I', '-u', str(CHECK), str(listing), '--engine', str(ENGINE)], env=env, capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=600)
    lines = [l for l in done.stdout.splitlines() if l.startswith(('OK', 'FAIL')) or 'plugins enable' in l or 'did not start' in l]
    return '\n'.join(lines) or done.stdout[-1500:] + done.stderr[-800:]


def deploy(folder, server):
    """Copies the plugin's DLL into SERVER/plugins: a running server swaps it within two seconds (hot reload)."""
    built = build(folder)
    if not built.startswith('OK'):
        return built
    dll = pathlib.Path(built.split(': ', 1)[1].split(' (', 1)[0])
    plugins = pathlib.Path(server) / 'plugins'
    plugins.mkdir(parents=True, exist_ok=True)
    shutil.copy2(dll, plugins / dll.name)
    return f'Copied {dll.name} to {plugins}: a running server swaps it in at the next look (about a second).'


TOOLS = {
    'api_lookup': (lookup, 'The horizon-bukkit Rust (path and signature) for a Bukkit/Paper name: Player.setHealth, setHealth, Player, getConfig.', {'java': 'string'}),
    'sdk_search': (search, 'SDK functions whose Java name, Rust name or signature hold every word ("teleport", "boss bar", "inventory click").', {'words': 'string'}),
    'event_info': (event, 'An event: how to listen to it (register_event), whether it can be cancelled, everything it has.', {'name': 'string'}),
    'example': (example, 'Tested example plugins: no topic lists them, a topic ("gui", "config", "timer") gives the best match\'s files.', {'topic': 'string'}),
    'new_plugin': (new, 'Make a plugin crate (Cargo.toml, plugin.yml, src/lib.rs) in `folder`; `command` is a comma list of commands.', {'name': 'string', 'folder': 'string', 'command': 'string'}),
    'build_plugin': (build, 'Build a plugin crate: OK and the DLL, or the errors with the usual fix for each kind.', {'folder': 'string'}),
    'test_plugin': (test, 'Build and start the plugin on a scratch server with a player, run /cmd and /cmd help for its commands: says whether it enables and works.', {'folder': 'string', 'command': 'string'}),
    'deploy_plugin': (deploy, 'Build and copy the DLL into a server folder\'s plugins/: a running server hot-reloads it.', {'folder': 'string', 'server': 'string'}),
}


def mcp():
    out = open(os.dup(1), 'w', encoding='utf-8', newline='\n', buffering=1)
    os.dup2(2, 1)

    def send(message):
        out.write(json.dumps(message) + '\n')
        out.flush()

    for raw in sys.stdin:
        try:
            request = json.loads(raw)
        except json.JSONDecodeError:
            continue
        method, rid = request.get('method'), request.get('id')
        if method == 'initialize':
            send({'jsonrpc': '2.0', 'id': rid, 'result': {'protocolVersion': request['params'].get('protocolVersion', '2024-11-05'), 'capabilities': {'tools': {}}, 'serverInfo': {'name': 'horizon-plugin-dev', 'version': '1.0.0'}}})
        elif method == 'tools/list':
            send({'jsonrpc': '2.0', 'id': rid, 'result': {'tools': [{'name': n, 'description': d, 'inputSchema': {'type': 'object', 'properties': {k: {'type': t} for k, t in f.items()}}} for n, (_, d, f) in TOOLS.items()]}})
        elif method == 'tools/call':
            name, args = request['params']['name'], request['params'].get('arguments') or {}
            try:
                text = TOOLS[name][0](**args)
                send({'jsonrpc': '2.0', 'id': rid, 'result': {'content': [{'type': 'text', 'text': text}]}})
            except Exception as error:
                send({'jsonrpc': '2.0', 'id': rid, 'result': {'content': [{'type': 'text', 'text': f'{type(error).__name__}: {error}'}], 'isError': True}})
        elif method == 'ping':
            send({'jsonrpc': '2.0', 'id': rid, 'result': {}})
        elif rid is not None:
            send({'jsonrpc': '2.0', 'id': rid, 'error': {'code': -32601, 'message': f'no method {method}'}})


def main():
    args = sys.argv[1:]
    if not args or args[0] in ('-h', '--help'):
        print(__doc__)
        return
    if args[0] == '--mcp':
        mcp()
        return
    opt = lambda flag: args[args.index(flag) + 1] if flag in args else ''
    plain = [a for i, a in enumerate(args) if not a.startswith('--') and (i == 0 or not args[i - 1].startswith('--'))]
    word = plain[0]
    if word == 'lookup':
        print(lookup(' '.join(plain[1:])))
    elif word == 'search':
        print(search(' '.join(plain[1:])))
    elif word == 'event':
        print(event(plain[1]))
    elif word == 'example':
        print(example(' '.join(plain[1:])))
    elif word == 'new':
        print(new(plain[1], plain[2], opt('--command')))
    elif word == 'build':
        print(build(plain[1]))
    elif word == 'test':
        print(test(plain[1], opt('--command')))
    elif word == 'deploy':
        print(deploy(plain[1], plain[2]))
    else:
        print(__doc__)


if __name__ == '__main__':
    main()
