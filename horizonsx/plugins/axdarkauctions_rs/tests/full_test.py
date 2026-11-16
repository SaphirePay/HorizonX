"""Every AxDarkAuctions feature one player can reach, on the axdalive rig: exit and entrance NPCs, the auctioneer NPC
opening the GUI, several bids, the bid page turning, a won item, the "can't pay" path (cheater, banned), view,
forcejoin, and a schedule starting the auction by itself."""
import datetime
import json
import pathlib
import re
import sys
import time

sys.stdout.reconfigure(encoding='utf-8', errors='replace')
sys.path.insert(0, r'D:\Horizon\fabric-port\tools')
import proof  # noqa: E402

proof.RUN = pathlib.Path(r'D:\Horizon\parity\axdalive')
CLIENT_LOG = proof.RUN / 'client' / 'logs' / 'latest.log'
DATA = proof.RUN / 'server' / 'plugins' / 'AxDarkAuctions'
RESULTS = []


def chat():
    text = CLIENT_LOG.read_text(encoding='utf-8', errors='replace') if CLIENT_LOG.exists() else ''
    return [l.split('[CHAT] ', 1)[1].rstrip() for l in text.splitlines() if '[CHAT] ' in l]


def say(line, wait=1.2):
    proof.ctl('chat', line)
    time.sleep(wait)


def check(name, ok, detail=''):
    RESULTS.append((name, ok))
    print(f'{"PASS" if ok else "FAIL"}  {name}  {detail}'[:300], flush=True)


def top():
    s = proof.ctl_json('screen')
    out = {}
    for x in s.get('slots', []) or []:
        if 300 <= x.get('slot', 0) < 400 and x.get('item'):
            names = re.findall(r'text:"((?:[^"\\]|\\.)*)"', x.get('components') or '')
            out[x['slot'] - 300] = (x['item'].replace('minecraft:', ''), x.get('count'), ''.join(names)[:60])
    return out


def kind(items):
    if items.get(5, ('',))[0] == 'clock':
        return 'bidding'
    if items.get(15, ('',))[0] == 'clock':
        return 'preview'
    return None


def wait_gui(want, seconds=30):
    end = time.time() + seconds
    while time.time() < end:
        items = top()
        if kind(items) == want:
            return items
        time.sleep(0.4)
    return None


def since(n):
    return chat()[n:]


def villagers():
    ents = proof.ctl_json('entities').get('entities') or []
    return {round(e.get('position', e.get('pos'))[2]): e['id'] for e in ents if e.get('type') == 'minecraft:villager'}


