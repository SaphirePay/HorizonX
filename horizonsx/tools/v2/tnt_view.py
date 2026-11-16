#!/usr/bin/env python3
"""What a player's game makes of a TNT field: a real Horizon client (offscreen) about 24 blocks from an 8,000 TNT cube
in the air (y 150, noon, fps uncapped),
its frames a second through the blast, on builds A B ...

    python -I tools/v2/tnt_view.py hxm1 hxfx [--client-bin D:/Horizon/perf/hx-client-snap] [--out DIR]

Each build: a fresh world, the client in creative looking at the cube's centre, the cube filled and lit in the
middle, the client's status read every half second for 25 s (fps), stills at 3 and 8 s. Prints min / average fps
and how many seconds went under 30 fps.
"""
import os
import pathlib
import statistics
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import lab  # noqa: E402
import ramp  # noqa: E402
from lan_e2e import Game  # noqa: E402

PERF = ramp.PERF
option = ramp.option
CUBE = (100, 165, -12)  # in the air near spawn (96, 145, -31 on seed 12345): its chunks are loaded at once


def run(name, client_bin, out):
    s = ramp.Server(PERF / name, PERF / '.ramp' / f'view-{name}', ramp.free_port(), ramp.RESOURCES)
    s.start(None)
    game = Game(f'view-{name}', client_bin / 'horizon-ctl.exe', lab.RESOURCES.resolve())
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'HORIZON_LAUNCHER_HOME'):
        game.d.env[key] = str(game.work / key.lower())
    try:
        game('launch', wait=False, offscreen=True)
        game.raw('connect', f'127.0.0.1:{s.port}', 'Viewer')
        game.raw('wait-for', 'loaded', '--timeout', 300)
        # Frames as fast as the GPU draws them: a cap hides what the blast costs.
        game.raw('options-set', 'enableVsync', 'false')
        game.raw('options-set', 'maxFps', '260')
        x, y, z = CUBE
        # In the air, by day: blasts in water break nothing (the cube wouldn't chain) and night hides them.
        s.run('cmd', 'time', 'set', 'noon', timeout=60)
        s.run('cmd', 'gamerule', 'doDaylightCycle', 'false', timeout=60)
        s.run('cmd', 'gamemode', 'creative', 'Viewer', timeout=60)
        s.run('cmd', 'tp', 'Viewer', x - 14, y + 12, z - 14, timeout=60)
        # Creative flight where the tp put it (a platform would be blown away, dropping the player into the sea):
        # the control's fly holds the player on a short line, then leaves it flying there.
        game.raw('fly', x - 14, y + 12, z - 14, x - 14, y + 12, z - 14.05, 1)
        game.wait(120)
        game.raw('look-at', x + 10, y + 10, z + 10)
        s.run('cmd', 'fill', x, y, z, x + 19, y + 9, z + 19, 'minecraft:tnt', timeout=60)
        s.run('cmd', 'fill', x, y + 10, z, x + 19, y + 19, z + 19, 'minecraft:tnt', timeout=60)
        game.wait(60)
        quiet = game.raw('status').get('fps')
        s.run('cmd', 'summon', 'minecraft:tnt', x + 10, y + 10, z + 10, '{fuse:1}', timeout=60)
        start, fps, shots = time.time(), [], {}
        while time.time() - start < 25:
            t = time.time() - start
            fps.append((round(t, 1), game.raw('status').get('fps') or 0))
            for mark in (3, 8):
                if t >= mark and mark not in shots:
                    shots[mark] = game.shot(out, f'{name}-{mark}s')
            time.sleep(0.5)
        values = [f for _, f in fps]
        low = sum(0.5 for f in values if f < 30)
        print(f'{name:8} before the blast {quiet} fps | during: min {min(values)}  average {statistics.mean(values):.0f}  '
              f'median {statistics.median(values):.0f}  | {low:.1f} s under 30 fps', flush=True)
        print(f'         {fps}', flush=True)
        return values
    finally:
        game.close()
        s.stop()


def main():
    names = [a for a in sys.argv[1:] if not a.startswith('--') and not sys.argv[sys.argv.index(a) - 1].startswith('--')]
    client_bin = pathlib.Path(option('--client-bin', str(PERF / 'hx-client-snap')))
    out = pathlib.Path(option('--out', str(PERF / 'tnt-view')))
    out.mkdir(parents=True, exist_ok=True)
    os.environ.update(lab.game_env())
    for name in names:
        run(name, client_bin, out)


if __name__ == '__main__':
    main()
