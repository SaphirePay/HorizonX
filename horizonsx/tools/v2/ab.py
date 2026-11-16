#!/usr/bin/env python3
"""Before/after on the same machine, back to back: for each scenario, build A then build B (bench.py), each run
waiting for a quiet machine first (no rustc, CPU under 25% for 9 s; at most 15 minutes, then it runs and says so).
Settled runs by default (about 3 minutes each): bots join every 0.15 s, 60 s of warm-up (the join and terrain burst), 60 s measured.

Usage: python -I tools/v2/ab.py BUILD [BUILD...] SCENARIO [SCENARIO...] [--warmup 60] [--seconds 60]
  BUILD    = NAME (a folder of D:/Horizon/perf) or NAME+KEY=VALUE (that build with an environment setting)
  SCENARIO = BOTS:BEHAVIOUR[:SPREAD]   e.g. 100:walk:512:5  (spread BLOCKS:GROUP)  or  100:mixed  (all at spawn)
Writes D:/Horizon/perf/ab-STAMP.json and prints one table: TPS, MSPT average and p99, RSS, CPU per tick.
"""
import json
import pathlib
import subprocess
import sys
import time

import psutil

TREE = pathlib.Path(__file__).resolve().parents[2]
PERF = pathlib.Path(r'D:\Horizon\perf')


def quiet(limit=900):
    start = time.time()
    calm = 0
    while time.time() - start < limit:
        busy = any((p.info['name'] or '').lower().startswith('rustc') for p in psutil.process_iter(['name']))
        cpu = psutil.cpu_percent(interval=3)
        calm = calm + 3 if not busy and cpu < 25 else 0
        if calm >= 9:
            return True, round(time.time() - start)
    return False, round(time.time() - start)


def run(spec, bots, behaviour, spread, warmup, seconds):
    build, *envs = spec.split('+')
    args = [sys.executable, '-I', str(TREE / 'tools' / 'v2' / 'bench.py'), build, bots, '--behaviour', behaviour,
            '--warmup', warmup, '--seconds', seconds, '--join-every', '0.15']
    for env in envs:
        args += ['--env', env]
    if spread:
        args += ['--spread', spread]
    out = subprocess.run(args, capture_output=True, text=True, encoding='utf-8', errors='replace')
    tag = '-'.join(e.replace('=', '_') for e in envs)
    (PERF / build / f'ab-{bots}-{behaviour}-{(spread or "spawn").replace(":", "x")}{"-" + tag if tag else ""}.txt').write_text(out.stdout + out.stderr, encoding='utf-8')
    row = None
    for line in out.stdout.splitlines():
        if line.startswith('{') and '"tps"' in line:
            row = json.loads(line)
    phases = sorted((PERF / build).glob('phases-*.json'), key=lambda p: p.stat().st_mtime)
    cpu = None
    if phases:
        cpu = json.loads(phases[-1].read_text(encoding='utf-8')).get('ticks', {}).get('cpu_ms_average')
    return row, cpu


def main():
    warmup = sys.argv[sys.argv.index('--warmup') + 1] if '--warmup' in sys.argv else '60'
    seconds = sys.argv[sys.argv.index('--seconds') + 1] if '--seconds' in sys.argv else '60'
    flags = ('--warmup', '--seconds')
    words = [w for i, w in enumerate(sys.argv[1:], 1) if w not in flags and sys.argv[i - 1] not in flags]
    builds = [w for w in words if ':' not in w]
    scenarios = [w for w in words if ':' in w]
    results = []
    for scenario in scenarios:
        bots, behaviour, *rest = scenario.split(':')
        spread = ':'.join(rest) if rest else None
        for build in builds:
            calm, waited = quiet()
            row, cpu = run(build, bots, behaviour, spread, warmup, seconds)
            entry = {'build': build, 'scenario': scenario, 'quiet': calm, 'waited_s': waited, 'tick_cpu_ms': cpu}
            if row:
                entry.update(tps=row['tps'], mspt=row['mspt']['average'], p99=row['mspt']['p99'],
                             rss_mb=row['rss_mb']['mean'], joined=row.get('bots_joined'))
            results.append(entry)
            print(json.dumps(entry), flush=True)
    stamp = time.strftime('%Y%m%d-%H%M%S')
    (PERF / f'ab-{stamp}.json').write_text(json.dumps(results, indent=2), encoding='utf-8')
    print(f'\n{"scenario":<18} {"build":<34} {"TPS":>6} {"MSPT":>7} {"p99":>7} {"RSS MB":>7} {"tick CPU":>8} quiet')
    for r in results:
        print(f'{r["scenario"]:<18} {r["build"]:<34} {r.get("tps", "-")!s:>6} {r.get("mspt", "-")!s:>7} '
              f'{r.get("p99", "-")!s:>7} {r.get("rss_mb", "-")!s:>7} {r.get("tick_cpu_ms", "-")!s:>8} {r["quiet"]}')


if __name__ == '__main__':
    main()
