"""One call from a plugin's name to a Rust plugin on a server: find the jar (a path, a URL, or a
Modrinth search), convert it with jport, build it, start it once on a scratch server to see it enable,
and put it on a launcher server. `plugin_mcp.py` serves it as `plugin_port`."""
import workspace  # noqa: E402
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time
import urllib.parse
import urllib.request

HORIZON = workspace.WORKSPACE
TOOLS = HORIZON / 'plugin-tools'
JPORT = TOOLS / 'jport'
PORTS = HORIZON / 'plugin-ports'
TARGET = PORTS / 'target'
DOWNLOADS = HORIZON / 'plugin-downloads'
LAUNCHER = pathlib.Path(os.environ['APPDATA']) / 'horizon-launcher'
SERVER = LAUNCHER / 'server-runtime' / 'current' / 'horizon-server.exe'
# The engine the launcher's Paper servers run: a plugin is checked on what it will run on.
ENGINE = pathlib.Path(os.environ['PARITY_ENGINE']) if os.environ.get('PARITY_ENGINE') else LAUNCHER / 'paper' / 'engine' / 'horizon-client.exe'
# Where a server takes plugin libraries: `mods` on the Fabric engine, `plugins` on the Paper engine (<workspace>/paper-engine).
PLUGIN_FOLDER = os.environ.get('PARITY_PLUGIN_FOLDER', 'mods')
# What says a server is up with its plugins on.
READY = ('server started',) if PLUGIN_FOLDER == 'plugins' else ('server started', 'Done (')
RESOURCES = LAUNCHER / 'server-runtime' / 'current' / '.local' / 'horizon-client' / '26.3'
MODRINTH = 'https://api.modrinth.com/v2'
AGENT = {'User-Agent': 'horizon-plugin-port/0.3'}
LOADERS = ('paper', 'spigot', 'bukkit', 'purpur', 'folia')
SMOKE_PORT = 25651
# Plugins with a Paper, Spigot or Bukkit build: Modrinth's "plugin" type alone also lists Fabric mods.
PLUGIN_FACETS = urllib.parse.quote(json.dumps([['project_type:plugin'], ['categories:paper', 'categories:spigot', 'categories:bukkit', 'categories:purpur']]))


def identifier(name):
    text = re.sub(r'[^a-z0-9]+', '_', name.lower()).strip('_')
    return text if text and not text[0].isdigit() else f'p_{text}'


def get_json(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers=AGENT), timeout=60) as answer:
        return json.load(answer)


def download(url, folder):
    folder.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(urllib.request.Request(url, headers=AGENT), timeout=180) as answer:
        name = pathlib.PurePosixPath(urllib.parse.unquote(urllib.parse.urlparse(answer.url).path)).name
        out = folder / (name if name.endswith('.jar') else 'plugin.jar')
        out.write_bytes(answer.read())
    if out.read_bytes()[:2] != b'PK':
        out.unlink()
        raise ValueError(f'{url} gave a web page, not a jar: give the direct download link')
    return out


def modrinth_slug(slug, folder_root=None):
    """The newest Paper, Spigot or Bukkit jar of one Modrinth project, by its exact slug."""
    loaders = urllib.parse.quote(json.dumps(list(LOADERS)))
    for found in get_json(f'{MODRINTH}/project/{slug}/version?loaders={loaders}'):
        jars = [f for f in found['files'] if f['filename'].endswith('.jar')]
        if jars:
            file = next((f for f in jars if f.get('primary')), jars[0])
            folder = (folder_root or DOWNLOADS) / slug / found['version_number'].replace('/', '_')
            existing = list(folder.glob('*.jar')) if folder.is_dir() else []
            return (existing[0] if existing else download(file['url'], folder)), found['version_number']
    raise LookupError(f'{slug} has no Paper, Spigot or Bukkit jar')


