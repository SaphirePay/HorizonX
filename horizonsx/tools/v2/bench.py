#!/usr/bin/env python3
"""HorizonsX's server bench: Horizon's own tools/server_bench.py (from this worktree, never another session's
copy) on frozen binaries in D:/Horizon/perf/NAME, mixed bots on this machine, plus the server's own
`horizon-report` over the measured window, so every run says where the tick's time went, phase by phase.

Usage: python -I tools/v2/bench.py NAME [BOTS] [--seconds 60] [--warmup 30] [--env NAME=VALUE] [--set key=value]
                                    [--paper | --folia]   (real Paper or Folia on Java 25 instead of HorizonsX)
                                    [--mods DIR]   (Paper plugin DLLs from DIR in the server's plugins folder)
                                    [--world-to DIR] [--world-from DIR]   (keep the world made / start from one: pre-generated land)
                                    [--spread BLOCKS[:GROUP]]   (bots sent to homes on a grid BLOCKS apart, GROUP a home)

BOTS is a comma list (default 50). Writes D:/Horizon/perf/NAME/report-*.json, cost-*.json (bots' CPU, memory,
tick-thread CPU, from plugin-tools' perf_cost.py) and phases-*.json (the server's report), and prints a table.
"""
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time

TREE = pathlib.Path(__file__).resolve().parents[2]
PERF = pathlib.Path(r'D:\Horizon\perf')
# The game files the engine reads: the launcher's prepared copy (as fabric-port's proof.py names it).
RESOURCES = pathlib.Path(os.environ['APPDATA']) / 'horizon-launcher' / 'server-runtime' / 'game-files' / 'horizon-client' / '26.3'
sys.path.insert(0, str(TREE / 'tools'))
sys.path.insert(1, r'D:\Horizon\plugin-tools\tools')


