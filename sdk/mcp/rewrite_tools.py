"""Tools for rewriting any Java plugin by hand in Rust (plugin-port MCP server). They read the plugin's decompiled,
decoded Java (its own code: bundled libraries are skipped) and the Horizon engine and client, and answer the
questions a rewrite keeps asking:

- java_decode: obfuscated constants (AES strings, scrambled numbers) back to plain Java, into java_clean/;
- method_body: a method's code by name, wherever in a class chain it is;
- trace_key: every place a string (YAML key, permission, message, packet name) is used, the method around it and who
  calls that method;
- config_schema: every config key the code reads, with its type and default;
- packet_map: the packets the plugin sends and reads, and for the ones it reads, the Bukkit event that carries the same
  input and whether the engine fires it;
- client_caps: what the Horizon client does that a plugin's resource pack or packets rely on;
- rig_doctor: why a test server would not start or would refuse a DLL, before a test run finds out.
"""
import workspace  # noqa: E402
import collections
import pathlib
import re
import sys

HORIZON = workspace.WORKSPACE
PORTS = HORIZON / 'plugin-ports'
JPORT = HORIZON / 'plugin-tools' / 'jport'
CLIENT = HORIZON / 'v2' / 'crates'
STAGED = HORIZON / 'v2-paper' / 'fresh' / 'crates' / 'horizon-client' / 'src' / 'game.rs'
SDK = HORIZON / 'paper-engine' / 'sdk' / 'horizon-bukkit' / 'src'
LIBS = re.compile(r'/(libs?|shaded?|shadow|relocated?|dependencies|deps|thirdparty|third_party|vendor|bundled)/|'
                  r'^(io/netty|com/google|kotlin|kotlinx|net/kyori|org/(apache|bstats|jetbrains|intellij|slf4j|yaml|json|checkerframework|jspecify)|'
                  r'com/(github/retrooper|zaxxer|fasterxml|mysql)|okhttp3|okio|it/unimi)/', re.I)


def java_root(id):
    """A plugin's Java: a folder path, or an id (`ultimateui_rs`, `ultimateui`) under plugin-ports; decoded first."""
    path = pathlib.Path(id)
    if path.is_dir():
        return path
    for candidate in (PORTS / id / 'java_clean', PORTS / f'{id}_rs' / 'java_clean', PORTS / id / 'java', PORTS / f'{id}_rs' / 'java',
                      PORTS / '_java' / id, PORTS / id / '.jar' / 'java'):
        if candidate.is_dir():
            return candidate
    return None


def own_files(root):
    """The plugin's own .java files (no bundled library), relative path and text."""
    for path in sorted(root.rglob('*.java')):
        rel = path.relative_to(root).as_posix()
        if LIBS.search('/' + rel) or LIBS.search(rel):
            continue
        yield rel, path.read_text(encoding='utf-8', errors='replace')


def _need(id):
    root = java_root(id)
    if root is None:
        raise ValueError(f'no Java for {id}: give a folder, or import the jar first (plugin_import / new_rewrite.py)')
    return root


def java_decode(id):
    """Decode an obfuscated plugin's Java into java_clean/ beside it (the tools then read that)."""
    root = _need(id)
    sys.path.insert(0, str(JPORT))
    import deobfuscate
    out = root.parent / 'java_clean' if root.name != 'java_clean' else root
    stats = deobfuscate.decode_tree(root, out if out != root else None)
    return {'java': str(root), 'decoded_into': str(out), **stats}


DECL = re.compile(r'^(\s*)(?:(?:public|protected|private|static|final|abstract|synchronized|native|default|@\w+(?:\([^)]*\))?)\s+)*'
                  r'(?:<[^>]+>\s+)?[\w.$<>\[\], ?]+\s+(\w+)\s*\(')


def is_decl(line):
    """A method declaration line, not a call or `new X(` in a statement."""
    m = DECL.match(line)
    if not m or '=' in line.split('(')[0] or ' new ' in f' {line.strip()}' or line.strip().startswith(('return', 'throw', 'else', 'case')):
        return None
    if m.group(2) in ('if', 'for', 'while', 'switch', 'catch', 'synchronized', 'return'):
        return None
    return m


