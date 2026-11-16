#!/usr/bin/env python3
"""HorizonsX's resident regions with the live server's plugins, end to end, on the VPS staging copy.

    python -I tools/v2/staging_e2e.py --client-bin D:/Horizon/v2-target/fast --bots-bin D:/Horizon/perf/before

The staging server (/opt/hx-stage, port 25599, offline, HORIZON_RESIDENT=1, the live plugins but Tebex and voice
chat) gets ten bots far away (two groups of five, so the regions split and stay resident), then one Horizon client
(offscreen) checks what the plugins do while its region stays out:

- sidebar:  HorizonBoard's sidebar is on the client's screen, and still there seconds later
- chat:     its chat line reaches the server's log and comes back to it
- balance:  /bal (Essentials) answers it
- spawn:    /spawn (Essentials) moves it
- menus:    /ah (zAuctionHouse) and /jobs (HorizonJobs) open a menu
- protect:  a block it breaks shows in CoreProtect's lookup
- resident: the server's report counts resident fast ticks through it all
"""
import argparse
import json
import os
import pathlib
import subprocess
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import lab  # noqa: E402
from lan_e2e import Game  # noqa: E402

HOST, PORT, PLAYER = 'your-server.example', 25599, 'StageTester'
SSH = ['ssh', '-i', os.path.expanduser('~/.ssh/id_ed25519_saphire_vps'), 'root@' + HOST]
DIR = '/opt/hx-stage/server'


class Failed(Exception):
    pass


def console(command):
    """A staging console command; the log lines it added."""
    before = int(subprocess.run(SSH + [f'wc -l < {DIR}/logs/latest.log'], capture_output=True, text=True).stdout.strip() or 0)
    subprocess.run(SSH + [f'/opt/hx-stage/runtime/horizon-server --dir {DIR} cmd {command}'], capture_output=True, text=True)
    time.sleep(1.5)
    out = subprocess.run(SSH + [f'tail -n +{before + 1} {DIR}/logs/latest.log'], capture_output=True, text=True,
                         encoding='utf-8', errors='replace').stdout
    return out


def log_has(text, timeout=20):
    deadline = time.time() + timeout
    while time.time() < deadline:
        r = subprocess.run(SSH + [f'grep -a -c -F {json.dumps(text)} {DIR}/logs/latest.log'], capture_output=True, text=True)
        if r.stdout.strip() not in ('', '0'):
            return True
        time.sleep(1)
    return False


def resident_ticks():
    out = subprocess.run(SSH + [f'grep -a "resident regions:" {DIR}/logs/latest.log | tail -1'], capture_output=True,
                         text=True).stdout
    try:
        return int(out.split('resident regions:')[1].split('fast ticks')[0])
    except (IndexError, ValueError):
        return 0


def messages(game):
    m = game.raw('messages')
    return json.dumps(m)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--client-bin', required=True)
    ap.add_argument('--bots-bin', required=True)
    args = ap.parse_args()
    os.environ.update(lab.game_env())
    exe = '.exe' if os.name == 'nt' else ''
    bots = [subprocess.Popen([str(pathlib.Path(args.bots_bin) / f'horizon-bots{exe}'), '--server', f'{HOST}:{PORT}',
                              '--bots', '5', '--behaviour', 'stand', '--seconds', '900', '--name', name],
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) for name in ('SA', 'SB')]
    game = Game('client', pathlib.Path(args.client_bin) / f'horizon-ctl{exe}', lab.RESOURCES.resolve())
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'HORIZON_LAUNCHER_HOME'):
        game.d.env[key] = str(game.work / key.lower())
    results, failures = {}, []

    def check(name, ok, detail=''):
        results[name] = ('ok ' if ok else 'FAILED ') + str(detail)[:200]
        print(name, results[name], flush=True)
        if not ok:
            failures.append(name)

    try:
        if not log_has('SB4 joined the game', 120):
            raise Failed('the bots did not join')
        for i in range(5):
            console(f'gamemode creative SA{i}')
            console(f'gamemode creative SB{i}')
            console(f'tp SA{i} 3000 150 3000')
            console(f'tp SB{i} -3000 150 -3000')
        game('launch', wait=False, offscreen=True)
        game.raw('connect', f'{HOST}:{PORT}', PLAYER)
        game.raw('wait-for', 'loaded', '--timeout', 300)
        if not log_has(f'{PLAYER} joined the game', 60):
            raise Failed('the client did not join')
        console(f'op {PLAYER}')
        console(f'gamemode creative {PLAYER}')
        console(f'tp {PLAYER} 600 120 600')
        console('setblock 601 119 600 minecraft:stone')
        game.wait(200)
        start = resident_ticks()
        # sidebar
        side = game.raw('status').get('sidebar')
        check('sidebar', bool(side) and json.dumps(side) not in ('null', '{}', '[]'), json.dumps(side)[:150])
        # chat
        game.raw('chat', 'staging hello')
        check('chat', log_has('staging hello', 20) and 'staging hello' in messages(game), 'in the log and on the client')
        # balance
        before = messages(game)
        game.raw('chat', '/bal')
        game.wait(40)
        after = messages(game)
        check('balance', len(after) > len(before), after[len(before):][:150])
        # spawn
        at = (game.raw('state').get('player') or {}).get('position')
        game.raw('chat', '/spawn')
        game.wait(100)
        moved = (game.raw('state').get('player') or {}).get('position')
        check('spawn', at != moved, f'{at} -> {moved}')
        # menus
        for command in ('/ah', '/jobs'):
            game.raw('chat', command)
            game.wait(80)
            menu = game.raw('menu')
            screen = game.raw('state').get('menu')
            # A container menu's slots come after the player's own (numbered from 100 on); the screen reads
            # "inventory" either way.
            theirs = [x for x in menu.get('slots') or [] if x.get('slot', 0) >= 100]
            check(command, bool(theirs) or bool(screen),
                  json.dumps({'container_slots': len(theirs), 'first': theirs[:2], 'state_menu': screen})[:200])
            try:
                game.raw('close')
            except Exception:  # noqa: BLE001
                pass
            game.wait(20)
        # protect: CoreProtect's lookup of the client's chat (it logs chat and commands; breaking needs build rights here)
        out = console(f'co lookup u:{PLAYER} t:10m a:chat')
        check('protect', 'staging hello' in out, out.replace(chr(10), ' ')[:200])
        game.wait(60)
        end = resident_ticks()
        check('resident', end > start, f'{start} -> {end} fast ticks')
    except Failed as e:
        failures.append(str(e))
        print('FAILED', e)
    finally:
        try:
            game.close()
        except Exception:  # noqa: BLE001
            pass
        for b in bots:
            b.kill()
    print(json.dumps(results, indent=1))
    if failures:
        sys.exit(f'staging_e2e failed: {failures}')
    print('staging_e2e: every check passed')


if __name__ == '__main__':
    main()
