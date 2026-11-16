"""Writes src/generated.rs: the Java layer's view of the whole SDK.

* `native_call` / `native_static` / `native_new` ...: every SDK function by its Bukkit name, made by jport's own
  binding writer (bindings.py) with every name wanted instead of one plugin's. Java calls land here.
* `java_kind`: the Bukkit class each SDK type stands for (from the `/// bukkit:` tags), so Java can give the
  object the right interface (`instanceof Player`).
* `register_event`: one SDK listener per Bukkit event class, which hands the event to Java.

Run with jport's python: `python gen.py [--check]`.
--check also runs the compile repair (a line the compiler refuses is dropped, as jport does).
"""
import json
import pathlib
import re
import sys

JPORT = pathlib.Path(r'D:\Horizon\plugin-tools\jport')
sys.path.insert(0, str(JPORT))
import bindings  # noqa: E402

HERE = pathlib.Path(__file__).resolve().parent

# `&[Option<ItemStack>]` (Inventory.getContents): each slot as `Option<&ItemStack>`, which bindings.py can write.
_to_v = bindings.to_v


def _to_v_slots(kind, value):
    plain = re.sub(r"'\w+ ", '', kind.strip())
    found = re.fullmatch(r'&(?:mut )?Option<(.+)>', plain)
    if found:
        return _to_v(f'Option<&{found.group(1)}>', value)
    return _to_v(kind, value)


bindings.to_v = _to_v_slots


def _to_v_more(kind, value):
    out = _to_v_slots(kind, value)
    if out is not None:
        return out
    plain = re.sub(r"'\w+ ", '', kind.strip())
    if plain in ('std::net::SocketAddr', 'std::net::IpAddr', 'SocketAddr', 'IpAddr'):
        return f'jrt::s(&{value}.to_string())'
    # A set of names (`BTreeSet<String>`): Java's Set.
    found = re.fullmatch(r'&?(?:std::collections::)?(?:BTreeSet|HashSet)<(.+)>', plain)
    if found:
        inner = bindings.to_v(found.group(1), 'x_')
        if inner:
            return f'jrt::list({value}.iter().map(|x_| {{ let x_ = x_.clone(); {inner} }}).collect())'
    # Pairs (`Vec<(String, AttributeModifier)>`): a Map of them, as Bukkit's Multimap-like answers read.
    found = re.fullmatch(r'Vec<\((.+?), *(.+)\)>', plain)
    if found:
        key, item = bindings.to_v(found.group(1), 'k_'), bindings.to_v(found.group(2), 'v_')
        if key and item:
            return f'jrt::map_of({value}.into_iter().map(|(k_, v_)| ({key}, {item})).collect())'
    found = re.fullmatch(r'&(?:std::collections::)?(?:BTreeMap|HashMap)<(.+?), *(.+)>', plain)
    if found:
        key, item = bindings.to_v(found.group(1), 'k_'), bindings.to_v(found.group(2), 'v_')
        if key and item:
            return f'jrt::map_of({value}.iter().map(|(k_, v_)| {{ let (k_, v_) = (k_.clone(), v_.clone()); ({key}, {item}) }}).collect())'
    # A generator, provider or populator: the Java object behind it (null for the game's own).
    if re.fullmatch(r'(?:Option<)?Arc<dyn (?:crate::worlds::)?(?:ChunkGenerator|BiomeProvider|BlockPopulator)(?: *\+ *(?:Send|Sync))*>>?', plain):
        if plain.startswith('Option<'):
            return f'{value}.as_ref().map_or(V::Null, |x_| crate::adapters::java_of(x_))'
        return f'crate::adapters::java_of(&{value})'
    if re.fullmatch(r'Vec<Arc<dyn (?:crate::worlds::)?BlockPopulator(?: *\+ *(?:Send|Sync))*>>', plain):
        return f'jrt::list({value}.iter().map(|x_| crate::adapters::java_of(x_)).filter(|v_| !v_.is_null()).collect())'
    # Enchanting table offers (`[Option<(String, i32, i32)>; 3]`): each as [enchantment, level, cost] or null.
    if plain == '[Option<(String, i32, i32)>; 3]':
        return f'jrt::list({value}.iter().map(|o_| match o_ {{ Some((k_, l_, c_)) => jrt::list(vec![jrt::s(k_), V::Int(*l_), V::Int(*c_)]), None => V::Null }}).collect())'
    return None