def from_modrinth(name, version=''):
    """The newest Paper, Spigot or Bukkit build of a Modrinth plugin, by its name or slug."""
    hits = get_json(f'{MODRINTH}/search?query={urllib.parse.quote(name)}&facets={PLUGIN_FACETS}&limit=8')['hits']
    if not hits:
        raise LookupError(f'no plugin called {name!r} on Modrinth: give a path or a download link')
    wanted = name.lower().replace(' ', '')
    project = next((h for h in hits if wanted in (h['slug'].lower(), h['title'].lower().replace(' ', ''))), hits[0])
    loaders = urllib.parse.quote(json.dumps(list(LOADERS)))
    versions = get_json(f'{MODRINTH}/project/{project["slug"]}/version?loaders={loaders}')
    if version:
        versions = [v for v in versions if v['version_number'] == version] or versions
    for found in versions:
        jars = [f for f in found['files'] if f['filename'].endswith('.jar')]
        if jars:
            file = next((f for f in jars if f.get('primary')), jars[0])
            folder = DOWNLOADS / project['slug'] / found['version_number']
            shutil.rmtree(folder, ignore_errors=True)
            jar = download(file['url'], folder)
            return jar, {'from': 'Modrinth', 'project': project['title'], 'version': found['version_number'], 'page': f'https://modrinth.com/plugin/{project["slug"]}', 'license': project.get('license', '')}
    raise LookupError(f'{project["title"]} has no Paper, Spigot or Bukkit jar on Modrinth')


def find_jar(source, version=''):
    if re.match(r'https?://', source):
        return download(source, DOWNLOADS / f'url_{int(time.time())}'), {'from': source}
    path = pathlib.Path(source.strip('"'))
    if path.suffix == '.jar' and path.is_file():
        return path, {'from': str(path)}
    return from_modrinth(source, version)


def jar_name(jar):
    """The name in the jar's own plugin.yml, which names the crate and the DLL."""
    import zipfile
    with zipfile.ZipFile(jar) as archive:
        for entry in ('plugin.yml', 'paper-plugin.yml'):
            if entry in archive.namelist():
                found = re.search(r'^[ \t]*name:\s*["\']?(.+?)["\']?\s*$', archive.read(entry).decode('utf-8', 'replace'), re.M)
                if found:
                    return found.group(1)
    return re.sub(r'[-_.]?\d.*$', '', jar.stem) or jar.stem


def yml(crate, key):
    found = re.search(rf'^[ \t]*{key}:\s*["\']?(.+?)["\']?\s*$', (crate / 'plugin.yml').read_text(encoding='utf-8', errors='replace'), re.M)
    return found.group(1) if found else ''


def quiet(stage, state, text=''):
    """Where each step reports itself: the app shows these, a plain call drops them."""


def stream(command, cwd, env, on_line, timeout):
    """Runs a command and hands each line it prints to `on_line` as it comes. Gives (exit code, all lines)."""
    process = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding='utf-8', errors='replace', creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
    lines, limit = [], time.time() + timeout
    for line in process.stdout:
        line = line.rstrip('\r\n')
        lines.append(line)
        on_line(line)
        if time.time() > limit:
            process.kill()
            raise TimeoutError(f'{command[0]} ran past {timeout} s')
    return process.wait(), lines


# The converter's compile rounds are real builds by default (the maintainer's JPORT_CHECK_AS_BUILD; JPORT_CHECK_AS_BUILD=0
# turns it off): the round that passes is the test build. The parts of a split plugin build in parallel, so with
# several lanes at once each cargo keeps to 4 jobs unless CARGO_BUILD_JOBS says otherwise.
os.environ.setdefault('JPORT_CHECK_AS_BUILD', '1')
if os.environ['JPORT_CHECK_AS_BUILD'] == '0':
    del os.environ['JPORT_CHECK_AS_BUILD']
os.environ.setdefault('CARGO_BUILD_JOBS', '4')


class Lane:
    """What one worker uses alone, so several plugins convert at once without sharing a build folder or a port:
    its own cargo target for the compile check and the build, a scratch-server port, and a Paper test pair."""

    def __init__(self, number=0):
        self.number = number
        self.target = PORTS / f'target_lane{number}'
        # JPORT_CHECK_AS_BUILD=1: the converter's compile rounds are real builds into this lane's own target, so
        # the round that passes is the test build (`build(profile='test')` then finds it up to date).
        self.check_target = self.target if os.environ.get('JPORT_CHECK_AS_BUILD') else JPORT / f'target_lane{number}'
        self.smoke_port = SMOKE_PORT + number * 10
        self.parity_run = f'lane{number}'
        self.parity_port = 25711 + number * 10