def _body(lines, start):
    """The lines of the method declared at `start`, to its closing brace."""
    depth, opened, out = 0, False, []
    for line in lines[start:start + 600]:
        out.append(line)
        clean = re.sub(r'"(?:[^"\\]|\\.)*"', '""', line)
        depth += clean.count('{') - clean.count('}')
        opened = opened or '{' in clean
        if opened and depth <= 0:
            break
        if not opened and clean.rstrip().endswith(';'):
            break
    return out


def method_body(id, name, cls='', limit=4):
    """A method's code by name (any overload), optionally in one class; with the class's `extends` chain."""
    root = _need(id)
    hits = []
    for rel, text in own_files(root):
        if cls and pathlib.PurePosixPath(rel).stem != cls and f'class {cls}' not in text:
            continue
        if f' {name}(' not in text:
            continue
        lines = text.split('\n')
        chain = re.search(r'class\s+(\w+)(?:<[^>]*>)?\s+extends\s+([\w.<>]+)', text)
        for i, line in enumerate(lines):
            m = is_decl(line)
            if m and m.group(2) == name:
                body = _body(lines, i)
                hits.append({'file': rel, 'line': i + 1, 'extends': chain.group(2) if chain else None,
                             'lines': len(body), 'code': '\n'.join(body[:160]) + ('\n   ...' if len(body) > 160 else '')})
                if len(hits) >= int(limit):
                    return {'java': str(root), 'found': hits}
    return {'java': str(root), 'found': hits}


def _enclosing(lines, at):
    for i in range(at, -1, -1):
        m = is_decl(lines[i])
        if m:
            return m.group(2), i + 1
    return None, 0


def trace_key(id, text, exact=False, limit=40):
    """Every place a string literal appears (a YAML key, permission, message...), the method around it, and who
    calls that method."""
    root = _need(id)
    needle = re.compile(r'"' + re.escape(text) + r'"' if exact else r'"[^"\n]*' + re.escape(text) + r'[^"\n]*"')
    uses, methods = [], collections.Counter()
    files = list(own_files(root))
    for rel, body in files:
        if text not in body:
            continue
        lines = body.split('\n')
        for i, line in enumerate(lines):
            if needle.search(line):
                method, at = _enclosing(lines, i)
                uses.append({'file': rel, 'line': i + 1, 'in': method, 'code': line.strip()[:200]})
                if method:
                    methods[method] += 1
                if len(uses) >= int(limit):
                    break
    callers = {}
    for method in list(methods)[:8]:
        calls = []
        for rel, body in files:
            if f'{method}(' not in body:
                continue
            lines = body.split('\n')
            for i, line in enumerate(lines):
                if re.search(r'[.\s(]' + re.escape(method) + r'\(', line) and not is_decl(line):
                    caller, _ = _enclosing(lines, i)
                    calls.append(f'{rel}:{i + 1} in {caller}')
                    if len(calls) >= 6:
                        break
            if len(calls) >= 6:
                break
        callers[method] = calls
    return {'java': str(root), 'uses': uses, 'callers_of_those_methods': callers}


READ = re.compile(r'\.(getString|getInt|getBoolean|getDouble|getLong|getList|getStringList|getIntegerList|getConfigurationSection|'
                  r'contains|isSet|isConfigurationSection|getItemStack|getLocation|getColor|getKeys|get)\(\s*"([^"\n]+)"\s*(?:,\s*([^)\n]{1,80}))?\)')
KEYLIKE = re.compile(r'"([a-z][\w-]*(?:\.[\w-]+)+)"')


