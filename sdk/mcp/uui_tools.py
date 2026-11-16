"""UltimateUI from the MCP server: everything its in-game editor does (pages, elements, their properties, actions,
animation keyframes, images, world UIs) done to a server's files, checked with the plugin's own layout
(`uui-check`), applied with `/uui reload`, and seen with a screenshot on the test server.

A `server` is a launcher server's name (AppData/Roaming/horizon-launcher/servers/<name>), a server folder, or empty
for the test rig. The plugin's folder is <server>/plugins/UltimateUI (or mods/UltimateUI).
"""
import workspace  # noqa: E402
import json
import os
import pathlib
import re
import shutil
import subprocess
import time
import urllib.request

import yaml

CRATE = workspace.at('plugin-ports', 'ultimateui_rs')
CHECK = CRATE / 'target' / 'release' / 'uui_check.exe'
DLL = CRATE / 'target' / 'release' / 'ultimateui_rs.dll'
SERVER_EXE = pathlib.Path(os.environ.get('APPDATA', '')) / 'horizon-launcher' / 'server-runtime' / 'current' / 'horizon-server.exe'
LAUNCHER_SERVERS = pathlib.Path(os.environ.get('APPDATA', '')) / 'horizon-launcher' / 'servers'
RIG = workspace.at('paper-server', 'server')
# The newest Horizon client (it draws the pack's screen text and lifted items).
CLIENT = workspace.at('v2-target', 'fast', 'horizon-client.exe')

