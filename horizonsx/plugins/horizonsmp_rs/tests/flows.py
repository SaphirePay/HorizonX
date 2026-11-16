#!/usr/bin/env python3
"""HorizonSMP flows on the live rig (smp_live.py up first): shop buy/sell, crate spin, claim protection for a
non-op, LuckPerms nodes and meta perks, combat-free teleports. Prints PASS/FAIL per check.

    flows.py [shop|crate|claims|perms|all]
"""
import json
import os
import sys
import time

os.environ['MSYS_NO_PATHCONV'] = '1'
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import smp_live as rig  # noqa: E402

ctl = rig.ctl
results = []


def j(*args, timeout=40):
    out = ctl(*args, timeout=timeout)
    try:
        return json.loads(out.splitlines()[-1]) if out else {}
    except (json.JSONDecodeError, IndexError):
        return {'raw': out}


def say(line, settle=1.0):
    ctl('chat', line)
    time.sleep(settle)


def chat_since(n):
    return rig.chat_lines()[n:]


def check(name, ok, detail=''):
    results.append((name, ok))
    print(f"{'PASS' if ok else 'FAIL'}  {name}  {detail}"[:300], flush=True)


def balance():
    n = len(rig.chat_lines())
    say('/balance', 1.2)
    for line in chat_since(n):
        if 'Balance' in line:
            try:
                return float(line.split('$', 1)[1].replace(',', '').split()[0])
            except (IndexError, ValueError):
                pass
    return None


def count(item):
    inv = j('inventory')
    total = 0
    for k in ('hotbar', 'main', 'inventory', 'slots'):
        for s in inv.get(k, []) or []:
            if isinstance(s, dict) and item in str(s.get('item', s.get('id', ''))):
                total += int(s.get('count', 1))
    if not total and isinstance(inv.get('counts'), dict):
        total = sum(v for k, v in inv['counts'].items() if item in k)
    return total


