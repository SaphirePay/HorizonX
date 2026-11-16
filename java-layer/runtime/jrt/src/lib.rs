//! What converted Java plugins stand on: Java's values, objects, exceptions and the parts of `java.lang`
//! and `java.util` plugins use. Every Java reference is a clone of a `V`, so aliasing, `null` and
//! inheritance behave as in Java.
use std::any::Any;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

pub mod configurate;
pub mod javaio;
pub mod javautil;
pub mod sql;
pub mod text;
pub mod kotlin;
pub mod net;
pub mod jtime;
pub mod commons;
pub mod reflect;
pub mod props;
pub mod zip;
pub mod bignum;
pub mod datautil;
pub mod javamore;
pub mod javacore;
pub mod jmap;
pub mod okhttp;
pub mod awt;
pub(crate) mod stdlib;
pub mod views;
pub mod fast;
pub use fast::{cached, d2i, d2l, direct, direct_static, idiv, irem, ldiv, leaf, lrem};
pub use stdlib::{KNOWN_METHODS, KNOWN_STATICS, builtin_method, builtin_static, new_builtin};

pub type R = Result<V, V>;
pub type Method = fn(&V, &[V]) -> R;
pub type Func = Arc<dyn Fn(&[V]) -> R + Send + Sync>;

#[derive(Clone)]
pub enum V {
    Null,
    Bool(bool),
    Char(char),
    Int(i32),
    Long(i64),
    Double(f64),
    Str(Arc<str>),
    Arr(Arc<Mutex<Vec<V>>>),
    List(Arc<Mutex<Vec<V>>>),
    Map(Arc<Mutex<JMap>>),
    Set(Arc<Mutex<JMap>>),
    Sb(Arc<Mutex<String>>),
    Iter(Arc<Mutex<(Vec<V>, usize)>>),
    Entry(Arc<(V, V)>),
    Stream(Arc<Mutex<Vec<V>>>),
    Opt(Arc<Option<V>>),
    Exc(Arc<Thrown>),
    Obj(Arc<Object>),
    Native(Arc<dyn Any + Send + Sync>),
    Func(Func),
    Class(&'static str),
}

/// A Java exception of a class the plugin doesn't define.
pub struct Thrown {
    pub class: String,
    pub message: Option<String>,
    /// `getCause()`: what a `CompletionException`/`ExecutionException` wraps.
    pub cause: Option<V>,
    /// `getStackTrace()`: the calls running when the plugin made it (`new Throwable()`), innermost first, as
    /// (class, method). Empty for jrt's own errors.
    pub trace: Vec<(String, String)>,
}

/// `HashMap`, `LinkedHashMap` and `TreeMap` (a set uses the keys), each walked in Java's order: see `jmap`.
pub use jmap::{JMap, Order};

pub struct Class {
    pub name: &'static str,
    pub parent: Option<fn() -> &'static Class>,
    pub interfaces: &'static [fn() -> &'static Class],
    /// Every Java type it can be cast to, by simple name, itself included.
    pub types: &'static [&'static str],
    pub find: fn(&str, &[V]) -> Option<Method>,
}

pub struct Object {
    pub class: &'static Class,
    pub fields: Mutex<BTreeMap<&'static str, V>>,
}

pub struct Statics(pub Mutex<BTreeMap<&'static str, V>>);

impl Statics {
    pub const fn new() -> Self {
        Self(Mutex::new(BTreeMap::new()))
    }
    pub fn get(&self, name: &str) -> V {
        lock(&self.0).get(name).cloned().unwrap_or(V::Null)
    }
    pub fn set(&self, name: &'static str, value: V) -> V {
        lock(&self.0).insert(name, value.clone());
        value
    }
}

/// A class of the same plugin compiled in another of its crates (jport's `split_crates`): its `c_X()` stands for
/// this, found by the class's full name the first time it's wanted. Going through the plugin's class table calls
/// the class's own `c_X()`, so its static initialiser runs just as with a direct call. No lock is held while it's
/// found: a static initialiser that reaches back into this class on the same thread finds it again.
pub struct ForeignClass {
    name: &'static str,
    found: std::sync::atomic::AtomicPtr<Class>,
}

impl ForeignClass {
    pub const fn new(name: &'static str) -> Self {
        Self { name, found: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()) }
    }
    pub fn get(&self) -> &'static Class {
        use std::sync::atomic::Ordering;
        let known = self.found.load(Ordering::Acquire);
        if !known.is_null() {
            // SAFETY: only ever a `&'static Class` from the plugin's class table is stored.
            return unsafe { &*known };
        }
        let class = plugin_class(self.name).unwrap_or_else(|| panic!("class {} is in no crate of this plugin", self.name));
        self.found.store(class as *const Class as *mut Class, Ordering::Release);
        class
    }
}

/// The statics (`S_X`) of a class in another of the plugin's crates, as `ForeignClass` finds the class.
pub struct ForeignStatics {
    name: &'static str,
    found: std::sync::atomic::AtomicPtr<Statics>,
}

impl ForeignStatics {
    pub const fn new(name: &'static str) -> Self {
        Self { name, found: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()) }
    }
    fn real(&self) -> &'static Statics {
        use std::sync::atomic::Ordering;
        let known = self.found.load(Ordering::Acquire);
        if !known.is_null() {
            // SAFETY: only ever a `&'static Statics` from the plugin's statics table is stored.
            return unsafe { &*known };
        }
        let statics = STATICS.get().and_then(|find| find(self.name)).unwrap_or_else(|| panic!("statics of {} are in no crate of this plugin", self.name));
        self.found.store(statics as *const Statics as *mut Statics, Ordering::Release);
        statics
    }
    pub fn get(&self, name: &str) -> V {
        self.real().get(name)
    }
    pub fn set(&self, name: &'static str, value: V) -> V {
        self.real().set(name, value)
    }
}

/// Every class's statics by its full name, for `ForeignStatics`: set by a split plugin as it enables.
static STATICS: OnceLock<fn(&str) -> Option<&'static Statics>> = OnceLock::new();

pub fn set_statics(find: fn(&str) -> Option<&'static Statics>) {
    let _ = STATICS.set(find);
}

/// A class's statics by its full or simple name, when the plugin keeps the table (a split plugin does).
pub fn statics_of(name: &str) -> Option<&'static Statics> {
    let find = STATICS.get()?;
    find(name).or_else(|| find(&java_class_name(name)))
}

/// `<clinit>`: runs once, and the class's own initialiser may use the class while it runs.
pub struct ClassInit {
    done: std::sync::atomic::AtomicBool,
    owner: Mutex<Option<std::thread::ThreadId>>,
    running: Mutex<()>,
}

impl ClassInit {
    pub const fn new() -> Self {
        Self { done: std::sync::atomic::AtomicBool::new(false), owner: Mutex::new(None), running: Mutex::new(()) }
    }
    pub fn run(&self, init: impl FnOnce()) {
        use std::sync::atomic::Ordering;
        if self.done.load(Ordering::Acquire) || *lock(&self.owner) == Some(std::thread::current().id()) {
            return;
        }
        let _running = lock(&self.running);
        if self.done.load(Ordering::Acquire) {
            return;
        }
        *lock(&self.owner) = Some(std::thread::current().id());
        // One initialiser can be a very large function.
        stacker::maybe_grow(16 << 20, 128 << 20, init);
        *lock(&self.owner) = None;
        self.done.store(true, Ordering::Release);
    }
}

pub fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the plugin's generated bindings add: the game's own classes.
#[derive(Clone, Copy)]
pub struct Hooks {
    pub call: fn(&V, &str, &[V]) -> Option<R>,
    pub call_static: fn(&str, &str, &[V]) -> Option<R>,
    pub construct: fn(&str, &[V]) -> Option<R>,
    pub field: fn(&str, &str) -> Option<V>,
    pub instance_of: fn(&V, &str) -> bool,
    /// A default method of the plugin's own functional interface, called on a lambda.
    pub func_method: fn(&V, &str, &[V]) -> Option<R>,
    /// `Foo.class.getFields()` of the plugin's own classes: name, type and value of each static.
    pub class_fields: fn(&str) -> Option<Vec<(&'static str, &'static str, V)>>,
    /// The plugin's classes and interfaces a lambda can't be: anything without one abstract method.
    pub not_lambda: &'static [&'static str],
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

pub fn set_hooks(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

pub(crate) fn hooks() -> Option<&'static Hooks> {
    HOOKS.get()
}

/// The last word on a game object's method nothing else answered (the bridge's: an entity's own class).
static FALLBACK: OnceLock<fn(&V, &str, &[V]) -> Option<R>> = OnceLock::new();

pub fn set_fallback(fallback: fn(&V, &str, &[V]) -> Option<R>) {
    let _ = FALLBACK.set(fallback);
}

/// Last of all, a class another converted plugin owns (its API: `LuckPermsProvider.get()`): a static
/// call, then a `new`. Only after Java's own classes and the game's have had their say.
static ELSEWHERE: OnceLock<(fn(&str, &str, &[V]) -> Option<R>, fn(&str, &[V]) -> Option<R>)> = OnceLock::new();

/// Whether another converted plugin has a class (`Class.forName("net.milkbowl.vault.economy.Economy")`).
static CLASS_ELSEWHERE: OnceLock<fn(&str) -> bool> = OnceLock::new();

pub fn set_class_elsewhere(has: fn(&str) -> bool) {
    let _ = CLASS_ELSEWHERE.set(has);
}

pub fn class_elsewhere(name: &str) -> bool {
    CLASS_ELSEWHERE.get().is_some_and(|has| has(name))
}

pub fn set_elsewhere(static_call: fn(&str, &str, &[V]) -> Option<R>, construct: fn(&str, &[V]) -> Option<R>) {
    let _ = ELSEWHERE.set((static_call, construct));
}

/// A game object's method as the hooks answer it; `None` when none does.
pub fn try_native(this: &V, name: &str, args: &[V]) -> Option<R> {
    hooks().and_then(|h| (h.call)(this, name, args))
}

pub fn exc(class: &str, message: impl Into<String>) -> V {
    V::Exc(Arc::new(Thrown { class: class.to_owned(), message: Some(message.into()), cause: None, trace: Vec::new() }))
}

pub fn unsupported(what: &str) -> R {
    Err(exc("UnsupportedOperationException", format!("not converted: {what}")))
}

pub fn s(text: &str) -> V {
    V::Str(Arc::from(text))
}

/// A Java map (insertion order kept) of these pairs: what an SDK function's `BTreeMap`/`HashMap` comes back as.
pub fn map_of(pairs: Vec<(V, V)>) -> V {
    let map = new_other("LinkedHashMap", &[]).unwrap_or(V::Null);
    for (key, value) in pairs {
        let _ = call(&map, "put", &[key, value]);
    }
    map
}

pub fn arr(items: Vec<V>) -> V {
    V::Arr(Arc::new(Mutex::new(items)))
}

pub fn list(items: Vec<V>) -> V {
    V::List(Arc::new(Mutex::new(items)))
}

/// A varargs parameter: the rest of the arguments, or the array Java handed over whole.
/// One argument, or null when the caller passed fewer.
pub fn arg(args: &[V], at: usize) -> V {
    args.get(at).cloned().unwrap_or(V::Null)
}

/// A method's first N arguments, null for any not passed: `let [mut a, mut b] = jrt::args::<2>(a_);`
pub fn args<const N: usize>(args: &[V]) -> [V; N] {
    std::array::from_fn(|at| arg(args, at))
}

pub fn varargs(args: &[V], from: usize) -> V {
    match args.get(from..) {
        Some([one @ V::Arr(_)]) => one.clone(),
        Some(rest) => arr(rest.to_vec()),
        None => arr(Vec::new()),
    }
}

pub fn native<T: Any + Send + Sync>(value: T) -> V {
    V::Native(Arc::new(value))
}

pub fn func(f: impl Fn(&[V]) -> R + Send + Sync + 'static) -> V {
    V::Func(Arc::new(f))
}