def config_schema(id, limit=300):
    """Every config key the code reads: the call (its type), the default it falls back to, and how often. Then
    dotted strings passed to the plugin's own readers (`readDouble(section, "screen.width", ...)`)."""
    root = _need(id)
    reads = collections.OrderedDict()
    seen_files = collections.defaultdict(set)
    other = collections.Counter()
    for rel, body in own_files(root):
        for m in READ.finditer(body):
            kind, key, default = m.group(1), m.group(2), (m.group(3) or '').strip()
            if not re.fullmatch(r'[A-Za-z][\w-]*(?:\.[\w-]+)*', key) or len(key) > 80:
                continue
            row = reads.setdefault(key, {'key': key, 'reads': collections.Counter(), 'defaults': set(), 'count': 0})
            row['reads'][kind] += 1
            row['count'] += 1
            if default:
                row['defaults'].add(default)
            seen_files[key].add(rel)
        for m in KEYLIKE.finditer(body):
            if m.group(1) not in reads:
                other[m.group(1)] += 1
    rows = [{'key': r['key'], 'type': r['reads'].most_common(1)[0][0].replace('get', '').lower() or 'any', 'default': sorted(r['defaults'])[:3],
             'count': r['count'], 'files': sorted(seen_files[r['key']])[:3]} for r in reads.values()]
    rows.sort(key=lambda r: r['key'])
    return {'java': str(root), 'keys': rows[:int(limit)], 'key_count': len(rows),
            'dotted_strings_read_some_other_way': [k for k, _ in other.most_common(80) if not k.startswith(('java.', 'org.', 'net.', 'com.', 'dev.', 'io.', 'me.'))][:80]}


PACKET = re.compile(r'PacketType\.(?:Play|Configuration|Login)\.(Client|Server)\.([A-Z_]+)|Wrapper(?:Play|Config)(Client|Server)(\w+)')
# A client packet a plugin reads -> the Bukkit event that carries the same input on Horizon.
CLIENT_EVENTS = {
    'ANIMATION': 'PlayerAnimationEvent', 'ARM_ANIMATION': 'PlayerAnimationEvent', 'INTERACT_ENTITY': 'PlayerUseUnknownEntityEvent',
    'USE_ENTITY': 'PlayerUseUnknownEntityEvent', 'USE_ITEM': 'PlayerInteractEvent', 'BLOCK_PLACE': 'PlayerInteractEvent',
    'PLAYER_BLOCK_PLACEMENT': 'PlayerInteractEvent', 'PLAYER_DIGGING': 'PlayerDropItemEvent / PlayerSwapHandItemsEvent / BlockBreakEvent',
    'BLOCK_DIG': 'PlayerDropItemEvent / PlayerSwapHandItemsEvent', 'PLAYER_INPUT': 'PlayerInputEvent', 'STEER_VEHICLE': 'PlayerInputEvent',
    'CHAT_MESSAGE': 'AsyncPlayerChatEvent', 'CHAT': 'AsyncPlayerChatEvent', 'CHAT_COMMAND': 'PlayerCommandPreprocessEvent',
    'RESOURCE_PACK_STATUS': 'PlayerResourcePackStatusEvent', 'PLAYER_ROTATION': 'PlayerMoveEvent (or read get_location each tick)',
    'PLAYER_POSITION': 'PlayerMoveEvent', 'PLAYER_POSITION_AND_ROTATION': 'PlayerMoveEvent', 'POSITION_LOOK': 'PlayerMoveEvent',
    'CLICK_WINDOW': 'InventoryClickEvent', 'WINDOW_CLICK': 'InventoryClickEvent', 'CLOSE_WINDOW': 'InventoryCloseEvent',
    'HELD_ITEM_CHANGE': 'PlayerItemHeldEvent', 'ENTITY_ACTION': 'PlayerToggleSneakEvent / PlayerToggleSprintEvent',
    'TAB_COMPLETE': 'TabCompleteEvent', 'UPDATE_SIGN': 'SignChangeEvent', 'CLIENT_SETTINGS': 'PlayerLocaleChangeEvent',
    'PLUGIN_MESSAGE': 'register_incoming_plugin_channel', 'NAME_ITEM': 'PrepareAnvilEvent',
}


def _engine_kinds():
    text = STAGED.read_text(encoding='utf-8', errors='replace') if STAGED.is_file() else ''
    return set(re.findall(r'"kind"\s*:\s*"(\w+)"', text))


