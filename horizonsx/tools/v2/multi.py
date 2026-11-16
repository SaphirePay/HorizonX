#!/usr/bin/env python3
"""A HorizonsX network on this machine: a Velocity proxy in front of N HorizonsX servers, bots joining through the
proxy (each server its own forced host: 127.0.0.K -> server K), every server measured at once. The players a server
holds at a steady 20 TPS while all the others hold theirs ("the sweet spot") and the network's total.

Usage: python -I tools/v2/multi.py NAME [--servers 3] [--start 200] [--step 25] [--max 1000] [--sweet 19.5]
                                   [--floor 15] [--settle 20] [--measure 10] [--warmup 60] [--behaviour walk]
                                   [--bot-threads 3] [--velocity JAR] [--java EXE] [--direct] [--threads N] [--pin N]
  --direct  no proxy: bots straight to each server (what each server sees with the proxy on another machine)
  --proxy rust   HorizonsX's own proxy (target/fast/horizon-proxy.exe, --rust-proxy EXE) in place of Velocity
  --proxy-workers N   the Rust proxy's worker threads (0: one a core)

Every server: the frozen build D:/Horizon/perf/NAME, no plugins, its bots all in one chunk (ramp.py's group mode).
Each step adds STEP bots to every server. Prints each server's TPS, MSPT, P95, CPU and RAM, the proxy's and the
bots' CPU, and writes D:/Horizon/perf/NAME/multi-TIME.json.
"""
import json
import os
import pathlib
import secrets
import shutil
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ramp  # noqa: E402

PERF = ramp.PERF
option = ramp.option


def find_pids(match_name, match_text, timeout=240):
    """{pid: command line} of the processes named MATCH_NAME whose command line has MATCH_TEXT (one slow CIM query)."""
    out = subprocess.run(['powershell', '-NoProfile', '-Command',
                          f"Get-CimInstance Win32_Process -Filter \"Name='{match_name}'\" | "
                          "Select-Object ProcessId,CommandLine | ConvertTo-Json"],
                         capture_output=True, text=True, timeout=timeout).stdout
    rows = json.loads(out or '[]')
    rows = rows if isinstance(rows, list) else [rows]
    return {r['ProcessId']: r.get('CommandLine') or '' for r in rows if match_text in (r.get('CommandLine') or '')}


def cpu_of(pids):
    """{pid: (cpu seconds, rss MB)} of PIDS, by Get-Process (fast; no command lines)."""
    pids = [p for p in pids if p]
    if not pids:
        return {}
    try:
        out = subprocess.run(['powershell', '-NoProfile', '-Command',
                              f"Get-Process -Id {','.join(map(str, pids))} -ErrorAction SilentlyContinue | "
                              "Select-Object Id,@{n='C';e={$_.TotalProcessorTime.TotalSeconds}},WorkingSet64 | ConvertTo-Json"],
                             capture_output=True, text=True, timeout=90).stdout
        rows = json.loads(out or '[]')
    except Exception:  # noqa: BLE001
        return {}
    rows = rows if isinstance(rows, list) else [rows]
    return {r['Id']: (r['C'] or 0.0, r['WorkingSet64'] / 2**20) for r in rows}


