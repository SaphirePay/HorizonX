"""A crowd of players on one HorizonsX server, tracked the old way (every player in view, every update) and the
new way (a lobby's spigot.yml: nearest 40 players, 48-block range, far ones at half the updates), switched live
on the same server: what each player receives a second, the server's CPU and its tick.

python tools/v2/crowd_ab.py [--bots 200] [--engine DIR]   (DIR: horizon-client.exe, horizon-server.exe,
horizon-bots.exe; default D:/Horizon/perf/hxtrack)"""
import json
import os
import pathlib
import subprocess
import sys
import threading
import time

import psutil

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import ramp  # noqa: E402

PERF = pathlib.Path(r'D:\Horizon\perf')
ENGINE = pathlib.Path(ramp.option('--engine', str(PERF / 'hxtrack')))
BOTS = int(ramp.option('--bots', '200'))
WINDOW = 40

OLD = """world-settings:
  default:
    entity-tracking-range:
      players: 512
    max-players-seen-per-player: 0
    far-entity-update-multiplier: 1
"""
NEW = """world-settings:
  default:
    entity-tracking-range:
      players: 48
    max-players-seen-per-player: 40
    far-entity-distance: 16
    far-entity-update-multiplier: 2
"""
NONE = """world-settings:
  default:
    entity-tracking-range:
      players: 0
"""


def main():
    os.environ.setdefault('HORIZON_NO_AUDIO', '1')
    port = ramp.free_port()
    s = ramp.Server(ENGINE, PERF / '.ramp' / 'crowd', port, ramp.RESOURCES)
    s.start(None)
    (s.dir / 'spigot.yml').write_text(OLD)
    engine = next(p for p in psutil.process_iter(['name']) if p.info['name'] == 'horizon-client.exe'
                  and 'crowd' in (p.cwd() if p.is_running() else ''))
    lines = []
    bots = subprocess.Popen([str(ENGINE / 'horizon-bots.exe'), '--server', f'127.0.0.1:{port}', '--bots', str(BOTS),
                             '--behaviour', 'walk', '--join-every', '0.05', '--seconds', '1000', '--report', 'NUL'],
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)

    def read():
        for line in bots.stdout:
            try:
                lines.append(json.loads(line))
            except ValueError:
                pass
    threading.Thread(target=read, daemon=True).start()
    try:
        while not lines or lines[-1].get('online', 0) < BOTS:
            time.sleep(1)
            if bots.poll() is not None:
                sys.exit('the bots stopped')
        print(f'{BOTS} bots online; settling 30 s', flush=True)
        time.sleep(30)

        def window(label):
            b0, c0, t0, k0 = lines[-1]['bytes_in'], sum(engine.cpu_times()[:2]), time.time(), lines[-1]['chunks']
            time.sleep(WINDOW)
            b1, c1, t1, k1 = lines[-1]['bytes_in'], sum(engine.cpu_times()[:2]), time.time(), lines[-1]['chunks']
            per_bot = (b1 - b0) / (t1 - t0) / BOTS
            status = s.run('status')
            mspt = next((l.strip() for l in status.splitlines() if 'MSPT' in l or 'mspt' in l), '')
            print(f'{label}: {per_bot / 1024:6.1f} KB/s per player ({per_bot * BOTS * 8 / 1e6:6.1f} Mbit/s all) | '
                  f'server CPU {(c1 - c0) / (t1 - t0) * 100:5.1f}% of a core | chunks {(k1 - k0) / (t1 - t0) / BOTS:.1f}/s a player | {mspt}', flush=True)

        window('old tracking')
        if '--none' in sys.argv:
            (s.dir / 'spigot.yml').write_text(NONE)
            time.sleep(10)
            window('no players  ')
        (s.dir / 'spigot.yml').write_text(NEW)
        time.sleep(10)
        window('new tracking')
        (s.dir / 'spigot.yml').write_text(OLD)
        time.sleep(10)
        window('old again   ')
    finally:
        bots.kill()
        s.stop()


if __name__ == '__main__':
    main()
