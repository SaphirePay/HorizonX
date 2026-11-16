#!/usr/bin/env python3
"""The Bukkit/Paper -> horizon-bukkit index, for people and AIs writing plugins: every Java method the SDK has, with
the Rust function that stands for it, its signature and where it lives. Read from the SDK's own `/// bukkit:` tags.

Usage: python -I tools/sdk_index.py [OUT.md]     (default sdk/horizon-bukkit/API_INDEX.md)
Also writes OUT's .tsv sibling: java_class, java_method, rust_path, signature, approx (one row a mapping), for grep.
"""
import collections
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / 'sdk' / 'horizon-bukkit' / 'src'

INLINE_MOD = re.compile(r'^(\s*)pub mod (\w+)\s*\{')
IMPL = re.compile(r'^(\s*)impl(?:<[^>]*>)?\s+(?:[\w:<>, ]+\s+for\s+)?([A-Z]\w*)(?:<[^>]*>)?\s*\{')
FN = re.compile(r'^\s*pub(?:\([^)]*\))?\s+(?:const\s+)?(?:unsafe\s+)?fn\s+(\w+)')
TAG = re.compile(r'^\s*///\s*bukkit(~?):\s*(.+)$')
STRUCT = re.compile(r'^\s*pub\s+(?:struct|enum|trait|type)\s+([A-Z]\w*)')
REEXPORT = re.compile(r'^pub use (?:crate::|self::)?([\w:]+)::\{?([^;]+?)\}?;')


def signature(lines, start):
    """The fn's signature up to its body: `fn name(args) -> out`, on one line."""
    text = []
    for line in lines[start:start + 12]:
        text.append(line.strip())
        if '{' in line or line.rstrip().endswith(';'):
            break
    joined = ' '.join(text)
    joined = joined.split('{', 1)[0].rstrip().rstrip(';')
    joined = re.sub(r'\s+', ' ', joined)
    joined = re.sub(r'^pub(\([^)]*\))?\s+', '', joined)
    joined = joined.replace('( ', '(').replace(' )', ')').replace(', )', ')')
    return joined


def main():
    out = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / 'sdk' / 'horizon-bukkit' / 'API_INDEX.md'
    # Where each type is defined, and what the crate root re-exports.
    defined = {}
    for path in sorted(SRC.glob('*.rs')):
        module = path.stem
        for line in path.read_text(encoding='utf-8', errors='replace').splitlines():
            m = STRUCT.match(line)
            if m and line.startswith(('pub ', 'pub(')) and m.group(1) not in defined:
                defined[m.group(1)] = 'horizon_bukkit' if module == 'lib' else f'horizon_bukkit::{module}'
    root_exports = set()
    for line in (SRC / 'lib.rs').read_text(encoding='utf-8').splitlines():
        m = REEXPORT.match(line.strip())
        if m:
            for name in m.group(2).split(','):
                root_exports.add(name.strip().split(' as ')[-1])
    rows = []
    for path in sorted(SRC.glob('*.rs')):
        module = path.stem
        lines = path.read_text(encoding='utf-8', errors='replace').splitlines()
        impl_stack = []  # (indent, type)
        mod_stack = []   # (indent, name): `pub mod x { ... }` written inside the file
        pending = None   # (approx, [java names])
        for i, line in enumerate(lines):
            stripped = line.strip()
            indent = len(line) - len(line.lstrip())
            im = INLINE_MOD.match(line)
            m = IMPL.match(line)
            if im:
                mod_stack.append((len(im.group(1)), im.group(2)))
            elif m:
                impl_stack.append((len(m.group(1)), m.group(2)))
            elif stripped == '}' and impl_stack and indent == impl_stack[-1][0]:
                impl_stack.pop()
            elif stripped == '}' and mod_stack and indent == mod_stack[-1][0]:
                mod_stack.pop()
            t = TAG.match(line)
            if t:
                names = [n.strip() for n in t.group(2).split(',') if '/' in n or '.' in n]
                pending = (t.group(1) == '~', (pending[1] if pending else []) + names)
                continue
            if pending and stripped.startswith('///'):
                continue
            if pending and stripped.startswith('#['):
                continue
            f = FN.match(line)
            if pending and f:
                approx, names = pending
                pending = None
                owner = impl_stack[-1][1] if impl_stack else None
                if owner:
                    base = 'horizon_bukkit' if owner in root_exports else defined.get(owner, f'horizon_bukkit::{module}')
                    rust = f'{base}::{owner}::{f.group(1)}'
                else:
                    base = 'horizon_bukkit' if module == 'lib' else f'horizon_bukkit::{module}'
                    rust = base + ''.join(f'::{name}' for _, name in mod_stack) + f'::{f.group(1)}'
                sig = signature(lines, i)
                for name in names:
                    java = name.replace('/', '.')
                    cls, _, method = java.rpartition('.')
                    rows.append((cls, method, rust, sig, approx))
                continue
            if pending and stripped and not stripped.startswith(('///', '#[')):
                pending = None
    by_class = collections.defaultdict(list)
    for row in rows:
        by_class[row[0]].append(row)
    tsv = out.with_suffix('.tsv')
    tsv.write_text('java_class\tjava_method\trust_path\tsignature\tapprox\n' + ''.join(
        f'{c}\t{m}\t{r}\t{s}\t{"~" if a else ""}\n' for c, m, r, s, a in sorted(rows)), encoding='utf-8')
    parts = [
        '# horizon-bukkit API index',
        '',
        'Generated by `tools/sdk_index.py` from the SDK\'s `/// bukkit:` tags: every Bukkit/Paper method the SDK has, the',
        'Rust function that stands for it and its signature. Search this file for the Java name you know',
        '(`Player.setHealth`, `getConfig`, `InventoryClickEvent`). `~` marks a mapping that works but is not exact (its doc',
        'comment says how). The same rows, one a line, are in API_INDEX.tsv.',
        '',
        f'{len(rows)} mappings over {len(by_class)} Java classes.',
        '',
    ]
    for cls in sorted(by_class):
        short = cls.rsplit('.', 1)[-1]
        parts.append(f'## {short}  ({cls})')
        seen = set()
        for _, method, rust, sig, approx in sorted(by_class[cls], key=lambda r: r[1]):
            key = (method, rust)
            if key in seen:
                continue
            seen.add(key)
            parts.append(f'- `{short}.{method}`{" ~" if approx else ""} → `{rust}` — `{sig}`')
        parts.append('')
    out.write_text('\n'.join(parts), encoding='utf-8')
    print(f'{len(rows)} mappings, {len(by_class)} classes -> {out} and {tsv}')


if __name__ == '__main__':
    main()
