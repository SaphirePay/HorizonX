#!/usr/bin/env python3
"""Where the functions a tick phase reaches use one `Game` field (tools/v2/fields.py's call graph).

Usage: python -I tools/v2/uses.py FIELD FN [FN...] [--calls]

Prints each reached function's lines that name `self.FIELD`; --calls instead counts the methods called on it
(`self.FIELD....name(`, `host.hub.name(` after taking it), to see what a region would have to do without it.
"""
import collections
import re
import sys

sys.path.insert(0, __file__.rsplit('\\', 1)[0].rsplit('/', 1)[0])
import fields  # noqa: E402


def main():
    field = sys.argv[1]
    names = [a for a in sys.argv[2:] if not a.startswith('--')]
    fns = fields.functions()
    seen, frontier = set(), list(names)
    while frontier:
        name = frontier.pop()
        if name in seen or name not in fns:
            continue
        seen.add(name)
        for _, body in fns[name]:
            frontier += re.findall(r'self\.([a-z_][a-z0-9_]*)\s*\(', body)
    calls = collections.Counter()
    for name in sorted(seen):
        for fname, body in fns[name]:
            if f'self.{field}' not in body:
                continue
            if '--calls' in sys.argv:
                for m in re.finditer(r'(?:hub|host|lan)\??\.(?:hub\.)?([a-z_][a-z0-9_]*)\s*\(', body):
                    calls[(m.group(1), name)] += 1
            else:
                for line in body.splitlines():
                    if f'self.{field}' in line:
                        print(f'{fname}:{name}: {line.strip()[:150]}')
    for (method, name), n in sorted(calls.items()):
        print(f'{method:30} in {name}')


if __name__ == '__main__':
    main()
