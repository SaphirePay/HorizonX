#!/usr/bin/env python3
"""Join a HorizonsX server with the real vanilla 26.3 client on Windows, offline, and report it.

Usage: python -I tools/v2/vanilla_join.py PERF_NAME [--seconds 40] [--name VanillaGuest]
                                          [--mods DIR] [--console "cmd;cmd"]
--mods puts Paper plugin DLLs in the server's plugins folder; --console runs console commands once the
player is in. With either, it prints what the plugins logged and the chat the client got.

Starts the engine in D:/Horizon/perf/PERF_NAME as a dedicated server (offline mode, its own folder
under D:/Horizon/perf/.vanilla-join), launches the official client from %APPDATA%/.minecraft
(versions/26.3, its libraries and assets, --offlineDeveloperMode, --quickPlayMultiplayer) and waits
for the server to log the player's join. Then it asks the server who is on, stops both and prints
PASS or FAIL with the lines that show it. A game window opens for those seconds.
"""
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time
import uuid

PERF = pathlib.Path(r'D:\Horizon\perf')
MC = pathlib.Path(os.environ['APPDATA']) / '.minecraft'
VERSION = '26.3'
JAVA = r'C:\Program Files\Java\jdk-25\bin\java.exe'
RESOURCES = pathlib.Path(os.environ['APPDATA']) / 'horizon-launcher' / 'versions' / 'nightly-2026-10-31-d91cab6' / '.local' / 'horizon-client' / '26.3'


