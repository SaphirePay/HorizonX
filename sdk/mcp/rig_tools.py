"""A private test server (the "rig": <workspace>/parity/jobs/server on :25797) as tools: build a plugin and hot-drop it, see items in game with
their tooltips, and rejoin after a pack change. All with the muted offscreen
"Tester" client (<workspace>/parity/pwarps/pw.py), which is closed again when a call is done.

    rig_drop(crate)                       build plugin-ports/<crate> and hot-drop it onto the rig
    rig_shots(commands, item, name)       run commands as Tester, hover every matching item, screenshot each
    rig_rejoin()                          reconnect and wait for the server packs to finish loading
"""
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

import workspace

HORIZON = workspace.WORKSPACE
PORTS = HORIZON / 'plugin-ports'
RIG = HORIZON / 'parity' / 'jobs' / 'server'
PLUGINS = RIG / 'plugins'
PW_DIR = HORIZON / 'parity' / 'pwarps'
SHOTS = PW_DIR / 'shots' / 'rig'
PACK_DIR = HORIZON / 'parity' / 'jobs' / 'packsrv'
ADDRESS = '127.0.0.1:25797'
TMP = pathlib.Path(os.environ.get('HORIZON_TMP') or HORIZON / 'tmp')


def _pw():
    sys.path.insert(0, str(PW_DIR))
    import pw  # noqa: E402
    return pw


def _client_up(pw):
    """Starts and joins the Tester client if it isn't in a world already. True if it had to start."""
    state = pw.j('state')
    if state.get('dimension') and state.get('player'):
        return False
    _close_client(pw)  # a client stuck outside a world: start clean
    subprocess.run([sys.executable, str(PW_DIR / 'pw.py'), 'up'], capture_output=True, timeout=150, cwd=PW_DIR)
    pw.c('connect', ADDRESS)
    pw.c('wait-ready', 120, timeout=130)
    time.sleep(20)  # server packs download and load after joining
    return True


def _tester_pids():
    ps = ("Get-CimInstance Win32_Process -Filter \"Name='horizon-client.exe'\" | "
          "Where-Object { $_.CommandLine -match 'pwarps.client2' } | ForEach-Object { $_.ProcessId }")
    run = subprocess.run(['powershell', '-NoProfile', '-Command', ps], capture_output=True, text=True, timeout=30)
    return [int(x) for x in run.stdout.split() if x.isdigit()]


def _close_client(pw):
    """Quit the Tester client, and stop any copy still running after that (by PID, never by name)."""
    pw.c('quit')
    for _ in range(10):
        if not _tester_pids():
            return
        time.sleep(0.5)
    for pid in _tester_pids():
        subprocess.run(['powershell', '-NoProfile', '-Command', f'Stop-Process -Id {pid} -Confirm:$false'], capture_output=True, timeout=30)


def rig_drop(crate, release=True):
    """Build plugin-ports/<crate> (shared target dir) and hot-drop its DLL onto the rig with a fresh time, so the
    rig swaps it in (an empty server swaps on the next join)."""
    path = PORTS / crate if not pathlib.Path(crate).is_absolute() else pathlib.Path(crate)
    if not (path / 'Cargo.toml').exists():
        return {'error': f'no crate at {path}'}
    name = re.search(r'^name = "([^"]+)"', (path / 'Cargo.toml').read_text(encoding='utf-8'), re.M).group(1).replace('-', '_')
    t0 = time.time()
    run = subprocess.run(['cargo', 'build', *(['--release'] if release else [])], cwd=path, capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=900)
    errors = [l for l in run.stderr.split('\n') if l.startswith('error') or l.strip().startswith('-->')]
    if run.returncode != 0:
        return {'ok': False, 'errors': errors[:30]}
    target = path / 'target' / ('release' if release else 'debug') / f'{name}.dll'
    if not target.exists():
        shared = PORTS / 'target' / ('release' if release else 'debug') / f'{name}.dll'
        target = shared if shared.exists() else target
    if not target.exists():
        return {'ok': False, 'error': f'built, but no {name}.dll found'}
    dest = next((p for p in PLUGINS.glob('*.dll') if p.stem.lower() == name.lower()), PLUGINS / f'{name}.dll')
    tmp = TMP / f'rigdrop-{name}.dll'
    TMP.mkdir(parents=True, exist_ok=True)
    shutil.copy2(target, tmp)
    os.utime(tmp, None)
    shutil.move(str(tmp), str(dest))
    return {'ok': True, 'dropped': str(dest), 'build_seconds': round(time.time() - t0, 1), 'warnings': run.stderr.count('warning:'),
            'note': 'the rig swaps it in now (or on the next join if nobody is on)'}


