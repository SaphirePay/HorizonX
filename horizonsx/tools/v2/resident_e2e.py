#!/usr/bin/env python3
"""HorizonsX's resident regions with a real Horizon client, end to end.

    python -I tools/v2/resident_e2e.py --server-bin D:/Horizon/perf/hxb34 --client-bin D:/Horizon/v2-target/fast

A HorizonsX dedicated server (HORIZON_RESIDENT=1, a flat world, simulation distance 7, view distance 10) with
two groups of five bots 1024 blocks apart, so the level runs as regions that stay resident on their threads,
and one Horizon client (offscreen) joined next to the second group. While the regions are resident (the
server's report counts their fast ticks), the client must see what a vanilla server shows:

- chunks:   the column under the client and one 8 chunks away are loaded on the client
- entities: pigs summoned beside it show up, and their wandering reaches the client (tracking)
- break:    a block it mines is air on the server too
- place:    a block it places is on the server
- chat:     its chat line reaches the server's log (a packet that reaches past the region)

Logs and the client's log go to --out.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import lab  # noqa: E402
from lan_e2e import Game  # noqa: E402
from server_horizon_only_e2e import Failed, server  # noqa: E402

EXE = '.exe' if os.name == 'nt' else ''
PLAYER = 'Resident'
HOME = (1024, 0)
Y = -60
PROPS = {
    'online-mode': 'false',
    'white-list': 'false',
    'level-type': 'minecraft\\:flat',
    'generate-structures': 'false',
    'gamemode': 'creative',
    'force-gamemode': 'true',
    'view-distance': '10',
    'simulation-distance': '7',
    'spawn-protection': '0',
}


def bots(bins, port, name):
    return subprocess.Popen(
        [bins / f'horizon-bots{EXE}', '--server', f'127.0.0.1:{port}', '--bots', '5', '--behaviour', 'walk',
         '--seconds', '900', '--name', name],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def passes(s, command, timeout=20):
    """Whether a console `execute if ...` test passes (its "Test passed" / "Test failed" line)."""
    mark = len(s.lines)
    s.console(command)
    deadline = time.time() + timeout
    while time.time() < deadline:
        for line in s.lines[mark:]:
            if 'Test passed' in line:
                return True
            if 'Test failed' in line:
                return False
        time.sleep(0.1)
    raise Failed(f'no answer to `{command}`')


def listed(reply):
    """The entities of an `entities` reply, by id: their positions."""
    found = reply.get('entities') if isinstance(reply, dict) else reply
    if isinstance(reply, dict) and found is None:
        found = next((v for v in reply.values() if isinstance(v, list)), [])
    out = {}
    for e in found or []:
        out[e.get('id', id(e))] = e.get('position') or e.get('pos') or [e.get('x'), e.get('y'), e.get('z')]
    return out


def fast_ticks(s, folder):
    """The resident regions' fast ticks over the last 3 s, from the server's own report (`horizon-report`)."""
    reports = folder / 'reports'
    before = set(reports.glob('*.json')) if reports.is_dir() else set()
    s.console('horizon-report 3')
    deadline = time.time() + 30
    while time.time() < deadline:
        new = (set(reports.glob('*.json')) if reports.is_dir() else set()) - before
        if new:
            time.sleep(0.5)
            d = json.loads(max(new, key=lambda p: p.stat().st_mtime).read_text(encoding='utf-8'))
            spans = d.get('spans') or []
            items = spans.items() if isinstance(spans, dict) else [(x['name'], x) for x in spans]
            return next((v.get('ticks_seen', 0) for k, v in items if k == 'rr_launch'), 0)
        time.sleep(0.2)
    raise Failed('no report from horizon-report')


def check(server_bins, client_bins, scratch, out, resident_on=True):
    if resident_on:
        os.environ['HORIZON_RESIDENT'] = '1'
    else:
        os.environ.pop('HORIZON_RESIDENT', None)
    folder = scratch / 'server'
    s, port = server(server_bins, folder, out, 'server', PROPS)
    groups = []
    game = Game('client', client_bins / f'horizon-ctl{EXE}', lab.RESOURCES.resolve())
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'HORIZON_LAUNCHER_HOME'):
        game.d.env[key] = str(game.work / key.lower())
    results = {}
    try:
        s.console('gamerule spawn_mobs false')
        groups = [bots(server_bins, port, 'A'), bots(server_bins, port, 'B')]
        s.wait_for(r'A4 joined the game', 120)
        s.wait_for(r'B4 joined the game', 120)
        for i in range(5):
            s.console(f'tp B{i} {HOME[0] + 2 * i} {Y} {HOME[1] - 4}')
        game('launch', wait=False, offscreen=True)
        game.raw('connect', f'127.0.0.1:{port}', PLAYER)
        game.raw('wait-for', 'loaded', '--timeout', 300)
        s.wait_for(rf'{PLAYER} joined the game', 60)
        s.console(f'op {PLAYER}')
        s.console(f'tp {PLAYER} {HOME[0] + 0.5} {Y} {HOME[1] + 6.5}')
        s.console(f'give {PLAYER} minecraft:stone 64')
        game.wait(200)
        resident = fast_ticks(s, folder)
        results['resident'] = resident
        if resident <= 0 and resident_on:
            raise Failed('the regions never stayed resident (no fast ticks in the report)')
        print(f'resident: {resident} fast ticks in 3 s')

        # chunks: under the client, and 8 chunks east
        here = game.raw('block', HOME[0], Y - 1, HOME[1] + 6)
        far = game.raw('block', HOME[0] + 128, Y - 1, HOME[1] + 6)
        print('block here', here, 'far', far)
        for label, b in (('here', here), ('8 chunks away', far)):
            if 'air' in json.dumps(b) or 'unloaded' in json.dumps(b).lower() or 'error' in b:
                raise Failed(f'chunk {label} not loaded on the client: {b}')
        results['chunks'] = 'loaded'

        # entities: pigs beside the client, tracked while resident
        for i in range(3):
            s.console(f'summon minecraft:pig {HOME[0] + 4 + i} {Y} {HOME[1] + 10}')
        game.wait(100)
        pigs = game.raw('entities', 32, 'pig')
        print('pigs', json.dumps(pigs)[:600])
        first = listed(pigs)
        if len(first) < 3:
            raise Failed(f'the client sees {len(first)} of 3 pigs: {pigs}')
        game.wait(200)
        resident = fast_ticks(s, folder)
        later = listed(game.raw('entities', 48, 'pig'))
        moved = sum(1 for k, p in later.items() if k in first and p != first[k])
        print(f'pigs moved on the client: {moved} of {len(first)} (fast ticks meanwhile: {resident})')
        if moved == 0:
            raise Failed('no pig moved on the client in 10 s: tracking stopped')
        results['entities'] = f'{moved} of {len(first)} moved, {resident} fast ticks'

        # break and place, while resident
        target = (HOME[0] + 2, Y - 1, HOME[1] + 6)
        broke = game.raw('break', *target)
        print('break', json.dumps(broke)[:300])
        game.wait(10)
        if not passes(s, f'execute if block {target[0]} {target[1]} {target[2]} minecraft:air'):
            raise Failed(f'the mined block is not air on the server: {broke}')
        results['break'] = 'air on the server'
        game.wait(60)
        placed = game.raw('place', *target, '--item', 'stone')
        print('place', json.dumps(placed)[:300])
        game.wait(10)
        if not passes(s, f'execute if block {target[0]} {target[1]} {target[2]} minecraft:stone'):
            raise Failed(f'the placed block is not on the server: {placed}')
        results['place'] = 'stone on the server'

        # chat
        game.wait(60)
        game.raw('chat', 'resident hello')
        s.wait_for(r'resident hello', 30)
        results['chat'] = 'reached the server'

        # a chest: opened by the client while its region is resident again, its items shown
        chest = (HOME[0] - 2, Y, HOME[1] + 6)
        s.console(f'setblock {chest[0]} {chest[1]} {chest[2]} minecraft:chest'
                  '{Items:[{Slot:0b,id:"minecraft:diamond",count:3}]}')
        game.wait(60)
        # Client and server must agree on the blocks around the chest (a ghost block stops a walk).
        ghosts = []
        for dx in range(-2, 4):
            for dz in range(-2, 4):
                for y in (Y - 1, Y, Y + 1):
                    at = (chest[0] + dx, y, chest[2] + dz)
                    seen = json.dumps(game.raw('block', *at))
                    name = seen.split('"block": "')[1].split('"')[0] if '"block": "' in seen else seen
                    if not passes(s, f'execute if block {at[0]} {at[1]} {at[2]} {name}'):
                        ghosts.append((at, name))
        print('ghost blocks', ghosts)
        game.raw('goto', chest[0], chest[1], chest[2] + 2, '--near', 1)
        opened = game.raw('interact', *chest)
        game.wait(10)
        menu = game.raw('menu')
        print('menu', json.dumps(menu)[:400])
        if 'diamond' not in json.dumps(menu):
            raise Failed(f'the chest opened without its diamonds: {menu}')
        game.raw('close')
        results['chest'] = 'opened with its items'

        # death and respawn (a respawn reaches past the region)
        game.wait(40)
        s.console(f'kill {PLAYER}')
        game.wait(40)
        game.raw('respawn')
        game.wait(60)
        state = game.raw('state')
        player = state.get('player') or {}
        health, at = player.get('health'), player.get('position') or [0, 0, 0]
        print('after respawn', json.dumps(player)[:400])
        if not health or health <= 0:
            raise Failed(f'not alive after the respawn: {player}')
        if at[1] < Y - 0.01:
            raise Failed(f'respawned inside the ground (y {at[1]}, the surface is {Y}): {player}')
        game.wait(100)
        results['respawn'] = f'alive (health {health}), {fast_ticks(s, folder)} fast ticks after'
        print(json.dumps(results, indent=1))
    finally:
        try:
            (out / 'client.log').write_text(game.close() or '', encoding='utf-8')
        except Exception:
            pass
        for g in groups:
            g.kill()
        s.stop()
        shutil.rmtree(game.work, ignore_errors=True)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--server-bin', required=True, help='folder with horizon-client and horizon-bots (the server)')
    parser.add_argument('--client-bin', required=True, help='folder with horizon-client and horizon-ctl (the client)')
    parser.add_argument('--out', default=str(lab.LOCAL / 'resident-e2e'))
    parser.add_argument('--off', action='store_true', help='the same checks with resident regions off (a baseline)')
    args = parser.parse_args()
    out = Path(args.out)
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    os.environ.update(lab.game_env())
    with tempfile.TemporaryDirectory(prefix='resident-e2e-') as scratch:
        try:
            check(Path(args.server_bin).resolve(), Path(args.client_bin).resolve(), Path(scratch), out, not args.off)
        except (Failed, RuntimeError, AssertionError) as e:
            sys.exit(f'resident_e2e failed (logs in {out}): {e}')
    print('resident_e2e: every check passed')


if __name__ == '__main__':
    main()
