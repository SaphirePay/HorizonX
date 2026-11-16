#!/usr/bin/env python3
"""A player-placed ore must pay nothing when broken (place-and-break). Run with the rig up and Prover a Miner."""
import json
import re
import time

import jobs_rig  # noqa: F401  (sets the rig up)
import smp_live

c = smp_live.ctl


def balance():
    c('chat', '/balance')
    time.sleep(1.2)
    for line in reversed(smp_live.chat_lines()):
        m = re.search(r'Balance: \$([\d,.]+)', line)
        if m:
            return float(m.group(1).replace(',', ''))
    return None


st = json.loads(c('state'))
x, y, z = st['player']['block_pos']
c('give', 'diamond_ore', '1')
time.sleep(0.6)
placed = None
for dx, dz in [(1, 0), (-1, 0), (0, 1), (0, -1), (2, 0), (0, 2)]:
    pos = (x + dx, y, z + dz)
    r = json.loads(c('place', *pos, '--item', 'diamond_ore'))
    print(pos, str(r)[:200])
    if r.get('ok'):
        placed = pos
        break
if not placed:
    raise SystemExit('could not place an ore next to the player')
time.sleep(0.6)
before = balance()
c('select', '3')
print(c('break', *placed, timeout=30)[:160])
time.sleep(4)
after = balance()
print(f'balance before {before} after {after}')
print('PASS: placed ore paid nothing' if before == after else 'FAIL: placed ore paid')