bindings.to_v = _to_v_more


def _return_of(signature):
    """The return type after the parameter list's own `)`: bindings.return_of took the first `) ->`, which in
    `op: impl Fn(i32) -> i32` is the closure's, and so refused every function that takes such a callback."""
    start = re.search(r'fn \w+(?:<[^(]*>)?\(', signature)
    if not start:
        return '()'
    depth, i = 1, start.end()
    while i < len(signature) and depth:
        c = signature[i]
        if c in '([':
            depth += 1
        elif c in ')]':
            depth -= 1
        i += 1
    found = re.match(r'\s*->\s*(.+?)\s*(?:\{|where\b|$)', signature[i:])
    return found.group(1).strip() if found else '()'


bindings.return_of = _return_of

_from_v = bindings.from_v

CLOSURE = re.compile(r"impl (FnMut|FnOnce|Fn)\((.*?)\)(?: *-> *(\w+))?((?: *\+ *(?:Send|Sync|'static))*)")


_NEWTYPES = None


def _entity_newtypes():
    """The SDK's entity kinds that wrap an `Entity` (`pub struct Mob(pub Entity)`), by name -> module."""
    global _NEWTYPES
    if _NEWTYPES is None:
        _NEWTYPES = {}
        src = pathlib.Path(r'D:\Horizon\paper-engine\sdk\horizon-bukkit\src')
        for f in ('entities.rs', 'fam_entity.rs'):
            text = (src / f).read_text(encoding='utf-8')
            for name in re.findall(r'pub struct (\w+)\(pub Entity\);', text):
                _NEWTYPES[name] = f[:-3]
    return _NEWTYPES


