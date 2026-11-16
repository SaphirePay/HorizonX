#!/usr/bin/env python3
"""Side by side: several server builds on the same bench, taken in turns, with the median of each.

Usage: python -I tools/v2/compare.py [--quick] BOTS ROUNDS LABEL=PERF_NAME[:flag,flag] ...
  --quick  joins 3x faster, 15 s warm-up, 30 s window: about a minute a run (for checks while working)
  BOTS     a comma list, e.g. 50,100
  ROUNDS   how many times each build runs at each count (A B C, A B C, ...)
  flags    no-snow (the Paper port's built-in Snow Under Trees off), env:NAME=VALUE,
           set:KEY=VALUE (server.properties, e.g. set:simulation-distance=6), paper (real Paper
           from the parity rig: PERF_NAME is then ignored), plugins (the Paper plugin DLLs in
           D:/Horizon/v2-paper/plugins on the server)
Every run is tools/v2/bench.py (mixed bots, 60 s window after a 30 s warm-up). Writes
D:/Horizon/perf/compare-<time>.json and prints a table: TPS (from MSPT), MSPT, the server's own tick
CPU per tick, server cores, RSS. Run it only on a quiet machine.
"""
import json
import pathlib
import statistics
import subprocess
import sys
import time

TREE = pathlib.Path(__file__).resolve().parents[2]
PERF = pathlib.Path(r'D:\Horizon\perf')
# The Paper plugins of the `plugins` flag: a copy of the port's test server's (D:/Horizon/paper-server/server/mods).
PLUGINS = pathlib.Path(r'D:\Horizon\v2-paper\plugins')


def run(label, name, flags, bots):
    args = [sys.executable, '-I', str(TREE / 'tools' / 'v2' / 'bench.py'), name, str(bots)]
    if QUICK:
        # About a minute a run instead of two and a half: for checks while working, not the final table.
        args += ['--join-every', '0.1', '--warmup', '15', '--seconds', '30']
    for flag in flags:
        if flag == 'no-snow':
            args.append('--no-snow')
        elif flag == 'paper':
            args.append('--paper')
        elif flag.startswith('env:'):
            args += ['--env', flag[4:]]
        elif flag.startswith('set:'):
            # server.properties, e.g. set:simulation-distance=6 (every server reads the same file).
            args += ['--set', flag[4:]]
        elif flag == 'plugins':
            # The same Paper plugins on every server that takes them (the port's DLLs).
            args += ['--mods', str(PLUGINS)]
    folder = PERF.joinpath(name)
    folder.mkdir(parents=True, exist_ok=True)
    before = set(folder.glob('report-*.json')) | set(folder.glob('cost-*.json'))
    out = subprocess.run(args, capture_output=True, text=True, encoding='utf-8', errors='replace')
    fresh = lambda pattern: sorted(set(folder.glob(pattern)) - before, key=lambda p: p.stat().st_mtime)
    reports = fresh('report-*.json')
    first = json.loads(reports[-1].read_text(encoding='utf-8'))['runs'][0] if reports else {}
    if not reports or 'error' in first or not first.get('mspt'):
        print(f'  {label} {bots}: no result {first.get("error", "")}\n{out.stdout[-1500:]}{out.stderr[-1500:]}', flush=True)
        return None
    # The bench's own numbers for every server (Paper's JVM included); the engine's tick-thread CPU
    # where perf_cost found an engine (Horizon builds).
    costs = fresh('cost-*.json')
    cost = (json.loads(costs[-1].read_text(encoding='utf-8')) or [{}])[0] if costs else {}
    row = {
        'tps': first.get('tps'),
        'mspt': first['mspt'].get('average'),
        'p99': first['mspt'].get('p99'),
        'rss_mb': (first.get('rss_mb') or {}).get('mean'),
        'server_cores': round((first.get('cpu_percent') or 0) / 100, 2),
        'tick_cpu_ms': cost.get('tick_cpu_ms'),
        'chunks_per_second': first.get('chunks_per_second'),
        'join_p50_ms': (first.get('join_ms') or {}).get('p50_ms'),
    }
    print(f'  {label:<14} {bots:>4} bots: TPS {row["tps"]}, MSPT {row["mspt"]}, p99 {row["p99"]}, '
          f'tick CPU {row["tick_cpu_ms"]}, cores {row["server_cores"]}, RSS {row["rss_mb"]} MB', flush=True)
    return row


QUICK = '--quick' in sys.argv


def main():
    if QUICK:
        sys.argv.remove('--quick')
    counts = [int(b) for b in sys.argv[1].split(',')]
    rounds = int(sys.argv[2])
    builds = []
    for spec in sys.argv[3:]:
        label, rest = spec.split('=', 1)
        name, _, flags = rest.partition(':')
        builds.append((label, name, [f for f in flags.split(',') if f]))
    results = {(label, bots): [] for label, _, _ in builds for bots in counts}
    for r in range(rounds):
        for bots in counts:
            print(f'round {r + 1}, {bots} bots', flush=True)
            for label, name, flags in builds:
                row = run(label, name, flags, bots)
                if row:
                    results[(label, bots)].append(row)
    median = lambda rows, key: round(statistics.median(x[key] for x in rows if x.get(key) is not None), 2) if any(x.get(key) is not None for x in rows) else None
    table = []
    print(f'\n{"build":<14} {"bots":>5} {"runs":>4} {"TPS":>6} {"MSPT":>7} {"p99":>7} {"tickCPU":>8} {"cores":>6} {"RSS MB":>7}')
    for (label, bots), rows in results.items():
        if not rows:
            continue
        line = {k: median(rows, k) for k in ('tps', 'mspt', 'p99', 'tick_cpu_ms', 'server_cores', 'rss_mb')}
        line.update(build=label, bots=bots, runs=len(rows))
        table.append(line)
        cell = lambda k, w: f'{str(line[k]):>{w}}'
        print(f'{label:<14} {bots:>5} {len(rows):>4} {cell("tps", 6)} {cell("mspt", 7)} {cell("p99", 7)} '
              f'{cell("tick_cpu_ms", 8)} {cell("server_cores", 6)} {cell("rss_mb", 7)}')
    out = PERF / f'compare-{time.strftime("%Y%m%d-%H%M%S")}.json'
    out.write_text(json.dumps({'table': table, 'runs': {f'{k[0]}@{k[1]}': v for k, v in results.items()}}, indent=1))
    print('wrote', out)


if __name__ == '__main__':
    main()