def option(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


def allowed(library):
    """The version file's rules for this machine (Windows, x86_64)."""
    rules = library.get('rules')
    if not rules:
        return True
    verdict = False
    for rule in rules:
        os_rule = rule.get('os', {})
        if os_rule and os_rule.get('name') not in (None, 'windows'):
            continue
        if os_rule.get('arch') not in (None, 'x86', 'x86_64', 'amd64') and 'arch' in os_rule:
            continue
        verdict = rule['action'] == 'allow'
    return verdict


def client_command(name, join, game_dir):
    # The launcher's verified copy of the official client (Horizon prepares it from the player's own
    # Minecraft): the client jar and its Windows libraries, natives included.
    version_root = RESOURCES.parents[2]
    game = version_root / '.local' / 'minecraft' / VERSION
    prepared = json.loads((game / 'prepared.json').read_text(encoding='utf-8'))
    manifest = json.loads((game / 'manifest.json').read_text(encoding='utf-8'))
    jars = [str(game / 'jars' / 'client.jar')]
    jars += [str(version_root / p) for p in prepared['libraries']['client']
             if 'arm64' not in p and 'natives-windows-x86' not in p]
    natives = game_dir / 'natives'
    natives.mkdir(parents=True, exist_ok=True)
    offline = uuid.UUID(bytes=bytes(__import__('hashlib').md5(f'OfflinePlayer:{name}'.encode()).digest()))
    return [JAVA, '-Xmx2G', '--enable-native-access=ALL-UNNAMED',
            f'-Djava.library.path={natives}', f'-Dorg.lwjgl.system.SharedLibraryExtractPath={natives}',
            '-cp', os.pathsep.join(jars), manifest.get('mainClass', 'net.minecraft.client.main.Main'),
            '--username', name, '--uuid', offline.hex, '--accessToken', '0', '--offlineDeveloperMode',
            '--version', VERSION, '--gameDir', str(game_dir), '--assetsDir', str(MC / 'assets'),
            '--assetIndex', manifest['assetIndex']['id'], '--width', '960', '--height', '540',
            '--quickPlayMultiplayer', join]


def main():
    name = sys.argv[1]
    folder = PERF / name
    player = option('--name', 'VanillaGuest')
    seconds = float(option('--seconds', '45'))
    run = PERF / '.vanilla-join' / name
    shutil.rmtree(run, ignore_errors=True)
    (run / 'server').mkdir(parents=True)
    port = 25690
    (run / 'server' / 'server.properties').write_text(
        f'server-port={port}\nonline-mode=false\nview-distance=6\nsimulation-distance=6\nlevel-seed=12345\n'
        'gamemode=creative\nspawn-protection=0\nwhite-list=false\nenforce-whitelist=false\n', encoding='utf-8')
    (run / 'server' / 'eula.txt').write_text('eula=true\n', encoding='utf-8')
    if '--mods' in sys.argv:
        # Paper plugins: the server loads them from its plugins folder, as Paper does.
        (run / 'server' / 'plugins').mkdir(exist_ok=True)
        for dll in pathlib.Path(option('--mods', '')).glob('*.dll'):
            shutil.copy2(dll, run / 'server' / 'plugins' / dll.name)
    server_exe = str(folder / 'horizon-server.exe')
    # As server_bench.py: server.toml from the properties, no web panel.
    subprocess.run([server_exe, '--dir', str(run / 'server'), 'import', str(run / 'server' / 'server.properties')],
                   check=True, capture_output=True)
    toml = run / 'server' / 'server.toml'
    toml.write_text(toml.read_text(encoding='utf-8').replace('bind = "127.0.0.1:8130"', 'bind = ""'), encoding='utf-8')
    subprocess.run([server_exe, '--dir', str(run / 'server'), 'config', 'set', f'engine.path={folder / "horizon-client.exe"}',
                    f'engine.resources={RESOURCES}', 'panel.bind='], capture_output=True)
    log = open(run / 'server.log', 'w', encoding='utf-8', errors='replace')
    server = subprocess.Popen([server_exe, '--dir', str(run / 'server'), 'run'], stdin=subprocess.PIPE, stdout=log,
                              stderr=subprocess.STDOUT, text=True)
    lines = lambda: (run / 'server.log').read_text(encoding='utf-8', errors='replace')
    started = time.time()
    while 'Done (' not in lines() and time.time() - started < 120:
        time.sleep(0.5)
    if 'Done (' not in lines():
        server.kill()
        print('FAIL: the server did not start\n' + lines()[-2000:])
        sys.exit(1)
    if '--no-client' in sys.argv:
        # Only the server: the console commands, then stop (no game window).
        for command in [c.strip() for c in option('--console', '').split(';') if c.strip()]:
            server.stdin.write(command + '\n')
            server.stdin.flush()
            time.sleep(2)
        server.stdin.write('stop\n')
        server.stdin.flush()
        try:
            server.wait(timeout=30)
        except subprocess.TimeoutExpired:
            server.kill()
        text = lines()
        start = text.find('Done (')
        print('--- plugins\n' + '\n'.join(l[:200] for l in text.splitlines() if l.startswith('[mod]') or 'fabric_loader' in l))
        print('--- after start\n' + text[start:][:6000])
        return
    game_dir = run / 'client'
    game_dir.mkdir()
    (game_dir / 'options.txt').write_text('onboardAccessibility:false\nskipMultiplayerWarning:true\njoinedFirstServer:true\n'
                                          'tutorialStep:none\npauseOnLostFocus:false\nrenderDistance:6\nsoundCategory_master:0.0\n'
                                          'narrator:0\n', encoding='utf-8')
    client_log = open(run / 'client.log', 'w', encoding='utf-8', errors='replace')
    client = subprocess.Popen(client_command(player, f'127.0.0.1:{port}', game_dir), cwd=game_dir, stdout=client_log,
                              stderr=subprocess.STDOUT)
    joined = False
    deadline = time.time() + seconds
    while time.time() < deadline and client.poll() is None:
        if f'{player} joined the game' in lines():
            joined = True
            break
        time.sleep(0.5)
    if joined:
        # Stay a few seconds: chunks, entities and the tick go on with the vanilla client in.
        time.sleep(8)
        # --console "a;b": console commands (a plugin's, say), each answered in the logs.
        for command in [c.strip() for c in option('--console', '').split(';') if c.strip()] + ['list']:
            server.stdin.write(command + '\n')
            server.stdin.flush()
            time.sleep(1.5)
    still_in = client.poll() is None
    exit_code = client.returncode
    client.kill()
    time.sleep(1)
    server.stdin.write('stop\n')
    server.stdin.flush()
    try:
        server.wait(timeout=30)
    except subprocess.TimeoutExpired:
        server.kill()
    text = lines()
    shown = [l for l in text.splitlines() if player in l or 'players online' in l or 'There are' in l
             or 'lost connection' in l or 'Disconnect' in l][-12:]
    print('PASS' if joined and still_in else 'FAIL',
          f'(joined={joined}, client still running after join={still_in}, client exit code={exit_code})')
    print('\n'.join(shown))
    if '--mods' in sys.argv or '--console' in sys.argv:
        # What the plugins said on the server, and the chat the vanilla client got.
        said = [l for l in text.splitlines() if l.startswith('[mod]') or 'fabric_loader' in l][-25:]
        chat = [l for l in (run / 'client.log').read_text(encoding='utf-8', errors='replace').splitlines() if '[CHAT]' in l][-15:]
        print('--- plugins\n' + '\n'.join(l[:200] for l in said))
        print('--- client chat\n' + '\n'.join(l[:200] for l in chat))
    if not joined:
        print('--- client log tail\n' + (run / 'client.log').read_text(encoding='utf-8', errors='replace')[-3000:])
    sys.exit(0 if joined and still_in else 1)


if __name__ == '__main__':
    main()