def convert(jar, id_, log=quiet, lane=None):
    out = PORTS / id_
    # Only a crate the converter wrote is the converter's to replace: plugin-ports also holds hand ports.
    # The converter's own: its report, or (after a stop half-way) its `.jar` working folder. A hand port has neither.
    if out.exists() and not (out / 'jport-report.json').is_file() and not (out / '.jar').is_dir():
        raise FileExistsError(f'{out} is a hand-written port: pass another id')
    shutil.rmtree(out, ignore_errors=True)
    started = time.time()
    state = {'json': False}

    def seen(line):
        if line.startswith('decompiled in'):
            log('decompile', 'done', line)
            log('convert', 'running', 'Java to Rust, then a compile check')
        elif line.startswith('{'):
            state['json'] = True
        elif not state['json'] and line.strip():
            log('convert', 'line', line)

    log('decompile', 'running', f'Vineflower on {jar.name}')
    env = {**os.environ, 'RUST_MIN_STACK': RUSTC_STACK, **({'JPORT_TARGET': str(lane.check_target)} if lane else {})}
    code, lines = stream([str(JPORT / '.venv' / 'Scripts' / 'python.exe'), str(JPORT / 'jport.py'), 'jar', str(jar), str(out), '--id', id_], None, env, seen, 1800)
    start = next((i for i, line in enumerate(lines) if line.startswith('{')), None)
    if code or start is None:
        raise RuntimeError('the converter stopped: ' + '\n'.join(lines)[-900:])
    report = json.loads('\n'.join(lines[start:]))
    report['seconds'] = round(time.time() - started, 1)
    return out, report


# rustc's own stack: a big plugin's long methods overflowed the default (cdiscord, mrdiscordlink: 0xc00000fd).
RUSTC_STACK = os.environ.get('RUST_MIN_STACK', str(256 * 1024 * 1024))


# Measured on LuckPerms: level 0 builds in 38 s against 142 s, the same 2 GB peak, and the same behaviour (parity 0).
TEST_OPT = os.environ.get('PORT_TEST_OPT', '0')


def build(crate, id_, log=quiet, target=None, profile='release'):
    """The DLL, rustc spread over 4 threads. `release` is for a server people play on; `test` is for checking
    a port: less optimised, so the compiler needs far less memory and time, and it behaves the same."""
    started = time.time()
    target = pathlib.Path(target or TARGET)
    level, units = ('2', '256') if profile == 'release' else (TEST_OPT, '64')
    env = {**os.environ, 'RUSTC_BOOTSTRAP': '1', 'CARGO_TARGET_DIR': str(target), 'CARGO_TERM_COLOR': 'never', 'RUST_MIN_STACK': RUSTC_STACK,
           'CARGO_PROFILE_RELEASE_OPT_LEVEL': level, 'CARGO_PROFILE_RELEASE_CODEGEN_UNITS': units}
    def seen(line):
        if line.strip().startswith(('Compiling', 'Finished', 'error')):
            log('build', 'line', line.strip())

    dll = target / 'release' / f'{id_}.dll'
    # The converter's last compile round already built it (JPORT_CHECK_AS_BUILD, test profile, the same flags).
    lib = pathlib.Path(crate) / 'src' / 'lib.rs'
    if profile == 'test' and os.environ.get('JPORT_CHECK_AS_BUILD') and dll.is_file() and lib.is_file() and dll.stat().st_mtime >= lib.stat().st_mtime:
        log("build", "line", "built by the converter's last compile round")
        return {'ok': True, 'dll': str(dll), 'seconds': 0.0, 'errors': [], 'mb': round(dll.stat().st_size / 1e6, 1), 'reused': True}
    # JPORT_RUSTC_FLAGS: the converter's compile rounds use the same, so a check-as-build round is this build.
    flags = os.environ.get('JPORT_RUSTC_FLAGS', '-Zthreads=4').split()
    code, lines = stream(['cargo', '+1.99.1', 'rustc', '--lib', '--release', '--', *flags], crate, env, seen, 1500)
    errors = [line for line in lines if line.startswith('error')]
    return {'ok': code == 0 and dll.is_file(), 'dll': str(dll), 'seconds': round(time.time() - started, 1), 'errors': errors[:20], 'mb': round(dll.stat().st_size / 1e6, 1) if dll.is_file() else 0}


def server(folder, *words, timeout=90):
    done = subprocess.run([str(SERVER), '--dir', str(folder), *map(str, words)], capture_output=True, text=True, encoding='utf-8', errors='replace', stdin=subprocess.DEVNULL, timeout=timeout)
    return (done.stdout + done.stderr).strip()


def start_detached(folder):
    """`start` leaves the server running: its pipes must not be ours, or this call never ends."""
    flags = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP
    subprocess.Popen([str(SERVER), '--dir', str(folder), 'start'], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=flags, close_fds=True)


