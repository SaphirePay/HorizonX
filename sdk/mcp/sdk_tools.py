"""What the SDK can do, in one place: every horizon-bukkit item, every event and whether the engine fires it, and
the horizon-kit helpers. For "can a plugin do X?" without reading the source.

    sdk_can(query)      one search over functions, events (with engine fire sites) and kit helpers
    sdk_map(area)       the SDK at a glance; an area = every method of that type, or every event of a family
    sdk_fires(event)    where the engine fires an event's hook, with the code around it

Everything is cached by file times: the first call after an SDK or engine change takes a few seconds, the rest are
instant.
"""
import collections
import pathlib
import re

import maker_tools
import workspace

HORIZON = workspace.WORKSPACE
# This repository: sdk/mcp/ -> sdk/ -> the repo root. Its own SDK, kit, cookbook and engine come first; a workspace
# checkout (HORIZON_WORKSPACE) is the fallback.
SDK_DIR = pathlib.Path(__file__).resolve().parent.parent
REPO = SDK_DIR.parent


def _first(*paths):
    return next((p for p in paths if p.exists()), paths[0])


SDK_SRC = _first(SDK_DIR / 'horizon-bukkit' / 'src', HORIZON / 'paper-engine' / 'sdk' / 'horizon-bukkit' / 'src')
KIT_SRC = _first(SDK_DIR / 'horizon-kit' / 'src', HORIZON / 'plugin-tools' / 'kit' / 'horizon-kit' / 'src')
# Where the engine's event hooks are written: the family patches, the Fabric API bundle and the engine source.
ENGINE_ROOTS = [
    REPO / 'horizonsx' / 'plugin-layer',
    REPO / 'horizonsx' / 'crates',
    HORIZON / 'paper-engine' / 'family',
    HORIZON / 'fabric-port' / 'mixins',
    HORIZON / 'fabric-port' / 'tools',
    HORIZON / 'fabric-port' / 'src',
]
WORD = re.compile(r'[a-z0-9]+')

_cache = {}


def _stamp(paths):
    return max((p.stat().st_mtime for p in paths), default=0.0)


def _cached(key, paths, build):
    stamp = _stamp(paths)
    hit = _cache.get(key)
    if hit and hit[0] == stamp:
        return hit[1]
    value = build()
    _cache[key] = (stamp, value)
    return value


def _words(text):
    """`set_food_level`, `setFoodLevel`, `FoodLevelChangeEvent` -> food, level, change, event."""
    text = re.sub(r'([a-z0-9])([A-Z])', r'\1 \2', text)
    return WORD.findall(text.lower())


# ---- Events and their hooks ----

def events():
    """Every listenable event: name, Java class, hook id, file:line, and its methods."""
    files = sorted(SDK_SRC.glob('*.rs'))

    def build():
        out = {}
        for path in files:
            text = path.read_text(encoding='utf-8', errors='replace')
            for m in re.finditer(r'impl Event for (\w+) \{.*?const HOOK: &\'static str = "(\w+)"', text, re.S):
                name, hook = m.group(1), m.group(2)
                line = text.count('\n', 0, m.start()) + 1
                body = ''
                for blk in re.finditer(rf'\nimpl {name} \{{\n(.*?)\n\}}', text, re.S):
                    body += blk.group(1)
                methods = re.findall(r'\n\s*pub fn (\w+)', '\n' + body)
                owners = re.findall(r'/// bukkit~?: (\S+?)\.\w+', body)
                java = next((o for o in owners if o.endswith('/' + name)), owners[0] if owners else '')
                jv = java
                fam = jv.split('/')[-2] if jv.count('/') >= 2 else (path.stem.split('_')[-1] if path.stem.startswith('fam_ev_') else 'misc')
                out[name] = {'event': name, 'java': jv, 'family': fam, 'hook': hook, 'place': f'{path.name}:{line}',
                             'methods': methods}
        return out
    return _cached('events', files, build)


def _engine_files():
    out = []
    for root in ENGINE_ROOTS:
        if root.exists():
            out += [p for p in root.rglob('*') if p.suffix in ('.py', '.rs') and 'target' not in p.parts and '__pycache__' not in p.parts and 'tests' not in p.parts]
    return out


