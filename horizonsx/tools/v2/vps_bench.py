#!/usr/bin/env python3
"""The VPS comparison (Linux): Horizon V2 against the Rust Paper port, on the same machine, bots and settings.

Runs on the VPS beside copies of tools/server_bench.py, lab.py, runtime.py and test_rust_parity.py:
    python3 vps_bench.py OUT_DIR BOTS[,BOTS] NAME=SUPERVISOR_DIR:ENGINE [NAME=...] [--behaviour walk]
                         [--spread 512:5] [--warmup 60] [--seconds 30] [--join-every 0.15] [--bots-bin PATH]
                         [--resources DIR] [--cpu-quota 800%]
Each NAME runs its own supervisor (SUPERVISOR_DIR/horizon-server) with ENGINE as the engine, no plugins, one after
the other at every bot count. With --cpu-quota the whole run sits in a systemd scope with that CPU cap (the VPS
serves other things too). Writes OUT_DIR/NAME.json (server_bench's report) and OUT_DIR/summary.json.
"""
import json
import os
import pathlib
import subprocess
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))


def option(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


def main():
    if '--in-scope' not in sys.argv and '--cpu-quota' in sys.argv:
        # Again inside a transient systemd scope with the CPU cap (systemd-run waits for it).
        quota = option('--cpu-quota', '800%')
        args = [a for i, a in enumerate(sys.argv) if a != '--cpu-quota' and sys.argv[i - 1] != '--cpu-quota']
        return subprocess.call(['systemd-run', '--scope', '--quiet', '-p', f'CPUQuota={quota}', '-p', 'MemoryMax=8G',
                                sys.executable, *args, '--in-scope'])
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    bots = sys.argv[2]
    flags = {'--behaviour', '--spread', '--warmup', '--seconds', '--join-every', '--bots-bin', '--resources', '--cpu-quota',
             '--plugins'}
    servers = [a for i, a in enumerate(sys.argv[3:], 3) if '=' in a and sys.argv[i - 1] not in flags]
    resources = option('--resources', '/opt/horizon-paper-test/resources/26.3')
    # Read now: sys.argv becomes server_bench's before each run.
    plugins = option('--plugins', None)
    import server_bench as bench
    bench.lab.LOCAL = out / '.work'
    bench.lab.LOCAL.mkdir(exist_ok=True)
    original = bench.server_command
    summary = {}
    for spec in servers:
        name, _, rest = spec.partition('=')
        supervisor_dir, _, engine = rest.partition(':')
        bench.BIN = pathlib.Path(supervisor_dir)

        def command(kind, directory, args, engine=engine):
            cmd = original(kind, directory, args)
            if plugins:
                # Paper plugins (.so) in the server's plugins folder, as on the real server.
                import shutil
                (directory / 'plugins').mkdir(exist_ok=True)
                for lib in pathlib.Path(plugins).glob('*.so'):
                    shutil.copy2(lib, directory / 'plugins' / lib.name)
            subprocess.run([str(bench.BIN / 'horizon-server'), '--dir', str(directory), 'config', 'set',
                            f'engine.path={engine}', f'engine.resources={resources}', 'panel.bind='],
                           capture_output=True)
            return cmd

        bench.server_command = command
        report = out / f'{name}.json'
        sys.argv = ['server_bench', '--servers', 'horizon', '--bots', bots,
                    '--behaviour', option('--behaviour', 'walk'), '--join-every', option('--join-every', '0.15'),
                    '--warmup', option('--warmup', '60'), '--seconds', option('--seconds', '30'),
                    '--bots-bin', option('--bots-bin', str(HERE / 'horizon-bots')), '--out', str(report),
                    *(['--spread', option('--spread', '')] if '--spread' in sys.argv else [])]
        started = time.time()
        bench.main()
        data = json.loads(report.read_text())
        summary[name] = [{k: r.get(k) for k in ('bots', 'tps', 'mspt', 'rss_mb', 'cpu_percent', 'start_seconds',
                                                 'join_ms', 'chunks_per_second', 'bots_joined', 'bots_failed')}
                         for r in data.get('runs', [])]
        summary[name + '_seconds'] = round(time.time() - started)
        (out / 'summary.json').write_text(json.dumps(summary, indent=1))
    print('vps bench done', flush=True)


if __name__ == '__main__':
    sys.exit(main())