def wait_for(path, words, seconds):
    limit = time.time() + seconds
    while time.time() < limit:
        text = path.read_text(encoding='utf-8', errors='replace') if path.is_file() else ''
        if any(word in text for word in words):
            return text
        time.sleep(0.5)
    return path.read_text(encoding='utf-8', errors='replace') if path.is_file() else ''


TROUBLE = re.compile(r'SEVERE|panic|not converted|NoSuchMethodError|failed to load|could not load', re.I)


def plugin_row(row, name):
    """A line the plugin itself or the loader said of it: `[Name] ...` (its logger, Paper's Loading and Enabling lines),
    or the Paper engine's `[PluginLoader]` naming it. Never a path that merely holds the name (`smoke_name/.horizon`)."""
    return f'[{name}]' in row or ('[PluginLoader]' in row and re.search(rf"['\s:]{re.escape(name)}(?:['\s,]|$| v)", row) is not None)


def load_report(log, name, since=0):
    rows = log.splitlines()[since:]
    mine = [row for row in rows if plugin_row(row, name)]
    trouble = [row[-220:] for row in rows if TROUBLE.search(row) and (plugin_row(row, name) or 'panic' in row.lower())][:15]
    # The Paper engine says `[Name] Enabling Name v1` as onEnable runs, as Paper does; the Fabric engine had no such
    # line, so there any line of the plugin's counts.
    enabled_line = any(f'[{name}] Enabling ' in row for row in rows) if PLUGIN_FOLDER == 'plugins' else bool(mine)
    return {'lines': [row[-200:] for row in mine[-12:]], 'trouble': trouble, 'enabled_line': enabled_line}


WORLDS = HORIZON / 'parity' / 'world-cache'


def world_key(label):
    """A world for this engine build and seed: another build may write it otherwise."""
    stamp = ENGINE.stat().st_mtime_ns if ENGINE.is_file() else 0
    return WORLDS / f'{label}-{ENGINE.parent.name}-{stamp}'


def world_from_cache(world, label):
    """The saved world, copied in before the first start: true if there was one."""
    cached = world_key(label)
    if not (cached / 'level.dat').is_file():
        return False
    shutil.copytree(cached, world, dirs_exist_ok=True)
    return True


def world_to_cache(world, label):
    """The world a start just generated, kept for the next server of this engine and seed."""
    cached = world_key(label)
    if (world / 'level.dat').is_file() and not cached.exists():
        temporary = cached.with_name(cached.name + f'.{os.getpid()}')
        try:
            shutil.copytree(world, temporary, ignore=shutil.ignore_patterns('session.lock', '*.lock'))
            temporary.rename(cached)
        except OSError:
            shutil.rmtree(temporary, ignore_errors=True)


def free_port(preferred):
    """The lane's own port when nothing holds it, else one the system has free: a server of another
    session on the same port made a plugin read as "doesn't enable" (FAILED TO BIND TO PORT)."""
    import socket
    for port in (preferred, 0):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
            try:
                probe.bind(('0.0.0.0', port))
                return probe.getsockname()[1]
            except OSError:
                continue
    return preferred


def smoke(dll, name, port=SMOKE_PORT, extra=()):
    """Start the plugin once on a scratch server with nobody on: it loads and enables, or it says why not."""
    folder = HORIZON / 'parity' / f'smoke_{dll.stem}'
    shutil.rmtree(folder, ignore_errors=True)
    if folder.exists():
        # A file still held from a run before (Windows): a folder of its own instead of a crash.
        folder = folder.with_name(f'{folder.name}_{int(time.time())}')
    folder.mkdir(parents=True)
    port = free_port(int(port))
    server(folder, 'init', '--eula', '--world', 'smoke', '--seed', '12345', '--port', port, '--no-whitelist', '--offline', '--yes')
    server(folder, 'config', 'set', f'engine.path={ENGINE}', f'engine.resources={RESOURCES}', 'panel.bind=')
    made = world_from_cache(folder / 'smoke', 'smoke-12345')
    (folder / PLUGIN_FOLDER).mkdir(exist_ok=True)
    shutil.copy(dll, folder / PLUGIN_FOLDER)
    # The plugins it depends on, beside it (plugin-sweep/deps).
    for other in extra:
        shutil.copy(other, folder / PLUGIN_FOLDER)
    start_detached(folder)
    # The Paper engine enables its plugins after vanilla's `Done (...)!`: its own `server started` line comes after them.
    log = wait_for(folder / 'logs' / 'latest.log', READY, 150)
    time.sleep(3)
    log = (folder / 'logs' / 'latest.log').read_text(encoding='utf-8', errors='replace') if (folder / 'logs' / 'latest.log').is_file() else log
    server(folder, 'stop', timeout=60)
    if not made:
        world_to_cache(folder / 'smoke', 'smoke-12345')
    result = load_report(log, name)
    result['started'] = 'Done (' in log or 'server started' in log
    result['enabled'] = result['started'] and result['enabled_line'] and not result['trouble']
    if not result['started']:
        supervisor = folder / 'logs' / 'supervisor.log'
        result['server_said'] = supervisor.read_text(encoding='utf-8', errors='replace')[-700:] if supervisor.is_file() else 'no log at all'
    time.sleep(1)
    shutil.rmtree(folder, ignore_errors=True)
    return result