impl V {
    pub fn is_null(&self) -> bool {
        matches!(self, V::Null)
    }
    pub fn truth(&self) -> bool {
        matches!(self, V::Bool(true))
    }
    pub fn int(&self) -> i32 {
        match self {
            V::Int(n) => *n,
            V::Long(n) => *n as i32,
            V::Double(n) => *n as i32,
            V::Char(c) => *c as i32,
            _ => 0,
        }
    }
    pub fn long(&self) -> i64 {
        match self {
            V::Int(n) => i64::from(*n),
            V::Long(n) => *n,
            V::Double(n) => *n as i64,
            V::Char(c) => i64::from(*c as u32),
            _ => 0,
        }
    }
    pub fn double(&self) -> f64 {
        match self {
            V::Int(n) => f64::from(*n),
            V::Long(n) => *n as f64,
            V::Double(n) => *n,
            V::Char(c) => f64::from(*c as u32),
            _ => 0.0,
        }
    }
    pub fn text(&self) -> String {
        match self {
            V::Null => "null".to_owned(),
            V::Bool(b) => b.to_string(),
            V::Char(c) => c.to_string(),
            V::Int(n) => n.to_string(),
            V::Long(n) => n.to_string(),
            V::Double(n) => format_double(*n),
            V::Str(t) => t.to_string(),
            V::Sb(t) => lock(t).clone(),
            V::Arr(_) => "[array]".to_owned(),
            V::List(items) | V::Stream(items) => format!("[{}]", lock(items).iter().map(V::text).collect::<Vec<_>>().join(", ")),
            V::Map(map) => format!("{{{}}}", lock(map).values().map(|(k, v)| format!("{}={}", k.text(), v.text())).collect::<Vec<_>>().join(", ")),
            V::Set(map) => format!("[{}]", lock(map).values().map(|(k, _)| k.text()).collect::<Vec<_>>().join(", ")),
            V::Entry(pair) => format!("{}={}", pair.0.text(), pair.1.text()),
            V::Opt(value) => value.as_ref().as_ref().map_or_else(|| "Optional.empty".to_owned(), |v| format!("Optional[{}]", v.text())),
            // `Throwable.toString()`: the class's full name, then the message.
            V::Exc(thrown) => {
                let name = java_class_name(&thrown.class);
                thrown.message.as_ref().map_or(name.clone(), |m| format!("{name}: {m}"))
            }
            V::Obj(object) => match call(self, "toString", &[]) {
                Ok(text) if !matches!(text, V::Obj(_)) => text.text(),
                _ => format!("{}@object", object.class.name),
            },
            V::Native(native) => call(self, "toString", &[]).map_or_else(|_| format!("object@{:p}", Arc::as_ptr(native)), |t| t.text()),
            V::Iter(_) => "iterator".to_owned(),
            V::Func(_) => "lambda".to_owned(),
            V::Class(name) => format!("class {name}"),
        }
    }
    /// The key a map or set files this under: `equals` and `hashCode` in one string.
    pub fn key(&self) -> String {
        match self {
            V::Str(_) | V::Sb(_) => format!("s{}", self.text()),
            V::Obj(object) if method_of(object.class, "hashCode", &[]).is_some() => match call(self, "hashCode", &[]) {
                Ok(V::Int(hash)) => format!("h{hash}"),
                _ => format!("o{:p}", Arc::as_ptr(object)),
            },
            V::Obj(object) => format!("o{:p}", Arc::as_ptr(object)),
            // A game value that is an enum constant (a Material from `block.getType()`) keys as its name, as `same`
            // already calls it equal to the constant the plugin wrote: `CONTAINERS.contains(type)` was false for a
            // chest (its text was `object@…`), and CoreProtect never logged a broken chest's items.
            V::Native(_) => {
                thread_local! {
                    static KEYING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
                }
                let text = self.text();
                let enum_like = |t: &str| !t.is_empty() && t.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_');
                if enum_like(&text) {
                    return format!("s{text}");
                }
                // Only the default `Object@…` text asks `name()`, and never from inside that call (it looked keys up
                // again: nine `name` calls deep before it failed).
                if (text.starts_with("object@") || text.contains("Object@")) && !KEYING.get() {
                    KEYING.set(true);
                    let name = call(self, "name", &[]);
                    KEYING.set(false);
                    if let Ok(V::Str(name)) = name {
                        if enum_like(&name) {
                            return format!("s{name}");
                        }
                    }
                }
                format!("n{text}")
            }
            // Equal maps and sets are equal whatever order each walks in: their entries' keys, sorted.
            V::Map(map) => {
                let mut rows: Vec<String> = lock(map).values().map(|(k, v)| format!("{}={}", k.key(), v.key())).collect();
                rows.sort();
                format!("m{{{}}}", rows.join(", "))
            }
            V::Set(set) => {
                let mut rows: Vec<String> = lock(set).keys().cloned().collect();
                rows.sort();
                format!("e[{}]", rows.join(", "))
            }
            other => format!("v{}", other.text()),
        }
    }
    pub fn native<T: 'static>(&self) -> Option<&T> {
        match self {
            V::Native(native) => native.downcast_ref::<T>(),
            _ => None,
        }
    }
    pub fn items(&self) -> Result<Vec<V>, V> {
        match self {
            V::Arr(items) | V::List(items) | V::Stream(items) => Ok(lock(items).clone()),
            V::Set(map) => Ok(lock(map).values().map(|(k, _)| k.clone()).collect()),
            V::Map(map) => Ok(lock(map).values().map(|(k, v)| V::Entry(Arc::new((k.clone(), v.clone())))).collect()),
            V::Iter(state) => {
                let state = lock(state);
                Ok(state.0[state.1..].to_vec())
            }
            V::Null => Err(exc("NullPointerException", "iterating over null")),
            other => {
                let iterator = call(other, "iterator", &[])?;
                if matches!(iterator, V::Iter(_)) {
                    return iterator.items();
                }
                let mut out = Vec::new();
                while call(&iterator, "hasNext", &[])?.truth() {
                    out.push(call(&iterator, "next", &[])?);
                }
                Ok(out)
            }
        }
    }
    pub fn get(&self, field: &str) -> R {
        match self {
            V::Obj(object) => Ok(lock(&object.fields).get(field).cloned().unwrap_or(V::Null)),
            // An array the game handed over as a list (`inventory.getContents()`) has a length too: it was null, so
            // CoreProtect's hopper-pull replay made a 0-slot copy and threw ArrayIndexOutOfBounds on every pull.
            V::Arr(items) | V::List(items) if field == "length" => Ok(V::Int(lock(items).len() as i32)),
            // A JSON object Gson read without its class (`new TypeToken<List<Entry>>() {}`): its fields are its keys,
            // as the typed object would have them (WorldEdit's bundled blocks.json / items.json / legacy.json).
            V::Map(map) => Ok(lock(map).get(&s(field).key()).map(|(_, v)| v.clone()).unwrap_or(V::Null)),
            V::Null => Err(null_field("reading", field)),
            // A game or bridge object's public field (`response.balance` of another plugin's record).
            V::Native(_) => Ok(hooks().and_then(|h| (h.call)(self, "$field", &[s(field)])).and_then(Result::ok).unwrap_or(V::Null)),
            _ => Ok(V::Null),
        }
    }
    pub fn set(&self, field: &'static str, value: V) -> R {
        match self {
            V::Obj(object) => {
                // `JRT_TRACE_FIELD=name`: every write of that field, with what was written and where.
                static WATCHED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
                if WATCHED.get_or_init(|| std::env::var("JRT_TRACE_FIELD").ok()).as_deref() == Some(field) {
                    let what = match &value { V::Obj(o) => o.class.name.to_owned(), other => describe(other).to_owned() };
                    let at: Vec<String> = stack_watch::frames().into_iter().take(3).map(|(c, m)| format!("{c}.{m}")).collect();
                    trace_line(&format!("FIELD {}.{field} = {what} at {}", object.class.name, at.join(" < ")));
                }
                lock(&object.fields).insert(field, value.clone());
                Ok(value)
            }
            V::Null => Err(null_field("writing", field)),
            _ => Ok(value),
        }
    }
}