REFERENCE = {
    'page': """A page: contents/pages/<name>.yml.
name: shop
command: shop                  # optional: /shop opens it
permission: shop.use           # optional
screen: { width: 1920, height: 1080, hitboxOffsetX: -2, hitboxOffsetY: -17, cursorSize: 10 }
behavior: { keep-open: false, close-on-f: true, close-on-damage: false }
variables: { coins: { value: "0" } }     # {coins} in texts; *name/$name persist per player
keybinds: { Q: [actions...], W: [...], "1": [...] }   # W A S D Q F SHIFT 1-9, SHIFT_x when sneaking
functions: { buy: { actions: [...] } }
blocks: [elements...]          # drawn in layer order; children's positions add to their parent's""",
    'elements': """Every element: id, layer, opacity 0-255, color rrggbb, position {x,y}, size {width,height} (numbers or sums
like 1920/2-200), rotation, visible, children, actions, hover_text, hover: {color: none|auto|hex, effect: <effects/name>},
click: {effect}, disable-hitbox, aligned/anchor: left|center|right (wide screens), editor_animation (see animation).
type: block       solid box (unicode glyph optional), outline: {size, color}
type: rounded     rounded box, radius (26), rounding.unicode glyphs
type: text        text: "MiniMessage <#ff0000>red {var} %player%", size = font size (64 normal), align left|center|right,
                  font (default game font), text-wrap; %player_head% or %player_head:<name>% draws a face
type: image       image: <file in contents/images without extension>, size scales it; frames: <folder>, interval-ms,
                  animation-loop; hover: {image}
type: block + item: DIAMOND | DIAMOND:<model data> | player_head:<name> | slot:<n>, glowing
type: skin        skin: %player% | <name>, slim, parts.<head|middle|right_arm|left_arm|right_leg|left_leg>.rotation.{x,y,z},
                  spin: {spinning, spin-speed (ticks a turn), mouse, up-down, unlimited-rot}
type: input       variable, placeholder, placeholder-color, text-color, text.size, max-length (click to type in chat)
type: hitbox      invisible click area
type: container   listLayout: {direction: vertical|horizontal, spacing, wrap, perLine, alignX, alignY, padding,
                  sortOrder: name|id, startCorner}; grid_block: {direction: row|column, gap, maxRow}
scroll: {axis, speed, smooth, fadeEnd, clip, scrollbar: {color, width}} on a box makes its children scroll;
filter: {input: <variable>, mode: contains|starts|words|regex, fields} keeps matching children;
component: <contents/components/name> with params: {...} and ${param} inside; loop: N (counter); case: "{var}" with
branches; enabled: false.""",
    'actions': """actions: a list run in order on click (or keybind/function):
{type: command, value: "spawn"} (as the player)   {type: console, value: "give %player% diamond 1"}
{type: message, value: "<#8dfc00>Hi"}   {type: sound, value: "ui.button.click 1 1.2"}   {type: close}
{type: redirect, value: <page>}   {type: teleport, value: "x y z [world]"}   {type: give, value: "DIAMOND 3"}
{type: delay, value: 20 | 1s | 500ms}   {type: variable, variable: coins, value: "{coins}+1"} (sums work)
{type: condition, value: "{coins} >= 5", actions: [...], else: [...]}   {type: permission, value: perm, actions, else}
{type: function, value: buy}   {type: visibility, value: "show|hide|toggle", targets: [ids]}
{type: input, variable: name, title: "Your name", filter: digits|signed|decimal, max-length: 12,
 on-submit: [...], on-cancel: [...]} (an anvil; chat when none opens)   {type: shaders|cursor, value: on|off|toggle}""",
    'animation': """editor_animation: {delay: ticks, keyframes: {position: {'0': {y: 1200}, '15': {y: 300, interpolation: back-out}},
 scale: {'0': {width: 10, height: 10}, '20': {width: 400, height: 300}}, rotation: {'0': -30, '20': 0}, opacity: {'0': 0, '10': 255}}}
Positions are page pixels (absolute). addX/addY add to where it is. Easings: linear smooth ease-in ease-out bezier bounce
bounce-in bounce-out back back-in back-out. 20 ticks = 1 s. animation-loop: true on the page repeats them.""",
    'world': """World UIs: a page shown in the world: contents/world/<id>.yml
page, location {world, x, y, z, yaw}, size (blocks wide), look-at off|vertical|on, interactive, per-player,
mode static|follow (follow: {distance, height, side}), enabled, view-distance, interact-distance, permission.
Commands: /uui world list|remove <id>|lookat <id> on|interactive <id> on|size <id> <w>|toggle <id>|reload.""",
    'images': """Images: PNG/JPG/GIF/BMP/WEBP files in contents/images (folders allowed); the self-hosted pack carries them as glyphs
after /uui reload (clients get the new pack when they join or /uui pack <player>). Refer to one by its file name in
lower case with _ for spaces and dashes.""",
}


def reference(topic=''):
    """How UltimateUI pages are written: page, elements, actions, animation, world, images (or all)."""
    if topic and topic in REFERENCE:
        return REFERENCE[topic]
    return '\n\n'.join(f'== {k}\n{v}' for k, v in REFERENCE.items())


def _server(server):
    if not server or server in ('rig', 'test'):
        return RIG
    path = pathlib.Path(server)
    if path.is_dir():
        return path
    named = LAUNCHER_SERVERS / server
    if named.is_dir():
        return named
    raise ValueError(f'no server "{server}": a launcher server name, a folder, or empty for the test rig')


def _data(server):
    root = _server(server)
    for sub in ('plugins', 'mods'):
        folder = root / sub / 'UltimateUI'
        if folder.is_dir():
            return root, folder
    return root, root / 'plugins' / 'UltimateUI'


def _page_file(server, page):
    _, data = _data(server)
    name = page.strip().removesuffix('.yml').lower()
    if not re.fullmatch(r'[a-z0-9_-]+', name):
        raise ValueError(f'bad page name {page!r}')
    return data / 'contents' / 'pages' / f'{name}.yml'


def _load(server, page):
    path = _page_file(server, page)
    if not path.is_file():
        raise ValueError(f'no page {page} ({path})')
    return path, yaml.safe_load(path.read_text(encoding='utf-8-sig')) or {}