def pos():
    st = j('state')
    p = st.get('player', {}).get('pos') or st.get('player', {}).get('position') or [0, 64, 0]
    return [int(float(v) // 1) for v in p]


def shop():
    say('/eco give Prover 5000', 1.0)
    before = balance()
    coal_before = count('coal')
    say('/shop ores', 1.5)
    ctl('click-slot', 310)  # first item: coal
    time.sleep(1.2)
    ctl('click-slot', 330)  # buy 1
    time.sleep(1.2)
    ctl('click-slot', 324)  # +10
    time.sleep(1.2)
    ctl('click-slot', 330)  # buy 11
    time.sleep(1.2)
    ctl('close')
    time.sleep(0.5)
    after = balance()
    coal_after = count('coal')
    check('shop buy 1 + 11 coal', coal_after - coal_before == 12, f'coal {coal_before}->{coal_after}')
    check('shop charged 12 x $12', before is not None and after is not None and abs((before - after) - 144) < 0.01, f'balance {before}->{after}')
    n = len(rig.chat_lines())
    say('/sell all', 1.5)
    lines = chat_since(n)
    check('sell all pays', any('Sold' in l and '$' in l for l in lines), ' | '.join(lines)[-160:])
    check('sell runs once', sum('Sold' in l for l in lines) == 1, f'{sum("Sold" in l for l in lines)} lines')
    check('coal gone after sell', count('coal') == 0)


def crate():
    x, y, z = pos()
    cx, cy, cz = x + 2, y, z
    ctl('setblock', cx, cy, cz, 'chest')
    time.sleep(0.8)
    say('/crate set rare', 0.8)
    ctl('look-at', 'block', cx, cy, cz)
    time.sleep(0.3)
    ctl('interact', cx, cy, cz)
    time.sleep(1.2)
    n = len(rig.chat_lines())
    say('/crate give Prover rare 1', 0.8)
    keys_before = count('tripwire_hook')
    ctl('select', 'tripwire_hook')
    time.sleep(0.4)
    ctl('interact', cx, cy, cz)
    time.sleep(1.5)
    scr = j('screen')
    check('crate opens a spin menu', 'Opening' in json.dumps(scr), str(scr.get('title', ''))[:80])
    time.sleep(9)
    lines = chat_since(n)
    check('crate pays a prize', any('You won' in l for l in lines), ' | '.join(lines)[-200:])
    check('crate took one key', count('tripwire_hook') == keys_before - 1, f'{keys_before}->{count("tripwire_hook")}')
    ctl('close')


def claims():
    x, y, z = pos()
    # Server land around a spot 30 blocks away, made while op; then the player loses op.
    say('/claim bypass', 0.6)
    say('/claim bypass', 0.6)
    ctl('tp', x + 40, y + 2, z)
    time.sleep(1.5)
    ctl('wait-ready', timeout=60)
    px, py, pz = pos()
    say('/claim admin 6', 1.0)
    say('/gamemode survival', 0.6)
    say('/deop Prover', 1.2)
    time.sleep(1.5)
    # A block under the player inside the server land.
    tx, tz = px + 2, pz + 2
    st = j('block', tx, py - 1, tz)
    before = st.get('block', st.get('name', ''))
    ctl('look-at', 'block', tx, py - 1, tz)
    r = j('break', tx, py - 1, tz, timeout=20)
    time.sleep(1.0)
    st2 = j('block', tx, py - 1, tz)
    after = st2.get('block', st2.get('name', ''))
    check('non-op cannot break server land', before == after and before not in ('', 'minecraft:air'), f'{before} -> {after} {str(r)[:80]}')
    n = len(rig.chat_lines())
    say('/claim admin 6', 1.0)
    lines = chat_since(n)
    check('non-op has no /claim admin', not any('Server land made' in l for l in lines), ' | '.join(lines)[-160:])


def perms():
    # Prover is not op here (claims() took it): the console grants and takes nodes through LuckPerms.
    def console(line):
        print('   console:', rig.proof.server('cmd', *line.split(), timeout=30)[:120])
        time.sleep(1.5)

    n = len(rig.chat_lines())
    say('/shop', 1.2)
    check('default player opens /shop', 'Shop' in json.dumps(j('screen')))
    ctl('close')
    console('lp user Prover permission set horizonsmp.shop false')
    n = len(rig.chat_lines())
    say('/shop', 1.5)
    scr = json.dumps(j('screen'))
    lines = chat_since(n)
    check('LuckPerms node takes /shop away', 'Server Shop' not in scr, ' | '.join(lines)[-160:])
    ctl('close')
    console('lp user Prover permission unset horizonsmp.shop')
    before = None
    n = len(rig.chat_lines())
    say('/claim blocks', 1.2)
    for l in chat_since(n):
        if 'Total' in l:
            before = l
    console('lp user Prover meta set horizonsmp.claim-blocks 5000')
    time.sleep(31)  # the meta cache
    n = len(rig.chat_lines())
    say('/claim blocks', 1.2)
    after = next((l for l in chat_since(n) if 'Total' in l), None)
    num = lambda l: int(''.join(ch for ch in (l or '').split('Total')[-1] if ch.isdigit()) or 0)
    check('LuckPerms meta adds 5000 claim blocks', num(after) - num(before) >= 5000, f'{before} -> {after}')
    console('lp user Prover meta unset horizonsmp.claim-blocks')
    console('op Prover')


def menu():
    # The hub's buttons open their screens (economy row: Shop first).
    st = j('state')
    check('test player alive', float(st.get('player', {}).get('health') or 0) > 0, str(st.get('player', {}).get('health')))
    say('/menu', 1.5)
    ctl('click-slot', 311)
    time.sleep(1.5)
    check('menu Shop button opens the shop', 'Server Shop' in json.dumps(j('screen')))
    ctl('click-slot', 345)  # back
    time.sleep(1.5)
    check('shop Back returns to the menu', 'Nyxora' in json.dumps(j('screen')))
    ctl('close')
    time.sleep(0.5)
    n = len(rig.chat_lines())
    say('/claim deleteall', 1.5)
    ctl('click-slot', 311)  # yes
    time.sleep(1.5)
    check('kit yes/no menu runs its action', any('deleted' in l for l in chat_since(n)), ' | '.join(chat_since(n))[-120:])
    say('/sell', 1.5)
    check('sell chest opens', 'Sell items' in json.dumps(j('screen')))
    ctl('close')
    time.sleep(1.0)


def main():
    what = sys.argv[1] if len(sys.argv) > 1 else 'all'
    for name, f in (('menu', menu), ('shop', shop), ('crate', crate), ('claims', claims), ('perms', perms)):
        if what in (name, 'all'):
            try:
                f()
            except Exception as e:  # noqa: BLE001
                check(f'{name} ran', False, repr(e))
    print(f"{sum(ok for _, ok in results)}/{len(results)} passed")


if __name__ == '__main__':
    main()