/// Java's `Double.toString`: the shortest digits that read back as the same double, plain from 10^-3 up to
/// 10^7 (with at least one decimal, `5.0`), else as `1.0E7` / `1.0E-7`; `NaN`, `Infinity`, `-0.0`.
fn format_double(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_owned();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if n == 0.0 {
        return if n.is_sign_negative() { "-0.0" } else { "0.0" }.to_owned();
    }
    if (1e-3..1e7).contains(&n.abs()) {
        let text = format!("{n}");
        return if text.contains('.') { text } else { format!("{text}.0") };
    }
    let text = format!("{n:e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let mantissa = if mantissa.contains('.') { mantissa.to_owned() } else { format!("{mantissa}.0") };
    format!("{mantissa}E{exponent}")
}

thread_local! {
    static LENIENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The second look: nothing in the hierarchy fit by type, so go by the number of arguments.
pub fn lenient() -> bool {
    LENIENT.get()
}

/// `method_of` remembered per thread by (class, name, the arguments' kinds and classes): resolving walks the
/// hierarchy and weighs overloads on every call. Only for arguments whose kind and class decide the fit (plain
/// values, strings, plugin objects); a call passing arrays, lambdas, collections or game objects resolves anew.
pub fn method_of(class: &'static Class, name: &str, args: &[V]) -> Option<Method> {
    use std::hash::{Hash, Hasher};
    let cacheable = args.iter().all(|a| matches!(a, V::Null | V::Bool(_) | V::Char(_) | V::Int(_) | V::Long(_) | V::Double(_) | V::Str(_) | V::Obj(_)));
    if !cacheable || LENIENT.get() || STRICT.get() {
        return method_of_uncached(class, name, args);
    }
    // FNV-1a over the class, name and argument kinds (SipHash twice, here and in the map, was a third of a call).
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    (class as *const Class as usize).hash(&mut h);
    h.write(name.as_bytes());
    for a in args {
        std::mem::discriminant(a).hash(&mut h);
        if let V::Obj(o) = a {
            (o.class as *const Class as usize).hash(&mut h);
            // `^Type` fits an object standing on a game one (the plugin's main class): that is per object.
            lock(&o.fields).contains_key("__native").hash(&mut h);
        }
    }
    let key = h.finish();
    thread_local! {
        static RESOLVED: std::cell::RefCell<std::collections::HashMap<u64, Option<Method>, std::hash::BuildHasherDefault<Same>>> = std::cell::RefCell::new(std::collections::HashMap::default());
    }
    if let Some(known) = RESOLVED.with_borrow(|m| m.get(&key).copied()) {
        return known;
    }
    let found = method_of_uncached(class, name, args);
    RESOLVED.with_borrow_mut(|m| m.insert(key, found));
    found
}

struct Fnv(u64);

impl std::hash::Hasher for Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn write_usize(&mut self, n: usize) {
        self.0 = (self.0 ^ n as u64).wrapping_mul(0x0100_0000_01b3);
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0 ^ n).wrapping_mul(0x0100_0000_01b3);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

/// A map keyed by hashes already made: the key is its own hash.
#[derive(Default)]
struct Same(u64);

impl std::hash::Hasher for Same {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(*b);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

fn method_of_uncached(class: &'static Class, name: &str, args: &[V]) -> Option<Method> {
    // Java weighs every overload of the hierarchy together: first one whose types fit anywhere up the chain, and
    // only then one that fits on trust (`fits_basic`) or by count. A subclass's trusted guess went before its
    // parent's exact match (FAWE's HorizonAdapter.adapt(BaseItemStack) took a World meant for adapt(World)).
    let outer_strict = STRICT.replace(true);
    let found = find_in(class, name, args);
    let found = match found {
        Some(method) => Some(more_specific_default(class, name, args).unwrap_or(method)),
        None => None,
    };
    STRICT.set(outer_strict);
    if found.is_some() {
        return found;
    }
    if let Some(found) = find_in(class, name, args) {
        return Some(found);
    }
    let outer = LENIENT.replace(true);
    let found = find_in(class, name, args);
    LENIENT.set(outer);
    found
}

thread_local! {
    static STRICT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Java picks the most specific overload of the whole hierarchy; `find_in` takes the class chain's before any
/// interface default. When the class chain's method takes a plugin type the argument only extends, an interface
/// default taking the argument's narrower type wins, as in Java: `pattern.applyBlock(filterBlock)` is Pattern's
/// default `applyBlock(FilterBlock)` (which writes the block), not BaseBlock's `applyBlock(BlockVector3)` (which
/// only returns it): every FAWE //set changed nothing.
fn more_specific_default(class: &'static Class, name: &str, args: &[V]) -> Option<Method> {
    if !args.iter().any(|a| matches!(a, V::Obj(_))) || class.interfaces.is_empty() && class.parent.is_none() {
        return None;
    }
    // Where `find_in` found it (the class chain, or a default of an interface along it), and its parameter types
    // (when it has one overload of this size).
    let owner = owner_in(class, name, args)?;
    let rows = reflect::overload_params(owner.name, name, args.len());
    if rows.len() != 1 {
        return None;
    }
    let taken = rows[0];
    // Only worth a look when an argument is a plugin object of a narrower class than the parameter.
    let narrower = args.iter().zip(taken).any(|(arg, param)| matches!(arg, V::Obj(o) if o.class.name != *param && class_named(param).is_some()));
    if !narrower {
        return None;
    }
    let mut seen = Vec::new();
    let mut current = Some(class);
    while let Some(c) = current {
        for interface in c.interfaces {
            interface_defaults(interface(), owner, name, args, taken, &mut seen);
        }
        current = c.parent.map(|p| p());
    }
    seen.into_iter().next()
}

/// The class `find_in` takes the method from, in the same order.
fn owner_in(class: &'static Class, name: &str, args: &[V]) -> Option<&'static Class> {
    let mut current = Some(class);
    while let Some(c) = current {
        if (c.find)(name, args).is_some() {
            return Some(c);
        }
        current = c.parent.map(|p| p());
    }
    let mut current = Some(class);
    while let Some(c) = current {
        for interface in c.interfaces {
            if std::ptr::eq(interface(), class) {
                continue;
            }
            if let Some(owner) = owner_in(interface(), name, args) {
                return Some(owner);
            }
        }
        current = c.parent.map(|p| p());
    }
    None
}

fn interface_defaults(interface: &'static Class, owner: &'static Class, name: &str, args: &[V], taken: &[&str], out: &mut Vec<Method>) {
    if !out.is_empty() {
        return;
    }
    if std::ptr::eq(interface, owner) {
        // Its parents can still hold the narrower one (BlockStateHolder's parent Pattern).
        for parent in interface.interfaces {
            interface_defaults(parent(), owner, name, args, taken, out);
        }
        return;
    }
    for params in reflect::overload_params(interface.name, name, args.len()).iter() {
        // Narrower than the class's: each parameter the same or a subtype, one strictly, and the arguments fit it.
        let narrower = params.iter().zip(taken).all(|(p, t)| p == t || class_named(p).is_some_and(|pc| has_type(pc, t)))
            && params.iter().zip(taken).any(|(p, t)| p != t);
        let fits = args.iter().zip(params.iter()).all(|(arg, p)| !matches!(arg, V::Obj(_)) || instance_of(arg, p));
        if narrower && fits {
            if let Some(method) = (interface.find)(name, args) {
                out.push(method);
                return;
            }
        }
    }
    for parent in interface.interfaces {
        interface_defaults(parent(), owner, name, args, taken, out);
    }
}

/// The class chain first, then the default methods of every interface along it.
fn find_in(class: &'static Class, name: &str, args: &[V]) -> Option<Method> {
    let mut current = Some(class);
    while let Some(c) = current {
        if let Some(method) = (c.find)(name, args) {
            return Some(method);
        }
        current = c.parent.map(|p| p());
    }
    let mut current = Some(class);
    while let Some(c) = current {
        for interface in c.interfaces {
            if std::ptr::eq(interface(), class) {
                continue;
            }
            if let Some(method) = find_in(interface(), name, args) {
                return Some(method);
            }
        }
        current = c.parent.map(|p| p());
    }
    None
}

/// A lambda standing in for the plugin's own functional interface, so its default methods work.
pub fn as_interface(class: &'static Class, function: V) -> V {
    V::Obj(Arc::new(Object { class, fields: Mutex::new(BTreeMap::from([("$fn", function)])) }))
}

/// Java's stack is deep and each of its calls is several of ours: grow rather than overflow. Grown only with
/// less than 1 MB left: a 2 MB thread (Rust's default: the Bukkit async scheduler's, the engine's) starts with
/// about 1.9 MB, and with 2 MB asked every call made there got a new 64 MB stack of its own, ~0.3 ms a call
/// (a converted plugin's async work ran ~4,600x slower than on the 8 MB server thread).
fn deep<T>(work: impl FnOnce() -> T) -> T {
    stacker::maybe_grow(1 << 20, 64 << 20, work)
}

thread_local! {
    static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static TRAIL: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The calls the last exception passed through, innermost first.
    static TRACE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// About where Java itself gives up.
const MAX_DEPTH: u32 = 4000;

fn describe(this: &V) -> &'static str {
    match this {
        V::Obj(object) => object.class.name,
        V::Native(_) => "game",
        V::Func(_) => "lambda",
        V::Null => "null",
        V::Bool(_) => "boolean",
        V::Char(_) => "char",
        V::Int(_) | V::Long(_) | V::Double(_) => "number",
        V::Str(_) => "String",
        V::Arr(_) => "array",
        V::List(_) => "List",
        V::Map(_) => "Map",
        V::Set(_) => "Set",
        V::Sb(_) => "StringBuilder",
        V::Iter(_) => "Iterator",
        V::Entry(_) => "Entry",
        V::Stream(_) => "Stream",
        V::Opt(_) => "Optional",
        V::Exc(_) => "Throwable",
        V::Class(_) => "Class",
    }
}

/// The Java calls in progress on each thread, for the server's watchdog: when the server thread hangs
/// in a plugin, the engine asks (`horizon_java_stack_ask`) and the server thread writes its calls at
/// its next Java call; the engine reads them (`horizon_java_stack_take`). A call costs a push and a pop.
pub mod stack_watch {
    use std::cell::RefCell;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    static ASKED: AtomicBool = AtomicBool::new(false);
    static ANSWER: Mutex<Option<String>> = Mutex::new(None);
    /// The answer handed out last, kept until the next ask.
    static HANDED: Mutex<String> = Mutex::new(String::new());

    /// The server thread: the one the plugin starts on (its name is "main" on a dedicated server).
    static SERVER_ID: std::sync::OnceLock<std::thread::ThreadId> = std::sync::OnceLock::new();

    thread_local! {
        // The names live for as long as their call: they are read only on this thread, during it.
        static CALLS: RefCell<Vec<(*const str, *const str)>> = const { RefCell::new(Vec::new()) };
    }

    /// `jrt-env.txt` in the server folder (`JRT_TRACE=...` lines), read at each load: the debug switches turned
    /// on or off by a hot reload, with no restart. Names a previous file set and this one doesn't are unset.
    fn env_from_file() {
        let wanted: Vec<(String, String)> = std::fs::read_to_string("jrt-env.txt")
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().split_once('='))
            .filter(|(k, _)| k.starts_with("JRT_"))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
            .collect();
        let before = std::env::var("JRT_ENV_FROM_FILE").unwrap_or_default();
        // SAFETY: at plugin load on the server thread, before the plugin starts any thread of its own.
        unsafe {
            for name in before.split(',').filter(|n| !n.is_empty()) {
                if !wanted.iter().any(|(k, _)| k == name) {
                    std::env::remove_var(name);
                }
            }
            for (k, v) in &wanted {
                std::env::set_var(k, v);
            }
            std::env::set_var("JRT_ENV_FROM_FILE", wanted.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(","));
        }
    }

    /// The plugin starts on this thread (jbridge, as it starts).
    pub fn mark_server_thread() {
        let _ = SERVER_ID.set(std::thread::current().id());
        env_from_file();
        // Also from `jrt-rust-bt.txt` in the server folder, read at each load: a hot reload turns it on, no restart.
        let from_file = std::fs::read_to_string("jrt-rust-bt.txt").ok().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
        if let Some(target) = std::env::var("JRT_RUST_BT").ok().or(from_file) {
            let _ = BT_TARGET.set(target);
            BT_ON.store(true, Ordering::Relaxed);
        }
        // `JRT_STACK_EVERY=<seconds>`: the server thread's Java calls written to `jrt-stacks.txt` (in the server's
        // folder) that often, to see where a plugin that freezes or crawls spends its time.
        if let Some(every) = std::env::var("JRT_STACK_EVERY").ok().and_then(|s| s.parse::<u64>().ok()).filter(|s| *s > 0) {
            OTHERS_ON.store(true, Ordering::Relaxed);
            let _ = std::thread::Builder::new().name("jrt stack dumper".into()).spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(every));
                horizon_java_stack_ask();
                std::thread::sleep(std::time::Duration::from_millis(500));
                let mut length = 0;
                let text = horizon_java_stack_take(&mut length);
                if text.is_null() {
                    continue;
                }
                // SAFETY: the bytes last until the next ask, which is this thread's own.
                let body = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(text, length) }).into_owned();
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("jrt-stacks.txt") {
                    let _ = writeln!(f, "==== {:?}
{body}
{}", std::time::SystemTime::now(), others_text());
                }
            });
        }
    }

    fn on_server_thread() -> bool {
        SERVER_ID.get().is_some_and(|id| *id == std::thread::current().id())
    }

    /// A call in progress: it leaves the stack when dropped, by a return or a panic alike.
    pub(crate) struct Call(());

    impl Drop for Call {
        #[inline]
        fn drop(&mut self) {
            CALLS.with_borrow_mut(|calls| calls.pop());
        }
    }

    /// The Java calls running on this thread, innermost first, as (class, method) with the class's Java name:
    /// what `new Throwable().getStackTrace()` and `Thread.getStackTrace()` hold.
    pub fn frames() -> Vec<(String, String)> {
        CALLS.with_borrow(|calls| {
            calls
                .iter()
                .rev()
                .take(256)
                // SAFETY: each pair borrows from a call still running on this thread (see CALLS).
                .map(|(owner, name)| unsafe { (&**owner, &**name) })
                // The constructor's own frames are not the caller's: Java's trace starts where `new` was written.
                .filter(|(owner, name)| !(*name == "<init>" && matches!(*owner, "Throwable" | "Exception" | "RuntimeException")))
                .map(|(owner, name)| (crate::java_class_name(owner), name.to_owned()))
                .collect()
        })
    }

    /// `JRT_STACK_EVERY` set: every other thread's calls as they were at its last blocking call (a `get`, `join`,
    /// `await`...) or at most 250 ms ago, written beside the server thread's, to see where a worker waits.
    static OTHERS_ON: AtomicBool = AtomicBool::new(false);
    static OTHERS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
    thread_local! {
        static LAST_SNAP: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
    }

    #[cold]
    fn snap_other(owner: &str, name: &str) {
        let blocking = matches!(name, "get" | "join" | "await" | "acquire" | "take" | "park" | "sleep" | "lock" | "wait" | "poll" | "invokeAll" | "awaitTermination" | "getNow");
        let due = LAST_SNAP.get().is_none_or(|at| at.elapsed() > std::time::Duration::from_millis(250));
        if !blocking && !due {
            return;
        }
        LAST_SNAP.set(Some(std::time::Instant::now()));
        let mut lines = vec![format!("{owner}.{name}")];
        CALLS.with_borrow(|calls| {
            for (owner, name) in calls.iter().rev().take(60) {
                // SAFETY: each pair borrows from a call still running on this thread (see CALLS).
                let (owner, name) = unsafe { (&**owner, &**name) };
                lines.push(format!("{owner}.{name}"));
            }
        });
        let me = std::thread::current();
        let id = format!("{:?} {}", me.id(), me.name().unwrap_or(""));
        let mut all = OTHERS.lock().unwrap_or_else(|e| e.into_inner());
        match all.iter_mut().find(|(who, _)| *who == id) {
            Some(row) => row.1 = lines.join("
"),
            None => all.push((id, lines.join("
"))),
        }
    }

    /// `JRT_RUST_BT=Owner.name`: the Rust stack (not the Java one) of the first 3 calls of that method, in
    /// `jrt-trace.txt`: which runtime code calls a plugin method nobody asked for (a `toString` used as a key).
    static BT_ON: AtomicBool = AtomicBool::new(false);
    static BT_LEFT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(3);
    static BT_TARGET: std::sync::OnceLock<String> = std::sync::OnceLock::new();

    #[cold]
    fn rust_backtrace(owner: &str, name: &str) {
        let Some(target) = BT_TARGET.get() else { return };
        if target.strip_suffix(name).and_then(|t| t.strip_suffix('.')) != Some(owner) {
            return;
        }
        if BT_LEFT.fetch_sub(1, Ordering::Relaxed) == 0 {
            BT_ON.store(false, Ordering::Relaxed);
            return;
        }
        let trace = std::backtrace::Backtrace::force_capture().to_string();
        let short: Vec<&str> = trace.lines().filter(|l| !l.trim_start().starts_with("at ")).take(60).collect();
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("jrt-trace.txt") {
            let _ = writeln!(f, "RUSTBT {owner}.{name}
{}", short.join("
"));
        }
    }

    pub(crate) fn others_text() -> String {
        OTHERS.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(who, text)| format!("---- thread {who}
{text}
")).collect()
    }

    #[inline]
    pub(crate) fn enter(owner: &str, name: &str) -> Call {
        if ASKED.load(Ordering::Relaxed) && on_server_thread() {
            answer(owner, name);
        }
        if OTHERS_ON.load(Ordering::Relaxed) && !on_server_thread() {
            snap_other(owner, name);
        }
        if BT_ON.load(Ordering::Relaxed) {
            rust_backtrace(owner, name);
        }
        CALLS.with_borrow_mut(|calls| calls.push((owner as *const str, name as *const str)));
        Call(())
    }

    #[cold]
    fn answer(owner: &str, name: &str) {
        ASKED.store(false, Ordering::Relaxed);
        let mut lines = vec![format!("{owner}.{name}")];
        CALLS.with_borrow(|calls| {
            for (owner, name) in calls.iter().rev().take(200) {
                // SAFETY: each pair borrows from a call still running on this thread (see CALLS).
                let (owner, name) = unsafe { (&**owner, &**name) };
                lines.push(format!("{owner}.{name}"));
            }
            if calls.len() > 200 {
                // A deep stack is mostly one repeating call: the outermost frames say who started it.
                let outer = (calls.len() - 200).min(60);
                lines.push(format!("... {} more, the outermost {outer}:", calls.len() - 200));
                for (owner, name) in calls.iter().take(outer).rev() {
                    // SAFETY: as above.
                    let (owner, name) = unsafe { (&**owner, &**name) };
                    lines.push(format!("{owner}.{name}"));
                }
            }
        });
        *ANSWER.lock().unwrap_or_else(|e| e.into_inner()) = Some(lines.join("\n"));
    }

    /// The engine's watchdog asks the server thread for its Java calls.
    #[unsafe(no_mangle)]
    pub extern "C" fn horizon_java_stack_ask() {
        *ANSWER.lock().unwrap_or_else(|e| e.into_inner()) = None;
        ASKED.store(true, Ordering::Relaxed);
    }

    /// The answer, once there is one: null until then. The bytes last until the next ask.
    #[unsafe(no_mangle)]
    pub extern "C" fn horizon_java_stack_take(length: *mut usize) -> *const u8 {
        let Some(text) = ANSWER.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return std::ptr::null();
        };
        let mut handed = HANDED.lock().unwrap_or_else(|e| e.into_inner());
        *handed = text;
        if !length.is_null() {
            // SAFETY: the engine passes a writable length.
            unsafe { *length = handed.len() };
        }
        handed.as_ptr()
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn the_server_thread_answers_with_its_calls() {
            let answer = std::thread::Builder::new()
                .name("main".into())
                .spawn(|| {
                    super::mark_server_thread();
                    let outer = super::enter("BoardService", "refresh");
                    let inner = super::enter("LeaderboardEntry", "toString");
                    super::horizon_java_stack_ask();
                    drop(super::enter("Duration", "toMinutesPart"));
                    drop(inner);
                    drop(outer);
                    super::CALLS.with_borrow(|calls| assert!(calls.is_empty()));
                    let mut length = 0;
                    let text = super::horizon_java_stack_take(&mut length);
                    assert!(!text.is_null());
                    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(text, length) }).into_owned()
                })
                .unwrap()
                .join()
                .unwrap();
            assert_eq!(answer, "Duration.toMinutesPart\nLeaderboardEntry.toString\nBoardService.refresh");
        }
    }
}

/// Every Java call goes through here: a stack that grows, and a limit like Java's.
pub fn guarded(owner: &str, name: &str, work: impl FnOnce() -> R) -> R {
    let depth = DEPTH.get();
    if depth >= MAX_DEPTH {
        let trail = TRAIL.with_borrow_mut(std::mem::take).join(" > ");
        return Err(exc("StackOverflowError", trail));
    }
    if depth >= MAX_DEPTH - 24 {
        TRAIL.with_borrow_mut(|t| t.push(format!("{owner}.{name}")));
    }
    DEPTH.set(depth + 1);
    let shadow = stack_watch::enter(owner, name);
    let traced = trace_wanted(owner, name);
    if traced {
        trace_line(&format!("{:indent$}{owner}.{name}", "", indent = (depth as usize).min(80)));
    }
    let timed = profile::on().then(profile::enter);
    let out = deep(work);
    if let Some(started) = timed {
        profile::leave(owner, name, started);
    }
    drop(shadow);
    DEPTH.set(depth);
    if traced {
        if let Err(thrown) = &out {
            trace_line(&format!("{:indent$}!! {owner}.{name} threw {}", "", throwable_text(thrown), indent = (depth as usize).min(80)));
        }
    }
    if out.is_err() {
        TRACE.with_borrow_mut(|t| {
            if t.len() >= 16 {
                t.remove(0);
            }
            t.push(format!("{owner}.{name}"));
        });
    }
    out
}

/// A piece of work counted under `owner.name` by `JRT_PROFILE=1` (free when it's off): a stage of the bridge's
/// dispatch, to see which one a slow call spends its time in.
pub fn timed<T>(owner: &str, name: &str, work: impl FnOnce() -> T) -> T {
    if !profile::on() {
        return work();
    }
    let started = profile::enter();
    let out = work();
    profile::leave(owner, name, started);
    out
}

/// `JRT_PROFILE=1`: where a plugin's time goes, as self time per `Class.method` (a call's time less its callees'),
/// with counts, written to `jrt-profile.txt` in the server's folder every 30 s, the 40 costliest first. To make a
/// converted plugin faster: which game calls or Java library calls cost the most, not just which listener.
mod profile {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    pub fn on() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| std::env::var("JRT_PROFILE").is_ok_and(|v| v == "1"))
    }

    thread_local! {
        /// The callees' time of each call in progress, innermost last.
        static CHILDREN: RefCell<Vec<Duration>> = const { RefCell::new(Vec::new()) };
    }

    struct Table {
        since: Instant,
        rows: HashMap<String, (u64, Duration)>,
    }
    static TABLE: std::sync::Mutex<Option<Table>> = std::sync::Mutex::new(None);

    pub fn enter() -> Instant {
        CHILDREN.with_borrow_mut(|c| c.push(Duration::ZERO));
        Instant::now()
    }

    pub fn leave(owner: &str, name: &str, started: Instant) {
        let total = started.elapsed();
        let children = CHILDREN.with_borrow_mut(|c| {
            let mine = c.pop().unwrap_or_default();
            if let Some(parent) = c.last_mut() {
                *parent += total;
            }
            mine
        });
        let own = total.saturating_sub(children);
        let mut held = crate::lock(&TABLE);
        let table = held.get_or_insert_with(|| Table { since: Instant::now(), rows: HashMap::new() });
        let row = table.rows.entry(format!("{owner}.{name}")).or_insert((0, Duration::ZERO));
        row.0 += 1;
        row.1 += own;
        if table.since.elapsed() >= Duration::from_secs(30) {
            let mut rows: Vec<(String, (u64, Duration))> = table.rows.drain().collect();
            table.since = Instant::now();
            drop(held);
            rows.sort_by(|a, b| b.1.1.cmp(&a.1.1));
            let mut text = String::from("==== 30 s, self time\n");
            for (call, (count, spent)) in rows.into_iter().take(40) {
                text.push_str(&format!("{:>9.1} ms {:>8} calls {:>7.1} us  {call}\n", spent.as_secs_f64() * 1000.0, count, spent.as_secs_f64() * 1e6 / count.max(1) as f64));
            }
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("jrt-profile.txt") {
                let _ = f.write_all(text.as_bytes());
            }
        }
    }
}

/// `JRT_TRACE=word,word`: every Java call whose class or method name holds one of the words goes to `jrt-trace.txt`
/// in the server's folder, indented by depth, with what it threw: to follow a plugin's start or a command that does
/// nothing, without a debugger.
static TRACE_WORDS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

fn trace_wanted(owner: &str, name: &str) -> bool {
    let words = TRACE_WORDS.get_or_init(|| std::env::var("JRT_TRACE").map(|w| w.split(',').filter(|s| !s.is_empty()).map(str::to_owned).collect()).unwrap_or_default());
    !words.is_empty() && words.iter().any(|w| owner.contains(w.as_str()) || name.contains(w.as_str()))
}

fn trace_line(line: &str) {
    use std::io::Write;
    static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);
    let mut file = lock(&FILE);
    if file.is_none() {
        *file = std::fs::OpenOptions::new().create(true).append(true).open("jrt-trace.txt").ok();
    }
    if let Some(f) = file.as_mut() {
        let _ = writeln!(f, "{line}");
    }
}