def _value(text):
    """A tool argument given as YAML or JSON text, or already a value."""
    if isinstance(text, (dict, list)):
        return text
    return yaml.safe_load(text) if text not in (None, '') else None


def console(server, command):
    """Runs a console command on a running server (no leading /) and gives its answer."""
    root = _server(server)
    done = subprocess.run([str(SERVER_EXE), '--dir', str(root), 'cmd', command], capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=60)
    return (done.stdout + done.stderr).strip()


def check(server, page):
    """The page as the plugin lays it out: each element's id, kind, block path, box, layer, text, and problems."""
    _, data = _data(server)
    path = _page_file(server, page)
    if not CHECK.is_file():
        return {'error': f'no {CHECK}: build it (cargo build --release --bin uui_check in {CRATE})'}
    done = subprocess.run([str(CHECK), str(data), str(path)], capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=60)
    try:
        return json.loads(done.stdout)
    except json.JSONDecodeError:
        return {'error': (done.stdout + done.stderr)[-800:]}


def _save(server, page, doc, reload=True):
    path = _page_file(server, page)
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_file():
        backups = path.parent.parent / 'backups'
        backups.mkdir(exist_ok=True)
        shutil.copy2(path, backups / f'{path.stem}-{time.strftime("%Y%m%d-%H%M%S")}.yml')
    doc.setdefault('name', path.stem)
    path.write_text(yaml.safe_dump(doc, sort_keys=False, allow_unicode=True, width=160), encoding='utf-8')
    report = check(server, page)
    out = {'file': str(path), 'elements': len(report.get('elements', [])), 'problems': report.get('problems', report.get('error'))}
    if reload:
        out['reload'] = _reload(server)
    return out


def _reload(server):
    try:
        return console(server, 'uui reload')[-300:]
    except Exception as error:  # the server is down: the files are written, it reads them when it starts
        return f'not reloaded ({error}); it loads the files when it starts'


def _paths(blocks, prefix=''):
    for n, b in enumerate(blocks or []):
        path = f'{prefix}{n}'
        yield path, b
        if isinstance(b, dict):
            yield from _paths(b.get('children'), path + '/')


def _find(doc, target):
    """A block by id, name or path (`3/1`), with the list holding it and its index."""
    blocks = doc.setdefault('blocks', [])
    for path, b in _paths(blocks):
        if target in (path, b.get('id') if isinstance(b, dict) else None, b.get('name') if isinstance(b, dict) else None):
            parts = [int(p) for p in path.split('/')]
            holder = blocks
            for p in parts[:-1]:
                holder = holder[p].setdefault('children', [])
            return b, holder, parts[-1], path
    raise ValueError(f'no element {target!r}; ids: {[b.get("id") for _, b in _paths(blocks) if isinstance(b, dict) and b.get("id")][:60]}')


def _set(block, key, value):
    parts = key.split('.')
    at = block
    for k in parts[:-1]:
        if not isinstance(at.get(k), dict):
            at[k] = {}
        at = at[k]
    if value is None:
        at.pop(parts[-1], None)
    else:
        at[parts[-1]] = value


def pages(server=''):
    """The server's UltimateUI pages: name, size, element count, command, description."""
    _, data = _data(server)
    out = []
    for path in sorted((data / 'contents' / 'pages').glob('*.yml')):
        try:
            doc = yaml.safe_load(path.read_text(encoding='utf-8-sig')) or {}
        except yaml.YAMLError as error:
            out.append({'page': path.stem, 'error': str(error)[:200]})
            continue
        if path.stem in ('editor', 'editor_menu', 'editor_empty'):
            continue
        screen = doc.get('screen') or {}
        out.append({'page': path.stem, 'size': f"{screen.get('width', 1920)}x{screen.get('height', 1080)}", 'blocks': sum(1 for _ in _paths(doc.get('blocks'))),
                    'command': doc.get('command'), 'description': doc.get('description')})
    return {'folder': str(data), 'pages': out, 'images': sorted(p.stem for p in (data / 'contents' / 'images').rglob('*') if p.is_file())[:200],
            'components': sorted(p.stem for p in (data / 'contents' / 'components').glob('*.yml'))}


