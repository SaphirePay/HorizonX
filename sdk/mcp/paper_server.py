#!/usr/bin/env python3
"""A Horizon server running Rust plugins through the Paper API, to play on.

Usage: paper_server.py up       creates it if new, copies the plugins, starts it, proves it with a test client
       paper_server.py down     stops it
       paper_server.py status

Its folder is D:\\Horizon\\paper-server, with its own engine copy, so rebuilding the engine never finds the exe
locked. Port 25570: 25565 belongs to another server on this PC.
"""
import workspace  # noqa: E402
import os
import pathlib
import shutil
import subprocess
import sys
import time

sys.path.insert(0, str(workspace.at('fabric-port', 'tools')))
import proof  # noqa: E402

ROOT = workspace.at('paper-server')
ENGINE = ROOT / 'engine' / 'horizon-client.exe'
PARITY = workspace.at('fabric-port', 'out', 'parity', 'horizon-client.exe')
PLUGINS = workspace.at('plugin-ports', 'target', 'release')
PORT = 25570
NAME = 'Prover'
proof.RUN = ROOT
proof.NAME = NAME


def environment():
    return dict(os.environ, HORIZON_HOME=str(ROOT / 'client'), HORIZON_CONTROL=str(ROOT / 'control.json'), HORIZON_NO_HEARTBEAT='1', HORIZON_NO_DISCORD='1')


def ctl(*args, timeout=30):
    result = subprocess.run([str(proof.CTL), '--timeout', str(timeout), *map(str, args)], env=environment(), capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=timeout + 15)
    return result.stdout


def up():
    ROOT.mkdir(parents=True, exist_ok=True)
    ENGINE.parent.mkdir(exist_ok=True)
    shutil.copy2(PARITY, ENGINE)
    server = ROOT / 'server'
    if not server.is_dir():
        print(proof.server('init', '--eula', '--world', 'paper', '--seed', '12345', '--port', str(PORT), '--no-whitelist', '--offline', '--op', NAME, '--yes')[:140])
    print(proof.server('config', 'set', f'engine.path={ENGINE}', f'engine.resources={proof.RESOURCES}', 'panel.bind=', 'world.game_mode=survival', 'properties.spawn-protection=0')[:140])
    (server / 'mods').mkdir(parents=True, exist_ok=True)
    for name in ('papertest', 'sethome', 'afkguard', 'realpeakcounter'):
        shutil.copy(PLUGINS / f'{name}.dll', server / 'mods')
    for name in ('sethome', 'afkguard', 'realpeakcounter', 'papertest'):
        print('plugin', name)
    print(proof.server('start')[:140], flush=True)
    log = server / 'logs' / 'latest.log'
    if not proof.wait(lambda: log.exists() and 'server started' in log.read_text(encoding='utf-8', errors='replace'), 120, 0.25):
        sys.exit('it did not start: ' + log.read_text(encoding='utf-8', errors='replace')[-800:])
    print('server started', flush=True)
    prove()


def prove():
    client = ROOT / 'client'
    client.mkdir(parents=True, exist_ok=True)
    (client / 'options.txt').write_text('tutorialStep:5\nmaxFps:30\n', encoding='utf-8')
    print(proof.captured([str(part) for part in (proof.CTL, '--timeout', 60, 'launch', '--game', ENGINE, '--offscreen', '--width', 320, '--height', 180, '--game-dir', client, '--resources', proof.RESOURCES, '--name', NAME)], 75, environment())[:100], flush=True)
    ctl('connect', f'localhost:{PORT}', NAME, timeout=60)
    ctl('wait-ready', timeout=90)
    time.sleep(1.0)
    ctl('chat', '/paperinfo')
    ctl('chat', '/paperboard')
    ctl('chat', '/sethome test')
    ctl('chat', '/kill')
    time.sleep(3.0)
    log = client / 'logs' / 'latest.log'
    lines = [line.split('[CHAT] ', 1)[1].rstrip() for line in log.read_text(encoding='utf-8', errors='replace').splitlines() if '[CHAT] ' in line] if log.is_file() else []
    print('what the test client saw:')
    print('\n'.join(f'  {line}' for line in lines[-12:]))
    ctl('quit', timeout=5)


def down():
    print(proof.server('stop', timeout=60)[:120])


if __name__ == '__main__':
    {'up': up, 'down': down, 'prove': prove, 'status': lambda: print(proof.server('status')[:300])}[sys.argv[1]]()
