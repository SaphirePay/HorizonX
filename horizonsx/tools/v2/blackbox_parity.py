#!/usr/bin/env python3
"""Black-box parity: vanilla Java vs Pumpkin vs HorizonsX, as a player sees them.

The white-box harnesses (tools/test_*_parity.py) call Horizon's own game code. A server whose code we don't link
(Pumpkin) can only be compared from the outside, so this runs each server for real on the PC (offline, one at a
time), builds the same things through its console, and reads the world back through one Horizon client:

  redstone  the random redstone circuits of tools/test_redstone_parity.py plus hand-built classics, built with
            /setblock on a floating platform while the game is frozen (/tick freeze), then powered, unpowered and
            stepped one tick at a time (/tick step 1); after every tick the client reads every block of the circuit.
            Exact comparison with vanilla, tick by tick.
  mobs      behaviour scenarios (/summon, the player in survival): chasing, shooting, exploding, burning in
            daylight, following food. Random numbers can't be lined up between separate servers, so each scenario
            checks what vanilla always does (e.g. "the zombie got within 2 blocks"), on every server.

    python -I tools/v2/blackbox_parity.py [--servers vanilla,pumpkin,horizonsx] [--count 40] [--seed 7]
        [--only redstone|mobs] [--keep]

Writes D:/Horizon/parity/blackbox/<server>-redstone.json, -mobs.json and report.txt.
"""
import json
import os
import pathlib
import random
import shutil
import subprocess
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, r'D:\Horizon\fabric-port\tools')
import block_update_harness as h  # noqa: E402
import proof  # noqa: E402

OUT = pathlib.Path(r'D:\Horizon\parity\blackbox')
V2 = HERE.parent.parent
VANILLA_JAR = V2 / '.local' / 'minecraft' / '26.3' / 'jars' / 'server.jar'
JAVA = pathlib.Path(r'C:\Program Files\Java\jdk-25\bin\java.exe')
PUMPKIN_EXE = pathlib.Path(r'D:\Horizon\bench\pumpkin-0.2.2\pumpkin-X64-v3-Windows.exe')
HX_ENGINE = pathlib.Path(os.environ.get('BB_HX_ENGINE', r'D:\Horizon\perf\hxnov8b\horizon-client.exe'))
VERSION = pathlib.Path(os.environ['APPDATA']) / 'horizon-launcher' / 'versions' / 'imported-2026-11-07-21733ec'
CLIENT = VERSION / 'horizon-client.exe'
RESOURCES = VERSION / '.local' / 'horizon-client' / '26.3'
NAME = 'Prover'
ORIGIN = (8, 120, 8)
BOX = (-5, -1, -5, 5, 2, 5)
PORTS = {'vanilla': 25811, 'pumpkin': 25812, 'horizonsx': 25813}
PREBUILT = None


