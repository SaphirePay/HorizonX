#!/usr/bin/env python3
"""Boot time and idle RAM of HorizonsX and Pumpkin, measured the same way on this machine.

    python -I tools/v2/bench_boot.py [--hx NAME] [--pumpkin EXE]

Each server twice: a fresh world (seed 12345), then the same world again. Boot = wall time from launching the process
to its ready line (HorizonsX "Done (", Pumpkin "Started server"); idle RAM = the engine's working set 30 s later, no
players.
"""
import os
import pathlib
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ramp  # noqa: E402
from multi import cpu_of, find_pids  # noqa: E402
from pumpkin_multi import Pumpkin  # noqa: E402

option = ramp.option


def main():
    hx = ramp.PERF / option('--hx', 'hxm1')
    pk = pathlib.Path(option('--pumpkin', r'D:\Horizon\bench\pumpkin\target\fast\pumpkin.exe'))
    rows = []

    # HorizonsX: ramp's own setup (fresh world), then a restart on that world.
    s = ramp.Server(hx, ramp.PERF / '.ramp' / 'boot-hx', ramp.free_port(), ramp.RESOURCES)
    t0 = time.time()
    s.start(None)
    fresh = time.time() - t0
    s.stop()
    time.sleep(3)
    t0 = time.time()
    subprocess.run([s.hs, '--dir', str(s.dir), 'start'], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                   stderr=subprocess.DEVNULL, timeout=120)
    while 'Done (' not in s.log():
        time.sleep(0.05)
    again = time.time() - t0
    said = [l for l in s.log().splitlines() if 'Done (' in l][-1]
    time.sleep(30)
    pid = next(iter(find_pids('horizon-client.exe', os.sep + 'boot-hx')), None)
    idle = cpu_of([pid]).get(pid, (0, 0))[1]
    s.stop()
    rows.append(('HorizonsX', fresh, again, idle, said.strip()[-60:]))

    # Pumpkin: its defaults but the seed; fresh world, then again.
    d = ramp.PERF / '.ramp' / 'boot-pk'
    import shutil
    shutil.rmtree(d, ignore_errors=True)
    d.mkdir(parents=True)
    p = Pumpkin(pk, d, ramp.free_port())
    p.start()
    p.stop()
    shutil.rmtree(d / 'world', ignore_errors=True)
    from pumpkin_multi import set_toml
    text = set_toml((d / 'pumpkin.toml').read_text(encoding='utf-8'), '', 'seed', '"12345"')
    text = set_toml(text, 'networking.java', 'address', f'"0.0.0.0:{p.port}"')
    (d / 'pumpkin.toml').write_text(text, encoding='utf-8')
    fresh = p.start()
    p.stop()
    again = p.start()
    time.sleep(30)
    idle = cpu_of([p.proc.pid]).get(p.proc.pid, (0, 0))[1]
    said = [l for l in p.log().splitlines() if 'Started server' in l][-1]
    p.stop()
    rows.append(('Pumpkin', fresh, again, idle, said.strip()[-60:]))

    print(f"{'server':<10} {'boot fresh':>11} {'boot again':>11} {'idle RAM':>9}  own line")
    for name, f, a, i, line in rows:
        print(f'{name:<10} {f:>10.2f}s {a:>10.2f}s {i:>7.0f}MB  {line}')


if __name__ == '__main__':
    main()
