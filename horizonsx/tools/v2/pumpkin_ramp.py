#!/usr/bin/env python3
"""Pumpkin (pumpkinmc.org, the official release binary) run through ramp.py's spread test, for a side-by-side.

    python -I tools/v2/pumpkin_ramp.py --exe D:/Horizon/bench/pumpkin-0.2.2/pumpkin-X64-v3-Windows.exe
        [--bots-from hxcmp] [--world-from D:/Horizon/perf/worlds/w2800/world] [--start 600] [--step 50] [--max 600]
        [--group 5] [--spacing 512] [--floor 16] [--settle 10] [--warmup 10] [--measure 25] [--join-wait 300] [--bot-threads 4]

The same as ramp.py gives HorizonsX: the same pre-generated world (Anvil, `world.chunk.type = "anvil"`), the same
horizon-bots (walking), GROUP bots a home, homes SPACING blocks apart on the same spiral, each bot teleported home
from the console, view 10, simulation 7, creative, spawn protection 0, offline, compression threshold 256, no
packet limiter. Pumpkin's telemetry, Bedrock and LAN broadcast are off.

TPS is read from outside, as ramp.py's `tps_clock` for HorizonsX: the game time the server sends the bots
(ClientboundSetTimePacket) over wall time, across the measure. Pumpkin has no tick-time report of its own (its /tps
is 1000 / MSPT), so no MSPT or P99 for it. CPU is the process's CPU time over the measure; memory is its working set
and its private bytes at the end of the measure.

Writes D:/Horizon/perf/pumpkin-ramp-TIME.json.
"""
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ramp  # noqa: E402

option = ramp.option


def set_toml(text, section, key, value):
    """TEXT with KEY in [SECTION] ('' = the top) set to VALUE (a TOML literal); added if missing."""
    out, current, done = [], '', False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith('[') and stripped.endswith(']'):
            if current == section and not done:
                out.append(f'{key} = {value}')
                done = True
            current = stripped.strip('[]')
        elif current == section and re.match(rf'^{re.escape(key)}\s*=', stripped) and not done:
            out.append(f'{key} = {value}')
            done = True
            continue
        out.append(line)
    if not done:
        if current != section:
            out += ['', f'[{section}]'] if section else []
        out.append(f'{key} = {value}')
    return '\n'.join(out) + '\n'


class Pumpkin:
    def __init__(self, exe, directory, port):
        self.exe, self.dir, self.port, self.proc = exe, directory, port, None

    def log(self):
        p = self.dir / 'out.log'
        return p.read_text(encoding='utf-8', errors='replace') if p.exists() else ''

    def start(self):
        self.out = open(self.dir / 'out.log', 'w', encoding='utf-8')
        t0 = time.time()
        self.proc = subprocess.Popen([str(self.exe)], cwd=self.dir, stdin=subprocess.PIPE, stdout=self.out,
                                     stderr=subprocess.STDOUT, env={**os.environ, 'NO_COLOR': '1'})
        for _ in range(3000):
            if 'Server is now running' in self.log():
                return time.time() - t0
            if self.proc.poll() is not None:
                sys.exit('pumpkin exited:\n' + self.log()[-2000:])
            time.sleep(0.1)
        sys.exit('pumpkin did not start:\n' + self.log()[-2000:])

    def cmd(self, line):
        self.proc.stdin.write((line + '\n').encode())
        self.proc.stdin.flush()

    def stop(self):
        if self.proc and self.proc.poll() is None:
            try:
                self.cmd('stop')
                self.proc.wait(90)
            except Exception:  # noqa: BLE001
                self.proc.kill()
        self.out.close()


def process(pid):
    """(CPU seconds, working set MB, private MB) of a process."""
    out = subprocess.run(['powershell', '-NoProfile', '-Command',
                          f'$p = Get-Process -Id {pid}; "$($p.TotalProcessorTime.TotalSeconds) $($p.WorkingSet64) '
                          f'$($p.PrivateMemorySize64) $($p.Threads.Count)"'],
                         capture_output=True, text=True, timeout=30).stdout.split()
    cpu, ws, private, threads = float(out[0]), int(out[1]), int(out[2]), int(out[3])
    return cpu, round(ws / 2**20), round(private / 2**20), threads


def online(path):
    """Bots online in the newest per-second line of a horizon-bots log."""
    for line in reversed(pathlib.Path(path).read_text(encoding='utf-8', errors='replace').splitlines()):
        try:
            return json.loads(line).get('online', 0)
        except json.JSONDecodeError:
            continue
    return 0


