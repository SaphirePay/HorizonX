//! The parts of `java.lang` and `java.util` plugins lean on. A name in `KNOWN_METHODS` or `KNOWN_STATICS`
//! counts as converted in the compiler's report, so add it there when adding it here.
use super::*;

pub const KNOWN_METHODS: &[&str] = &[
    "length", "charAt", "isEmpty", "isBlank", "equals", "equalsIgnoreCase", "toLowerCase", "toUpperCase", "trim", "strip", "startsWith", "endsWith", "contains", "indexOf", "lastIndexOf", "substring", "replace", "replaceAll", "replaceFirst", "split", "concat", "hashCode", "compareTo", "compareToIgnoreCase", "matches", "toString", "toCharArray", "repeat", "chars",
    "add", "addAll", "get", "set", "remove", "removeAll", "retainAll", "removeIf", "size", "clear", "containsAll", "iterator", "stream", "forEach", "sort", "toArray", "subList", "getFirst", "getLast", "addFirst", "addLast", "reversed",
    "put", "putAll", "containsKey", "containsValue", "keySet", "values", "entrySet", "getOrDefault", "putIfAbsent", "computeIfAbsent", "computeIfPresent", "compute", "merge", "getKey", "getValue", 
    "hasNext", "next", "append", "insert", "reverse", "deleteCharAt", "setLength", "setCharAt",
    "map", "filter", "collect", "toList", "anyMatch", "allMatch", "noneMatch", "findFirst", "findAny", "count", "sorted", "distinct", "limit", "skip", "mapToInt", "mapToObj", "mapToLong", "mapToDouble", "boxed", "sum", "max", "min", "flatMap", "peek", "reduce", "average", "getAsInt", "getAsDouble",
    "isPresent", "ifPresent", "ifPresentOrElse", "orElse", "orElseGet", "orElseThrow", "intValue", "longValue", "doubleValue", "floatValue", "shortValue", "byteValue", "booleanValue", "charValue",
    "getMessage", "getLocalizedMessage", "printStackTrace", "getCause", "getClass", "getName", "getSimpleName", "name", "ordinal", "run", "accept", "apply", "test", "call",
];

pub const KNOWN_STATICS: &[&str] = &[
    "Math.max", "Math.min", "Math.abs", "Math.floor", "Math.ceil", "Math.round", "Math.sqrt", "Math.pow", "Math.random", "Math.sin", "Math.cos", "Math.tan", "Math.atan2", "Math.toRadians", "Math.toDegrees", "Math.floorMod", "Math.floorDiv", "Math.clamp", "Math.log", "Math.exp",
    "Integer.parseInt", "Integer.valueOf", "Integer.toString", "Integer.compare", "Integer.max", "Integer.min", "Integer.sum", "Long.parseLong", "Long.valueOf", "Long.toString", "Long.compare", "Double.parseDouble", "Double.valueOf", "Double.toString", "Double.compare", "Double.isNaN", "Float.parseFloat", "Float.valueOf", "Boolean.parseBoolean", "Boolean.valueOf", "Boolean.toString", "Character.isDigit", "Character.isLetter", "Character.isLetterOrDigit", "Character.isWhitespace", "Character.toLowerCase", "Character.toUpperCase", "Character.valueOf", "Character.toString",
    "String.valueOf", "String.join", "String.format", "String.copyValueOf", "System.currentTimeMillis", "System.nanoTime", "System.getProperty", "System.getenv", "System.lineSeparator", "System.identityHashCode", "System.arraycopy",
    "Arrays.asList", "Arrays.stream", "Arrays.copyOf", "Arrays.copyOfRange", "Arrays.fill", "Arrays.sort", "Arrays.toString", "List.of", "List.copyOf", "Set.of", "Set.copyOf", "Map.of", "Map.entry", "Map.copyOf", "Collections.emptyList", "Collections.emptyMap", "Collections.emptySet", "Collections.singletonList", "Collections.singleton", "Collections.unmodifiableList", "Collections.unmodifiableMap", "Collections.unmodifiableSet", "Collections.unmodifiableCollection", "Collections.sort", "Collections.reverse", "Collections.shuffle", "Collections.addAll", "Objects.equals", "Objects.hash", "Objects.hashCode", "Objects.requireNonNull", "Objects.requireNonNullElse", "Objects.isNull", "Objects.nonNull", "Objects.toString", "Optional.of", "Optional.ofNullable", "Optional.empty", "UUID.fromString", "UUID.randomUUID", "UUID.nameUUIDFromBytes", "Collectors.toList", "Collectors.toSet", "Collectors.joining", "Stream.of", "IntStream.range", "IntStream.rangeClosed", "Thread.sleep", "Thread.currentThread",
];

pub fn builtin_field(class: &str, name: &str) -> Option<V> {
    if let Some(found) = crate::javacore::field(class, name).or_else(|| crate::okhttp::field(class, name)).or_else(|| crate::javamore::field(class, name)).or_else(|| crate::bignum::field(class, name)).or_else(|| crate::javaio::field(class, name)).or_else(|| crate::jtime::field(class, name)).or_else(|| crate::javautil::field(class, name)) {
        return Some(found);
    }
    Some(match (class, name) {
        ("Integer", "MAX_VALUE") => V::Int(i32::MAX),
        ("Integer", "MIN_VALUE") => V::Int(i32::MIN),
        ("Long", "MAX_VALUE") => V::Long(i64::MAX),
        ("Long", "MIN_VALUE") => V::Long(i64::MIN),
        ("Double", "MAX_VALUE") => V::Double(f64::MAX),
        ("Math", "PI") => V::Double(std::f64::consts::PI),
        ("Math", "E") => V::Double(std::f64::consts::E),
        ("Boolean", "TRUE") => V::Bool(true),
        ("Boolean", "FALSE") => V::Bool(false),
        ("String", "CASE_INSENSITIVE_ORDER") => func(|a| Ok(V::Int(match (a.first(), a.get(1)) {
            (Some(x), Some(y)) => x.text().to_lowercase().cmp(&y.text().to_lowercase()) as i32,
            _ => 0,
        }))),
        _ => return None,
    })
}

const FASTUTIL_KINDS: [&str; 10] = ["Int", "Long", "Double", "Float", "Short", "Byte", "Char", "Boolean", "Object", "Reference"];

/// fastutil's collections, which Paper ships (`IntArrayList`, `Int2ObjectOpenHashMap`, `ObjectOpenHashSet`...):
/// "list", "map" or "set", as the runtime's own collections stand in for them.
fn fastutil_kind(class: &str) -> Option<&'static str> {
    let class = class.rsplit('.').next().unwrap_or(class);
    let kind = FASTUTIL_KINDS.iter().find(|k| class.starts_with(*k))?;
    let rest = &class[kind.len()..];
    if let Some(map) = rest.strip_prefix('2') {
        let value = FASTUTIL_KINDS.iter().find(|k| map.starts_with(*k))?;
        let shape = &map[value.len()..];
        return ["OpenHashMap", "LinkedOpenHashMap", "AVLTreeMap", "RBTreeMap", "ArrayMap", "OpenCustomHashMap", "Map"].contains(&shape).then_some("map");
    }
    if ["ArrayList", "BigArrayList", "LinkedList", "ImmutableList"].contains(&rest) {
        return Some("list");
    }
    ["OpenHashSet", "LinkedOpenHashSet", "AVLTreeSet", "RBTreeSet", "ArraySet", "OpenCustomHashSet"].contains(&rest).then_some("set")
}

/// fastutil's names for what the runtime's collections already do: `getInt(i)` is `get(i)`, `addTo(k, n)`
/// adds to a count, `toIntArray()` is `toArray()`. A primitive map gives 0 for a missing key, as fastutil's.
fn fastutil_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if !matches!(this, V::List(_) | V::Map(_) | V::Set(_)) {
        return None;
    }
    let primitive = ["Int", "Long", "Double", "Float", "Short", "Byte", "Char", "Boolean"];
    let typed = |prefix: &str| name.strip_prefix(prefix).is_some_and(|rest| primitive.contains(&rest));
    let zero = || match name.trim_start_matches(|c: char| c.is_ascii_lowercase()) {
        "Long" => V::Long(0),
        "Double" | "Float" => V::Double(0.0),
        "Boolean" => V::Bool(false),
        _ => V::Int(0),
    };
    if typed("get") && a.len() == 1 {
        return Some(crate::call(this, "get", a).map(|v| if v.is_null() { zero() } else { v }));
    }
    if typed("remove") && a.len() == 1 {
        return Some(crate::call(this, "remove", a).map(|v| if v.is_null() { zero() } else { v }));
    }
    if name.starts_with("to") && name.ends_with("Array") && primitive.contains(&&name[2..name.len() - 5]) {
        return Some(crate::call(this, "toArray", &[]));
    }
    if name.ends_with("Iterator") && primitive.iter().any(|p| name == format!("{}Iterator", p.to_lowercase())) {
        return Some(crate::call(this, "iterator", &[]));
    }
    match (this, name, a.len()) {
        (V::Map(_), "addTo", 2) => {
            let old = crate::call(this, "get", &a[..1]).ok().filter(|v| !v.is_null()).unwrap_or(V::Int(0));
            let sum = crate::add(&old, &a[1]);
            let _ = crate::call(this, "put", &[a[0].clone(), sum]);
            Some(Ok(old))
        }
        (V::Map(_), "defaultReturnValue", 1) => Some(Ok(V::Null)),
        (V::Map(_), n, 0) if n.ends_with("EntrySet") => Some(crate::call(this, "entrySet", &[])),
        _ => None,
    }
}

