"""What the compile repair dropped from src/generated.rs (gen.py --check writes dropped.json): grouped by rustc's
message with types and names blanked, so a binding-writer fix can take a whole group at once.

    python dropped.py [N]            the groups, biggest first, N examples each (default 2)
    python dropped.py grep WORD      the dropped lines that mention WORD (a Bukkit name: "setFloats")
"""
import collections
import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent


def shape(message):
    m = re.sub(r'`[^`]*`', '`_`', message)
    return re.sub(r'\d+', 'N', m)


def main():
    rows = json.loads((HERE / 'dropped.json').read_text(encoding='utf-8'))
    a = sys.argv[1:]
    if a and a[0] == 'grep':
        for r in rows:
            if a[1] in r['line']:
                print(r['message'])
                print('   ', r['line'][:300])
        return
    n = int(a[0]) if a else 2
    groups = collections.defaultdict(list)
    for r in rows:
        groups[shape(r['message'])].append(r)
    print(f'{len(rows)} lines dropped, {len(groups)} kinds of error')
    for k, rs in sorted(groups.items(), key=lambda x: -len(x[1])):
        print(f'\n{len(rs):4}  {k[:160]}')
        for r in rs[:n]:
            print(f'      {r["message"][:160]}')
            names = re.findall(r'name == "(\w+)"', r['line'])
            print(f'      {",".join(names[:3]) or "-"}: {r["line"][:200]}')


if __name__ == '__main__':
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
    main()
