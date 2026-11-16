"""How much of the SDK's Bukkit API the Java layer reaches, and why the rest doesn't.

For every SDK function with a `/// bukkit:` tag: is there an arm in src/generated.rs that calls it? If not, why:
generic, a parameter or return type the binding writer can't convert, or dropped by the compile repair.
Writes ../COVERAGE.md and prints the top reasons.
"""
import collections
import json
import pathlib
import re
import sys

sys.path.insert(0, r'D:\Horizon\plugin-tools\jport')
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import gen  # noqa: E402  (patches bindings.to_v as the generator does)
import bindings  # noqa: E402

HERE = pathlib.Path(__file__).resolve().parent
text = (HERE / 'src' / 'generated.rs').read_text(encoding='utf-8')

# The arms: per Rust type, the SDK methods called; the statics by (class, method).
called = collections.defaultdict(set)
for block in re.split(r'\nfn nc_\d+\(', text)[1:]:
    head = re.search(r'r\.native::<(?:Mutex<|jbridge::EvL<)(.*?)>>\(\)\?', block[:400])
    if not head:
        continue
    body = block.split('\nfn ', 1)[0]
    called[head.group(1)].update(re.findall(r'(?:\bt|\(\*t\)\.clone\(\))\.(\w+)\(', body))
statics = set(re.findall(r'class == "([^"]+)" && name == "([^"]+)"', text))

types = bindings.Types()
impls, _ = bindings.implementors()
traits = {i['rust'] for i in bindings.items() if i['kind'] == 'trait'}
rows = []
for item in bindings.items():
    if item['kind'] != 'fn' or not item['java']:
        continue
    tags = [j for j in item['java'] if '.' in j.rsplit('/', 1)[-1]]
    if not tags:
        continue
    path = item['rust']
    owner = path.split('::')[0] if '::' in path else ''
    rust_name = path.rsplit('::', 1)[-1]
    near = item['place'].split('.')[0]
    receiver, params = bindings.params_of(item['signature'])
    out = bindings.return_of(item['signature'])
    if '<' in item['signature'].split('(')[0]:
        reason = 'generic function'
    elif any(bindings.from_v(p, 'a[0]', 0, types, near) is None for p in params):
        bad = next(p for p in params if bindings.from_v(p, 'a[0]', 0, types, near) is None)
        reason = f'parameter type {re.sub(chr(39) + r"[a-z]+ ", "", bad)}'
    elif bindings.to_v(out, 'x') is None:
        reason = f'return type {out}'
    else:
        reason = None
    if receiver and owner:
        targets = impls.get(owner, ()) if owner in traits else (owner,)
        reached = any(rust_name in called.get(types.path(t, near), ()) for t in targets) or any(rust_name in called.get(p, ()) for p in called if p.endswith('::' + owner) or p == owner)
    else:
        reached = any((j.rsplit('/', 1)[-1].rsplit('.', 1)[0].split('$')[-1], j.rsplit('.', 1)[1]) in statics for j in tags)
    if reached:
        reason = None
    elif reason is None:
        reason = 'dropped by the compile repair' if receiver else 'static not routed'
    for tag in tags:
        rows.append((tag, reached, reason))

# One row per Bukkit method: reached when any SDK function tagged with it is (Player.getName has several).
by_tag = {}
for tag, reached, reason in rows:
    if tag not in by_tag or (reached and not by_tag[tag][1]):
        by_tag[tag] = (tag, reached, reason)
# What the Java layer answers itself (java_answers.txt, each checked by a probe).
answered = {l.strip() for l in (HERE / 'java_answers.txt').read_text(encoding='utf-8').splitlines() if l.strip() and not l.startswith('#')}
for tag in answered:
    if tag in by_tag and not by_tag[tag][1]:
        by_tag[tag] = (tag, True, None)
rows = list(by_tag.values())
total = len(rows)
ok = sum(1 for r in rows if r[1])

# What Java hands to the server: interfaces the server's objects stand behind, events (read and written back through
# their getters and setters), ItemStack, and Bukkit/Server statics. Classes, enums and the interfaces Java itself
# implements (configs, schedulers, Brigadier, plugin plumbing, Adventure) run as Java code and need nothing here.
api_kind = {}
for line in (HERE / 'api_kinds.txt').read_text(encoding='utf-8').splitlines():
    parts = line.split()
    if len(parts) == 3:
        api_kind[parts[2]] = (parts[0], parts[1])
JAVA_SIDE = ('org/bukkit/configuration/', 'org/bukkit/plugin/', 'org/bukkit/scheduler/', 'io/papermc/paper/command/brigadier/',
             'io/papermc/paper/plugin/', 'org/bukkit/metadata/', 'net/kyori/', 'org/bukkit/conversations/', 'org/bukkit/help/',
             'io/papermc/paper/registry/', 'org/bukkit/permissions/', 'org/bukkit/util/', 'io/papermc/paper/util/')


def relevant(tag):
    cls = tag.rsplit('.', 1)[0]
    kind, event = api_kind.get(cls, ('?', '--'))
    if event != '--':
        return True
    if cls in ('org/bukkit/inventory/ItemStack', 'org/bukkit/Bukkit', 'org/bukkit/Server'):
        return True
    return kind == 'I' and not cls.startswith(JAVA_SIDE)


rel = [r for r in rows if relevant(r[0])]
rel_ok = sum(1 for r in rel if r[1])
print(f'what Java hands to the server: {rel_ok} of {len(rel)} reached ({100 * rel_ok / max(1, len(rel)):.1f}%)')
reasons = collections.Counter(r[2] for r in rows if not r[1])
by_class = collections.Counter(r[0].rsplit('.', 1)[0] for r in rows if not r[1])
lines = [f'# Java layer: SDK reach', '', f'{ok} of {total} tagged Bukkit methods reached ({100 * ok / total:.1f}%).', '',
         f'Of what Java hands to the server (server objects\' interfaces, events, ItemStack, Bukkit/Server): {rel_ok} of {len(rel)} ({100 * rel_ok / max(1, len(rel)):.1f}%).', '',
         '## Why the rest is not', '']
lines += [f'- {n}: {why}' for why, n in reasons.most_common(60)]
lines += ['', '## Most gaps by class', ''] + [f'- {n}: {c}' for c, n in by_class.most_common(40)]
lines += ['', '## Gaps in what Java hands to the server', ''] + [f'- {tag}: {why}' for tag, reached, why in sorted(rel) if not reached]
lines += ['', '## Every gap', ''] + [f'- {tag}: {why}' for tag, reached, why in sorted(rows) if not reached]
(HERE.parent / 'COVERAGE.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')
print(f'{ok} of {total} reached ({100 * ok / total:.1f}%)')
for why, n in reasons.most_common(25):
    print(f'{n:6} {why}')