def _from_v_more(kind, value, index, types, near):
    p = f'p{index}'
    raw_kind = kind.strip()
    # An entity kind (`&Mob`): Java's entity handles are the plain `Entity`, which the kind wraps.
    found = re.fullmatch(r'&(?:crate::(?:entities|fam_entity)::)?(\w+)', raw_kind)
    if found and found.group(1) in _entity_newtypes():
        name = found.group(1)
        path = f'horizon_bukkit::{_entity_newtypes()[name]}::{name}'
        setup = (f'let {p} = match jbridge::held::<{path}>(&{value}) {{ Some(x_) => x_.clone(), '
                 f'None => {path}(jbridge::held::<horizon_bukkit::entity::Entity>(&{value})?.clone()) }};')
        return setup, f'&{p}', f'({value}.native::<Mutex<{path}>>().is_some() || {value}.native::<Mutex<horizon_bukkit::entity::Entity>>().is_some())'
    # A command's sender (`&CommandSender`): Java hands a player or another entity as itself, which the SDK's
    # sender wraps (Bukkit.dispatchCommand(player, line), a sender's permission check).
    if re.fullmatch(r'&?(?:crate::)?CommandSender', raw_kind):
        sender = 'horizon_bukkit::CommandSender'
        setup = (f'let {p} = match jbridge::held::<{sender}>(&{value}) {{ Some(x_) => x_.clone(), None => match jbridge::held::<horizon_bukkit::Player>(&{value}) {{ '
                 f'Some(x_) => {sender}::Player(x_.clone()), None => {sender}::Entity(jbridge::held::<horizon_bukkit::entity::Entity>(&{value})?.clone()) }} }};')
        check = (f'({value}.native::<Mutex<{sender}>>().is_some() || {value}.native::<Mutex<horizon_bukkit::Player>>().is_some() '
                 f'|| {value}.native::<Mutex<horizon_bukkit::entity::Entity>>().is_some())')
        return setup, (f'&{p}' if raw_kind.startswith('&') else p), check
    # Text kept for good (`&'static str`): bindings.py drops lifetimes and then borrows a local.
    if raw_kind == "&'static str":
        return f'let {p} = static_text(&jbridge::text_arg(&{value}));', p, f'jbridge::text_like(&{value})'
    # A callback filling a JSON map (a damage source's context): Java passes the map already filled.
    if re.fullmatch(r'impl FnOnce\(&mut (?:std::collections::)?BTreeMap<String, *(?:serde_json::)?Value>\)', raw_kind):
        return (f'let x_ = jrt::javaio::to_json(&{value}); let {p} = move |m_: &mut std::collections::BTreeMap<String, serde_json::Value>| '
                f'{{ if let serde_json::Value::Object(o_) = x_ {{ m_.extend(o_); }} }};'), p, f'matches!({value}, V::Map(_))'
    # A plugin's world generator, biome provider or populator object (WorldCreator.generator, getPopulators().add).
    found = re.fullmatch(r'Arc<dyn (?:crate::worlds::)?(ChunkGenerator|BiomeProvider|BlockPopulator)(?: *\+ *(?:Send|Sync))*>', raw_kind)
    if found:
        adapter = {'ChunkGenerator': 'JavaChunkGenerator', 'BiomeProvider': 'JavaBiomeProvider', 'BlockPopulator': 'JavaBlockPopulator'}[found.group(1)]
        return f'let {p} = crate::adapters::{adapter}::new({value}.clone());', p, f'{value}.native::<crate::conv::JavaRef>().is_some()'
    # A structure's block or entity transformer (AsyncStructureGenerateEvent.setBlockTransformer).
    found = re.fullmatch(r"impl (?:crate::fam_core::)?(BlockTransformer|EntityTransformer)(?: *\+ *(?:Send|Sync|'static))*", raw_kind)
    if found:
        return f'let {p} = crate::adapters::Java{found.group(1)}::new({value}.clone());', p, f'{value}.native::<crate::conv::JavaRef>().is_some()'
    # A map of JSON values by name (a saved ItemStack's map, ConfigurationSerializable's form).
    if re.fullmatch(r'&(?:std::collections::)?(?:BTreeMap|HashMap)<String, *(?:serde_json::)?Value>', raw_kind):
        kind_ = 'HashMap' if 'HashMap' in raw_kind else 'BTreeMap'
        setup = (f'let {p}: std::collections::{kind_}<String, serde_json::Value> = match jrt::javaio::to_json(&{value}) '
                 f'{{ serde_json::Value::Object(o_) => o_.into_iter().collect(), _ => Default::default() }};')
        return setup, f'&{p}', f'matches!({value}, V::Map(_))'
    # JSON that may be left out (`Option<&Value>`: a particle's options, a block entity's data).
    if raw_kind in ('Option<&Value>', 'Option<&serde_json::Value>'):
        return f'let {p} = (!{value}.is_null()).then(|| jrt::javaio::to_json(&{value}));', f'{p}.as_ref()', 'true'
    out = _from_v(kind, value, index, types, near)
    if out is not None:
        return out
    plain = re.sub(r"'\w+ ", '', kind.strip())
    # JSON-like values (`impl Into<Value>`: a config value, a data component's field): Java's value as JSON.
    if plain in ('impl Into<Value>', 'impl Into<serde_json::Value>'):
        return '', f'jrt::javaio::to_json(&{value})', 'true'
    if plain in ('Option<Value>', 'Option<serde_json::Value>'):
        return '', f'(!{value}.is_null()).then(|| jrt::javaio::to_json(&{value}))', 'true'
    if plain in ('&serde_json::Map<String, Value>', '&Map<String, Value>', '&mut Map<String, Value>', '&mut serde_json::Map<String, Value>'):
        mutable = 'mut ' if '&mut' in plain else ''
        return f'let {mutable}{p} = match jrt::javaio::to_json(&{value}) {{ serde_json::Value::Object(m_) => m_, _ => serde_json::Map::new() }};', f'&{mutable}{p}', f'matches!({value}, V::Map(_))'
    # A map of names to numbers (`&BTreeMap<String, i32>`): from a Java Map.
    found = re.fullmatch(r'&(?:std::collections::)?(BTreeMap|HashMap)<String, *(i32|u32|i64|u64|f64|f32|u8|i16|bool)>', plain)
    if found:
        conv = 'x_.as_bool().unwrap_or(false)' if found.group(2) == 'bool' else (f'x_.as_f64().unwrap_or(0.0) as {found.group(2)}' if found.group(2).startswith('f') else f'x_.as_i64().unwrap_or(0) as {found.group(2)}')
        return f'let {p}: std::collections::{found.group(1)}<String, {found.group(2)}> = jrt::javaio::to_json(&{value}).as_object().map(|m_| m_.iter().map(|(k_, x_)| (k_.clone(), {conv})).collect()).unwrap_or_default();', f'&{p}', f'matches!({value}, V::Map(_))'
    # A list of the game's objects by reference (`&[&Entity]`).
    found = re.fullmatch(r'&\[&((?:\w+::)*[A-Z]\w*)\]', plain)
    if found:
        path = types.path(found.group(1), near)
        return f'let {p}v: Vec<{path}> = {value}.items().ok()?.iter().map(|x_| jbridge::held::<{path}>(x_).map(|h_| h_.clone())).collect::<Option<Vec<_>>>()?; let {p}: Vec<&{path}> = {p}v.iter().collect();', f'&{p}', f'!{value}.is_null()'
    # The game's object to change in place (`&mut Inventory`).
    found = re.fullmatch(r'&mut ((?:crate::|\w+::)*[A-Z]\w*)', plain)
    if found and found.group(1).rsplit('::', 1)[-1] not in ('Map', 'String', 'Value', 'JavaRandom'):
        path = types.path(found.group(1).replace('crate::', ''), near)
        return f'let mut {p} = jbridge::held::<{path}>(&{value})?;', f'&mut *{p}', f'{value}.native::<Mutex<{path}>>().is_some()'
    # A callback: a Java object (a lambda, a Runnable, a Consumer...). Its one abstract method runs ("*").
    found = CLOSURE.fullmatch(plain)
    if found:
        params = [x.strip() for x in found.group(2).split(',') if x.strip()]
        names = [f'c{i}_' for i in range(len(params))]
        args = []
        for name, param in zip(names, params):
            conv = bindings.to_v(param if param.startswith('&') else param, name)
            if conv is None:
                return None
            args.append(conv)
        ret = found.group(3)
        call = f'jrt::call(&{p}, "*", &[{", ".join(args)}])'
        if ret is None:
            body = f'{{ let _ = jrt::report("callback", {call}); }}'
        elif ret == 'bool':
            body = f'{call}.map(|v_| v_.truth()).unwrap_or(false)'
        elif ret in ('i32', 'i64', 'u32', 'u64', 'i16', 'u8', 'usize'):
            body = f'{call}.map(|v_| v_.long() as {ret}).unwrap_or(0)'
        elif ret in ('f64', 'f32'):
            body = f'{call}.map(|v_| v_.double() as {ret}).unwrap_or(0.0)'
        elif ret == 'String':
            body = f'{call}.map(|v_| v_.text()).unwrap_or_default()'
        else:
            return None
        closure_params = ', '.join(names)
        return f'let {p} = {value}.clone();', f'move |{closure_params}| {body}', f'matches!({value}, V::Native(_) | V::Obj(_) | V::Func(_))'
    # A task the server runs once (a scheduler's Runnable/Consumer): `Option<Box<dyn FnOnce() + Send>>`.
    found = re.fullmatch(r"(Option<)?Box<dyn (?:FnOnce|FnMut|Fn)\(\)(?: *\+ *(?:Send|Sync|'static))*>(>)?", plain)
    if found:
        boxed = f'Box::new(move || {{ let _ = jrt::report("task", jrt::call(&{p}, "*", &[])); }})'
        if found.group(1):
            return f'let {p} = {value}.clone();', f'(!{p}.is_null()).then(|| {boxed} as Box<dyn FnOnce() + Send>)', f'({value}.is_null() || matches!({value}, V::Native(_) | V::Obj(_) | V::Func(_)))'
        return f'let {p} = {value}.clone();', boxed, f'matches!({value}, V::Native(_) | V::Obj(_) | V::Func(_))'
    # Fixed arrays of numbers (`[f64; 3]` a vector, `&[f32; 16]` a matrix): from a Java array or list.
    found = re.fullmatch(r'(&)?\[(f64|f32|i32|i64|u8|i16|u32)\s*;\s*(\d+)\]', plain)
    if found:
        t, size = found.group(2), int(found.group(3))
        conv = f'x_[i_].double() as {t}' if t.startswith('f') else f'x_[i_].long() as {t}'
        setup = f'let x_ = {value}.items().ok()?; if x_.len() < {size} {{ return None; }} let {p}: [{t}; {size}] = std::array::from_fn(|i_| {conv});'
        return setup, (f'&{p}' if found.group(1) else p), f'{value}.items().map(|x_| x_.len() == {size}).unwrap_or(false)'
    # Enchanting table offers from a list of [enchantment, level, cost] or null.
    if plain == '&[Option<(String, i32, i32)>; 3]':
        setup = (f'let x_ = {value}.items().ok()?; if x_.len() < 3 {{ return None; }} let {p}: [Option<(String, i32, i32)>; 3] = std::array::from_fn(|i_| '
                 f'x_[i_].items().ok().and_then(|t_| Some((t_.first()?.text(), t_.get(1)?.long() as i32, t_.get(2)?.long() as i32))));')
        return setup, f'&{p}', f'{value}.items().is_ok()'
    # Name, value and signature triples (a profile's properties): from a list of [name, value, signature].
    if plain in ('&[(String, String, Option<String>)]', 'Vec<(String, String, Option<String>)>'):
        setup = (f'let {p}: Vec<(String, String, Option<String>)> = {value}.items().ok()?.iter().map(|t_| {{ let x_ = t_.items().ok()?; '
                 f'Some((x_.first()?.text(), x_.get(1)?.text(), x_.get(2).filter(|v_| !v_.is_null()).map(V::text))) }}).collect::<Option<Vec<_>>>()?;')
        return setup, (p if plain.startswith('Vec') else f'&{p}'), f'{value}.items().is_ok()'
    # Pairs of a key and text (advancements by key): from a list of [key, text].
    if plain == '&[(NamespacedKey, String)]':
        setup = (f'let {p}: Vec<(horizon_bukkit::persistence::NamespacedKey, String)> = {value}.items().ok()?.iter().map(|t_| {{ let x_ = t_.items().ok()?; '
                 f'Some((horizon_bukkit::persistence::NamespacedKey::from_string(&x_.first()?.text())?, x_.get(1)?.text())) }}).collect::<Option<Vec<_>>>()?;')
        return setup, f'&{p}', f'{value}.items().is_ok()'
    # A text and a link (server links): from a list of [component, url].
    if plain == '&[(Component, String)]':
        setup = (f'let {p}: Vec<(horizon_bukkit::adventure::Component, String)> = {value}.items().ok()?.iter().map(|t_| {{ let x_ = t_.items().ok()?; '
                 f'Some((jbridge::component::component_of(x_.first()?)?, x_.get(1)?.text())) }}).collect::<Option<Vec<_>>>()?;')
        return setup, f'&{p}', f'{value}.items().is_ok()'
    # Names to names (report details): from a Java Map.
    if plain in ('&BTreeMap<String, String>', '&std::collections::BTreeMap<String, String>', '&HashMap<String, String>'):
        kind_ = 'HashMap' if 'HashMap' in plain else 'BTreeMap'
        setup = f'let {p}: std::collections::{kind_}<String, String> = jrt::javaio::to_json(&{value}).as_object().map(|m_| m_.iter().map(|(k_, x_)| (k_.clone(), x_.as_str().map(str::to_owned).unwrap_or_else(|| x_.to_string()))).collect()).unwrap_or_default();'
        return setup, f'&{p}', f'matches!({value}, V::Map(_))'
    # A tamer: a player or an offline player, or nobody.
    # A Java object implementing an SDK trait (a mob Goal): an adapter that calls it.
    java_ref = f'{value}.native::<crate::conv::JavaRef>().is_some()'
    if plain in ('Box<dyn Goal>', 'Box<dyn crate::fam_entity::Goal>'):
        return f'let {p}: Box<dyn horizon_bukkit::fam_entity::Goal> = Box::new(crate::adapters::JavaGoal::new({value}.clone()));', p, java_ref
    if plain in ('Option<&dyn AnimalTamer>', 'Option<&dyn crate::fam_entity::AnimalTamer>'):
        return f'let {p} = jbridge::offline_of(&{value});', f'{p}.as_ref().map(|x_| x_ as &dyn horizon_bukkit::fam_entity::AnimalTamer)', f'({value}.is_null() || jbridge::offline_of(&{value}).is_some())'
    return None