def fire_sites():
    """hook id -> [(file, line, enclosing fn, doc line)], from every place the engine names the hook."""
    files = _engine_files()

    def build():
        hooks = {e['hook'] for e in events().values()}
        pat = re.compile(r'"(' + '|'.join(sorted(map(re.escape, hooks), key=len, reverse=True)) + r')"')
        sites = collections.defaultdict(list)
        for path in files:
            try:
                lines = path.read_text(encoding='utf-8', errors='replace').split('\n')
            except OSError:
                continue
            for i, line in enumerate(lines):
                for m in pat.finditer(line):
                    fn, doc = '', ''
                    for j in range(i, max(i - 60, -1), -1):
                        if (f := re.match(r'\s*(?:pub(?:\([^)]*\))? )?fn (\w+)', lines[j])):
                            fn = f.group(1)
                            k = j - 1
                            docs = []
                            while k >= 0 and lines[k].strip().startswith('///'):
                                docs.insert(0, lines[k].strip()[3:].strip())
                                k -= 1
                            doc = ' '.join(docs)[:200]
                            break
                    site = (str(path), i + 1, fn, doc)
                    if not any(s[0] == site[0] and s[2] == fn for s in sites[m.group(1)]):
                        sites[m.group(1)].append(site)
        return dict(sites)
    return _cached('fires', files + sorted(SDK_SRC.glob('*.rs')), build)


def _rel(path):
    """A path shown from the repo or the workspace, never as a full local path."""
    p = pathlib.Path(path)
    for base in (REPO, HORIZON):
        try:
            return str(p.relative_to(base))
        except ValueError:
            pass
    return p.name


def _fires(hook):
    return [{'at': f'{_rel(f)}:{n}', 'fn': fn, 'doc': doc} for f, n, fn, doc in fire_sites().get(hook, [])]


def sdk_fires(event):
    """Where the engine fires an event (name, Java class or hook id), with the code around the first site."""
    q = event.strip().rsplit('/', 1)[-1].rsplit('.', 1)[-1]
    evs = events()
    hit = evs.get(q) or next((e for e in evs.values() if e['hook'] == q or e['event'].lower() == q.lower()), None)
    if not hit:
        close = [n for n in evs if q.lower() in n.lower()][:20]
        return {'error': f'no event {event}', 'close': close}
    sites = fire_sites().get(hit['hook'], [])
    shared = sorted(n for n, e in evs.items() if e['hook'] == hit['hook'] and n != hit['event'])
    out = {**hit, 'engine_fires': bool(sites), 'sites': _fires(hit['hook'])}
    if shared:
        out['shares_hook_with'] = shared
        out['warning'] = 'other events ride the same hook: cancelling this one may also cancel what they cover'
    if sites:
        f, n, _, _ = sites[0]
        lines = pathlib.Path(f).read_text(encoding='utf-8', errors='replace').split('\n')
        out['code'] = '\n'.join(f'{k + 1}: {lines[k]}' for k in range(max(0, n - 6), min(len(lines), n + 12)))
    else:
        out['note'] = 'no place in the engine names this hook: listening compiles but the event never fires'
    return out


# ---- horizon-kit ----

def kit_items():
    files = sorted(KIT_SRC.glob('*.rs'))

    def build():
        out = []
        for path in files:
            lines = path.read_text(encoding='utf-8', errors='replace').split('\n')
            for i, l in enumerate(lines):
                t = l.strip()
                if t.startswith(('pub fn ', 'pub struct ', 'pub enum ', 'pub trait ')):
                    doc, j = [], i - 1
                    while j >= 0 and lines[j].strip().startswith('///'):
                        doc.insert(0, lines[j].strip()[3:].strip())
                        j -= 1
                    out.append({'kit': f'horizon_kit::{path.stem}', 'signature': t.split('{')[0].rstrip(), 'doc': ' '.join(doc)[:200], 'place': f'{path.name}:{i + 1}'})
        return out
    return _cached('kit', files, build)


# ---- Search and map ----