/// `invokevirtual`.
pub fn call(this: &V, name: &str, args: &[V]) -> R {
    if trace_wanted(describe(this), name) {
        let kinds: Vec<String> = args.iter().map(|a| match a {
            V::Obj(o) => format!("Obj<{}>", o.class.name),
            V::Native(_) => "Native".to_owned(),
            V::Class(c) => format!("Class<{c}>"),
            other => describe(other).to_owned(),
        }).collect();
        let receiver = match this { V::Obj(o) => format!("Obj<{}>", o.class.name), other => describe(other).to_owned() };
        trace_line(&format!("   args {receiver}.{name}({})", kinds.join(", ")));
        // `JRT_TRACE_STACK=name`: who called it, outermost last (the first few times only).
        static STACKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if std::env::var("JRT_TRACE_STACK").is_ok_and(|w| w == name) && STACKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 5 {
            let frames: Vec<String> = stack_watch::frames().into_iter().take(40).map(|(c, m)| format!("{c}.{m}")).collect();
            trace_line(&format!("   stack {}", frames.join(" < ")));
        }
    }
    // Profiling: a game object by its kind (`Block`, `Material`), not "game", to see which kind's calls cost.
    let owner = match (this, NATIVE_LABEL.get()) {
        (V::Native(_), Some(label)) if profile::on() => label(this),
        _ => describe(this),
    };
    guarded(owner, name, || dispatch(this, name, args))
}

static NATIVE_LABEL: OnceLock<fn(&V) -> &'static str> = OnceLock::new();

/// What a game object is, for `JRT_PROFILE=1`'s table.
pub fn set_native_label(label: fn(&V) -> &'static str) {
    let _ = NATIVE_LABEL.set(label);
}