def option(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


def main():
    name = sys.argv[1]
    bots = sys.argv[2] if len(sys.argv) > 2 and not sys.argv[2].startswith('--') else '50'
    folder = PERF / name
    seconds = int(option('--seconds', '60'))
    # Read now: sys.argv becomes server_bench's before the runs.
    world_from, world_to = option('--world-from', None), option('--world-to', None)
    no_snow, mods = '--no-snow' in sys.argv, option('--mods', None)
    os.environ['HORIZON_BENCH_BIN'] = str(folder)
    import server_bench as bench
    import perf_cost
    bench.lab.LOCAL = PERF / '.bench-v2'
    bench.lab.LOCAL.mkdir(exist_ok=True)
    for i, word in enumerate(sys.argv):
        if word == '--env':
            key, value = sys.argv[i + 1].split('=', 1)
            os.environ[key] = value
        if word == '--set':
            key, value = sys.argv[i + 1].split('=', 1)
            bench.PROPERTIES[key] = value

    runs = []
    original_command = bench.server_command

    def with_engine(kind, directory, args):
        runs.append(directory)
        command = original_command(kind, directory, args)
        if no_snow:
            # The Paper port's engine has the Snow Under Trees mod built in: its settings turn it off.
            (directory / 'config').mkdir(exist_ok=True)
            (directory / 'config' / 'snowundertrees.json').write_text(
                '{"enable_biome_feature": false, "enable_when_snowing": false}')
        if world_from:
            # A world made before (--world-to of an earlier run): the bots' seeded paths cross land already
            # generated, as on a server with a pre-generated map.
            saved = pathlib.Path(world_from)
            for part in saved.iterdir():
                target = directory / part.name
                if part.is_dir():
                    shutil.copytree(part, target, dirs_exist_ok=True)
                else:
                    shutil.copy2(part, target)
        if mods:
            # Paper plugins (the port's DLLs) on the server: --mods names a folder of them.
            (directory / 'plugins').mkdir(exist_ok=True)
            for dll in pathlib.Path(mods).glob('*.dll'):
                shutil.copy2(dll, directory / 'plugins' / dll.name)
        subprocess.run([str(folder / 'horizon-server.exe'), '--dir', str(directory), 'config', 'set',
                        f'engine.path={folder / "horizon-client.exe"}', f'engine.resources={RESOURCES}', 'panel.bind='],
                       capture_output=True)
        return command

    original_query = bench.tick_query
    asked = set()

    def query(server):
        # The first query of a run opens the measured window: record the tick's spans over it, ending a few
        # seconds before the bench stops the server.
        if id(server) not in asked:
            asked.add(id(server))
            server.console(f'horizon-report {max(seconds - 6, 10)}')
        return original_query(server)

    def tree_usage(root):
        # server_bench asks `ps`, which Windows doesn't have (Git's reads 0): the process tree's RSS and CPU.
        import psutil
        try:
            top = psutil.Process(root)
            procs = [top, *top.children(recursive=True)]
        except psutil.Error:
            return 0, 0.0
        rss = cpu = 0
        for p in procs:
            try:
                rss += p.memory_info().rss
                t = p.cpu_times()
                cpu += t.user + t.system
            except psutil.Error:
                pass
        return rss, cpu

    servers = 'horizon'
    if '--paper' in sys.argv:
        # Real Paper (Java), from the parity rig's own files, on the same bench and bots (as plugin-tools'
        # perf_bench.py --paper). NAME is only where its reports go.
        servers = 'paper'
        home = pathlib.Path(r'D:\Horizon\parity\rig\paper')

        def paper(kind, directory, args):
            runs.append(directory)
            for part in ('cache', 'libraries', 'versions'):
                shutil.copytree(home / part, directory / part)
            # Paper refuses a second login from one address within 4 s; every bot comes from one.
            (directory / 'bukkit.yml').write_text('settings:\n  connection-throttle: -1\n')
            return [r'C:\Program Files\Java\jdk-25\bin\java.exe', '-Xms4G', '-Xmx4G', '-Duser.timezone=UTC',
                    '-jar', r'D:\Horizon\parity\paper-26.3-235.jar', '--nogui']

        bench.server_command = paper
        bench.paper_jar = lambda: (pathlib.Path(r'D:\Horizon\parity\paper-26.3-235.jar'), '26.3-235')
    elif '--folia' in sys.argv:
        # Real Folia (Java, PaperMC's region-threaded fork), patched once in D:/Horizon/folia (its cache,
        # libraries and versions), on the same bench, bots, settings and saved world as HorizonsX.
        # Its region threads: Folia's default (`threaded-regions.threads: -1`).
        servers = 'paper'
        home = pathlib.Path(r'D:\Horizon\folia')
        jar = home / 'folia-26.3-6.jar'

        def folia(kind, directory, args):
            runs.append(directory)
            for part in ('cache', 'libraries', 'versions'):
                shutil.copytree(home / part, directory / part)
            (directory / 'bukkit.yml').write_text('settings:\n  connection-throttle: -1\n')
            # The bench's probe (D:/Horizon/folia-probe): Folia's own per-region tick times, read-only.
            (directory / 'plugins').mkdir(exist_ok=True)
            shutil.copy2(r'D:\Horizon\folia-probe\hx-tick-probe.jar', directory / 'plugins' / 'hx-tick-probe.jar')
            if world_from:
                for part in pathlib.Path(world_from).iterdir():
                    target = directory / part.name
                    if part.is_dir():
                        shutil.copytree(part, target, dirs_exist_ok=True)
                    else:
                        shutil.copy2(part, target)
            return [r'C:\Program Files\Java\jdk-25\bin\java.exe', '-Xms4G', '-Xmx4G', '-Duser.timezone=UTC',
                    '-jar', str(jar), '--nogui']

        bench.server_command = folia
        bench.paper_jar = lambda: (jar, 'folia-26.3-6')
        import re

        def folia_query(server):
            # Folia has no `tick query`: the probe plugin's `hxtick` reads Folia's per-region tick times over
            # the last 15 s; the busiest region's are the tick (each region is a tick of its own). Folia's own
            # `tps` report goes in folia-queries.txt beside them.
            start = server.count()
            server.console('hxtick')
            server.console('tps')
            deadline = time.time() + 5
            found = None
            while time.time() < deadline and found is None:
                for line in server.since(start):
                    if 'HXTICK' in line and '_avg=' in line:
                        found = line
                time.sleep(0.1)
            time.sleep(0.5)
            lines = list(server.since(start))
            with open(folder / 'folia-queries.txt', 'a', encoding='utf-8') as out:
                out.write(f'--- {time.strftime("%H:%M:%S")}\n' + '\n'.join(lines) + '\n')
            if found is None:
                return None
            values = dict(re.findall(r'(\w+)=([0-9.]+)', found))
            return {'average': float(values['worst_avg']), 'p50': float(values['worst_p50']),
                    'p95': float(values['worst_p95']), 'p99': float(values['worst_p99'])}

        bench.tick_query = folia_query
    else:
        bench.server_command = with_engine
    if '--folia' in sys.argv:
        original_query = bench.tick_query
    bench.tick_query = query
    bench.tree_usage = tree_usage
    if '--profile' in sys.argv:
        # Samples of the server thread once the bots are in, with this build's own symbols.
        import threading
        count = option('--profile', '600')
        bots_n = max(int(b) for b in bots.split(','))
        after = float(option('--profile-after', str(bots_n * float(option('--join-every', '0.3')) + 35)))

        def sample():
            time.sleep(after)
            out = subprocess.run([sys.executable, '-I', str(TREE / 'tools' / 'v2' / 'stacks.py'), '--match', name,
                                  '--profile', count, '--symbols', str(folder), '--lines', '--keep', str(folder / 'stacks.jsonl')],
                                 capture_output=True, text=True, encoding='utf-8', errors='replace')
            (folder / 'profile.txt').write_text(out.stdout + out.stderr, encoding='utf-8')
        threading.Thread(target=sample, daemon=True).start()
    cost = perf_cost.Cost(folder, '--pin' in sys.argv, '--solo' in sys.argv)
    cost.start()
    stamp = time.strftime('%H%M%S')
    sys.argv = ['server_bench', '--servers', servers, '--bots', bots, '--behaviour', option('--behaviour', 'mixed'),
                '--join-every', option('--join-every', '0.3'), '--warmup', option('--warmup', '30'),
                '--seconds', str(seconds), '--gamerule', option('--gamerule', ''), '--out', str(folder / f'report-{stamp}.json'),
                *(['--spread', option('--spread', '')] if '--spread' in sys.argv else [])]
    bench.main()
    if world_to and runs:
        # The last run's world folders (the server saved them as it stopped), for --world-from.
        keep = pathlib.Path(world_to)
        shutil.rmtree(keep, ignore_errors=True)
        keep.mkdir(parents=True)
        for part in pathlib.Path(runs[-1]).iterdir():
            if part.is_dir() and (part / 'level.dat').is_file():
                shutil.copytree(part, keep / part.name)
        print('kept the world in', keep, [p.name for p in keep.iterdir()], flush=True)
    report = json.loads((folder / f'report-{stamp}.json').read_text(encoding='utf-8'))
    cost.report(seconds, report['runs'], folder / f'cost-{stamp}.json')
    for k, directory in enumerate(runs):
        reports = sorted(pathlib.Path(directory, 'reports').glob('perf-*.json'))
        if not reports:
            print('no horizon-report in', directory)
            continue
        out = folder / f'phases-{stamp}-{k}.json'
        shutil.copy2(reports[-1], out)
        data = json.loads(out.read_text(encoding='utf-8'))
        ticks = data.get('ticks', {})
        level = (data.get('levels') or [{}])[0]
        print(f'\n{out.name}: players {data.get("players")}, mspt {ticks.get("mspt")}, cpu/tick {ticks.get("cpu_ms_average")}, '
              f'chunks {level.get("chunks")}, mobs {level.get("mobs")}, items {level.get("item_entities")}, '
              f'rss {data.get("memory", {}).get("rss_mb")} MB, chunk heap {level.get("chunk_heap_mb")} MB, '
              f'pipeline (protos, light views, generating, finishing, decorated, written, lighting, deferred) '
              f'{level.get("pipeline")} {level.get("pipeline_mb")}, encoder cache {level.get("encoder_cache")}, '
              f'chunk parts {level.get("chunk_parts_mb")}')
        for s in data.get('spans', [])[:20]:
            print(f'  {s["name"]:<28} {s["ms_per_tick"]:7.2f} ms  {s["share_of_tick"] * 100:5.1f}%')


if __name__ == '__main__':
    main()
