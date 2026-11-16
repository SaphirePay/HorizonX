"""Making Horizon mods: a new mod, its build into an .hmod, a try in a real single-player game, an install into the
launcher.

A mod is a plugin crate (horizon-bukkit + horizon-kit) whose DLL rides inside an .hmod beside its assets and data:

    horizon-mod.toml            id, name, version, side (the .hmod's manifest)
    plugin.yml                  the code's name, version and commands (compiled into the DLL)
    assets/<id>/...             textures, models, lang (key names), sounds
    data/<id>/...               recipes, loot, tags, Horizon blocks and items (data/<id>/horizon/block/*.json)
    src/lib.rs                  the code: events, commands, timers, and horizon_kit::client for the player's window
    tests/try.json              what mod_try does in the game when no steps are given

The mod engine (<workspace>/mods-engine: the Paper engine + the client bundle) loads the DLL from the .hmod in single
player and on a LAN host, with no game build. `.hmod` files go in a game folder's `horizon-mods/` and are switched on
in its `horizon-mods.txt` (`mod.<id>:true`).
"""
import workspace  # noqa: E402
import json
import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import time
import zipfile

HORIZON = workspace.WORKSPACE
MODS = HORIZON / 'mod-ports'
SDK = HORIZON / 'paper-engine' / 'sdk' / 'horizon-bukkit'
KIT = HORIZON / 'plugin-tools' / 'kit' / 'horizon-kit'
TARGET = MODS / 'target-mods'
ENGINES = [HORIZON / 'mods-engine' / 'out' / 'mods' / 'horizon-client.exe', HORIZON / 'mod-converter' / 'dist' / 'engine' / 'horizon-client.exe']
CTL = pathlib.Path(os.environ.get('HORIZON_CTL_EXE', str(workspace.at('src', 'target', 'release', 'horizon-ctl.exe'))))
APPDATA = pathlib.Path(os.environ.get('APPDATA', ''))
GAME_HOME = APPDATA / 'horizon'
CARGO_ENV = {**os.environ, 'CARGO_TERM_COLOR': 'never'}


def identifier(name):
    text = re.sub(r'[^a-z0-9]+', '_', name.lower()).strip('_')
    return text if text and not text[0].isdigit() else f'm_{text}'


def words(value):
    if not value:
        return []
    return list(value) if isinstance(value, list) else [w for w in re.split(r'[,\s]+', value) if w]


def engine():
    if os.environ.get('HORIZON_MOD_ENGINE'):
        return pathlib.Path(os.environ['HORIZON_MOD_ENGINE'])
    return next((e for e in ENGINES if e.is_file()), ENGINES[0])


def resources():
    if os.environ.get('MODCONV_RESOURCES'):
        return pathlib.Path(os.environ['MODCONV_RESOURCES'])
    versions = APPDATA / 'horizon-launcher' / 'versions'
    found = [v / '.local' / 'horizon-client' / '26.3' for v in versions.iterdir() if (v / '.local' / 'horizon-client' / '26.3' / 'assets').is_dir()]
    return max(found, key=lambda p: p.stat().st_mtime)


def crate(id):
    folder = MODS / id
    return folder if (folder / 'horizon-mod.toml').is_file() and (folder / 'Cargo.toml').is_file() else None


# ---- mod_new

CARGO = '''[package]
name = "{id}"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
horizon-bukkit = {{ path = "{sdk}" }}
horizon-kit = {{ path = "{kit}" }}
serde_json = "1"

[profile.release]
panic = "unwind"

[workspace]
'''