def server_folder(name):
    folder = pathlib.Path(name)
    if folder.is_dir():
        return folder
    found = LAUNCHER / 'servers' / name
    if found.is_dir():
        return found
    have = sorted(p.name for p in (LAUNCHER / 'servers').iterdir() if p.is_dir())
    raise LookupError(f'no server {name!r}: the launcher has {", ".join(have) or "none"}')


def twins_of(folder, dll, name):
    """Other DLLs on the server that are the same plugin: two copies fight over its commands and data folder."""
    return [other for other in (folder / PLUGIN_FOLDER).glob('*.dll') if other.name != dll.name and re.search(rb'name: ["\']?' + re.escape(name.encode()) + rb'["\']?\r?\n', other.read_bytes())]


def install_many(items, server_name, force=False):
    """Several plugins onto one server with one backup and one restart. `items` is [(dll, name)];
    gives {name: result}. Each is refused on its own (another copy installed) or all are (players on)."""
    folder = server_folder(server_name)
    try:
        online = json.loads(server(folder, 'players', '--json')).get('online', [])
    except (json.JSONDecodeError, subprocess.TimeoutExpired):
        online = []
    if online and not force:
        why = f'{", ".join(p.get("name", "?") if isinstance(p, dict) else str(p) for p in online)} on {folder.name}: ask them first, then pass force'
        return {name: {'installed': False, 'why': why} for _, name in items}
    (folder / PLUGIN_FOLDER).mkdir(exist_ok=True)
    results, going = {}, []
    for dll, name in items:
        twins = twins_of(folder, dll, name)
        if twins and not force:
            results[name] = {'installed': False, 'why': f'{folder.name} already runs {name} as {", ".join(t.name for t in twins)}: pass force to switch that one off and use this one'}
        else:
            going.append((dll, name, twins))
    if not going:
        return results
    running = 'Running' in server(folder, 'status')
    if running:
        server(folder, 'backup', 'before-' + '-'.join(dll.stem for dll, _, _ in going)[:60])
        server(folder, 'stop', timeout=90)
    for dll, name, twins in going:
        for twin in twins:
            # The launcher's own way of switching a plugin off: it shows as off, and on brings it back.
            twin.rename(twin.with_name(twin.name + '.disabled'))
        old = folder / PLUGIN_FOLDER / dll.name
        if old.is_file():
            shutil.copy2(old, folder / PLUGIN_FOLDER / f'{dll.name}.before-port')
        shutil.copy2(dll, old)
        results[name] = {'installed': True, 'server': folder.name, 'file': str(old), 'switched_off': [t.name for t in twins], 'restarted': running}
    if running:
        log_path = folder / 'logs' / 'latest.log'
        since = len(log_path.read_text(encoding='utf-8', errors='replace').splitlines()) if log_path.is_file() else 0
        start_detached(folder)
        time.sleep(4)
        wait_for(log_path, READY, 150)
        time.sleep(3)
        log = log_path.read_text(encoding='utf-8', errors='replace')
        since = since if len(log.splitlines()) > since else 0
        for _, name, _ in going:
            results[name]['load'] = load_report(log, name, since)
    else:
        for _, name, _ in going:
            results[name]['note'] = 'the server was stopped: it loads the plugin at its next start'
    return results


def install(dll, name, server_name, force=False):
    return install_many([(dll, name)], server_name, force)[name]