pub fn new_builtin(class: &str, args: &[V]) -> Option<R> {
    match fastutil_kind(class) {
        Some("list") => return new_builtin("ArrayList", args.first().filter(|a| !matches!(a, V::Int(_))).map(std::slice::from_ref).unwrap_or(&[])),
        Some("map") => return new_builtin("HashMap", args.first().filter(|a| matches!(a, V::Map(_))).map(std::slice::from_ref).unwrap_or(&[])),
        Some("set") => return new_builtin("HashSet", args.first().filter(|a| !matches!(a, V::Int(_))).map(std::slice::from_ref).unwrap_or(&[])),
        _ => {}
    }
    if let Some(found) = crate::javacore::construct(class, args).or_else(|| crate::okhttp::construct(class, args)).or_else(|| crate::datautil::construct(class, args)).or_else(|| crate::javamore::construct(class, args)).or_else(|| crate::awt::construct(class, args)).or_else(|| crate::bignum::construct(class, args)).or_else(|| crate::views::construct(class, args)).or_else(|| crate::javaio::construct(class, args)).or_else(|| crate::javautil::construct(class, args)).or_else(|| crate::configurate::construct(class, args)).or_else(|| crate::sql::construct(class, args)).or_else(|| crate::text::construct(class, args)).or_else(|| crate::props::construct(class, args)).or_else(|| crate::net::construct(class, args)).or_else(|| crate::zip::construct(class, args)) {
        return Some(found);
    }
    let from = |a: &V| a.items().map(|items| items);
    Some(match class {
        "ArrayList" | "LinkedList" | "Vector" | "CopyOnWriteArrayList" | "ArrayDeque" | "Stack" => match args.first() {
            Some(V::Int(_)) | None => Ok(list(Vec::new())),
            Some(other) => from(other).map(list),
        },
        "HashMap" | "LinkedHashMap" | "TreeMap" | "ConcurrentHashMap" | "EnumMap" | "WeakHashMap" | "IdentityHashMap" | "ConcurrentSkipListMap" | "Hashtable" => {
            let order = map_order(class);
            let map = match args.first() {
                Some(V::Map(other)) => lock(other).copy_as(order),
                // `new HashMap<>(n)`: Java's table for n; `new ConcurrentHashMap<>(n)` makes room for 1.5 n.
                Some(V::Int(n)) if order == Order::Hash => JMap::hashed(match class {
                    "ConcurrentHashMap" => crate::jmap::table_size_for(i64::from(*n) + i64::from(*n >> 1) + 1),
                    _ => crate::jmap::table_size_for(i64::from(*n)),
                }),
                _ => JMap::with_order(order),
            };
            Ok(V::Map(Arc::new(Mutex::new(map))))
        }
        "HashSet" | "LinkedHashSet" | "TreeSet" | "ConcurrentSkipListSet" => {
            let order = map_order(class);
            let mut map = match args.first() {
                // `new HashSet<>(n)`.
                Some(V::Int(n)) if order == Order::Hash => JMap::hashed(crate::jmap::table_size_for(i64::from(*n))),
                // `new HashSet<>(collection)`: Java's `max(size / .75 + 1, 16)`.
                Some(other) if order == Order::Hash && !matches!(other, V::Func(_)) => {
                    let size = other.items().map_or(0, |i| i.len());
                    JMap::hashed(crate::jmap::table_size_for(((size as f64 / 0.75) as i64 + 1).max(16)))
                }
                _ => JMap::with_order(order),
            };
            // `new TreeSet(comparator)`: an ordering, not items to copy.
            let comparator = |a: &V| matches!(a, V::Func(_) | V::Str(_)) || (matches!(a, V::Obj(_)) && defines(a, "compare", 2));
            if let Some(other) = args.first().filter(|a| !matches!(a, V::Int(_)) && !comparator(a)) {
                match from(other) {
                    Ok(items) => {
                        for item in items {
                            map.insert(item.key(), (item, V::Null));
                        }
                    }
                    Err(e) => return Some(Err(e)),
                }
            }
            Ok(V::Set(Arc::new(Mutex::new(map))))
        }
        "StringBuilder" | "StringBuffer" => Ok(V::Sb(Arc::new(Mutex::new(args.first().filter(|a| !matches!(a, V::Int(_))).map_or_else(String::new, V::text))))),
        "Object" => Ok(native(())),
        "String" => Ok(s(&args.first().map_or_else(String::new, V::text))),
        "Random" | "SecureRandom" => Ok(native(Rng(Mutex::new(time_nanos() | 1)))),
        "MessageFormat" => Ok(native(JMessageFormat(args.first().map(V::text).unwrap_or_default()))),
        // `WeakReference` / `SoftReference`: held as long as the reference is (WorldEdit's BukkitWorld keeps its world so).
        // `LongAdder` / `DoubleAdder`: a sum several threads add to (FAWE's cache counts its pools with them).
        "LongAdder" | "DoubleAdder" => Ok(native(JAdder(Mutex::new(if class == "LongAdder" { V::Long(0) } else { V::Double(0.0) })))),
        "WeakReference" | "SoftReference" | "PhantomReference" => Ok(native(JRef(Mutex::new(args.first().cloned().unwrap_or(V::Null))))),
        // Each starts as Java's: `new AtomicBoolean()` is false, not 0 (WorldEdit's `initialized.compareAndSet(false,
        // true)` failed against an Int 0, so WorldEdit never counted itself initialised), a reference null, a long 0L.
        "AtomicInteger" | "AtomicLong" | "AtomicBoolean" | "AtomicReference" => Ok(native(Cell(Mutex::new(args.first().cloned().unwrap_or(match class {
            "AtomicBoolean" => V::Bool(false),
            "AtomicReference" => V::Null,
            "AtomicLong" => V::Long(0),
            _ => V::Int(0),
        }))))),
        name if name.ends_with("Exception") || name.ends_with("Error") || name == "Throwable" => Ok(V::Exc(Arc::new(Thrown { class: name.to_owned(), message: args.first().filter(|a| matches!(a, V::Str(_))).map(V::text) , cause: args.iter().find(|a| matches!(a, V::Exc(_)) || crate::instance_of(a, "Throwable")).cloned() , trace: crate::stack_watch::frames() }))),
        _ => return None,
    })
}

/// `java.util.Random`, near enough: plugins use it for dice, not for reproducing a seed.
pub struct Rng(pub Mutex<u64>);
/// The `java.util.concurrent.atomic` boxes.
pub struct Cell(pub Mutex<V>);

fn time_nanos() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
}

fn next_random(state: &Mutex<u64>) -> u64 {
    let mut x = lock(state);
    *x ^= *x << 13;
    *x ^= *x >> 7;
    *x ^= *x << 17;
    *x
}

fn callf(f: &V, args: &[V]) -> R {
    call(f, "apply", args)
}

/// `LongAdder` / `DoubleAdder`.
pub struct JAdder(Mutex<V>);

/// Guava's `ComparisonChain`: the first comparison that isn't equal decides.
pub struct JComparisonChain(Mutex<i32>);

/// `java.lang.ref.Reference`: the value until `clear()`.
pub struct JRef(Mutex<V>);

/// Guava's `MoreObjects.ToStringHelper`: (class name, the parts added, omit nulls).
pub struct JToStringHelper(Mutex<(String, Vec<(Option<String>, V)>, bool)>);

/// `java.text.MessageFormat`: a pattern whose `{0}`, `{1}` ... take the arguments.
pub struct JMessageFormat(String);