LIB = '''//! {name}: {description}
//!
//! Runs in single player (and for everyone on a LAN host). Events and commands are Bukkit's (`horizon_bukkit`),
//! the player's window is `client` (HUD, keys, crosshair, outlines). Build and try: `mod_build {id}`, `mod_try {id}`.
use horizon_kit::prelude::*;
use std::sync::Mutex;
use std::sync::atomic::{{AtomicBool, Ordering}};

const ID: &str = "{id}";
/// On or off, flipped by the mod's key.
static ON: AtomicBool = AtomicBool::new(true);
static KEY: Mutex<Option<client::Key>> = Mutex::new(None);
{command_fns}
fn on_enable(plugin: Plugin) {{
{commands}    // The key shows in Controls; its name comes from assets/{id}/lang/en_us.json.
    *KEY.lock().unwrap() = Some(client::Key::register("key.{id}.toggle", "{key}", "key.categories.{id}"));
    // Every tick: the key, then the HUD (sent only when it changes).
    plugin.run_task_timer(1, 1, || {{
        if KEY.lock().unwrap().as_ref().is_some_and(|k| k.clicked()) {{
            ON.fetch_xor(true, Ordering::Relaxed);
        }}
        let on = ON.load(Ordering::Relaxed);
        client::Hud::new()
            .fill(4.0, 4.0, 86.0, 12.0, 0x80000000)
            .text(7.0, 6.0, &format!("{name}: {{}}", if on {{ "§aON" }} else {{ "§cOFF" }}), 0xFFFFFFFF)
            .show(ID);
    }});
    plugin.register_event(|event: &mut PlayerJoinEvent| {{
        event.get_player().send_message("{name} is on: press {key_label} to switch it");
    }});
}}

fn on_disable(_plugin: Plugin) {{
    client::clear(ID);
}}

bukkit_plugin!(on_enable, on_disable);
'''


def mod_new(name, description='', commands='', key='key.keyboard.k', side='both'):
    """Start a new mod: a crate that builds into an .hmod and runs as it stands (a key, a HUD line, a join message)."""
    id_ = identifier(name)
    out = MODS / id_
    if out.exists():
        return {'error': f'{out} is there already'}
    for sub in ('src', 'tests', f'assets/{id_}/lang', f'data/{id_}'):
        (out / sub).mkdir(parents=True, exist_ok=True)
    description = description or f'{name}, a Horizon mod.'
    (out / 'Cargo.toml').write_text(CARGO.format(id=id_, sdk=SDK.as_posix(), kit=KIT.as_posix()), encoding='utf-8')
    names = words(commands)
    yaml = [f'name: {name}', f'main: horizon.{id_}', 'version: 0.1.0', "api-version: '1.21'", f'description: {json.dumps(description)}']
    if names:
        yaml.append('commands:')
        for command in names:
            yaml += [f'  {command}:', f'    description: /{command}', f'    usage: /{command}']
    (out / 'plugin.yml').write_text('\n'.join(yaml) + '\n', encoding='utf-8')
    manifest = [f'id = "{id_}"', f'name = {json.dumps(name)}', 'version = "0.1.0"', f'description = {json.dumps(description)}', 'authors = []',
                'api = "0.2"', f'side = "{side}"', 'capabilities = ["registries"]']
    (out / 'horizon-mod.toml').write_text('\n'.join(manifest) + '\n', encoding='utf-8')
    (out / f'assets/{id_}/lang/en_us.json').write_text(json.dumps({f'key.{id_}.toggle': f'Toggle {name}', f'key.categories.{id_}': name}, indent=2) + '\n', encoding='utf-8')
    command_fns = ''.join(f'\nfn {identifier(c)}(sender: &CommandSender, _label: &str, _args: &[&str]) -> bool {{\n    sender.send_message("/{c} works");\n    true\n}}\n' for c in names)
    command_lines = ''.join(f'    plugin.set_executor("{c}", {identifier(c)});\n' for c in names)
    label = key.rsplit('.', 1)[-1].upper()
    (out / 'src' / 'lib.rs').write_text(LIB.format(id=id_, name=name, description=description, key=key, key_label=label, command_fns=command_fns, commands=command_lines), encoding='utf-8')
    try_steps = [{'ctl': 'wait-ready'}, {'log': f'{id_}'}] + [{'ctl': f'chat /{c}', 'expect_chat': f'/{c} works'} for c in names]
    (out / 'tests' / 'try.json').write_text(json.dumps(try_steps, indent=2) + '\n', encoding='utf-8')
    (out / '.gitignore').write_text('/target\n*.hmod\n', encoding='utf-8')
    return {'id': id_, 'crate': str(out), 'next': f'write src/lib.rs (kit_doc client for the window), mod_build {id_}, then mod_try {id_}'}