bindings.from_v = _from_v_more
OUT = HERE / 'src' / 'generated.rs'


class Member:
    def __init__(self, event):
        self.params = [('event', event)]
        self.name = 'on' + event
        self.static = False
        self.node = None


class Listeners:
    """A stand-in for a plugin class with a listener for every event the SDK can fire."""
    kind = 'class'
    java_name = 'HorizonJavaLayer'
    fqn = 'HorizonJavaLayer'
    name = 'HorizonJavaLayer'
    static_fields = {}
    methods = []

    def __init__(self, events):
        self.listeners = [Member(e) for e in events]

    def chain(self, classes):
        return [self]


class Everything:
    """bindings.generate's view of a plugin that calls every SDK function."""

    def __init__(self, events):
        known = bindings.known_tables()
        self.used_methods = set(known['methods'])
        self.used_statics = set(known['statics'])
        self.used_new = set(known['made'])
        self.classes = {'HorizonJavaLayer': Listeners(events)}
        self.used_fields = set()
        self.used_classes = set()
        self.files = []
        self.dynamic_events = set()

    def own(self, name):
        return False


def main():
    types = bindings.Types()
    _, events = bindings.implementors()
    mapping = json.loads(bindings.MAPPING.read_text(encoding='utf-8'))['events']
    # Every concrete Bukkit/Paper event class (api_kinds.txt, from java/gen/Kinds.java) and the SDK event it fires
    # from: the mapping's listener type (FurnaceSmeltEvent is the SDK's BlockCookEvent), else the same simple name.
    # Two Java classes may share a simple name (Paper's and Bukkit's TNTPrimeEvent): each gets its own arm.
    api = [line.split() for line in (HERE / 'api_kinds.txt').read_text(encoding='utf-8').splitlines() if line.strip()]
    java_events = [n.replace('/', '.') for t, e, n in api if t == 'C' and e in ('EH', 'E-')]

    def sdk_event(java):
        row = mapping.get(java.replace('.', '/'))
        found = re.search(r'register_event::<(\w+)>', row.get('listener', '')) if row else None
        if found and found.group(1) in events:
            return found.group(1)
        simple = java.rsplit('.', 1)[-1].split('$')[-1]
        return simple if simple in events else None

    pairs = [(java, sdk_event(java)) for java in java_events]
    pairs = [(java, sdk) for java, sdk in pairs if sdk]
    full = {}
    for java, sdk in pairs:
        if java.rsplit('.', 1)[-1] == sdk:
            full.setdefault(sdk, java)
    for java, sdk in pairs:
        full.setdefault(sdk, java)
    registerable = sorted({sdk for _, sdk in pairs})
    out = bindings.generate(Everything(registerable))
    code = out['code']
    all_events = out['events']

    # Each SDK type to the Bukkit class it stands for. Its own tags name classes (`org/bukkit/entity/Entity`); the core
    # types (Player, World, Block...) have none, so the classes its methods' tags name count, the one of its own
    # name first (Player's methods are `org/bukkit/entity/Player.getName`...).
    import collections
    by_owner = collections.defaultdict(collections.Counter)
    for item in bindings.items():
        if item['kind'] == 'fn' and '::' in item['rust']:
            owner = item['rust'].split('::')[0]
            for java in item['java']:
                if '.' in java.rsplit('/', 1)[-1]:
                    by_owner[owner][java.rsplit('.', 1)[0]] += 1
    kinds = []
    seen = set()
    for item in bindings.items():
        if item['kind'] not in ('struct', 'enum') or '::' in item['rust']:
            continue
        classes = [j for j in item['java'] if '.' not in j.rsplit('/', 1)[-1]]
        own = [j for j in classes if j.rsplit('/', 1)[-1].split('$')[-1] == item['rust']]
        counted = by_owner.get(item['rust'], collections.Counter())
        own_counted = [c for c, _ in counted.most_common() if c.rsplit('/', 1)[-1].split('$')[-1] == item['rust']]
        java = (own or own_counted or classes or [c for c, _ in counted.most_common(1)] or [None])[0]
        if not java:
            continue
        module = item['place'].split('.')[0]
        path = types.path(item['rust'], module)
        if path in seen:
            continue
        seen.add(path)
        kinds.append((path, java.replace('/', '.')))
    kind_lines = [f'        ({bindings.rs(java)}, || std::any::TypeId::of::<std::sync::Mutex<{path}>>()),' for path, java in kinds]
    # Events are held as `jbridge::EvL<T>`.
    for name, path in sorted(all_events.items()):
        if name in full:
            kind_lines.append(f'        ({bindings.rs(full[name])}, || std::any::TypeId::of::<jbridge::EvL<{path}>>()),')

    event_lines = []
    for java, sdk in sorted(pairs):
        path = all_events.get(sdk)
        if not path:
            continue
        event_lines.append(f'        {bindings.rs(java)} => {{ plugin.register_event_with(priority, false, move |event: &mut {path}| crate::events::fire({bindings.rs(java)}, slot, event)); true }}')

    text = f'''// Generated by gen.py: do not edit by hand.
#![allow(unused, clippy::all)]
use jrt::{{R, V}};

/// Text the SDK keeps as `&'static str` (an event's state names): each distinct one leaked once, then shared.
fn static_text(text: &str) -> &'static str {{
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<&'static str>>> = std::sync::OnceLock::new();
    let mut seen = SEEN.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(have) = seen.get(text) {{
        return have;
    }}
    let made: &'static str = Box::leak(text.to_owned().into_boxed_str());
    seen.insert(made);
    made
}}

{code}

pub static NOT_A_CLASS: () = ();

/// Bukkit class names by Rust type (one entry a line: the compile repair drops a line, never the table).
pub static KINDS: &[(&str, fn() -> std::any::TypeId)] = &[
{chr(10).join(kind_lines)}
];

/// The SDK listener for one Bukkit event class at one priority (`slot` 0 LOWEST .. 5 MONITOR).
pub fn register_event(class: &str, slot: u8, plugin: &horizon_bukkit::Plugin) -> bool {{
    let priority = match slot {{
        0 => horizon_bukkit::enums::event_priority::LOWEST,
        1 => horizon_bukkit::enums::event_priority::LOW,
        3 => horizon_bukkit::enums::event_priority::HIGH,
        4 => horizon_bukkit::enums::event_priority::HIGHEST,
        5 => horizon_bukkit::enums::event_priority::MONITOR,
        _ => horizon_bukkit::enums::event_priority::NORMAL,
    }};
    match class {{
{chr(10).join(event_lines)}
        _ => false,
    }}
}}

static RESOURCES: () = ();

/// The hooks, with the layer's own first word on calls and `instanceof` (a Java object handed to the SDK).
pub fn hooks(call: fn(&V, &str, &[V]) -> Option<R>, instance_of: fn(&V, &str) -> bool) -> jrt::Hooks {{
    jrt::Hooks {{ call, call_static: native_static, construct: native_new, field: native_field, instance_of, func_method: native_func, class_fields: native_class_fields, not_lambda: NOT_LAMBDA }}
}}

pub fn sdk_call(r: &V, name: &str, a: &[V]) -> Option<R> {{
    native_call(r, name, a)
}}

pub fn sdk_field(class: &str, name: &str) -> Option<V> {{
    native_field(class, name)
}}

pub fn sdk_instance(v: &V, name: &str) -> bool {{
    native_instance(v, name)
}}

/// Every Bukkit event class the server can fire to Java.
pub static EVENT_CLASSES: &[&str] = &[{', '.join(bindings.rs(j) for j, s in sorted(pairs) if all_events.get(s))}];
'''
    OUT.write_text(text, encoding='utf-8')
    print(f'{OUT}: {text.count(chr(10))} lines, {len(kinds)} kinds, {len(event_lines)} events')
    if '--check' in sys.argv:
        repair()