def rig_rejoin():
    """Reconnect the Tester client and wait for the server packs to load. Needed after a server pack changes:
    a pack change during a join can make the client drop its last pack reload and keep the old packs."""
    pw = _pw()
    if _client_up(pw):
        return {'ok': True, 'note': 'client started and joined fresh'}
    pw.c('disconnect')
    time.sleep(2)
    pw.c('connect', ADDRESS)
    pw.c('wait-ready', 120, timeout=130)
    time.sleep(20)
    log = (PW_DIR / 'client2' / 'logs' / 'latest.log').read_text(encoding='utf-8', errors='replace').split('\n')
    applied = [l for l in log if 'pack reload: applied' in l]
    return {'ok': True, 'last_pack_load': applied[-1][:80] if applied else 'none seen'}


def _center(pw, slot):
    for w in pw.j('ui').get('widgets', []):
        if w.get('id') == f'slot-{slot}':
            x, y, ww, hh = w['bounds']
            return x + ww / 2, y + hh / 2
    return None


def _crop_tooltip(path):
    """Cut a screenshot down to the item tooltip: its purple top border, then its left border down to the end."""
    import numpy as np
    from PIL import Image
    im = Image.open(path).convert('RGB')
    a = np.array(im).astype(int)
    top_border = (abs(a[:, :, 0] - 36) < 6) & (a[:, :, 1] < 8) & (abs(a[:, :, 2] - 91) < 8)
    ys, xs = np.where(top_border)
    if not len(xs):
        return
    top, left, right = ys.min(), max(xs.min(), 1), xs.max()
    y = top
    while y + 1 < a.shape[0]:
        r, g, b = a[y + 1, left]
        if not (g < 6 and 18 < r < 42 and b > 40):
            break
        y += 1
    im.crop((max(left - 4, 0), max(top - 6, 0), min(right + 5, a.shape[1]), min(y + 6, a.shape[0]))).save(path)


def rig_shots(commands, item='', name='shot', crop=True, close=True, fill_hotbar=True):
    """Run `commands` (`;`-separated, as the opped Tester, no slash) on the rig, open the inventory, hover every
    slot holding `item` (an item id like `clock`, or empty for every non-empty slot from the commands' items) and
    screenshot each tooltip. Gives the image paths (cropped to the inventory and tooltip). Clears the inventory first."""
    from PIL import Image
    pw = _pw()
    _client_up(pw)
    SHOTS.mkdir(parents=True, exist_ok=True)
    pw.c('command', 'minecraft:clear @s')
    if fill_hotbar:  # so given items land in the inventory rows, where their tooltips fit on screen
        for i in range(9):
            pw.c('command', f'minecraft:item replace entity @s hotbar.{i} with minecraft:gray_stained_glass_pane')
    for cmd in [c.strip().lstrip('/') for c in commands.split(';') if c.strip()]:
        pw.c('command', cmd)
        time.sleep(0.4)
    time.sleep(1.0)
    pw.c('open-inventory')
    time.sleep(0.9)
    want = item.strip().lower()
    if want and ':' not in want:
        want = 'minecraft:' + want
    slots = [x['slot'] for x in pw.j('menu').get('slots', []) if (x.get('stack') or {}).get('item') and (not want or x['stack']['item'] == want) and x['slot'] >= 9]
    paths = []
    for i, slot in enumerate(slots[:12]):
        at = _center(pw, slot)
        if not at:
            continue
        pw.c('scroll-at', *at, 0)
        time.sleep(0.8)
        out = SHOTS / f'{name}_{i + 1}.png'
        pw.c('screenshot', str(out))
        if crop and out.exists():
            _crop_tooltip(out)
        paths.append(str(out))
    pw.c('close')
    if close:
        _close_client(pw)
    return {'slots': slots, 'shots': paths, 'tip': 'items in hotbar slots get cut off: put them in slots 9-35 (set_item_at) for full tooltips'}



TOOLS = {
    'rig_drop': (rig_drop, 'Build a plugin crate (plugin-ports/<crate>) and hot-drop its DLL onto the private test server (the jobs rig :25797) with a fresh file time. Errors come back as file:line.', {'crate': 'string', 'release': 'boolean'}),
    'rig_shots': (rig_shots, 'See items in game: on the rig, as the muted offscreen Tester (op), clear the inventory (hotbar filled so gives land where tooltips fit), run `;`-separated commands, open the inventory, hover each slot with `item` (e.g. clock) and screenshot every tooltip. Gives image paths cropped to each tooltip; closes the client after.', {'commands': 'string', 'item': 'string', 'name': 'string', 'crop': 'boolean', 'close': 'boolean', 'fill_hotbar': 'boolean'}),
    'rig_rejoin': (rig_rejoin, 'Reconnect the rig Tester client and wait for server packs to load (after any server pack change: a change during a join can leave the client on old packs).', {}),
}