fn dispatch(this: &V, name: &str, args: &[V]) -> R {
    match this {
        V::Null => Err(exc("NullPointerException", format!("calling {name} on null"))),
        V::Obj(object) => {
            if let Some(method) = method_of(object.class, name, args) {
                // An AutoValue class is immutable by contract, so its hash is worked out once, as a JVM caches a
                // String's. Piston (WorldEdit's commands) hashes whole command trees made of them on every map
                // put: without this FAWE spent minutes of its start re-hashing the same trees.
                if name == "hashCode" && args.is_empty() && object.class.name.starts_with("AutoValue_") {
                    if let Some(known) = lock(&object.fields).get("$hash").cloned() {
                        return Ok(known);
                    }
                    let hash = method(this, args)?;
                    lock(&object.fields).insert("$hash", hash.clone());
                    return Ok(hash);
                }
                return method(this, args);
            }
            // `Throwable`'s own on a plugin's exception: `toString()` is the full name and the message, and
            // `getLocalizedMessage()` is `getMessage()` (which the plugin may override).
            if args.is_empty() && matches!(name, "toString" | "getLocalizedMessage") && instance_of(this, "Throwable") {
                return if name == "toString" { Ok(s(&throwable_text(this))) } else { call(this, "getMessage", &[]) };
            }
            // A class of the plugin handed over where Java wants a function.
            if name == "$invoke" {
                for wanted in ["compare", "apply", "accept", "test", "get", "run", "call", "load"] {
                    if let Some(method) = method_of(object.class, wanted, args) {
                        return method(this, args);
                    }
                }
            }
            // The defaults of `Comparator`, on a class that implements it.
            if matches!(name, "reversed" | "thenComparing" | "thenComparingInt" | "thenComparingLong" | "thenComparingDouble") && method_of(object.class, "compare", &[V::Null, V::Null]).is_some() {
                let target = this.clone();
                let as_function = func(move |x| call(&target, "compare", x));
                if let Some(found) = javautil::func_default(&as_function, name, args) {
                    return found;
                }
            }
            // The defaults of `Iterable`/`Collection`/`List` on a class of the plugin's that implements them
            // (a bundled Kotlin `EmptyList`, a custom list): its items, as it gives them, through Java's own.
            if matches!(name, "forEach" | "stream" | "parallelStream" | "spliterator" | "toArray" | "isEmpty" | "contains" | "containsAll" | "indexOf" | "toList")
                && method_of(object.class, name, args).is_none()
            {
                let items = if method_of(object.class, "iterator", &[]).is_some() {
                    let iterator = call(this, "iterator", &[])?;
                    let mut items = Vec::new();
                    while call(&iterator, "hasNext", &[])?.truth() {
                        items.push(call(&iterator, "next", &[])?);
                    }
                    Some(items)
                } else if method_of(object.class, "get", &[V::Int(0)]).is_some() && (method_of(object.class, "size", &[]).is_some() || method_of(object.class, "getSize", &[]).is_some()) {
                    let size = call(this, if method_of(object.class, "size", &[]).is_some() { "size" } else { "getSize" }, &[])?.int();
                    Some((0..size).map(|i| call(this, "get", &[V::Int(i)])).collect::<Result<Vec<_>, _>>()?)
                } else {
                    None
                };
                if let Some(items) = items {
                    return call(&list(items), name, args);
                }
            }
            // What a `java.util.logging.Handler` (or `Formatter`) of the plugin's gets from its parent.
            if matches!(name, "setLevel" | "setFormatter" | "setFilter" | "setEncoding" | "setErrorManager" | "getLevel" | "getFormatter" | "getFilter" | "getErrorManager" | "getEncoding" | "isLoggable" | "flush" | "close")
                && (has_type(object.class, "Handler") || has_type(object.class, "StreamHandler"))
            {
                match (name, args.len()) {
                    ("setLevel" | "setFormatter" | "setFilter" | "setEncoding" | "setErrorManager", 1) => {
                        lock(&object.fields).insert(if name == "setLevel" { "$level" } else if name == "setFormatter" { "$formatter" } else { "$filter" }, args[0].clone());
                        return Ok(V::Null);
                    }
                    ("getLevel", 0) => return Ok(lock(&object.fields).get("$level").cloned().unwrap_or_else(|| s("ALL"))),
                    ("getFormatter", 0) => return Ok(lock(&object.fields).get("$formatter").cloned().unwrap_or(V::Null)),
                    ("getFilter" | "getErrorManager" | "getEncoding", 0) => return Ok(lock(&object.fields).get("$filter").cloned().unwrap_or(V::Null)),
                    ("isLoggable", 1) => return Ok(V::Bool(true)),
                    ("flush" | "close", 0) => return Ok(V::Null),
                    _ => {}
                }
            }
            // What every object has from `java.lang.Object`.
            match (name, args.len()) {
                ("hashCode", 0) => return Ok(V::Int((Arc::as_ptr(object) as usize >> 4) as i32)),
                ("equals", 1) => return Ok(V::Bool(same(this, &args[0]))),
                // `Object.clone()` on a class without its own: a shallow copy, the same class and field values.
                ("clone", 0) => return Ok(V::Obj(Arc::new(Object { class: object.class, fields: Mutex::new(lock(&object.fields).clone()) }))),
                _ => {}
            }
            let lambda = lock(&object.fields).get("$fn").cloned();
            if let Some(lambda) = lambda {
                return call(&lambda, "$invoke", args);
            }
            // Guava's `Forwarding*` and `AbstractIterator`: what such a class gets from its parent.
            if name != "delegate" && !matches!(name, "toString" | "hashCode" | "equals") {
                if let Some(delegate) = method_of(object.class, "delegate", &[]) {
                    return call(&delegate(this, &[])?, name, args);
                }
            }
            if matches!(name, "hasNext" | "next" | "endOfData") && method_of(object.class, "computeNext", &[]).is_some() {
                return javautil::abstract_iterator(this, name);
            }
            // Gson's `new TypeToken<List<X>>() {}.getType()`: Gson here reads JSON without one.
            if matches!(name, "getType" | "getRawType") && args.is_empty() && has_type(object.class, "TypeToken") {
                return Ok(V::Class("Object"));
            }
            // Guava's `new TypeToken<X>() {}`: the rest of TypeToken's methods, as a token of this class (the type
            // argument isn't kept, so two are equal when made at the same place).
            if has_type(object.class, "TypeToken") && matches!(name, "wrap" | "unwrap" | "equals" | "hashCode" | "toString" | "isSubtypeOf" | "isSupertypeOf" | "isArray" | "isPrimitive" | "getComponentType") {
                let token = native(javacore::JTypeToken::of_class(object.class.name));
                let args: Vec<V> = args.iter().map(|a| match a {
                    V::Obj(o) if has_type(o.class, "TypeToken") => native(javacore::JTypeToken::of_class(o.class.name)),
                    other => other.clone(),
                }).collect();
                return call(&token, name, &args);
            }
            // A plugin's main class or a listener: what it extends in the game.
            let parent = lock(&object.fields).get("__native").cloned();
            if let Some(parent) = parent {
                return call(&parent, name, args);
            }
            hooks().and_then(|h| (h.call)(this, name, args)).or_else(|| builtin_method(this, name, args)).unwrap_or_else(|| {
                // `java.lang.Object.toString()` for a class that doesn't write one: `Name@hash`, as Java's
                // (WorldEdit logs its PlatformCommandManager and SessionManager so).
                if name == "toString" && args.is_empty() {
                    return Ok(s(&format!("{}@{:x}", java_class_name(object.class.name), (Arc::as_ptr(object) as usize >> 4) as u32)));
                }
                no_method(object.class.name, name)
            })
        }
        V::Func(f) if name == "$invoke" => f(args),
        V::Func(_) if matches!((name, args.len()), ("hashCode", 0) | ("equals", 1)) => views::method(this, name, args).unwrap_or_else(|| no_method("lambda", name)),
        V::Func(f) if !matches!((name, args.len()), ("toString", 0)) => {
            if let Some(found) = hooks().and_then(|h| (h.func_method)(this, name, args)).or_else(|| javautil::func_default(this, name, args)) {
                return found;
            }
            f(args)
        }
        V::Native(_) => hooks()
            .and_then(|h| timed("stage", "hooks", || (h.call)(this, name, args)))
            .or_else(|| timed("builtin", name, || builtin_method(this, name, args)))
            .or_else(|| timed("stage", "spread_varargs", || spread_varargs(this, name, args)))
            .or_else(|| timed("stage", "fallback", || FALLBACK.get().and_then(|f| f(this, name, args))))
            .unwrap_or_else(|| no_method("game object", name)),
        // Java's own methods first; then the game's, for a value that stands for one of its things
        // (`Material.STONE` reads as the name "STONE").
        other => builtin_method(other, name, args).or_else(|| hooks().and_then(|h| (h.call)(this, name, args))).unwrap_or_else(|| no_method(describe(other), name)),
    }
}

/// Does the class define this itself? `onEnable` and friends are optional.
pub fn defines(this: &V, name: &str, count: usize) -> bool {
    let V::Obj(object) = this else { return false };
    method_of(object.class, name, &vec![V::Null; count]).is_some()
}

pub fn call_if_defined(this: &V, name: &str, args: &[V]) -> R {
    if defines(this, name, args.len()) { call(this, name, args) } else { Ok(V::Null) }
}

/// An index into something that is no array, with the Java calls that led to it under `JRT_TRACE_MISSING=1`.
fn not_array(value: &V) -> V {
    if std::env::var("JRT_TRACE_MISSING").is_ok_and(|v| v == "1") {
        let frames: Vec<String> = stack_watch::frames().into_iter().take(30).map(|(c, m)| format!("{c}.{m}")).collect();
        trace_line(&format!("NOTARRAY {} at {}", describe(value), frames.join(" < ")));
    }
    exc("ClassCastException", "not an array")
}

/// A field read or written on null, with the Java calls that led to it under `JRT_TRACE_MISSING=1`.
fn null_field(verb: &str, field: &str) -> V {
    if std::env::var("JRT_TRACE_MISSING").is_ok_and(|v| v == "1") {
        let frames: Vec<String> = stack_watch::frames().into_iter().take(30).map(|(c, m)| format!("{c}.{m}")).collect();
        trace_line(&format!("NULL {verb} {field} at {}", frames.join(" < ")));
    }
    exc("NullPointerException", format!("{verb} {field} of null"))
}

fn no_method(on: &str, name: &str) -> R {
    // `JRT_TRACE_MISSING=1`: each "not converted" with the Java calls that led to it, in jrt-trace.txt.
    if std::env::var("JRT_TRACE_MISSING").is_ok_and(|v| v == "1") {
        let frames: Vec<String> = stack_watch::frames().into_iter().take(30).map(|(c, m)| format!("{c}.{m}")).collect();
        trace_line(&format!("MISSING {on}.{name} at {}", frames.join(" < ")));
    }
    Err(exc("NoSuchMethodError", format!("{on}.{name} is not converted")))
}

/// A game method's varargs as the decompiler writes them, `addItem(new ItemStack[]{rod})`: the
/// array's items as arguments, as javac passes them; or, for a method the SDK takes one at a time
/// (`addItem(ItemStack)`), each item in turn, the last answer kept.
fn spread_varargs(this: &V, name: &str, args: &[V]) -> Option<R> {
    let (V::Arr(last), fixed) = args.split_last()? else { return None };
    let items = lock(last).clone();
    let mut spread = fixed.to_vec();
    spread.extend(items.iter().cloned());
    if let Some(found) = hooks().and_then(|h| (h.call)(this, name, &spread)) {
        return Some(found);
    }
    if items.len() < 2 {
        return None;
    }
    let mut answer = None;
    for item in items {
        let mut one = fixed.to_vec();
        one.push(item);
        match hooks().and_then(|h| (h.call)(this, name, &one))? {
            Ok(value) => answer = Some(value),
            Err(error) => return Some(Err(error)),
        }
    }
    answer.map(Ok)
}

/// `Foo::bar` of a class outside the plugin: a static method, or an instance method of the first argument.
pub fn static_or_instance(class: &str, name: &str, args: &[V]) -> R {
    if let Some(found) = builtin_static(class, name, args).or_else(|| hooks().and_then(|h| (h.call_static)(class, name, args))) {
        return found;
    }
    match args.split_first() {
        Some((first, rest)) => call(first, name, rest),
        None => no_method(class, name),
    }
}

pub fn call_static(class: &str, name: &str, args: &[V]) -> R {
    builtin_static(class, name, args).or_else(|| hooks().and_then(|h| (h.call_static)(class, name, args))).unwrap_or_else(|| match (name, args) {
        // `SomeEnum.valueOf("RED")` of a library's enum nothing here knows: enums travel as their names.
        ("valueOf", [V::Str(text)]) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') => Ok(V::Str(text.clone())),
        _ => ELSEWHERE.get().and_then(|(elsewhere, _)| elsewhere(class, name, args)).unwrap_or_else(|| no_method(class, name)),
    })
}

/// A static the plugin's class doesn't define but inherits from a game or library class it extends:
/// `Main.getPlugin(Main.class)` is `JavaPlugin.getPlugin`. None when no type above has it either.
pub fn inherited_static(class: &'static Class, name: &str, args: &[V]) -> Option<R> {
    let mut at = Some(class);
    while let Some(k) = at {
        for kind in k.types.iter().skip(1) {
            if let Some(found) = builtin_static(kind, name, args).or_else(|| hooks().and_then(|h| (h.call_static)(kind, name, args))) {
                return Some(found);
            }
        }
        at = k.parent.map(|p| p());
    }
    None
}

