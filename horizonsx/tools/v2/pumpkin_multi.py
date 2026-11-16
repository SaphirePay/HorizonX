#!/usr/bin/env python3
"""Pumpkin (github.com/Pumpkin-MC/Pumpkin) measured the way multi.py measures HorizonsX: N Pumpkin servers behind
horizon-proxy (Velocity modern forwarding, each server its own forced host 127.0.0.K), the same horizon-bots all in
one chunk (group mode), the same steps, every server's report at once.

    python -I tools/v2/pumpkin_multi.py [--pumpkin EXE] [--bots-from NAME] [--servers 3] [--start 250] [--step 150]
                                        [--max 400] [--settle 20] [--warmup 60] [--behaviour walk] [--bot-threads 3]
                                        [--proxy-workers 0] [--idle-only]

Pumpkin's own /tps is 1000 / MSPT (never measured); the build here carries a `benchreport` console command (patched
into its tps.rs): ticks that really ended in the last 10 s / 10, their average, P95, P99 and max. Settings as
ramp.py gives HorizonsX: seed 12345, creative, view 10, simulation 7, compression 256, spawn protection 0, offline
behind the proxy. Also prints each server's boot time (start to "Done") and idle RAM (no players, 30 s after Done).
"""
import json
import os
import pathlib
import re
import secrets
import shutil
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ramp  # noqa: E402
from multi import cpu_of  # noqa: E402

PERF = ramp.PERF
option = ramp.option


def set_toml(text, section, key, value):
    """TEXT with KEY in [SECTION] ('' = the top) set to VALUE (a TOML literal); added if missing."""
    out, current, done = [], '', False
    lines = text.splitlines()
    for i, line in enumerate(lines):
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
        self.exe, self.dir, self.port = exe, directory, port
        self.proc = None
        self.lock = threading.Lock()

    def log(self):
        return (self.dir / 'out.log').read_text(encoding='utf-8', errors='replace')

    def start(self):
        self.out = open(self.dir / 'out.log', 'w', encoding='utf-8')
        t0 = time.time()
        self.proc = subprocess.Popen([str(self.exe)], cwd=self.dir, stdin=subprocess.PIPE, stdout=self.out,
                                     stderr=subprocess.STDOUT, env={**os.environ, 'NO_COLOR': '1'})
        for _ in range(1200):
            if 'Done (' in self.log() or 'Started server' in self.log() or 'Server is now running' in self.log():
                return time.time() - t0
            if self.proc.poll() is not None:
                sys.exit('pumpkin exited:\n' + self.log()[-2000:])
            time.sleep(0.1)
        sys.exit('pumpkin did not start:\n' + self.log()[-2000:])

    def cmd(self, line):
        with self.lock:
            self.proc.stdin.write((line + '\n').encode())
            self.proc.stdin.flush()

    def report(self):
        before = self.log().count('bench: ')
        self.cmd('benchreport')
        for _ in range(100):
            lines = [x for x in self.log().splitlines() if 'bench: ' in x]
            if len(lines) > before:
                line = lines[-1]
                nums = {}
                for key, pattern in (('players', r'(\d+) players'), ('tps', r'TPS ([\d.]+)'), ('mspt', r'MSPT average ([\d.]+)'),
                                     ('p95', r'P95 ([\d.]+)'), ('p99', r'P99 ([\d.]+)'), ('max', r'max ([\d.]+)')):
                    m = re.search(pattern, line)
                    if m:
                        nums[key] = float(m.group(1))
                return nums, line
            time.sleep(0.2)
        return {}, ''

    def stop(self):
        if self.proc and self.proc.poll() is None:
            try:
                self.cmd('stop')
                self.proc.wait(60)
            except Exception:  # noqa: BLE001
                self.proc.kill()


