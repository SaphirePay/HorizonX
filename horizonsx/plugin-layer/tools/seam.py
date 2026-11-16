#!/usr/bin/env python3
"""Say which injection kinds a function of the pinned source takes, before writing a hook.

Usage: seam.py NAME [FILE_WORD]    NAME is a function, FILE_WORD narrows the file (game/remote.rs)

The rules are horizon-mixins' own, read from its planner. Rust's borrow checker still has the last
word, and a value or call site can be refused for reasons only `cycle.py --check` sees.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
COMMIT = 'cbfae59314dfa74d54cb8a379cae609c6f5ec91d'
CRATES = ROOT / 'target' / 'source' / COMMIT / 'crates'
MACROS_OK = {'format', 'matches', 'vec'}
ITEM = re.compile(r'^\s*(use|fn|const|static|struct|enum|impl|type|mod|trait)\s', re.M)


def code_of(text):
    """The text with comments and string contents blanked, line numbers kept."""
    out, i, n = [], 0, len(text)
    while i < n:
        two = text[i:i + 2]
        if two == '//':
            while i < n and text[i] != '\n':
                i += 1
        elif two == '/*':
            end = text.find('*/', i)
            i = n if end < 0 else end + 2
        elif re.match(r'b?r#*"', text[i:i + 12]) and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == '_')):
            opening = re.match(r'b?r(#*)"', text[i:i + 12])
            end = text.find('"' + opening.group(1), i + opening.end())
            i = n if end < 0 else end + 1 + len(opening.group(1))
            out.append('""')
        elif text[i] == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == '\\' else 1
            i += 1
            out.append('""')
        elif re.match(r"'(\\.|[^\\'])'", text[i:i + 4]):
            i = text.index("'", i + 1) + 1
            out.append("' '")
        else:
            out.append(text[i])
            i += 1
    return ''.join(out)


def find(name, word):
    for path in sorted(CRATES.rglob('*.rs')):
        if word and word.replace('\\', '/') not in path.as_posix():
            continue
        code = code_of(path.read_text(encoding='utf-8', errors='replace'))
        for match in re.finditer(rf'\bfn {re.escape(name)}\b', code):
            brace = code.find('{', match.end())
            semicolon = code.find(';', match.end())
            if brace < 0 or (0 <= semicolon < brace):
                continue
            depth, i = 0, brace
            while i < len(code):
                depth += {'{': 1, '}': -1}.get(code[i], 0)
                if depth == 0:
                    break
                i += 1
            yield path, code[:match.start()].count('\n') + 1, code[match.start():brace], code[brace + 1:i]


def report(path, line, head, body):
    result = head.split('->')[-1].strip() if '->' in head else '()'
    macros = sorted(set(re.findall(r'(?<![\w:])(\w+(?:::\w+)*)!\s*[(\[{]', body)) - MACROS_OK)
    closures = len(re.findall(r'(?<![|\w])\|[^|\n]*\|\s*[^|=]|\|\|\s', body))
    tries = len(re.findall(r'[\w)\]]\?', body))
    items = len(ITEM.findall(body))
    self_result = bool(re.search(r'\bSelf\b', result)) or result.startswith('impl')
    generic = bool(re.match(r'fn \w+<\s*[A-Z]', head))
    refs = len(re.findall(r'&\s*(?:mut\s+)?[\w.]+\([^)]*\)', body))
    print(f'{path.relative_to(CRATES)}:{line}  {" ".join(head.split())[:110]}')
    print(f'  {len(body.splitlines())} lines; closures {closures}, ? {tries}, nested items {items}, calls under & {refs}, macros {macros or "none"}')
    bad = [why for why, on in (('nested item', items), ('macro', macros), ('generic', generic)) if on]
    verdicts = {
        'HEAD': 'yes' if not generic else 'no: generic',
        'cancellable HEAD': 'no: the result names Self' if self_result else ('yes' if not generic else 'no: generic'),
        'RETURN': 'no: ' + ', '.join(bad + (['?'] if tries else []) + (['Self result'] if self_result else [])) if bad or tries or self_result else 'yes (unless a call under & is in the value it returns)',
        'MODIFY_VARIABLE': 'no: ' + ', '.join(bad) if bad else 'yes: reads of plain numbers and bools, stores of owned values with no closure or ? inside',
        'BEFORE_CALL, AFTER_CALL, WRAP, OVERWRITE, value': 'no: ' + ', '.join(bad + (['closures'] if closures else []) + (['?'] if tries else []) + (['calls under &'] if refs else []))
        if bad or closures or tries or refs else 'yes',
    }
    for kind, verdict in verdicts.items():
        print(f'  {kind}: {verdict}')


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    word = sys.argv[2] if len(sys.argv) > 2 else ''
    found = list(find(sys.argv[1], word))
    if not found:
        sys.exit(f'no function {sys.argv[1]} found')
    for hit in found[:8]:
        report(*hit)
    if len(found) > 8:
        print(f'... and {len(found) - 8} more: name a file')


if __name__ == '__main__':
    main()
