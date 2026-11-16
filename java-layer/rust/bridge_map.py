"""Paper's bridge services (ItemComponentTypesBridge, DialogInstancesProvider, ConsumableTypesBridge...) against the
statics the server has (src/generated.rs): which bridge method is which API factory, matched by the words of their
names (plainMessageDialogBody <-> DialogBody.plainMessage). Writes java/src/horizon/javalayer/BridgeAliases.java and
prints what has no match.

    python bridge_map.py
"""
import collections
import os
import pathlib
import re
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
LAYER = HERE.parent
LIB = LAYER / 'java' / 'out' / 'lib'
OUT = LAYER / 'java' / 'src' / 'horizon' / 'javalayer' / 'BridgeAliases.java'
JAVAP = r'C:\Program Files\Java\jdk-25\bin\javap.exe'
SERVICES = ['io.papermc.paper.datacomponent.item.ItemComponentTypesBridge', 'io.papermc.paper.registry.data.dialog.DialogInstancesProvider',
            'io.papermc.paper.datacomponent.item.consumable.ConsumableTypesBridge', 'io.papermc.paper.datacomponent.item.blocksattacks.BlocksAttacksBridge',
            'io.papermc.paper.datacomponent.item.attribute.AttributeModifierDisplayBridge', 'io.papermc.paper.block.bed.BedEnterActionBridge',
            'io.papermc.paper.registry.data.InlinedRegistryBuilderProvider']


def words(s):
    return [w.lower() for w in re.findall(r'[A-Z]?[a-z0-9]+|[A-Z]+(?![a-z])', s)]


def statics():
    text = (HERE / 'src' / 'generated.rs').read_text(encoding='utf-8')
    out = collections.defaultdict(set)
    for cls, name, n in re.findall(r'class == "(\w+)" && name == "(\w+)" && a\.len\(\) == (\d+)', text):
        out[(cls, name)].add(int(n))
    return out


def bridge_methods(service):
    cp = os.pathsep.join(str(j) for j in LIB.glob('*.jar'))
    done = subprocess.run([JAVAP, '-cp', cp, service], capture_output=True, text=True)
    rows = []
    for line in done.stdout.splitlines():
        m = re.search(r'abstract (?:<.*?> )?([\w.$<>?, ]+?) (\w+)\((.*)\);', line)
        if m:
            params = [p for p in m.group(3).split(',') if p.strip()] if m.group(3).strip() else []
            # generics inside params contain commas: count top-level ones
            depth, count = 0, 0
            for ch in m.group(3):
                if ch == '<':
                    depth += 1
                elif ch == '>':
                    depth -= 1
                elif ch == ',' and depth == 0:
                    count += 1
            n = 0 if not m.group(3).strip() else count + 1
            rows.append((m.group(2), n, m.group(1).split('.')[-1].split('$')[-1].split('<')[0]))
    return rows


# Bridge methods whose API factory has another name (checked by hand against src/generated.rs).
MANUAL = {
    'DialogInstancesProvider.textBuilder/2': ('DialogInput', 'text', 2),
    'DialogInstancesProvider.booleanBuilder/2': ('DialogInput', 'bool', 2),
    'DialogInstancesProvider.numberRangeBuilder/4': ('DialogInput', 'numberRange', 4),
    'DialogInstancesProvider.singleOptionBuilder/3': ('DialogInput', 'singleOption', 3),
    'DialogInstancesProvider.singleOptionEntry/3': ('OptionEntry', 'create', 3),
    'DialogInstancesProvider.customClick/2': ('DialogAction', 'customClick', 2),
    'ItemComponentTypesBridge.modifiers/0': ('ItemAttributeModifiers', 'itemAttributes', 0),
    'ItemComponentTypesBridge.mapDecorationEntry/4': ('MapDecorations', 'decorationEntry', 4),
    'DialogInstancesProvider.itemDialogBodyBuilder/1': ('DialogBody', 'item', 1),
    'ConsumableTypesBridge.removeStatusEffects/1': ('ConsumeEffect', 'removeEffects', 1),
    'ConsumableTypesBridge.playSoundEffect/1': ('ConsumeEffect', 'playSoundConsumeEffect', 1),
    'BlocksAttacksBridge.blocksAttacksDamageReduction/0': ('DamageReduction', 'damageReduction', 0),
    'BlocksAttacksBridge.blocksAttacksItemDamageFunction/0': ('ItemDamageFunction', 'itemDamageFunction', 0),
}
# Answered in Java (Services.invoke), not by a server factory: counted as mapped.
JAVA = {
    'InlinedRegistryBuilderProvider.createDialog/1', 'ItemComponentTypesBridge.enchantments/0', 'ItemComponentTypesBridge.lore/0',
    'BedEnterActionBridge.createTooFarAwayProblem/0', 'BedEnterActionBridge.createObstructedProblem/0', 'BedEnterActionBridge.createNotSafeProblem/0',
    'BedEnterActionBridge.createExplosionProblem/0', 'BedEnterActionBridge.createOtherProblem/0', 'DialogInstancesProvider.register/2',
    'InlinedRegistryBuilderProvider.createTrimMaterial/1', 'InlinedRegistryBuilderProvider.createTrimPattern/1', 'DialogInstancesProvider.multilineOptions/2',
}


def main():
    have = statics()
    by_words = collections.defaultdict(list)
    for (cls, name), ns in have.items():
        by_words[frozenset(words(cls) + words(name))].append((cls, name, ns))
        by_words[frozenset(words(name))].append((cls, name, ns))
    lines, missing, found = [], [], 0
    for service in SERVICES:
        simple = service.rsplit('.', 1)[-1]
        for name, n, ret in bridge_methods(service):
            key = f'{simple}.{name}/{n}'
            if key in JAVA:
                found += 1
                continue
            if key in MANUAL:
                o, s, k = MANUAL[key]
                found += 1
                lines.append(f'        A.put("{key}", new String[]{{"{o}", "{s}", "{k}"}});')
                continue
            candidates = by_words.get(frozenset(words(name)), []) + by_words.get(frozenset(words(name) + words(ret)), [])
            pick = None
            for cls, sname, ns in candidates:
                if n in ns:
                    pick = (cls, sname, n)
                    break
            if pick is None:
                for cls, sname, ns in candidates:
                    pick = (cls, sname, min(ns))
                    break
            if pick:
                found += 1
                lines.append(f'        A.put("{key}", new String[]{{"{pick[0]}", "{pick[1]}", "{pick[2]}"}});')
            else:
                missing.append(key)
    src = ['package horizon.javalayer;', '', '// Generated by rust/bridge_map.py: Paper bridge methods -> the server\'s API factories (Owner, name, argc).',
           'final class BridgeAliases {', '    private BridgeAliases() {}', '    static final java.util.Map<String, String[]> A = new java.util.HashMap<>();', '    static {']
    src += lines + ['    }', '}', '']
    OUT.write_text('\n'.join(src), encoding='utf-8')
    print(f'{found} bridge methods matched, {len(missing)} not: {OUT}')
    for k in missing:
        print('  no match:', k)


if __name__ == '__main__':
    main()