def page_get(server, page):
    """A page's YAML, its block tree (path: id type) and the laid-out elements' boxes."""
    path, doc = _load(server, page)
    tree = [f"{p}: {b.get('id', '-')} {b.get('type', 'component' if 'component' in b else 'block')}" for p, b in _paths(doc.get('blocks')) if isinstance(b, dict)]
    report = check(server, page)
    return {'file': str(path), 'yaml': path.read_text(encoding='utf-8-sig'), 'tree': tree,
            'layout': [{k: e[k] for k in ('id', 'kind', 'block', 'x', 'y', 'w', 'h', 'layer')} for e in report.get('elements', [])][:300], 'problems': report.get('problems')}


def page_write(server, page, content, reload=True):
    """Creates or replaces a whole page from YAML (the old one is kept in contents/backups), then reloads."""
    doc = _value(content)
    if not isinstance(doc, dict):
        raise ValueError('the page must be a YAML mapping (name, screen, blocks...)')
    return _save(server, page, doc, reload)


def page_new(server, page, width=1920, height=1080, command='', backdrop=True, reload=True):
    """A new empty page (optionally with a dimmed full-screen backdrop and a /command)."""
    if _page_file(server, page).is_file():
        raise ValueError(f'{page} exists: use page_write to replace it')
    doc = {'name': page, 'screen': {'width': width, 'height': height, 'hitboxOffsetX': -2, 'hitboxOffsetY': -17, 'cursorSize': 10}, 'blocks': []}
    if command:
        doc['command'] = command.lstrip('/')
    if backdrop:
        doc['blocks'].append({'type': 'block', 'id': 'backdrop', 'layer': 0, 'opacity': 150, 'color': '000000', 'position': {'x': 0, 'y': 0}, 'size': {'width': width, 'height': height}})
    return _save(server, page, doc, reload)


def page_delete(server, page, reload=True):
    """Deletes a page (a copy goes to contents/backups)."""
    path = _page_file(server, page)
    if not path.is_file():
        raise ValueError(f'no page {page}')
    backups = path.parent.parent / 'backups'
    backups.mkdir(exist_ok=True)
    shutil.move(str(path), str(backups / f'{path.stem}-deleted-{time.strftime("%Y%m%d-%H%M%S")}.yml'))
    return {'deleted': page, 'reload': _reload(server) if reload else 'skipped'}


def element_add(server, page, element, parent='', index=-1, reload=True):
    """Adds an element (YAML/JSON mapping: type, id, position, size...) at the top level or under `parent` (id or path)."""
    path, doc = _load(server, page)
    el = _value(element)
    items = el if isinstance(el, list) else [el]
    if parent:
        holder = _find(doc, parent)[0].setdefault('children', [])
    else:
        holder = doc.setdefault('blocks', [])
    taken = {b.get('id') for _, b in _paths(doc.get('blocks')) if isinstance(b, dict)}
    for item in items:
        if not isinstance(item, dict):
            raise ValueError('each element is a mapping')
        if not item.get('id'):
            kind = item.get('type', 'block')
            item['id'] = next(f'{kind}_{n}' for n in range(1, 10000) if f'{kind}_{n}' not in taken)
        taken.add(item['id'])
        holder.insert(len(holder) if index < 0 else index, item)
    out = _save(server, page, doc, reload)
    out['added'] = [i['id'] for i in items]
    return out


def element_edit(server, page, target, changes, reload=True):
    """Changes an element's keys: `changes` maps dotted keys to values ({"position.x": 300, "color": "ff0000",
    "hover.color": "auto"}); null removes a key. `target` is an id, name or block path."""
    path, doc = _load(server, page)
    block = _find(doc, target)[0]
    for key, value in (_value(changes) or {}).items():
        _set(block, key, value)
    return _save(server, page, doc, reload)