def plugin_port(source, server='', version='', id='', force=False, log=quiet, lane=None):
    """Name, path or link to a running Rust plugin. Each step says how it went."""
    out = {}
    log('source', 'running', source)
    jar, out['source'] = find_jar(source, version)
    out['source']['jar'] = str(jar)
    log('source', 'done', f'{jar.name} · {round(jar.stat().st_size / 1e6, 1)} MB')
    name = jar_name(jar)
    id_ = id or identifier(name) + '_auto'
    crate, report = convert(jar, id_, log, lane)
    name = yml(crate, 'name') or name
    out['plugin'] = {'name': name, 'version': yml(crate, 'version'), 'id': id_, 'crate': str(crate)}
    out['convert'] = {key: report.get(key) for key in ('classes', 'methods', 'translated_pct', 'ready_pct', 'methods_stubbed', 'bindings_dropped', 'top_missing', 'by_area', 'rust_lines', 'compiled', 'seconds')}
    if isinstance(out['convert'].get('top_missing'), list):
        out['convert']['top_missing'] = out['convert']['top_missing'][:12]
    if not report.get('compiled'):
        log('convert', 'failed', 'the Rust did not compile')
        out['next'] = 'the Rust did not compile: see top_missing; the converter or the SDK needs those calls'
        return out
    log('convert', 'done', f'{report.get("methods", 0):,} methods · {report.get("rust_lines", 0):,} lines of Rust')
    log('convert', 'data', {'plugin': out['plugin'], 'convert': out['convert'], 'source': out['source']})
    log('build', 'running', 'release build on 4 threads')
    out['build'] = build(crate, id_, log, lane.target if lane else None)
    if not out['build']['ok']:
        log('build', 'failed', (out['build']['errors'] or ['the build failed'])[0])
        out['next'] = 'the release build failed: see build.errors'
        return out
    log('build', 'done', f'{out["build"]["mb"]} MB · {out["build"]["seconds"]} s')
    dll = pathlib.Path(out['build']['dll'])
    log('check', 'running', 'starting it on a scratch server')
    out['check'] = smoke(dll, name, lane.smoke_port if lane else SMOKE_PORT)
    log('check', 'done' if out['check']['enabled'] else 'failed', 'enabled cleanly' if out['check']['enabled'] else (out['check']['trouble'] or [out['check'].get('server_said', 'it did not enable')])[0][-160:])
    if server:
        log('install', 'running', f'installing on {server}')
        if out['check']['enabled'] or force:
            out['install'] = install(dll, name, server, force)
        else:
            out['install'] = {'installed': False, 'why': 'it did not enable cleanly on the scratch server (see check.trouble): pass force to install anyway'}
        installed = out['install'].get('installed')
        log('install', 'done' if installed else 'failed', f'on {out["install"].get("server")}' + (' · restarted' if out['install'].get('restarted') else ' · loads at next start') if installed else out['install'].get('why', ''))
    else:
        log('install', 'skipped', 'no server chosen')
    out['next'] = ('installed: join and try its commands' if out.get('install', {}).get('installed')
                   else f'pass server to install it (the launcher has: {", ".join(sorted(p.name for p in (LAUNCHER / "servers").iterdir() if p.is_dir()))})' if not server else 'see install.why')
    out['honest'] = 'Enabling cleanly is not proof of behaviour: methods_stubbed and top_missing name what may not work; plugin_parity compares it with the Java plugin on Paper.'
    return out


def plugin_port_many(sources, server='', force=False, workers=3):
    """Several plugins at once, each on its own lane, then all of them onto `server` with one restart."""
    from concurrent.futures import ThreadPoolExecutor
    lanes = [Lane(number) for number in range(max(1, workers))]
    free = list(lanes)
    guard = __import__('threading').Lock()

    def one(source):
        with guard:
            lane = free.pop()
        try:
            return source, plugin_port(source, lane=lane)
        except Exception as error:
            return source, {'error': f'{type(error).__name__}: {error}'}
        finally:
            with guard:
                free.append(lane)

    with ThreadPoolExecutor(len(lanes)) as pool:
        results = dict(pool.map(one, sources))
    if server:
        ready = [(pathlib.Path(r['build']['dll']), r['plugin']['name']) for r in results.values() if r.get('check', {}).get('enabled') or (force and r.get('build', {}).get('ok'))]
        installed = install_many(ready, server, force) if ready else {}
        for result in results.values():
            name = (result.get('plugin') or {}).get('name')
            if name:
                result['install'] = installed.get(name, {'installed': False, 'why': 'it did not enable cleanly on the scratch server'})
    return results


if __name__ == '__main__':
    print(json.dumps(plugin_port(*sys.argv[1:3]), indent=1))
