#!/usr/bin/env python3
"""Who calls a hot function, from a profile's kept stacks (bench.py --profile keeps PERF/NAME/stacks.jsonl).

Usage: python -I tools/v2/callers.py NAME TEXT [--depth 4] [--top 25] [--phase]

For every sample whose top frame contains TEXT, the next DEPTH frames down the stack (its callers) are counted;
--phase instead names the tick phase (`spans::time` closure's game.rs line, or the nearest Game:: function) each
sample was in. Shares are of all samples.
"""
import collections
import json
import pathlib
import re
import sys

PERF = pathlib.Path(r'D:\Horizon\perf')


def clean(frame):
    frame = frame.split('\u0000')[0]
    frame = re.sub(r'<.*', '', frame) + (frame[frame.find(' @'):] if ' @' in frame else '')
    return frame.strip()


def main():
    name, text = sys.argv[1], sys.argv[2]
    depth = int(sys.argv[sys.argv.index('--depth') + 1]) if '--depth' in sys.argv else 4
    top = int(sys.argv[sys.argv.index('--top') + 1]) if '--top' in sys.argv else 25
    phase = '--phase' in sys.argv
    stacks = [json.loads(line) for line in (PERF / name / 'stacks.jsonl').read_text(encoding='utf-8', errors='replace').splitlines() if line.strip()]
    counts = collections.Counter()
    hits = 0
    for stack in stacks:
        frames = [clean(f) for f in stack]
        if not frames or text not in frames[0]:
            continue
        hits += 1
        if phase:
            named = next((f for f in frames if 'Game::' in f and 'spans::time' not in f and '@' in f), '?')
            counts[named] += 1
        else:
            counts[' <- '.join(frames[1:1 + depth])] += 1
    print(f'{hits} of {len(stacks)} samples ({100 * hits / max(1, len(stacks)):.1f}%) have {text!r} on top')
    for key, n in counts.most_common(top):
        print(f'{100 * n / len(stacks):5.1f}%  {key}')


if __name__ == '__main__':
    main()