def element_remove(server, page, target, reload=True):
    """Removes an element (and its children)."""
    path, doc = _load(server, page)
    _, holder, index, where = _find(doc, target)
    gone = holder.pop(index)
    out = _save(server, page, doc, reload)
    out['removed'] = gone.get('id', where)
    return out


def element_move(server, page, target, parent='', index=-1, reload=True):
    """Moves an element under another (or to the top level), at `index` (-1: last). Positions stay its own."""
    path, doc = _load(server, page)
    _, holder, i, _ = _find(doc, target)
    block = holder.pop(i)
    dest = _find(doc, parent)[0].setdefault('children', []) if parent else doc.setdefault('blocks', [])
    dest.insert(len(dest) if index < 0 else index, block)
    return _save(server, page, doc, reload)


def action_add(server, page, target, action, reload=True):
    """Adds actions (one mapping or a list) to an element's click actions, e.g. {type: message, value: hi}."""
    path, doc = _load(server, page)
    block = _find(doc, target)[0]
    new = _value(action)
    block.setdefault('actions', []).extend(new if isinstance(new, list) else [new])
    return _save(server, page, doc, reload)


def actions_set(server, page, target, actions, reload=True):
    """Replaces an element's actions with a list (empty list: none)."""
    path, doc = _load(server, page)
    block = _find(doc, target)[0]
    block['actions'] = _value(actions) or []
    return _save(server, page, doc, reload)


def keyframe(server, page, target, track, tick, value, reload=True):
    """Sets an open-animation keyframe: track position|scale|rotation|opacity, tick (20 = 1 s), value (a number for
    rotation/opacity, or a mapping {x, y} / {width, height}, with interpolation). Null value removes the keyframe."""
    path, doc = _load(server, page)
    block = _find(doc, target)[0]
    keys = block.setdefault('editor_animation', {}).setdefault('keyframes', {}).setdefault(track, {})
    v = _value(value) if isinstance(value, str) else value
    if v is None:
        keys.pop(str(tick), None)
    else:
        keys[str(tick)] = v
    return _save(server, page, doc, reload)


def image_add(server, source, name=''):
    """Adds an image to contents/images from a file path or URL, then reloads (the pack is rebuilt with it)."""
    _, data = _data(server)
    folder = data / 'contents' / 'images'
    folder.mkdir(parents=True, exist_ok=True)
    suffix = pathlib.Path(source.split('?')[0]).suffix.lower() or '.png'
    if suffix not in ('.png', '.jpg', '.jpeg', '.gif', '.bmp', '.webp'):
        raise ValueError(f'not an image: {source}')
    stem = re.sub(r'[^a-z0-9]+', '_', (name or pathlib.Path(source.split('?')[0]).stem).lower()).strip('_') or 'image'
    target = folder / f'{stem}{suffix}'
    if re.match(r'https?://', source):
        with urllib.request.urlopen(source, timeout=20) as reply:
            target.write_bytes(reply.read(20_000_000))
    else:
        shutil.copy2(source, target)
    return {'image': stem, 'file': str(target), 'reload': _reload(server)}


def world_place(server, id, page, world, x, y, z, yaw=0.0, size=2.0, look_at='off', interactive=True, per_player=False, reload=True):
    """Places a page in the world (contents/world/<id>.yml) at a position facing `yaw`, `size` blocks wide."""
    _, data = _data(server)
    folder = data / 'contents' / 'world'
    folder.mkdir(parents=True, exist_ok=True)
    doc = {'page': page, 'location': {'world': world, 'x': x, 'y': y, 'z': z, 'yaw': yaw}, 'size': size, 'look-at': look_at, 'interactive': interactive,
           'per-player': per_player, 'mode': 'static', 'enabled': True, 'view-distance': 48, 'interact-distance': 6}
    (folder / f'{id}.yml').write_text(yaml.safe_dump(doc, sort_keys=False), encoding='utf-8')
    return {'file': str(folder / f'{id}.yml'), 'reload': console(server, 'uui world reload') if reload else 'skipped'}


