#!/usr/bin/env python3
"""Build HorizonsX's server engine, fully optimized, into D:/Horizon/perf/NAME for perf_bench.py.

Usage: python tools/v2/build.py NAME [--check] [--symbols] [--heap-profile] [--quick] [--test]

--symbols builds with line tables (cache D:/Horizon/v2-target-sym) so profiles name every function.
--heap-profile also makes the allocator a sampling heap profiler (heap-profile.txt in the server's folder,
  named by tools/v2/heap.py): which code holds the memory. Never bench its speed.
--test: fabric-port's quick mode (horizon-client at opt-level 1 in 256 units, the other crates optimized, no debug
  info, rust-lld) and no local ThinLTO: a real engine edit rebuilds in about a minute. `fast` (benches, deploys)
  keeps its local ThinLTO: off, an edit rebuilt in 62 s instead of 105-112 s, but ticks took 7-12% longer
  (600 players in groups of 5: 47.5 -> 53.0 ms): for the correctness checks (resident_e2e...), cache D:/Horizon/v2-target-test. Never bench it.
--quick: horizon-client unoptimized (cache D:/Horizon/v2-target-quick): an engine edit rebuilds ~3x faster.
  For trying a change only: never bench it, never deploy it.

Same command as fabric-port's build_engine.py optimized mode (profile `fast`, no audio or
controllers), from this worktree, with its own cache in D:/Horizon/v2-target so it never
shares a cache with another session's builds. horizon-server.exe and horizon-bots.exe come
from perf/before, as perf_build.py does, so every bench drives the engine the same way.
"""
import os
import pathlib
import shutil
import subprocess
import sys
import time

TREE = pathlib.Path(__file__).resolve().parents[2]
CACHE = pathlib.Path(r'D:\Horizon\v2-target')
PERF = pathlib.Path(r'D:\Horizon\perf')
BELOW_NORMAL = 0x00004000


def main():
    name = sys.argv[1]
    check = '--check' in sys.argv
    env = dict(os.environ, CARGO_TARGET_DIR=str(CACHE))
    # The compiler's parallel front end (8 threads), as the Linux build uses: the engine crate's type checking
    # was one thread for most of a build. Same code out.
    if 'RUSTFLAGS' not in env:
        env['RUSTC_BOOTSTRAP'] = '1'
        env['RUSTFLAGS'] = '-Zthreads=8'
    if '--heap-profile' in sys.argv:
        # The sampling heap profiler as the allocator (horizon-threads heap_profile.rs), with line tables:
        # which code holds the memory (tools/v2/heap.py). A diagnostic build: never benched for speed.
        sys.argv += ['--symbols', '--feature', 'heap-profile']
    if '--symbols' in sys.argv:
        # Every function named in the PDB (line tables, same code): exact stacks for bench.py --profile.
        # Its own cache, so switching never rebuilds the plain one.
        env['CARGO_TARGET_DIR'] = str(CACHE) + '-sym'
        env['CARGO_PROFILE_FAST_DEBUG'] = 'line-tables-only'
    command = ['cargo', '+1.99.1', 'check' if check else 'build', '--locked', '--profile', 'fast',
               '--no-default-features', '-p', 'horizon-client', '--bin', 'horizon-client']
    if '--test' in sys.argv:
        # fabric-port build_engine.py's quick mode: the engine crate lightly optimized in many units, everything
        # it depends on fully, no debug info, LLVM's linker. Fast to build, fast enough to play and test.
        for setting in ('profile.fast.opt-level=1', 'profile.fast.package."*".opt-level=3',
                        'profile.fast.package.horizon-items.opt-level=2', 'profile.fast.package.horizon-world.opt-level=2',
                        'profile.fast.package.horizon-core.opt-level=2', 'profile.fast.package.horizon-assets.opt-level=2',
                        'profile.fast.codegen-units=256', 'profile.fast.debug=false', 'profile.fast.lto="off"',
                        'target.x86_64-pc-windows-msvc.linker="rust-lld"'):
            command += ['--config', setting]
        env['CARGO_TARGET_DIR'] = str(CACHE) + '-test'
    if '--quick' in sys.argv:
        # Iteration builds only, never benched and never for the VPS: horizon-client unoptimized in many small
        # units, so an edit to the engine rebuilds ~3x faster (the program runs slower). Its own cache, so a
        # normal build stays warm.
        command += ['--config', 'profile.fast.package.horizon-client.opt-level=0',
                    '--config', 'profile.fast.package.horizon-client.codegen-units=256']
        env['CARGO_TARGET_DIR'] = str(CACHE) + '-quick'
    # --feature NAME (repeatable): a cargo feature of horizon-client, e.g. windows-segment-heap.
    features = [sys.argv[i + 1] for i, a in enumerate(sys.argv) if a == '--feature']
    # HorizonsX on Windows: the system heap as the Segment Heap (main.rs), 20% less RAM at 100 players.
    if os.name == 'nt' and '--mimalloc' not in sys.argv:
        features.append('windows-segment-heap')
    if features:
        command += ['--features', ','.join(features)]
    started = time.monotonic()
    subprocess.run(command, cwd=TREE, env=env, check=True, creationflags=BELOW_NORMAL)
    print(f'compiled in {time.monotonic() - started:.0f} s', flush=True)
    if check:
        return
    folder = PERF / name
    folder.mkdir(parents=True, exist_ok=True)
    out = pathlib.Path(env['CARGO_TARGET_DIR']) / 'fast'
    shutil.copy2(out / 'horizon-client.exe', folder / 'horizon-client.exe')
    # The symbols of this very build, for plugin-tools' stacks.py profiles (bench.py --profile).
    pdb = out / 'horizon_client.pdb'
    if pdb.exists():
        shutil.copy2(pdb, folder / 'horizon_client.pdb')
    for tool in ('horizon-server.exe', 'horizon-bots.exe'):
        shutil.copy2(PERF / 'before' / tool, folder / tool)
    print('built', folder, flush=True)


if __name__ == '__main__':
    main()
