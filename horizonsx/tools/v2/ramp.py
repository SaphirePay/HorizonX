#!/usr/bin/env python3
"""HorizonsX's player ramp: the most players one server keeps above a TPS floor, in one run.

One server (the frozen build in D:/Horizon/perf/NAME, no plugins, no RAM cap), and bots added STEP at a time; after
each step the bots settle, then the server's own report measures TPS and MSPT. The ramp stops at the first step
under the floor (or when bots stop getting in) and prints every step.

Usage: python -I tools/v2/ramp.py NAME [--mode spread|group] [--start 50] [--step 25] [--max 1000]
                                  [--floor 15] [--settle 8] [--measure 10] [--warmup 60] [--spacing 512] [--group 1] [--bot-threads 2]
                                  [--world-from DIR] [--behaviour walk] [--resources DIR] [--env K=V] [--prop KEY=VALUE]

  spread  each bot at its own home, homes on a grid SPACING blocks apart growing out from 0,0 (a survival server)
  scatter each bot its own random home (seeded) in a square SPACING x sqrt(MAX) blocks wide around 0,0 (an SMP)
  group   every bot in one chunk (chunk 0,0: 16 spots 4 blocks apart), sent back there before each measure

Writes D:/Horizon/perf/NAME/ramp-MODE-TIME.json.
"""
import json
import os
import pathlib
import re
import shutil
import socket
import subprocess
import sys
import time

PERF = pathlib.Path(r'D:\Horizon\perf')
RESOURCES = pathlib.Path(os.environ['APPDATA']) / 'horizon-launcher' / 'server-runtime' / 'game-files' / 'horizon-client' / '26.3'