def command(server, line):
    """Any /uui command from the console: open <page> <player>, close <player>, list, reload, world ..., pack <player>."""
    return console(server, 'uui ' + line.removeprefix('/').removeprefix('uui ').strip())


def screenshot(page, server='', seconds=3.0, look=''):
    """Shows a page on the test rig and gives a screenshot path: copies the page (and the server's images and
    components) and the newest UltimateUI to the rig, joins with the newest Horizon client, opens the page.
    `look` "yaw pitch" turns the head first (moves the cursor). Refuses while someone plays on the rig."""
    import maker_tools
    paper_server, proof = maker_tools.rig()
    if (on := maker_tools.players_on(paper_server)):
        return {'error': f'{", ".join(on)} on the test server'}
    _, data = _data(server)
    _, rig_data = _data('')
    if data != rig_data:
        for sub in ('pages', 'components', 'images'):
            src = data / 'contents' / sub
            if src.is_dir():
                shutil.copytree(src, rig_data / 'contents' / sub, dirs_exist_ok=True)
    picture = str(paper_server.ROOT / f'uui-{page}.png')
    with maker_tools.Lock():
        paper_server.down()
        time.sleep(2)
        shutil.copy2(DLL, paper_server.ROOT / 'server' / 'plugins' / DLL.name)
        if not maker_tools.restart(paper_server, proof):
            return {'error': 'the server did not start', 'problems': maker_tools.problems(paper_server)}
        client = CLIENT if CLIENT.is_file() else paper_server.ENGINE
        proof.captured([str(x) for x in (proof.CTL, '--timeout', 90, 'launch', '--game', client, '--offscreen', '--width', 1280, '--height', 720,
                                          '--game-dir', paper_server.ROOT / 'client', '--resources', proof.RESOURCES, '--name', paper_server.NAME)], 120, paper_server.environment())
        paper_server.ctl('connect', f'localhost:{paper_server.PORT}', paper_server.NAME, timeout=60)
        for _ in range(8):
            time.sleep(1.5)
            ui = paper_server.ctl('ui', timeout=10)
            try:
                widgets = (ui if isinstance(ui, dict) else json.loads(ui)).get('widgets', [])
            except Exception:
                widgets = []
            yes = next((w for w in widgets if str(w.get('label', '')).strip().lower() in ('yes', 'proceed', 'accept')), None)
            if yes:
                paper_server.ctl('click', yes['id'], timeout=10)
                break
        paper_server.ctl('wait-ready', timeout=120)
        time.sleep(5)
        paper_server.ctl('look', '0', '0', timeout=10)
        paper_server.ctl('chat', f'/uui open {page}', timeout=10)
        if look:
            time.sleep(1)
            paper_server.ctl('look', *look.split()[:2], timeout=10)
        time.sleep(seconds)
        paper_server.ctl('screenshot', picture, timeout=20)
        paper_server.ctl('quit', timeout=15)
        chat = [row.split('[CHAT] ', 1)[1].rstrip() for row in maker_tools.read(paper_server.ROOT / 'client' / 'logs' / 'latest.log').splitlines() if '[CHAT] ' in row]
    return {'screenshot': picture, 'chat': chat[-6:], 'problems': maker_tools.problems(paper_server)}


