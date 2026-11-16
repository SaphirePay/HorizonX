#!/usr/bin/env python3
"""Hot reload on Horizon V2, proven with players on: plugins are swapped while bots stay connected.

Usage: python -I tools/v2/hot_reload_proof.py PERF_NAME --mods DIR [--bots 20] [--swap NAME.dll,...]
                                              [--console "cmd;cmd"]

Starts the engine in D:/Horizon/perf/PERF_NAME (a paper_build.py build) as a dedicated server with the
plugin DLLs of --mods in its plugins folder, joins --bots headless bots (horizon-bots, walking), then for each
--swap file (default: every plugin) gives the file a new time, as copying a new build over it would, and waits
for the server's [HotReload] lines. After the swaps it asks `list` and runs the --console commands.
PASS when every swap finished, the server never stopped ticking, and every bot is still on.
No game window opens.
"""
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

PERF = pathlib.Path(r'D:\Horizon\perf')
RESOURCES = pathlib.Path(os.environ['APPDATA']) / 'horizon-launcher' / 'versions' / 'nightly-2026-10-31-d91cab6' / '.local' / 'horizon-client' / '26.3'


def option(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


def main():
    name = sys.argv[1]
    folder = PERF / name
    mods = pathlib.Path(option('--mods', ''))
    bots = int(option('--bots', '20'))
    run = PERF / '.hot-reload' / name
    shutil.rmtree(run, ignore_errors=True)
    server_dir = run / 'server'
    (server_dir / 'plugins').mkdir(parents=True)
    port = 25691
    (server_dir / 'server.properties').write_text(
        f'server-port={port}\nonline-mode=false\nview-distance=6\nsimulation-distance=6\nlevel-seed=12345\n'
        'gamemode=creative\nspawn-protection=0\nwhite-list=false\nenforce-whitelist=false\nmax-players=200\n', encoding='utf-8')
    (server_dir / 'eula.txt').write_text('eula=true\n', encoding='utf-8')
    dlls = sorted(mods.glob('*.dll'))
    for dll in dlls:
        shutil.copy2(dll, server_dir / 'plugins' / dll.name)
    server_exe = str(folder / 'horizon-server.exe')
    subprocess.run([server_exe, '--dir', str(server_dir), 'import', str(server_dir / 'server.properties')],
                   check=True, capture_output=True)
    toml = server_dir / 'server.toml'
    toml.write_text(toml.read_text(encoding='utf-8').replace('bind = "127.0.0.1:8130"', 'bind = ""'), encoding='utf-8')
    subprocess.run([server_exe, '--dir', str(server_dir), 'config', 'set', f'engine.path={folder / "horizon-client.exe"}',
                    f'engine.resources={RESOURCES}', 'panel.bind='], capture_output=True)
    log_path = run / 'server.log'
    log = open(log_path, 'w', encoding='utf-8', errors='replace')
    server = subprocess.Popen([server_exe, '--dir', str(server_dir), 'run'], stdin=subprocess.PIPE, stdout=log,
                              stderr=subprocess.STDOUT, text=True)
    text = lambda: log_path.read_text(encoding='utf-8', errors='replace')

    def console(command, wait=1.5):
        server.stdin.write(command + '\n')
        server.stdin.flush()
        time.sleep(wait)

    def wait_for(predicate, seconds):
        deadline = time.time() + seconds
        while time.time() < deadline:
            if predicate(text()):
                return True
            time.sleep(0.25)
        return False

    ok = True
    bot_proc = None
    try:
        if not wait_for(lambda t: 'Done (' in t, 120):
            print('FAIL: the server did not start\n' + text()[-3000:])
            return 1
        print('plugins loaded:', ', '.join(l.split('] ', 1)[-1][:80] for l in text().splitlines() if 'Enabling' in l)[:1500])
        bot_log = open(run / 'bots.log', 'w', encoding='utf-8', errors='replace')
        bot_proc = subprocess.Popen([str(folder / 'horizon-bots.exe'), '--server', f'127.0.0.1:{port}', '--bots', str(bots),
                                     '--behaviour', 'walk', '--join-every', '0.2', '--seconds', '600', '--view-distance', '6'],
                                    stdout=bot_log, stderr=subprocess.STDOUT)
        if not wait_for(lambda t: t.count('joined the game') >= bots, 60 + bots):
            print(f'FAIL: only {text().count("joined the game")} of {bots} bots joined')
            return 1
        time.sleep(5)
        console('list')
        before = re.findall(r'There are (\d+) of a max', text())
        print('players on before the swaps:', before[-1] if before else '?')
        swap = [s for s in option('--swap', '').split(',') if s] or [d.name for d in dlls]
        for file in swap:
            path = server_dir / 'plugins' / file
            seen = text().count('[HotReload] swapped')
            touched = time.time()
            # A new build copied over it: same bytes, a new time (the watcher compares size and time).
            os.utime(path, (touched, touched))
            done = wait_for(lambda t: t.count('[HotReload] swapped') > seen, 30)
            elapsed = time.time() - touched
            lines = [l for l in text().splitlines() if '[HotReload]' in l][-4:]
            print(f'swap {file}: {"done" if done else "NOT SEEN"} {elapsed:.1f} s after the file changed')
            for line in lines:
                print('   ', line[:200])
            ok &= done
            time.sleep(2)
        for command in [c.strip() for c in option('--console', '').split(';') if c.strip()]:
            console(command, 2)
        console('list', 2)
        after = re.findall(r'There are (\d+) of a max', text())
        still = int(after[-1]) if after else -1
        print('players on after the swaps:', still)
        ok &= still == bots and bot_proc.poll() is None
        dropped = [l for l in text().splitlines() if 'lost connection' in l]
        if dropped:
            print('dropped:', *dropped[:10], sep='\n   ')
            ok = False
        print('--- server log after the bots joined (HotReload, plugin and error lines)')
        for line in text().splitlines():
            if any(k in line for k in ('[HotReload]', 'Disabling', 'Enabling', 'ERROR', 'panicked', 'Exception')):
                print('   ', line[:220])
    finally:
        if bot_proc and bot_proc.poll() is None:
            bot_proc.kill()
        try:
            console('stop', 0)
            server.wait(timeout=30)
        except Exception:
            server.kill()
    print('PASS' if ok else 'FAIL')
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