def _event_kinds():
    out = {}
    for path in SDK.glob('*.rs'):
        for name, kind in re.findall(r'held_event!\((\w+),\s*\|value\|\s*value\["kind"\]\s*==\s*"(\w+)"', path.read_text(encoding='utf-8', errors='replace')):
            out[name] = kind
    return out


def packet_map(id):
    """Packets the plugin's own code reads and sends; for each read, the Bukkit event with the same input and whether
    the engine fires it (from the staged engine's `kind`s)."""
    root = _need(id)
    reads, sends = collections.Counter(), collections.Counter()
    for _, body in own_files(root):
        for m in PACKET.finditer(body):
            side, name = (m.group(1), m.group(2)) if m.group(1) else (m.group(3), re.sub(r'(?<!^)([A-Z])', r'_\1', m.group(4)).upper())
            (reads if side == 'Client' else sends)[name] += 1
    engine, kinds = _engine_kinds(), _event_kinds()
    rows = []
    for name, n in reads.most_common():
        event = CLIENT_EVENTS.get(name, '')
        fires = []
        for ev in re.findall(r'\b(\w+Event)\b', event):
            kind = kinds.get(ev)
            fires.append(f'{ev}: ' + ('fires' if kind and kind in engine else f'kind "{kind}" not in the staged engine' if kind else 'fired by the engine itself (no held kind)'))
        rows.append({'packet': name, 'uses': n, 'event': event or 'none known: needs an engine hook', 'engine': fires})
    return {'java': str(root), 'reads': rows, 'sends': [{'packet': k, 'uses': v, 'rust': 'horizon_kit::packets / wire::packet_id'} for k, v in sends.most_common()],
            'engine_kinds': sorted(engine)}


CAPS = {
    'pack text shader on GUI text': ('horizon-client-render/src', r'pack_text::load'),
    'pack text shader on world text (text displays lifted to the screen)': ('horizon-client/src/game', r'pack_hud_text'),
    'SetCamera (spectate an entity)': ('horizon-client/src/game', r'remote_set_camera'),
    'SetPassengers (riding, also client-only vehicles)': ('horizon-client/src/game', r'SetPassengers'),
    'text displays': ('horizon-client/src/game', r'fn text_display_geometry'),
    'item / block displays': ('horizon-client/src/game', r'fn block_item_display_geometry'),
    'interaction entities (click boxes)': ('horizon-client/src/game', r'minecraft:interaction|Interaction'),
    'boss bars': ('horizon-client/src/game', r'BossEvent|boss_bar|BossBar'),
    'server resource packs': ('horizon-client/src', r'ResourcePackPush|resource_pack_push|AddResourcePack'),
    'pack core shaders (other than text)': ('horizon-shaders/src', r'translate_core'),
    'pack post shaders': ('horizon-client-render/src/gfx', r'pack_post'),
    'custom fonts (bitmap, ttf) from packs': ('horizon-client-render/src', r'"bitmap"|ttf'),
    'dialogs': ('horizon-client/src/game', r'ShowDialog|show_dialog'),
}


def client_caps(query=''):
    """What the Horizon client supports that plugins' packs and packets rely on: each feature with where it is (or
    that it is missing), searched in the client source."""
    out = []
    for feature, (folder, pattern) in CAPS.items():
        if query and query.lower() not in feature.lower():
            continue
        base = CLIENT / folder
        where = None
        rx = re.compile(pattern)
        for path in sorted(base.rglob('*.rs')) if base.is_dir() else []:
            text = path.read_text(encoding='utf-8', errors='replace')
            m = rx.search(text)
            if m:
                where = f'{path.relative_to(CLIENT).as_posix()}:{text[:m.start()].count(chr(10)) + 1}'
                break
        out.append({'feature': feature, 'supported': where is not None, 'where': where})
    return {'client_source': str(CLIENT), 'features': out}