def main():
    exe = pathlib.Path(option('--pumpkin', r'D:\Horizon\bench\pumpkin\target\fast\pumpkin.exe'))
    bots_dir = PERF / option('--bots-from', 'hxm1')
    count = int(option('--servers', '3'))
    start, step, most = int(option('--start', '250')), int(option('--step', '150')), int(option('--max', '400'))
    settle, warmup = int(option('--settle', '20')), int(option('--warmup', '60'))
    behaviour = option('--behaviour', 'walk')
    bot_threads = int(option('--bot-threads', '3'))
    rust_proxy = option('--rust-proxy', str(pathlib.Path(__file__).resolve().parents[2] / 'target' / 'fast' / 'horizon-proxy.exe'))
    root = PERF / '.ramp'
    secret = secrets.token_hex(16)
    # --direct: no proxy, the bots straight to each server (proxy forwarding off).
    direct = '--direct' in sys.argv

    servers = []
    for k in range(count):
        d = root / f'pk-s{k + 1}'
        shutil.rmtree(d, ignore_errors=True)
        d.mkdir(parents=True)
        port = ramp.free_port()
        s = Pumpkin(exe, d, port)
        # First run writes the full pumpkin.toml; then ours set in it.
        s.start()
        s.stop()
        for f in d.iterdir():
            if f.is_dir() and f.name == 'world':
                shutil.rmtree(f)
        text = (d / 'pumpkin.toml').read_text(encoding='utf-8')
        for section, key, value in (
                ('', 'seed', '"12345"'), ('', 'default_gamemode', '"Creative"'), ('', 'spawn_protection', '0'),
                ('networking.java', 'address', f'"0.0.0.0:{port}"'), ('networking.java', 'online_mode', 'false'),
                ('networking.java', 'encryption', 'false'), ('networking.java', 'max_players', '1000'),
                ('networking.java', 'view_distance', '10'), ('networking.java', 'simulation_distance', '7'),
                ('networking.java.compression.info', 'threshold', '256'),
                # The bots and the proxy come from this machine: HorizonsX has no packet limiter in these runs either.
                ('networking.java.packet_limiter', 'enabled', 'false'),
                ('networking.proxy', 'enabled', 'false' if direct else 'true'),
                ('networking.proxy.velocity', 'enabled', 'false' if direct else 'true'), ('networking.proxy.velocity', 'secret', f'"{secret}"'),
                ('networking.query', 'enabled', 'false'), ('networking.lan_broadcast', 'enabled', 'false'),
                ('networking.bedrock', 'enabled', 'false')):
            text = set_toml(text, section, key, value)
        (d / 'pumpkin.toml').write_text(text, encoding='utf-8')
        boot = s.start()
        time.sleep(30)
        idle = cpu_of([s.proc.pid]).get(s.proc.pid, (0, 0))[1]
        s.boot, s.idle = boot, idle
        print(f'pumpkin s{k + 1} on {port}: boot {boot:.2f} s (fresh world), idle RAM {idle:.0f} MB', flush=True)
        servers.append(s)
    if '--idle-only' in sys.argv:
        for s in servers:
            s.stop()
        return

    vdir = root / 'pk-rustproxy'
    proxy = None
    shutil.rmtree(vdir, ignore_errors=True)
    vdir.mkdir(parents=True)
    proxy_port = ramp.free_port()
    nl = chr(10)
    conf = (f'bind = "0.0.0.0:{proxy_port}"' + nl + 'motd = "Pumpkin test network"' + nl + 'max_players = 100000' + nl
            + f"workers = {option('--proxy-workers', '0')}" + nl + 'forwarding_secret_file = "forwarding.secret"' + nl
            + 'buffer = 32768' + nl + 'online_mode = false' + nl + 'try = ["s1"]' + nl + nl + '[servers]' + nl
            + ''.join(f's{k + 1} = "127.0.0.1:{s.port}"' + nl for k, s in enumerate(servers))
            + nl + '[forced_hosts]' + nl + ''.join(f'"127.0.0.{k + 1}" = "s{k + 1}"' + nl for k in range(count)))
    (vdir / 'proxy.toml').write_text(conf, encoding='utf-8')
    (vdir / 'forwarding.secret').write_text(secret, encoding='utf-8')
    if not direct:
        proxy = subprocess.Popen([rust_proxy, str(vdir / 'proxy.toml')], cwd=vdir, stdin=subprocess.DEVNULL,
                                 stdout=open(vdir / 'proxy.log', 'w', encoding='utf-8'), stderr=subprocess.STDOUT)
        for _ in range(60):
            if 'Done' in (vdir / 'proxy.log').read_text(encoding='utf-8', errors='replace'):
                break
            time.sleep(0.5)
        else:
            sys.exit('the proxy did not start')
        print(f'horizon-proxy on {proxy_port} in front of {count} Pumpkin servers', flush=True)
    else:
        print(f'no proxy: bots straight to each of {count} Pumpkin servers', flush=True)

    total_threads = os.cpu_count() or 1
    mask = sum(1 << i for i in range(total_threads - bot_threads, total_threads))
    procs, steps, per = [], [], 0
    out = PERF / f'pumpkin-multi-{time.strftime("%H%M%S")}.json'
    try:
        target = start
        while target <= most:
            add = target - per
            for k, s in enumerate(servers):
                p = subprocess.Popen(
                    [str(bots_dir / 'horizon-bots.exe'), '--server', f'127.0.0.1:{s.port}' if direct else f'127.0.0.{k + 1}:{proxy_port}', '--bots', str(add),
                     '--name', f'P{k + 1}x{len(procs)}_', '--behaviour', behaviour, '--join-every', '0.05',
                     '--seconds', '100000', '--report', 'NUL'],
                    stdout=open(root / f'pk-bots-s{k + 1}-{len(procs)}.jsonl', 'w'), stderr=subprocess.DEVNULL)
                subprocess.run(['powershell', '-NoProfile', '-Command', f'(Get-Process -Id {p.pid}).ProcessorAffinity = {mask}'],
                               capture_output=True, timeout=30)
                procs.append(p)
            time.sleep(add * 0.05 + 4)
            per = target
            time.sleep(settle)
            if not steps and warmup:
                print(f'warm-up {warmup} s', flush=True)
                time.sleep(warmup)
            for s in servers:
                s.cmd('tp @a 8 140 8')
            time.sleep(3)
            watch = [s.proc.pid for s in servers] + ([proxy.pid] if proxy else []) + [p.pid for p in procs]
            a0 = time.time()
            before = cpu_of(watch)
            t0 = (a0 + time.time()) / 2
            time.sleep(10)
            results = [s.report() for s in servers]
            b0 = time.time()
            after = cpu_of(watch)
            wall = max((b0 + time.time()) / 2 - t0, 0.001)

            def cores(pids):
                return sum(after[p][0] - before[p][0] for p in pids if p in after and p in before) / wall
            row = {'per_server': per, 'servers': []}
            print(f'--- {per} bots a server, {per * count} in all', flush=True)
            for k, s in enumerate(servers):
                nums, line = results[k]
                c = cores([s.proc.pid])
                nums.update({'cores': round(c, 2), 'cpu_pct': round(100 * c / total_threads, 1),
                             'rss_mb': round(after.get(s.proc.pid, (0, 0))[1])})
                row['servers'].append(nums)
                print(f"  s{k + 1}: players {nums.get('players')}  TPS {nums.get('tps')}  MSPT {nums.get('mspt')}  P95 {nums.get('p95')}  "
                      f"max {nums.get('max')}  CPU {nums['cores']} cores ({nums['cpu_pct']}%)  RAM {nums['rss_mb']} MB", flush=True)
            row['proxy_cores'] = round(cores([proxy.pid]), 2) if proxy else 0.0
            row['bots_cores'] = round(cores([p.pid for p in procs]), 2)
            print(f"  proxy CPU {row['proxy_cores']} cores, bots CPU {row['bots_cores']} cores", flush=True)
            steps.append(row)
            out.write_text(json.dumps({'servers': count, 'boot_s': [s.boot for s in servers],
                                       'idle_mb': [s.idle for s in servers], 'steps': steps}, indent=1), encoding='utf-8')
            if min(x.get('tps', 0) for x in row['servers']) < float(option('--floor', '5')):
                break
            target += step
    finally:
        for p in procs:
            p.kill()
        if proxy:
            proxy.kill()
        for s in servers:
            s.stop()
    print(f'wrote {out}')


if __name__ == '__main__':
    main()