/// `invokespecial`: `super.x()`.
/// `Iface.super.m(args)`: the default `m` of the plugin's interface `Iface` (or of one it extends), on `this`.
pub fn call_default(interface: &'static Class, this: &V, name: &str, args: &[V]) -> R {
    match method_of(interface, name, args) {
        Some(method) => method(this, args),
        None => builtin_method(this, name, args).unwrap_or(Ok(V::Null)),
    }
}

pub fn call_super(class: &'static Class, this: &V, name: &str, args: &[V]) -> R {
    if let Some(parent) = class.parent {
        if let Some(method) = method_of(parent(), name, args) {
            return method(this, args);
        }
    }
    if let V::Obj(object) = this {
        let native = lock(&object.fields).get("__native").cloned();
        if let Some(native) = native {
            return call(&native, name, args);
        }
        // `super.get(i)` in Guava's `Forwarding*`: the wrapped collection.
        if let Some(delegate) = method_of(object.class, "delegate", &[]) {
            return call(&delegate(this, &[])?, name, args);
        }
        // `java.lang.Object`'s own, for a class whose parent is Object (snakeyaml-engine's `Node.hashCode()` is
        // `super.hashCode()`): identity, as Java's.
        match (name, args.len()) {
            ("hashCode", 0) => return Ok(V::Int((Arc::as_ptr(object) as usize >> 4) as i32)),
            ("equals", 1) => return Ok(V::Bool(same(this, &args[0]))),
            // `Object.clone()` (a Cloneable class's `super.clone()`): a shallow copy, the same class and field values.
            // It answered null: FAWE's `region.clone()` gave null and every edit of 2+ chunks stopped.
            ("clone", 0) => return Ok(V::Obj(Arc::new(Object { class: object.class, fields: Mutex::new(lock(&object.fields).clone()) }))),
            ("toString", 0) if instance_of(this, "Throwable") => return Ok(s(&throwable_text(this))),
            ("toString", 0) => return Ok(s(&format!("{}@{:x}", java_class_name(object.class.name), (Arc::as_ptr(object) as usize >> 4) as u32))),
            _ => {}
        }
    }
    builtin_method(this, name, args).unwrap_or(Ok(V::Null))
}

/// The plugin's classes that have made an object, by name: what `obj.getClass().getDeclaredMethod`
/// looks in.
static MADE: std::sync::RwLock<BTreeMap<&'static str, &'static Class>> = std::sync::RwLock::new(BTreeMap::new());

pub fn class_named(name: &str) -> Option<&'static Class> {
    // The read lock goes before the plugin's own lookup: that can run the class's static initialiser, whose
    // `new` takes the write lock (AntiMap's packetevents hung its server's start on it).
    let made = MADE.read().ok()?.get(name).copied();
    made.or_else(|| CLASSES.get().and_then(|find| find(name)))
}

/// Every class of the plugin by its full or simple name, made or not: what `Class.forName("a.b.Foo")` and
/// `getConstructor(..).newInstance(..)` find (a jar-in-jar loader's bootstrap, a class picked by config).
static CLASSES: OnceLock<fn(&str) -> Option<&'static Class>> = OnceLock::new();

pub fn set_classes(find: fn(&str) -> Option<&'static Class>) {
    let _ = CLASSES.set(find);
}

/// A plugin class by the name Java gave it (`a.b.Foo`, `a.b.Outer$Inner` or `Foo`).
pub fn plugin_class(name: &str) -> Option<&'static Class> {
    let find = CLASSES.get()?;
    find(name).or_else(|| find(name.rsplit(['.', '$']).next().unwrap_or(name)))
}

/// `new` of a class of the plugin.
pub fn new_obj(class: &'static Class, args: &[V], hidden: &[(&'static str, V)]) -> R {
    if MADE.read().is_ok_and(|made| !made.contains_key(class.name)) {
        if let Ok(mut made) = MADE.write() {
            made.insert(class.name, class);
        }
    }
    let object = V::Obj(Arc::new(Object { class, fields: Mutex::new(hidden.iter().cloned().collect()) }));
    match (class.find)("<init>", args) {
        Some(init) => {
            guarded(class.name, "<init>", || init(&object, args))?;
        }
        // `new PacketAdapter(plugin, priority, types) { ... }`: an anonymous subclass hands its arguments to
        // the parent it extends from another library.
        None if !args.is_empty() && class.parent.is_none() && class.types.len() > 1 => init_base(&object, class.types[class.types.len() - 1], args),
        None if !args.is_empty() => return Err(exc("NoSuchMethodError", format!("{} has no constructor for {} arguments", class.name, args.len()))),
        None => {}
    }
    Ok(object)
}

/// `super(...)` of a class from Java itself: `class X extends HashMap`. What X doesn't define, it answers.
pub fn init_base(this: &V, base: &str, args: &[V]) {
    // Game parents the bridge keeps (a plugin's own `Command` class) come after Java's own.
    // SnakeYAML's representers and constructors too (WorldEdit's `FancyRepresenter extends Representer`), whose
    // setters the plugin's subclass calls on itself.
    let game = || matches!(base, "Command" | "BukkitCommand" | "PacketAdapter" | "PlaceholderExpansion" | "Representer" | "SafeRepresenter" | "BaseRepresenter" | "Constructor" | "SafeConstructor" | "BaseConstructor" | "YamlRepresenter" | "YamlConstructor").then(|| hooks().and_then(|h| (h.construct)(base, args))).flatten();
    if let Some(Ok(parent)) = new_builtin(base, args).or_else(game) {
        let _ = this.set("__native", parent);
    }
}

/// `new` of a class that isn't the plugin's.
pub fn new_other(class: &str, args: &[V]) -> R {
    let simple = class.rsplit('.').next().unwrap_or(class);
    new_builtin(class, args)
        .or_else(|| hooks().and_then(|h| (h.construct)(class, args)))
        .or_else(|| (simple != class).then(|| hooks().and_then(|h| (h.construct)(simple, args))).flatten())
        .or_else(|| ELSEWHERE.get().and_then(|(_, construct)| construct(class, args)))
        .unwrap_or_else(|| no_method(class, "<init>"))
}

/// What Java declares an `Iterator`: the SDK answers some (`recipeIterator()`) with the whole list.
pub fn as_iterator(value: V) -> V {
    match value {
        V::List(_) | V::Set(_) | V::Arr(_) => call(&value, "iterator", &[]).unwrap_or(value),
        other => other,
    }
}

/// A field of a class that isn't the plugin's: `Material.STONE`.
pub fn other_field(class: &str, name: &str) -> V {
    stdlib::builtin_field(class, name).or_else(|| hooks().and_then(|h| (h.field)(class, name))).unwrap_or_else(|| s(name))
}

pub fn instance_of(value: &V, name: &str) -> bool {
    match value {
        V::Null => false,
        V::Obj(object) => {
            if has_type(object.class, name) {
                return true;
            }
            let parent = lock(&object.fields).get("__native").cloned();
            name == "Object" || parent.is_some_and(|p| instance_of(&p, name))
        }
        V::Native(_) => name == "Object" || views::is_collection(value, name)
            || (name == "TypeToken" && value.native::<javacore::JTypeToken>().is_some())
            // An annotation value is an `Annotation`, and an instance of its own annotation type.
            || value.native::<reflect::JAnnotation>().is_some_and(|found| name == "Annotation" || reflect::annotation_kind(found) == name.rsplit(['.', '$']).next().unwrap_or(name))
            || (matches!(name, "Table" | "ImmutableTable" | "HashBasedTable" | "TreeBasedTable" | "RowSortedTable") && value.native::<javacore::JTable>().is_some())
            || (matches!(name, "Map" | "SortedMap" | "NavigableMap" | "ImmutableSortedMap" | "ImmutableMap") && value.native::<javacore::JComparatorMap>().is_some()) || (matches!(name, "JsonPrimitive" | "JsonElement") && value.native::<javautil::JPrim>().is_some()) || (matches!(name, "File" | "Path") && value.native::<javaio::JFile>().is_some())
            // Streams, readers and writers: so `load(InputStream)` is told from `load(File)`.
            || (matches!(name, "InputStream" | "Reader" | "BufferedReader" | "InputStreamReader" | "FileInputStream" | "FileReader" | "BufferedInputStream" | "ByteArrayInputStream" | "Closeable" | "AutoCloseable" | "Readable") && value.native::<javaio::JBytes>().is_some())
            || (matches!(name, "OutputStream" | "Writer" | "FileWriter" | "BufferedWriter" | "PrintWriter" | "OutputStreamWriter" | "FileOutputStream" | "BufferedOutputStream" | "PrintStream" | "Flushable" | "Closeable" | "AutoCloseable" | "Appendable") && value.native::<javaio::JWriter>().is_some())
            || (matches!(name, "ParameterizedType" | "Type") && reflect::is_param_type(value))
            || sql::is_instance(value, name)
            || hooks().is_some_and(|h| (h.instance_of)(value, name)),
        // The game's enums travel as their names (`Material.STONE` is "STONE"): such a name is one of them.
        V::Str(text) => matches!(name, "String" | "CharSequence" | "Object" | "Comparable" | "UUID")
            || (GAME_ENUMS.contains(&name) && !text.is_empty() && text.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')),
        V::Int(_) => matches!(name, "Integer" | "Number" | "Object"),
        V::Long(_) => matches!(name, "Long" | "Number" | "Object"),
        V::Double(_) => matches!(name, "Double" | "Float" | "Number" | "Object"),
        V::Bool(_) => matches!(name, "Boolean" | "Object"),
        V::List(_) => matches!(name, "List" | "ArrayList" | "Collection" | "Iterable" | "Object" | "JsonArray" | "JsonElement" | "ImmutableList" | "LinkedList" | "Deque" | "Queue"),
        V::Map(_) => matches!(name, "Map" | "HashMap" | "LinkedHashMap" | "Object" | "JsonObject" | "JsonElement" | "ImmutableMap" | "TreeMap" | "SortedMap" | "NavigableMap" | "ConcurrentMap" | "ConcurrentHashMap"),
        V::Set(_) => matches!(name, "Set" | "HashSet" | "Collection" | "Iterable" | "Object"),
        V::Char(_) => matches!(name, "Character" | "Object" | "Comparable"),
        // `Object[].class.isInstance(x)`: arrays only (snakeyaml's representers keep `Object[]` beside `List`).
        V::Arr(_) => name.ends_with("[]") || matches!(name, "Object" | "Cloneable" | "Serializable"),
        V::Exc(thrown) => caught(&thrown.class, name),
        // `Foo.class`: a `Class`, which is a `Type` (Gson's `type instanceof Class`).
        V::Class(_) => matches!(name, "Class" | "Type" | "Object" | "GenericDeclaration" | "AnnotatedElement"),
        // A lambda is any functional interface, and nothing that holds data.
        V::Func(_) => !hooks().is_some_and(|h| h.not_lambda.contains(&name)) && !matches!(name, "Collection" | "List" | "Set" | "Map" | "Iterable" | "Queue" | "Deque" | "SortedSet" | "SortedMap" | "NavigableMap" | "String" | "CharSequence" | "Number" | "Integer" | "Long" | "Double" | "Float" | "Boolean" | "Character" | "UUID" | "Stream" | "Optional" | "Throwable" | "Exception"),
        _ => name == "Object",
    }
}

static FULL_NAMES: OnceLock<std::collections::HashMap<&'static str, &'static str>> = OnceLock::new();

/// The plugin's classes by the name each `Class` carries, with their Java names (`a.b.Outer$Inner`): what
/// `getName()` and an exception's `toString()` print. A name two classes share isn't listed (it stays short).
pub fn set_full_names(names: &'static [(&'static str, &'static str)]) {
    let _ = FULL_NAMES.set(names.iter().copied().collect());
}

/// A class's Java name: the plugin's own from its table, the JDK's by their packages, else as given.
pub fn java_class_name(name: &str) -> String {
    if name.contains('.') {
        return name.to_owned();
    }
    if let Some(full) = FULL_NAMES.get().and_then(|names| names.get(name)) {
        return (*full).to_owned();
    }
    let package = match name {
        "Exception" | "RuntimeException" | "Error" | "Throwable" | "NullPointerException" | "IllegalArgumentException" | "IllegalStateException"
        | "IndexOutOfBoundsException" | "ArrayIndexOutOfBoundsException" | "StringIndexOutOfBoundsException" | "ClassCastException"
        | "NumberFormatException" | "ArithmeticException" | "UnsupportedOperationException" | "InterruptedException" | "CloneNotSupportedException"
        | "SecurityException" | "NegativeArraySizeException" | "StackOverflowError" | "OutOfMemoryError" | "ClassNotFoundException"
        | "NoSuchFieldException" | "NoSuchMethodException" | "ReflectiveOperationException" | "IllegalAccessException" | "InstantiationException"
        | "AssertionError" | "LinkageError" | "NoSuchMethodError" | "NoClassDefFoundError" | "ExceptionInInitializerError"
        | "IllegalMonitorStateException" | "ArrayStoreException" | "String" | "Object" | "Integer" | "Long" | "Double" | "Boolean" | "Character"
        | "StringBuilder" | "Class" | "Enum" | "Number" | "Thread" | "Runnable" | "Iterable" | "Comparable" | "CharSequence" => "java.lang",
        "NoSuchElementException" | "ConcurrentModificationException" | "MissingResourceException" | "InputMismatchException" | "ArrayList"
        | "HashMap" | "LinkedHashMap" | "TreeMap" | "HashSet" | "LinkedHashSet" | "TreeSet" | "Optional" | "UUID" | "List" | "Map" | "Set" => "java.util",
        "CompletionException" | "ExecutionException" | "TimeoutException" | "CancellationException" | "RejectedExecutionException" | "CompletableFuture" => "java.util.concurrent",
        "IOException" | "FileNotFoundException" | "UncheckedIOException" | "EOFException" => "java.io",
        "MalformedURLException" | "URISyntaxException" | "UnknownHostException" | "ConnectException" | "SocketTimeoutException" | "SocketException" => "java.net",
        "SQLException" => "java.sql",
        "DateTimeException" => "java.time",
        "DateTimeParseException" => "java.time.format",
        "JsonSyntaxException" | "JsonParseException" | "JsonIOException" => "com.google.gson",
        _ => return name.to_owned(),
    };
    format!("{package}.{name}")
}

/// Java's `Throwable.toString()` of any thrown value: `a.b.FooException: message` (`getLocalizedMessage`, which a
/// plugin's exception may override through `getMessage`).
pub fn throwable_text(thrown: &V) -> String {
    match thrown {
        V::Obj(object) => {
            let message = call(thrown, "getLocalizedMessage", &[]).ok().filter(|m| !m.is_null()).map(|m| m.text());
            let name = java_class_name(object.class.name);
            message.map_or(name.clone(), |m| format!("{name}: {m}"))
        }
        other => other.text(),
    }
}

/// `x.getClass()`: the Java class each value is. A string is a `String` (snakeyaml-engine finds its representer by
/// it: `Object` sent every string to the array representer, which wrote it as a list that aliased itself).
pub fn class_of(value: &V) -> &'static str {
    match value {
        V::Obj(object) => object.class.name,
        V::Str(_) => "String",
        V::Sb(_) => "StringBuilder",
        V::Int(_) => "Integer",
        V::Long(_) => "Long",
        V::Double(_) => "Double",
        V::Bool(_) => "Boolean",
        V::Char(_) => "Character",
        V::List(_) => "ArrayList",
        V::Arr(_) => "Object[]",
        V::Map(map) => match lock(map).order {
            jmap::Order::Hash => "HashMap",
            jmap::Order::Insertion => "LinkedHashMap",
            jmap::Order::Sorted => "TreeMap",
        },
        V::Set(set) => match lock(set).order {
            jmap::Order::Hash => "HashSet",
            jmap::Order::Insertion => "LinkedHashSet",
            jmap::Order::Sorted => "TreeSet",
        },
        V::Opt(_) => "Optional",
        V::Class(_) => "Class",
        V::Exc(thrown) => intern(&thrown.class),
        _ => "Object",
    }
}