# ---- mod_build

def pack(folder, dll, dest):
    """The .hmod: manifest, assets/, data/, the DLL under native/windows-x64/. Written beside, then renamed."""
    id_ = folder.name
    partial = dest.with_suffix('.hmod.partial')
    with zipfile.ZipFile(partial, 'w', zipfile.ZIP_DEFLATED) as z:
        z.write(folder / 'horizon-mod.toml', 'horizon-mod.toml')
        for top in ('assets', 'data'):
            for path in sorted((folder / top).rglob('*')):
                if path.is_file():
                    z.write(path, path.relative_to(folder).as_posix())
        z.write(dll, f'native/windows-x64/{id_}.dll')
    os.replace(partial, dest)
    return dest


def mod_build(id):
    """Build a mod's DLL and pack its .hmod (manifest, assets, data, DLL): seconds, no game build."""
    folder = crate(id)
    if not folder:
        return {'error': f'no mod crate {id} in {MODS} (needs horizon-mod.toml and Cargo.toml)'}
    begin = time.time()
    done = subprocess.run(['cargo', '+1.99.1', 'build', '--release', '--message-format=short'], cwd=folder, capture_output=True, text=True,
                          encoding='utf-8', errors='replace', env={**CARGO_ENV, 'CARGO_TARGET_DIR': str(TARGET)}, timeout=900)
    errors = [line for line in done.stderr.splitlines() if ': error' in line or line.startswith('error')]
    warnings = [line for line in done.stderr.splitlines() if ': warning' in line and line.startswith('src')]
    if done.returncode:
        return {'ok': False, 'seconds': round(time.time() - begin, 1), 'errors': errors[:40] or done.stderr.splitlines()[-20:]}
    dll = TARGET / 'release' / f'{id}.dll'
    hmod = pack(folder, dll, folder / f'{id}.hmod')
    return {'ok': True, 'seconds': round(time.time() - begin, 1), 'hmod': str(hmod), 'mb': round(hmod.stat().st_size / 1e6, 1), 'warnings': warnings[:15]}


# ---- mod_try

def ctl(socket, args, timeout=60):
    done = subprocess.run([str(CTL), '--socket', str(socket), *args], capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=timeout)
    return (done.stdout + done.stderr).strip()


def steps_of(steps, folder):
    """Steps from a string (`;` or new lines: a horizon-ctl line, `line => expected text`, `wait 2`, `screenshot`),
    a list, or the crate's tests/try.json."""
    if not steps:
        path = folder / 'tests' / 'try.json'
        return json.loads(path.read_text(encoding='utf-8')) if path.is_file() else []
    if isinstance(steps, list):
        return steps
    out = []
    for line in (s.strip() for s in re.split(r'[\n;]+', steps)):
        if not line:
            continue
        if line.startswith('wait ') and re.fullmatch(r'wait [\d.]+', line):
            out.append({'wait': float(line.split()[1])})
        elif line == 'screenshot':
            out.append({'screenshot': True})
        elif '=>' in line:
            command, expect = (p.strip() for p in line.split('=>', 1))
            out.append({'ctl': command, 'expect': expect})
        else:
            out.append({'ctl': line})
    return out