def main():
    say('/axda stopall')
    say('/axda reload', 1.5)
    proof.ctl('close')
    say('/eco set Prover 100000')
    n = len(chat())
    say('/axda start example', 1.5)
    say('/axda join example', 1.5)
    v = villagers()
    check('three NPCs shown (entrance, exit, auctioneer)', len(v) == 3, v)
    # Exit NPC at z=-33.5, entrance at -29.5, auctioneer at -28.5 (x 99.5).
    ents = proof.ctl_json('entities').get('entities') or []
    by = {(round((e.get('position') or e.get('pos'))[0]), round((e.get('position') or e.get('pos'))[2])): e['id'] for e in ents if e.get('type') == 'minecraft:villager'}
    exit_id, entrance_id, auctioneer_id = by.get((94, -34)) or by.get((93, -34)), by.get((94, -30)) or by.get((93, -30)), by.get((100, -28)) or by.get((99, -28))
    proof.ctl('interact-entity', str(exit_id)); time.sleep(1.5)
    check('exit NPC leaves', any('You have left' in l for l in since(n)), since(n)[-2:])
    proof.ctl('interact-entity', str(entrance_id)); time.sleep(1.5)
    check('entrance NPC joins', sum('You have joined' in l for l in since(n)) >= 2, since(n)[-2:])
    say('/axda view', 1.0)
    check('view before showing says nothing wrong', not any('not in a dark auction' in l for l in since(n)[-1:]), since(n)[-1:])
    # Showing: the preview opens by itself.
    items = wait_gui('preview')
    check('preview GUI opens with the item and clock', items is not None and 11 in items, items and {k: items[k] for k in (11, 15) if k in items})
    items = wait_gui('bidding')
    check('bidding GUI opens', items is not None, items and items.get(18))
    proof.ctl('close'); time.sleep(0.6)
    proof.ctl('interact-entity', str(auctioneer_id)); time.sleep(1.5)
    items = top()
    check('auctioneer NPC opens the bidding GUI', kind(items) == 'bidding', kind(items))
    first = items.get(18, ('', 0, ''))[2]
    proof.ctl('click-slot', '318'); time.sleep(1.4)
    proof.ctl('click-slot', '320'); time.sleep(1.4)
    items = top()
    bids = [l for l in since(n) if 'has bid' in l]
    check('two bids broadcast, higher each time', len(bids) >= 2, bids[-2:])
    check('the first slot now says a higher bid exists', items.get(18, ('',))[0] == 'barrier', items.get(18))
    check('the bid slot shows the bidder', items.get(20, ('',))[0] == 'player_head', items.get(20))
    # The page's last slot (53): the next page a second later, starting above the page before.
    last_before = items.get(53, ('', 0, ''))[2]
    proof.ctl('click-slot', '353'); time.sleep(2.6)
    items = top()
    check('bidding the last slot turns the page', items.get(18, ('', 0, ''))[2] not in ('', first), f'{first!r} -> {items.get(18)} (last was {last_before!r})')
    proof.ctl('close')
    end = time.time() + 25
    while time.time() < end and not any('has won' in l or 'Nobody has bid' in l for l in since(n)):
        time.sleep(0.5)
    won = [l for l in since(n) if 'has won' in l]
    check('the top bidder wins', bool(won), won[-1:] or since(n)[-3:])
    m = len(chat())
    say('/balance', 1.2)
    check('the winning bid is paid', any('$' in l and '100,000' not in l for l in since(m)), since(m)[-1:])
    # Next item: bid, then lose the money before the end: the cheater path.
    wait_gui('preview', 40)
    items = wait_gui('bidding', 20)
    if items is not None:
        proof.ctl('click-slot', '318'); time.sleep(1.2)
        say('/eco set Prover 0', 1.0)
        end = time.time() + 25
        while time.time() < end and not any('tried to cheat' in l for l in since(m)):
            time.sleep(0.5)
        check('a winner who cannot pay is called out', any('tried to cheat' in l for l in since(m)), since(m)[-3:])
        time.sleep(1.5)
        say('/axda join example', 1.2)
        check('and is banned from joining again', any('banned' in l for l in since(m)[-2:]), since(m)[-2:])
    else:
        check('second bidding round came', False)
    say('/axda stop example', 1.2)
    say('/eco set Prover 100000', 1.0)
    # forcejoin from the console needs a running auction: start, forcejoin, stop.
    m = len(chat())
    say('/axda start example', 1.2)
    say('/axda forcejoin example Prover', 1.5)
    check('forcejoin joins the player', any('joined' in l for l in since(m)), since(m)[-2:])
    say('/axda stop example', 1.2)
    # A schedule two minutes from now (local time).
    at = datetime.datetime.now() + datetime.timedelta(minutes=2)
    sched = DATA / 'schedulers.yml'
    text = sched.read_text(encoding='utf-8')
    text = re.sub(r'- "?\d+ \d+ \* \* \*"?.*', f'- "{at.minute} {at.hour} * * *"', text, count=1)
    sched.write_text(text, encoding='utf-8')
    m = len(chat())
    say('/axda reload', 1.2)
    say('/axda info example', 1.2)
    print('  info:', since(m)[-1:])
    target = at.replace(second=0, microsecond=0)
    while datetime.datetime.now() < target + datetime.timedelta(seconds=4):
        time.sleep(1)
    check('the schedule starts the auction by itself', any('is starting in' in l for l in since(m)), since(m)[-3:])
    say('/axda stop example', 1.0)
    passed = sum(ok for _, ok in RESULTS)
    print(f'{passed}/{len(RESULTS)} passed')


if __name__ == '__main__':
    main()
