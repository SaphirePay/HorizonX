#!/usr/bin/env python3
"""Hunter pays for a zombie kill. Rig up, Prover a Hunter (jobs_flow.py joins every job)."""
import json
import re
import sys
import time

import jobs_rig  # noqa: F401
import smp_live

c = smp_live.ctl


def balance():
    time.sleep(3.5)
    c('chat', '/balance')
    time.sleep(1.2)
    for line in reversed(smp_live.chat_lines()):
        m = re.search(r'Balance: \$([\d,.]+)', line)
        if m:
            return float(m.group(1).replace(',', ''))
    return None


def zombie():
    ents = json.loads(c('entities')).get('entities', [])
    return next((e for e in ents if e.get('type') == 'minecraft:zombie' and e.get('id') != 1), None)


z = None
if z is None:
    c('chat', '/summon minecraft:zombie ~1.5 ~ ~ {NoAI:1b}')
    time.sleep(1)
    z = zombie()
print('zombie', z)
before = balance()
c('select', '1')
for _ in range(20):
    z = zombie()
    if z is None:
        break
    c('look-at-entity', z['id'])
    r = c('attack', z['id'])
    print(r[:160])
    if 'out of reach' in r:
        px, py, pz = z['pos']
        c('goto', int(px), int(py), int(pz), timeout=30)
    time.sleep(0.7)
after = balance()
paid = (after or 0) - (before or 0)
print(f'{"PASS" if paid > 0 else "FAIL"} zombie kill paid {paid:+.2f}')
sys.exit(0 if paid > 0 else 1)