def main():
    exe = pathlib.Path(option('--exe', r'D:\Horizon\bench\pumpkin-0.2.2\pumpkin-X64-v3-Windows.exe'))
    bots_exe = ramp.PERF / option('--bots-from', 'hxcmp') / 'horizon-bots.exe'
    world_from = pathlib.Path(option('--world-from', r'D:\Horizon\perf\worlds\w2800\world'))
    start, step, most = int(option('--start', '600')), int(option('--step', '50')), int(option('--max', '600'))
    group, spacing = int(option('--group', '5')), int(option('--spacing', '512'))
    settle, warmup, measure = int(option('--settle', '10')), int(option('--warmup', '10')), int(option('--measure', '25'))
    join_wait = int(option('--join-wait', '300'))
    floor = float(option('--floor', '0'))
    d = ramp.PERF / '.ramp' / 'pumpkin-spread'
    for _ in range(30):
        shutil.rmtree(d, ignore_errors=True)
        if not d.exists():
            break
        time.sleep(1)
    d.mkdir(parents=True)
    port = ramp.free_port()
    server = Pumpkin(exe, d, port)
    # The first start writes pumpkin.toml (and a fresh world, replaced by the test's).
    server.start()
    server.stop()
    shutil.rmtree(d / 'world', ignore_errors=True)
    shutil.copytree(world_from, d / 'world')
    for p in (d / 'world').rglob('session.lock'):
        p.unlink()
    text = (d / 'pumpkin.toml').read_text(encoding='utf-8')
    for section, key, value in (
            ('', 'seed', '"12345"'), ('', 'default_gamemode', '"Creative"'), ('', 'spawn_protection', '0'),
            ('world.chunk', 'type', '"anvil"'),
            ('networking.java', 'address', f'"0.0.0.0:{port}"'), ('networking.java', 'online_mode', 'false'),
            ('networking.java', 'encryption', 'false'), ('networking.java', 'max_players', '1000'),
            ('networking.java', 'view_distance', '10'), ('networking.java', 'simulation_distance', '7'),
            ('networking.java.compression', 'threshold', '256'),
            ('networking.java.packet_limiter', 'enabled', 'false'),
            ('networking.java.authentication', 'enabled', 'false'),
            ('networking.bedrock', 'enabled', 'false'), ('networking.lan_broadcast', 'enabled', 'false'),
            ('networking.query', 'enabled', 'false'), ('telemetry', 'enabled', 'false')):
        text = set_toml(text, section, key, value)
    (d / 'pumpkin.toml').write_text(text, encoding='utf-8')
    boot = server.start()
    time.sleep(5)
    idle = process(server.proc.pid)
    print(f'pumpkin {exe.name} on {port}: started in {boot:.2f} s, idle {idle[1]} MB working set, {idle[2]} MB private',
          flush=True)
    homes = ramp.spiral(most + step)
    bots, procs, steps, logs = 0, [], [], []
    out = ramp.PERF / f'pumpkin-ramp-{time.strftime("%H%M%S")}.json'
    try:
        target = start
        while target <= most:
            add = target - bots
            prefix = f'R{len(procs)}_'
            log = d.parent / f'pumpkin-spread-bots-{prefix}.jsonl'
            logs.append(log)
            procs.append(subprocess.Popen(
                [str(bots_exe), '--server', f'127.0.0.1:{port}', '--bots', str(add), '--name', prefix,
                 '--behaviour', 'walk', '--join-every', '0.05', '--seconds', '100000', '--report', 'NUL'],
                stdout=open(log, 'w', encoding='utf-8'), stderr=subprocess.DEVNULL))
            ramp.pin_bots(procs[-1].pid)
            time.sleep(add * 0.05 + 2)
            for k in range(add):
                j = bots + k
                hx, hz = homes[j // group]
                x, z = hx * spacing + (j % group % 3) * 4, hz * spacing + (j % group // 3) * 4
                server.cmd(f'tp {prefix}{k} {x} 140 {z}')
                time.sleep(0.01)
            time.sleep(0.5)
            first = bots == 0
            bots = target
            time.sleep(settle)
            if first and warmup:
                print(f'warm-up {warmup} s', flush=True)
                time.sleep(warmup)
            waited = 0
            while waited < join_wait and sum(online(x) for x in logs) < bots * 0.95:
                time.sleep(3)
                waited += 3
            if waited:
                print(f'waited {waited} s for the bots to be in', flush=True)
            cpu0, t0 = process(server.proc.pid)[0], time.time()
            clock0 = ramp.bot_clock(logs[0])
            time.sleep(measure)
            cpu1, ws, private, threads = process(server.proc.pid)
            t1 = time.time()
            clock1 = ramp.bot_clock(logs[0])
            in_now = sum(online(x) for x in logs)
            cores = (cpu1 - cpu0) / max(t1 - t0, 0.001)
            row = {'players': bots, 'online': in_now, 'tps_clock': ramp.clock_tps(clock0, clock1),
                   'cores': round(cores, 2), 'cpu_pct': round(100 * cores / (os.cpu_count() or 1), 1),
                   'rss_mb': ws, 'private_mb': private, 'threads': threads,
                   'mb_per_player': round(private / max(in_now, 1), 2)}
            steps.append(row)
            print(f"{bots:5d} bots  online {in_now}  clock TPS {row['tps_clock']}  RSS {ws} MB  private {private} MB  "
                  f"({row['mb_per_player']} MB a player)  CPU {row['cores']} cores ({row['cpu_pct']}%)  threads {threads}",
                  flush=True)
            out.write_text(json.dumps({'server': 'pumpkin', 'exe': str(exe), 'boot_s': round(boot, 2),
                                       'idle_rss_mb': idle[1], 'idle_private_mb': idle[2], 'steps': steps}, indent=1),
                           encoding='utf-8')
            if server.proc.poll() is not None:
                print('pumpkin exited during the run:\n' + server.log()[-1500:], flush=True)
                break
            if floor and (row['tps_clock'] or 0) < floor:
                print(f'under {floor} TPS at {bots} players: stopping', flush=True)
                break
            target += step
    finally:
        for p in procs:
            p.kill()
        server.stop()
    print(f'wrote {out}')


if __name__ == '__main__':
    main()
