#!/usr/bin/env python3
"""HorizonSMP's live test rig: EssentialsX (economy) + LuckPerms-rs + HorizonSMP on one scratch server (port 25795)
with one offscreen test client "Prover". New builds go in by hot reload.

    smp_live.py up | deploy [NAME] | say LINE... | log [N] | connect | down
"""
import os
import pathlib
import shutil
import sys
import time

sys.stdout.reconfigure(encoding='utf-8', errors='replace')
sys.path.insert(0, r'D:\Horizon\fabric-port\tools')
import proof  # noqa: E402

ENGINE = pathlib.Path(os.environ.get('RW_ENGINE', r'D:\Horizon\paper-engine\out\hwe14\horizon-client.exe'))
RUN = pathlib.Path(r'D:\Horizon\parity\smp')
PORT = 25795
SERVER = RUN / 'server'
LOG = SERVER / 'logs' / 'latest.log'
CLIENT_LOG = RUN / 'client' / 'logs' / 'latest.log'
B = pathlib.Path(r'D:\Horizon\hx-plugin-build')
DLLS = [
    B / 't_ess' / 'release' / 'essentials_bundle.dll',
    B / 't_rw' / 'release' / 'luckperms_rs.dll',
    B / 't_rw' / 'release' / 'horizonsmp.dll',
    B / 't_rw' / 'release' / 'jobs_rs.dll',
    B / 't_rw' / 'release' / 'mcmmo_rs.dll',
    B / 't_rw' / 'release' / 'zauctionhouse_rs.dll',
]
proof.RUN = RUN
# The launcher deletes old nightly folders: the server runtime's copy of the game files stays.
proof.RESOURCES = pathlib.Path(os.environ.get('RW_RESOURCES', r'%USERPROFILE%\AppData\Roaming\horizon-launcher\server-runtime\game-files\horizon-client\26.3'))
proof.PORT = PORT
proof.NAME = 'Prover'
ctl = proof.ctl


def text(path):
    return path.read_text(encoding='utf-8', errors='replace') if path.exists() else ''


def chat_lines():
    return [line.split('[CHAT] ', 1)[1].rstrip() for line in text(CLIENT_LOG).splitlines() if '[CHAT] ' in line]


def copy_dlls(match=''):
    (SERVER / 'plugins').mkdir(parents=True, exist_ok=True)
    n = 0
    for d in DLLS:
        target = SERVER / 'plugins' / d.name
        if d.exists() and match.lower() in d.name.lower() and not (target.exists() and target.read_bytes() == d.read_bytes()):
            shutil.copy(d, target)
            n += 1
    return n


def up():
    RUN.mkdir(parents=True, exist_ok=True)
    print(proof.server('init', '--eula', '--world', 'smp', '--seed', '12345', '--port', str(PORT), '--no-whitelist', '--offline', '--op', 'Prover', '--yes')[:160], flush=True)
    print(proof.server('config', 'set', f'engine.path={ENGINE}', f'engine.resources={proof.RESOURCES}', 'panel.bind=', 'world.game_mode=survival', 'properties.spawn-protection=0')[:160], flush=True)
    print(f'{copy_dlls()} dlls in', flush=True)
    started = time.time()
    print(proof.server('start')[:160], flush=True)
    if not proof.wait(lambda: 'server started' in text(LOG).lower() or 'done (' in text(LOG).lower(), 240):
        print(text(LOG)[-3000:])
        sys.exit('server did not start')
    print(f'server up in {time.time() - started:.1f} s', flush=True)
    print(ctl('launch', '--game', ENGINE, '--offscreen', '--width', 640, '--height', 360, '--game-dir', RUN / 'client', '--resources', proof.RESOURCES, '--name', 'Prover', timeout=90)[:160], flush=True)
    print(ctl('connect', f'localhost:{PORT}', 'Prover', timeout=90)[:160], flush=True)
    ctl('wait-ready', timeout=120)
    print('client in', flush=True)


def deploy(match=''):
    size = len(text(LOG))
    started = time.time()
    n = copy_dlls(match)
    want = n
    if not proof.wait(lambda: text(LOG)[size:].count('[HotReload] swapped') + text(LOG)[size:].count('HotReload] failed') >= want, 300):
        print(text(LOG)[size:][-2000:])
        sys.exit(f'not every hot reload seen in 300 s ({n} dlls)')
    print(f'{n} swapped in {time.time() - started:.1f} s', flush=True)
    for line in text(LOG)[size:].splitlines():
        if any(w in line for w in ('SEVERE', 'panick', 'HotReload] failed', 'ERROR')):
            print('  ' + line[:300])


def steps(lines, settle=None):
    seen_chat = len(chat_lines())
    seen_log = len(text(LOG))
    for line in lines:
        if line.startswith('wait '):
            time.sleep(float(line.split()[1]))
            continue
        ctl('chat', line)
        time.sleep(float(settle or os.environ.get('RW_STEP', 0.8)))
    time.sleep(1.5)
    print('---- chat')
    for line in chat_lines()[seen_chat:]:
        print(line[:300])
    print('---- log')
    for line in text(LOG)[seen_log:].splitlines():
        if any(word in line for word in ('SEVERE', 'WARN', 'Exception', 'panick', 'ERROR')):
            print(line[:300])


def main():
    what = sys.argv[1] if len(sys.argv) > 1 else 'say'
    if what == 'up':
        up()
    elif what == 'deploy':
        deploy(sys.argv[2] if len(sys.argv) > 2 else '')
    elif what == 'say':
        steps(sys.argv[2:])
    elif what == 'log':
        n = int(sys.argv[2]) if len(sys.argv) > 2 else 60
        print('\n'.join(text(LOG).splitlines()[-n:]))
    elif what == 'shot':
        # shot NAME [LINE...]: chat the lines, then a screenshot to parity/smp/shots/NAME.png
        out = RUN / 'shots'
        out.mkdir(exist_ok=True)
        for line in sys.argv[3:]:
            if line.startswith('wait '):
                time.sleep(float(line.split()[1]))
            elif line.startswith('key '):
                ctl('key', *line.split()[1:])
            else:
                ctl('chat', line)
                time.sleep(1.2)
        time.sleep(0.8)
        print(ctl('screenshot', str(out / f'{sys.argv[2]}.png'))[:200])
    elif what == 'relaunch':
        # A bigger window for menu screenshots.
        print(ctl('quit', timeout=15)[:160])
        time.sleep(2)
        print(ctl('launch', '--game', ENGINE, '--offscreen', '--width', 1280, '--height', 720, '--game-dir', RUN / 'client', '--resources', proof.RESOURCES, '--name', 'Prover', timeout=90)[:160], flush=True)
        print(ctl('connect', f'localhost:{PORT}', 'Prover', timeout=90)[:160], flush=True)
        ctl('wait-ready', timeout=120)
        print('client in', flush=True)
    elif what == 'connect':
        print(ctl('connect', f'localhost:{PORT}', 'Prover', timeout=90)[:160], flush=True)
        ctl('wait-ready', timeout=120)
        print('client in', flush=True)
    elif what == 'down':
        print(ctl('quit', timeout=15)[:160])
        print(proof.server('stop', timeout=90)[:160])


if __name__ == '__main__':
    main()