def mod_try(id, steps='', screenshot=True, keep=False, seconds=0.5):
    """Play a mod in a real single-player game (offscreen, its own temporary game folder): the .hmod switched on as a
    player has it, a fresh world, then the steps. Gives each step's reply, the mod's log lines, trouble and screenshots.
    Steps: horizon-ctl lines split by `;`/new lines (`chat /x`, `key k`, `break {x} {y-1} {z}`, `command give @s
    diamond_axe`); `line => text` must answer with the text; `wait 2`; `screenshot`. `{x}`/`{y+1}`: the player's block
    position. No steps: tests/try.json."""
    folder = crate(id)
    hmod = folder / f'{id}.hmod' if folder else None
    if not hmod or not hmod.is_file():
        return {'error': f'no {id}.hmod: run mod_build {id} first'}
    game_exe = engine()
    if not game_exe.is_file():
        return {'error': f'no mod engine at {game_exe}'}
    plan = steps_of(steps, folder)
    work = pathlib.Path(tempfile.mkdtemp(prefix=f'modtry-{id}-'))
    game, home = work / 'game', work / 'home'
    (game / 'horizon-mods').mkdir(parents=True)
    home.mkdir()
    shutil.copy(hmod, game / 'horizon-mods' / hmod.name)
    (game / 'horizon-mods.txt').write_text(f'mod.{id}:true\n', encoding='utf-8')
    socket = work / 'control.json'
    begin = time.time()
    log_file = work / 'game.out'
    with log_file.open('wb') as out:
        proc = subprocess.Popen([str(game_exe), '--resources', str(resources()), '--offscreen', '--width', '1280', '--height', '720', '--world', 'modtry',
                                 '--seed', '1104', '--control-socket', str(socket), '--timeout-seconds', '300', '--game-dir', str(game), '--home', str(home)],
                                stdout=out, stderr=subprocess.STDOUT)
    results, pictures, failed = [], [], []
    try:
        for _ in range(120):
            if socket.exists():
                break
            time.sleep(1)
        ctl(socket, ['wait-ready'], timeout=150)
        state = json.loads(ctl(socket, ['state']) or '{}')
        pos = state.get('block_pos') or state.get('player', {}).get('block_pos') or [0, 64, 0]
        at = dict(zip('xyz', pos))
        ready_s = round(time.time() - begin, 1)
        for step in plan:
            if 'wait' in step:
                time.sleep(float(step['wait']))
                continue
            if step.get('screenshot'):
                picture = work / f'shot{len(pictures) + 1}.png'
                ctl(socket, ['screenshot', str(picture)], timeout=20)
                pictures.append(picture)
                continue
            if 'log' in step or 'ctl' not in step:
                continue
            line = re.sub(r'\{([xyz])([+-]\d+)?\}', lambda m: str(at[m.group(1)] + int(m.group(2) or 0)), step['ctl'])
            reply = ctl(socket, line.split(), timeout=step.get('timeout', 60))
            ok = ('expect' not in step or step['expect'] in reply) and ('expect_not' not in step or step['expect_not'] not in reply)
            if 'expect_chat' in step:
                time.sleep(0.5)
                ok = ok and step['expect_chat'] in ctl(socket, ['messages'])
            results.append(f'{"ok " if ok else "BAD"} {line}  ->  {reply[:240]}')
            if not ok:
                failed.append(line)
            time.sleep(seconds)
        if screenshot:
            picture = work / 'last.png'
            ctl(socket, ['screenshot', str(picture)], timeout=20)
            pictures.append(picture)
    finally:
        try:
            ctl(socket, ['quit'], timeout=20)
        except Exception:
            pass
        try:
            proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            proc.kill()
    log = log_file.read_text(encoding='utf-8', errors='replace')
    latest = home / 'logs' / 'latest.log'
    if latest.is_file():
        log += latest.read_text(encoding='utf-8', errors='replace')
    for step in plan:
        if 'log' in step:
            held = step['log'] in log
            results.append(f'{"ok " if held else "BAD"} log has {step["log"]!r}')
            if not held:
                failed.append('log ' + step['log'])
    # The mod logs as its plugin.yml name (`[Tree_Timber]`), the engine by its id: either, any case, `_`/`-`/space.
    names = {id.lower(), id.lower().replace('_', '-'), id.lower().replace('_', ' ')}
    mod_lines = [l[:220] for l in log.splitlines() if any(n in l.lower() for n in names)][-30:]
    trouble = [l[:220] for l in log.splitlines() if re.search(r'panic|ERROR|SEVERE|failed to load|could not load|refused', l) and not re.search(r'telemetry|realms|ping', l, re.I)][-20:]
    shots = HORIZON / 'mod-ports' / '_tries'
    shots.mkdir(exist_ok=True)
    kept = []
    for picture in pictures:
        if picture.is_file():
            dest = shots / f'{id}-{picture.name}'
            shutil.copy(picture, dest)
            kept.append(str(dest))
    if keep:
        (shots / f'{id}.log').write_text(log, encoding='utf-8')
    shutil.rmtree(work, ignore_errors=True)
    return {'pass': not failed and not any('panic' in t for t in trouble), 'world_ready_s': ready_s if 'ready_s' in dir() else None,
            'seconds': round(time.time() - begin, 1), 'steps': results, 'mod_log': mod_lines, 'trouble': trouble, 'screenshots': kept,
            'engine': str(game_exe)}