# Game words players use -> the words the SDK uses for them.
SYNONYMS = {
    'hunger': ['food', 'exhaustion', 'saturation'], 'starve': ['food', 'starvation'], 'eat': ['consume', 'food'],
    'crop': ['ageable', 'age', 'grow', 'farmland'], 'crops': ['ageable', 'age', 'grow'], 'grow': ['growth', 'ageable', 'fertilize'],
    'farm': ['farmland', 'ageable', 'harvest'], 'trample': ['farmland', 'physical', 'interact'],
    'aggro': ['target'], 'angry': ['target', 'anger'], 'mob': ['entity', 'creature', 'mob'], 'hostile': ['monster', 'target'],
    'heal': ['health', 'regain', 'regen'], 'hp': ['health'], 'xp': ['experience', 'exp'], 'level': ['level', 'experience'],
    'actionbar': ['action', 'bar'], 'bossbar': ['boss', 'bar'], 'hologram': ['text', 'display'], 'npc': ['entity', 'villager', 'mannequin'],
    'gui': ['inventory', 'menu', 'view'], 'menu': ['inventory', 'view', 'gui'], 'chest': ['inventory', 'container'],
    'pdc': ['persistent', 'data'], 'nbt': ['persistent', 'component'], 'lore': ['lore'], 'rename': ['display', 'name'],
    'tp': ['teleport'], 'warp': ['teleport', 'location'], 'speed': ['speed', 'velocity', 'attribute'], 'jump': ['jump', 'velocity'],
    'fly': ['flying', 'flight', 'allow'], 'glow': ['glowing', 'glint'], 'enchanted': ['enchantment', 'glint'],
    'drop': ['drop', 'drops', 'item'], 'loot': ['loot', 'drops'], 'break': ['break', 'broken', 'destroy'], 'mine': ['break', 'damage'],
    'explode': ['explosion', 'explode', 'tnt'], 'leaves': ['leaves', 'decay'], 'fish': ['fishing', 'fish'], 'breed': ['breed', 'love'],
    'chat': ['chat', 'message'], 'message': ['message', 'send', 'chat'], 'title': ['title', 'subtitle'], 'sound': ['sound', 'play'],
    'particle': ['particle', 'spawn'], 'particles': ['particle', 'spawn'], 'model': ['model', 'custom', 'component'],
    'icon': ['model', 'item'], 'rightclick': ['interact', 'right'], 'click': ['click', 'interact'], 'sneak': ['sneak', 'toggle'],
    'death': ['death', 'die', 'killed'], 'respawn': ['respawn'], 'fall': ['fall', 'distance'], 'water': ['water', 'fluid', 'bucket'],
    'lava': ['lava', 'fluid', 'fire'], 'fire': ['fire', 'combust', 'burn'], 'night': ['time', 'night'], 'day': ['time', 'day'],
}


def _score(query_words, text_words):
    """Whole-word hits count most, word starts next; every query word (or a synonym of it) must hit somehow."""
    total = hits = 0
    for q in query_words:
        best = 0
        for i, alt in enumerate([q, *SYNONYMS.get(q, [])]):
            weight = 0 if i == 0 else 1
            if alt in text_words:
                best = max(best, 3 - weight)
            elif any(w.startswith(alt) for w in text_words):
                best = max(best, 2 - weight)
            elif len(alt) > 3 and any(alt in w for w in text_words):
                best = max(best, 1)
        if best:
            hits += 1
            total += best
    # Most words hit first; at least half of them must.
    if hits == 0 or hits * 2 < len(query_words):
        return 0
    return hits * 10 + total


