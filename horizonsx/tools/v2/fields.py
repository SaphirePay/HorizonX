#!/usr/bin/env python3
"""Which `Game` fields a tick phase touches, through every `self.method()` it calls (a rough call graph of the
engine's `impl Game` blocks, made from the source text).

Usage: python -I tools/v2/fields.py FN [FN...] [--writes] [--level] [--depth N]

For the functions named (and everything they call on `self`), lists each field of `Game` read or written,
marked `level` when it is one of the per-level fields `levels.rs` parks (a region's own data in HorizonsX's
Folia design) and `GLOBAL` otherwise. --writes keeps only fields that look written (`self.f =`, `&mut self.f`,
`self.f.push(...)` and the like). Built to sort the tick into region work and global work.
"""
import collections
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2] / 'crates' / 'horizon-client' / 'src'
FILES = [ROOT / 'game.rs', *sorted((ROOT / 'game').rglob('*.rs'))]
MUTATORS = r'(push|push_str|insert|remove|clear|extend|append|retain|drain|take|replace|get_mut|entry|iter_mut|sort|sort_by|sort_by_key|dedup|truncate|pop|swap_remove|set|tick|reset|values_mut|split_off|resize|fill|or_insert|as_mut|borrow_mut|swap_level)\b'


def functions():
    """Each `fn` in the game's files: name -> list of bodies (several impls may share a name)."""
    out = collections.defaultdict(list)
    for path in FILES:
        text = path.read_text(encoding='utf-8', errors='replace')
        for m in re.finditer(r'\bfn\s+([a-z_][a-z0-9_]*)\s*(?:<[^>{]*>)?\s*\(', text):
            # The body: the first `{` after the signature, to its matching `}`.
            start = text.find('{', m.end())
            semi = text.find(';', m.end())
            if start < 0 or (0 <= semi < start):
                continue
            depth, i = 0, start
            while i < len(text):
                c = text[i]
                if c == '{':
                    depth += 1
                elif c == '}':
                    depth -= 1
                    if depth == 0:
                        break
                i += 1
            out[m.group(1)].append((path.name, text[start:i + 1]))
    return out


def parked_fields():
    text = (ROOT / 'game' / 'levels.rs').read_text(encoding='utf-8')
    swap = text[text.index('fn swap_level'):]
    swap = swap[:swap.index('\n    }\n')]
    fields = set(re.findall(r'self\.([a-z_][a-z0-9_]*)', swap))
    fields |= {'be_anims', 'raiding', 'sculk', 'creaking', 'creatures'}
    return fields


def main():
    names = [a for a in sys.argv[1:] if not a.startswith('--') and not a.isdigit()]
    writes_only = '--writes' in sys.argv
    depth_limit = int(sys.argv[sys.argv.index('--depth') + 1]) if '--depth' in sys.argv else 99
    fns = functions()
    level = parked_fields()
    seen, frontier = set(), [(n, 0) for n in names]
    fields = collections.defaultdict(lambda: [0, 0, set()])  # reads, writes, via
    while frontier:
        name, depth = frontier.pop()
        if name in seen or name not in fns:
            continue
        seen.add(name)
        for fname, body in fns[name]:
            for m in re.finditer(r'self\.([a-z_][a-z0-9_]*)(\s*\(|\s*::<)?', body):
                ident = m.group(1)
                if m.group(2):
                    if depth < depth_limit:
                        frontier.append((ident, depth + 1))
                    continue
                after = body[m.end():m.end() + 60]
                before = body[max(0, m.start() - 6):m.start()]
                wrote = (re.match(r'\s*(=[^=]|\+=|-=|\*=|\|=|&=)', after) is not None
                         or before.endswith('&mut ')
                         or re.match(r'(\.[a-z_0-9]+)*\.' + MUTATORS, after) is not None)
                entry = fields[ident]
                entry[1 if wrote else 0] += 1
                entry[2].add(name)
    rows = []
    for f, (r, w, via) in fields.items():
        if writes_only and not w:
            continue
        rows.append((f in level, f, r, w, sorted(via)[:4]))
    rows.sort(key=lambda x: (x[0], x[1]))
    print(f'{len(seen)} functions reached; {len(rows)} fields')
    for is_level, f, r, w, via in rows:
        print(f'{"level " if is_level else "GLOBAL"}  {f:32} reads {r:4} writes {w:4}  via {", ".join(via)}')


if __name__ == '__main__':
    main()
