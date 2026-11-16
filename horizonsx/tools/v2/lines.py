#!/usr/bin/env python3
"""Where in the source a profile's samples sit, from bench.py --profile's stacks.jsonl (stacks.py --lines).

Usage: python -I tools/v2/lines.py NAME [TEXT] [--depth N]
  no TEXT: the source lines at the top of the stack, most samples first
  TEXT:    for samples whose top frame matches TEXT, the line of each of the next N frames (call sites)
NAME is a folder of D:/Horizon/perf. Percentages are of all samples.
"""
import collections
import json
import pathlib
import sys


def main():
    name = sys.argv[1]
    text = sys.argv[2] if len(sys.argv) > 2 and not sys.argv[2].startswith('--') else None
    depth = int(sys.argv[sys.argv.index('--depth') + 1]) if '--depth' in sys.argv else 2
    samples = [json.loads(line) for line in (pathlib.Path(r'D:\Horizon\perf') / name / 'stacks.jsonl').open(encoding='utf-8')]
    total = len(samples)
    where = lambda frame: frame.split(' @', 1)[1] if ' @' in frame else frame[:90]
    if not text:
        counts = collections.Counter(where(s[0]) for s in samples if s)
        for line, n in counts.most_common(45):
            print(f'{100 * n / total:5.1f}%  {line}')
        return
    if '--within' in sys.argv:
        # Samples with TEXT anywhere in the stack: the lines at their top (inlined code included).
        hits = [s for s in samples if any(text in f for f in s)]
        print(f'{len(hits)} of {total} samples ({100 * len(hits) / total:.1f}%) are inside {text!r}')
        counts = collections.Counter(f'{where(s[0])}  [{s[0].split(" @")[0][-45:]}]' for s in hits)
        for line, n in counts.most_common(35):
            print(f'{100 * n / total:5.1f}%  {line}')
        return
    hits = [s for s in samples if s and text in s[0]]
    print(f'{len(hits)} of {total} samples ({100 * len(hits) / total:.1f}%) have {text!r} on top')
    for level in range(1, depth + 1):
        counts = collections.Counter(where(s[level]) for s in hits if len(s) > level)
        print(f'-- frame {level} (the call site)')
        for line, n in counts.most_common(15):
            print(f'{100 * n / total:5.1f}%  {line}')


if __name__ == '__main__':
    main()