def sdk_can(query, limit=30):
    """Can a plugin do X? Searches SDK functions, events (with whether the engine fires them) and kit helpers by
    words, best first: `hunger`, `enderman target`, `crop age`, `item model`, `action bar`."""
    qw = _words(query)
    if not qw:
        return {'error': 'give some words'}
    sites = fire_sites()
    ev_hits = []
    for e in events().values():
        s = _score(qw, set(_words(f"{e['event']} {e['java']} {' '.join(e['methods'])}"))) + _score(qw, set(_words(e['event'])))
        if s:
            ev_hits.append((s, {'event': e['event'], 'hook': e['hook'], 'engine_fires': bool(sites.get(e['hook'])), 'methods': e['methods'][:12], 'place': e['place']}))
    fn_hits = []
    for item in maker_tools.sdk_items():
        s = _score(qw, set(_words(' '.join([item['rust'], *item['java']])))) + _score(qw, set(_words(item['rust'])))
        if s:
            row = {'rust': item['rust'], 'signature': item['signature'], 'place': item['place']}
            if item.get('partial') or item.get('note'):
                row['not_exact'] = item.get('note') or 'partial'
            fn_hits.append((s, row))
    kit_hits = []
    for k in kit_items():
        s = _score(qw, set(_words(f"{k['kit']} {k['signature']} {k['doc']}")))
        if s:
            kit_hits.append((s, k))
    best = lambda rows: [r for _, r in sorted(rows, key=lambda x: -x[0])]
    ev, fn, kit = best(ev_hits), best(fn_hits), best(kit_hits)
    for row in fn[:limit]:
        row['use'] = use_path(row['rust'], row['place'])
    return {
        'events': ev[:max(5, limit // 3)],
        'functions': fn[:limit],
        'kit': kit[:max(5, limit // 3)],
        'counts': {'events': len(ev), 'functions': len(fn), 'kit': len(kit)},
        'recipes': _recipe_rank(qw)[:3],
        'next': 'sdk_recipe <task> = working code; sdk_doc <name> = one item in full; sdk_usage <name> = real uses; sdk_fires <Event> = where the engine fires it',
    }


def sdk_map(area=''):
    """The SDK at a glance. No area: every type with its method count, events by family with how many the engine
    fires, and the kit modules. An area: a type (`Player`, `ItemStack`, `World`, `Block`...) = all its methods; an
    event family (`entity`, `player`, `block`, `inventory`...) = its events and whether each fires; a kit module
    (`kit:gui`) = its helpers."""
    items = maker_tools.sdk_items()
    sites = fire_sites()
    evs = events()
    if not area:
        owners = collections.Counter(i['rust'].split('::')[0] for i in items if '::' in i['rust'])
        fams = collections.defaultdict(lambda: [0, 0])
        for e in evs.values():
            fam = e['family']
            fams[fam][0] += 1
            fams[fam][1] += bool(sites.get(e['hook']))
        kits = collections.Counter(k['kit'] for k in kit_items())
        return {
            'functions': len(items),
            'types': dict(owners.most_common(80)),
            'events': {'total': len(evs), 'engine_fires': sum(bool(sites.get(e['hook'])) for e in evs.values()),
                       'by_family': {f: f'{n[1]}/{n[0]} fire' for f, n in sorted(fams.items())}},
            'kit_modules': dict(sorted(kits.items())),
            'never_fire': sorted(n for n, e in evs.items() if not sites.get(e['hook'])),
        }
    a = area.strip()
    if a.lower().startswith('kit:'):
        mod = 'horizon_kit::' + a[4:].strip().lower()
        return [f"{k['signature']}{'  // ' + k['doc'] if k['doc'] else ''}" for k in kit_items() if k['kit'] == mod] or {'error': f'no kit module {a[4:]}'}
    rows = [i for i in items if i['rust'].split('::')[0] == a]
    if rows:
        return {'type': a, 'methods': len(rows), 'list': [f"{r['signature']}   [{r['place']}]{'  ~' + (r.get('note') or 'partial') if r.get('partial') else ''}" for r in rows]}
    fam = [e for e in evs.values() if e['family'] == a.lower()]
    if fam:
        return {'family': a, 'events': [f"{e['event']}  {'fires' if sites.get(e['hook']) else 'NEVER FIRES'}  hook {e['hook']}" for e in sorted(fam, key=lambda e: e['event'])]}
    close = sorted({i['rust'].split('::')[0] for i in items if '::' in i['rust'] and a.lower() in i['rust'].split('::')[0].lower()})[:30]
    return {'error': f'no type, event family or kit module {a}', 'close': close}


TOOLS = {
    'sdk_can': (sdk_can, 'Can a plugin do X? One search over every horizon-bukkit function, every event (with whether the engine really fires it) and the horizon-kit helpers, best match first. Use before reading SDK source: `hunger`, `enderman target`, `crop age`, `action bar`.', {'query': 'string', 'limit': 'integer'}),
    'sdk_map': (sdk_map, 'The whole SDK at a glance (types and method counts, events by family and how many fire, kit modules, events that never fire). With an area: a type (Player, ItemStack, World...) = all its methods; an event family (entity, player, block...) = its events and whether each fires; kit:<module> = its helpers.', {'area': 'string'}),
    'sdk_fires': (sdk_fires, 'Where the engine fires an event (name, Java class or hook id): every site with its function and doc, the code around the first, and other events sharing the same hook. Says plainly when nothing fires it.', {'event': 'string'}),
}


# ---- Paths, docs, real uses and recipes ----

PORTS = HORIZON / 'plugin-ports'
COOKBOOK = _first(SDK_DIR / 'cookbook' / 'src' / 'lib.rs', HORIZON / 'plugin-tools' / 'kit' / 'cookbook' / 'src' / 'lib.rs')


def _pub_mods():
    def build():
        lib = (SDK_SRC / 'lib.rs').read_text(encoding='utf-8', errors='replace')
        return set(re.findall(r'^pub mod (\w+);', lib, re.M)), lib
    return _cached('mods', [SDK_SRC / 'lib.rs'], build)


def use_path(rust, place):
    """`Player::get_location` -> horizon_bukkit::Player; `action_is_right_click` in core2.rs ->
    horizon_bukkit::core2::action_is_right_click."""
    mods, lib = _pub_mods()
    stem = place.split('.rs')[0]
    head = rust.split('::')[0]
    types = _cached('types', sorted(SDK_SRC.glob('*.rs')), lambda: {i['rust']: i['place'].split('.rs')[0] for i in maker_tools.sdk_items() if i['kind'] in ('struct', 'enum', 'trait', 'type')})
    if '::' in rust:
        stem = types.get(head, stem)
    if stem == 'lib' or re.search(rf'pub use {stem}::(?:\*|\{{[^}}]*\b{head}\b|{head}\b)', lib):
        return f'horizon_bukkit::{head}'
    if stem in mods:
        return f'horizon_bukkit::{stem}::{head}'
    return f'horizon_bukkit (private module {stem}: not reachable from a plugin)'


def _port_files():
    """Our Rust plugins' sources (hand-written `_rs` crates first), the SDK examples and horizon-kit."""
    out = sorted((SDK_DIR / 'examples').glob('*/src/*.rs'))
    try:
        crates = [p for p in PORTS.iterdir() if p.is_dir() and (p / 'src').is_dir()]
    except OSError:
        crates = []
    # Machine-converted crates (_sw, _why7, _rb...: over 1 GB) are skipped: hand-written code is the useful example.
    machine = re.compile(r'_(sw|rb|why\d*|auto|dep|t|shk|slow)$')
    for c in sorted(crates, key=lambda c: (not c.name.endswith('_rs'), c.name)):
        if not machine.search(c.name):
            out += sorted((c / 'src').glob('*.rs'))
    return sorted(KIT_SRC.glob('*.rs')) + out


def _texts():
    files = _port_files()

    def build():
        texts = []
        for f in files:
            try:
                texts.append((f, f.read_text(encoding='utf-8', errors='replace').split('\n')))
            except OSError:
                pass
        return texts
    return _cached('texts', files, build)


def sdk_usage(name, limit=8):
    """Real uses of an SDK or kit name in our own plugins (hand-written ones first), each with the lines around it:
    `send_action_bar`, `EntityTargetEvent`, `set_item_at`, `window::menu`."""
    key = name.strip().split('::')[-1]
    pat = re.compile(rf'\b{re.escape(key)}\b')
    seen, hits, total = set(), [], 0
    for f, lines in _texts():
        crate = f.parent.parent.name
        for i, line in enumerate(lines):
            if pat.search(line) and not line.lstrip().startswith(('//', 'use ', 'pub use')) and f'fn {key}' not in line:
                total += 1
                if len(hits) < limit and crate not in seen:
                    seen.add(crate)
                    hits.append({'at': f'{_rel(f)}:{i + 1}', 'code': '\n'.join(lines[max(0, i - 3):i + 5])})
    return {'name': name, 'uses': total, 'examples': hits, 'note': 'one example per crate; horizon-kit and hand-written `_rs` plugins first'}


def sdk_doc(name):
    """One SDK item in full: where it is, the `use` path, doc and Java name, its whole definition, and how many of
    our plugins already use it (with one example)."""
    q = name.strip()
    short = q.split('::')[-1]
    items = maker_tools.sdk_items()
    rows = [i for i in items if i['rust'] == q or i['rust'].endswith('::' + q)]
    if not rows:
        rows = [i for i in items if i['rust'].split('::')[-1].lower() == short.lower()]
    if not rows:
        return {'error': f'nothing named {name}', 'try': 'sdk_can with words'}
    out = []
    for r in rows[:4]:
        fname, line = r['place'].rsplit(':', 1)
        lines = (SDK_SRC / fname).read_text(encoding='utf-8', errors='replace').split('\n')
        n = int(line) - 1
        start = n
        while start > 0 and lines[start - 1].strip().startswith(('///', '#[')):
            start -= 1
        depth, end = 0, n
        for end in range(n, min(len(lines), n + 40)):
            depth += lines[end].count('{') - lines[end].count('}')
            if depth <= 0 and ('}' in lines[end] or lines[end].rstrip().endswith(';')):
                break
        out.append({'rust': r['rust'], 'use': use_path(r['rust'], r['place']), 'place': r['place'], 'java': r.get('java'),
                    'not_exact': (r.get('note') or 'partial') if r.get('partial') else None, 'source': '\n'.join(lines[start:end + 1])})
    used = sdk_usage(short, 1)
    return {'found': out, 'used_in_our_plugins': used['uses'], 'example': used['examples'][:1]}


def recipes():
    def build():
        lines = COOKBOOK.read_text(encoding='utf-8').split('\n')
        heads = [(i, m) for i, m in ((i, re.match(r'/// recipe: (\w+) \| ([^|]+)\| (.*)', l)) for i, l in enumerate(lines)) if m]
        out = {}
        for k, (i, m) in enumerate(heads):
            stop = heads[k + 1][0] if k + 1 < len(heads) else len(lines)
            out[m.group(1)] = {'title': m.group(2).strip(), 'words': m.group(3).split(), 'code': '\n'.join(lines[i + 1:stop]).strip()}
        imports = '\n'.join(l for l in lines if l.startswith('use '))
        return out, imports
    return _cached('recipes', [COOKBOOK], build)


def _recipe_rank(qw):
    rs, _ = recipes()
    ranked = sorted(((_score(qw, set(v['words']) | set(_words(k + ' ' + v['title']))), k) for k, v in rs.items()), reverse=True)
    return [k for s, k in ranked if s]


def sdk_recipe(task=''):
    """Working code for a common plugin job, compiled against the live SDK (plugin-tools/kit/cookbook). No task:
    the list. A task in words (`right click item`, `hunger`, `extra drops`, `menu`) or a key: the code and imports."""
    rs, imports = recipes()
    if not task.strip():
        return {k: v['title'] for k, v in rs.items()}
    key = task.strip()
    if key not in rs:
        ranked = _recipe_rank(_words(task))
        if not ranked:
            return {'none_for': task, 'recipes': {k: v['title'] for k, v in rs.items()}, 'try': 'sdk_can'}
        key = ranked[0]
        also = ranked[1:4]
    else:
        also = []
    return {'recipe': key, 'title': rs[key]['title'], 'imports': imports, 'code': rs[key]['code'], 'also': also}


TOOLS.update({
    'sdk_recipe': (sdk_recipe, 'Working, compiler-checked code for a common plugin job (skeleton, command, chat, action bar, sound, particles, custom item with pack model/glint/tag, item tags, inventory slots, right-click item, timers, config, menu, blocks near, crop age, extra drops, hunger, mob target, fake blocks). No task = the list.', {'task': 'string'}),
    'sdk_usage': (sdk_usage, 'Real uses of an SDK or kit name in our own plugins, hand-written ones first, with the code around each: the fastest way to see how a call is really used.', {'name': 'string', 'limit': 'integer'}),
    'sdk_doc': (sdk_doc, 'One SDK item in full: the `use` path, doc, Java name, whole definition, what is not exact, and how many of our plugins use it.', {'name': 'string'}),
})
