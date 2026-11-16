#!/usr/bin/env python3
"""Every job pays for its work and nothing else: logs, natural vs placed leaves, ripe vs unripe wheat, a zombie.
Run with the rig up (jobs_rig.py up). Prover gets every job by admin join."""
import json
import re
import time

import jobs_rig  # noqa: F401
import smp_live

c = smp_live.ctl


def balance():
    time.sleep(3.5)  # pay goes out every 3 s
    c('chat', '/balance')
    time.sleep(1.2)
    for line in reversed(smp_live.chat_lines()):
        m = re.search(r'Balance: \$([\d,.]+)', line)
        if m:
            return float(m.group(1).replace(',', ''))
    return None


for job in ('lumberjack', 'farmer', 'hunter', 'fisher'):
    c('chat', f'/jobs admin join Prover {job}')
    time.sleep(0.4)
st = json.loads(c('state'))
x, y, z = st['player']['block_pos']
spot = (x + 2, y, z)
results = []


def case(name, block, expect_pay):
    if block.startswith('wheat'):
        c('setblock', spot[0], spot[1] - 1, spot[2], 'farmland')
    c('setblock', *spot, block)
    time.sleep(0.4)
    before = balance()
    r = c('break', *spot, timeout=30)
    after = balance()
    paid = (after or 0) - (before or 0)
    ok = (paid > 0) == expect_pay
    results.append(ok)
    print(f'{"PASS" if ok else "FAIL"} {name}: paid {paid:+.2f} ({r[:60]})')


c('select', '4')  # axe
case('natural oak log', 'oak_log', True)
case('placed (persistent) leaves', 'oak_leaves[persistent=true]', False)
case('natural leaves', 'oak_leaves[persistent=false]', True)
c('select', '2')
case('ripe wheat', 'wheat[age=7]', True)
case('unripe wheat', 'wheat[age=2]', False)
case('dirt (no job pays)', 'dirt', False)
print(f'{sum(results)}/{len(results)} passed (zombie kill: kill_check.py, placed ore: placed_check.py)')
