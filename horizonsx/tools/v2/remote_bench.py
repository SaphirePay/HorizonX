#!/usr/bin/env python3
"""HorizonsX on the VPS, bots on this PC: the server runs alone on the VPS (as for real players), the bots run
here (the 7800X3D) and join over the internet; the numbers are the server's own (`horizon-server report`).

Usage: python -I tools/v2/remote_bench.py NAME BOTS [--spread 512:5] [--warmup 60] [--seconds 60]
                                          [--join-every 0.1] [--engine REMOTE_PATH] [--plugins REMOTE_DIR]
                                          [--world LOCAL_DIR] [--env NAME=VALUE] [--port 25592] [--bots-bin EXE]

The VPS side is ONE server, /opt/horizon-v2-bench/remote/server (its own folder and port: the live server stays
up), made once and used by every run: the same world each time (owner, 11-05: never generate it again). --world
gives that world the first time (a saved world from this PC); later runs keep what is there. NAME only names the
result. Each run sets the engine, the plugins and --env, then starts the server.
The port is opened in the VPS firewall for the run and closed after. Writes D:/Horizon/perf/remote/NAME.json.
"""
import json
import pathlib
import shlex
import subprocess
import sys
import tarfile
import time

KEY = str(pathlib.Path.home() / '.ssh' / 'id_ed25519_saphire_vps')
VPS = 'root@your-server.example'
HOST = 'your-server.example'
BASE = '/opt/horizon-v2-bench'
OUT = pathlib.Path(r'D:\Horizon\perf\remote')
PROPERTIES = {
    'level-seed': '12345', 'view-distance': '10', 'simulation-distance': '10', 'online-mode': 'false',
    'white-list': 'false', 'gamemode': 'creative', 'force-gamemode': 'true', 'spawn-protection': '0',
    'max-players': '300', 'difficulty': 'easy', 'sync-chunk-writes': 'true',
}


