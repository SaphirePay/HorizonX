"""One dark auction on the axdalive rig: start, join, see the preview GUI, bid in the bidding GUI, see the winner
paid and given the reward."""
import json
import pathlib
import sys
import time

sys.stdout.reconfigure(encoding='utf-8', errors='replace')
sys.path.insert(0, r'D:\Horizon\fabric-port\tools')
import proof  # noqa: E402

proof.RUN = pathlib.Path(r'D:\Horizon\parity\axdalive')
CLIENT_LOG = proof.RUN / 'client' / 'logs' / 'latest.log'


def chat():
    text = CLIENT_LOG.read_text(encoding='utf-8', errors='replace') if CLIENT_LOG.exists() else ''
    return [l.split('[CHAT] ', 1)[1].rstrip() for l in text.splitlines() if '[CHAT] ' in l]


def top():
    s = proof.ctl_json('screen')
    out = {}
    for x in s.get('slots', []) or []:
        if 300 <= x.get('slot', 0) < 400 and x.get('item'):
            out[x['slot'] - 300] = (x['item'].replace('minecraft:', ''), x.get('count'))
    return s.get('screen'), out


def balance():
    seen = len(chat())
    proof.ctl('chat', '/balance')
    time.sleep(1.2)
    return [l for l in chat()[seen:] if '$' in l]


def main():
    seen = len(chat())
    inv_before = proof.ctl_json('inventory')
    print('balance before:', balance())
    for line in ['/axda start example', '/axda join example']:
        proof.ctl('chat', line)
        time.sleep(1.0)
    phase = None
    bid_done = False
    started = time.time()
    while time.time() - started < 45:
        screen, items = top()
        kind = 'bidding' if items.get(5, ('',))[0] == 'clock' else 'preview' if items.get(15, ('',))[0] == 'clock' else None
        fresh = kind == 'bidding' and phase == 'preview'
        if kind != phase:
            phase = kind
            shown = {k: v for k, v in items.items() if 'glass_pane' not in v[0]}
            print(f'{time.time() - started:5.1f}s gui={kind} {dict(sorted(shown.items()))}')
        if fresh and not bid_done:
            print('bid on slot 18:', proof.ctl('click-slot', '318'))
            bid_done = True
            time.sleep(1.5)
            _, after = top()
            print('slot 18 after bid:', after.get(18), ' slot 45:', after.get(45))
        if bid_done and kind is None:
            break
        time.sleep(0.5)
    time.sleep(3)
    print('---- chat')
    for line in chat()[seen:]:
        print(line[:200])
    print('balance after:', balance())
    inv = proof.ctl_json('inventory')
    print('counts before:', {k: v for k, v in (inv_before.get('counts') or {}).items() if 'iron' in k or 'diamond' in k})
    print('counts after: ', {k: v for k, v in (inv.get('counts') or {}).items() if 'iron' in k or 'diamond' in k})
    print('entities near:', [(e.get('type'), e.get('id')) for e in (proof.ctl_json('entities').get('entities') or [])][:12])


if __name__ == '__main__':
    main()