/// A name kept for the life of the server, once per name (classes are few).
pub fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<BTreeMap<String, &'static str>> = Mutex::new(BTreeMap::new());
    let mut names = lock(&NAMES);
    if let Some(&known) = names.get(name) {
        return known;
    }
    let kept: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(name.to_owned(), kept);
    kept
}

/// Bukkit's enums and enum-like registries, which a converted plugin holds as their constants' names.
pub const GAME_ENUMS: &[&str] = &[
    "Material", "Sound", "Particle", "EntityType", "Biome", "ChatColor", "GameMode", "Difficulty", "WeatherType", "TreeType",
    "DyeColor", "BarColor", "BarStyle", "BarFlag", "InventoryType", "ClickType", "DragType", "Action", "DamageCause", "SpawnReason",
    "TeleportCause", "BlockFace", "Art", "Rotation", "EquipmentSlot", "ItemFlag", "Statistic", "Attribute", "Environment", "WorldType",
    "PatternType", "Instrument", "SoundCategory", "PermissionDefault", "TimeUnit", "ChronoUnit", "EntityEffect", "FireworkEffectType",
    "PotionType", "RegainReason", "IgniteCause", "RemoveCause", "ChatMessageType", "Tone", "Profession",
    // `java.util.logging.Level.WARNING` is "WARNING": `log(Level, ComponentLike, Throwable)` must still fit it.
    "Level", "EventPriority", "ServicePriority",
];

pub fn has_type(class: &'static Class, name: &str) -> bool {
    class.types.contains(&name)
        || class.types.iter().any(|t| LIBRARY_SUPERS.iter().any(|(what, supers)| what == t && supers.contains(&name)))
        || class.parent.is_some_and(|p| has_type(p(), name))
        || class.interfaces.iter().any(|i| !std::ptr::eq(i(), class) && has_type(i(), name))
}

/// The library's types a plugin class names, and what each of them is too: `implements TabExecutor`
/// is a `CommandExecutor` (`if (x instanceof CommandExecutor) cmd.setExecutor(x)`).
const LIBRARY_SUPERS: &[(&str, &[&str])] = &[
    ("TabExecutor", &["CommandExecutor", "TabCompleter"]),
    ("JavaPlugin", &["Plugin", "PluginBase", "TabExecutor", "CommandExecutor", "TabCompleter"]),
    ("PluginBase", &["Plugin", "TabExecutor", "CommandExecutor", "TabCompleter"]),
    ("Plugin", &["TabExecutor", "CommandExecutor", "TabCompleter"]),
    ("BukkitRunnable", &["Runnable"]),
    ("BukkitCommand", &["Command"]),
];

fn caught(thrown: &str, wanted: &str) -> bool {
    const RUNTIME: &[&str] = &["CompletionException", "CancellationException", "StringIndexOutOfBoundsException", "PatternSyntaxException", "NullPointerException", "IllegalArgumentException", "IllegalStateException", "NumberFormatException", "ArithmeticException", "IndexOutOfBoundsException", "ArrayIndexOutOfBoundsException", "ClassCastException", "UnsupportedOperationException", "ConcurrentModificationException", "NoSuchElementException"];
    thrown == wanted
        || matches!(wanted, "Throwable" | "Object")
        || (wanted == "Exception" && !matches!(thrown, "NoSuchMethodError" | "StackOverflowError"))
        || (wanted == "RuntimeException" && RUNTIME.contains(&thrown))
        || (wanted == "ReflectiveOperationException" && matches!(thrown, "NoSuchMethodException" | "NoSuchFieldException" | "ClassNotFoundException" | "IllegalAccessException" | "InstantiationException" | "InvocationTargetException"))
        || (wanted == "IOException" && matches!(thrown, "FileNotFoundException" | "NoSuchFileException" | "MalformedURLException" | "UncheckedIOException" | "EOFException" | "AccessDeniedException" | "ConnectException" | "UnknownHostException" | "SocketException" | "HttpTimeoutException" | "BindException" | "MalformedInputException" | "CharacterCodingException"))
        || (wanted == "IllegalArgumentException" && matches!(thrown, "NumberFormatException" | "PatternSyntaxException"))
        || (wanted == "IndexOutOfBoundsException" && matches!(thrown, "ArrayIndexOutOfBoundsException" | "StringIndexOutOfBoundsException"))
        || (wanted == "Error" && matches!(thrown, "NoSuchMethodError"))
}

/// A `catch` clause for this exception?
pub fn catches(thrown: &V, names: &[&str]) -> bool {
    names.iter().any(|name| instance_of(thrown, name) || (matches!(thrown, V::Obj(_)) && matches!(*name, "Exception" | "Throwable")))
}

/// A call with a varargs parameter last: the fixed part, then each of the rest, or an array whole.
pub fn fits_varargs(args: &[V], types: &[&str]) -> bool {
    let Some((last, fixed)) = types.split_last() else { return false };
    if !fits(&args[..fixed.len()], fixed) {
        return false;
    }
    let rest = &args[fixed.len()..];
    let element = &last[..last.len().saturating_sub(3)];
    matches!(rest, [V::Arr(items)] if array_fits(items, element)) || rest.iter().all(|value| fits(std::slice::from_ref(value), &[element]))
}

/// An array passed for `T...`: its items must be `T`s, or `of(key, color, Component[])` took the `TextDecoration...`
/// overload (piston's ColorConfig, then a loop in Adventure's decoration).
fn array_fits(items: &Arc<Mutex<Vec<V>>>, element: &str) -> bool {
    let items = lock(items);
    items.iter().all(|item| item.is_null() || fits(std::slice::from_ref(item), &[element]))
}

/// The second try when no overload `fits`: only primitives and text are checked, any class is taken on trust.
/// Before it the guess was the first overload of that many arguments, so `prepare(conn, query)` with a String
/// took `prepare(Connection, int)` when the connection wasn't known as a `Connection` (CoreProtect's inserts).
pub fn fits_basic(args: &[V], types: &[&str]) -> bool {
    if STRICT.get() {
        return false;
    }
    args.iter().zip(types).all(|(value, name)| match *name {
        "int" | "short" | "byte" | "long" | "double" | "float" | "boolean" | "char" => fits(std::slice::from_ref(value), &[name]),
        "String" | "CharSequence" => matches!(value, V::Str(_) | V::Null | V::Sb(_)),
        "Integer" | "Short" | "Byte" | "Long" => matches!(value, V::Int(_) | V::Long(_) | V::Char(_) | V::Null),
        "Double" | "Float" => matches!(value, V::Double(_) | V::Int(_) | V::Long(_) | V::Null),
        "Boolean" => matches!(value, V::Bool(_) | V::Null),
        _ => !matches!(value, V::Int(_) | V::Long(_) | V::Double(_) | V::Bool(_) | V::Char(_)) || matches!(*name, "Object" | "Number" | "Comparable" | "Serializable" | "*") || name.len() <= 2,
    })
}

pub fn fits(args: &[V], types: &[&str]) -> bool {
    args.iter().zip(types).all(|(value, name)| match *name {
        "int" | "short" | "byte" => matches!(value, V::Int(_) | V::Char(_)),
        "long" => matches!(value, V::Int(_) | V::Long(_)),
        "double" | "float" => matches!(value, V::Double(_) | V::Int(_) | V::Long(_)),
        "boolean" => matches!(value, V::Bool(_)),
        "char" => matches!(value, V::Char(_)),
        "*" | "Object" | "Object..." => true,
        // A name the plugin and the game share (jport's `fit_type`): `^World` the game's (a game or bridge object, or
        // the plugin's main class, which stands on the game's plugin), `@World` the plugin's own (one of its objects,
        // or a lambda for a functional interface of it).
        outside if outside.starts_with('^') => match value {
            V::Null => true,
            V::Obj(object) => lock(&object.fields).contains_key("__native") && instance_of(value, &outside[1..]),
            _ => fits(std::slice::from_ref(value), &[&outside[1..]]),
        },
        own if own.starts_with('@') => match value {
            V::Null | V::Func(_) => true,
            V::Obj(_) => fits(std::slice::from_ref(value), &[&own[1..]]),
            _ => false,
        },
        // A Locale is its text here (`Locale.ENGLISH` is "en"): FAWE's `Caption.color(component, locale)` took the
        // `color(Component, List)` overload, then `size()` of the text.
        "Locale" => matches!(value, V::Str(_) | V::Null),
        // An array for `T[]` whose items are `T`s: `deserialize(char[])` is not `deserialize(int[])` (ANTLR's
        // ATN reader looped between them).
        array if array.ends_with("[]") => value.is_null() || matches!(value, V::Arr(items) if array_fits(items, &array[..array.len() - 2])),
        spread if spread.ends_with("...") => matches!(value, V::Arr(items) if array_fits(items, &spread[..spread.len() - 3])) || fits(std::slice::from_ref(value), &[&spread[..spread.len() - 3]]),
        generic if generic.len() <= 2 && generic.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) => true,
        other => value.is_null() || instance_of(value, other),
    })
}