def option(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


def ssh(script, check=True, capture=True):
    """One SSH call running `script` in bash on the VPS (calls kept few: each does a whole step)."""
    done = subprocess.run(['ssh', '-i', KEY, '-o', 'ConnectTimeout=20', VPS, 'bash -s'], input=script,
                          text=True, capture_output=capture)
    if check and done.returncode != 0:
        raise RuntimeError(f'VPS step failed ({done.returncode}): {done.stderr[-2000:] if capture else ""}')
    return done.stdout if capture else ''


def spread_points(bots, spread):
    """As server_bench's: groups of GROUP bots on a square grid BLOCKS apart around 0,0."""
    blocks, _, group = str(spread).partition(':')
    blocks, group = int(blocks), max(1, int(group or 1))
    homes = -(-bots // group)
    side = max(1, int(homes ** 0.5 + 0.999))
    points = []
    for i in range(bots):
        home = i // group
        gx, gz = home % side - (side - 1) / 2, home // side - (side - 1) / 2
        k = i % group
        points.append((round(gx * blocks) + (k % 3) * 4, round(gz * blocks) + (k // 3) * 4))
    return points


def main():
    name, bots = sys.argv[1], int(sys.argv[2])
    warmup, seconds = int(option('--warmup', '60')), int(option('--seconds', '60'))
    join_every = float(option('--join-every', '0.1'))
    port = int(option('--port', '25592'))
    engine = option('--engine', f'{BASE}/hx-new/horizon-client')
    plugins = option('--plugins', None)
    world = option('--world', None)
    spread = option('--spread', None)
    bots_bin = option('--bots-bin', r'D:\Horizon\perf\hx-g4s\horizon-bots.exe')
    env = [sys.argv[i + 1] for i, a in enumerate(sys.argv) if a == '--env']
    remote = f'{BASE}/remote/server'
    OUT.mkdir(parents=True, exist_ok=True)

    # 1. The saved world, uploaded once.
    tar_name = None
    if world:
        world = pathlib.Path(world)
        tar_name = f'{world.name}.tar'
        have = ssh(f'test -f {BASE}/worlds/{tar_name} && echo yes || true').strip()
        if have != 'yes':
            local_tar = OUT / tar_name
            with tarfile.open(local_tar, 'w') as tar:
                for part in world.iterdir():
                    tar.add(part, arcname=part.name)
            ssh(f'mkdir -p {BASE}/worlds')
            subprocess.run(['scp', '-q', '-i', KEY, str(local_tar), f'{VPS}:{BASE}/worlds/{tar_name}'], check=True)
            print('uploaded the world', flush=True)

    # 2. The server's folder, settings, plugins, firewall; started, waited for.
    props = dict(PROPERTIES, **{'server-port': str(port)})
    prop_text = ''.join(f'{k}={v}\n' for k, v in props.items())
    env_prefix = ' '.join(shlex.quote(e) for e in env)
    setup = f'''set -e
R={remote}
mkdir -p $R/plugins
cd $R
{BASE}/horizon-server --dir $R stop >/dev/null 2>&1 || true
rm -f $R/logs/latest.log $R/status.jsonl $R/report.out $R/reports/perf-*.json
printf %s {shlex.quote(prop_text)} > server.properties
echo eula=true > eula.txt
# The world: made once, kept.
{f"[ -d $R/world ] || tar -xf {BASE}/worlds/{tar_name} -C $R" if tar_name else ""}
rm -f $R/plugins/*.so
{f"cp {plugins}/*.so $R/plugins/" if plugins else ""}
{BASE}/horizon-server --dir $R config set engine.path={engine} engine.resources=/opt/horizon-paper-test/resources/26.3 panel.bind= >/dev/null 2>&1 || true
ufw allow {port}/tcp comment 'HorizonsX bench (temporary)' >/dev/null
env {env_prefix} {BASE}/horizon-server --dir $R start >/dev/null 2>&1 || (cd $R && nohup env {env_prefix} {BASE}/horizon-server --dir $R run > $R/run.out 2>&1 &)
for i in $(seq 1 120); do
  sleep 1
  if grep -qs "Done (" $R/logs/latest.log; then echo "up after $i s"; exit 0; fi
done
echo "not up after 120 s"; tail -20 $R/logs/latest.log 2>/dev/null; exit 1
'''
    print(ssh(setup).strip(), flush=True)

    # 3. The bots here, and on the VPS their homes (tp through the console) and the server's own report,
    # recorded over the measured window, with its status sampled meanwhile.
    total = int(bots * join_every + warmup + seconds + 10)
    tps = ''
    if spread:
        points = spread_points(bots, spread)
        lines = []
        for i, (x, z) in enumerate(points):
            for k in range(2):
                lines.append((i * join_every + 4 + k * 6, f'tp Bot{i} {x} 140 {z}'))
        lines.sort()
        tps = '\n'.join(f'sleep_until {t:.2f}; {BASE}/horizon-server --dir $R cmd {shlex.quote(c)} >/dev/null 2>&1 &'
                        for t, c in lines)
    window_start = bots * join_every + warmup
    vps_side = f'''R={remote}
START=$(date +%s.%N)
sleep_until() {{ now=$(date +%s.%N); d=$(echo "$START + $1 - $now" | bc); if [ "$(echo "$d > 0" | bc)" = 1 ]; then sleep $d; fi; }}
(
{tps}
wait
) &
(sleep_until {window_start:.0f}; {BASE}/horizon-server --dir $R report {seconds} > $R/report.out 2>&1) &
(for i in $(seq 1 {total // 5}); do {BASE}/horizon-server --dir $R status --json >> $R/status.jsonl 2>/dev/null; echo >> $R/status.jsonl; sleep 5; done) &
wait
'''
    driver = subprocess.Popen(['ssh', '-i', KEY, VPS, 'bash -s'], stdin=subprocess.PIPE, text=True,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    driver.stdin.write(vps_side)
    driver.stdin.close()
    print(f'{bots} bots from this PC to {HOST}:{port}', flush=True)
    bots_out = subprocess.run([bots_bin, '--server', f'{HOST}:{port}', '--bots', str(bots), '--behaviour',
                               option('--behaviour', 'walk'), '--join-every', str(join_every), '--seconds',
                               str(int(warmup + seconds + 5))], capture_output=True, text=True, timeout=total + 300)
    driver.wait(timeout=120)

    # 4. The server's report and status, then the server stopped and the port closed.
    collected = ssh(f'''R={remote}
ls -t $R/reports/perf-*.json 2>/dev/null | head -1 | xargs -r cat
echo "@@STATUS@@"
cat $R/status.jsonl 2>/dev/null
echo "@@LOG@@"
grep -E "region threads:|server started|\\[v2\\]|panicked" $R/logs/latest.log | tail -6
{BASE}/horizon-server --dir $R stop >/dev/null 2>&1 || pkill -f "$R" || true
ufw delete allow {port}/tcp >/dev/null 2>&1 || true
''', check=False)
    report_text, _, rest = collected.partition('@@STATUS@@')
    status_text, _, log_text = rest.partition('@@LOG@@')
    report = json.loads(report_text) if report_text.strip().startswith('{') else {}
    statuses = []
    for line in status_text.splitlines():
        line = line.strip()
        if line.startswith('{'):
            try:
                statuses.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    try:
        bots_report = json.loads(bots_out.stdout.strip().splitlines()[-1])
    except (IndexError, json.JSONDecodeError):
        bots_report = {'raw': bots_out.stdout[-2000:], 'stderr': bots_out.stderr[-2000:]}
    ticks = report.get('ticks', {})
    rss = [s.get('memory', {}).get('rss_mb') or s.get('rss_mb') for s in statuses]
    rss = [r for r in rss if r]
    summary = {
        'name': name, 'bots': bots, 'spread': spread, 'engine': engine, 'plugins': plugins, 'env': env,
        'tps': ticks.get('tps'), 'mspt': ticks.get('mspt'), 'cpu_ms_average': ticks.get('cpu_ms_average'),
        'over_50_ms': ticks.get('over_50_ms'), 'rss_mb_max': max(rss) if rss else None,
        'players_seen': report.get('players'), 'bots_joined': bots_report.get('joined') if isinstance(bots_report, dict) else None,
        'spans': report.get('spans', [])[:12], 'log': log_text.strip().splitlines(), 'bots_report': bots_report,
        'statuses': statuses[-3:],
    }
    (OUT / f'{name}.json').write_text(json.dumps(summary, indent=1))
    print(json.dumps({k: summary[k] for k in ('name', 'bots', 'tps', 'mspt', 'over_50_ms', 'rss_mb_max',
                                               'players_seen', 'bots_joined')}), flush=True)
    for s in summary['spans'][:8]:
        print(f'  {s.get("name", ""):<28} {s.get("ms_per_tick", 0):7.2f} ms', flush=True)
    for line in summary['log']:
        print('  ' + line[:150], flush=True)


if __name__ == '__main__':
    main()
