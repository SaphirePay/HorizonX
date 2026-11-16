#!/usr/bin/env python3
"""Print family rows (plugin-tools/tools/family/*_engine.py) by `table#index`, old and new text.

Usage: python tools/v2/paper_rows.py entity_engine#136 fab_entity_engine#100 ...
"""
import pathlib
import sys

FAMILY = pathlib.Path(r'D:\Horizon\plugin-tools\tools\family')


def main():
    for key in sys.argv[1:]:
        table, index = key.split('#')
        scope = {}
        path = FAMILY / f'{table}.py'
        exec(compile(path.read_text(encoding='utf-8'), str(path), 'exec'), scope)
        name, old, new = scope['EDITS'][int(index)]
        print(f'===== {key}  {name}\n----- old\n{old}\n----- new\n{new}\n')


if __name__ == '__main__':
    main()