def option(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


def log_text(path):
    return path.read_text(encoding='utf-8', errors='replace') if path.exists() else ''


# ---- Servers: start, console command, stop ----

class Piped:
    """A server run as a child with its console on stdin (vanilla, Pumpkin)."""
    ready = 'Done ('

    def __init__(self, kind, directory, port):
        self.kind, self.dir, self.port, self.proc = kind, directory, port, None

    def log(self):
        return log_text(self.dir / 'out.log')

    def launch(self):
        raise NotImplementedError

    def start(self):
        self.out = open(self.dir / 'out.log', 'w', encoding='utf-8')
        self.proc = self.launch()
        t0 = time.time()
        while time.time() - t0 < 300:
            if self.ready in self.log():
                return
            if self.proc.poll() is not None:
                sys.exit(f'{self.kind} exited:\n' + self.log()[-2000:])
            time.sleep(0.2)
        sys.exit(f'{self.kind} did not start:\n' + self.log()[-2000:])

    def cmd(self, line):
        self.proc.stdin.write((line.lstrip('/') + '\n').encode())
        self.proc.stdin.flush()

    def stop(self):
        if self.proc and self.proc.poll() is None:
            try:
                self.cmd('stop')
                self.proc.wait(60)
            except Exception:  # noqa: BLE001
                self.proc.kill()
        self.out.close()


class Vanilla(Piped):
    def setup(self):
        self.dir.mkdir(parents=True, exist_ok=True)
        (self.dir / 'eula.txt').write_text('eula=true\n')
        (self.dir / 'server.properties').write_text('\n'.join([
            'online-mode=false', f'server-port={self.port}', 'level-type=minecraft\\:flat', 'spawn-protection=0',
            'view-distance=6', 'simulation-distance=6', 'gamemode=creative', 'difficulty=normal',
            'enforce-secure-profile=false', 'level-seed=12345', 'spawn-monsters=true', 'pause-when-empty-seconds=-1',
            'sync-chunk-writes=false', 'white-list=false', 'enforce-whitelist=false', '']))

    def launch(self):
        return subprocess.Popen([str(JAVA), '-Xmx2G', '-jar', str(VANILLA_JAR), 'nogui'], cwd=self.dir,
                                stdin=subprocess.PIPE, stdout=self.out, stderr=subprocess.STDOUT)


class Pumpkin(Piped):
    ready = 'Server is now running'

    def launch(self):
        return subprocess.Popen([str(PUMPKIN_EXE)], cwd=self.dir, stdin=subprocess.PIPE, stdout=self.out,
                                stderr=subprocess.STDOUT, env={**os.environ, 'NO_COLOR': '1'})

    def setup(self):
        self.dir.mkdir(parents=True, exist_ok=True)
        if not (self.dir / 'pumpkin.toml').exists():
            # The first start writes its config.
            self.start()
            time.sleep(2)
            self.stop()
        sys.path.insert(0, str(HERE))
        from pumpkin_ramp import set_toml
        text = (self.dir / 'pumpkin.toml').read_text(encoding='utf-8')
        for section, key, value in (
                ('', 'seed', '"12345"'), ('', 'default_gamemode', '"Creative"'), ('', 'spawn_protection', '0'),
                ('networking.java', 'address', f'"0.0.0.0:{self.port}"'), ('networking.java', 'online_mode', 'false'),
                ('networking.java', 'encryption', 'false'), ('networking.java', 'view_distance', '6'),
                ('networking.java', 'simulation_distance', '6'),
                ('networking.java.packet_limiter', 'enabled', 'false'),
                ('networking.java.authentication', 'enabled', 'false'),
                ('networking.bedrock', 'enabled', 'false'), ('networking.lan_broadcast', 'enabled', 'false'),
                ('networking.query', 'enabled', 'false'), ('telemetry', 'enabled', 'false')):
            text = set_toml(text, section, key, value)
        (self.dir / 'pumpkin.toml').write_text(text, encoding='utf-8')


class Horizon:
    """HorizonsX through horizon-server (its console is `horizon-server cmd`)."""

    def __init__(self, kind, directory, port):
        self.kind, self.dir, self.port = kind, directory, port

    def server(self, *args, timeout=60):
        return proof.captured([proof.SERVER, '--dir', self.dir / 'server', *args], timeout)

    def setup(self):
        self.dir.mkdir(parents=True, exist_ok=True)
        if not (self.dir / 'server' / 'server.toml').exists():
            print(self.server('init', '--eula', '--world', 'world', '--seed', '12345', '--port', str(self.port),
                              '--no-whitelist', '--offline', '--op', NAME, '--yes')[:200], flush=True)
        print(self.server('config', 'set', f'engine.path={HX_ENGINE}', f'engine.resources={RESOURCES}', 'panel.bind=',
                          'world.game_mode=creative', 'properties.spawn-protection=0', 'properties.view-distance=6',
                          'properties.simulation-distance=6', 'properties.level-type=minecraft:flat',
                          'properties.difficulty=normal')[:200], flush=True)

    def log(self):
        return log_text(self.dir / 'server' / 'logs' / 'latest.log')

    def start(self):
        print(self.server('start')[:200], flush=True)
        t0 = time.time()
        while time.time() - t0 < 300:
            low = self.log().lower()
            if 'done (' in low or 'server started' in low:
                return
            time.sleep(0.3)
        sys.exit('horizonsx did not start:\n' + self.log()[-2000:])

    def cmd(self, line):
        self.server('cmd', *line.lstrip('/').split(), timeout=30)

    def stop(self):
        print(self.server('stop', timeout=90)[:200], flush=True)


def make(kind):
    d = OUT / kind
    return {'vanilla': Vanilla, 'pumpkin': Pumpkin, 'horizonsx': Horizon}[kind](kind, d, PORTS[kind])


# ---- The client ----

def client_env(kind):
    run = OUT / f'client-{kind}'
    run.mkdir(parents=True, exist_ok=True)
    return dict(os.environ, HORIZON_HOME=str(run / 'home'), HORIZON_CONTROL=str(run / 'control.json'),
                HORIZON_NO_HEARTBEAT='1', HORIZON_NO_DISCORD='1', HORIZON_NO_AUDIO='1')


class Client:
    def __init__(self, kind):
        self.env = client_env(kind)

    def ctl(self, *args, timeout=40):
        return proof.captured([proof.CTL, '--timeout', str(timeout), *map(str, args)], timeout + 15, self.env).strip()

    def json(self, *args, timeout=40):
        text = self.ctl(*args, timeout=timeout)
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            return {'error': text[:300]}

    def start(self, port):
        print(self.ctl('launch', '--game', CLIENT, '--offscreen', '--width', 640, '--height', 360, '--resources',
                       RESOURCES, '--name', NAME, timeout=120)[:160], flush=True)
        print(self.ctl('connect', f'localhost:{port}', NAME, timeout=120)[:160], flush=True)
        self.ctl('wait-ready', timeout=180)

    def blocks(self, origin=ORIGIN):
        lo = [origin[i] + BOX[i] for i in range(3)]
        hi = [origin[i] + BOX[i + 3] for i in range(3)]
        got = self.json('blocks', *lo, *hi)
        if 'blocks' not in got:
            return {'error': str(got)[:200]}
        return {f'{x - origin[0]},{y - origin[1]},{z - origin[2]}': s for x, y, z, s in got['blocks']}

    def quit(self):
        self.ctl('quit', timeout=20)


# ---- Redstone cases ----

PLATFORM = 'minecraft:smooth_stone'


def random_cases(facts, count, seed):
    """test_redstone_parity's circuits, as block state strings (TNT left out: it explodes after the window)."""
    sys.path.insert(0, str(HERE.parent))
    import test_redstone_parity as rp
    rp.COMPONENTS = [c for c in rp.COMPONENTS if c != 'tnt']
    rng = random.Random(seed)
    out = []
    names = [n for n in ('minecraft:' + c for c in rp.COMPONENTS) if n in facts.by_block]
    for n in range(count):
        cells = []
        for x in range(-4, 5):
            for z in range(-4, 5):
                if rng.random() < 0.5:
                    cells.append([x, 0, z, facts.describe(rng.choice(facts.by_block[rng.choice(names)]))])
                    if rng.random() < 0.15:
                        cells.append([x, 1, z, facts.describe(rng.choice(facts.by_block[rng.choice(names)]))])
        at = [rng.randrange(-4, 5), 0, rng.randrange(-4, 5)]
        cells = [c for c in cells if c[:3] != at]
        out.append({'name': f'random-{n}', 'cells': cells, 'power': at, 'source': 'minecraft:redstone_block'})
    return out


def line(x0, z, blocks):
    return [[x0 + i, 0, z, b] for i, b in enumerate(blocks)]


def classic_cases():
    """Builds every player knows."""
    w = 'minecraft:redstone_wire[east=side,west=side,north=none,south=none,power=0]'
    r = lambda d: f'minecraft:repeater[delay={d},facing=west,locked=false,powered=false]'  # noqa: E731
    lamp = 'minecraft:redstone_lamp[lit=false]'
    cases = [
        {'name': 'repeater-chain', 'cells': line(-3, 0, [w, r(1), r(2), r(3), r(4), w, lamp]), 'power': [-4, 0, 0]},
        {'name': 'torch-inverter', 'cells': [[-2, 0, 0, 'minecraft:stone'], [-1, 0, 0, 'minecraft:redstone_wall_torch[facing=east,lit=true]'],
                                             [0, 0, 0, w], [1, 0, 0, lamp]], 'power': [-3, 0, 0]},
        {'name': 'observer-clock', 'cells': [[0, 0, 0, 'minecraft:observer[facing=east,powered=false]'],
                                             [1, 0, 0, 'minecraft:observer[facing=west,powered=false]'],
                                             [0, 0, 1, lamp], [1, 0, 1, lamp]], 'power': [0, 1, 0], 'source': 'minecraft:stone'},
        {'name': 'piston-push-line', 'cells': [[0, 0, 0, 'minecraft:piston[extended=false,facing=east]']]
            + [[1 + i, 0, 0, b] for i, b in enumerate(['minecraft:stone', 'minecraft:oak_planks', 'minecraft:slime_block', 'minecraft:stone'])],
         'power': [-1, 0, 0]},
        {'name': 'sticky-piston-pull', 'cells': [[0, 0, 0, 'minecraft:sticky_piston[extended=false,facing=east]'], [1, 0, 0, 'minecraft:stone']],
         'power': [-1, 0, 0]},
        {'name': 'comparator-subtract', 'cells': [[-2, 0, 0, w], [-1, 0, 0, w],
                                                  [0, 0, 0, 'minecraft:comparator[facing=west,mode=subtract,powered=false]'],
                                                  [0, 0, 1, 'minecraft:redstone_wire[north=side,south=side,east=none,west=none,power=0]'],
                                                  [0, 0, 2, 'minecraft:redstone_block'], [1, 0, 0, w], [2, 0, 0, lamp]], 'power': [-3, 0, 0]},
        {'name': 'torch-clock-burnout', 'cells': [[0, 0, 0, 'minecraft:stone'], [1, 0, 0, 'minecraft:redstone_wall_torch[facing=east,lit=true]'],
                                                  [1, 0, 1, 'minecraft:redstone_wire[north=side,south=side,east=none,west=none,power=0]'],
                                                  [0, 0, 1, 'minecraft:redstone_wire[east=side,west=side,north=none,south=none,power=0]'],
                                                  [-1, 0, 1, 'minecraft:redstone_wire[north=side,east=side,south=none,west=none,power=0]'],
                                                  [-1, 0, 0, 'minecraft:redstone_wire[north=side,south=side,east=none,west=none,power=0]']],
         'power': [0, 1, 0], 'source': 'minecraft:stone', 'ticks': 40},
        {'name': 'copper-bulb-toggle', 'cells': [[0, 0, 0, 'minecraft:copper_bulb[lit=false,powered=false]'], [1, 0, 0, lamp]], 'power': [-1, 0, 0]},
        {'name': 'powered-rail-line', 'cells': [[i, 0, 0, 'minecraft:powered_rail[powered=false,shape=east_west,waterlogged=false]'] for i in range(-3, 6)],
         'power': [-4, 0, 0]},
        {'name': 'iron-door-and-trapdoor', 'cells': [[0, 0, 0, 'minecraft:iron_door[facing=east,half=lower,hinge=left,open=false,powered=false]'],
                                                     [0, 1, 0, 'minecraft:iron_door[facing=east,half=upper,hinge=left,open=false,powered=false]'],
                                                     [1, 0, -1, 'minecraft:iron_trapdoor[facing=north,half=bottom,open=false,powered=false,waterlogged=false]']],
         'power': [1, 0, 0]},
        {'name': 'note-and-dispenser', 'cells': [[0, 0, 0, 'minecraft:note_block[instrument=harp,note=0,powered=false]'],
                                                 [1, 0, 1, 'minecraft:dispenser[facing=up,triggered=false]'],
                                                 [0, 0, 1, 'minecraft:redstone_wire[east=side,west=side,north=side,south=none,power=0]']], 'power': [-1, 0, 1]},
        {'name': 'daylight-sensor', 'cells': [[0, 0, 0, 'minecraft:daylight_detector[inverted=false,power=0]'], [1, 0, 0, w], [2, 0, 0, lamp]],
         'power': [0, 1, 0], 'source': 'minecraft:air', 'ticks': 40},
    ]
    for c in cases:
        c.setdefault('source', 'minecraft:redstone_block')
    return cases


def run_redstone(server, client, cases, freeze_tick=True):
    ox, oy, oz = ORIGIN
    results = []
    lo = [ORIGIN[i] + BOX[i] for i in range(3)]
    hi = [ORIGIN[i] + BOX[i + 3] for i in range(3)]

    def step(n, wait):
        server.cmd(f'tick step {n}')
        time.sleep(wait)

    for k, case in enumerate(cases):
        # Clear what the last case left, let its pending ticks run out, then a clean floor.
        server.cmd('tick freeze')
        server.cmd(f'fill {lo[0]} {lo[1] + 1} {lo[2]} {hi[0]} {hi[1]} {hi[2]} minecraft:air')
        step(40, 2.6)
        server.cmd(f'fill {lo[0]} {lo[1]} {lo[2]} {hi[0]} {lo[1]} {hi[2]} {PLATFORM}')
        server.cmd(f'fill {lo[0]} {lo[1] + 1} {lo[2]} {hi[0]} {hi[1]} {hi[2]} minecraft:air')
        step(20, 1.4)
        for x, y, z, state in case['cells']:
            server.cmd(f'setblock {ox + x} {oy + y} {oz + z} {state}')
        time.sleep(0.8)
        frames = [client.blocks()]
        px, py, pz = case['power']
        server.cmd(f'setblock {ox + px} {oy + py} {oz + pz} {case["source"]}')
        time.sleep(0.4)
        frames.append(client.blocks())
        ticks = case.get('ticks', 12)
        for _ in range(ticks):
            step(1, 0.25)
            frames.append(client.blocks())
        if case['source'] != 'minecraft:air':
            server.cmd(f'setblock {ox + px} {oy + py} {oz + pz} minecraft:air')
            time.sleep(0.4)
            frames.append(client.blocks())
            for _ in range(12):
                step(1, 0.25)
                frames.append(client.blocks())
        results.append({'name': case['name'], 'frames': frames})
        if (k + 1) % 10 == 0:
            print(f'  {server.kind}: {k + 1}/{len(cases)} circuits', flush=True)
    return results


# ---- Redstone on a world built once in vanilla (Pumpkin's /setblock takes no block states) ----

def site(k):
    return (8 + 24 * k, 120, 8)


def build_world(cases):
    """Builds every circuit in a fresh vanilla world (one site each), lets it settle, saves it; returns its folder."""
    server = Vanilla('vanilla', OUT / 'build', PORTS['vanilla'])
    shutil.rmtree(server.dir, ignore_errors=True)
    server.setup()
    server.start()
    server.cmd(f'op {NAME}')
    client = Client('build')
    try:
        client.start(server.port)
        server.cmd(f'gamemode creative {NAME}')
        for rule in ('spawn_mobs false', 'advance_time false'):
            server.cmd('gamerule ' + rule)
        server.cmd('time set 6000')
        server.cmd('tick freeze')
        for k, case in enumerate(cases):
            ox, oy, oz = site(k)
            lo = [ox + BOX[0], oy + BOX[1], oz + BOX[2]]
            hi = [ox + BOX[3], oy + BOX[4], oz + BOX[5]]
            server.cmd(f'tp {NAME} {ox} {oy + 8} {oz - 10}')
            time.sleep(2.0)
            server.cmd(f'fill {lo[0]} {lo[1]} {lo[2]} {hi[0]} {lo[1]} {hi[2]} {PLATFORM}')
            for x, y, z, state in case['cells']:
                server.cmd(f'setblock {ox + x} {oy + y} {oz + z} {state}')
            server.cmd('tick step 60')
            time.sleep(3.5)
            if (k + 1) % 10 == 0:
                print(f'  built {k + 1}/{len(cases)}', flush=True)
        server.cmd('save-all flush')
        time.sleep(4)
    finally:
        try:
            client.quit()
        finally:
            server.stop()
    return server.dir / 'world'


def run_prebuilt(server, client, cases):
    results = []
    server.cmd('tick freeze')
    for k, case in enumerate(cases):
        origin = site(k)
        ox, oy, oz = origin
        server.cmd(f'tp {NAME} {ox} {oy + 8} {oz - 10}')
        time.sleep(2.5)
        client.ctl('wait-ready', timeout=60)
        frames = [client.blocks(origin)]
        px, py, pz = case['power']
        source = case['source']
        server.cmd(f'setblock {ox + px} {oy + py} {oz + pz} {source}')
        time.sleep(0.4)
        frames.append(client.blocks(origin))
        for _ in range(case.get('ticks', 12)):
            server.cmd('tick step 1')
            time.sleep(0.25)
            frames.append(client.blocks(origin))
        if source != 'minecraft:air':
            server.cmd(f'setblock {ox + px} {oy + py} {oz + pz} minecraft:air')
            time.sleep(0.4)
            frames.append(client.blocks(origin))
            for _ in range(12):
                server.cmd('tick step 1')
                time.sleep(0.25)
                frames.append(client.blocks(origin))
        results.append({'name': case['name'], 'frames': frames})
        if (k + 1) % 10 == 0:
            print(f'  {server.kind}: {k + 1}/{len(cases)} circuits', flush=True)
    return results


# ---- Mob behaviour ----

ARENA = (8, 120, 40)


def mob_scenarios():
    """(name, setup commands, seconds, check over the samples) - what vanilla always does."""
    ax, ay, az = ARENA
    s = []

    def add(name, summon, seconds, check, extra=(), time_of_day=18000, gm='survival', hand=None, armour=True):
        s.append({'name': name, 'summon': summon, 'seconds': seconds, 'check': check, 'extra': list(extra),
                  'time': time_of_day, 'gamemode': gm, 'hand': hand, 'armour': armour})

    add('zombie_chases_player', f'minecraft:zombie {ax + 10} {ay} {az}', 12, 'closest<=2')
    add('skeleton_shoots_player', f'minecraft:skeleton {ax + 10} {ay} {az}', 12, 'player_hurt', armour=False)
    add('creeper_explodes_near_player', f'minecraft:creeper {ax + 6} {ay} {az}', 15, 'gone_or_hole')
    add('spider_chases_player', f'minecraft:spider {ax + 10} {ay} {az}', 12, 'closest<=2.5')
    add('zombie_burns_in_daylight', f'minecraft:zombie {ax + 6} {ay} {az + 6}', 15, 'burns', time_of_day=6000, gm='creative')
    add('skeleton_burns_in_daylight', f'minecraft:skeleton {ax + 6} {ay} {az + 6}', 15, 'burns', time_of_day=6000, gm='creative')
    add('cow_follows_wheat', f'minecraft:cow {ax + 7} {ay} {az}', 15, 'closest<=3.5', time_of_day=6000, hand='minecraft:wheat')
    add('sheep_follows_wheat', f'minecraft:sheep {ax + 7} {ay} {az}', 15, 'closest<=3.5', time_of_day=6000, hand='minecraft:wheat')
    add('chicken_follows_seeds', f'minecraft:chicken {ax + 7} {ay} {az}', 15, 'closest<=3.5', time_of_day=6000, hand='minecraft:wheat_seeds')
    add('cow_wanders', f'minecraft:cow {ax + 6} {ay} {az + 6}', 20, 'moved>=1', time_of_day=6000, gm='creative')
    add('creative_player_ignored', f'minecraft:zombie {ax + 6} {ay} {az}', 10, 'closest>=4', gm='creative')
    add('iron_golem_kills_zombie', f'minecraft:zombie {ax + 6} {ay} {az + 6}', 20, 'zombie_dies',
        extra=[f'summon minecraft:iron_golem {ax + 6} {ay} {az + 8}'], gm='creative')
    return s


def run_mobs(server, client, scenarios):
    ax, ay, az = ARENA
    out = []
    server.cmd('tick unfreeze')
    for rule in ('doMobSpawning false', 'doDaylightCycle false', 'spawn_mobs false', 'advance_time false'):
        server.cmd('gamerule ' + rule)
    server.cmd('difficulty normal')
    for sc in scenarios:
        server.cmd(f'kill @e[type=!player]')
        server.cmd(f'fill {ax - 12} {ay - 1} {az - 12} {ax + 18} {ay - 1} {az + 12} minecraft:grass_block')
        server.cmd(f'fill {ax - 12} {ay} {az - 12} {ax + 18} {ay + 4} {az + 12} minecraft:air')
        server.cmd(f'time set {sc["time"]}')
        server.cmd(f'gamemode {sc["gamemode"]} {NAME}')
        server.cmd(f'tp {NAME} {ax + 0.5} {ay} {az + 0.5} -90 0')
        server.cmd(f'effect clear {NAME}')
        server.cmd(f'effect give {NAME} minecraft:instant_health 1 10 true')
        if sc.get('armour', True):
            server.cmd(f'effect give {NAME} minecraft:resistance 60 4 true')
            server.cmd(f'effect give {NAME} minecraft:regeneration 60 4 true')
        server.cmd(f'clear {NAME}')
        if sc['hand']:
            server.cmd(f'item replace entity {NAME} weapon.mainhand with {sc["hand"]}')
        time.sleep(1.5)
        server.cmd('summon ' + sc['summon'])
        for line_ in sc['extra']:
            server.cmd(line_)
        samples = []
        t0 = time.time()
        while time.time() - t0 < sc['seconds']:
            time.sleep(0.5)
            ents = client.json('entities', 40)
            st = client.json('state')
            samples.append({'t': round(time.time() - t0, 1), 'entities': ents.get('entities', ents),
                            'projectiles': len(st.get('projectiles') or []) if isinstance(st, dict) else 0,
                            'player_health': (st.get('player') or {}).get('health') if isinstance(st, dict) else None})
        floor = client.json('blocks', ax - 3, ay - 1, az - 3, ax + 9, ay - 1, az + 3)
        out.append({'name': sc['name'], 'check': sc['check'], 'samples': samples,
                    'floor_blocks': len(floor.get('blocks', [])) if isinstance(floor, dict) else None})
        print(f'  {server.kind}: {sc["name"]} done', flush=True)
    return out


# ---- Running a server through both suites ----

def run_server(kind, red_cases, mob_list, only, keep):
    server = make(kind)
    if kind == 'pumpkin' and not PUMPKIN_EXE.exists():
        sys.exit(f'no Pumpkin at {PUMPKIN_EXE}')
    server.setup()
    if PREBUILT and only in (None, 'redstone'):
        shutil.rmtree(server.dir / 'world', ignore_errors=True)
        shutil.copytree(PREBUILT, server.dir / 'world')
        for lock in (server.dir / 'world').rglob('session.lock'):
            lock.unlink()
        if kind == 'pumpkin':
            sys.path.insert(0, str(HERE))
            from pumpkin_ramp import set_toml
            text = (server.dir / 'pumpkin.toml').read_text(encoding='utf-8')
            (server.dir / 'pumpkin.toml').write_text(set_toml(text, 'world.chunk', 'type', '"anvil"'), encoding='utf-8')
    server.start()
    if kind in ('vanilla', 'pumpkin'):
        server.cmd(f'op {NAME}')
    client = Client(kind)
    try:
        client.start(server.port)
        server.cmd(f'gamemode creative {NAME}')
        server.cmd(f'tp {NAME} {ORIGIN[0]} {ORIGIN[1] + 6} {ORIGIN[2] - 8}')
        for rule in ('doMobSpawning false', 'doDaylightCycle false', 'spawn_mobs false', 'advance_time false'):
            server.cmd('gamerule ' + rule)
        server.cmd('time set 6000')
        time.sleep(4)
        client.ctl('wait-ready', timeout=120)
        if only in (None, 'redstone'):
            t0 = time.time()
            red = run_prebuilt(server, client, red_cases) if PREBUILT else run_redstone(server, client, red_cases)
            (OUT / f'{kind}-redstone.json').write_text(json.dumps(red))
            print(f'{kind}: redstone done in {time.time() - t0:.0f} s', flush=True)
        if only in (None, 'mobs'):
            mobs = run_mobs(server, client, mob_list)
            (OUT / f'{kind}-mobs.json').write_text(json.dumps(mobs))
    finally:
        try:
            client.quit()
        finally:
            server.stop()
    if not keep and kind != 'horizonsx':
        pass


# ---- Comparing ----

def compare_redstone(kinds):
    base = json.loads((OUT / 'vanilla-redstone.json').read_text())
    report = []
    for kind in kinds:
        if kind == 'vanilla' or not (OUT / f'{kind}-redstone.json').exists():
            continue
        other = json.loads((OUT / f'{kind}-redstone.json').read_text())
        same = 0
        diffs = []
        for a, b in zip(base, other):
            first = next((i for i, (fa, fb) in enumerate(zip(a['frames'], b['frames'])) if fa != fb), None)
            if first is None and len(a['frames']) == len(b['frames']):
                same += 1
                continue
            fa, fb = a['frames'][first], b['frames'][first]
            keys = sorted(set(fa) | set(fb)) if isinstance(fa, dict) and isinstance(fb, dict) else []
            delta = [(k, fa.get(k, 'air'), fb.get(k, 'air')) for k in keys if fa.get(k) != fb.get(k)][:4]
            diffs.append((a['name'], first, delta))
        report.append(f'{kind}: redstone {same}/{len(base)} circuits identical to vanilla on every tick')
        for name, first, delta in diffs:
            when = 'as built' if first == 0 else ('when powered' if first == 1 else f'tick {first - 1} after powering')
            report.append(f'   {name}: first difference {when}')
            for k, va, vb in delta:
                report.append(f'      at {k}: vanilla {va}  |  {kind} {vb}')
    return report


def mob_check(result):
    check = result['check']
    closest, moved, arrows = [], 0.0, 0
    first_pos, health = {}, {}
    kinds_at_end = []
    for s in result['samples']:
        ents = [e for e in (s['entities'] if isinstance(s['entities'], list) else []) if e.get('kind') == 'mob']
        arrows = max(arrows, s.get('projectiles') or 0)
        if ents:
            closest.append(min(e.get('distance', 99) for e in ents))
        for e in ents:
            i, pos = e.get('id'), e.get('pos')
            first_pos.setdefault(i, pos)
            moved = max(moved, sum((p - q) ** 2 for p, q in zip(pos, first_pos[i])) ** 0.5)
            health.setdefault(i, []).append(e.get('health'))
        kinds_at_end = [e.get('type', '') for e in ents]
    hurt = any(h_ and h_[0] is not None and min(x for x in h_ if x is not None) < h_[0] for h_ in health.values())
    if check.startswith('closest<='):
        return bool(closest) and min(closest) <= float(check.split('=')[1]), f'closest {min(closest) if closest else None:.1f}' if closest else 'no mob seen'
    if check.startswith('closest>='):
        return bool(closest) and min(closest) >= float(check.split('=')[1]), f'closest {min(closest):.1f}' if closest else 'no mob seen'
    if check == 'player_hurt':
        hp = [x.get('player_health') for x in result['samples'] if x.get('player_health') is not None]
        return bool(hp) and min(hp) < max(hp), f'player health {max(hp) if hp else None} -> lowest {min(hp) if hp else None}'
    if check.startswith('arrows>='):
        return arrows >= int(check.split('=')[1]), f'arrows in the air (most at once) {arrows}'
    if check.startswith('moved>='):
        return moved >= float(check.split('=')[1]), f'moved {moved:.1f} blocks'
    if check == 'gone_or_hole':
        gone = 'minecraft:creeper' not in kinds_at_end
        holed = result.get('floor_blocks') is not None and result['floor_blocks'] < 13 * 7
        return gone and holed, f'creeper gone {gone}, floor blocks left {result.get("floor_blocks")}/91'
    if check == 'burns':
        alive = any(k in ('minecraft:zombie', 'minecraft:skeleton') for k in kinds_at_end)
        return hurt or not alive, f'took damage {hurt}, alive at end {alive}'
    if check == 'zombie_dies':
        return 'minecraft:zombie' not in kinds_at_end, f'zombie alive at end {"minecraft:zombie" in kinds_at_end}'
    return False, 'unknown check'


def compare_mobs(kinds):
    report = []
    for kind in kinds:
        p = OUT / f'{kind}-mobs.json'
        if not p.exists():
            continue
        res = json.loads(p.read_text())
        rows = [(r['name'], *mob_check(r)) for r in res]
        report.append(f'{kind}: mob behaviour {sum(1 for _, ok, _ in rows if ok)}/{len(rows)} as vanilla does')
        for name, ok, why in rows:
            report.append(f'   {"ok  " if ok else "FAIL"} {name}: {why}')
    return report


def main():
    kinds = option('--servers', 'vanilla,pumpkin,horizonsx').split(',')
    only = option('--only', None)
    keep = '--keep' in sys.argv
    OUT.mkdir(parents=True, exist_ok=True)
    facts = h.load_facts()
    red_cases = classic_cases() + random_cases(facts, int(option('--count', '40')), int(option('--seed', '7')))
    mob_list = mob_scenarios()
    global PREBUILT
    if '--reuse-build' in sys.argv and (OUT / 'build' / 'world').is_dir():
        PREBUILT = OUT / 'build' / 'world'
    elif '--report-only' not in sys.argv and only in (None, 'redstone') and '--live-build' not in sys.argv:
        print('==== building the circuits once in vanilla', flush=True)
        PREBUILT = build_world(red_cases)
    if '--report-only' not in sys.argv:
        for kind in kinds:
            print(f'==== {kind}', flush=True)
            t0 = time.time()
            run_server(kind, red_cases, mob_list, only, keep)
            print(f'==== {kind} finished in {time.time() - t0:.0f} s', flush=True)
    report = []
    if only in (None, 'redstone') and (OUT / 'vanilla-redstone.json').exists():
        report += compare_redstone(kinds)
    if only in (None, 'mobs'):
        report += compare_mobs(['vanilla'] + [k for k in kinds if k != 'vanilla'])
    text = '\n'.join(report)
    (OUT / 'report.txt').write_text(text, encoding='utf-8')
    print(text)


if __name__ == '__main__':
    main()