/// `{n}` (or `{n,number}` and the like: the argument as text) replaced by argument n; `'text'` kept as written,
/// `''` a single quote, as MessageFormat reads its pattern.
fn message_format(pattern: &str, args: &[V]) -> String {
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\'' => {
                if chars.get(i + 1) == Some(&'\'') {
                    out.push('\'');
                    i += 2;
                    continue;
                }
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    out.push(chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '{' => {
                let end = chars[i..].iter().position(|c| *c == '}').map(|n| i + n);
                let inner: String = end.map(|e| chars[i + 1..e].iter().collect()).unwrap_or_default();
                match (end, inner.split(',').next().and_then(|n| n.trim().parse::<usize>().ok())) {
                    (Some(e), Some(n)) => {
                        out.push_str(&args.get(n).map_or_else(|| format!("{{{n}}}"), V::text));
                        i = e + 1;
                    }
                    _ => {
                        out.push('{');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn opt(value: Option<V>) -> V {
    V::Opt(Arc::new(value.filter(|v| !v.is_null())))
}

/// Which order a Java map or set class walks in.
pub(crate) fn map_order(class: &str) -> Order {
    match class.rsplit('.').next().unwrap_or(class) {
        "HashMap" | "ConcurrentHashMap" | "WeakHashMap" | "Hashtable" | "HashSet" | "Object2ObjectOpenHashMap" => Order::Hash,
        "TreeMap" | "TreeSet" | "ConcurrentSkipListMap" | "ConcurrentSkipListSet" | "EnumMap" => Order::Sorted,
        _ => Order::Insertion,
    }
}

fn java_hash(text: &str) -> i32 {
    text.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
}

fn str_method(t: &str, name: &str, a: &[V]) -> Option<R> {
    // `indexOf(46)`: an int is a character's code, as Java's `indexOf(int ch)` takes it.
    let arg = |i: usize| match a.get(i) {
        Some(V::Int(code)) if matches!(name, "indexOf" | "lastIndexOf") && i == 0 => char::from_u32(*code as u32).map(String::from).unwrap_or_default(),
        other => other.map(V::text).unwrap_or_default(),
    };
    let chars = || t.chars().collect::<Vec<_>>();
    Some(Ok(match (name, a.len()) {
        ("length", 0) => V::Int(t.encode_utf16().count() as i32),
        ("isEmpty", 0) => V::Bool(t.is_empty()),
        ("isBlank", 0) => V::Bool(t.trim().is_empty()),
        ("charAt", 1) => {
            let i = a[0].int();
            return Some(usize::try_from(i).ok().and_then(|i| chars().get(i).copied()).map(V::Char).ok_or_else(|| exc("StringIndexOutOfBoundsException", format!("index {i}"))));
        }
        ("equals", 1) => V::Bool(matches!(&a[0], V::Str(o) if &**o == t)),
        ("equalsIgnoreCase", 1) => V::Bool(matches!(&a[0], V::Str(o) if o.to_lowercase() == t.to_lowercase())),
        ("toLowerCase", _) => s(&t.to_lowercase()),
        ("toUpperCase", _) => s(&t.to_uppercase()),
        ("trim" | "strip", 0) => s(t.trim()),
        // `intern()`: the JVM's one copy of an equal string; any equal string is as good here (WorldEdit's ids).
        ("intern", 0) => s(t),
        ("stripLeading", 0) => s(t.trim_start()),
        ("stripTrailing", 0) => s(t.trim_end()),
        ("startsWith", 1) => V::Bool(t.starts_with(&arg(0))),
        ("endsWith", 1) => V::Bool(t.ends_with(&arg(0))),
        ("contains", 1) => V::Bool(t.contains(&arg(0))),
        ("indexOf", 1) => V::Int(t.find(&arg(0)).map_or(-1, |at| t[..at].chars().count() as i32)),
        ("lastIndexOf", 1) => V::Int(t.rfind(&arg(0)).map_or(-1, |at| t[..at].chars().count() as i32)),
        // `indexOf(text or char, from)` / `lastIndexOf(text or char, from)`: counted in characters, as Java.
        ("indexOf" | "lastIndexOf", 2) => {
            let wanted = arg(0);
            let chars: Vec<char> = t.chars().collect();
            let from = a[1].int();
            let starts = |i: usize| chars[i..].iter().collect::<String>().starts_with(&wanted);
            let found = if name == "indexOf" {
                (from.max(0) as usize..chars.len()).find(|&i| starts(i))
            } else {
                (0..=(from.min(chars.len() as i32 - 1)).max(-1) as isize).rev().map(|i| i as usize).find(|&i| i < chars.len() && starts(i))
            };
            V::Int(found.map_or(-1, |i| i as i32))
        }
        ("substring", 1 | 2) => {
            let c = chars();
            let (from, to) = (a[0].int(), a.get(1).map_or(c.len() as i32, V::int));
            if from < 0 || to > c.len() as i32 || from > to {
                return Some(Err(exc("StringIndexOutOfBoundsException", format!("begin {from}, end {to}, length {}", c.len()))));
            }
            s(&c[from as usize..to as usize].iter().collect::<String>())
        }
        ("replace", 2) => s(&t.replace(&arg(0), &arg(1))),
        ("replaceAll" | "replaceFirst", 2) => s(&t.replace(&arg(0), &arg(1))),
        ("split", 1 | 2) => {
            let sep = arg(0);
            let sep = sep.strip_prefix('\\').unwrap_or(&sep);
            let mut parts: Vec<&str> = if sep == "\\s+" || sep == "s+" { t.split_whitespace().collect() } else { t.split(sep).collect() };
            while parts.last() == Some(&"") {
                parts.pop();
            }
            arr(parts.into_iter().map(s).collect())
        }
        ("concat", 1) => s(&format!("{t}{}", arg(0))),
        ("hashCode", 0) => V::Int(java_hash(t)),
        ("compareTo", 1) => V::Int(t.cmp(arg(0).as_str()) as i32),
        ("compareToIgnoreCase", 1) => V::Int(t.to_lowercase().cmp(&arg(0).to_lowercase()) as i32),
        ("toString", 0) => s(t),
        ("toCharArray", 0) => arr(t.chars().map(V::Char).collect()),
        ("repeat", 1) => s(&t.repeat(a[0].int().max(0) as usize)),
        ("formatted", _) => {
            let args = match a { [one @ V::Arr(_)] => one.items().unwrap_or_default(), many => many.to_vec() };
            s(&java_format(t, &args))
        }
        ("toURL" | "toURI" | "toExternalForm", 0) => s(t),
        // Code points: each character one (a `V::Char` holds any character).
        ("codePointAt" | "codePointBefore", 1) => {
            let at = a[0].int() - i32::from(name == "codePointBefore");
            return Some(usize::try_from(at).ok().and_then(|i| chars().get(i).copied()).map(|c| V::Int(c as i32)).ok_or_else(|| exc("StringIndexOutOfBoundsException", format!("index {at}, length {}", chars().len()))));
        }
        ("codePointCount", 2) => V::Int((a[1].int() - a[0].int()).max(0)),
        ("offsetByCodePoints", 2) => V::Int(a[0].int() + a[1].int()),
        ("codePoints", 0) => V::Stream(Arc::new(Mutex::new(t.chars().map(|c| V::Int(c as i32)).collect()))),
        _ => return None,
    }))
}

fn list_method(this: &V, items: &Arc<Mutex<Vec<V>>>, name: &str, a: &[V]) -> Option<R> {
    let oob = |i: i32, n: usize| exc("IndexOutOfBoundsException", format!("Index {i} out of bounds for length {n}"));
    Some(Ok(match (name, a.len()) {
        ("size", 0) => V::Int(lock(items).len() as i32),
        ("isEmpty", 0) => V::Bool(lock(items).is_empty()),
        // json-simple's `JSONArray.toJSONString`.
        ("toJSONString", 0) => crate::s(&crate::javaio::to_json(this).to_string()),
        // A BlockingQueue's `drainTo(target[, max])`: what's queued, oldest first, moved across.
        ("drainTo", 1 | 2) => {
            let max = a.get(1).map_or(usize::MAX, |n| n.int().max(0) as usize);
            let moved: Vec<V> = { let mut queue = lock(items); let n = max.min(queue.len()); queue.drain(..n).collect() };
            let count = moved.len();
            for item in moved {
                if let Err(error) = crate::call(&a[0], "add", &[item]) {
                    return Some(Err(error));
                }
            }
            V::Int(count as i32)
        }
        ("add" | "addLast", 1) => {
            lock(items).push(a[0].clone());
            V::Bool(true)
        }
        ("add", 2) => {
            let mut g = lock(items);
            let (i, n) = (a[0].int(), g.len());
            if i < 0 || i as usize > n {
                return Some(Err(oob(i, n)));
            }
            g.insert(i as usize, a[1].clone());
            V::Null
        }
        ("addFirst", 1) => {
            lock(items).insert(0, a[0].clone());
            V::Null
        }
        ("addAll", 1) => {
            let more = match a[0].items() {
                Ok(more) => more,
                Err(e) => return Some(Err(e)),
            };
            lock(items).extend(more);
            V::Bool(true)
        }
        ("get", 1) => {
            let g = lock(items);
            let i = a[0].int();
            return Some(usize::try_from(i).ok().and_then(|at| g.get(at).cloned()).ok_or_else(|| oob(i, g.len())));
        }
        ("getFirst", 0) => return Some(lock(items).first().cloned().ok_or_else(|| exc("NoSuchElementException", "empty"))),
        ("getLast", 0) => return Some(lock(items).last().cloned().ok_or_else(|| exc("NoSuchElementException", "empty"))),
        ("set", 2) => {
            let mut g = lock(items);
            let (i, n) = (a[0].int(), g.len());
            let Some(slot) = usize::try_from(i).ok().and_then(|at| g.get_mut(at)) else { return Some(Err(oob(i, n))) };
            std::mem::replace(slot, a[1].clone())
        }
        ("remove", 1) => {
            let mut g = lock(items);
            if let V::Int(i) = a[0] {
                let n = g.len();
                if i < 0 || i as usize >= n {
                    return Some(Err(oob(i, n)));
                }
                g.remove(i as usize)
            } else {
                match g.iter().position(|x| same(x, &a[0])) {
                    Some(at) => {
                        g.remove(at);
                        V::Bool(true)
                    }
                    None => V::Bool(false),
                }
            }
        }
        ("contains", 1) => V::Bool(lock(items).iter().any(|x| same(x, &a[0]))),
        ("indexOf", 1) => V::Int(lock(items).iter().position(|x| same(x, &a[0])).map_or(-1, |i| i as i32)),
        ("clear", 0) => {
            lock(items).clear();
            V::Null
        }
        ("subList", 2) => list(lock(items)[a[0].int() as usize..a[1].int() as usize].to_vec()),
        ("toArray", _) => arr(lock(items).clone()),
        ("reversed", 0) => list(lock(items).iter().rev().cloned().collect()),
        ("removeIf", 1) => {
            let snapshot = lock(items).clone();
            let mut kept = Vec::new();
            for item in snapshot {
                match call(&a[0], "test", std::slice::from_ref(&item)) {
                    Ok(gone) => {
                        if !gone.truth() {
                            kept.push(item);
                        }
                    }
                    Err(e) => return Some(Err(e)),
                }
            }
            *lock(items) = kept;
            V::Bool(true)
        }
        ("sort", 1) => {
            let mut snapshot = lock(items).clone();
            sort_values(&mut snapshot, a.first());
            *lock(items) = snapshot;
            V::Null
        }
        // `List.hashCode()` as Java defines it, from the items' own hashes: hashing the text rendered every item
        // (FAWE's command trees, keyed in maps, cost a full toString of the tree per put).
        ("hashCode", 0) => {
            let snapshot = lock(items).clone();
            V::Int(snapshot.iter().fold(1i32, |h, v| h.wrapping_mul(31).wrapping_add(if v.is_null() { 0 } else { crate::call(v, "hashCode", &[]).map_or(0, |x| x.int()) })))
        }
        ("toString", 0) => s(&this.text()),
        _ => return None,
    }))
}

pub(crate) fn sort_values(items: &mut [V], comparator: Option<&V>) {
    items.sort_by(|x, y| {
        if let Some(c) = comparator.filter(|c| !c.is_null()) {
            return call(c, "compare", &[x.clone(), y.clone()]).map_or(std::cmp::Ordering::Equal, |o| o.int().cmp(&0));
        }
        match (x, y) {
            (V::Str(p), V::Str(q)) => p.cmp(q),
            _ => cmp(x, y),
        }
    });
}

fn map_method(this: &V, map: &Arc<Mutex<JMap>>, name: &str, a: &[V]) -> Option<R> {
    let value_of = |key: &V| lock(map).get(&key.key()).map(|(_, v)| v.clone());
    Some(Ok(match (name, a.len()) {
        ("size", 0) => V::Int(lock(map).len() as i32),
        ("isEmpty", 0) => V::Bool(lock(map).is_empty()),
        // json-simple's `JSONObject.toJSONString`.
        ("toJSONString", 0) => crate::s(&crate::javaio::to_json(this).to_string()),
        ("get", 1) => value_of(&a[0]).unwrap_or(V::Null),
        // Guava's `BiMap.inverse()`: values to keys, as the map holds them now.
        ("inverse", 0) => {
            let pairs: Vec<(V, V)> = lock(map).values().cloned().collect();
            let inverse = match crate::new_other("LinkedHashMap", &[]) {
                Ok(m) => m,
                Err(e) => return Some(Err(e)),
            };
            for (k, v) in pairs {
                if let Err(e) = call(&inverse, "put", &[v, k]) {
                    return Some(Err(e));
                }
            }
            inverse
        }
        ("getOrDefault", 2) => value_of(&a[0]).unwrap_or_else(|| a[1].clone()),
        ("containsKey", 1) => V::Bool(lock(map).contains_key(&a[0].key())),
        ("containsValue", 1) => V::Bool(lock(map).values().any(|(_, v)| same(v, &a[0]))),
        ("put", 2) => lock(map).insert(a[0].key(), (a[0].clone(), a[1].clone())).map_or(V::Null, |(_, old)| old),
        ("putIfAbsent", 2) => match value_of(&a[0]) {
            Some(old) => old,
            None => {
                lock(map).insert(a[0].key(), (a[0].clone(), a[1].clone()));
                V::Null
            }
        },
        ("putAll", 1) => {
            if let V::Map(other) = &a[0] {
                let copy = lock(other).clone();
                lock(map).extend(copy);
            }
            V::Null
        }
        ("remove", 1) => lock(map).remove(&a[0].key()).map_or(V::Null, |(_, old)| old),
        ("clear", 0) => {
            lock(map).clear();
            V::Null
        }
        ("keySet", 0) => V::Set(Arc::new(Mutex::new(lock(map).key_set()))),
        ("values", 0) => list(lock(map).values().map(|(_, v)| v.clone()).collect()),
        ("entrySet", 0) => list(lock(map).values().map(|(k, v)| V::Entry(Arc::new((k.clone(), v.clone())))).collect()),
        ("computeIfAbsent", 2) => match value_of(&a[0]) {
            Some(old) if !old.is_null() => old,
            _ => {
                let made = match callf(&a[1], std::slice::from_ref(&a[0])) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                lock(map).insert(a[0].key(), (a[0].clone(), made.clone()));
                made
            }
        },
        // `map.replaceAll((k, v) -> ...)`: each value, as the function makes it from its key and value.
        ("replaceAll", 1) if matches!(a[0], V::Func(_) | V::Obj(_)) => {
            let entries: Vec<(String, V, V)> = lock(map).iter().map(|(key, (k, v))| (key.clone(), k.clone(), v.clone())).collect();
            for (key, k, v) in entries {
                let made = match callf(&a[0], &[k.clone(), v]) {
                    Ok(made) => made,
                    Err(e) => return Some(Err(e)),
                };
                lock(map).insert(key, (k, made));
            }
            V::Null
        }
        ("computeIfPresent" | "compute", 2) => {
            let old = value_of(&a[0]);
            if name == "computeIfPresent" && old.is_none() {
                return Some(Ok(V::Null));
            }
            let made = match callf(&a[1], &[a[0].clone(), old.unwrap_or(V::Null)]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            if made.is_null() {
                lock(map).remove(&a[0].key());
            } else {
                lock(map).insert(a[0].key(), (a[0].clone(), made.clone()));
            }
            made
        }
        ("merge", 3) => {
            let made = match value_of(&a[0]) {
                Some(old) => match callf(&a[2], &[old, a[1].clone()]) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                },
                None => a[1].clone(),
            };
            lock(map).insert(a[0].key(), (a[0].clone(), made.clone()));
            made
        }
        ("forEach", 1) => {
            let rows: Vec<(V, V)> = lock(map).values().cloned().collect();
            for (k, v) in rows {
                if let Err(e) = call(&a[0], "accept", &[k, v]) {
                    return Some(Err(e));
                }
            }
            V::Null
        }
        ("toString", 0) => s(&this.text()),
        _ => return None,
    }))
}

fn set_method(this: &V, map: &Arc<Mutex<JMap>>, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("size", 0) => V::Int(lock(map).len() as i32),
        ("isEmpty", 0) => V::Bool(lock(map).is_empty()),
        ("add", 1) => V::Bool(lock(map).insert(a[0].key(), (a[0].clone(), V::Null)).is_none()),
        ("addAll", 1) => {
            let more = match a[0].items() {
                Ok(more) => more,
                Err(e) => return Some(Err(e)),
            };
            let mut g = lock(map);
            for item in more {
                g.insert(item.key(), (item, V::Null));
            }
            V::Bool(true)
        }
        ("contains", 1) => V::Bool(lock(map).contains_key(&a[0].key())),
        ("remove", 1) => V::Bool(lock(map).remove(&a[0].key()).is_some()),
        ("clear", 0) => {
            lock(map).clear();
            V::Null
        }
        ("toArray", _) => arr(lock(map).values().map(|(k, _)| k.clone()).collect()),
        ("toString", 0) => s(&this.text()),
        _ => return None,
    }))
}

fn stream_method(items: &Arc<Mutex<Vec<V>>>, name: &str, a: &[V]) -> Option<R> {
    let snapshot = lock(items).clone();
    let stream = |v: Vec<V>| V::Stream(Arc::new(Mutex::new(v)));
    macro_rules! each {
        ($item:ident, $body:expr) => {
            for $item in &snapshot {
                match $body {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
            }
        };
    }
    Some(Ok(match (name, a.len()) {
        ("map" | "mapToObj" | "mapToInt" | "mapToLong" | "mapToDouble", 1) => {
            let mut out = Vec::new();
            each!(x, callf(&a[0], std::slice::from_ref(x)).map(|v| out.push(v)));
            stream(out)
        }
        ("flatMap", 1) => {
            let mut out = Vec::new();
            each!(x, callf(&a[0], std::slice::from_ref(x)).and_then(|v| v.items()).map(|v| out.extend(v)));
            stream(out)
        }
        ("filter", 1) => {
            let mut out = Vec::new();
            each!(x, call(&a[0], "test", std::slice::from_ref(x)).map(|keep| if keep.truth() { out.push(x.clone()) }));
            stream(out)
        }
        ("peek" | "forEach", 1) => {
            each!(x, call(&a[0], "accept", std::slice::from_ref(x)));
            if name == "peek" { stream(snapshot) } else { V::Null }
        }
        ("anyMatch" | "allMatch" | "noneMatch", 1) => {
            let mut any = false;
            let mut all = true;
            each!(x, call(&a[0], "test", std::slice::from_ref(x)).map(|hit| if hit.truth() { any = true } else { all = false }));
            V::Bool(match name {
                "anyMatch" => any,
                "allMatch" => all,
                _ => !any,
            })
        }
        ("toList", 0) => list(snapshot),
        ("collect", 1) if matches!(a[0], V::Func(_)) => return Some(call(&a[0], "$invoke", &[list(snapshot)])),
        ("collect", 3) => {
            let holder = match call(&a[0], "$invoke", &[]) {
                Ok(holder) => holder,
                Err(error) => return Some(Err(error)),
            };
            for item in snapshot {
                if let Err(error) = call(&a[1], "$invoke", &[holder.clone(), item]) {
                    return Some(Err(error));
                }
            }
            holder
        }
        ("collect", 1) => match a[0].text().as_str() {
            "toSet" => {
                let mut set = JMap::with_order(Order::Hash);
                for item in snapshot {
                    set.insert(item.key(), (item, V::Null));
                }
                V::Set(Arc::new(Mutex::new(set)))
            }
            joined if joined.starts_with("joining:") => s(&snapshot.iter().map(V::text).collect::<Vec<_>>().join(&joined[8..])),
            _ => list(snapshot),
        },
        ("findFirst" | "findAny", 0) => opt(snapshot.first().cloned()),
        ("count", 0) => V::Long(snapshot.len() as i64),
        ("sorted", _) => {
            let mut sorted = snapshot;
            sort_values(&mut sorted, a.first());
            stream(sorted)
        }
        ("distinct", 0) => {
            let mut seen = std::collections::BTreeSet::new();
            stream(snapshot.into_iter().filter(|x| seen.insert(x.key())).collect())
        }
        ("limit", 1) => stream(snapshot.into_iter().take(a[0].long().max(0) as usize).collect()),
        ("skip", 1) => stream(snapshot.into_iter().skip(a[0].long().max(0) as usize).collect()),
        ("boxed", 0) => stream(snapshot),
        ("sum", 0) => snapshot.iter().fold(V::Int(0), |t, x| add(&t, x)),
        ("average", 0) => opt((!snapshot.is_empty()).then(|| V::Double(snapshot.iter().map(V::double).sum::<f64>() / snapshot.len() as f64))),
        ("max" | "min", _) => {
            let mut sorted = snapshot;
            sort_values(&mut sorted, a.first());
            opt(if name == "max" { sorted.last().cloned() } else { sorted.first().cloned() })
        }
        ("toArray", _) => arr(snapshot),
        ("iterator", 0) => V::Iter(Arc::new(Mutex::new((snapshot, 0)))),
        _ => return None,
    }))
}

fn opt_method(value: &Option<V>, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        // `Optional.hashCode()` is the value's, `equals` compares the values.
        ("hashCode", 0) => V::Int(value.as_ref().filter(|v| !v.is_null()).map_or(0, |v| call(v, "hashCode", &[]).map_or(0, |h| h.int()))),
        ("equals", 1) => V::Bool(match (&a[0], value) {
            (V::Opt(other), mine) => match ((**other).as_ref(), mine) {
                (None, None) => true,
                (Some(x), Some(y)) => same(x, y) || equals(y, x).is_ok_and(|e| e.truth()),
                _ => false,
            },
            _ => false,
        }),
        ("isPresent", 0) => V::Bool(value.is_some()),
        ("isEmpty", 0) => V::Bool(value.is_none()),
        ("get" | "orElseThrow" | "getAsInt" | "getAsDouble", _) => return Some(value.clone().ok_or_else(|| exc("NoSuchElementException", "No value present"))),
        ("orElse", 1) => value.clone().unwrap_or_else(|| a[0].clone()),
        ("orElseGet", 1) => match value {
            Some(v) => v.clone(),
            None => return Some(call(&a[0], "get", &[])),
        },
        // `or(() -> otherOptional)`: this one when it has a value, else what the supplier gives.
        ("or", 1) => match value {
            Some(v) => opt(Some(v.clone())),
            None => return Some(call(&a[0], "get", &[])),
        },
        ("ifPresent", 1) => {
            if let Some(v) = value {
                return Some(call(&a[0], "accept", std::slice::from_ref(v)).map(|_| V::Null));
            }
            V::Null
        }
        ("ifPresentOrElse", 2) => {
            return Some(match value {
                Some(v) => call(&a[0], "accept", std::slice::from_ref(v)),
                None => call(&a[1], "run", &[]),
            });
        }
        ("map", 1) => match value {
            Some(v) => match callf(&a[0], std::slice::from_ref(v)) {
                Ok(m) => opt(Some(m)),
                Err(e) => return Some(Err(e)),
            },
            None => opt(None),
        },
        ("filter", 1) => match value {
            Some(v) => match call(&a[0], "test", std::slice::from_ref(v)) {
                Ok(keep) => opt(keep.truth().then(|| v.clone())),
                Err(e) => return Some(Err(e)),
            },
            None => opt(None),
        },
        _ => return None,
    }))
}

pub fn builtin_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(found) = fastutil_method(this, name, a) {
        return Some(found);
    }
    if let Some(found) = crate::javacore::method(this, name, a).or_else(|| crate::okhttp::method(this, name, a)).or_else(|| crate::datautil::method(this, name, a)).or_else(|| crate::javamore::method(this, name, a)).or_else(|| crate::awt::method(this, name, a)).or_else(|| crate::bignum::method(this, name, a)).or_else(|| crate::props::method(this, name, a)).or_else(|| crate::zip::method(this, name, a)).or_else(|| crate::views::method(this, name, a)).or_else(|| crate::javaio::method(this, name, a)).or_else(|| crate::configurate::method(this, name, a)).or_else(|| crate::sql::method(this, name, a)).or_else(|| crate::text::method(this, name, a)).or_else(|| crate::kotlin::method(this, name, a)).or_else(|| crate::net::method(this, name, a)).or_else(|| crate::jtime::method(this, name, a)).or_else(|| crate::reflect::method(this, name, a)).or_else(|| crate::javautil::method(this, name, a)) {
        return Some(found);
    }
    // What every object has.
    let common = match (name, a.len()) {
        ("equals", 1) if !matches!(this, V::Str(_)) => Some(Ok(V::Bool(same(this, &a[0])))),
        ("toString", 0) if matches!(this, V::Obj(_)) => None,
        // A game object nothing above names: Java's own `Object.toString()`. `text()` of a native asks `toString`,
        // so answering with it recursed to the depth limit on every call (FAWE's load took minutes in it).
        ("toString", 0) if matches!(this, V::Native(_)) => {
            let V::Native(native) = this else { unreachable!() };
            Some(Ok(s(&format!("{}@{:x}", crate::java_class_name(crate::class_of(this)), (std::sync::Arc::as_ptr(native) as *const u8 as usize >> 4) as u32))))
        }
        ("toString", 0) if !matches!(this, V::Str(_) | V::List(_) | V::Map(_) | V::Set(_)) => Some(Ok(s(&this.text()))),
        ("hashCode", 0) if matches!(this, V::Int(_) | V::Long(_) | V::Bool(_) | V::Double(_)) => Some(Ok(V::Int(java_hash(&this.text())))),
        // A game object nothing above hashes: Java's identity hash, as its `toString` above is Java's own.
        ("hashCode", 0) if matches!(this, V::Native(_)) => {
            let V::Native(native) = this else { unreachable!() };
            Some(Ok(V::Int((std::sync::Arc::as_ptr(native) as *const u8 as usize >> 4) as i32)))
        }
        ("getClass", 0) => Some(Ok(V::Class(crate::class_of(this)))),
        ("intValue", 0) => Some(Ok(cast(this, "int"))),
        ("longValue", 0) => Some(Ok(cast(this, "long"))),
        ("doubleValue" | "floatValue", 0) => Some(Ok(cast(this, "double"))),
        ("booleanValue" | "charValue", 0) => Some(Ok(this.clone())),
        ("isNaN", 0) if matches!(this, V::Double(_)) => Some(Ok(V::Bool(this.double().is_nan()))),
        ("isInfinite", 0) if matches!(this, V::Double(_)) => Some(Ok(V::Bool(this.double().is_infinite()))),
        // A boxed number's or char's `compareTo` (Kotlin's `coerceIn` calls it): -1, 0 or 1.
        ("compareTo", 1) if matches!(this, V::Int(_) | V::Long(_) | V::Double(_) | V::Char(_) | V::Bool(_)) => Some(Ok(V::Int(cmp(this, &a[0]) as i32))),
        _ => None,
    };
    if common.is_some() {
        return common;
    }
    match this {
        V::Str(t) => str_method(t, name, a),
        V::List(items) => list_method(this, items, name, a).or_else(|| shared(this, name, a)),
        // `array.clone()`: a new array with the same items, as Java's shallow copy.
        V::Arr(items) if name == "clone" && a.is_empty() => Some(Ok(V::Arr(Arc::new(Mutex::new(lock(items).clone()))))),
        V::Arr(items) => list_method(this, items, name, a).or_else(|| shared(this, name, a)),
        V::Map(map) => map_method(this, map, name, a),
        V::Set(map) => set_method(this, map, name, a).or_else(|| shared(this, name, a)),
        V::Stream(items) => stream_method(items, name, a),
        V::Opt(value) => opt_method(value, name, a),
        V::Sb(text) => Some(Ok(match (name, a.len()) {
            ("append", 1) => {
                lock(text).push_str(&a[0].text());
                this.clone()
            }
            ("appendCodePoint", 1) => {
                if let Some(c) = char::from_u32(a[0].long() as u32) {
                    lock(text).push(c);
                }
                this.clone()
            }
            ("insert", 2) => {
                lock(text).insert_str(a[0].int().max(0) as usize, &a[1].text());
                this.clone()
            }
            ("reverse", 0) => {
                let flipped: String = lock(text).chars().rev().collect();
                *lock(text) = flipped;
                this.clone()
            }
            ("setLength", 1) => {
                let cut: String = lock(text).chars().take(a[0].int().max(0) as usize).collect();
                *lock(text) = cut;
                V::Null
            }
            // `replace(start, end, text)`: StringBuilder's own, by char positions (String.replace takes two strings).
            ("replace", 3) if matches!(a[0], V::Int(_) | V::Long(_)) => {
                let mut c: Vec<char> = lock(text).chars().collect();
                let start = (a[0].int().max(0) as usize).min(c.len());
                let end = (a[1].int().max(0) as usize).min(c.len()).max(start);
                c.splice(start..end, a[2].text().chars());
                *lock(text) = c.into_iter().collect();
                this.clone()
            }
            ("delete", 2) => {
                let mut c: Vec<char> = lock(text).chars().collect();
                let start = (a[0].int().max(0) as usize).min(c.len());
                let end = (a[1].int().max(0) as usize).min(c.len()).max(start);
                c.drain(start..end);
                *lock(text) = c.into_iter().collect();
                this.clone()
            }
            ("deleteCharAt", 1) => {
                let mut c: Vec<char> = lock(text).chars().collect();
                let at = a[0].int().max(0) as usize;
                if at < c.len() {
                    c.remove(at);
                }
                *lock(text) = c.into_iter().collect();
                this.clone()
            }
            _ => {
                let copy = lock(text).clone();
                return str_method(&copy, name, a);
            }
        })),
        V::Iter(state) => Some(Ok(match (name, a.len()) {
            ("hasNext", 0) => {
                let g = lock(state);
                V::Bool(g.1 < g.0.len())
            }
            ("next", 0) => {
                let mut g = lock(state);
                let at = g.1;
                g.1 += 1;
                return Some(g.0.get(at).cloned().ok_or_else(|| exc("NoSuchElementException", "end")));
            }
            _ => return None,
        })),
        V::Entry(pair) => Some(Ok(match name {
            "getKey" => pair.0.clone(),
            "getValue" => pair.1.clone(),
            "setValue" if a.len() == 1 => return Some(crate::views::entry_set_value(pair, a[0].clone())),
            _ => return None,
        })),
        V::Exc(thrown) => Some(Ok(match name {
            "getMessage" | "getLocalizedMessage" => thrown.message.as_deref().map_or(V::Null, s),
            "printStackTrace" => {
                eprintln!("[plugin] {}", this.text());
                V::Null
            }
            "getCause" => thrown.cause.clone().unwrap_or(V::Null),
            "toString" => s(&this.text()),
            // What every Throwable has: itself after these, an empty trace (none is kept here).
            "fillInStackTrace" | "initCause" => this.clone(),
            "getStackTrace" => crate::javacore::frames_of(&thrown.trace),
            "getSuppressed" => crate::arr(Vec::new()),
            "setStackTrace" | "addSuppressed" => V::Null,
            _ => return None,
        })),
        V::Class(class) => Some(Ok(match name {
            "getName" | "getSimpleName" => s(class),
            _ => return None,
        })),
        V::Native(native) => {
            if let Some(adder) = native.downcast_ref::<JAdder>() {
                let mut sum = lock(&adder.0);
                let long = matches!(*sum, V::Long(_));
                let add = |by: &V, sum: &V| if long { V::Long(sum.long().wrapping_add(by.long())) } else { V::Double(sum.double() + by.double()) };
                return Some(Ok(match (name, a.len()) {
                    ("add", 1) => {
                        *sum = add(&a[0], &sum);
                        V::Null
                    }
                    ("increment", 0) => {
                        *sum = add(&V::Long(1), &sum);
                        V::Null
                    }
                    ("decrement", 0) => {
                        *sum = add(&V::Long(-1), &sum);
                        V::Null
                    }
                    ("sum" | "longValue" | "doubleValue", 0) => sum.clone(),
                    ("intValue", 0) => V::Int(sum.long() as i32),
                    ("reset", 0) => {
                        *sum = if long { V::Long(0) } else { V::Double(0.0) };
                        V::Null
                    }
                    ("sumThenReset", 0) => {
                        let was = sum.clone();
                        *sum = if long { V::Long(0) } else { V::Double(0.0) };
                        was
                    }
                    ("toString", 0) => s(&sum.text()),
                    _ => return None,
                }));
            }
            if let Some(chain) = native.downcast_ref::<JComparisonChain>() {
                let mut decided = lock(&chain.0);
                return Some(Ok(match (name, a.len()) {
                    ("compare", 2 | 3) => {
                        if *decided == 0 {
                            *decided = match a.get(2).filter(|c| !c.is_null()) {
                                Some(comparator) => match call(comparator, "compare", &a[..2]) {
                                    Ok(n) => n.int().signum(),
                                    Err(e) => return Some(Err(e)),
                                },
                                None => match call(&a[0], "compareTo", &a[1..2]) {
                                    Ok(n) => n.int().signum(),
                                    Err(_) => cmp(&a[0], &a[1]) as i32,
                                },
                            };
                        }
                        this.clone()
                    }
                    ("compareTrueFirst" | "compareFalseFirst", 2) => {
                        if *decided == 0 {
                            let (x, y) = (a[0].truth(), a[1].truth());
                            *decided = if x == y { 0 } else if (x && name == "compareTrueFirst") || (!x && name == "compareFalseFirst") { -1 } else { 1 };
                        }
                        this.clone()
                    }
                    ("result", 0) => V::Int(*decided),
                    _ => return None,
                }));
            }
            if let Some(reference) = native.downcast_ref::<JRef>() {
                return Some(Ok(match (name, a.len()) {
                    ("get", 0) => lock(&reference.0).clone(),
                    ("clear", 0) => {
                        *lock(&reference.0) = V::Null;
                        V::Null
                    }
                    ("refersTo", 1) => V::Bool(same(&lock(&reference.0), &a[0])),
                    ("enqueue" | "isEnqueued", 0) => V::Bool(false),
                    _ => return None,
                }));
            }
            if let Some(helper) = native.downcast_ref::<JToStringHelper>() {
                let mut state = lock(&helper.0);
                return Some(Ok(match (name, a.len()) {
                    ("add", 2) => {
                        state.1.push((Some(a[0].text()), a[1].clone()));
                        this.clone()
                    }
                    ("addValue", 1) => {
                        state.1.push((None, a[0].clone()));
                        this.clone()
                    }
                    ("omitNullValues", 0) => {
                        state.2 = true;
                        this.clone()
                    }
                    ("omitEmptyValues", 0) => this.clone(),
                    ("toString", 0) => {
                        let parts: Vec<String> = state.1.iter().filter(|(_, v)| !(state.2 && v.is_null())).map(|(k, v)| match k {
                            Some(k) => format!("{k}={}", v.text()),
                            None => v.text(),
                        }).collect();
                        s(&format!("{}{{{}}}", state.0, parts.join(", ")))
                    }
                    _ => return None,
                }));
            }
            if let Some(format) = native.downcast_ref::<JMessageFormat>() {
                return Some(Ok(match name {
                    "format" => {
                        let args: Vec<V> = match a {
                            [V::Arr(items)] => lock(items).clone(),
                            more => more.to_vec(),
                        };
                        s(&message_format(&format.0, &args))
                    }
                    "toPattern" => s(&format.0),
                    "applyPattern" => V::Null,
                    _ => return None,
                }));
            }
            if let Some(rng) = native.downcast_ref::<Rng>() {
                let n = next_random(&rng.0);
                return Some(Ok(match (name, a.len()) {
                    ("nextInt", 1) => V::Int((n % a[0].int().max(1) as u64) as i32),
                    ("nextInt", 0) => V::Int(n as i32),
                    ("nextInt", 2) => V::Int(a[0].int() + (n % (a[1].int() - a[0].int()).max(1) as u64) as i32),
                    ("nextDouble", 0) => V::Double((n >> 11) as f64 / (1u64 << 53) as f64),
                    ("nextBoolean", 0) => V::Bool(n & 1 == 1),
                    ("nextLong", 0) => V::Long(n as i64),
                    // `nextLong(bound)` / `nextLong(origin, bound)` (RandomGenerator): FAWE's queue picks with it.
                    ("nextLong", 1) => V::Long((n % (a[0].long().max(1) as u64)) as i64),
                    ("nextLong", 2) => V::Long(a[0].long().wrapping_add((n % (a[1].long().wrapping_sub(a[0].long()).max(1) as u64)) as i64)),
                    ("nextDouble", 1) => V::Double((n >> 11) as f64 / (1u64 << 53) as f64 * a[0].double()),
                    ("nextDouble", 2) => V::Double(a[0].double() + (n >> 11) as f64 / (1u64 << 53) as f64 * (a[1].double() - a[0].double())),
                    ("nextFloat", 0) => V::Double((n >> 40) as f64 / (1u64 << 24) as f64),
                    ("nextGaussian", 0) => {
                        // Box-Muller over two draws.
                        let u1 = ((n >> 11) as f64 / (1u64 << 53) as f64).max(f64::MIN_POSITIVE);
                        let u2 = (next_random(&rng.0) >> 11) as f64 / (1u64 << 53) as f64;
                        V::Double((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos())
                    }
                    ("nextBytes", 1) => {
                        if let V::Arr(bytes) = &a[0] {
                            let mut bytes = lock(bytes);
                            let mut word = n;
                            for (i, slot) in bytes.iter_mut().enumerate() {
                                if i > 0 && i % 8 == 0 {
                                    word = next_random(&rng.0);
                                }
                                *slot = V::Int((word >> ((i % 8) * 8)) as u8 as i8 as i32);
                            }
                        }
                        V::Null
                    }
                    _ => return None,
                }));
            }
            let cell = native.downcast_ref::<Cell>()?;
            Some(Ok(match (name, a.len()) {
                ("get" | "intValue" | "longValue", 0) => lock(&cell.0).clone(),
                ("set", 1) => {
                    *lock(&cell.0) = a[0].clone();
                    V::Null
                }
                ("incrementAndGet", 0) => {
                    let mut g = lock(&cell.0);
                    *g = add(&g, &V::Int(1));
                    g.clone()
                }
                ("decrementAndGet", 0) => {
                    let mut g = lock(&cell.0);
                    *g = sub(&g, &V::Int(1));
                    g.clone()
                }
                ("getAndIncrement", 0) => {
                    let mut g = lock(&cell.0);
                    let old = g.clone();
                    *g = add(&old, &V::Int(1));
                    old
                }
                ("addAndGet", 1) => {
                    let mut g = lock(&cell.0);
                    *g = add(&g, &a[0]);
                    g.clone()
                }
                _ => return None,
            }))
        }
        _ => None,
    }
}

/// What every `Collection` adds.
fn shared(this: &V, name: &str, a: &[V]) -> Option<R> {
    let items = match this.items() {
        Ok(items) => items,
        Err(e) => return Some(Err(e)),
    };
    Some(Ok(match (name, a.len()) {
        ("iterator", 0) => V::Iter(Arc::new(Mutex::new((items, 0)))),
        ("stream", 0) => V::Stream(Arc::new(Mutex::new(items))),
        ("forEach", 1) => {
            for item in items {
                if let Err(e) = call(&a[0], "accept", &[item]) {
                    return Some(Err(e));
                }
            }
            V::Null
        }
        ("containsAll", 1) => {
            let wanted = match a[0].items() {
                Ok(w) => w,
                Err(e) => return Some(Err(e)),
            };
            V::Bool(wanted.iter().all(|w| items.iter().any(|x| same(x, w))))
        }
        _ => return None,
    }))
}

/// `Character`'s code-point methods: `isDigit(int)` and friends take a code point (the `char` ones are the
/// same test), and the surrogate helpers a YAML or JSON reader uses. Each `V::Char` holds a whole character.
fn character_static(name: &str, a: &[V]) -> Option<R> {
    let code = |v: &V| -> Option<char> {
        match v {
            V::Char(c) => Some(*c),
            V::Int(_) | V::Long(_) => char::from_u32(v.long() as u32),
            V::Str(t) if t.chars().count() == 1 => t.chars().next(),
            _ => None,
        }
    };
    let first = a.first()?;
    let int_form = matches!(first, V::Int(_) | V::Long(_));
    let c = code(first);
    Some(Ok(match (name, a.len()) {
        ("isDigit", 1) => V::Bool(c.is_some_and(|c| c.is_ascii_digit() || (c.is_numeric() && !c.is_ascii()))),
        ("isLetter" | "isAlphabetic", 1) => V::Bool(c.is_some_and(char::is_alphabetic)),
        ("isLetterOrDigit", 1) => V::Bool(c.is_some_and(char::is_alphanumeric)),
        ("isWhitespace" | "isSpaceChar", 1) => V::Bool(c.is_some_and(char::is_whitespace)),
        ("isUpperCase", 1) => V::Bool(c.is_some_and(char::is_uppercase)),
        ("isLowerCase", 1) => V::Bool(c.is_some_and(char::is_lowercase)),
        ("isISOControl", 1) => V::Bool(c.is_some_and(char::is_control)),
        ("toLowerCase" | "toUpperCase", 1) if int_form => {
            let c = c.unwrap_or('\0');
            let out = if name == "toLowerCase" { c.to_lowercase().next() } else { c.to_uppercase().next() };
            V::Int(out.unwrap_or(c) as i32)
        }
        ("isSupplementaryCodePoint", 1) => V::Bool(first.long() >= 0x10000 && first.long() <= 0x10FFFF),
        ("isBmpCodePoint", 1) => V::Bool((0..0x10000).contains(&first.long())),
        ("isValidCodePoint", 1) => V::Bool((0..=0x10FFFF).contains(&first.long())),
        ("charCount", 1) => V::Int(if first.long() >= 0x10000 { 2 } else { 1 }),
        // A `V::Char` is never half a pair: no surrogates come up here.
        ("isHighSurrogate" | "isLowSurrogate" | "isSurrogate", 1) => V::Bool(c.is_none() && (0xD800..0xE000).contains(&first.long())),
        ("isSurrogatePair", 2) => V::Bool(false),
        // `codePointCount(seq, begin, end)`: one code point per character here.
        ("codePointCount", 3) => V::Int((a[2].int() - a[1].int()).max(0)),
        ("offsetByCodePoints", 3) => V::Int(a[1].int() + a[2].int()),
        ("codePointAt" | "codePointBefore", 3) => return character_static(name, &a[..2]),
        ("codePointAt", 2) => {
            let at = a[1].int().max(0) as usize;
            let found = match first {
                V::Arr(items) => lock(items).get(at).cloned(),
                other => other.text().chars().nth(at).map(V::Char),
            };
            match found.as_ref().and_then(code) {
                Some(c) => V::Int(c as i32),
                None => return Some(Err(exc("IndexOutOfBoundsException", format!("index {at}")))),
            }
        }
        ("getNumericValue", 1) => V::Int(c.and_then(|c| c.to_digit(36)).map_or(-1, |d| d as i32)),
        ("digit", 2) => V::Int(c.and_then(|c| c.to_digit(a[1].int().clamp(2, 36) as u32)).map_or(-1, |d| d as i32)),
        _ => return None,
    }))
}

/// `Math.addExact` and friends: an int stays an int and a long a long, and an overflow throws as Java's.
fn math_exact(name: &str, a: &[V]) -> Option<R> {
    let long = a.iter().any(|v| matches!(v, V::Long(_)));
    let overflow = || exc("ArithmeticException", if long { "long overflow" } else { "integer overflow" });
    let pair = |f32_: fn(i32, i32) -> Option<i32>, f64_: fn(i64, i64) -> Option<i64>| -> R {
        if long { f64_(a[0].long(), a[1].long()).map(V::Long).ok_or_else(overflow) } else { f32_(a[0].int(), a[1].int()).map(V::Int).ok_or_else(overflow) }
    };
    let one = |f32_: fn(i32) -> Option<i32>, f64_: fn(i64) -> Option<i64>| -> R {
        if long { f64_(a[0].long()).map(V::Long).ok_or_else(overflow) } else { f32_(a[0].int()).map(V::Int).ok_or_else(overflow) }
    };
    Some(match (name, a.len()) {
        ("addExact", 2) => pair(i32::checked_add, i64::checked_add),
        ("subtractExact", 2) => pair(i32::checked_sub, i64::checked_sub),
        ("multiplyExact", 2) => pair(i32::checked_mul, i64::checked_mul),
        ("negateExact", 1) => one(i32::checked_neg, i64::checked_neg),
        ("incrementExact", 1) => one(|x| x.checked_add(1), |x| x.checked_add(1)),
        ("decrementExact", 1) => one(|x| x.checked_sub(1), |x| x.checked_sub(1)),
        ("absExact", 1) => one(i32::checked_abs, i64::checked_abs),
        ("toIntExact", 1) => i32::try_from(a[0].long()).map(V::Int).map_err(|_| exc("ArithmeticException", "integer overflow")),
        ("floorMod", 2) if long => Ok(V::Long(a[0].long().rem_euclid(a[1].long().max(1)))),
        ("floorDiv", 2) if long => Ok(V::Long(a[0].long().div_euclid(a[1].long().max(1)))),
        ("signum", 1) => Ok(V::Double(a[0].double().signum() * f64::from(u8::from(a[0].double() != 0.0)))),
        ("hypot", 2) => Ok(V::Double(a[0].double().hypot(a[1].double()))),
        ("cbrt", 1) => Ok(V::Double(a[0].double().cbrt())),
        ("log10", 1) => Ok(V::Double(a[0].double().log10())),
        ("asin", 1) => Ok(V::Double(a[0].double().asin())),
        ("acos", 1) => Ok(V::Double(a[0].double().acos())),
        ("atan", 1) => Ok(V::Double(a[0].double().atan())),
        _ => return None,
    })
}

/// A `Locale` as the runtime holds one: a constant's name (`Locale.ENGLISH` is "ENGLISH") or a tag ("en", "en_US").
fn is_locale(value: &V) -> bool {
    const NAMES: [&str; 21] = ["ENGLISH", "US", "UK", "ROOT", "GERMAN", "GERMANY", "FRENCH", "FRANCE", "ITALIAN", "ITALY", "JAPANESE", "JAPAN", "KOREAN", "KOREA", "CHINESE", "CHINA", "SIMPLIFIED_CHINESE", "TRADITIONAL_CHINESE", "CANADA", "CANADA_FRENCH", "PRC"];
    let V::Str(text) = value else { return false };
    let bytes = text.as_bytes();
    NAMES.contains(&&**text) || (bytes.len() == 2 && bytes.iter().all(u8::is_ascii_lowercase)) || (bytes.len() == 5 && bytes[..2].iter().all(u8::is_ascii_lowercase) && matches!(bytes[2], b'_' | b'-') && bytes[3..].iter().all(u8::is_ascii_uppercase))
}

pub fn builtin_static(class: &str, name: &str, a: &[V]) -> Option<R> {
    if let Some(found) = crate::javacore::static_call(class, name, a).or_else(|| crate::okhttp::static_call(class, name, a)).or_else(|| crate::datautil::static_call(class, name, a)).or_else(|| crate::javamore::static_call(class, name, a)).or_else(|| crate::awt::static_call(class, name, a)).or_else(|| crate::bignum::static_call(class, name, a)).or_else(|| crate::javaio::static_call(class, name, a)).or_else(|| crate::configurate::static_call(class, name, a)).or_else(|| crate::sql::static_call(class, name, a)).or_else(|| crate::text::static_call(class, name, a)).or_else(|| crate::kotlin::static_call(class, name, a)).or_else(|| crate::net::static_call(class, name, a)).or_else(|| crate::jtime::static_call(class, name, a)).or_else(|| crate::commons::static_call(class, name, a)).or_else(|| crate::javautil::static_call(class, name, a)) {
        return Some(found);
    }
    // Guava's `Ints.tryParse("12")`: the number, or null when the text isn't one.
    if matches!(class, "Ints" | "Longs" | "Doubles" | "Floats") && name == "tryParse" && !a.is_empty() {
        let text = a[0].text();
        let radix = a.get(1).map_or(10, V::int) as u32;
        return Some(Ok(match class {
            "Ints" => i32::from_str_radix(&text, radix).map_or(V::Null, V::Int),
            "Longs" => i64::from_str_radix(&text, radix).map_or(V::Null, V::Long),
            _ => text.trim().parse::<f64>().ok().filter(|_| !text.trim().is_empty() && text.trim() == text).map_or(V::Null, V::Double),
        }));
    }
    // `Enum.valueOf(Color.class, "RED")`: that enum's own `valueOf`.
    if class == "Enum" && name == "valueOf" && a.len() == 2 {
        let V::Class(enum_class) = &a[0] else {
            return Some(Err(exc("NullPointerException", "enumClass")));
        };
        if let Some(own) = crate::plugin_class(enum_class) {
            return Some(match method_of(own, "valueOf", &a[1..]) {
                Some(found) => found(&V::Null, &a[1..]),
                None => crate::inherited_static(own, "valueOf", &a[1..]).unwrap_or_else(|| Err(exc("IllegalArgumentException", format!("No enum constant {enum_class}.{}", a[1].text())))),
            });
        }
        return Some(crate::call_static(enum_class.rsplit('.').next().unwrap_or(enum_class), "valueOf", &a[1..]));
    }
    if class == "Character" {
        if let Some(found) = character_static(name, a) {
            return Some(found);
        }
    }
    if class == "Math" || class == "StrictMath" {
        if let Some(found) = math_exact(name, a) {
            return Some(found);
        }
    }
    let x = || a.first().map_or(0.0, V::double);
    let y = || a.get(1).map_or(0.0, V::double);
    let num = |class: &str| exc("NumberFormatException", format!("For input string: \"{}\" ({class})", a.first().map(V::text).unwrap_or_default()));
    Some(Ok(match (class, name) {
        ("Math", "max") => {
            if cmp(&a[0], &a[1]).is_ge() { a[0].clone() } else { a[1].clone() }
        }
        ("Math", "min") => {
            if cmp(&a[0], &a[1]).is_le() { a[0].clone() } else { a[1].clone() }
        }
        ("Math", "abs") => match &a[0] {
            V::Int(n) => V::Int(n.wrapping_abs()),
            V::Long(n) => V::Long(n.wrapping_abs()),
            other => V::Double(other.double().abs()),
        },
        ("Math", "floor") => V::Double(x().floor()),
        ("Math", "ceil") => V::Double(x().ceil()),
        ("Math", "round") => V::Long((x() + 0.5).floor() as i64),
        ("Math", "sqrt") => V::Double(x().sqrt()),
        ("Math", "pow") => V::Double(x().powf(y())),
        ("Math", "log") => V::Double(x().ln()),
        ("Math", "exp") => V::Double(x().exp()),
        ("Math", "sin") => V::Double(x().sin()),
        ("Math", "cos") => V::Double(x().cos()),
        ("Math", "tan") => V::Double(x().tan()),
        ("Math", "atan2") => V::Double(x().atan2(y())),
        ("Math", "toRadians") => V::Double(x().to_radians()),
        ("Math", "toDegrees") => V::Double(x().to_degrees()),
        ("Math", "floorMod") => V::Int(a[0].int().rem_euclid(a[1].int().max(1))),
        ("Math", "floorDiv") => V::Int(a[0].int().div_euclid(a[1].int().max(1))),
        // Library gaps FastAsyncWorldEdit reaches (the call probe's list): plain arithmetic and Guava helpers.
        ("Math", "ceilDiv") => match (&a[0], &a[1]) {
            (V::Long(_), _) | (_, V::Long(_)) => V::Long(-(-a[0].long()).div_euclid(a[1].long().max(1))),
            _ => V::Int(-(-a[0].int()).div_euclid(a[1].int().max(1))),
        },
        ("Math", "nextUp") => V::Double(x().next_up()),
        ("Long", "highestOneBit") => V::Long(if a[0].long() == 0 { 0 } else { 1i64 << (63 - a[0].long().leading_zeros()) }),
        ("Long", "lowestOneBit") => V::Long(a[0].long() & a[0].long().wrapping_neg()),
        ("Integer", "signum") => V::Int(a[0].int().signum()),
        ("Long", "signum") => V::Int(a[0].long().signum() as i32),
        ("Boolean", "hashCode") => V::Int(if a[0].truth() { 1231 } else { 1237 }),
        ("Double", "hashCode") => V::Int({ let bits = a[0].double().to_bits(); (bits ^ (bits >> 32)) as i32 }),
        ("Collections", "max" | "min") => {
            let mut items = match a[0].items() {
                Ok(items) => items,
                Err(e) => return Some(Err(e)),
            };
            if items.is_empty() {
                return Some(Err(exc("NoSuchElementException", "")));
            }
            sort_values(&mut items, a.get(1));
            if name == "max" { items.pop().unwrap_or(V::Null) } else { items.swap_remove(0) }
        }
        ("Lists", "reverse") => match a[0].items() {
            Ok(mut items) => {
                items.reverse();
                list(items)
            }
            Err(e) => return Some(Err(e)),
        },
        ("Iterables" | "Iterators", "concat") => {
            let parts: Vec<V> = match a {
                [one] if !matches!(one, V::Arr(_)) => match one.items() {
                    Ok(items) => items,
                    Err(e) => return Some(Err(e)),
                },
                [V::Arr(items)] => lock(items).clone(),
                more => more.to_vec(),
            };
            let mut all = Vec::new();
            for part in parts {
                let items = if class == "Iterators" {
                    let mut out = Vec::new();
                    loop {
                        match call(&part, "hasNext", &[]) {
                            Ok(more) if more.truth() => match call(&part, "next", &[]) {
                                Ok(item) => out.push(item),
                                Err(e) => return Some(Err(e)),
                            },
                            Ok(_) => break,
                            Err(e) => return Some(Err(e)),
                        }
                    }
                    out
                } else {
                    match part.items() {
                        Ok(items) => items,
                        Err(e) => return Some(Err(e)),
                    }
                };
                all.extend(items);
            }
            if class == "Iterators" { return Some(call(&list(all), "iterator", &[])) } else { list(all) }
        }
        ("Iterables", "toArray") => match a[0].items() {
            Ok(items) => arr(items),
            Err(e) => return Some(Err(e)),
        },
        ("Iterators", "transform") => {
            let mut out = Vec::new();
            loop {
                match call(&a[0], "hasNext", &[]) {
                    Ok(more) if more.truth() => match call(&a[0], "next", &[]).and_then(|item| callf(&a[1], &[item])) {
                        Ok(mapped) => out.push(mapped),
                        Err(e) => return Some(Err(e)),
                    },
                    Ok(_) => break,
                    Err(e) => return Some(Err(e)),
                }
            }
            return Some(call(&list(out), "iterator", &[]));
        }
        ("Collections2", "filter") => match a[0].items() {
            Ok(items) => {
                let mut kept = Vec::new();
                for item in items {
                    match call(&a[1], "test", std::slice::from_ref(&item)).or_else(|_| call(&a[1], "apply", std::slice::from_ref(&item))) {
                        Ok(keep) if keep.truth() => kept.push(item),
                        Ok(_) => {}
                        Err(e) => return Some(Err(e)),
                    }
                }
                list(kept)
            }
            Err(e) => return Some(Err(e)),
        },
        ("Maps", "transformValues") => {
            let made = match crate::new_other("LinkedHashMap", &[]) {
                Ok(m) => m,
                Err(e) => return Some(Err(e)),
            };
            let entries = match call(&a[0], "entrySet", &[]).and_then(|s| s.items()) {
                Ok(entries) => entries,
                Err(e) => return Some(Err(e)),
            };
            for entry in entries {
                let pair = call(&entry, "getKey", &[]).and_then(|k| call(&entry, "getValue", &[]).and_then(|v| callf(&a[1], &[v])).map(|v| (k, v)));
                match pair.and_then(|(k, v)| call(&made, "put", &[k, v])) {
                    Ok(_) => {}
                    Err(e) => return Some(Err(e)),
                }
            }
            made
        }
        ("EnumSet", "allOf") => match call_static(&a[0].text().trim_start_matches("class ").to_owned(), "values", &[]).and_then(|v| v.items()) {
            Ok(values) => match crate::new_other("LinkedHashSet", &[]) {
                Ok(set) => {
                    for value in values {
                        let _ = call(&set, "add", &[value]);
                    }
                    set
                }
                Err(e) => return Some(Err(e)),
            },
            Err(e) => return Some(Err(e)),
        },
        ("OptionalLong" | "OptionalInt" | "OptionalDouble", "of") => opt(Some(a[0].clone())),
        ("OptionalLong" | "OptionalInt" | "OptionalDouble", "empty") => opt(None),
        ("Preconditions", "checkElementIndex" | "checkPositionIndex") => {
            let (index, size) = (a[0].int(), a.get(1).map_or(0, V::int));
            let fits = if name == "checkElementIndex" { index >= 0 && index < size } else { index >= 0 && index <= size };
            if !fits {
                return Some(Err(exc("IndexOutOfBoundsException", format!("{} ({index}) must be less than size ({size})", a.get(2).map(V::text).unwrap_or_else(|| "index".into())))));
            }
            V::Int(index)
        }
        ("Suppliers", "memoize" | "memoizeWithExpiration") => {
            let supplier = a[0].clone();
            let known: std::sync::Arc<Mutex<Option<V>>> = std::sync::Arc::new(Mutex::new(None));
            crate::func(move |_| {
                if let Some(value) = lock(&known).clone() {
                    return Ok(value);
                }
                let value = call(&supplier, "get", &[])?;
                *lock(&known) = Some(value.clone());
                Ok(value)
            })
        }
        // Guava's `Strings` (WorldEdit's command parser pads with padStart).
        ("Strings", "padStart" | "padEnd") => {
            let text = a[0].text();
            let (min, pad) = (a[1].int().max(0) as usize, a.get(2).map_or(' ', |c| c.text().chars().next().unwrap_or(' ')));
            let count = text.encode_utf16().count();
            if count >= min {
                s(&text)
            } else {
                let fill: String = std::iter::repeat_n(pad, min - count).collect();
                s(&if name == "padStart" { fill + &text } else { text + &fill })
            }
        }
        ("Strings", "repeat") => s(&a[0].text().repeat(a[1].int().max(0) as usize)),
        ("Strings", "isNullOrEmpty") => V::Bool(a[0].is_null() || a[0].text().is_empty()),
        ("Strings", "nullToEmpty") => if a[0].is_null() { s("") } else { a[0].clone() },
        ("Strings", "emptyToNull") => if a[0].is_null() || a[0].text().is_empty() { V::Null } else { a[0].clone() },
        ("Strings", "commonPrefix" | "commonSuffix") => {
            let (x, y): (Vec<char>, Vec<char>) = (a[0].text().chars().collect(), a[1].text().chars().collect());
            if name == "commonPrefix" {
                s(&x.iter().zip(&y).take_while(|(p, q)| p == q).map(|(p, _)| *p).collect::<String>())
            } else {
                let n = x.iter().rev().zip(y.iter().rev()).take_while(|(p, q)| p == q).count();
                s(&x[x.len() - n..].iter().collect::<String>())
            }
        }
        // `MessageFormat.format(pattern, args...)`: `{0}`, `{1}` ... filled in, `''` a quote.
        ("MessageFormat", "format") => {
            let args: Vec<V> = match &a[1..] {
                [V::Arr(items)] => lock(items).clone(),
                more => more.to_vec(),
            };
            s(&message_format(&a[0].text(), &args))
        }
        // `URLDecoder.decode(text[, charset])`: %XX escapes as UTF-8 bytes, `+` a space.
        ("URLDecoder", "decode") => {
            let text = a[0].text();
            let bytes = text.as_bytes();
            let mut out = Vec::with_capacity(bytes.len());
            let mut i = 0;
            while i < bytes.len() {
                match bytes[i] {
                    b'+' => out.push(b' '),
                    b'%' if i + 2 < bytes.len() => {
                        match u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                            Ok(b) => {
                                out.push(b);
                                i += 2;
                            }
                            Err(_) => out.push(b'%'),
                        }
                    }
                    b => out.push(b),
                }
                i += 1;
            }
            s(&String::from_utf8_lossy(&out))
        }
        // Guava's `MoreObjects.toStringHelper(object or name)`: `add(name, value)`..., then `toString()` is
        // `Name{a=1, b=x}` (piston's AutoValue keys print so).
        ("MoreObjects" | "Objects", "toStringHelper") => {
            let name = match &a[0] {
                V::Str(t) => t.to_string(),
                V::Class(c) => crate::java_class_name(c).rsplit(['.', '$']).next().unwrap_or(c).to_owned(),
                other => crate::java_class_name(crate::class_of(other)).rsplit(['.', '$']).next().unwrap_or_default().to_owned(),
            };
            native(JToStringHelper(Mutex::new((name, Vec::new(), false))))
        }
        // Guava's `ComparisonChain.start().compare(a, b)...result()` (FAWE's MinecraftVersion.compareTo).
        ("ComparisonChain", "start") => native(JComparisonChain(Mutex::new(0))),
        // Guava's primitive helpers: `Booleans.toArray(list)` and its siblings (WorldEdit's block ticking table).
        ("Booleans" | "Ints" | "Longs" | "Doubles" | "Floats" | "Shorts" | "Bytes" | "Chars", "toArray") => match a[0].items() {
            Ok(items) => arr(items),
            Err(e) => return Some(Err(e)),
        },
        ("Booleans" | "Ints" | "Longs" | "Doubles" | "Floats" | "Shorts" | "Bytes" | "Chars", "asList") => match a {
            [V::Arr(items)] => list(lock(items).clone()),
            more => list(more.to_vec()),
        },
        ("Ints", "checkedCast" | "saturatedCast") => V::Int(a[0].long().clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32),
        ("Functions", "compose") => {
            let (g, f) = (a[0].clone(), a[1].clone());
            crate::func(move |args| callf(&f, args).and_then(|v| callf(&g, &[v])))
        }
        ("Math", "random") => V::Double((time_nanos() % 1_000_000) as f64 / 1e6),
        // parseInt(s, radix): a second argument is the radix (Integer.parseInt("4c", 16) is 76).
        ("Integer" | "Short" | "Byte", "parseInt" | "valueOf" | "parseShort" | "parseByte") => match &a[0] {
            V::Str(t) => match if a.len() > 1 { i32::from_str_radix(t, a[1].int().clamp(2, 36) as u32) } else { t.parse() } {
                Ok(n) => V::Int(n),
                Err(_) => return Some(Err(num(class))),
            },
            other => V::Int(other.int()),
        },
        ("Long", "parseLong" | "valueOf") => match &a[0] {
            V::Str(t) => match if a.len() > 1 { i64::from_str_radix(t, a[1].int().clamp(2, 36) as u32) } else { t.parse() } {
                Ok(n) => V::Long(n),
                Err(_) => return Some(Err(num(class))),
            },
            other => V::Long(other.long()),
        },
        ("Double" | "Float", "parseDouble" | "parseFloat" | "valueOf") => match &a[0] {
            V::Str(t) => match t.trim().parse() {
                Ok(n) => V::Double(n),
                Err(_) => return Some(Err(num(class))),
            },
            other => V::Double(other.double()),
        },
        ("Boolean", "parseBoolean") => V::Bool(a[0].text().eq_ignore_ascii_case("true")),
        // `java.lang.reflect.Array`: arrays made, measured and indexed by reflection.
        ("Array", "newInstance") if a.len() == 2 => {
            let kind = match &a[0] {
                V::Class(k) => k.rsplit('.').next().unwrap_or(k).to_owned(),
                other => other.text(),
            };
            let sizes = match &a[1] {
                V::Arr(_) => a[1].items().unwrap_or_default(),
                one => vec![one.clone()],
            };
            new_array(&kind, &sizes)
        }
        ("Array", "getLength") if a.len() == 1 => match a[0].items() {
            Ok(items) => V::Int(items.len() as i32),
            Err(error) => return Some(Err(error)),
        },
        ("Array", "get" | "getInt" | "getLong" | "getDouble" | "getBoolean" | "getChar") if a.len() == 2 => return Some(index(&a[0], &a[1])),
        ("Array", "set" | "setInt" | "setLong" | "setDouble" | "setBoolean" | "setChar") if a.len() == 3 => return Some(store(&a[0], &a[1], a[2].clone()).map(|_| V::Null)),
        // `Predicate.not(p)`: the opposite test.
        ("Predicate", "not") if a.len() == 1 => {
            let inner = a[0].clone();
            func(move |x| Ok(V::Bool(!call(&inner, "test", x)?.truth())))
        }
        ("Collections", "nCopies") if a.len() == 2 => list(vec![a[1].clone(); a[0].int().max(0) as usize]),
        // `ServiceLoader.load(Service.class)`: no `META-INF/services` providers are loaded here, so none,
        // as Java finds when the jar lists none (shaded Adventure then uses its defaults).
        ("ServiceLoader", "load" | "loadInstalled") => list(Vec::new()),
        // `reduce(true, Boolean::logicalAnd)` and friends.
        ("Boolean", "logicalAnd") if a.len() == 2 => V::Bool(a[0].truth() && a[1].truth()),
        ("Boolean", "logicalOr") if a.len() == 2 => V::Bool(a[0].truth() || a[1].truth()),
        ("Boolean", "logicalXor") if a.len() == 2 => V::Bool(a[0].truth() != a[1].truth()),
        ("Boolean", "compare") if a.len() == 2 => V::Int(i32::from(a[0].truth()) - i32::from(a[1].truth())),
        ("Long", "sum") if a.len() == 2 => V::Long(a[0].long().wrapping_add(a[1].long())),
        ("Double", "sum") if a.len() == 2 => V::Double(a[0].double() + a[1].double()),
        ("Long" | "Double", "max") if a.len() == 2 => if cmp(&a[0], &a[1]).is_ge() { a[0].clone() } else { a[1].clone() },
        ("Long" | "Double", "min") if a.len() == 2 => if cmp(&a[0], &a[1]).is_le() { a[0].clone() } else { a[1].clone() },
        // System properties read as numbers or flags: none are set, so the default (or null, false).
        ("Long" | "Integer", "getLong" | "getInteger") => a.get(1).cloned().unwrap_or(V::Null),
        ("Boolean", "getBoolean") => V::Bool(false),
        ("Integer", "numberOfLeadingZeros") => V::Int(a[0].int().leading_zeros() as i32),
        ("Integer", "numberOfTrailingZeros") => V::Int(a[0].int().trailing_zeros() as i32),
        ("Integer", "bitCount") => V::Int(a[0].int().count_ones() as i32),
        ("Integer", "highestOneBit") => V::Int(if a[0].int() == 0 { 0 } else { ((1u32 << 31) >> (a[0].int() as u32).leading_zeros()) as i32 }),
        ("Long", "numberOfLeadingZeros") => V::Int(a[0].long().leading_zeros() as i32),
        ("Long", "numberOfTrailingZeros") => V::Int(a[0].long().trailing_zeros() as i32),
        ("Long", "bitCount") => V::Int(a[0].long().count_ones() as i32),
        ("Character", "isJavaIdentifierPart") => V::Bool(a[0].text().chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')),
        ("Character", "isJavaIdentifierStart") => V::Bool(a[0].text().chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')),
        ("Boolean", "valueOf") => match &a[0] {
            V::Bool(b) => V::Bool(*b),
            other => V::Bool(other.text().eq_ignore_ascii_case("true")),
        },
        // `Integer.toString(255, 16)` is "ff": a radix other than ten, lowercase, '-' for a negative.
        ("Integer" | "Long", "toString") if a.len() == 2 && a[1].int() != 10 && (2..=36).contains(&a[1].int()) => {
            let (radix, n) = (a[1].int() as u32, a[0].long());
            let mut digits = Vec::new();
            let mut left = n.unsigned_abs();
            loop {
                digits.push(std::char::from_digit((left % radix as u64) as u32, radix).unwrap_or('0'));
                left /= radix as u64;
                if left == 0 {
                    break;
                }
            }
            if n < 0 {
                digits.push('-');
            }
            s(&digits.iter().rev().collect::<String>())
        }
        ("Integer" | "Long" | "Double" | "Boolean" | "String" | "Character" | "Objects", "toString" | "valueOf") if !a.is_empty() => s(&a[0].text()),
        ("Integer" | "Long" | "Double", "compare") => V::Int(cmp(&a[0], &a[1]) as i32),
        ("Integer", "max") => V::Int(a[0].int().max(a[1].int())),
        ("Integer", "min") => V::Int(a[0].int().min(a[1].int())),
        ("Integer", "sum") => V::Int(a[0].int().wrapping_add(a[1].int())),
        ("Double", "isNaN") => V::Bool(x().is_nan()),
        ("Character", "isDigit") => V::Bool(a[0].text().chars().all(|c| c.is_numeric())),
        ("Character", "isLetter") => V::Bool(a[0].text().chars().all(char::is_alphabetic)),
        ("Character", "isLetterOrDigit") => V::Bool(a[0].text().chars().all(char::is_alphanumeric)),
        ("Character", "isWhitespace") => V::Bool(a[0].text().chars().all(char::is_whitespace)),
        ("Character", "toLowerCase") => V::Char(a[0].text().to_lowercase().chars().next().unwrap_or('\0')),
        ("Character", "toUpperCase") => V::Char(a[0].text().to_uppercase().chars().next().unwrap_or('\0')),
        ("String", "join") => {
            let parts = if a.len() == 2 && !matches!(a[1], V::Str(_)) { a[1].items() } else { Ok(a[1..].to_vec()) };
            match parts {
                Ok(parts) => s(&parts.iter().map(V::text).collect::<Vec<_>>().join(&a[0].text())),
                Err(e) => return Some(Err(e)),
            }
        }
        // `String.format(Locale.ENGLISH, template, args...)`: a Locale first (its name or tag here) isn't the template.
        ("String", "format") if a.len() >= 2 && matches!(a[1], V::Str(_)) && is_locale(&a[0]) => s(&java_format(&a[1].text(), &a[2..])),
        ("String", "format") => s(&java_format(&a[0].text(), &a[1..])),
        ("System", "currentTimeMillis") => V::Long((time_nanos() / 1_000_000) as i64),
        ("System", "nanoTime") => V::Long(time_nanos() as i64),
        ("System", "lineSeparator") => s("\n"),
        ("Arrays", "asList") | ("List" | "Set", "of" | "copyOf") => {
            let items = match a {
                // `copyOf(collection)` takes any collection: a map's `values()` view, a plugin's own.
                [one] if name == "copyOf" => match one.items() {
                    Ok(i) => i,
                    Err(e) => return Some(Err(e)),
                },
                [one @ (V::Arr(_) | V::List(_) | V::Set(_))] => match one.items() {
                    Ok(i) => i,
                    Err(e) => return Some(Err(e)),
                },
                _ => a.to_vec(),
            };
            if class == "Set" {
                let mut set = JMap::new();
                for item in items {
                    set.insert(item.key(), (item, V::Null));
                }
                V::Set(Arc::new(Mutex::new(set)))
            } else {
                list(items)
            }
        }
        ("Arrays", "stream") | ("Stream", "of") => match a {
            [one @ V::Arr(_)] => match one.items() {
                Ok(i) => V::Stream(Arc::new(Mutex::new(i))),
                Err(e) => return Some(Err(e)),
            },
            _ => V::Stream(Arc::new(Mutex::new(a.to_vec()))),
        },
        ("Arrays", "toString") => match a[0].items() {
            Ok(i) => s(&format!("[{}]", i.iter().map(V::text).collect::<Vec<_>>().join(", "))),
            Err(e) => return Some(Err(e)),
        },
        ("Arrays", "copyOf" | "copyOfRange") => {
            let items = match a[0].items() {
                Ok(i) => i,
                Err(e) => return Some(Err(e)),
            };
            let (from, to) = if name == "copyOf" { (0, a[1].int() as usize) } else { (a[1].int() as usize, a[2].int() as usize) };
            // Past the end: the element type's zero, as Java pads an `int[]` with 0 (not null).
            let zero = match items.first() {
                Some(V::Int(_)) => V::Int(0),
                Some(V::Long(_)) => V::Long(0),
                Some(V::Double(_)) => V::Double(0.0),
                Some(V::Char(_)) => V::Char(' '),
                Some(V::Bool(_)) => V::Bool(false),
                _ => V::Null,
            };
            arr((from..to).map(|i| items.get(i).cloned().unwrap_or_else(|| zero.clone())).collect())
        }
        ("Map", "of") => {
            let mut map = JMap::new();
            for pair in a.chunks(2) {
                if let [k, v] = pair {
                    map.insert(k.key(), (k.clone(), v.clone()));
                }
            }
            V::Map(Arc::new(Mutex::new(map)))
        }
        ("Map", "entry") => V::Entry(Arc::new((a[0].clone(), a[1].clone()))),
        // `Map.ofEntries(entry(k, v), ...)`: the entries one by one, or the varargs array whole.
        ("Map", "ofEntries") => {
            let entries = match a {
                [one @ V::Arr(_)] => one.items().unwrap_or_default(),
                _ => a.to_vec(),
            };
            let mut map = JMap::new();
            for entry in entries {
                if let V::Entry(pair) = &entry {
                    map.insert(pair.0.key(), (pair.0.clone(), pair.1.clone()));
                }
            }
            V::Map(Arc::new(Mutex::new(map)))
        }
        ("Double" | "Float", "isInfinite") => V::Bool(x().is_infinite()),
        ("Double" | "Float", "isFinite") => V::Bool(x().is_finite()),
        ("Float", "isNaN") => V::Bool(x().is_nan()),
        ("Map", "copyOf") | ("Collections", "unmodifiableMap" | "unmodifiableList" | "unmodifiableSet" | "unmodifiableCollection") => a[0].clone(),
        ("Collections", "emptyList") => list(Vec::new()),
        ("Collections", "emptyMap") => V::Map(Arc::new(Mutex::new(JMap::new()))),
        ("Collections", "emptySet") => V::Set(Arc::new(Mutex::new(JMap::new()))),
        ("Collections", "singletonList" | "singleton") => list(vec![a[0].clone()]),
        ("Collections", "sort") => return Some(call(&a[0], "sort", if a.len() > 1 { &a[1..] } else { &[V::Null] })),
        ("Collections", "reverse") => {
            if let V::List(items) | V::Arr(items) = &a[0] {
                lock(items).reverse();
            }
            V::Null
        }
        // Guava's `HashBiMap.create()`: a map; its `inverse()` (below, on any map) is worked out when asked.
        ("HashBiMap" | "EnumBiMap" | "EnumHashBiMap", "create") => return Some(crate::new_other("LinkedHashMap", &[])),
        // Guava's `Verify.verify(condition[, message, args...])`.
        ("Verify", "verify") => {
            if a.first().is_some_and(|c| !c.truth()) {
                return Some(Err(exc("VerifyException", a.get(1).map(V::text).unwrap_or_default())));
            }
            V::Null
        }
        ("Verify", "verifyNotNull") => {
            if a.first().is_none_or(V::is_null) {
                return Some(Err(exc("VerifyException", a.get(1).map(V::text).unwrap_or_else(|| "expected a non-null reference".into()))));
            }
            a[0].clone()
        }
        ("Collections", "addAll") => {
            // `addAll(c, T... items)` given an array (`clazz.getInterfaces()`): its items, as varargs take them.
            let items = match &a[1..] {
                [V::Arr(items)] => lock(items).clone(),
                rest => rest.to_vec(),
            };
            for item in &items {
                if let Err(e) = call(&a[0], "add", std::slice::from_ref(item)) {
                    return Some(Err(e));
                }
            }
            V::Bool(true)
        }
        ("Objects", "equals") => V::Bool(same(&a[0], &a[1]) || (!a[0].is_null() && equals(&a[0], &a[1]).is_ok_and(|e| e.truth()))),
        ("Objects", "isNull") => V::Bool(a[0].is_null()),
        ("Objects", "nonNull") => V::Bool(!a[0].is_null()),
        // As Java defines them, from each value's own `hashCode`: hashing the text rendered whole trees (Adventure's
        // components hash with `Objects.hash`, and FAWE keys its commands, which hold them, in maps).
        ("Objects", "hashCode") => V::Int(a.first().filter(|v| !v.is_null()).map_or(0, |v| call(v, "hashCode", &[]).map_or(0, |h| h.int()))),
        ("Objects", "hash") => {
            let values = match a {
                [V::Arr(items)] => lock(items).clone(),
                rest => rest.to_vec(),
            };
            V::Int(values.iter().fold(1i32, |h, v| h.wrapping_mul(31).wrapping_add(if v.is_null() { 0 } else { call(v, "hashCode", &[]).map_or(0, |x| x.int()) })))
        }
        ("Objects", "requireNonNull") => {
            if a[0].is_null() {
                return Some(Err(exc("NullPointerException", a.get(1).map(V::text).unwrap_or_default())));
            }
            a[0].clone()
        }
        ("Objects", "requireNonNullElse") => {
            if a[0].is_null() { a[1].clone() } else { a[0].clone() }
        }
        // `Objects.requireNonNullElseGet(value, () -> fallback)`.
        ("Objects", "requireNonNullElseGet") => {
            if a[0].is_null() {
                return Some(callf(&a[1], &[]));
            }
            a[0].clone()
        }
        ("Optional", "of" | "ofNullable") => opt(a.first().cloned()),
        ("Optional", "empty") => opt(None),
        ("UUID", "fromString") => {
            let text = a[0].text();
            let parts: Vec<&str> = text.split('-').collect();
            if parts.len() != 5 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_hexdigit())) {
                return Some(Err(exc("IllegalArgumentException", format!("Invalid UUID string: {text}"))));
            }
            s(&text.to_lowercase())
        }
        ("UUID", "randomUUID") => s(&format!("{:08x}-0000-4000-8000-{:012x}", time_nanos() as u32, time_nanos() & 0xffff_ffff_ffff)),
        ("Collectors", "toList" | "toUnmodifiableList") => s("toList"),
        ("Collectors", "toSet" | "toUnmodifiableSet") => s("toSet"),
        // `toCollection(TreeSet::new)`: a set or a list, by what the supplier makes.
        ("Collectors", "toCollection") => match call(&a[0], "get", &[]) {
            Ok(V::Set(_)) => s("toSet"),
            Ok(_) => s("toList"),
            Err(error) => return Some(Err(error)),
        },
        ("Collectors", "joining") => s(&format!("joining:{}", a.first().map(V::text).unwrap_or_default())),
        ("IntStream", "range") => V::Stream(Arc::new(Mutex::new((a[0].int()..a[1].int()).map(V::Int).collect()))),
        ("IntStream", "rangeClosed") => V::Stream(Arc::new(Mutex::new((a[0].int()..=a[1].int()).map(V::Int).collect()))),
        _ => return None,
    }))
}

/// `String.format`: `%s`, `%d`, `%.2f`, `%%`, `%n`.
fn java_format(pattern: &str, args: &[V]) -> String {
    let args: Vec<V> = match args {
        [one @ V::Arr(_)] => one.items().unwrap_or_default(),
        _ => args.to_vec(),
    };
    let (mut out, mut used, mut chars) = (String::new(), 0, pattern.chars().peekable());
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&next) = chars.peek() {
            if next.is_ascii_alphabetic() || next == '%' {
                break;
            }
            spec.push(next);
            chars.next();
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('n') => out.push('\n'),
            Some(kind) => {
                let value = args.get(used).cloned().unwrap_or(V::Null);
                used += 1;
                match (kind, spec.strip_prefix('.').and_then(|p| p.parse::<usize>().ok())) {
                    ('f', precision) => out.push_str(&format!("{:.*}", precision.unwrap_or(6), value.double())),
                    _ => out.push_str(&value.text()),
                }
            }
            None => {}
        }
    }
    out
}
