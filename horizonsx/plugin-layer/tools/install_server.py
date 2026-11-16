#!/usr/bin/env python3
"""Put a built engine on a Horizon server, with a backup, and roll back if it doesn't come up.

Usage: install_server.py SERVER_DIR ENGINE_EXE
       install_server.py SERVER_DIR --rollback

Refuses while players are on. Keeps server.toml as server.toml.before-fabric, and a world backup.
"""
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

APPDATA = pathlib.Path(os.environ['APPDATA'])
SERVER = APPDATA / 'horizon-launcher' / 'server-runtime' / 'current' / 'horizon-server.exe'
RESOURCES = APPDATA / 'horizon-launcher' / 'server-runtime' / 'current' / '.local' / 'horizon-client' / '26.3'


def run(folder, *args, timeout=120):
    """Output through a file: `restart` leaves processes behind that would hold a pipe open."""
    with tempfile.NamedTemporaryFile('w+', suffix='.txt', delete=False, encoding='utf-8', errors='replace') as out:
        name = out.name
        try:
            subprocess.run([SERVER, '--dir', folder, *args], stdout=out, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, timeout=timeout)
        except subprocess.TimeoutExpired:
            pass
    text = pathlib.Path(name).read_text(encoding='utf-8', errors='replace')
    try:
        os.unlink(name)
    except OSError:
        pass
    return text


def status(folder):
    try:
        return json.loads(run(folder, 'status', '--json', timeout=20))
    except json.JSONDecodeError:
        return {}


def running(folder):
    state = status(folder)
    return state.get('phase') == 'running' and (state.get('engine_pid') or state.get('sample'))


def healthy(folder, log, since):
    """The server runs, and this start's log says the mod is on."""
    limit = time.time() + 120
    while time.time() < limit:
        if log.exists():
            text = log.read_text(encoding='utf-8', errors='replace')
            if 'panicked' in text and log.stat().st_mtime >= since:
                return False
            if '[paper_events] server started' in text and log.stat().st_mtime >= since and running(folder):
                return True
        time.sleep(2)
    return False


def rollback(folder):
    kept = folder / 'server.toml.before-fabric'
    if not kept.is_file():
        sys.exit('no server.toml.before-fabric to go back to')
    shutil.copy2(kept, folder / 'server.toml')
    print(run(folder, 'restart' if running(folder) else 'start')[:200])
    print('rolled back:', 'running' if wait_running(folder) else 'NOT RUNNING: look at the logs')


def wait_running(folder):
    limit = time.time() + 90
    while time.time() < limit:
        if running(folder):
            return True
        time.sleep(2)
    return False


def main():
    folder = pathlib.Path(sys.argv[1])
    if sys.argv[2] == '--rollback':
        return rollback(folder)
    engine = pathlib.Path(sys.argv[2])
    if not engine.is_file():
        sys.exit(f'no engine at {engine}')
    players = status(folder).get('sample', {}).get('game', {}).get('players', [])
    if players:
        sys.exit(f'players are on: {players}. Nothing changed.')
    kept = folder / 'server.toml.before-fabric'
    if not kept.exists():
        shutil.copy2(folder / 'server.toml', kept)
    print('backup:', run(folder, 'backup', 'before-fabric-api', timeout=300).strip()[:200], flush=True)
    print(run(folder, 'config', 'set', f'engine.path={engine}', f'engine.resources={RESOURCES}').strip()[:200], flush=True)
    since = time.time()
    # `restart` does nothing to a server that is stopped.
    print(run(folder, 'restart' if running(folder) else 'start', timeout=180).strip()[:200], flush=True)
    log = folder / 'logs' / 'latest.log'
    if healthy(folder, log, since):
        state = status(folder)
        print('OK: the mod is on and the server runs.', json.dumps({k: state.get(k) for k in ('phase', 'engine_pid', 'crashes')}))
        return
    print('The server did not come up with the mod: rolling back.', flush=True)
    rollback(folder)
    sys.exit(1)


if __name__ == '__main__':
    main()