def option(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


def spiral(n):
    """The first n points of the square grid ordered by ring (Chebyshev distance), then angle: each step's homes are
    next to the last step's, so the area in use grows evenly from 0,0."""
    import math
    r, points = 0, [(0, 0)]
    while len(points) < n:
        r += 1
        ring = [(x, z) for x in range(-r, r + 1) for z in range(-r, r + 1) if max(abs(x), abs(z)) == r]
        ring.sort(key=lambda p: math.atan2(p[1], p[0]))
        points.extend(ring)
    return points[:n]


class Server:
    def __init__(self, exe_dir, directory, port, resources):
        self.hs = str(exe_dir / 'horizon-server.exe')
        self.dir = directory
        self.engine = exe_dir / 'horizon-client.exe'
        self.port = port
        self.resources = resources

    def run(self, *args, timeout=120):
        r = subprocess.run([self.hs, '--dir', str(self.dir), *map(str, args)], capture_output=True, text=True,
                           timeout=timeout, encoding='utf-8', errors='replace')
        return (r.stdout + r.stderr).strip()

    def log(self):
        p = self.dir / 'logs' / 'latest.log'
        return p.read_text(encoding='utf-8', errors='replace') if p.exists() else ''

    def start(self, world_from):
        # A server stopping a moment ago may still hold its files: try again for a while.
        for _ in range(30):
            shutil.rmtree(self.dir, ignore_errors=True)
            if not self.dir.exists():
                break
            time.sleep(1)
        self.dir.mkdir(parents=True, exist_ok=True)
        print(self.run('init', '--eula', '--world', 'world', '--seed', '12345', '--port', self.port, '--no-whitelist',
                       '--offline', '--yes')[-120:], flush=True)
        if world_from:
            shutil.rmtree(self.dir / 'world', ignore_errors=True)
            shutil.copytree(world_from, self.dir / 'world')
        said = self.run('config', 'set', f'engine.path={self.engine}', f'engine.resources={self.resources}', 'panel.bind=',
                        'world.game_mode=creative', 'server.max_players=1000', 'properties.spawn-protection=0',
                        'server.view_distance=10', f"server.simulation_distance={option('--sim', '7')}",
                        'properties.network-compression-threshold=256',
                        # --prop KEY=VALUE (repeatable): any other server.properties key, e.g. horizon-ticks-per-monster-spawns=2.
                        *[f'properties.{sys.argv[i + 1]}' for i, w in enumerate(sys.argv) if w == '--prop'])
        print(said[-160:], flush=True)
        if 'horizon-server:' in said:
            sys.exit('the settings were refused: ' + said[-400:])
        # The built-in anticheat off unless --anticheat: the bots' simple walking fails its movement prediction
        # thousands of times a minute (300k log lines a run), work real players don't make (owner 11-06).
        toml = self.dir / 'server.toml'
        if '--anticheat' not in sys.argv and '\n[anticheat]' not in toml.read_text(encoding='utf-8'):
            with toml.open('a', encoding='utf-8', newline='\n') as f:
                f.write('\n[anticheat]\nenabled = false\n')
        # Not captured: the background server keeps the pipe open, and a captured run would wait on it forever.
        subprocess.run([self.hs, '--dir', str(self.dir), 'start'], stdin=subprocess.DEVNULL,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120)
        supervisor = self.dir / 'logs' / 'supervisor.log'
        for _ in range(240):
            if 'Done (' in self.log():
                return
            text = supervisor.read_text(encoding='utf-8', errors='replace') if supervisor.exists() else ''
            if 'the server stopped' in text or 'no prepared game files' in text:
                sys.exit('the server did not start:\n' + text[-1500:])
            time.sleep(1)
        sys.exit('the server did not start:\n' + self.log()[-2000:])

    def status(self):
        try:
            return json.loads(self.run('status', '--json'))
        except (json.JSONDecodeError, subprocess.TimeoutExpired):
            return {}

    def report(self, seconds):
        out = self.run('report', seconds, timeout=seconds + 300)
        line = next((l for l in out.splitlines() if 'TPS' in l and 'MSPT' in l), '')
        nums = {}
        for key, pattern in (('players', r'(\d+) players'), ('tps', r'TPS ([\d.]+)'), ('mspt', r'MSPT average ([\d.]+)'),
                             ('p95', r'P95 ([\d.]+)'), ('p99', r'P99 ([\d.]+)'), ('max', r'max ([\d.]+)')):
            m = re.search(pattern, line)
            if m:
                nums[key] = float(m.group(1))
        per_tick = next((l for l in out.splitlines() if l.startswith('per tick')), '')
        # The whole report (every phase, not only the line's busiest) beside the run's records.
        self.last_report = out
        return nums, line, per_tick

    def stop(self):
        # --quick: the test's world is thrown away, so no save: its processes are ended (a save of 600 players'
        # chunks took most of a minute).
        if '--quick' in sys.argv:
            subprocess.run(['powershell', '-NoProfile', '-Command',
                            "Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -and $_.CommandLine.Contains('"
                            + str(self.dir) + "') } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }"],
                           capture_output=True, timeout=60)
            return
        self.run('stop', timeout=120)


def pin_bots(pid):
    """The bots on the machine's last BOT_THREADS logical processors (default 2, `--bot-threads`), so the server has the
    others to itself: what the bots cost never comes out of the server's measure. 0 leaves them unpinned."""
    threads = int(option('--bot-threads', '2'))
    total = os.cpu_count() or 1
    if threads <= 0 or threads >= total:
        return
    mask = sum(1 << i for i in range(total - threads, total))
    subprocess.run(['powershell', '-NoProfile', '-Command', f'(Get-Process -Id {pid}).ProcessorAffinity = {mask}'],
                   capture_output=True, timeout=30)


def engine_cpu_seconds(tag):
    """CPU seconds (user + kernel) of the engine whose command line names the ramp folder TAG, or None."""
    try:
        out = subprocess.run(['powershell', '-NoProfile', '-Command',
                              "Get-CimInstance Win32_Process -Filter \"Name='horizon-client.exe'\" | "
                              "Select-Object KernelModeTime,UserModeTime,CommandLine | ConvertTo-Json"],
                             capture_output=True, text=True, timeout=30).stdout
        rows = json.loads(out or '[]')
        rows = rows if isinstance(rows, list) else [rows]
        mine = [r for r in rows if r.get('CommandLine') and ('.ramp' + os.sep + tag) in r['CommandLine']]
        return sum((r['KernelModeTime'] + r['UserModeTime']) / 1e7 for r in mine) if mine else None
    except Exception:  # noqa: BLE001
        return None


def private_mb(threads=False):
    """The engine's private memory (commit charge): what it holds, trimmed working set or not. With THREADS:
    (private MB, thread count)."""
    try:
        out = subprocess.run(['powershell', '-NoProfile', '-Command',
                              "Get-CimInstance Win32_Process -Filter \"Name='horizon-client.exe'\" | "
                              "Select-Object PrivatePageCount,ThreadCount,CommandLine | ConvertTo-Json"],
                             capture_output=True, text=True, timeout=30).stdout
        rows = json.loads(out or '[]')
        rows = rows if isinstance(rows, list) else [rows]
        mine = [r for r in rows if r.get('CommandLine') and '.ramp' in r['CommandLine']]
        mb = round(sum(int(r['PrivatePageCount']) for r in mine) / 2**20) if mine else None
        return (mb, sum(int(r.get('ThreadCount') or 0) for r in mine)) if threads else mb
    except Exception:  # noqa: BLE001
        return (None, None) if threads else None


def bot_clock(path):
    """The server's clock as the bots last heard it (horizon-bots' per-second line): (game_time, ms it came in), or
    None. Two readings make a TPS seen from outside the server, the same way for any server."""
    try:
        lines = pathlib.Path(path).read_text(encoding='utf-8', errors='replace').splitlines()
    except OSError:
        return None
    for line in reversed(lines):
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get('game_time', -1) > 0:
            return row['game_time'], row['game_time_ms']
    return None


def clock_tps(before, after):
    """Ticks per second between two bot_clock readings."""
    if not before or not after or after[1] <= before[1]:
        return None
    return round((after[0] - before[0]) * 1000 / (after[1] - before[1]), 2)


def rss_mb(port):
    """The engine's RSS: the horizon-client listening on our port (Windows: by its command line's server dir)."""
    try:
        out = subprocess.run(['powershell', '-NoProfile', '-Command',
                              "Get-CimInstance Win32_Process -Filter \"Name='horizon-client.exe'\" | "
                              "Select-Object ProcessId,WorkingSetSize,CommandLine | ConvertTo-Json"],
                             capture_output=True, text=True, timeout=30).stdout
        rows = json.loads(out or '[]')
        rows = rows if isinstance(rows, list) else [rows]
        mine = [r for r in rows if r.get('CommandLine') and '.ramp' in r['CommandLine']]
        return round(sum(r['WorkingSetSize'] for r in mine) / 2**20) if mine else None
    except Exception:  # noqa: BLE001
        return None


def main():
    name = sys.argv[1]
    exe_dir = PERF / name
    mode = option('--mode', 'spread')
    start, step, most = int(option('--start', '50')), int(option('--step', '25')), int(option('--max', '1000'))
    floor, settle, measure = float(option('--floor', '15')), int(option('--settle', '20')), int(option('--measure', '15'))
    spacing, behaviour = int(option('--spacing', '512')), option('--behaviour', 'walk')
    warmup = int(option('--warmup', '60'))
    group = max(1, int(option('--group', '1')))
    world_from = option('--world-from', None)
    resources = pathlib.Path(option('--resources', str(RESOURCES)))
    for i, word in enumerate(sys.argv):
        if word == '--env':
            k, v = sys.argv[i + 1].split('=', 1)
            os.environ[k] = v
    port = free_port()
    server = Server(exe_dir, PERF / '.ramp' / f'{name}-{mode}', port, resources)
    server.start(world_from)
    bots, procs, steps = 0, [], []
    homes = spiral(most + step)
    # scatter: every bot a home of its own at a random place (seeded, the same each run) in a square whose side
    # grows with the most bots asked for, SPACING blocks apart on average: loners, pairs and clusters, as an SMP's
    # players spread. The square is centred on 0,0 and must lie inside the world given with --world-from.
    import random
    side = spacing * max(1, int((most + step) ** 0.5))
    rng = random.Random(int(option('--seed', '7')))
    scatter = [(rng.uniform(-side / 2, side / 2), rng.uniform(-side / 2, side / 2)) for _ in range(most + step)]
    stamp = time.strftime('%H%M%S')
    out = exe_dir / f'ramp-{mode}-{stamp}.json'
    try:
        target = start
        while target <= most:
            add = target - bots
            prefix = f'R{len(procs)}_'
            bot_log = server.dir.parent / f'{server.dir.name}-bots-{prefix}.jsonl'
            procs.append(subprocess.Popen(
                [str(exe_dir / 'horizon-bots.exe'), '--server', f'127.0.0.1:{port}', '--bots', str(add), '--name', prefix,
                 '--behaviour', behaviour, '--join-every', '0.05', '--seconds', '100000', '--report', 'NUL'],
                stdout=open(bot_log, 'w', encoding='utf-8'), stderr=subprocess.DEVNULL))
            bot_logs = [bot_log] if len(procs) == 1 else bot_logs
            pin_bots(procs[-1].pid)
            names = [f'{prefix}{i}' for i in range(add)]
            time.sleep(add * 0.05 + 2)
            # Where the new bots live.
            for _ in range(1):
                for k, bot in enumerate(names):
                    if mode == 'scatter':
                        x, z = (int(c) for c in scatter[bots + k])
                    elif mode == 'spread':
                        # GROUP bots a home (bench.py's --spread 512:5 is homes of 5), a few blocks apart there.
                        j = bots + k
                        hx, hz = homes[j // group]
                        x, z = hx * spacing + (j % group % 3) * 4, hz * spacing + (j % group // 3) * 4
                    else:
                        j = bots + k
                        x, z = (j % 4) * 4 + 2, (j // 4 % 4) * 4 + 2
                    server.run('cmd', 'tp', bot, x, 140, z, timeout=30)
                time.sleep(0.5)
            first = bots == 0
            bots = target
            time.sleep(settle)
            if first and warmup:
                # Once, after the first step: fresh land's mob caps fill and its chunks load (steady state), as a
                # server's players' areas are; later steps add their few bots to a settled world.
                print(f'warm-up {warmup} s', flush=True)
                time.sleep(warmup)
            # The bots in before the measure (spread bots load their homes from the save: far homes took longer
            # than any fixed settle, and a measure with a fifth of them in said nothing). Up to --join-wait s.
            waited, join_wait = 0, int(option('--join-wait', '180'))
            while waited < join_wait:
                # The report's own count: players whose world has loaded (the status lists every connection).
                nums_now, _, _ = server.report(3)
                count = nums_now.get('players')
                if isinstance(count, (int, float)) and count >= bots * 0.95:
                    break
                waited += 3
            if waited:
                print(f'waited {waited} s for the bots to be in', flush=True)
            if mode == 'group':
                # Walking bots drift: all back into the chunk before the measure.
                server.run('cmd', 'tp', '@a', 8, 140, 8, timeout=60)
                time.sleep(3)
            tag = server.dir.name
            cpu0, t0 = engine_cpu_seconds(tag), time.time()
            clock0 = bot_clock(bot_logs[0])
            nums, line, per_tick = server.report(measure)
            (PERF / name / f'report-{mode}-{bots}.txt').write_text(getattr(server, 'last_report', ''), encoding='utf-8')
            cpu1, t1 = engine_cpu_seconds(tag), time.time()
            # The same TPS seen from the bots (the server's clock packets), as pumpkin_ramp.py measures Pumpkin's.
            nums['tps_clock'] = clock_tps(clock0, bot_clock(bot_logs[0]))
            if cpu0 is not None and cpu1 is not None:
                cores = (cpu1 - cpu0) / max(t1 - t0, 0.001)
                nums['cores'] = round(cores, 2)
                nums['cpu_pct'] = round(100 * cores / (os.cpu_count() or 1), 1)
            nums['bots'] = bots
            nums['rss_mb'] = rss_mb(port)
            nums['private_mb'], nums['threads'] = private_mb(threads=True)
            # The report's own count of players in the game.
            nums['online'] = nums.get('players')
            steps.append({**nums, 'line': line, 'per_tick': per_tick})
            print(f"{bots:5d} bots  online {nums.get('online')}  TPS {nums.get('tps')}  MSPT {nums.get('mspt')}  "
                  f"P95 {nums.get('p95')}  P99 {nums.get('p99')}  max {nums.get('max')}  RSS {nums.get('rss_mb')} MB  private {nums.get('private_mb')} MB  threads {nums.get('threads')}  "
                  f"CPU {nums.get('cores')} cores ({nums.get('cpu_pct')}%)  clock TPS {nums.get('tps_clock')}", flush=True)
            if per_tick:
                print('       ' + per_tick[:200], flush=True)
            out.write_text(json.dumps({'mode': mode, 'build': name, 'steps': steps}, indent=1), encoding='utf-8')
            if nums.get('tps', 0) < floor:
                break
            online = nums.get('online')
            if isinstance(online, (int, float)) and online < bots * 0.9:
                print(f'only {online} of {bots} bots in: stopping', flush=True)
                break
            target += step
    finally:
        for p in procs:
            p.kill()
        server.stop()
    keep = [s for s in steps if s.get('tps', 0) >= floor]
    best = keep[-1]['bots'] if keep else 0
    print(f'\n{mode}: most bots at or above {floor} TPS: {best} (steps of {step}); wrote {out}')


if __name__ == '__main__':
    main()