def main():
    name = sys.argv[1]
    exe_dir = PERF / name
    count = int(option('--servers', '3'))
    start, step, most = int(option('--start', '200')), int(option('--step', '25')), int(option('--max', '1000'))
    sweet, floor = float(option('--sweet', '19.5')), float(option('--floor', '15'))
    settle, measure, warmup = int(option('--settle', '20')), int(option('--measure', '10')), int(option('--warmup', '60'))
    behaviour = option('--behaviour', 'walk')
    bot_threads = int(option('--bot-threads', '3'))
    jar = pathlib.Path(option('--velocity', r'D:\Horizon\downloads\velocity\velocity.jar'))
    java = option('--java', os.path.expandvars(r'%LOCALAPPDATA%\Programs\Eclipse Adoptium\jdk-25.0.1.8-hotspot\bin\java.exe'))
    root = PERF / '.ramp'
    secret = secrets.token_hex(16)
    direct = '--direct' in sys.argv
    # --threads N: each server's worker and region pools (HORIZON_WORKERS, HORIZON_REGION_WORKERS); several servers
    # on one machine each starting a pool per core fight over the same cores.
    threads = option('--threads', None)
    # --pin N: each server on its own N logical processors (whole cores: Windows numbers a core's two threads side by
    # side), none shared with another server or the bots; the bots get every processor after the servers'. Its pools
    # sized to them unless --threads says otherwise.
    pin = int(option('--pin', '0'))
    if pin and not threads:
        threads = str(pin)
    if threads:
        os.environ['HORIZON_WORKERS'] = threads
        os.environ['HORIZON_REGION_WORKERS'] = threads
    kind = option('--proxy', 'velocity')
    # --mode spread: each server's bots in GROUP-bot homes SPACING blocks apart (ramp.py's spread), on a copy of
    # --world-from; --resources, --env K=V, --prop KEY=VALUE as ramp.py's.
    spread = option('--mode', 'group') == 'spread'
    group, spacing = max(1, int(option('--group', '5'))), int(option('--spacing', '512'))
    world_from = option('--world-from', None)
    resources = pathlib.Path(option('--resources', str(ramp.RESOURCES)))
    props = [f'properties.{sys.argv[i + 1]}' for i, w in enumerate(sys.argv) if w == '--prop']
    for i, word in enumerate(sys.argv):
        if word == '--env':
            key, value = sys.argv[i + 1].split('=', 1)
            os.environ[key] = value
    homes = ramp.spiral(most // group + 2) if spread else []
    rust_proxy = option('--rust-proxy', str(pathlib.Path(__file__).resolve().parents[2] / 'target' / 'fast' / 'horizon-proxy.exe'))

    # The servers, each behind the proxy: offline, the proxy's secret in forwarding.secret.
    servers = []
    for k in range(count):
        port = ramp.free_port()
        s = ramp.Server(exe_dir, root / f'multi-s{k + 1}', port, resources)
        s.start(world_from)
        s.stop()
        if not direct:
            (s.dir / 'forwarding.secret').write_text(secret, encoding='utf-8')
        # Behind the proxy on this machine: no compression between them (a network's servers sit on a private
        # network with the proxy; compressing both legs made the proxy unpack and repack every packet: 9.5 cores
        # for 600 players). The proxy still compresses for the players.
        # Velocity unpacks and repacks every packet, so its servers send uncompressed (a private network); the Rust
        # proxy passes the servers' compressed frames through, so they keep compressing (each chunk once for all).
        settings = ['server.online_mode=false'] + ([] if kind == 'rust' else ['properties.network-compression-threshold=-1']) + props
        print(s.run('config', 'set', *settings)[-120:], flush=True)
        subprocess.run([s.hs, '--dir', str(s.dir), 'start'], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=120)
        for _ in range(240):
            if ('forwarding is on' in s.log() or direct) and 'Done (' in s.log():
                break
            time.sleep(1)
        else:
            sys.exit(f'server {k + 1} did not start behind the proxy:\n' + s.log()[-1500:])
        servers.append(s)
        print(f'server s{k + 1} on {port}: ' + ('direct (no proxy)' if direct else 'Velocity forwarding on'), flush=True)

    proxy = None
    proxy_port = None
    if not direct and kind == 'rust':
        vdir = root / 'multi-rustproxy'
        shutil.rmtree(vdir, ignore_errors=True)
        vdir.mkdir(parents=True)
        proxy_port = ramp.free_port()
        nl = chr(10)
        conf = (f'bind = "0.0.0.0:{proxy_port}"' + nl + 'motd = "HorizonsX test network"' + nl + 'max_players = 100000' + nl
                + f"workers = {option('--proxy-workers', '0')}" + nl + 'forwarding_secret_file = "forwarding.secret"' + nl + 'buffer = 32768' + nl
                # The bots sign in with no Mojang account (the proxy's default is online mode since 11-06).
                + 'online_mode = false' + nl
                + 'try = ["s1"]' + nl + nl + '[servers]' + nl
                + ''.join(f's{k + 1} = "127.0.0.1:{s.port}"' + nl for k, s in enumerate(servers))
                + nl + '[forced_hosts]' + nl + ''.join(f'"127.0.0.{k + 1}" = "s{k + 1}"' + nl for k in range(count)))
        (vdir / 'proxy.toml').write_text(conf, encoding='utf-8')
        (vdir / 'forwarding.secret').write_text(secret, encoding='utf-8')
        proxy_log = open(vdir / 'proxy.log', 'w', encoding='utf-8')
        proxy = subprocess.Popen([rust_proxy, str(vdir / 'proxy.toml')], cwd=vdir, stdin=subprocess.DEVNULL,
                                 stdout=proxy_log, stderr=subprocess.STDOUT)
        for _ in range(60):
            if 'Done' in (vdir / 'proxy.log').read_text(encoding='utf-8', errors='replace'):
                break
            time.sleep(0.5)
        else:
            sys.exit('the Rust proxy did not start:' + nl + (vdir / 'proxy.log').read_text(encoding='utf-8', errors='replace')[-1500:])
        print(f'horizon-proxy (Rust) on {proxy_port} in front of {count} servers', flush=True)
    if not direct and kind != 'rust':
        # The proxy: its own config written once by Velocity, then ours set in it.
        vdir = root / 'multi-velocity'
        shutil.rmtree(vdir, ignore_errors=True)
        vdir.mkdir(parents=True)
        proxy_port = ramp.free_port()
        first = subprocess.Popen([java, '-jar', str(jar)], cwd=vdir, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL)
        for _ in range(120):
            if (vdir / 'velocity.toml').exists():
                time.sleep(3)
                break
            time.sleep(1)
        first.kill()
        first.wait()
        toml = (vdir / 'velocity.toml').read_text(encoding='utf-8')
        # Top-level keys set in place; [servers] and [forced-hosts] (multi-line lists) replaced whole, up to [advanced].
        lines = []
        for line in toml.splitlines():
            key = line.split('=')[0].strip() if '=' in line and not line.lstrip().startswith('#') else ''
            line = {
                'bind': f'bind = "0.0.0.0:{proxy_port}"',
                'online-mode': 'online-mode = false',
                'player-info-forwarding-mode': 'player-info-forwarding-mode = "modern"',
                'show-max-players': 'show-max-players = 100000',
                # One login every 3 s from an address by default: every bot here comes from this machine.
                'login-ratelimit': 'login-ratelimit = 0',
            }.get(key, line) if not line.startswith(' ') else line
            lines.append(line)
        nl = chr(10)
        text = nl.join(lines) + nl
        servers_block = ('[servers]' + nl + ''.join(f's{k + 1} = "127.0.0.1:{s.port}"' + nl for k, s in enumerate(servers))
                         + 'try = ["s1"]' + nl + nl + '[forced-hosts]' + nl
                         + ''.join(f'"127.0.0.{k + 1}" = ["s{k + 1}"]' + nl for k in range(count)) + nl)
        a, b = text.index('[servers]'), text.index('[advanced]')
        text = text[:a] + servers_block + text[b:]
        (vdir / 'velocity.toml').write_text(text, encoding='utf-8')
        (vdir / 'forwarding.secret').write_text(secret, encoding='utf-8')
        proxy_log = open(vdir / 'proxy.log', 'w', encoding='utf-8')
        proxy = subprocess.Popen([java, '-Xms2G', '-Xmx2G', '-jar', str(jar)], cwd=vdir, stdin=subprocess.PIPE,
                                 stdout=proxy_log, stderr=subprocess.STDOUT)
        for _ in range(120):
            if 'Done (' in (vdir / 'proxy.log').read_text(encoding='utf-8', errors='replace'):
                break
            time.sleep(1)
        else:
            sys.exit('the proxy did not start:\n' + (vdir / 'proxy.log').read_text(encoding='utf-8', errors='replace')[-1500:])
        print(f'Velocity on {proxy_port} in front of {count} servers', flush=True)
    found = find_pids('horizon-client.exe', os.sep + 'multi-s')
    engine_pid = {}
    for k in range(count):
        tag = os.sep + f'multi-s{k + 1}'
        engine_pid[k] = next((pid for pid, cmd in found.items() if tag in cmd and (tag + '0') not in cmd), None)
    print(f'engines: {engine_pid}', flush=True)
    total_threads = os.cpu_count() or 1
    if pin:
        if pin * count >= total_threads:
            sys.exit(f'--pin {pin} for {count} servers leaves the bots none of the {total_threads} processors')
        for k in range(count):
            server_mask = sum(1 << i for i in range(k * pin, (k + 1) * pin))
            subprocess.run(['powershell', '-NoProfile', '-Command',
                            f'(Get-Process -Id {engine_pid[k]}).ProcessorAffinity = {server_mask}'], capture_output=True, timeout=30)
            print(f'server s{k + 1} pinned to processors {k * pin}-{(k + 1) * pin - 1}', flush=True)
        bot_threads = total_threads - pin * count

    procs, steps = [], []
    per = 0
    stamp = time.strftime('%H%M%S')
    out = exe_dir / f'multi-{stamp}.json'
    mask = sum(1 << i for i in range(total_threads - bot_threads, total_threads))
    try:
        target = start
        while target <= most:
            add = target - per
            for k, s in enumerate(servers):
                p = subprocess.Popen(
                    [str(exe_dir / 'horizon-bots.exe'), '--server', f'127.0.0.1:{s.port}' if direct else f'127.0.0.{k + 1}:{proxy_port}', '--bots', str(add),
                     '--name', f'S{k + 1}x{len(procs)}_', '--behaviour', behaviour, '--join-every', '0.05',
                     '--seconds', '100000', '--report', 'NUL'], stdout=open(PERF / '.ramp' / f'bots-s{k + 1}-{len(procs)}.jsonl', 'w'), stderr=subprocess.DEVNULL)
                subprocess.run(['powershell', '-NoProfile', '-Command', f'(Get-Process -Id {p.pid}).ProcessorAffinity = {mask}'],
                               capture_output=True, timeout=30)
                procs.append(p)
            time.sleep(add * 0.05 + 4)
            if spread:
                # Each server's new bots to their homes, as ramp.py's spread (named S{server}x{their process}_{i}).
                def place(k, s):
                    for i in range(add):
                        j = per + i
                        hx, hz = homes[j // group]
                        x, z = hx * spacing + (j % group % 3) * 4, hz * spacing + (j % group // 3) * 4
                        s.run('cmd', 'tp', f'S{k + 1}x{len(procs) - count + k}_{i}', x, 140, z, timeout=30)
                movers = [threading.Thread(target=place, args=(k, s)) for k, s in enumerate(servers)]
                for t in movers:
                    t.start()
                for t in movers:
                    t.join()
            per = target
            time.sleep(settle)
            if len(steps) == 0 and warmup:
                print(f'warm-up {warmup} s', flush=True)
                time.sleep(warmup)
            if not spread:
                for s in servers:
                    s.run('cmd', 'tp', '@a', 8, 140, 8, timeout=60)
            time.sleep(3)
            # Every server's report at once, and every process's CPU over the same window.
            watch = list(engine_pid.values()) + ([proxy.pid] if proxy else []) + [p.pid for p in procs]
            # Each reading's time is the middle of its (slow) PowerShell call.
            a0 = time.time()
            before = cpu_of(watch)
            t0 = (a0 + time.time()) / 2
            results = [None] * count
            def report(k, s):
                results[k] = s.report(measure)
            threads = [threading.Thread(target=report, args=(k, s)) for k, s in enumerate(servers)]
            for t in threads:
                t.start()
            for t in threads:
                t.join()
            b0 = time.time()
            after = cpu_of(watch)
            wall = max((b0 + time.time()) / 2 - t0, 0.001)
            def cores(pids):
                return sum(after[p][0] - before[p][0] for p in pids if p in after and p in before) / wall
            row = {'per_server': per, 'servers': []}
            print(f'--- {per} bots a server, {per * count} in all', flush=True)
            for k, s in enumerate(servers):
                nums, line, per_tick = results[k]
                pid = engine_pid.get(k)
                c = cores([pid])
                ram = after.get(pid, (0, 0))[1]
                nums.update({'cores': round(c, 2), 'cpu_pct': round(100 * c / total_threads, 1), 'rss_mb': round(ram)})
                row['servers'].append(nums)
                print(f"  s{k + 1}: players {nums.get('players')}  TPS {nums.get('tps')}  MSPT {nums.get('mspt')}  P95 {nums.get('p95')}  "
                      f"CPU {nums['cores']} cores ({nums['cpu_pct']}%)  RAM {nums['rss_mb']} MB", flush=True)
                if per_tick:
                    print('      ' + per_tick[:170], flush=True)
            row['proxy_cores'] = round(cores([proxy.pid]), 2) if proxy else 0.0
            row['proxy_mb'] = round(after.get(proxy.pid, (0, 0))[1]) if proxy else 0
            row['bots_cores'] = round(cores([p.pid for p in procs]), 2)
            all_cores = sum(x['cores'] for x in row['servers']) + row['proxy_cores'] + row['bots_cores']
            row['all_cores'] = round(all_cores, 2)
            print(f"  proxy CPU {row['proxy_cores']} cores ({row['proxy_mb']} MB), bots CPU {row['bots_cores']} cores, all together {row['all_cores']} "
                  f"of {total_threads} ({round(100 * all_cores / total_threads, 1)}%)", flush=True)
            steps.append(row)
            out.write_text(json.dumps({'build': name, 'servers': count, 'steps': steps}, indent=1), encoding='utf-8')
            worst = min(x.get('tps', 0) for x in row['servers'])
            if worst < floor:
                break
            target += step
    finally:
        for p in procs:
            p.kill()
        if proxy:
            proxy.kill()
        for s in servers:
            s.stop()
    ok = [r for r in steps if min(x.get('tps', 0) for x in r['servers']) >= sweet
          and max(x.get('p95', 999) for x in r['servers']) < 50]
    fine = [r for r in steps if min(x.get('tps', 0) for x in r['servers']) >= floor]
    best = ok[-1]['per_server'] if ok else 0
    most_ok = fine[-1]['per_server'] if fine else 0
    print(f'\nsweet spot: {best} players a server at a steady 20 TPS on all {count} (P95 under 50 ms): {best * count} in all')
    print(f'most at or above {floor} TPS on every server: {most_ok} a server, {most_ok * count} in all; wrote {out}')


if __name__ == '__main__':
    main()