def cargo_check():
    """rustc's errors as (file, line, message), the outermost span of a macro's."""
    import os
    import subprocess
    env = dict(os.environ, CARGO_TARGET_DIR=str(HERE / 'target'), RUSTC_BOOTSTRAP='1')
    command = ['cargo', '+1.99.1', 'rustc', '--lib', '--profile', 'check', '--message-format=json', '--quiet', '--', '-Zthreads=4']
    run = subprocess.run(command, cwd=HERE, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace')
    errors = []
    for line in run.stdout.splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            continue
        message = row.get('message')
        if row.get('reason') != 'compiler-message' or not message or message.get('level') != 'error':
            continue
        for span in message.get('spans', []):
            if not span.get('is_primary'):
                continue
            while span.get('expansion') and span['expansion'].get('span'):
                span = span['expansion']['span']
            where = pathlib.Path(span['file_name'])
            if not where.is_absolute():
                where = HERE / where
            errors.append((str(where.resolve()), span['line_start'], message['message']))
            break
        else:
            errors.append((None, 0, message['message']))
    return errors, run


DROPPED = []


def repair():
    """jport's compile repair on this crate: each refused line goes, until it compiles. What went, with rustc's
    message, is kept in dropped.json (rust/dropped.py groups it: what to fix in the binding writer)."""
    for round_ in range(1, 15):
        errors, run = cargo_check()
        if not errors:
            print('compiles after', round_, 'round(s)' if run.returncode == 0 else run.stderr[-3000:])
            (HERE / 'dropped.json').write_text(json.dumps(DROPPED, indent=1), encoding='utf-8')
            return
        mine = [(at, msg) for where, at, msg in errors if where and pathlib.Path(where).resolve() == OUT.resolve()]
        others = [(where, at, msg) for where, at, msg in errors if not (where and pathlib.Path(where).resolve() == OUT.resolve())]
        if others and not mine:
            for row in others[:20]:
                print('NOT GENERATED:', row)
            print(run.stderr[-3000:])
            return
        dropped = fix(mine)
        print(f'round {round_}: {len(errors)} errors, {dropped} lines dropped')
    print('still errors after 14 rounds')


def fix(errors):
    lines = OUT.read_text(encoding='utf-8').split('\n')
    remove = set()
    dropped = 0
    for at, message in errors:
        index = at - 1
        if index < 0 or index >= len(lines):
            continue
        line = lines[index]
        DROPPED.append({'message': message, 'line': line.strip()[:600]})
        if line.startswith('fn nc_') and not line.endswith('}'):
            close = next(i for i in range(index + 1, len(lines)) if lines[i] == '    None }' or lines[i].startswith('    None })();'))
            lines[index] = line.split(' {', 1)[0] + ' { None }'
            remove.update(range(index + 1, close + 1))
        elif re.match(r'fn (nc|ns)_\d+\(', line) and line.endswith('}'):
            lines[index] = line.split(' {', 1)[0] + ' { None }'
        elif line.startswith('    if let Some(') and line.endswith('{'):
            close = next(i for i in range(index + 1, len(lines)) if lines[i] == '    }')
            remove.update(range(index, close + 1))
        else:
            remove.add(index)
        dropped += 1
    lines = [line for i, line in enumerate(lines) if i not in remove]
    OUT.write_text('\n'.join(lines), encoding='utf-8')
    return dropped


if __name__ == '__main__':
    main()