def rig_doctor():
    """The test servers before a test run: their game files, engine build, port and who is on."""
    import maker_tools
    out = []
    for name, folder in (('paper test server', HORIZON / 'paper-server' / 'server'), ('parity server', HORIZON / 'parity' / 'jport' / 'horizon' / 'server')):
        toml = folder / 'server.toml'
        row = {'server': name, 'folder': str(folder)}
        if not toml.is_file():
            row['problem'] = 'no server.toml'
            out.append(row)
            continue
        text = toml.read_text(encoding='utf-8', errors='replace')
        m = re.search(r"^\s*resources\s*=\s*'([^']*)'", text, re.M)
        res = m.group(1) if m else ''
        row['resources'] = res or '(resources/ in the folder)'
        res_path = pathlib.Path(res) if res else folder / 'resources'
        if not (res_path / 'asset-index.json').is_file() and not (res_path / 'assets').is_dir():
            versions = pathlib.Path.home() / 'AppData' / 'Roaming' / 'horizon-launcher' / 'versions'
            ok = sorted((p for p in versions.glob('*/.local/horizon-client/26.3') if (p / 'assets').is_dir()), key=lambda p: p.stat().st_mtime)
            row['problem'] = 'the prepared game files are not there: the server will not start'
            if ok:
                row['fix'] = f"set resources in {toml} to '{ok[-1]}'"
        m = re.search(r"^\s*path\s*=\s*'([^']*)'", text, re.M)
        if m and pathlib.Path(m.group(1)).is_file():
            row['engine'] = m.group(1)
        out.append(row)
    try:
        paper_server, _ = maker_tools.rig()
        on = maker_tools.players_on(paper_server)
        engine, newest = pathlib.Path(paper_server.ENGINE), pathlib.Path(paper_server.PARITY)
        rig = {'players_on': on, 'test_server_engine': str(engine)}
        if engine.is_file() and newest.is_file() and newest.stat().st_mtime > engine.stat().st_mtime + 60:
            rig['problem'] = 'the test server runs an older engine than the newest build: a DLL built with today\'s SDK may be refused ("built for mod interface N"). plugin_try new_engine=true installs the newest.'
        if on:
            rig['note'] = 'someone is on: plugin_try / deploy would restart under them'
        out.append(rig)
    except Exception as error:
        out.append({'rig': f'not readable: {error}'})
    return {'checks': out}


TOOLS = {
    'java_decode': (java_decode, "Decode an obfuscated plugin's Java (AES-encrypted strings, rotate/xor numbers, fake throw branches) into java_clean/ beside it. plugin_import and new_rewrite.py already do it; this is for Java decompiled before.", {'id': 'string'}),
    'method_body': (method_body, "A method's code by name in a plugin's own Java (decoded, libraries skipped), any overload, optionally in one class (`cls`), with the class's extends chain. `id`: a plugin-ports id or a folder.", {'id': 'string', 'name': 'string', 'cls': 'string', 'limit': 'integer'}),
    'trace_key': (trace_key, 'Every place a string is used in a plugin (a YAML key, permission, message, packet name): file, line, the method around it, and who calls that method. exact=true for the whole literal only.', {'id': 'string', 'text': 'string', 'exact': 'boolean', 'limit': 'integer'}),
    'config_schema': (config_schema, 'Every config key a plugin reads (getString/getInt/... and contains), with type, default and where; then dotted strings it reads through its own helpers.', {'id': 'string', 'limit': 'integer'}),
    'packet_map': (packet_map, 'The packets a plugin reads and sends (PacketEvents, ProtocolLib). For each read: the Bukkit event with the same input on Horizon and whether the staged engine fires it.', {'id': 'string'}),
    'client_caps': (client_caps, 'What the Horizon client supports that a plugin relies on (pack text shaders on GUI and world text, SetCamera, passengers, displays, interaction boxes, boss bars, packs, fonts, dialogs), with where in the client source.', {'query': 'string'}),
    'rig_doctor': (rig_doctor, 'Why a test server would not start or would refuse a DLL, before a test finds out: prepared game files, engine build age, who is on.', {}),
}
