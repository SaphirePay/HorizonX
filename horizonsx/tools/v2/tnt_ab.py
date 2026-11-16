#!/usr/bin/env python3
"""A TNT cube (20x20x20: 8,000 TNT, lit in the middle) on builds A B ...: each on a fresh world (seed 12345), with
CoreProtect and EssentialsX (a real server's explosion listeners), 5 bots in creative beside it, the server's own
report from a second before the cube is lit. (The live field of 11-06, tnt-repro.tgz, can't be used: its TNT
entities were saved by a newer engine than Rust Paper's and don't load there.)

    python -I tools/v2/tnt_ab.py rp-base rp-tnt [--plugins DIR] [--seconds 20]

Prints each build's TPS, MSPT average, P95, P99 and worst tick, and whether the bots stayed connected.
"""
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tarfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ramp  # noqa: E402

PERF = ramp.PERF
option = ramp.option
FIELD = (64, 100, 64)


def world():
    """The TNT field's world, unpacked once."""
    target = PERF / 'tnt-world'
    if not (target / 'level.dat').exists():
        shutil.rmtree(target, ignore_errors=True)
        unpack = PERF / '_tntx'
        shutil.rmtree(unpack, ignore_errors=True)
        with tarfile.open(PERF / 'tnt-repro.tgz') as tar:
            tar.extractall(unpack, filter='data')
        shutil.move(str(unpack / 'world'), str(target))
        shutil.rmtree(unpack, ignore_errors=True)
    return target


def run(name, plugins, seconds):
    s = ramp.Server(PERF / name, PERF / '.ramp' / f'tnt-{name}', ramp.free_port(), ramp.RESOURCES)
    s.start(None)
    s.stop()
    (s.dir / 'plugins').mkdir(exist_ok=True)
    for lib in plugins:
        shutil.copy2(lib, s.dir / 'plugins' / lib.name)
    subprocess.run([s.hs, '--dir', str(s.dir), 'start'], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL, timeout=120)
    for _ in range(240):
        if 'Done (' in s.log():
            break
        time.sleep(1)
    time.sleep(5)
    enabled = re.search(r'server started: (\d+) plugins', s.log())
    bots = subprocess.Popen([str(PERF / name / 'horizon-bots.exe'), '--server', f'127.0.0.1:{s.port}', '--bots', '5',
                             '--name', 'Tnt', '--behaviour', 'stand', '--seconds', '100000', '--report', 'NUL'],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(60):
            if s.log().count('joined the game') >= 5:
                break
            time.sleep(1)
        s.run('cmd', 'gamemode', 'creative', '@a', timeout=60)
        s.run('cmd', 'tp', '@a', FIELD[0] - 24, FIELD[1] + 10, FIELD[2] - 24, timeout=60)
        time.sleep(6)
        x, y, z = FIELD
        s.run('cmd', 'fill', x, y, z, x + 19, y + 9, z + 19, 'minecraft:tnt', timeout=60)
        s.run('cmd', 'fill', x, y + 10, z, x + 19, y + 19, z + 19, 'minecraft:tnt', timeout=60)
        # The report starts first and the bots arrive a second into it: the field's first ticks are the worst.
        import threading
        got = {}
        reporter = threading.Thread(target=lambda: got.update(r=s.report(seconds)))
        reporter.start()
        time.sleep(1)
        s.run('cmd', 'summon', 'minecraft:tnt', x + 10, y + 10, z + 10, '{fuse:1}', timeout=60)
        reporter.join()
        nums, line, _ = got['r']
        log = s.log()
        nums['online_after'] = s.status().get('players', {}).get('online') if isinstance(s.status().get('players'), dict) else None
        nums['lost'] = log.count('lost connection') + log.count('kicked')
        nums['plugins'] = int(enabled.group(1)) if enabled else 0
        print(f"{name:10} TPS {nums.get('tps')}  MSPT {nums.get('mspt')}  P95 {nums.get('p95')}  P99 {nums.get('p99')}  "
              f"worst {nums.get('max')}  | plugins {nums['plugins']}, disconnects {nums['lost']}", flush=True)
        print(f'           {line[:200]}', flush=True)
        return nums
    finally:
        bots.kill()
        s.stop()


def main():
    names = [a for a in sys.argv[1:] if not a.startswith('--') and not sys.argv[sys.argv.index(a) - 1].startswith('--')]
    plugins_dir = pathlib.Path(option('--plugins', r'D:\Horizon\horizonsx-repo\out\plugins'))
    plugins = [p for p in (plugins_dir / 'coreprotect_rs.dll', plugins_dir / 'essentials_bundle.dll') if p.exists()]
    seconds = int(option('--seconds', '20'))
    print(f'TNT field replay, plugins: {", ".join(p.name for p in plugins) or "none"}', flush=True)
    for name in names:
        run(name, plugins, seconds)


if __name__ == '__main__':
    main()
