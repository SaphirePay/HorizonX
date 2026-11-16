#!/usr/bin/env python3
"""The tick thread's time as a tree of the engine's own functions, from a profile's kept stacks.

Usage: python -I tools/v2/tree.py NAME [--root TEXT] [--levels 3] [--min 0.4]

Each sample is followed from the frame matching --root (default `Game::tick_server`) toward the leaf, keeping only
the horizon frames (with their source lines), so each level says which call took the time. Shares are of the
tick's samples (idle between ticks left out).
"""
import collections
import json
import pathlib
import re
import sys

PERF = pathlib.Path(r'D:\Horizon\perf')


def clean(frame):
    frame = frame.split('\u0000')[0]
    line = frame[frame.find(' @') + 2:].split(' @')[0].strip() if ' @' in frame else ''
    name = re.sub(r'<.*', '', frame.split(' @')[0]).strip()
    name = re.sub(r'::impl\$\d+', '', name).replace('horizon_client::', '').replace('horizon_world::', 'world::')
    return name, line


def main():
    name = sys.argv[1]
    root = sys.argv[sys.argv.index('--root') + 1] if '--root' in sys.argv else 'Game::tick_server'
    levels = int(sys.argv[sys.argv.index('--levels') + 1]) if '--levels' in sys.argv else 3
    least = float(sys.argv[sys.argv.index('--min') + 1]) if '--min' in sys.argv else 0.4
    stacks = [json.loads(line) for line in (PERF / name / 'stacks.jsonl').read_text(encoding='utf-8', errors='replace').splitlines() if line.strip()]
    paths = []
    for stack in stacks:
        frames = [clean(f) for f in stack]
        at = next((i for i in range(len(frames) - 1, -1, -1) if root in frames[i][0]), None)
        if at is None:
            continue
        chain = []
        for fname, line in reversed(frames[:at]):
            if not line or not re.search(r'horizon|game|lan|mobs|world', fname) or 'spans::time' in fname:
                continue
            label = f'{fname.split("::")[-1]} {line.split("/")[-1]}'
            if not chain or chain[-1] != label:
                chain.append(label)
        leaf = frames[0][0].split('::')[-1]
        paths.append((chain, leaf))
    total = len(paths)
    print(f'{total} tick samples of {len(stacks)}')

    def walk(prefix, depth):
        counts = collections.Counter()
        for chain, leaf in paths:
            if chain[:len(prefix)] == prefix:
                counts[chain[len(prefix)] if len(chain) > len(prefix) else f'(self: {leaf})'] += 1
        for key, n in counts.most_common():
            share = 100 * n / total
            if share < least:
                break
            print(f'{"  " * depth}{share:5.1f}%  {key}')
            if depth + 1 < levels and not key.startswith('(self'):
                walk(prefix + [key], depth + 1)

    walk([], 0)


if __name__ == '__main__':
    main()