// ---- Operators ----

fn numeric(a: &V, b: &V, int: fn(i32, i32) -> i32, long: fn(i64, i64) -> i64, double: fn(f64, f64) -> f64) -> V {
    match (a, b) {
        (V::Double(_), _) | (_, V::Double(_)) => V::Double(double(a.double(), b.double())),
        (V::Long(_), _) | (_, V::Long(_)) => V::Long(long(a.long(), b.long())),
        _ => V::Int(int(a.int(), b.int())),
    }
}

pub fn add(a: &V, b: &V) -> V {
    if matches!(a, V::Str(_) | V::Null | V::Sb(_) | V::Obj(_) | V::Native(_) | V::List(_) | V::Map(_) | V::Set(_) | V::Bool(_)) || matches!(b, V::Str(_) | V::Null | V::Sb(_) | V::Obj(_) | V::Native(_) | V::List(_) | V::Map(_) | V::Set(_) | V::Bool(_)) {
        return s(&format!("{}{}", a.text(), b.text()));
    }
    numeric(a, b, i32::wrapping_add, i64::wrapping_add, |x, y| x + y)
}

pub fn sub(a: &V, b: &V) -> V {
    numeric(a, b, i32::wrapping_sub, i64::wrapping_sub, |x, y| x - y)
}

pub fn mul(a: &V, b: &V) -> V {
    numeric(a, b, i32::wrapping_mul, i64::wrapping_mul, |x, y| x * y)
}

pub fn div(a: &V, b: &V) -> R {
    if !matches!(a, V::Double(_)) && !matches!(b, V::Double(_)) && b.long() == 0 {
        return Err(exc("ArithmeticException", "/ by zero"));
    }
    Ok(numeric(a, b, i32::wrapping_div, i64::wrapping_div, |x, y| x / y))
}

pub fn rem(a: &V, b: &V) -> R {
    if !matches!(a, V::Double(_)) && !matches!(b, V::Double(_)) && b.long() == 0 {
        return Err(exc("ArithmeticException", "/ by zero"));
    }
    Ok(numeric(a, b, i32::wrapping_rem, i64::wrapping_rem, |x, y| x % y))
}

pub fn neg(a: &V) -> V {
    match a {
        V::Double(n) => V::Double(-n),
        V::Long(n) => V::Long(n.wrapping_neg()),
        other => V::Int(other.int().wrapping_neg()),
    }
}

pub fn bits(op: &str, a: &V, b: &V) -> V {
    if let (V::Bool(x), V::Bool(y)) = (a, b) {
        return V::Bool(match op {
            "&" => *x && *y,
            "|" => *x || *y,
            _ => x != y,
        });
    }
    let long = matches!(a, V::Long(_)) || (matches!(b, V::Long(_)) && matches!(op, "&" | "|" | "^"));
    if long {
        let (x, y) = (a.long(), b.long());
        return V::Long(match op {
            "&" => x & y,
            "|" => x | y,
            "^" => x ^ y,
            "<<" => x.wrapping_shl(y as u32),
            ">>" => x.wrapping_shr(y as u32),
            _ => ((x as u64).wrapping_shr(y as u32)) as i64,
        });
    }
    let (x, y) = (a.int(), b.int());
    V::Int(match op {
        "&" => x & y,
        "|" => x | y,
        "^" => x ^ y,
        "<<" => x.wrapping_shl(y as u32),
        ">>" => x.wrapping_shr(y as u32),
        _ => ((x as u32).wrapping_shr(y as u32)) as i32,
    })
}

pub fn not(a: &V) -> V {
    match a {
        V::Long(n) => V::Long(!n),
        other => V::Int(!other.int()),
    }
}

pub fn cmp(a: &V, b: &V) -> std::cmp::Ordering {
    a.double().partial_cmp(&b.double()).unwrap_or(std::cmp::Ordering::Equal)
}

/// `==`: numbers by value, strings as a `.equals` (Java's interned literals behave the same in
/// plugins), everything else by identity.
pub fn same(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Null, _) | (_, V::Null) => false,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Str(x), V::Str(y)) => x == y,
        (V::Obj(x), V::Obj(y)) => Arc::ptr_eq(x, y),
        (V::List(x), V::List(y)) | (V::Arr(x), V::Arr(y)) => Arc::ptr_eq(x, y),
        (V::Map(x), V::Map(y)) | (V::Set(x), V::Set(y)) => Arc::ptr_eq(x, y),
        // One class loader for everything here: the plugin's is the system's.
        (V::Native(_), V::Native(_)) if a.native::<javautil::JPlain>().is_some() && b.native::<javautil::JPlain>().is_some() => true,
        (V::Native(x), V::Native(y)) => Arc::ptr_eq(x, y) || a.text() == b.text(),
        // `stack.getType() == Material.STONE`: the game hands over its value, the plugin wrote the constant,
        // which is its name here. Equal when the value's `name()` is that constant.
        (V::Native(_), V::Str(name)) | (V::Str(name), V::Native(_)) if !name.is_empty() && name.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_') => {
            let value = if matches!(a, V::Native(_)) { a } else { b };
            call(value, "name", &[]).is_ok_and(|n| matches!(&n, V::Str(t) if **t == **name))
        }
        (V::Class(x), V::Class(y)) => x == y,
        (V::Int(_) | V::Long(_) | V::Double(_) | V::Char(_), V::Int(_) | V::Long(_) | V::Double(_) | V::Char(_)) => a.double() == b.double(),
        _ => false,
    }
}

/// `case RED:`: an enum constant by name, a game constant (a string) by value.
pub fn is_constant(value: &V, name: &str) -> bool {
    match value {
        V::Obj(object) => lock(&object.fields).get("$name").is_some_and(|n| n.text() == name),
        other => other.text() == name,
    }
}

/// `.equals`.
pub fn equals(a: &V, b: &V) -> R {
    match a {
        V::Null => Err(exc("NullPointerException", "equals on null")),
        V::Obj(_) => call(a, "equals", std::slice::from_ref(b)),
        V::Map(_) | V::Set(_) => Ok(V::Bool(same(a, b) || (matches!((a, b), (V::Set(_), V::Set(_)) | (V::Map(_), V::Map(_))) && a.key() == b.key()))),
        _ => Ok(V::Bool(same(a, b) || (matches!((a, b), (V::List(_), V::List(_)) | (V::Native(_), V::Native(_))) && a.text() == b.text()))),
    }
}

/// `(int) x` and friends.
pub fn cast(value: &V, to: &str) -> V {
    match to {
        "int" => V::Int(match value {
            V::Long(n) => *n as i32,
            V::Double(n) => *n as i32,
            other => other.int(),
        }),
        "long" => V::Long(value.long()),
        "double" | "float" => V::Double(value.double()),
        "short" => V::Int(i32::from(value.long() as i16)),
        "byte" => V::Int(i32::from(value.long() as i8)),
        "char" => V::Char(char::from_u32(value.long() as u32).unwrap_or('\0')),
        _ => value.clone(),
    }
}

/// `(Foo) x`.
pub fn cast_to(value: V, to: &str) -> R {
    if value.is_null() || instance_of(&value, to) || !matches!(value, V::Obj(_)) {
        return Ok(value);
    }
    if let V::Obj(object) = &value {
        // Generic parameters erase to Object: only the plugin's classes are checked.
        if object.class.types.iter().any(|t| *t == to) {
            return Ok(value);
        }
    }
    Ok(value)
}

pub fn index(array: &V, at: &V) -> R {
    match array {
        V::Arr(items) | V::List(items) => {
            let items = lock(items);
            let at = at.int();
            usize::try_from(at).ok().and_then(|i| items.get(i).cloned()).ok_or_else(|| exc("ArrayIndexOutOfBoundsException", format!("Index {at} out of bounds for length {}", items.len())))
        }
        V::Null => Err(exc("NullPointerException", "indexing null")),
        other => Err(not_array(other)),
    }
}

pub fn store(array: &V, at: &V, value: V) -> R {
    match array {
        // An array the game handed over as a list (`inventory.getContents()`), as `index` reads it.
        V::List(items) => {
            let mut items = lock(items);
            let len = items.len();
            let at = at.int();
            let slot = usize::try_from(at).ok().and_then(|i| items.get_mut(i)).ok_or_else(|| exc("ArrayIndexOutOfBoundsException", format!("Index {at} out of bounds for length {len}")))?;
            *slot = value.clone();
            Ok(value)
        }
        V::Arr(items) => {
            let mut items = lock(items);
            let len = items.len();
            let at = at.int();
            let slot = usize::try_from(at).ok().and_then(|i| items.get_mut(i)).ok_or_else(|| exc("ArrayIndexOutOfBoundsException", format!("Index {at} out of bounds for length {len}")))?;
            // `chars[i] = 167` in a `char[]` is the character (`§`), as Java's assignment makes it; a number
            // in a `long[]`/`double[]` takes that array's kind.
            let value = match (&*slot, &value) {
                (V::Char(_), V::Int(n)) => V::Char(char::from_u32(*n as u32 & 0xffff).unwrap_or('\u{fffd}')),
                (V::Char(_), V::Long(n)) => V::Char(char::from_u32(*n as u32 & 0xffff).unwrap_or('\u{fffd}')),
                (V::Long(_), V::Int(n)) => V::Long(i64::from(*n)),
                (V::Double(_), V::Int(n)) => V::Double(f64::from(*n)),
                (V::Double(_), V::Long(n)) => V::Double(*n as f64),
                _ => value,
            };
            *slot = value.clone();
            Ok(value)
        }
        V::Null => Err(exc("NullPointerException", "storing in null")),
        other => Err(not_array(other)),
    }
}

/// The value a new array of this Java type starts filled with.
/// A field declared without a value: its default (`zero`), unless something set it already (the parent's
/// constructor, which in Java runs after every field has its default and before this class's initialisers).
pub fn default_field(this: &V, name: &'static str, zero: V) {
    if let V::Obj(object) = this {
        lock(&object.fields).entry(name).or_insert(zero);
    }
}

pub fn zero(kind: &str) -> V {
    match kind {
        "int" | "short" | "byte" => V::Int(0),
        "long" => V::Long(0),
        "double" | "float" => V::Double(0.0),
        "boolean" => V::Bool(false),
        "char" => V::Char('\0'),
        _ => V::Null,
    }
}

pub fn new_array(kind: &str, sizes: &[V]) -> V {
    let Some((first, rest)) = sizes.split_first() else { return V::Null };
    let n = first.int().max(0) as usize;
    if rest.is_empty() {
        arr(vec![zero(kind); n])
    } else {
        arr((0..n).map(|_| new_array(kind, rest)).collect())
    }
}

static PUMP: OnceLock<fn()> = OnceLock::new();

/// What a thread does while it waits on another, so that the other can finish.
pub fn set_pump(pump: fn()) {
    let _ = PUMP.set(pump);
}

pub(crate) fn pump() {
    if let Some(pump) = PUMP.get() {
        pump();
    }
}

/// Where the last exception came from, for a logger.
pub fn take_trace() -> String {
    TRACE.with_borrow_mut(std::mem::take).join(" < ")
}

static REPORTER: OnceLock<fn(&str)> = OnceLock::new();

/// Where an exception nothing caught is written.
pub fn set_reporter(reporter: fn(&str)) {
    let _ = REPORTER.set(reporter);
}

/// What makes a Rust panic in a converted plugin behave like a Java exception reaching the server.
pub fn report(where_: &str, result: R) -> V {
    match result {
        Ok(value) => value,
        Err(thrown) => {
            let trace = TRACE.with_borrow_mut(std::mem::take).join(" < ");
            let line = format!("{where_}: {} at {trace}", thrown.text());
            match REPORTER.get() {
                Some(reporter) => reporter(&line),
                None => eprintln!("[plugin] {line}"),
            }
            V::Null
        }
    }
}