TOOLS = {
    'uui_reference': (reference, 'UltimateUI page format: how to write pages, elements, actions, animations, world UIs and images (topic: page, elements, actions, animation, world, images; empty = all). Read before making a UI.', {'topic': 'string'}),
    'uui_pages': (pages, "A server's UltimateUI pages, images and components (server: launcher server name, folder, or empty for the test rig).", {'server': 'string'}),
    'uui_page_get': (page_get, "A page's YAML, block tree (paths and ids) and each element's laid-out box from the plugin's own layout.", {'server': 'string', 'page': 'string'}),
    'uui_page_new': (page_new, 'Create an empty page (size, optional /command, dimmed backdrop), then reload.', {'server': 'string', 'page': 'string', 'width': 'integer', 'height': 'integer', 'command': 'string', 'backdrop': 'boolean', 'reload': 'boolean'}),
    'uui_page_write': (page_write, 'Create or replace a whole page from YAML text (old copy kept in contents/backups), check its layout, reload.', {'server': 'string', 'page': 'string', 'content': 'string', 'reload': 'boolean'}),
    'uui_page_delete': (page_delete, 'Delete a page (moved to contents/backups).', {'server': 'string', 'page': 'string', 'reload': 'boolean'}),
    'uui_element_add': (element_add, 'Add an element or a list of them (YAML/JSON) to a page, at the top level or under `parent` (id or path). Ids are made when missing.', {'server': 'string', 'page': 'string', 'element': 'string', 'parent': 'string', 'index': 'integer', 'reload': 'boolean'}),
    'uui_element_edit': (element_edit, 'Change an element\'s properties: `changes` maps dotted keys to values (YAML/JSON), null deletes (e.g. {"position.x": 200, "size.width": 400, "color": "2563eb", "text": "Buy"}).', {'server': 'string', 'page': 'string', 'target': 'string', 'changes': 'string', 'reload': 'boolean'}),
    'uui_element_remove': (element_remove, 'Remove an element and its children.', {'server': 'string', 'page': 'string', 'target': 'string', 'reload': 'boolean'}),
    'uui_element_move': (element_move, 'Move an element under another parent (or the top level) at an index.', {'server': 'string', 'page': 'string', 'target': 'string', 'parent': 'string', 'index': 'integer', 'reload': 'boolean'}),
    'uui_action_add': (action_add, 'Add click actions (one or a list, YAML/JSON) to an element; uui_reference actions lists them.', {'server': 'string', 'page': 'string', 'target': 'string', 'action': 'string', 'reload': 'boolean'}),
    'uui_actions_set': (actions_set, "Replace an element's actions with a list.", {'server': 'string', 'page': 'string', 'target': 'string', 'actions': 'string', 'reload': 'boolean'}),
    'uui_keyframe': (keyframe, 'Set (or with null remove) an open-animation keyframe on an element: track position|scale|rotation|opacity, tick, value.', {'server': 'string', 'page': 'string', 'target': 'string', 'track': 'string', 'tick': 'integer', 'value': 'string', 'reload': 'boolean'}),
    'uui_image_add': (image_add, 'Add an image (file path or URL) to contents/images and reload so the pack carries it.', {'server': 'string', 'source': 'string', 'name': 'string'}),
    'uui_world_place': (world_place, 'Place a page in the world as a world UI (id, page, world, x, y, z, yaw, size in blocks, look_at off|vertical|on, interactive, per_player).', {'server': 'string', 'id': 'string', 'page': 'string', 'world': 'string', 'x': 'number', 'y': 'number', 'z': 'number', 'yaw': 'number', 'size': 'number', 'look_at': 'string', 'interactive': 'boolean', 'per_player': 'boolean', 'reload': 'boolean'}),
    'uui_command': (command, 'Run a /uui command on a running server from the console (open <page> <player>, close <player>, list, reload, world ..., pack <player>).', {'server': 'string', 'line': 'string'}),
    'uui_check': (check, "A page's layout as the plugin computes it (every element's box) and its problems (missing images, off-page elements), without a server.", {'server': 'string', 'page': 'string'}),
    'uui_screenshot': (screenshot, "See a page: it is copied to the test rig, opened for the test client and screenshotted (the newest UltimateUI build). Refuses while someone plays on the rig.", {'page': 'string', 'server': 'string', 'seconds': 'number', 'look': 'string'}),
}