# ---- mod_install

def instance_dir(instance):
    if instance in ('', 'horizon', 'default'):
        return GAME_HOME
    return GAME_HOME / 'instances' / instance


def mod_install(id, instance='mods', on=True):
    """Put a mod's .hmod in a launcher instance's game folder (`horizon-mods/`) and switch it on (or off)."""
    folder = crate(id)
    hmod = folder / f'{id}.hmod' if folder else None
    if not hmod or not hmod.is_file():
        return {'error': f'no {id}.hmod: run mod_build {id} first'}
    game = instance_dir(instance)
    if not game.is_dir():
        return {'error': f'no game folder {game}: create the instance first'}
    (game / 'horizon-mods').mkdir(exist_ok=True)
    shutil.copy(hmod, game / 'horizon-mods' / hmod.name)
    switches = game / 'horizon-mods.txt'
    lines = [l for l in (switches.read_text(encoding='utf-8').splitlines() if switches.is_file() else []) if not l.startswith(f'mod.{id}:')]
    lines.append(f'mod.{id}:{"true" if on else "false"}')
    switches.write_text('\n'.join(lines) + '\n', encoding='utf-8')
    return {'installed': str(game / 'horizon-mods' / hmod.name), 'on': on, 'switches': str(switches), 'note': 'start (or restart) the game: mods load when it starts'}


def mod_list():
    """The mod crates in mod-ports: id, whether an .hmod is built, its size."""
    rows = []
    for folder in sorted(MODS.iterdir()):
        if (folder / 'horizon-mod.toml').is_file() and (folder / 'Cargo.toml').is_file():
            hmod = folder / f'{folder.name}.hmod'
            rows.append({'id': folder.name, 'hmod': hmod.is_file(), 'mb': round(hmod.stat().st_size / 1e6, 1) if hmod.is_file() else None})
    return rows


TOOLS = {
    'mod_new': (mod_new, 'Start a new mod (or a rewrite of one): a crate that builds into an .hmod with its code inside and runs in single player as it stands (key, HUD line, join message). Write src/lib.rs with horizon-kit (kit_doc client: HUD, keys, crosshair, outlines) and horizon-bukkit events.', {'name': 'string', 'description': 'string', 'commands': 'string', 'key': 'string', 'side': 'string'}),
    'mod_build': (mod_build, "Build a mod's DLL and pack its .hmod (manifest, assets, data, DLL): seconds, no game build.", {'id': 'string'}),
    'mod_try': (mod_try, 'Play a mod in a real single-player game (offscreen): the .hmod on, a fresh world, then the steps (horizon-ctl lines split by ;, `line => expected`, `wait 2`, `screenshot`, {x} {y-1} {z} = player position). Gives replies, mod log, trouble, screenshots.', {'id': 'string', 'steps': 'string', 'screenshot': 'boolean', 'keep': 'boolean', 'seconds': 'number'}),
    'mod_install': (mod_install, "Put a mod's .hmod into a launcher instance (default: the Mods instance) and switch it on.", {'id': 'string', 'instance': 'string', 'on': 'boolean'}),
    'mod_list': (mod_list, 'The mods in mod-ports and whether each has a built .hmod.', {}),
}
