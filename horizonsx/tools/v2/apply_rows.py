#!/usr/bin/env python3
"""Apply a plugin-tools family file's EDITS rows (file, old, new) to this worktree's crates, once.

Usage: python tools/v2/apply_rows.py PATH/TO/<f>_engine.py [--dry]

Each row's `old` text must occur exactly once in crates/<file>; a row whose `new` text is already there is
counted as applied. Prints one line per row and exits non-zero if any row neither matched nor was in.
"""
import pathlib
import runpy
import sys

CRATES = pathlib.Path(__file__).resolve().parents[2] / 'crates'


def main():
    rows = runpy.run_path(sys.argv[1])['EDITS']
    dry = '--dry' in sys.argv
    failed = 0
    for n, row in enumerate(rows):
        path, old, new = row[0], row[1], row[2]
        file = CRATES / path
        text = file.read_text(encoding='utf-8')
        if new in text and old not in text:
            print(f'{n:3} in already  {path}')
            continue
        count = text.count(old)
        if count != 1:
            print(f'{n:3} FOUND {count}x    {path}: {old.strip().splitlines()[0][:70]!r}')
            failed += 1
            continue
        if not dry:
            file.write_text(text.replace(old, new, 1), encoding='utf-8', newline='')
        print(f'{n:3} applied     {path}')
    sys.exit(1 if failed else 0)


if __name__ == '__main__':
    main()
