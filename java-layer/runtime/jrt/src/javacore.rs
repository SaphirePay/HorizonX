//! java.lang / java.util calls that plugins' own code makes and jrt never answered (the bytecode audit of the 450
//! plugin jars on disk, plugin-sweep/audit/missing_sure.json, by how many plugins make each): `Enumeration`,
//! `StringTokenizer`, `StringJoiner`, `BitSet`, `subSequence`, `Character.toCodePoint`, `Integer/Long.rotateLeft`
//! and `reverseBytes`, `Math.sinh/cosh/tanh/rint/expm1/log1p`, `Arrays.deepToString/deepEquals/deepHashCode`,
//! `Collections.swap`, `ArrayList.trimToSize`, a thread's context class loader, stack traces with frames (code that
//! reads `getStackTrace()[2]` for its caller's name threw on the empty array), `SecurityManager` (none, as on a
//! modern JVM), `ResourceBundle` / `PropertyResourceBundle` over a plugin's `.properties`, and `HexFormat`.
//! First in jrt's chains: every hook returns None quickly for a name that isn't its own.
use crate::{R, V, arr, call, call_static, exc, list, lock, native, s};
use std::sync::Mutex;

// ---- java.security.MessageDigest: MD5, SHA-1, SHA-256, SHA-384, SHA-512 over the bytes given so far ----

pub struct JDigest {
    algorithm: String,
    data: Mutex<Vec<u8>>,
}

fn digest_of(algorithm: &str, bytes: &[u8]) -> Vec<u8> {
    use sha2::Digest;
    match algorithm {
        "MD5" => md5::Md5::digest(bytes).to_vec(),
        "SHA-1" | "SHA1" | "SHA" => sha1::Sha1::digest(bytes).to_vec(),
        "SHA-384" => sha2::Sha384::digest(bytes).to_vec(),
        "SHA-512" => sha2::Sha512::digest(bytes).to_vec(),
        _ => sha2::Sha256::digest(bytes).to_vec(),
    }
}

fn digest_new(a: &[V]) -> R {
    let algorithm = a.first().map(V::text).unwrap_or_default().to_uppercase();
    match algorithm.as_str() {
        "MD5" | "SHA-1" | "SHA1" | "SHA" | "SHA-256" | "SHA256" | "SHA-384" | "SHA-512" => {
            let algorithm = if algorithm == "SHA256" { "SHA-256".to_owned() } else { algorithm };
            Ok(native(JDigest { algorithm, data: Mutex::new(Vec::new()) }))
        }
        _ => Err(exc("NoSuchAlgorithmException", format!("{algorithm} MessageDigest not available"))),
    }
}

fn digest_method(d: &JDigest, name: &str, a: &[V]) -> Option<R> {
    let take = |a: &[V]| -> Vec<u8> {
        match a {
            [V::Int(b)] => vec![*b as u8],
            [data] => crate::javaio::bytes_of(data).unwrap_or_default(),
            [data, off, len] => {
                let all = crate::javaio::bytes_of(data).unwrap_or_default();
                let (off, len) = (off.int().max(0) as usize, len.int().max(0) as usize);
                all.get(off..(off + len).min(all.len())).unwrap_or_default().to_vec()
            }
            _ => Vec::new(),
        }
    };
    Some(Ok(match name {
        "update" => {
            lock(&d.data).extend(take(a));
            V::Null
        }
        // `digest()` / `digest(bytes)`: the hash of everything given, then the digest starts over, as Java's does.
        "digest" => {
            let mut data = std::mem::take(&mut *lock(&d.data));
            data.extend(take(a));
            crate::javaio::to_byte_array(&digest_of(&d.algorithm, &data))
        }
        "reset" => {
            lock(&d.data).clear();
            V::Null
        }
        "getAlgorithm" => s(&d.algorithm),
        "getDigestLength" => V::Int(digest_of(&d.algorithm, b"").len() as i32),
        _ => return None,
    }))
}

// ---- StringTokenizer: the tokens between delimiters (and the delimiters, when asked) ----

pub struct JTokenizer(Mutex<TokenState>);

struct TokenState {
    text: Vec<char>,
    at: usize,
    delimiters: Vec<char>,
    with_delimiters: bool,
}

impl TokenState {
    fn skip(&mut self) {
        if !self.with_delimiters {
            while self.at < self.text.len() && self.delimiters.contains(&self.text[self.at]) {
                self.at += 1;
            }
        }
    }
    fn next(&mut self) -> Option<String> {
        self.skip();
        if self.at >= self.text.len() {
            return None;
        }
        let start = self.at;
        if self.delimiters.contains(&self.text[self.at]) {
            // Only with `returnDelims`: the delimiter is a token of its own.
            self.at += 1;
            return Some(self.text[start..self.at].iter().collect());
        }
        while self.at < self.text.len() && !self.delimiters.contains(&self.text[self.at]) {
            self.at += 1;
        }
        Some(self.text[start..self.at].iter().collect())
    }
    fn count(&self) -> i32 {
        let mut copy = TokenState { text: self.text.clone(), at: self.at, delimiters: self.delimiters.clone(), with_delimiters: self.with_delimiters };
        let mut n = 0;
        while copy.next().is_some() {
            n += 1;
        }
        n
    }
}

fn tokenizer_method(t: &JTokenizer, name: &str, a: &[V]) -> Option<R> {
    let mut state = lock(&t.0);
    Some(Ok(match name {
        "hasMoreTokens" | "hasMoreElements" => {
            state.skip();
            V::Bool(state.at < state.text.len())
        }
        "nextToken" | "nextElement" => {
            if name == "nextToken" && a.len() == 1 {
                state.delimiters = a[0].text().chars().collect();
            }
            match state.next() {
                Some(token) => s(&token),
                None => return Some(Err(exc("NoSuchElementException", "no more tokens"))),
            }
        }
        "countTokens" => V::Int(state.count()),
        _ => return None,
    }))
}

// ---- StringJoiner ----

pub struct JJoiner(Mutex<JoinState>);

struct JoinState {
    delimiter: String,
    prefix: String,
    suffix: String,
    parts: Vec<String>,
    empty: Option<String>,
}

impl JoinState {
    fn text(&self) -> String {
        if self.parts.is_empty() {
            if let Some(empty) = &self.empty {
                return empty.clone();
            }
        }
        format!("{}{}{}", self.prefix, self.parts.join(&self.delimiter), self.suffix)
    }
}

fn joiner_method(j: &JJoiner, this: &V, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("add", 1) => {
            lock(&j.0).parts.push(if a[0].is_null() { "null".to_owned() } else { a[0].text() });
            this.clone()
        }
        ("setEmptyValue", 1) => {
            lock(&j.0).empty = Some(a[0].text());
            this.clone()
        }
        ("merge", 1) => {
            if let Some(other) = a[0].native::<JJoiner>() {
                let joined = {
                    let o = lock(&other.0);
                    (!o.parts.is_empty()).then(|| o.parts.join(&o.delimiter))
                };
                if let Some(joined) = joined {
                    lock(&j.0).parts.push(joined);
                }
            }
            this.clone()
        }
        ("toString", 0) => s(&lock(&j.0).text()),
        ("length", 0) => V::Int(lock(&j.0).text().encode_utf16().count() as i32),
        _ => return None,
    }))
}

// ---- BitSet: 64 bits a word ----

pub struct JBitSet(Mutex<Vec<u64>>);

fn bits_of(value: &V) -> Option<Vec<u64>> {
    value.native::<JBitSet>().map(|b| lock(&b.0).clone())
}

fn bit_get(words: &[u64], i: usize) -> bool {
    words.get(i / 64).is_some_and(|w| w >> (i % 64) & 1 == 1)
}

fn bit_put(words: &mut Vec<u64>, i: usize, on: bool) {
    if words.len() <= i / 64 {
        if !on {
            return;
        }
        words.resize(i / 64 + 1, 0);
    }
    if on {
        words[i / 64] |= 1 << (i % 64);
    } else {
        words[i / 64] &= !(1 << (i % 64));
    }
}

fn bits_length(words: &[u64]) -> usize {
    words.iter().rposition(|&w| w != 0).map_or(0, |i| i * 64 + 64 - words[i].leading_zeros() as usize)
}

fn bitset_method(b: &JBitSet, name: &str, a: &[V]) -> Option<R> {
    let index = |at: usize| -> Result<usize, V> {
        let i = a.get(at).map_or(0, V::int);
        if i < 0 { Err(exc("IndexOutOfBoundsException", format!("bitIndex < 0: {i}"))) } else { Ok(i as usize) }
    };
    let range = |from: usize, to: usize, on: Option<bool>, words: &mut Vec<u64>| {
        for i in from..to {
            let now = on.unwrap_or(!bit_get(words, i));
            bit_put(words, i, now);
        }
    };
    let result = (|| -> R {
        let mut words = lock(&b.0);
        Ok(match (name, a.len()) {
            ("get", 1) => V::Bool(bit_get(&words, index(0)?)),
            ("set", 1) => {
                bit_put(&mut words, index(0)?, true);
                V::Null
            }
            ("set", 2) if matches!(a[1], V::Bool(_)) => {
                bit_put(&mut words, index(0)?, a[1].truth());
                V::Null
            }
            ("set", 2) => {
                range(index(0)?, index(1)?, Some(true), &mut words);
                V::Null
            }
            ("set", 3) => {
                range(index(0)?, index(1)?, Some(a[2].truth()), &mut words);
                V::Null
            }
            ("clear", 0) => {
                words.clear();
                V::Null
            }
            ("clear", 1) => {
                bit_put(&mut words, index(0)?, false);
                V::Null
            }
            ("clear", 2) => {
                range(index(0)?, index(1)?, Some(false), &mut words);
                V::Null
            }
            ("flip", 1) => {
                let i = index(0)?;
                let now = !bit_get(&words, i);
                bit_put(&mut words, i, now);
                V::Null
            }
            ("flip", 2) => {
                range(index(0)?, index(1)?, None, &mut words);
                V::Null
            }
            ("nextSetBit", 1) => {
                let from = index(0)?;
                V::Int((from..words.len() * 64).find(|&i| bit_get(&words, i)).map_or(-1, |i| i as i32))
            }
            ("nextClearBit", 1) => {
                let from = index(0)?;
                V::Int((from..).find(|&i| !bit_get(&words, i)).unwrap_or(from) as i32)
            }
            ("previousSetBit", 1) => {
                let from = a[0].int();
                V::Int((0..=from.max(-1)).rev().find(|&i| i >= 0 && bit_get(&words, i as usize)).unwrap_or(-1))
            }
            ("cardinality", 0) => V::Int(words.iter().map(|w| w.count_ones() as i32).sum()),
            ("length", 0) => V::Int(bits_length(&words) as i32),
            ("size", 0) => V::Int((words.len().max(1) * 64) as i32),
            ("isEmpty", 0) => V::Bool(words.iter().all(|&w| w == 0)),
            ("and" | "or" | "xor" | "andNot", 1) => {
                let other = bits_of(&a[0]).unwrap_or_default();
                let n = words.len().max(other.len());
                words.resize(n, 0);
                for i in 0..n {
                    let o = other.get(i).copied().unwrap_or(0);
                    words[i] = match name {
                        "and" => words[i] & o,
                        "or" => words[i] | o,
                        "xor" => words[i] ^ o,
                        _ => words[i] & !o,
                    };
                }
                V::Null
            }
            ("intersects", 1) => {
                let other = bits_of(&a[0]).unwrap_or_default();
                V::Bool(words.iter().zip(&other).any(|(x, y)| x & y != 0))
            }
            ("toLongArray", 0) => {
                let n = bits_length(&words).div_ceil(64);
                arr(words[..n].iter().map(|&w| V::Long(w as i64)).collect())
            }
            ("toString", 0) => {
                let on: Vec<String> = (0..words.len() * 64).filter(|&i| bit_get(&words, i)).map(|i| i.to_string()).collect();
                s(&format!("{{{}}}", on.join(", ")))
            }
            ("clone", 0) => native(JBitSet(Mutex::new(words.clone()))),
            ("equals", 1) => {
                let other = bits_of(&a[0]);
                V::Bool(other.is_some_and(|o| {
                    let n = words.len().max(o.len());
                    (0..n).all(|i| words.get(i).copied().unwrap_or(0) == o.get(i).copied().unwrap_or(0))
                }))
            }
            ("hashCode", 0) => {
                let n = bits_length(&words).div_ceil(64);
                let mut h: u64 = 1234;
                for i in (0..n).rev() {
                    h ^= words[i].wrapping_mul(i as u64 + 1);
                }
                V::Int(((h >> 32) ^ h) as i32)
            }
            ("stream", 0) => list((0..words.len() * 64).filter(|&i| bit_get(&words, i)).map(|i| V::Int(i as i32)).collect()),
            _ => return Err(V::Null),
        })
    })();
    match result {
        Err(V::Null) => None,
        other => Some(other),
    }
}

// ---- Stack traces: frames whose getters answer ----

/// A frame of a stack trace. The converted code keeps none of Java's, so a trace is frames with neutral names:
/// enough for code that logs `getStackTrace()[2].getMethodName()` or walks the trace.
pub struct JFrame {
    class: String,
    method: String,
}

/// `Thread.currentThread().getStackTrace()`: `Thread.getStackTrace` first, as Java's, then the calls running.
fn frames() -> V {
    let mut rows = vec![("java.lang.Thread".to_owned(), "getStackTrace".to_owned())];
    rows.extend(crate::stack_watch::frames());
    frames_of(&rows)
}

/// A trace's frames as `StackTraceElement`s; never shorter than four (code reads `[2]` blindly), padded with
/// neutral frames when the calls running are fewer.
pub fn frames_of(rows: &[(String, String)]) -> V {
    let mut out: Vec<V> = rows.iter().map(|(class, method)| native(JFrame { class: class.clone(), method: method.clone() })).collect();
    while out.len() < 4 {
        out.push(native(JFrame { class: "plugin".to_owned(), method: "caller".to_owned() }));
    }
    arr(out)
}

fn frame_method(f: &JFrame, name: &str) -> Option<R> {
    Some(Ok(match name {
        "getClassName" => s(&f.class),
        "getMethodName" => s(&f.method),
        "getFileName" => V::Null,
        "getLineNumber" => V::Int(-1),
        "isNativeMethod" => V::Bool(false),
        "getModuleName" | "getModuleVersion" | "getClassLoaderName" => V::Null,
        "toString" => s(&format!("{}.{}(Unknown Source)", f.class, f.method)),
        _ => return None,
    }))
}

// ---- ResourceBundle: a .properties file's keys ----

pub struct JBundle(V);

fn bundle_from(stream: &V) -> R {
    let properties = crate::new_other("Properties", &[])?;
    call(&properties, "load", &[stream.clone()])?;
    Ok(native(JBundle(properties)))
}

fn bundle_method(b: &JBundle, name: &str, a: &[V]) -> Option<R> {
    Some(match (name, a.len()) {
        ("getString" | "getObject", 1) => match call(&b.0, "getProperty", &[a[0].clone()]) {
            Ok(V::Null) => Err(exc("MissingResourceException", format!("Can't find resource for bundle, key {}", a[0].text()))),
            other => other,
        },
        ("containsKey", 1) => call(&b.0, "containsKey", &[a[0].clone()]),
        ("keySet", 0) => call(&b.0, "stringPropertyNames", &[]),
        ("getKeys", 0) => call(&b.0, "stringPropertyNames", &[]).and_then(|keys| call(&keys, "iterator", &[])),
        ("getLocale", 0) => Ok(s("")),
        ("getBaseBundleName", 0) => Ok(V::Null),
        _ => return None,
    })
}

/// `ResourceBundle.getBundle("messages", locale)`: the plugin's `messages_en_US.properties`, `messages_en.properties`
/// or `messages.properties`, the first one it has, as Java falls back.
fn get_bundle(a: &[V]) -> R {
    let base = a.first().map(V::text).unwrap_or_default().replace('.', "/");
    let locale = a.get(1).filter(|v| !v.is_null()).map(V::text).unwrap_or_default();
    let mut names = Vec::new();
    if !locale.is_empty() {
        names.push(format!("{base}_{locale}.properties"));
        if let Some((language, _)) = locale.split_once('_') {
            names.push(format!("{base}_{language}.properties"));
        }
    }
    names.push(format!("{base}.properties"));
    let loader = call_static("ClassLoader", "getSystemClassLoader", &[])?;
    for name in names {
        if let Ok(stream) = call(&loader, "getResourceAsStream", &[s(&name)]) {
            if !stream.is_null() {
                return bundle_from(&stream);
            }
        }
    }
    Err(exc("MissingResourceException", format!("Can't find bundle for base name {base}, locale {locale}")))
}

// ---- HexFormat ----

pub struct JHex {
    upper: bool,
}

fn hex_method(h: &JHex, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("formatHex", 1) => {
            let bytes: Vec<i32> = a[0].items().unwrap_or_default().iter().map(V::int).collect();
            let text: String = bytes.iter().map(|b| if h.upper { format!("{:02X}", *b as u8) } else { format!("{:02x}", *b as u8) }).collect();
            s(&text)
        }
        ("parseHex", 1) => {
            let text = a[0].text();
            if text.len() % 2 != 0 {
                return Some(Err(exc("IllegalArgumentException", "an odd number of hex digits")));
            }
            let mut out = Vec::new();
            for i in (0..text.len()).step_by(2) {
                match u8::from_str_radix(&text[i..i + 2], 16) {
                    Ok(b) => out.push(V::Int(b as i8 as i32)),
                    Err(_) => return Some(Err(exc("IllegalArgumentException", format!("not hex: {text}")))),
                }
            }
            arr(out)
        }
        ("withUpperCase", 0) => native(JHex { upper: true }),
        ("withLowerCase", 0) => native(JHex { upper: false }),
        ("toHexDigits", 1) => {
            let value = a[0].long();
            let width = match &a[0] {
                V::Int(_) => 8,
                V::Long(_) => 16,
                _ => 2,
            };
            let text = format!("{:0width$x}", (value as u64) & if width == 16 { u64::MAX } else { (1u64 << (width * 4)) - 1 }, width = width);
            s(&if h.upper { text.to_uppercase() } else { text })
        }
        _ => return None,
    }))
}

// ---- Deep array helpers ----

fn deep_text(value: &V) -> String {
    match value {
        V::Arr(items) => format!("[{}]", lock(items).iter().map(deep_text).collect::<Vec<_>>().join(", ")),
        V::Null => "null".to_owned(),
        other => other.text(),
    }
}

fn deep_equal(x: &V, y: &V) -> bool {
    match (x, y) {
        (V::Arr(p), V::Arr(q)) => {
            let (p, q) = (lock(p).clone(), lock(q).clone());
            p.len() == q.len() && p.iter().zip(&q).all(|(a, b)| deep_equal(a, b))
        }
        _ => crate::same(x, y) || (!x.is_null() && call(x, "equals", &[y.clone()]).map(|v| v.truth()).unwrap_or(false)),
    }
}

fn deep_hash(value: &V) -> i32 {
    match value {
        V::Arr(items) => lock(items).iter().fold(1i32, |h, v| h.wrapping_mul(31).wrapping_add(deep_hash(v))),
        V::Null => 0,
        other => call(other, "hashCode", &[]).map(|v| v.int()).unwrap_or(0),
    }
}

// ---- What stopped plugins in the 199-plugin run (s200r, missing_report.py "stopped a plugin as it ran") ----

/// `AtomicReferenceArray` (Kotlin coroutines' LockFreeTaskQueue, Jackson's LRU map).
pub struct JRefArray(Mutex<Vec<V>>);

fn ref_array_method(r: &JRefArray, name: &str, a: &[V]) -> Option<R> {
    let mut items = lock(&r.0);
    let len = items.len();
    let at = |i: usize| -> Result<usize, V> {
        let n = a.get(i).map_or(0, V::int);
        if n < 0 || n as usize >= len { Err(exc("IndexOutOfBoundsException", format!("index {n}"))) } else { Ok(n as usize) }
    };
    Some((|| -> R {
        Ok(match (name, a.len()) {
            ("length", 0) => V::Int(items.len() as i32),
            ("get" | "getPlain" | "getOpaque" | "getAcquire", 1) => items[at(0)?].clone(),
            ("set" | "lazySet" | "setPlain" | "setOpaque" | "setRelease", 2) => {
                let i = at(0)?;
                items[i] = a[1].clone();
                V::Null
            }
            ("getAndSet", 2) => {
                let i = at(0)?;
                std::mem::replace(&mut items[i], a[1].clone())
            }
            ("compareAndSet" | "weakCompareAndSet" | "weakCompareAndSetPlain" | "weakCompareAndSetVolatile" | "weakCompareAndSetAcquire" | "weakCompareAndSetRelease" | "compareAndExchange", 3) => {
                let i = at(0)?;
                let was = items[i].clone();
                let same = crate::same(&was, &a[1]);
                if same {
                    items[i] = a[2].clone();
                }
                if name == "compareAndExchange" { was } else { V::Bool(same) }
            }
            ("getAndUpdate" | "updateAndGet", 2) => {
                let i = at(0)?;
                let was = items[i].clone();
                drop(items);
                let now = call(&a[1], "apply", &[was.clone()])?;
                lock(&r.0)[i] = now.clone();
                return Ok(if name == "getAndUpdate" { was } else { now });
            }
            ("getAndAccumulate" | "accumulateAndGet", 3) => {
                let i = at(0)?;
                let was = items[i].clone();
                drop(items);
                let now = call(&a[2], "apply", &[was.clone(), a[1].clone()])?;
                lock(&r.0)[i] = now.clone();
                return Ok(if name == "getAndAccumulate" { was } else { now });
            }
            ("toString", 0) => s(&format!("[{}]", items.iter().map(|v| if v.is_null() { "null".to_owned() } else { v.text() }).collect::<Vec<_>>().join(", "))),
            _ => return Err(V::Null),
        })
    })())
    .filter(|r| !matches!(r, Err(V::Null)))
}

/// `MethodHandles.lookup().findConstructor/findStatic/findVirtual/findGetter/unreflect(...)`: a handle calls what it
/// names by name, as jrt calls everything (Caffeine's LocalCacheFactory, ByteBuddy-free reflection helpers).
#[derive(Clone)]
pub enum JHandle {
    Ctor(String),
    Static(String, String),
    Virtual(String),
    Getter(String),
    Setter(String),
    Reflected(V),
    Bound(Box<JHandle>, V),
    /// A handle with the type it was found by or given (`asType`): what `type()` says of it.
    Typed(Box<JHandle>, JMethodType),
    /// `MethodHandles.invoker(type)`: calls the handle it's given first with the rest.
    Invoker(JMethodType),
    /// `filterReturnValue(target, filter)`.
    Filter(Box<JHandle>, Box<JHandle>),
    /// `catchException(target, exceptionClass, handler)`: the handler gets the exception and the leading arguments.
    Catch(Box<JHandle>, String, Box<JHandle>),
    /// `dropArguments(target, position, types...)`: those arguments are left out.
    Drop(Box<JHandle>, usize, Vec<V>),
    /// `insertArguments(target, position, values...)`: those values go in there.
    Insert(Box<JHandle>, usize, Vec<V>),
    /// `constant(type, value)`, `identity(type)`.
    Constant(V),
    Identity,
    /// What `LambdaMetafactory` makes: called with the captured values, it gives a lambda that calls the
    /// implementation with them and its own arguments.
    Lambda(Box<JHandle>),
    /// `guardWithTest(test, target, fallback)`: the test with the leading arguments it takes picks which runs.
    Guard(Box<JHandle>, Box<JHandle>, Box<JHandle>),
    /// `permuteArguments(target, newType, reorder...)`: the target's argument i is the new argument reorder[i].
    Permute(Box<JHandle>, JMethodType, Vec<usize>),
    /// `collectArguments(target, pos, filter)`: the filter takes the arguments from pos on that it wants and its
    /// answer (none for void) goes to the target at pos.
    Collect(Box<JHandle>, usize, Box<JHandle>),
    /// `filterArguments(target, pos, filters...)`: argument pos+k through filter k (a null filter leaves it).
    FilterArgs(Box<JHandle>, usize, Vec<Option<JHandle>>),
    /// `throwException(returnType, exceptionType)`: throws its argument.
    Throw,
}

/// `MethodHandles.Lookup`.
pub struct JLookup;

/// `MethodType`: the return type and the parameter types, as classes.
#[derive(Clone)]
pub struct JMethodType {
    ret: V,
    params: Vec<V>,
}

impl JMethodType {
    fn unknown() -> Self {
        JMethodType { ret: V::Class("Object"), params: Vec::new() }
    }
}

/// `MethodType.methodType(ret, params...)` in each of its forms: `(ret)`, `(ret, p)`, `(ret, p...)`,
/// `(ret, Class[])`, `(ret, List<Class>)`, `(ret, MethodType)` (its parameters).
fn method_type_of(a: &[V]) -> JMethodType {
    let ret = a.first().cloned().unwrap_or(V::Class("Object"));
    let params = match &a[1.min(a.len())..] {
        [V::Arr(_) | V::List(_)] => a[1].items().unwrap_or_default(),
        [one] if one.native::<JMethodType>().is_some() => one.native::<JMethodType>().map(|t| t.params.clone()).unwrap_or_default(),
        rest => rest.to_vec(),
    };
    JMethodType { ret, params }
}

/// What a handle takes and gives, as far as it is known.
fn handle_type(h: &JHandle) -> JMethodType {
    match h {
        JHandle::Typed(_, t) => t.clone(),
        JHandle::Bound(inner, _) => {
            let mut t = handle_type(inner);
            if !t.params.is_empty() {
                t.params.remove(0);
            }
            t
        }
        JHandle::Invoker(t) => {
            let mut params = vec![V::Class("MethodHandle")];
            params.extend(t.params.iter().cloned());
            JMethodType { ret: t.ret.clone(), params }
        }
        JHandle::Filter(target, filter) => JMethodType { ret: handle_type(filter).ret, params: handle_type(target).params },
        JHandle::Catch(target, _, _) => handle_type(target),
        JHandle::Guard(_, target, _) => handle_type(target),
        JHandle::Permute(_, t, _) => t.clone(),
        JHandle::Collect(target, at, filter) => {
            let mut t = handle_type(target);
            let f = handle_type(filter);
            let at = (*at).min(t.params.len());
            let returns_void = matches!(&f.ret, V::Class(c) if *c == "void");
            if !returns_void && at < t.params.len() {
                t.params.remove(at);
            }
            for (i, p) in f.params.iter().enumerate() {
                t.params.insert((at + i).min(t.params.len()), p.clone());
            }
            t
        }
        JHandle::FilterArgs(target, at, filters) => {
            let mut t = handle_type(target);
            for (k, filter) in filters.iter().enumerate() {
                if let (Some(filter), Some(slot)) = (filter, t.params.get_mut(at + k)) {
                    if let Some(p) = handle_type(filter).params.first() {
                        *slot = p.clone();
                    }
                }
            }
            t
        }
        JHandle::Drop(target, at, types) => {
            let mut t = handle_type(target);
            let at = (*at).min(t.params.len());
            for (i, kind) in types.iter().enumerate() {
                t.params.insert(at + i, kind.clone());
            }
            t
        }
        JHandle::Insert(target, at, values) => {
            let mut t = handle_type(target);
            for _ in 0..values.len() {
                if *at < t.params.len() {
                    t.params.remove(*at);
                }
            }
            t
        }
        _ => JMethodType::unknown(),
    }
}

fn as_handle(value: &V) -> Option<JHandle> {
    value.native::<JHandle>().cloned()
}

fn class_name(value: &V) -> String {
    match value {
        V::Class(name) => (*name).to_owned(),
        other => other.text(),
    }
}

fn handle_invoke(h: &JHandle, args: &[V]) -> R {
    match h {
        JHandle::Ctor(class) => match crate::plugin_class(class) {
            Some(own) => crate::new_obj(own, args, &[]),
            None => crate::new_other(class.rsplit(['.', '$']).next().unwrap_or(class), args),
        },
        JHandle::Static(class, name) => call_static(class.rsplit(['.', '$']).next().unwrap_or(class), name, args),
        JHandle::Virtual(name) => match args.split_first() {
            Some((this, rest)) => call(this, name, rest),
            None => Err(exc("WrongMethodTypeException", "no receiver")),
        },
        JHandle::Getter(field) => match args.first() {
            Some(this) => this.get(field),
            None => Err(exc("WrongMethodTypeException", "no receiver")),
        },
        JHandle::Setter(field) => match args {
            [this, value, ..] => this.set(Box::leak(field.clone().into_boxed_str()), value.clone()).map(|_| V::Null),
            _ => Err(exc("WrongMethodTypeException", "a setter takes the object and the value")),
        },
        JHandle::Reflected(method) => {
            let (target, rest) = args.split_first().map_or((V::Null, &[][..]), |(t, r)| (t.clone(), r));
            let mut all = vec![target];
            all.extend_from_slice(rest);
            call(method, "invoke", &all)
        }
        JHandle::Bound(inner, first) => {
            let mut all = vec![first.clone()];
            all.extend_from_slice(args);
            handle_invoke(inner, &all)
        }
        JHandle::Typed(inner, _) => handle_invoke(inner, args),
        JHandle::Invoker(_) => match args.split_first() {
            Some((target, rest)) => match as_handle(target) {
                Some(target) => handle_invoke(&target, rest),
                None => call(target, "invoke", rest),
            },
            None => Err(exc("WrongMethodTypeException", "an invoker takes the handle first")),
        },
        JHandle::Filter(target, filter) => {
            let result = handle_invoke(target, args)?;
            handle_invoke(filter, &[result])
        }
        JHandle::Catch(target, class, handler) => match handle_invoke(target, args) {
            Err(thrown) if matches!(class.as_str(), "Throwable" | "java.lang.Throwable") || crate::instance_of(&thrown, class.rsplit(['.', '$']).next().unwrap_or(class)) => {
                // The handler's leading parameters after the exception are the target's first arguments.
                let wanted = handle_type(handler).params.len().saturating_sub(1).min(args.len());
                let mut all = vec![thrown];
                all.extend_from_slice(&args[..wanted]);
                handle_invoke(handler, &all)
            }
            other => other,
        },
        JHandle::Drop(target, at, types) => {
            let mut kept = args.to_vec();
            let at = (*at).min(kept.len());
            let end = (at + types.len()).min(kept.len());
            kept.drain(at..end);
            handle_invoke(target, &kept)
        }
        JHandle::Insert(target, at, values) => {
            let mut all = args.to_vec();
            let at = (*at).min(all.len());
            for (i, value) in values.iter().enumerate() {
                all.insert(at + i, value.clone());
            }
            handle_invoke(target, &all)
        }
        JHandle::Guard(test, target, fallback) => {
            let wanted = handle_type(test).params.len();
            let leading = if wanted == 0 && !matches!(**test, JHandle::Typed(..)) { args.len() } else { wanted.min(args.len()) };
            if handle_invoke(test, &args[..leading])?.truth() { handle_invoke(target, args) } else { handle_invoke(fallback, args) }
        }
        JHandle::Permute(target, _, reorder) => {
            let moved: Vec<V> = reorder.iter().map(|i| args.get(*i).cloned().unwrap_or(V::Null)).collect();
            handle_invoke(target, &moved)
        }
        JHandle::Collect(target, at, filter) => {
            let f = handle_type(filter);
            let at = (*at).min(args.len());
            let count = f.params.len().min(args.len() - at);
            let answer = handle_invoke(filter, &args[at..at + count])?;
            let mut all = args[..at].to_vec();
            if !matches!(&f.ret, V::Class(c) if *c == "void") {
                all.push(answer);
            }
            all.extend_from_slice(&args[at + count..]);
            handle_invoke(target, &all)
        }
        JHandle::FilterArgs(target, at, filters) => {
            let mut all = args.to_vec();
            for (k, filter) in filters.iter().enumerate() {
                if let (Some(filter), Some(value)) = (filter, all.get(at + k).cloned()) {
                    all[at + k] = handle_invoke(filter, &[value])?;
                }
            }
            handle_invoke(target, &all)
        }
        JHandle::Throw => Err(args.first().cloned().unwrap_or_else(|| exc("NullPointerException", "throwException of null"))),
        JHandle::Constant(value) => Ok(value.clone()),
        JHandle::Identity => Ok(args.first().cloned().unwrap_or(V::Null)),
        JHandle::Lambda(implementation) => {
            let (implementation, captured) = ((**implementation).clone(), args.to_vec());
            Ok(crate::func(move |more| {
                let mut all = captured.clone();
                all.extend_from_slice(more);
                handle_invoke(&implementation, &all)
            }))
        }
    }
}

/// The type a lookup was given (`findStatic(c, n, type)`), or for a constructor its class as the return type, or
/// for a virtual method its class as the first parameter: what `handle.type()` answers.
fn typed(handle: JHandle, t: Option<&V>, ret: Option<&V>, receiver: Option<&V>) -> JHandle {
    let Some(mut t) = t.and_then(|t| t.native::<JMethodType>()).cloned() else { return handle };
    if let Some(ret) = ret {
        t.ret = ret.clone();
    }
    if let Some(receiver) = receiver {
        t.params.insert(0, receiver.clone());
    }
    JHandle::Typed(Box::new(handle), t)
}

fn lookup_method(name: &str, a: &[V]) -> Option<R> {
    let text = |i: usize| a.get(i).map(V::text).unwrap_or_default();
    Some(Ok(native(match name {
        "findConstructor" => typed(JHandle::Ctor(class_name(&a[0])), a.get(1), a.first(), None),
        "findStatic" => typed(JHandle::Static(class_name(&a[0]), text(1)), a.get(2), None, None),
        "findVirtual" | "findSpecial" => typed(JHandle::Virtual(text(1)), a.get(2), None, a.first()),
        "findGetter" | "findVarHandle" => JHandle::Getter(text(1)),
        "findSetter" => JHandle::Setter(text(1)),
        "findStaticGetter" => JHandle::Static(class_name(&a[0]), format!("get{}", text(1))),
        "unreflect" | "unreflectSpecial" => JHandle::Reflected(a[0].clone()),
        "unreflectConstructor" => JHandle::Ctor(call(&a[0], "getDeclaringClass", &[]).map(|c| class_name(&c)).unwrap_or_default()),
        "unreflectGetter" => JHandle::Getter(call(&a[0], "getName", &[]).map(|n| n.text()).unwrap_or_default()),
        "in" | "dropLookupMode" => return Some(Ok(native(JLookup))),
        "lookupClass" => return Some(Ok(V::Class("Object"))),
        _ => return None,
    })))
}

fn handle_method(h: &JHandle, this: &V, name: &str, a: &[V]) -> Option<R> {
    Some(match name {
        "invoke" | "invokeExact" => handle_invoke(h, a),
        "invokeWithArguments" => {
            let args = match a {
                [V::Arr(items)] | [V::List(items)] => lock(items).clone(),
                rest => rest.to_vec(),
            };
            handle_invoke(h, &args)
        }
        "bindTo" => Ok(native(JHandle::Bound(Box::new(h.clone()), a.first().cloned().unwrap_or(V::Null)))),
        "asType" => match a.first().and_then(|t| t.native::<JMethodType>()) {
            Some(t) => Ok(native(JHandle::Typed(Box::new(h.clone()), t.clone()))),
            None => Ok(this.clone()),
        },
        "asFixedArity" | "withVarargs" | "asVarargsCollector" | "asSpreader" | "asCollector" => Ok(this.clone()),
        "isVarargsCollector" => Ok(V::Bool(false)),
        "type" => Ok(native(handle_type(h))),
        _ => return None,
    })
}

/// `FileSystems.getDefault()` and its `WatchService`: paths as `Paths.get` makes them; a watch reports no change
/// (a watcher thread's `take()` waits until it's closed), so a plugin that watches its folder runs and just never
/// reloads by itself.
pub struct JFileSystem;
pub struct JWatch(std::sync::atomic::AtomicBool);
pub struct JWatchKey(V);

fn watch_method(w: &JWatch, name: &str, a: &[V]) -> Option<R> {
    use std::sync::atomic::Ordering;
    Some(match name {
        "poll" if a.is_empty() => Ok(V::Null),
        "poll" => {
            let wait = a.first().map_or(0, V::long).clamp(0, 60_000);
            let unit = a.get(1).map(V::text).unwrap_or_default();
            let ms = match unit.as_str() {
                "SECONDS" => wait.saturating_mul(1000),
                "MINUTES" => wait.saturating_mul(60_000),
                "MICROSECONDS" => wait / 1000,
                "NANOSECONDS" => wait / 1_000_000,
                _ => wait,
            }
            .min(60_000);
            std::thread::sleep(std::time::Duration::from_millis(ms as u64));
            Ok(V::Null)
        }
        "take" => loop {
            if w.0.load(Ordering::Acquire) {
                break Err(exc("ClosedWatchServiceException", "closed"));
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        },
        "close" => {
            w.0.store(true, Ordering::Release);
            Ok(V::Null)
        }
        _ => return None,
    })
}

fn locale_from_parts(language: &str, region: &str, variant: &str) -> V {
    let mut name = language.to_ascii_lowercase();
    if !region.is_empty() || !variant.is_empty() {
        name.push('_');
        name.push_str(&region.to_ascii_uppercase());
    }
    if !variant.is_empty() {
        name.push('_');
        name.push_str(variant);
    }
    s(&name)
}

/// `new Locale.Builder().setLanguage("en").setRegion("US").build()`.
pub struct JLocaleBuilder(Mutex<(String, String, String)>);

fn locale_builder_method(b: &JLocaleBuilder, this: &V, name: &str, a: &[V]) -> Option<R> {
    let text = a.first().map(V::text).unwrap_or_default();
    let mut parts = lock(&b.0);
    Some(Ok(match name {
        "setLanguage" => {
            parts.0 = text;
            this.clone()
        }
        "setRegion" => {
            parts.1 = text;
            this.clone()
        }
        "setVariant" => {
            parts.2 = text;
            this.clone()
        }
        "setLanguageTag" => {
            let mut split = text.split(['-', '_']);
            parts.0 = split.next().unwrap_or("").to_owned();
            parts.1 = split.next().unwrap_or("").to_owned();
            this.clone()
        }
        "setLocale" => {
            let mut split = text.split('_');
            parts.0 = split.next().unwrap_or("").to_owned();
            parts.1 = split.next().unwrap_or("").to_owned();
            parts.2 = split.next().unwrap_or("").to_owned();
            this.clone()
        }
        "setScript" | "addUnicodeLocaleAttribute" | "setUnicodeLocaleKeyword" | "setExtension" => this.clone(),
        "clear" => {
            *parts = Default::default();
            this.clone()
        }
        "build" => locale_from_parts(&parts.0, &parts.1, &parts.2),
        _ => return None,
    }))
}

/// `Class.getAnnotatedSuperclass()` / `getAnnotatedInterfaces()`: the generic type, without annotations.
pub struct JAnnotatedType(V);

fn annotated_method(t: &JAnnotatedType, name: &str) -> Option<R> {
    Some(Ok(match name {
        "getType" => t.0.clone(),
        "getAnnotations" | "getDeclaredAnnotations" => arr(Vec::new()),
        "getAnnotation" | "getDeclaredAnnotation" => V::Null,
        "isAnnotationPresent" => V::Bool(false),
        "toString" => s(&t.0.text()),
        _ => return None,
    }))
}

fn generic_interfaces(class: &str) -> V {
    let names: Vec<V> = crate::plugin_class(class).map_or_else(Vec::new, |own| own.interfaces.iter().map(|i| V::Class(i().name)).collect());
    arr(names)
}

/// A plugin class that implements `Map` but not its default methods (JDA's `CaseInsensitiveMap`): the defaults
/// Java's `Map` interface gives, over the class's own `get`, `put`, `containsKey` and `remove`.
fn map_default(this: &V, name: &str, a: &[V]) -> Option<R> {
    if !crate::instance_of(this, "Map") {
        return None;
    }
    let get = |key: &V| call(this, "get", &[key.clone()]);
    Some((|| -> R {
        Ok(match (name, a.len()) {
            ("getOrDefault", 2) => {
                let found = get(&a[0])?;
                if found.is_null() && !call(this, "containsKey", &[a[0].clone()])?.truth() { a[1].clone() } else { found }
            }
            ("putIfAbsent", 2) => {
                let found = get(&a[0])?;
                if found.is_null() {
                    call(this, "put", &[a[0].clone(), a[1].clone()])?;
                }
                found
            }
            ("computeIfAbsent", 2) => {
                let found = get(&a[0])?;
                if !found.is_null() {
                    return Ok(found);
                }
                let made = call(&a[1], "apply", &[a[0].clone()])?;
                if !made.is_null() {
                    call(this, "put", &[a[0].clone(), made.clone()])?;
                }
                made
            }
            ("computeIfPresent", 2) => {
                let found = get(&a[0])?;
                if found.is_null() {
                    return Ok(V::Null);
                }
                let made = call(&a[1], "apply", &[a[0].clone(), found])?;
                if made.is_null() { call(this, "remove", &[a[0].clone()])? } else { call(this, "put", &[a[0].clone(), made.clone()])? };
                made
            }
            ("compute", 2) => {
                let found = get(&a[0])?;
                let made = call(&a[1], "apply", &[a[0].clone(), found])?;
                if made.is_null() { call(this, "remove", &[a[0].clone()])? } else { call(this, "put", &[a[0].clone(), made.clone()])? };
                made
            }
            ("merge", 3) => {
                let found = get(&a[0])?;
                let made = if found.is_null() { a[1].clone() } else { call(&a[2], "apply", &[found, a[1].clone()])? };
                if made.is_null() { call(this, "remove", &[a[0].clone()])? } else { call(this, "put", &[a[0].clone(), made.clone()])? };
                made
            }
            ("forEach", 1) => {
                let entries = call(this, "entrySet", &[])?;
                for entry in entries.items()? {
                    call(&a[0], "accept", &[call(&entry, "getKey", &[])?, call(&entry, "getValue", &[])?])?;
                }
                V::Null
            }
            ("replace", 2) => {
                if call(this, "containsKey", &[a[0].clone()])?.truth() { call(this, "put", &[a[0].clone(), a[1].clone()])? } else { V::Null }
            }
            _ => return Err(V::Null),
        })
    })())
    .filter(|r| !matches!(r, Err(V::Null)))
}

fn ip_value(ip: std::net::IpAddr) -> V {
    native(Mutex::new(ip))
}

/// This PC's address on its network, as `InetAddress.getLocalHost()` gives: the local end of a UDP socket pointed at
/// a public address (nothing is sent), else the loopback.
fn local_ip() -> std::net::IpAddr {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| socket.connect("8.8.8.8:80").map(|_| socket))
        .and_then(|socket| socket.local_addr())
        .map(|address| address.ip())
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
}

fn resolve(host: &str) -> Option<Vec<std::net::IpAddr>> {
    if host.is_empty() || host == "localhost" {
        return Some(vec![std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)]);
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Some(vec![ip]);
    }
    use std::net::ToSocketAddrs;
    let ips: Vec<std::net::IpAddr> = (host, 0).to_socket_addrs().ok()?.map(|a| a.ip()).collect();
    (!ips.is_empty()).then_some(ips)
}

/// `IdentityHashMap` (and a set made from one): keys by identity, as Java's `==`. A plain map files a list by its content,
/// so two equal lists were one key: snakeyaml-engine's representer then wrote the second as an alias (`*id003`) of the
/// first (Tebex's config.yml), where Java writes both. Strings and numbers have no identity worth keeping: their value.
pub struct JIdentityMap(Mutex<Vec<(String, V, V)>>);
pub struct JIdentitySet(Mutex<Vec<(String, V)>>);

fn identity(value: &V) -> String {
    fn at<T: ?Sized>(p: *const T) -> String {
        format!("i{:p}", p as *const ())
    }
    match value {
        V::Obj(o) => at(std::sync::Arc::as_ptr(o)),
        V::Arr(x) | V::List(x) | V::Stream(x) => at(std::sync::Arc::as_ptr(x)),
        V::Map(x) | V::Set(x) => at(std::sync::Arc::as_ptr(x)),
        V::Sb(x) => at(std::sync::Arc::as_ptr(x)),
        V::Iter(x) => at(std::sync::Arc::as_ptr(x)),
        V::Entry(x) => at(std::sync::Arc::as_ptr(x)),
        V::Opt(x) => at(std::sync::Arc::as_ptr(x)),
        V::Exc(x) => at(std::sync::Arc::as_ptr(x)),
        V::Native(x) => at(std::sync::Arc::as_ptr(x)),
        other => other.key(),
    }
}

fn identity_map_method(m: &JIdentityMap, this: &V, name: &str, a: &[V]) -> Option<R> {
    let mut entries = lock(&m.0);
    let find = |entries: &Vec<(String, V, V)>, key: &V| {
        let id = identity(key);
        entries.iter().position(|(k, _, _)| *k == id)
    };
    Some(Ok(match (name, a.len()) {
        ("put", 2) => match find(&entries, &a[0]) {
            Some(i) => std::mem::replace(&mut entries[i].2, a[1].clone()),
            None => {
                entries.push((identity(&a[0]), a[0].clone(), a[1].clone()));
                V::Null
            }
        },
        ("putIfAbsent", 2) => match find(&entries, &a[0]) {
            Some(i) if !entries[i].2.is_null() => entries[i].2.clone(),
            Some(i) => {
                entries[i].2 = a[1].clone();
                V::Null
            }
            None => {
                entries.push((identity(&a[0]), a[0].clone(), a[1].clone()));
                V::Null
            }
        },
        ("get", 1) => find(&entries, &a[0]).map_or(V::Null, |i| entries[i].2.clone()),
        ("getOrDefault", 2) => find(&entries, &a[0]).map_or_else(|| a[1].clone(), |i| entries[i].2.clone()),
        ("containsKey", 1) => V::Bool(find(&entries, &a[0]).is_some()),
        ("containsValue", 1) => {
            let id = identity(&a[0]);
            V::Bool(entries.iter().any(|(_, _, v)| identity(v) == id))
        }
        ("remove", 1) => match find(&entries, &a[0]) {
            Some(i) => entries.remove(i).2,
            None => V::Null,
        },
        ("size", 0) => V::Int(entries.len() as i32),
        ("isEmpty", 0) => V::Bool(entries.is_empty()),
        ("clear", 0) => {
            entries.clear();
            V::Null
        }
        ("keySet", 0) => list(entries.iter().map(|(_, k, _)| k.clone()).collect()),
        ("values", 0) => list(entries.iter().map(|(_, _, v)| v.clone()).collect()),
        ("entrySet", 0) => list(entries.iter().map(|(_, k, v)| V::Entry(std::sync::Arc::new((k.clone(), v.clone())))).collect()),
        ("computeIfAbsent", 2) => {
            if let Some(i) = find(&entries, &a[0]).filter(|&i| !entries[i].2.is_null()) {
                return Some(Ok(entries[i].2.clone()));
            }
            drop(entries);
            let made = match call(&a[1], "apply", &[a[0].clone()]) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            if !made.is_null() {
                let _ = identity_map_method(m, this, "put", &[a[0].clone(), made.clone()]);
            }
            made
        }
        ("forEach", 1) => {
            let pairs: Vec<(V, V)> = entries.iter().map(|(_, k, v)| (k.clone(), v.clone())).collect();
            drop(entries);
            for (k, v) in pairs {
                if let Err(e) = call(&a[0], "accept", &[k, v]) {
                    return Some(Err(e));
                }
            }
            V::Null
        }
        ("putAll", 1) => {
            drop(entries);
            if let Ok(pairs) = call(&a[0], "entrySet", &[]).and_then(|e| e.items()) {
                for pair in pairs {
                    let (k, v) = (call(&pair, "getKey", &[]).unwrap_or(V::Null), call(&pair, "getValue", &[]).unwrap_or(V::Null));
                    let _ = identity_map_method(m, this, "put", &[k, v]);
                }
            }
            V::Null
        }
        ("toString", 0) => s(&format!("{{{}}}", entries.iter().map(|(_, k, v)| format!("{}={}", k.text(), v.text())).collect::<Vec<_>>().join(", "))),
        _ => return None,
    }))
}

fn identity_set_method(set: &JIdentitySet, name: &str, a: &[V]) -> Option<R> {
    let mut items = lock(&set.0);
    Some(Ok(match (name, a.len()) {
        ("add", 1) => {
            let id = identity(&a[0]);
            if items.iter().any(|(k, _)| *k == id) {
                V::Bool(false)
            } else {
                items.push((id, a[0].clone()));
                V::Bool(true)
            }
        }
        ("contains", 1) => {
            let id = identity(&a[0]);
            V::Bool(items.iter().any(|(k, _)| *k == id))
        }
        ("remove", 1) => {
            let id = identity(&a[0]);
            let before = items.len();
            items.retain(|(k, _)| *k != id);
            V::Bool(items.len() != before)
        }
        ("size", 0) => V::Int(items.len() as i32),
        ("isEmpty", 0) => V::Bool(items.is_empty()),
        ("clear", 0) => {
            items.clear();
            V::Null
        }
        ("iterator", 0) => return Some(call(&list(items.iter().map(|(_, v)| v.clone()).collect()), "iterator", &[])),
        _ => return None,
    }))
}

/// `CallSite` (what `LambdaMetafactory` gives): its target.
pub struct JCallSite(JHandle);

fn method_type_method(t: &JMethodType, this: &V, name: &str, a: &[V]) -> Option<R> {
    let changed = |f: &dyn Fn(&mut JMethodType)| {
        let mut out = t.clone();
        f(&mut out);
        native(out)
    };
    Some(Ok(match (name, a.len()) {
        ("parameterCount", 0) => V::Int(t.params.len() as i32),
        ("returnType", 0) => t.ret.clone(),
        ("parameterType", 1) => match t.params.get(a[0].int().max(0) as usize) {
            Some(p) => p.clone(),
            None => return Some(Err(exc("IndexOutOfBoundsException", format!("parameter {}", a[0].int())))),
        },
        ("parameterList", 0) => list(t.params.clone()),
        ("parameterArray", 0) => arr(t.params.clone()),
        ("lastParameterType", 0) => t.params.last().cloned().unwrap_or(V::Class("void")),
        ("changeReturnType", 1) => changed(&|m| m.ret = a[0].clone()),
        ("changeParameterType", 2) => changed(&|m| {
            let at = a[0].int().max(0) as usize;
            if at < m.params.len() {
                m.params[at] = a[1].clone();
            }
        }),
        ("appendParameterTypes", _) => changed(&|m| m.params.extend(a.iter().flat_map(|x| if matches!(x, V::Arr(_) | V::List(_)) { x.items().unwrap_or_default() } else { vec![x.clone()] }))),
        ("insertParameterTypes", _) if !a.is_empty() => changed(&|m| {
            let at = (a[0].int().max(0) as usize).min(m.params.len());
            let more: Vec<V> = a[1..].iter().flat_map(|x| if matches!(x, V::Arr(_) | V::List(_)) { x.items().unwrap_or_default() } else { vec![x.clone()] }).collect();
            for (i, p) in more.into_iter().enumerate() {
                m.params.insert(at + i, p);
            }
        }),
        ("dropParameterTypes", 2) => changed(&|m| {
            let (from, to) = (a[0].int().max(0) as usize, a[1].int().max(0) as usize);
            let to = to.min(m.params.len());
            if from < to {
                m.params.drain(from..to);
            }
        }),
        // `wrap()`: each primitive as its box (`double` -> `Double`), `unwrap()` back: WorldEdit's expression
        // functions check `type().wrap().returnType()` is a Number.
        ("wrap" | "unwrap", 0) => {
            let box_of = |v: &V, wrap: bool| -> V {
                let V::Class(c) = v else { return v.clone() };
                const PAIRS: [(&str, &str); 9] = [("double", "Double"), ("int", "Integer"), ("long", "Long"), ("float", "Float"), ("boolean", "Boolean"), ("char", "Character"), ("short", "Short"), ("byte", "Byte"), ("void", "Void")];
                PAIRS.iter().find(|(p, b)| if wrap { p == c } else { b == c }).map_or_else(|| v.clone(), |(p, b)| V::Class(if wrap { b } else { p }))
            };
            let wrap = name == "wrap";
            changed(&|m| {
                m.ret = box_of(&m.ret, wrap);
                for p in &mut m.params {
                    *p = box_of(p, wrap);
                }
            })
        }
        ("erase" | "generic", 0) => this.clone(),
        ("hasPrimitives", 0) => V::Bool(std::iter::once(&t.ret).chain(&t.params).any(|v| matches!(v, V::Class(c) if matches!(*c, "double" | "int" | "long" | "float" | "boolean" | "char" | "short" | "byte" | "void")))),
        ("equals", 1) => V::Bool(a[0].native::<JMethodType>().is_some_and(|o| o.ret.text() == t.ret.text() && o.params.len() == t.params.len() && o.params.iter().zip(&t.params).all(|(x, y)| x.text() == y.text()))),
        ("hashCode", 0) => V::Int(t.params.len() as i32),
        ("toString", 0) => s(&format!("({}){}", t.params.iter().map(|p| match p { V::Class(c) => (*c).to_owned(), o => o.text() }).collect::<Vec<_>>().join(","), match &t.ret { V::Class(c) => (*c).to_owned(), o => o.text() })),
        _ => return None,
    }))
}

/// `ThreadGroup`: its name and whether it's a daemon group; the threads keep no tie to it here.
pub struct JThreadGroup(String, Mutex<bool>);

fn thread_group_method(g: &JThreadGroup, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("getName" | "toString", 0) => s(&g.0),
        ("getParent", 0) => V::Null,
        ("activeCount" | "activeGroupCount" | "enumerate", _) => V::Int(0),
        ("isDaemon", 0) => V::Bool(*lock(&g.1)),
        ("setDaemon", 1) => {
            *lock(&g.1) = a[0].truth();
            V::Null
        }
        ("getMaxPriority", 0) => V::Int(10),
        ("setMaxPriority" | "interrupt" | "destroy" | "checkAccess", _) => V::Null,
        ("isDestroyed", 0) => V::Bool(false),
        ("parentOf", 1) => V::Bool(crate::same(&a[0], &V::Null)),
        // An uncaught exception in a thread of the group: logged, as Java prints it.
        ("uncaughtException", 2) => {
            eprintln!("[plugin] Exception in thread {}: {}", a[0].text(), crate::throwable_text(&a[1]));
            V::Null
        }
        _ => return None,
    }))
}

// ---- A map ordered by a comparator (Guava's ImmutableSortedMap.orderedBy) ----

/// Keys kept in the comparator's order and found with it, as Java's sorted maps do: under
/// `String.CASE_INSENSITIVE_ORDER`, "NORTH" finds "north" (WorldEdit's argument converters).
pub struct JComparatorMap {
    cmp: Option<V>,
    entries: Mutex<Vec<(V, V)>>,
}

/// `ImmutableSortedMap.orderedBy(c)` / `naturalOrder()` / `reverseOrder()`: its builder.
pub struct JSortedBuilder {
    cmp: Option<V>,
    reverse: bool,
    items: Mutex<Vec<(V, V)>>,
}

fn compare_with(cmp: &Option<V>, a: &V, b: &V) -> Result<std::cmp::Ordering, V> {
    match cmp {
        Some(c) => call(c, "compare", &[a.clone(), b.clone()]).map(|r| r.int().cmp(&0)),
        None => Ok(crate::views::order(a, b)),
    }
}

impl JComparatorMap {
    fn find(&self, entries: &[(V, V)], key: &V) -> Result<Result<usize, usize>, V> {
        let (mut low, mut high) = (0, entries.len());
        while low < high {
            let mid = (low + high) / 2;
            match compare_with(&self.cmp, &entries[mid].0, key)? {
                std::cmp::Ordering::Less => low = mid + 1,
                std::cmp::Ordering::Greater => high = mid,
                std::cmp::Ordering::Equal => return Ok(Ok(mid)),
            }
        }
        Ok(Err(low))
    }
    fn put(&self, key: V, value: V) -> R {
        let mut entries = lock(&self.entries).clone();
        let old = match self.find(&entries, &key)? {
            Ok(i) => std::mem::replace(&mut entries[i].1, value),
            Err(i) => {
                entries.insert(i, (key, value));
                V::Null
            }
        };
        *lock(&self.entries) = entries;
        Ok(old)
    }
}

fn comparator_map(cmp: Option<V>, rows: Vec<(V, V)>) -> R {
    let map = JComparatorMap { cmp, entries: Mutex::new(Vec::new()) };
    for (k, v) in rows {
        map.put(k, v)?;
    }
    Ok(native(map))
}

fn entry_of(k: &V, v: &V) -> V {
    V::Entry(std::sync::Arc::new((k.clone(), v.clone())))
}

fn comparator_map_method(m: &JComparatorMap, name: &str, a: &[V]) -> Option<R> {
    let entries = lock(&m.entries).clone();
    let cmp = |x: &V, y: &V| compare_with(&m.cmp, x, y);
    let range = |keep: &dyn Fn(&V) -> Result<bool, V>| -> R {
        let mut rows = Vec::new();
        for (k, v) in &entries {
            if keep(k)? {
                rows.push((k.clone(), v.clone()));
            }
        }
        Ok(native(JComparatorMap { cmp: m.cmp.clone(), entries: Mutex::new(rows) }))
    };
    Some(match (name, a.len()) {
        ("get", 1) => m.find(&entries, &a[0]).map(|f| f.map_or(V::Null, |i| entries[i].1.clone())),
        ("getOrDefault", 2) => m.find(&entries, &a[0]).map(|f| f.map_or_else(|_| a[1].clone(), |i| entries[i].1.clone())),
        ("containsKey", 1) => m.find(&entries, &a[0]).map(|f| V::Bool(f.is_ok())),
        ("containsValue", 1) => Ok(V::Bool(entries.iter().any(|(_, v)| crate::same(v, &a[0]) || v.text() == a[0].text()))),
        ("put", 2) => m.put(a[0].clone(), a[1].clone()),
        ("remove", 1) => match m.find(&entries, &a[0]) {
            Ok(Ok(i)) => {
                let mut kept = entries.clone();
                let (_, old) = kept.remove(i);
                *lock(&m.entries) = kept;
                Ok(old)
            }
            Ok(Err(_)) => Ok(V::Null),
            Err(e) => Err(e),
        },
        ("putAll", 1) => {
            let pairs = match call(&a[0], "entrySet", &[]).and_then(|e| e.items()) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            for pair in pairs {
                let (k, v) = (call(&pair, "getKey", &[]).unwrap_or(V::Null), call(&pair, "getValue", &[]).unwrap_or(V::Null));
                if let Err(e) = m.put(k, v) {
                    return Some(Err(e));
                }
            }
            Ok(V::Null)
        }
        ("clear", 0) => {
            lock(&m.entries).clear();
            Ok(V::Null)
        }
        ("size", 0) => Ok(V::Int(entries.len() as i32)),
        ("isEmpty", 0) => Ok(V::Bool(entries.is_empty())),
        ("keySet" | "navigableKeySet", 0) => Ok(list(entries.iter().map(|(k, _)| k.clone()).collect())),
        ("values", 0) => Ok(list(entries.iter().map(|(_, v)| v.clone()).collect())),
        ("entrySet", 0) => Ok(list(entries.iter().map(|(k, v)| entry_of(k, v)).collect())),
        ("forEach", 1) => {
            for (k, v) in entries {
                if let Err(e) = call(&a[0], "accept", &[k, v]) {
                    return Some(Err(e));
                }
            }
            Ok(V::Null)
        }
        ("firstKey", 0) => entries.first().map(|(k, _)| k.clone()).ok_or_else(|| exc("NoSuchElementException", "empty map")),
        ("lastKey", 0) => entries.last().map(|(k, _)| k.clone()).ok_or_else(|| exc("NoSuchElementException", "empty map")),
        ("firstEntry", 0) => Ok(entries.first().map_or(V::Null, |(k, v)| entry_of(k, v))),
        ("lastEntry", 0) => Ok(entries.last().map_or(V::Null, |(k, v)| entry_of(k, v))),
        ("comparator", 0) => Ok(m.cmp.clone().unwrap_or(V::Null)),
        ("headMap", 1 | 2) => {
            let inclusive = a.get(1).is_some_and(V::truth);
            range(&|k| cmp(k, &a[0]).map(|o| o.is_lt() || (inclusive && o.is_eq())))
        }
        ("tailMap", 1 | 2) => {
            let inclusive = a.get(1).is_none_or(V::truth);
            range(&|k| cmp(k, &a[0]).map(|o| o.is_gt() || (inclusive && o.is_eq())))
        }
        ("subMap", 2) => range(&|k| Ok(cmp(k, &a[0])?.is_ge() && cmp(k, &a[1])?.is_lt())),
        ("floorKey" | "lowerKey" | "ceilingKey" | "higherKey", 1) => {
            let mut found = V::Null;
            for (k, _) in &entries {
                let o = match cmp(k, &a[0]) {
                    Ok(o) => o,
                    Err(e) => return Some(Err(e)),
                };
                let fits = match name {
                    "floorKey" => o.is_le(),
                    "lowerKey" => o.is_lt(),
                    "ceilingKey" => o.is_ge(),
                    _ => o.is_gt(),
                };
                if fits {
                    found = k.clone();
                    if matches!(name, "ceilingKey" | "higherKey") {
                        break;
                    }
                }
            }
            Ok(found)
        }
        ("toString", 0) => Ok(s(&format!("{{{}}}", entries.iter().map(|(k, v)| format!("{}={}", k.text(), v.text())).collect::<Vec<_>>().join(", ")))),
        ("hashCode", 0) => Ok(V::Int(entries.len() as i32)),
        _ => return None,
    })
}

fn sorted_builder_method(b: &JSortedBuilder, this: &V, name: &str, a: &[V]) -> Option<R> {
    Some(match (name, a.len()) {
        ("put", 2) => {
            lock(&b.items).push((a[0].clone(), a[1].clone()));
            Ok(this.clone())
        }
        ("put", 1) => {
            let (k, v) = (call(&a[0], "getKey", &[]).unwrap_or(V::Null), call(&a[0], "getValue", &[]).unwrap_or(V::Null));
            lock(&b.items).push((k, v));
            Ok(this.clone())
        }
        ("putAll", 1) => {
            let pairs = match call(&a[0], "entrySet", &[]).and_then(|e| e.items()) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            let mut items = lock(&b.items);
            for pair in pairs {
                items.push((call(&pair, "getKey", &[]).unwrap_or(V::Null), call(&pair, "getValue", &[]).unwrap_or(V::Null)));
            }
            Ok(this.clone())
        }
        ("build" | "buildOrThrow" | "buildKeepingLast", 0) => {
            let rows = lock(&b.items).clone();
            let map = match comparator_map(b.cmp.clone(), rows) {
                Ok(map) => map,
                Err(e) => return Some(Err(e)),
            };
            if b.reverse {
                if let Some(m) = map.native::<JComparatorMap>() {
                    lock(&m.entries).reverse();
                }
            }
            Ok(map)
        }
        _ => return None,
    })
}

// ---- Guava's Table ----

/// `Table<R, C, V>`: cells in the order put (rows and columns compare as `equals`, by their keys).
pub struct JTable(Mutex<Vec<(V, V, V)>>);

/// `ImmutableTable.builder()`: cells, then `build()`.
pub struct JTableBuilder(Mutex<Vec<(V, V, V)>>);

/// One cell of `cellSet()`.
pub struct JCell(V, V, V);

fn table_put(cells: &mut Vec<(V, V, V)>, row: V, column: V, value: V) -> V {
    let (rk, ck) = (row.key(), column.key());
    match cells.iter_mut().find(|(r, c, _)| r.key() == rk && c.key() == ck) {
        Some(cell) => std::mem::replace(&mut cell.2, value),
        None => {
            cells.push((row, column, value));
            V::Null
        }
    }
}

fn ordered_map(rows: Vec<(V, V)>) -> V {
    let map = crate::new_other("LinkedHashMap", &[]).unwrap_or(V::Null);
    for (k, v) in rows {
        let _ = call(&map, "put", &[k, v]);
    }
    map
}

fn distinct(values: impl Iterator<Item = V>) -> V {
    let set = crate::new_other("LinkedHashSet", &[]).unwrap_or(V::Null);
    for v in values {
        let _ = call(&set, "add", &[v]);
    }
    set
}

fn table_method(t: &JTable, name: &str, a: &[V]) -> Option<R> {
    let key = |v: &V| v.key();
    Some(Ok(match (name, a.len()) {
        ("put", 3) => table_put(&mut lock(&t.0), a[0].clone(), a[1].clone(), a[2].clone()),
        ("putAll", 1) => {
            let cells = match call(&a[0], "cellSet", &[]).and_then(|c| c.items()) {
                Ok(c) => c,
                Err(e) => return Some(Err(e)),
            };
            let mut mine = lock(&t.0);
            for cell in cells {
                let part = |n: &str| call(&cell, n, &[]).unwrap_or(V::Null);
                table_put(&mut mine, part("getRowKey"), part("getColumnKey"), part("getValue"));
            }
            V::Null
        }
        ("get", 2) => lock(&t.0).iter().find(|(r, c, _)| key(r) == key(&a[0]) && key(c) == key(&a[1])).map_or(V::Null, |(_, _, v)| v.clone()),
        ("contains", 2) => V::Bool(lock(&t.0).iter().any(|(r, c, _)| key(r) == key(&a[0]) && key(c) == key(&a[1]))),
        ("containsRow", 1) => V::Bool(lock(&t.0).iter().any(|(r, _, _)| key(r) == key(&a[0]))),
        ("containsColumn", 1) => V::Bool(lock(&t.0).iter().any(|(_, c, _)| key(c) == key(&a[0]))),
        ("containsValue", 1) => V::Bool(lock(&t.0).iter().any(|(_, _, v)| key(v) == key(&a[0]))),
        ("remove", 2) => {
            let mut cells = lock(&t.0);
            match cells.iter().position(|(r, c, _)| key(r) == key(&a[0]) && key(c) == key(&a[1])) {
                Some(i) => cells.remove(i).2,
                None => V::Null,
            }
        }
        ("row", 1) => ordered_map(lock(&t.0).iter().filter(|(r, _, _)| key(r) == key(&a[0])).map(|(_, c, v)| (c.clone(), v.clone())).collect()),
        ("column", 1) => ordered_map(lock(&t.0).iter().filter(|(_, c, _)| key(c) == key(&a[0])).map(|(r, _, v)| (r.clone(), v.clone())).collect()),
        ("rowKeySet", 0) => distinct(lock(&t.0).iter().map(|(r, _, _)| r.clone()).collect::<Vec<_>>().into_iter()),
        ("columnKeySet", 0) => distinct(lock(&t.0).iter().map(|(_, c, _)| c.clone()).collect::<Vec<_>>().into_iter()),
        ("values", 0) => list(lock(&t.0).iter().map(|(_, _, v)| v.clone()).collect()),
        ("cellSet", 0) => list(lock(&t.0).iter().map(|(r, c, v)| native(JCell(r.clone(), c.clone(), v.clone()))).collect()),
        ("rowMap" | "columnMap", 0) => {
            let cells = lock(&t.0).clone();
            let by_row = name == "rowMap";
            let outer: Vec<V> = {
                let mut seen: Vec<V> = Vec::new();
                for (r, c, _) in &cells {
                    let k = if by_row { r } else { c };
                    if !seen.iter().any(|s| key(s) == key(k)) {
                        seen.push(k.clone());
                    }
                }
                seen
            };
            ordered_map(outer.into_iter().map(|k| {
                let inner = cells.iter().filter(|(r, c, _)| key(if by_row { r } else { c }) == key(&k)).map(|(r, c, v)| ((if by_row { c } else { r }).clone(), v.clone())).collect();
                (k, ordered_map(inner))
            }).collect())
        }
        ("size", 0) => V::Int(lock(&t.0).len() as i32),
        ("isEmpty", 0) => V::Bool(lock(&t.0).is_empty()),
        ("clear", 0) => {
            lock(&t.0).clear();
            V::Null
        }
        ("toString", 0) => s(&format!("{{{}}}", lock(&t.0).iter().map(|(r, c, v)| format!("({},{})={}", r.text(), c.text(), v.text())).collect::<Vec<_>>().join(", "))),
        _ => return None,
    }))
}

fn table_builder_method(b: &JTableBuilder, this: &V, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("put", 3) => {
            table_put(&mut lock(&b.0), a[0].clone(), a[1].clone(), a[2].clone());
            this.clone()
        }
        ("put", 1) => {
            let part = |n: &str| call(&a[0], n, &[]).unwrap_or(V::Null);
            table_put(&mut lock(&b.0), part("getRowKey"), part("getColumnKey"), part("getValue"));
            this.clone()
        }
        ("putAll", 1) => {
            let cells = match call(&a[0], "cellSet", &[]).and_then(|c| c.items()) {
                Ok(c) => c,
                Err(e) => return Some(Err(e)),
            };
            for cell in cells {
                let part = |n: &str| call(&cell, n, &[]).unwrap_or(V::Null);
                table_put(&mut lock(&b.0), part("getRowKey"), part("getColumnKey"), part("getValue"));
            }
            this.clone()
        }
        ("orderRowsBy" | "orderColumnsBy", 1) => this.clone(),
        ("build" | "buildOrThrow", 0) => native(JTable(Mutex::new(lock(&b.0).clone()))),
        _ => return None,
    }))
}

// ---- Guava's TypeToken ----

/// `TypeToken.of(type)`: the type it stands for (a class, or a parameterized type, whose arguments jrt doesn't keep).
pub struct JTypeToken(V);

impl JTypeToken {
    pub fn of_class(name: &'static str) -> Self {
        JTypeToken(V::Class(name))
    }
}

fn raw_type(t: &V) -> V {
    match t {
        V::Class(_) => t.clone(),
        other => call(other, "getRawType", &[]).unwrap_or_else(|_| V::Class("Object")),
    }
}

fn type_name(t: &V) -> String {
    match t {
        V::Class(name) => crate::java_class_name(name),
        other => call(other, "getTypeName", &[]).map(|n| n.text()).unwrap_or_else(|_| other.text()),
    }
}

fn type_token_method(t: &JTypeToken, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("getType", 0) => t.0.clone(),
        ("getRawType", 0) => raw_type(&t.0),
        ("wrap" | "unwrap" | "getComponentType", 0) if name != "getComponentType" => native(JTypeToken(t.0.clone())),
        ("getComponentType", 0) => match &t.0 {
            V::Class(c) if c.ends_with("[]") => native(JTypeToken(V::Class(crate::intern(&c[..c.len() - 2])))),
            _ => V::Null,
        },
        ("isArray", 0) => V::Bool(matches!(&t.0, V::Class(c) if c.ends_with("[]"))),
        ("isPrimitive", 0) => V::Bool(matches!(&t.0, V::Class(c) if matches!(*c, "int" | "long" | "double" | "float" | "boolean" | "char" | "byte" | "short"))),
        ("equals", 1) => V::Bool(a[0].native::<JTypeToken>().is_some_and(|o| type_name(&o.0) == type_name(&t.0))),
        ("hashCode", 0) => V::Int(type_name(&t.0).encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))),
        ("toString", 0) => s(&type_name(&t.0)),
        ("isSubtypeOf" | "isSupertypeOf", 1) => {
            let other = a[0].native::<JTypeToken>().map_or_else(|| a[0].clone(), |o| o.0.clone());
            let (sub, sup) = if name == "isSubtypeOf" { (raw_type(&t.0), raw_type(&other)) } else { (raw_type(&other), raw_type(&t.0)) };
            let (V::Class(sub), V::Class(sup)) = (&sub, &sup) else { return Some(Ok(V::Bool(false))) };
            V::Bool(sub == sup || *sup == "Object" || crate::plugin_class(sub).is_some_and(|k| crate::has_type(k, sup)))
        }
        _ => return None,
    }))
}

// ---- The plugin jar: JarURLConnection, Manifest, Attributes ----

/// `jar:file:/...!/entry`'s connection: the entry is one of the plugin's resources; the manifest is its jar's.
pub struct JJarConnection {
    url: String,
    entry: String,
}

/// `java.util.jar.Manifest`: the main section and the named ones, as read from MANIFEST.MF.
pub struct JManifest {
    main: Vec<(String, String)>,
    sections: Vec<(String, Vec<(String, String)>)>,
}

/// `java.util.jar.Attributes`: one section's names and values (names compare without case, as Java's).
pub struct JAttributes(Vec<(String, String)>);

pub fn jar_connection(url: &str) -> V {
    let entry = url.split_once("!/").map(|(_, e)| e.to_owned()).unwrap_or_default();
    native(JJarConnection { url: url.to_owned(), entry })
}

/// The plugin's own resource, as its class loader gives it (jbridge answers from the plugin's files).
fn plugin_resource(name: &str) -> V {
    call(&native(crate::javautil::JPlain), "getResourceAsStream", &[s(name)]).unwrap_or(V::Null)
}

fn read_manifest(bytes: &[u8]) -> JManifest {
    let text = String::from_utf8_lossy(bytes).replace("\r\n", "\n").replace('\r', "\n");
    // A line starting with a space continues the one before.
    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
        match line.strip_prefix(' ') {
            Some(rest) if !lines.is_empty() => lines.last_mut().expect("one").push_str(rest),
            _ => lines.push(line.to_owned()),
        }
    }
    let mut manifest = JManifest { main: Vec::new(), sections: Vec::new() };
    let mut in_main = true;
    for line in lines {
        if line.is_empty() {
            in_main = false;
            continue;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        let (key, value) = (key.trim().to_owned(), value.trim().to_owned());
        if in_main {
            manifest.main.push((key, value));
        } else if key.eq_ignore_ascii_case("Name") {
            manifest.sections.push((value, Vec::new()));
        } else if let Some((_, rows)) = manifest.sections.last_mut() {
            rows.push((key, value));
        }
    }
    manifest
}

fn the_manifest() -> Option<JManifest> {
    let stream = plugin_resource("META-INF/MANIFEST.MF");
    let bytes = call(&stream, "readAllBytes", &[]).ok().and_then(|b| crate::javaio::bytes_of(&b))?;
    Some(read_manifest(&bytes))
}

fn jar_connection_method(c: &JJarConnection, name: &str, _a: &[V]) -> Option<R> {
    Some(Ok(match name {
        "getManifest" => match the_manifest() {
            Some(manifest) => native(manifest),
            None => return Some(Err(exc("IOException", "no manifest in the plugin's jar"))),
        },
        "getMainAttributes" | "getAttributes" => match the_manifest() {
            Some(manifest) => native(JAttributes(manifest.main)),
            None => V::Null,
        },
        "getInputStream" => {
            let stream = plugin_resource(&c.entry);
            if stream.is_null() {
                return Some(Err(exc("FileNotFoundException", format!("JAR entry {} not found", c.entry))));
            }
            stream
        }
        "getEntryName" => s(&c.entry),
        "getJarFileURL" => s(c.url.trim_start_matches("jar:").split("!/").next().unwrap_or_default()),
        "getURL" | "toString" => s(&c.url),
        "connect" | "setUseCaches" | "setDefaultUseCaches" | "setDoInput" => V::Null,
        "getUseCaches" => V::Bool(false),
        _ => return None,
    }))
}

fn manifest_method(m: &JManifest, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("getMainAttributes", 0) => native(JAttributes(m.main.clone())),
        ("getAttributes", 1) => {
            let wanted = a[0].text();
            m.sections.iter().find(|(n, _)| *n == wanted).map_or(V::Null, |(_, rows)| native(JAttributes(rows.clone())))
        }
        ("getEntries", 0) => {
            let map = crate::new_other("LinkedHashMap", &[]).unwrap_or(V::Null);
            for (section, rows) in &m.sections {
                let _ = call(&map, "put", &[s(section), native(JAttributes(rows.clone()))]);
            }
            map
        }
        _ => return None,
    }))
}

fn attributes_method(at: &JAttributes, name: &str, a: &[V]) -> Option<R> {
    let find = |key: &V| at.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(&key.text())).map(|(_, v)| s(v));
    Some(Ok(match (name, a.len()) {
        ("getValue" | "get", 1) => find(&a[0]).unwrap_or(V::Null),
        ("containsKey", 1) => V::Bool(find(&a[0]).is_some()),
        ("size", 0) => V::Int(at.0.len() as i32),
        ("isEmpty", 0) => V::Bool(at.0.is_empty()),
        ("keySet", 0) => crate::new_other("LinkedHashSet", &[list(at.0.iter().map(|(k, _)| s(k)).collect())]).unwrap_or(V::Null),
        ("values", 0) => list(at.0.iter().map(|(_, v)| s(v)).collect()),
        _ => return None,
    }))
}

/// `java.util.Timer`: each scheduled task runs on a thread of its own, as the timer's one thread would run them in
/// turn; `cancel()` stops them all, `TimerTask.cancel()` just that one (WorldEdit's session flushing and its
/// "still working" messages).
pub struct JTimer(std::sync::Arc<std::sync::atomic::AtomicBool>);

fn timer_method(t: &JTimer, name: &str, a: &[V]) -> Option<R> {
    use std::sync::atomic::Ordering;
    match (name, a.len()) {
        ("cancel", 0) => {
            t.0.store(true, Ordering::SeqCst);
            Some(Ok(V::Null))
        }
        ("purge", 0) => Some(Ok(V::Int(0))),
        ("schedule" | "scheduleAtFixedRate", 2 | 3) => {
            if t.0.load(Ordering::SeqCst) {
                return Some(Err(exc("IllegalStateException", "Timer already cancelled.")));
            }
            let task = a[0].clone();
            // A `Date` for the first run: the time until it.
            let delay = match &a[1] {
                V::Int(_) | V::Long(_) => a[1].long(),
                date => call(date, "getTime", &[]).map_or(0, |at| at.long()) - std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64),
            }
            .max(0) as u64;
            let period = a.get(2).map(|p| p.long().max(1) as u64);
            let stopped = t.0.clone();
            let off = move || stopped.load(Ordering::SeqCst) || task_cancelled(&task);
            let task_for_run = a[0].clone();
            let _ = std::thread::Builder::new().name("Timer".into()).stack_size(16 << 20).spawn(move || {
                // Sleeps in slices, so a cancel ends the thread soon.
                let wait = |ms: u64| {
                    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
                    while std::time::Instant::now() < until {
                        if off() {
                            return false;
                        }
                        std::thread::sleep((until - std::time::Instant::now()).min(std::time::Duration::from_millis(50)));
                    }
                    !off()
                };
                if !wait(delay) {
                    return;
                }
                loop {
                    let _ = crate::report("TimerTask", call(&task_for_run, "run", &[]));
                    match period {
                        Some(every) if wait(every) => {}
                        _ => return,
                    }
                }
            });
            Some(Ok(V::Null))
        }
        _ => None,
    }
}

fn task_cancelled(task: &V) -> bool {
    matches!(task, V::Obj(_)) && task.get("$cancelled").is_ok_and(|c| c.truth())
}

// ---- The hooks jrt calls ----

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    Some(Ok(match class {
        // `new Timer()`, `new Timer(name)`, `new Timer(daemon)`, `new Timer(name, daemon)`: tasks on threads of their own.
        "Timer" => native(JTimer(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)))),
        "StringTokenizer" => native(JTokenizer(Mutex::new(TokenState {
            text: a.first().map(V::text).unwrap_or_default().chars().collect(),
            at: 0,
            delimiters: a.get(1).map_or_else(|| " \t\n\r\x0c".chars().collect(), |d| d.text().chars().collect()),
            with_delimiters: a.get(2).is_some_and(V::truth),
        }))),
        "StringJoiner" => native(JJoiner(Mutex::new(JoinState {
            delimiter: a.first().map(V::text).unwrap_or_default(),
            prefix: a.get(1).map(V::text).unwrap_or_default(),
            suffix: a.get(2).map(V::text).unwrap_or_default(),
            parts: Vec::new(),
            empty: None,
        }))),
        "BitSet" => {
            let size = a.first().map_or(64, V::int);
            if size < 0 {
                return Some(Err(exc("NegativeArraySizeException", format!("nbits < 0: {size}"))));
            }
            native(JBitSet(Mutex::new(vec![0; (size as usize).div_ceil(64)])))
        }
        "PropertyResourceBundle" => return Some(a.first().map_or_else(|| Err(exc("NullPointerException", "stream")), bundle_from)),
        "InheritableThreadLocal" => return Some(crate::new_other("ThreadLocal", a)),
        "IdentityHashMap" => native(JIdentityMap(Mutex::new(Vec::new()))),
        // `new ThreadGroup([parent,] name)`: a name threads are made in.
        "ThreadGroup" => native(JThreadGroup(a.last().map(V::text).unwrap_or_default(), Mutex::new(false))),
        // `new Manifest(stream)`.
        "Manifest" => match a.first() {
            Some(stream) => match call(stream, "readAllBytes", &[]).ok().and_then(|b| crate::javaio::bytes_of(&b)) {
                Some(bytes) => native(read_manifest(&bytes)),
                None => native(JManifest { main: Vec::new(), sections: Vec::new() }),
            },
            None => native(JManifest { main: Vec::new(), sections: Vec::new() }),
        },
        "AtomicReferenceArray" => match a.first() {
            Some(V::Arr(items)) => native(JRefArray(Mutex::new(lock(items).clone()))),
            Some(size) => native(JRefArray(Mutex::new(vec![V::Null; size.int().max(0) as usize]))),
            None => return Some(Err(exc("NullPointerException", "length"))),
        },
        // A hand-off queue: jrt's executors run each task on a thread of its own, so a queue that holds is enough.
        "SynchronousQueue" => return Some(crate::new_other("LinkedBlockingQueue", &[])),
        // `new GregorianCalendar(zone)` / `(zone, locale)` (Jackson's StdDateFormat): the calendar, in UTC as jrt keeps it.
        "GregorianCalendar" if (1..=2).contains(&a.len()) => return Some(crate::new_other("GregorianCalendar", &[])),
        "Locale.Builder" => native(JLocaleBuilder(Mutex::new(Default::default()))),
        // A pool's rejection policy: jrt's executors never reject (each task gets a thread), so the policy is only held.
        "ThreadPoolExecutor.CallerRunsPolicy" | "ThreadPoolExecutor.AbortPolicy" | "ThreadPoolExecutor.DiscardPolicy" | "ThreadPoolExecutor.DiscardOldestPolicy" => native(JPlainCheck),
        // No security manager on a modern JVM; a plugin that makes one only checks with it.
        "SecurityManager" | "RuntimePermission" => native(JPlainCheck),
        _ => return None,
    }))
}

/// A `SecurityManager` or `Permission`: nothing to check here.
pub struct JPlainCheck;

pub fn field(class: &str, name: &str) -> Option<V> {
    match (class, name) {
        ("StandardWatchEventKinds", "ENTRY_CREATE" | "ENTRY_DELETE" | "ENTRY_MODIFY" | "OVERFLOW") => Some(s(name)),
        _ => None,
    }
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    let int = |i: usize| a.get(i).map_or(0, V::int);
    let long = |i: usize| a.get(i).map_or(0, V::long);
    let double = |i: usize| a.get(i).map_or(0.0, V::double);
    if name == "getInstance" && matches!(class, "MessageDigest" | "java.security.MessageDigest") {
        return Some(digest_new(a));
    }
    // `URLEncoder.encode(text[, charset])`: letters, digits and `.-*_` kept, a space `+`, the rest %XX of UTF-8.
    if name == "encode" && matches!(class, "URLEncoder" | "java.net.URLEncoder") {
        let text = a.first().map(V::text).unwrap_or_default();
        let mut out = String::with_capacity(text.len());
        for byte in text.bytes() {
            match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => out.push(byte as char),
                b' ' => out.push('+'),
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        return Some(Ok(s(&out)));
    }
    Some(Ok(match (class, name) {
        ("Character", "toCodePoint") => {
            let (high, low) = (a[0].int() as u32, a[1].int() as u32);
            V::Int((((high - 0xD800) << 10) + (low - 0xDC00) + 0x10000) as i32)
        }
        ("Character", "forDigit") => {
            let (digit, radix) = (int(0), int(1));
            if !(2..=36).contains(&radix) || digit < 0 || digit >= radix { V::Char('\0') } else { V::Char(std::char::from_digit(digit as u32, radix as u32).unwrap_or('\0')) }
        }
        ("Character", "isDefined") => V::Bool(char::from_u32(int(0) as u32).is_some()),
        ("Character", "toTitleCase") => V::Char(match &a[0] {
            V::Char(c) => c.to_uppercase().next().unwrap_or(*c),
            other => char::from_u32(other.int() as u32).map_or('\0', |c| c.to_uppercase().next().unwrap_or(c)),
        }),
        ("Character", "getDirectionality") => V::Int(0),
        ("Integer", "rotateLeft") => V::Int((int(0) as u32).rotate_left(int(1) as u32 & 31) as i32),
        ("Integer", "rotateRight") => V::Int((int(0) as u32).rotate_right(int(1) as u32 & 31) as i32),
        ("Integer", "reverseBytes") => V::Int(int(0).swap_bytes()),
        ("Integer", "reverse") => V::Int(int(0).reverse_bits()),
        ("Long", "rotateLeft") => V::Long((long(0) as u64).rotate_left(int(1) as u32 & 63) as i64),
        ("Long", "rotateRight") => V::Long((long(0) as u64).rotate_right(int(1) as u32 & 63) as i64),
        ("Long", "reverseBytes") => V::Long(long(0).swap_bytes()),
        ("Long", "reverse") => V::Long(long(0).reverse_bits()),
        ("Short", "reverseBytes") => V::Int((int(0) as i16).swap_bytes() as i32),
        ("Math" | "StrictMath", "sinh") => V::Double(double(0).sinh()),
        ("Math" | "StrictMath", "cosh") => V::Double(double(0).cosh()),
        ("Math" | "StrictMath", "tanh") => V::Double(double(0).tanh()),
        ("Math" | "StrictMath", "rint") => {
            let x = double(0);
            let r = x.round();
            // Java rounds a .5 to the even neighbour.
            V::Double(if (x - x.trunc()).abs() == 0.5 { 2.0 * (x / 2.0).round() } else { r })
        }
        ("Math" | "StrictMath", "expm1") => V::Double(double(0).exp_m1()),
        ("Math" | "StrictMath", "log1p") => V::Double(double(0).ln_1p()),
        ("Arrays", "deepToString") => s(&deep_text(&a[0])),
        // `Arrays.setAll(array, i -> value)`: each slot from its index (CoreProtect fills its lock array so in a
        // static block; without it the whole class failed to load, and with it container logging).
        ("Arrays", "setAll" | "parallelSetAll") => {
            let V::Arr(items) = &a[0] else { return Some(Err(exc("NullPointerException", "setAll on null"))) };
            let len = lock(items).len();
            for i in 0..len {
                let value = match call(&a[1], "apply", &[V::Int(i as i32)]) {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error)),
                };
                lock(items)[i] = value;
            }
            V::Null
        }
        ("Arrays", "deepEquals") | ("Objects", "deepEquals") => V::Bool(deep_equal(&a[0], &a[1])),
        ("Arrays", "deepHashCode") => V::Int(deep_hash(&a[0])),
        ("Collections", "swap") => {
            let (i, j) = (int(1), int(2));
            let size = call(&a[0], "size", &[]).map(|v| v.int()).unwrap_or(0);
            if i < 0 || j < 0 || i >= size || j >= size {
                return Some(Err(exc("IndexOutOfBoundsException", format!("Index {} out of bounds for length {size}", i.max(j)))));
            }
            let x = call(&a[0], "get", &[V::Int(i)]);
            let y = call(&a[0], "get", &[V::Int(j)]);
            match (x, y) {
                (Ok(x), Ok(y)) => {
                    let _ = call(&a[0], "set", &[V::Int(i), y]);
                    let _ = call(&a[0], "set", &[V::Int(j), x]);
                    V::Null
                }
                (Err(e), _) | (_, Err(e)) => return Some(Err(e)),
            }
        }
        ("System", "getSecurityManager") => V::Null,
        ("Thread", "yield" | "onSpinWait") => {
            std::thread::yield_now();
            V::Null
        }
        ("BitSet", "valueOf") => {
            let words: Vec<u64> = a[0].items().unwrap_or_default().iter().map(|w| w.long() as u64).collect();
            native(JBitSet(Mutex::new(words)))
        }
        ("ResourceBundle", "getBundle") => return Some(get_bundle(a)),
        // A converted plugin runs no native library: Java's own answer when one can't be loaded, which libraries
        // with a pure-Java fallback (lz4-java, zstd-jni's callers) catch.
        ("System" | "Runtime", "loadLibrary" | "load") if !a.is_empty() => {
            return Some(Err(exc("UnsatisfiedLinkError", format!("no {} in java.library.path: a converted plugin loads no native library", a.last().map(V::text).unwrap_or_default()))));
        }
        // Guava's `TypeToken.of(type)`: a type as a value, equal to another of the same type.
        ("TypeToken", "of") if a.len() == 1 => return Some(Ok(native(JTypeToken(a[0].clone())))),
        ("ImmutableTable", "builder") => return Some(Ok(native(JTableBuilder(Mutex::new(Vec::new()))))),
        ("ImmutableTable", "of") if a.is_empty() => return Some(Ok(native(JTable(Mutex::new(Vec::new()))))),
        ("ImmutableTable", "of") if a.len() == 3 => return Some(Ok(native(JTable(Mutex::new(vec![(a[0].clone(), a[1].clone(), a[2].clone())]))))),
        ("HashBasedTable" | "TreeBasedTable", "create") => {
            let table = JTable(Mutex::new(Vec::new()));
            // `create(other)`: a copy.
            if let Some(other) = a.first().filter(|o| o.native::<JTable>().is_some()) {
                *lock(&table.0) = lock(&other.native::<JTable>().expect("a table").0).clone();
            }
            return Some(Ok(native(table)));
        }
        ("ImmutableTable", "copyOf") if a.len() == 1 => {
            return Some(Ok(native(JTable(Mutex::new(a[0].native::<JTable>().map(|t| lock(&t.0).clone()).unwrap_or_default())))));
        }
        ("Tables", "immutableCell") if a.len() == 3 => return Some(Ok(native(JCell(a[0].clone(), a[1].clone(), a[2].clone())))),
        ("ImmutableSortedMap", "orderedBy") if a.len() == 1 => return Some(Ok(native(JSortedBuilder { cmp: Some(a[0].clone()), reverse: false, items: Mutex::new(Vec::new()) }))),
        ("ImmutableSortedMap", "naturalOrder" | "reverseOrder") if a.is_empty() => {
            return Some(Ok(native(JSortedBuilder { cmp: None, reverse: name == "reverseOrder", items: Mutex::new(Vec::new()) })));
        }
        // Guava's `Streams`: a stream of an Iterable, Iterator or Optional; streams one after another; two zipped.
        ("Streams", "stream") if a.len() == 1 => {
            let items = match &a[0] {
                V::Opt(value) => value.as_ref().clone().into_iter().collect(),
                V::Iter(_) => {
                    let mut out = Vec::new();
                    while call(&a[0], "hasNext", &[]).is_ok_and(|v| v.truth()) {
                        match call(&a[0], "next", &[]) {
                            Ok(v) => out.push(v),
                            Err(e) => return Some(Err(e)),
                        }
                    }
                    out
                }
                other => match other.items() {
                    Ok(items) => items,
                    Err(e) => return Some(Err(e)),
                },
            };
            return Some(Ok(V::Stream(std::sync::Arc::new(Mutex::new(items)))));
        }
        ("Streams", "concat") => {
            let mut all = Vec::new();
            for part in a.iter().flat_map(|x| if matches!(x, V::Arr(_)) { x.items().unwrap_or_default() } else { vec![x.clone()] }) {
                match part.items() {
                    Ok(items) => all.extend(items),
                    Err(e) => return Some(Err(e)),
                }
            }
            return Some(Ok(V::Stream(std::sync::Arc::new(Mutex::new(all)))));
        }
        ("Streams", "zip") if a.len() == 3 => {
            let (left, right) = match (a[0].items(), a[1].items()) {
                (Ok(l), Ok(r)) => (l, r),
                (Err(e), _) | (_, Err(e)) => return Some(Err(e)),
            };
            let mut out = Vec::new();
            for (x, y) in left.into_iter().zip(right) {
                match call(&a[2], "apply", &[x, y]) {
                    Ok(v) => out.push(v),
                    Err(e) => return Some(Err(e)),
                }
            }
            return Some(Ok(V::Stream(std::sync::Arc::new(Mutex::new(out)))));
        }
        ("Streams", "mapWithIndex") if a.len() == 2 => {
            let items = match a[0].items() {
                Ok(items) => items,
                Err(e) => return Some(Err(e)),
            };
            let mut out = Vec::new();
            for (i, x) in items.into_iter().enumerate() {
                match call(&a[1], "apply", &[x, V::Long(i as i64)]) {
                    Ok(v) => out.push(v),
                    Err(e) => return Some(Err(e)),
                }
            }
            return Some(Ok(V::Stream(std::sync::Arc::new(Mutex::new(out)))));
        }
        // `IntStream.of(1, 2)` / `of(int[])`, and its long and double kin.
        ("IntStream" | "LongStream" | "DoubleStream", "of") => {
            let items: Vec<V> = a.iter().flat_map(|x| if matches!(x, V::Arr(_)) { x.items().unwrap_or_default() } else { vec![x.clone()] }).collect();
            return Some(Ok(V::Stream(std::sync::Arc::new(Mutex::new(items)))));
        }
        ("ByteOrder", "nativeOrder") if a.is_empty() => {
            return Some(Ok(s(if cfg!(target_endian = "little") { "LITTLE_ENDIAN" } else { "BIG_ENDIAN" })));
        }
        // Guava's `Throwables`: a trace as a list or as text, the root cause, and rethrowing what is unchecked.
        ("Throwables", "lazyStackTrace") if a.len() == 1 => return Some(call(&a[0], "getStackTrace", &[]).and_then(|t| t.items()).map(list)),
        ("Throwables", "getStackTraceAsString") if a.len() == 1 => {
            let mut text = crate::throwable_text(&a[0]);
            for frame in call(&a[0], "getStackTrace", &[]).and_then(|t| t.items()).unwrap_or_default() {
                text.push_str(&format!("\n\tat {}", frame.text()));
            }
            return Some(Ok(s(&text)));
        }
        ("Throwables", "getRootCause") if a.len() == 1 => {
            let mut at = a[0].clone();
            for _ in 0..64 {
                match call(&at, "getCause", &[]) {
                    Ok(cause) if !cause.is_null() && !crate::same(&cause, &at) => at = cause,
                    _ => break,
                }
            }
            return Some(Ok(at));
        }
        ("Throwables", "getCausalChain") if a.len() == 1 => {
            let mut chain = vec![a[0].clone()];
            while chain.len() < 64 {
                match call(chain.last().expect("one"), "getCause", &[]) {
                    Ok(cause) if !cause.is_null() && !chain.iter().any(|c| crate::same(c, &cause)) => chain.push(cause),
                    _ => break,
                }
            }
            return Some(Ok(list(chain)));
        }
        ("Throwables", "throwIfUnchecked") if a.len() == 1 => {
            return Some(if crate::instance_of(&a[0], "RuntimeException") || crate::instance_of(&a[0], "Error") { Err(a[0].clone()) } else { Ok(V::Null) });
        }
        ("Throwables", "throwIfInstanceOf") if a.len() == 2 => {
            let V::Class(class) = &a[1] else { return Some(Ok(V::Null)) };
            return Some(if crate::instance_of(&a[0], class) { Err(a[0].clone()) } else { Ok(V::Null) });
        }
        ("Throwables", "propagate") if a.len() == 1 => {
            return Some(Err(if crate::instance_of(&a[0], "RuntimeException") || crate::instance_of(&a[0], "Error") {
                a[0].clone()
            } else {
                crate::new_other("RuntimeException", &[s(&crate::throwable_text(&a[0])), a[0].clone()]).unwrap_or_else(|e| e)
            }));
        }
        // `Collections.newSetFromMap(new IdentityHashMap<>())`: a set by identity.
        ("Collections", "newSetFromMap") if a.first().is_some_and(|m| m.native::<JIdentityMap>().is_some()) => native(JIdentitySet(Mutex::new(Vec::new()))),
        ("ResourceBundle", "clearCache") => V::Null,
        ("HexFormat", "of") => native(JHex { upper: false }),
        ("MethodHandles", "lookup" | "publicLookup" | "privateLookupIn") => native(JLookup),
        ("MethodHandles", "invoker" | "exactInvoker") if a.len() == 1 => {
            native(JHandle::Invoker(a[0].native::<JMethodType>().cloned().unwrap_or_else(JMethodType::unknown)))
        }
        ("MethodHandles", "filterReturnValue") if a.len() == 2 => match (as_handle(&a[0]), as_handle(&a[1])) {
            (Some(target), Some(filter)) => native(JHandle::Filter(Box::new(target), Box::new(filter))),
            _ => return Some(Err(exc("NullPointerException", "filterReturnValue of null"))),
        },
        ("MethodHandles", "catchException") if a.len() == 3 => match (as_handle(&a[0]), as_handle(&a[2])) {
            (Some(target), Some(handler)) => native(JHandle::Catch(Box::new(target), class_name(&a[1]), Box::new(handler))),
            _ => return Some(Err(exc("NullPointerException", "catchException of null"))),
        },
        ("MethodHandles", "dropArguments") if a.len() >= 2 => match as_handle(&a[0]) {
            Some(target) => {
                let types = match &a[2..] {
                    [V::Arr(_) | V::List(_)] => a[2].items().unwrap_or_default(),
                    rest => rest.to_vec(),
                };
                native(JHandle::Drop(Box::new(target), a[1].int().max(0) as usize, types))
            }
            None => return Some(Err(exc("NullPointerException", "dropArguments of null"))),
        },
        ("MethodHandles", "insertArguments") if a.len() >= 2 => match as_handle(&a[0]) {
            Some(target) => {
                let values = match &a[2..] {
                    [V::Arr(_)] => a[2].items().unwrap_or_default(),
                    rest => rest.to_vec(),
                };
                native(JHandle::Insert(Box::new(target), a[1].int().max(0) as usize, values))
            }
            None => return Some(Err(exc("NullPointerException", "insertArguments of null"))),
        },
        ("MethodHandles", "constant") if a.len() == 2 => native(JHandle::Constant(a[1].clone())),
        ("MethodHandles", "guardWithTest") if a.len() == 3 => match (as_handle(&a[0]), as_handle(&a[1]), as_handle(&a[2])) {
            (Some(test), Some(target), Some(fallback)) => native(JHandle::Guard(Box::new(test), Box::new(target), Box::new(fallback))),
            _ => return Some(Err(exc("NullPointerException", "guardWithTest of null"))),
        },
        ("MethodHandles", "permuteArguments") if a.len() >= 2 => match as_handle(&a[0]) {
            Some(target) => {
                let order: Vec<usize> = match &a[2..] {
                    [V::Arr(items)] => lock(items).iter().map(|i| i.int().max(0) as usize).collect(),
                    rest => rest.iter().map(|i| i.int().max(0) as usize).collect(),
                };
                native(JHandle::Permute(Box::new(target), a[1].native::<JMethodType>().cloned().unwrap_or_else(JMethodType::unknown), order))
            }
            None => return Some(Err(exc("NullPointerException", "permuteArguments of null"))),
        },
        ("MethodHandles", "collectArguments") if a.len() == 3 => match (as_handle(&a[0]), as_handle(&a[2])) {
            (Some(target), Some(filter)) => native(JHandle::Collect(Box::new(target), a[1].int().max(0) as usize, Box::new(filter))),
            _ => return Some(Err(exc("NullPointerException", "collectArguments of null"))),
        },
        ("MethodHandles", "filterArguments") if a.len() >= 2 => match as_handle(&a[0]) {
            Some(target) => {
                let filters: Vec<Option<JHandle>> = match &a[2..] {
                    [V::Arr(items)] => lock(items).iter().map(as_handle).collect(),
                    rest => rest.iter().map(as_handle).collect(),
                };
                native(JHandle::FilterArgs(Box::new(target), a[1].int().max(0) as usize, filters))
            }
            None => return Some(Err(exc("NullPointerException", "filterArguments of null"))),
        },
        ("MethodHandles", "throwException") => native(JHandle::Throw),
        ("MethodHandles", "identity") if a.len() == 1 => native(JHandle::Identity),
        // `LambdaMetafactory.metafactory(lookup, name, factoryType, samType, implementation, instantiatedType)`: a call
        // site whose target, given the captured values, makes the lambda.
        ("LambdaMetafactory", "metafactory" | "altMetafactory") if a.len() >= 5 => match as_handle(&a[4]) {
            Some(implementation) => native(JCallSite(JHandle::Lambda(Box::new(implementation)))),
            None => return Some(Err(exc("LambdaConversionException", "no implementation handle"))),
        },
        ("MethodType", "methodType") => native(method_type_of(a)),
        ("MethodType", "genericMethodType") => {
            let count = a.first().map_or(0, V::int).max(0) as usize;
            native(JMethodType { ret: V::Class("Object"), params: vec![V::Class("Object"); count] })
        }
        ("System", "mapLibraryName") => {
            let library = a.first().map(V::text).unwrap_or_default();
            s(&if cfg!(windows) { format!("{library}.dll") } else if cfg!(target_os = "macos") { format!("lib{library}.dylib") } else { format!("lib{library}.so") })
        }
        ("FileSystems", "getDefault") => native(JFileSystem),
        // No security manager: a privileged action just runs (JDA's SimpleLogger, many shaded libraries).
        ("AccessController", "doPrivileged" | "doPrivilegedWithCombiner") => {
            return Some(match a.first() {
                Some(action) => call(action, "run", &[]),
                None => Err(exc("NullPointerException", "action")),
            });
        }
        ("AccessController", "getContext") => V::Null,
        ("AccessController", "checkPermission") => V::Null,
        // An InetAddress is jbridge's: a held `std::net::IpAddr`.
        ("InetAddress", "getLoopbackAddress") => ip_value(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
        ("InetAddress", "getLocalHost") => ip_value(local_ip()),
        ("InetAddress", "getByName" | "getAllByName") => {
            let host = a.first().map(V::text).unwrap_or_default();
            let found = resolve(&host);
            return Some(match found {
                Some(ips) if name == "getAllByName" => Ok(arr(ips.into_iter().map(ip_value).collect())),
                Some(ips) => Ok(ip_value(ips[0])),
                None => Err(exc("UnknownHostException", format!("{host}: no address"))),
            });
        }
        ("InetAddress", "getByAddress") => {
            let bytes: Vec<u8> = a.last().and_then(|v| v.items().ok()).unwrap_or_default().iter().map(|b| b.int() as u8).collect();
            return Some(match bytes.len() {
                4 => Ok(ip_value(std::net::IpAddr::from([bytes[0], bytes[1], bytes[2], bytes[3]]))),
                16 => {
                    let mut octets = [0u8; 16];
                    octets.copy_from_slice(&bytes);
                    Ok(ip_value(std::net::IpAddr::from(octets)))
                }
                n => Err(exc("UnknownHostException", format!("addr is of illegal length {n}"))),
            });
        }
        _ => return None,
    }))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if matches!(name, "update" | "digest" | "reset" | "getAlgorithm" | "getDigestLength")
        && let Some(d) = this.native::<JDigest>()
    {
        return digest_method(d, name, a);
    }
    // `list.clone()` (ArrayList's), and an array the game handed over as a list (`inventory.getContents()`, which
    // CoreProtect copies with `(ItemStack[]) array.clone()`): the same items in a new list.
    if name == "clone" && a.is_empty()
        && let V::List(items) = this
    {
        return Some(Ok(crate::list(lock(items).clone())));
    }
    // The common names first, cheaply: most calls aren't this module's.
    match name {
        "hasMoreElements" | "nextElement" if a.is_empty() => {
            if let Some(t) = this.native::<JTokenizer>() {
                return tokenizer_method(t, name, a);
            }
            // Any iterator is an Enumeration's work (Collections.enumeration, Vector.elements, Hashtable.keys).
            if !matches!(this, V::Obj(_)) {
                return Some(call(this, if name == "hasMoreElements" { "hasNext" } else { "next" }, &[]));
            }
            return None;
        }
        "subSequence" if a.len() == 2 && matches!(this, V::Str(_) | V::Sb(_)) => return Some(call(this, "substring", a)),
        "trimToSize" | "ensureCapacity" if matches!(this, V::List(_)) => return Some(Ok(V::Null)),
        "getContextClassLoader" if this.native::<crate::javautil::JThread>().is_some() => return Some(call_static("ClassLoader", "getSystemClassLoader", &[])),
        "setContextClassLoader" if this.native::<crate::javautil::JThread>().is_some() => return Some(Ok(V::Null)),
        "getUncaughtExceptionHandler" if this.native::<crate::javautil::JThread>().is_some() => return Some(Ok(V::Null)),
        "getStackTrace" if a.is_empty() && this.native::<crate::javautil::JThread>().is_some() => return Some(Ok(frames())),
        "getStackTrace" if a.is_empty() && matches!(this, V::Exc(_)) => {
            let V::Exc(thrown) = this else { return None };
            return Some(Ok(frames_of(&thrown.trace)));
        }
        "notifyAll" | "notify" if a.is_empty() && !matches!(this, V::Obj(_)) => return Some(Ok(V::Null)),
        "checkPermission" | "checkAccess" | "checkRead" | "checkWrite" | "checkExit" if this.native::<JPlainCheck>().is_some() => return Some(Ok(V::Null)),
        // `builder.append(text, start, end)` and `append(chars, offset, length)`.
        "append" if a.len() == 3 && matches!(this, V::Sb(_)) => {
            let V::Sb(text) = this else { return None };
            let piece: String = match &a[0] {
                V::Arr(chars) => {
                    let chars = lock(chars);
                    let (from, count) = (a[1].int().max(0) as usize, a[2].int().max(0) as usize);
                    chars.iter().skip(from).take(count).map(|c| c.text()).collect()
                }
                other => {
                    let units: Vec<u16> = other.text().encode_utf16().collect();
                    let (from, to) = (a[1].int().max(0) as usize, (a[2].int().max(0) as usize).min(units.len()));
                    String::from_utf16_lossy(&units[from.min(to)..to])
                }
            };
            lock(text).push_str(&piece);
            return Some(Ok(this.clone()));
        }
        "register" if a.first().is_some_and(|w| w.native::<JWatch>().is_some()) => return Some(Ok(native(JWatchKey(this.clone())))),
        // The server class's `commandMap` field (`getServer().getClass().getDeclaredField("commandMap")`): the bridge's,
        // asked before jrt's reflection says there is no such field (Tebex's /buy).
        "getDeclaredField" | "getField" if a.len() == 1 && a[0].text() == "commandMap" => {
            if let Some(found) = crate::hooks().and_then(|h| (h.call)(this, name, a)) {
                return Some(found);
            }
        }
        "getAnnotatedSuperclass" if matches!(this, V::Class(_)) => return Some(call(this, "getGenericSuperclass", &[]).map(|t| native(JAnnotatedType(t)))),
        "getGenericInterfaces" if matches!(this, V::Class(_)) => return Some(Ok(generic_interfaces(&class_name(this)))),
        "getAnnotatedInterfaces" if matches!(this, V::Class(_)) => {
            let types = generic_interfaces(&class_name(this)).items().unwrap_or_default();
            return Some(Ok(arr(types.into_iter().map(|t| native(JAnnotatedType(t))).collect())));
        }
        "getOrDefault" | "putIfAbsent" | "computeIfAbsent" | "computeIfPresent" | "compute" | "merge" | "forEach" | "replace" if matches!(this, V::Obj(_)) => {
            return map_default(this, name, a);
        }
        // Gson's `TypeAdapter.nullSafe()` on a plugin's own adapter: the adapter (FAWE's ChunkDeleter).
        "nullSafe" if a.is_empty() && matches!(this, V::Obj(_)) && crate::instance_of(this, "TypeAdapter") => return Some(Ok(this.clone())),
        // A plugin's `RecursiveTask`/`RecursiveAction` (ForkJoinTask): no work-stealing pool here, so `fork()` works it
        // out at once on this thread and `join()`/`invoke()`/`get()` give what it made (FAWE's ApplyTask).
        "fork" | "join" | "invoke" | "get" | "getRawResult" | "isDone" | "tryUnfork" | "quietlyJoin" | "quietlyInvoke"
            if a.is_empty() && matches!(this, V::Obj(_)) && (crate::instance_of(this, "RecursiveAction") || crate::instance_of(this, "RecursiveTask") || crate::instance_of(this, "ForkJoinTask") || crate::instance_of(this, "CountedCompleter")) =>
        {
            let done = this.get("$fj_done").map_or(false, |d| d.truth());
            if name == "isDone" {
                return Some(Ok(V::Bool(done)));
            }
            if name == "tryUnfork" {
                return Some(Ok(V::Bool(false)));
            }
            if !done {
                let made = match call(this, "compute", &[]) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                let _ = this.set("$fj_result", made);
                let _ = this.set("$fj_done", V::Bool(true));
            }
            return Some(match name {
                "fork" => Ok(this.clone()),
                "quietlyJoin" | "quietlyInvoke" => Ok(V::Null),
                _ => this.get("$fj_result"),
            });
        }
        // `TimerTask`'s own methods on a plugin's task.
        "cancel" | "scheduledExecutionTime" if a.is_empty() && matches!(this, V::Obj(_)) && crate::instance_of(this, "TimerTask") => {
            if name == "scheduledExecutionTime" {
                return Some(Ok(V::Long(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64))));
            }
            let was = this.get("$cancelled").map_or(false, |c| c.truth());
            return Some(this.set("$cancelled", V::Bool(true)).map(|_| V::Bool(!was)));
        }
        _ => {}
    }
    if let V::Native(_) = this {
        if let Some(t) = this.native::<JTimer>() {
            return timer_method(t, name, a);
        }
        if let Some(t) = this.native::<JTokenizer>() {
            return tokenizer_method(t, name, a);
        }
        if let Some(j) = this.native::<JJoiner>() {
            return joiner_method(j, this, name, a);
        }
        if let Some(b) = this.native::<JBitSet>() {
            return bitset_method(b, name, a);
        }
        if let Some(f) = this.native::<JFrame>() {
            return frame_method(f, name);
        }
        if let Some(b) = this.native::<JBundle>() {
            return bundle_method(b, name, a);
        }
        if let Some(h) = this.native::<JHex>() {
            return hex_method(h, name, a);
        }
        if let Some(r) = this.native::<JRefArray>() {
            return ref_array_method(r, name, a);
        }
        if this.native::<JLookup>().is_some() {
            return lookup_method(name, a);
        }
        if let Some(h) = this.native::<JHandle>() {
            return handle_method(h, this, name, a);
        }
        if let Some(t) = this.native::<JMethodType>() {
            return method_type_method(t, this, name, a);
        }
        if let Some(site) = this.native::<JCallSite>() {
            return match name {
                "dynamicInvoker" | "getTarget" => Some(Ok(native(site.0.clone()))),
                "type" => Some(Ok(native(handle_type(&site.0)))),
                _ => None,
            };
        }
        if this.native::<JFileSystem>().is_some() {
            return Some(match name {
                "getPath" => call_static("Paths", "get", a),
                "getSeparator" => Ok(s(std::path::MAIN_SEPARATOR_STR)),
                "newWatchService" => Ok(native(JWatch(Default::default()))),
                "isOpen" => Ok(V::Bool(true)),
                "isReadOnly" => Ok(V::Bool(false)),
                "close" => Ok(V::Null),
                _ => return None,
            });
        }
        if let Some(w) = this.native::<JWatch>() {
            return watch_method(w, name, a);
        }
        if let Some(k) = this.native::<JWatchKey>() {
            return Some(Ok(match name {
                "pollEvents" => list(Vec::new()),
                "reset" | "isValid" => V::Bool(true),
                "cancel" => V::Null,
                "watchable" => k.0.clone(),
                _ => return None,
            }));
        }
        if let Some(b) = this.native::<JLocaleBuilder>() {
            return locale_builder_method(b, this, name, a);
        }
        if let Some(t) = this.native::<JAnnotatedType>() {
            return annotated_method(t, name);
        }
        if let Some(m) = this.native::<JIdentityMap>() {
            return identity_map_method(m, this, name, a);
        }
        if let Some(t) = this.native::<JTable>() {
            return table_method(t, name, a);
        }
        if let Some(b) = this.native::<JTableBuilder>() {
            return table_builder_method(b, this, name, a);
        }
        if let Some(c) = this.native::<JCell>() {
            return match name {
                "getRowKey" => Some(Ok(c.0.clone())),
                "getColumnKey" => Some(Ok(c.1.clone())),
                "getValue" => Some(Ok(c.2.clone())),
                "toString" => Some(Ok(s(&format!("({},{})={}", c.0.text(), c.1.text(), c.2.text())))),
                _ => None,
            };
        }
        if let Some(m) = this.native::<JComparatorMap>() {
            return comparator_map_method(m, name, a);
        }
        if let Some(b) = this.native::<JSortedBuilder>() {
            return sorted_builder_method(b, this, name, a);
        }
        if let Some(g) = this.native::<JThreadGroup>() {
            return thread_group_method(g, name, a);
        }
        if let Some(t) = this.native::<JTypeToken>() {
            return type_token_method(t, name, a);
        }
        if let Some(c) = this.native::<JJarConnection>() {
            return jar_connection_method(c, name, a);
        }
        if let Some(m) = this.native::<JManifest>() {
            return manifest_method(m, name, a);
        }
        if let Some(at) = this.native::<JAttributes>() {
            return attributes_method(at, name, a);
        }
        if let Some(set) = this.native::<JIdentitySet>() {
            return identity_set_method(set, name, a);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(r: Option<R>) -> V {
        match r {
            Some(Ok(v)) => v,
            Some(Err(e)) => panic!("threw {}", e.text()),
            None => panic!("no value"),
        }
    }

    #[test]
    fn a_tokenizer_splits_counts_and_keeps_delimiters_when_asked() {
        let t = ok(construct("StringTokenizer", &[s("a, b,,c"), s(", ")]));
        assert_eq!(ok(method(&t, "countTokens", &[])).int(), 3);
        assert_eq!(ok(method(&t, "nextToken", &[])).text(), "a");
        assert!(ok(method(&t, "hasMoreTokens", &[])).truth());
        assert_eq!(ok(method(&t, "nextElement", &[])).text(), "b");
        assert_eq!(ok(method(&t, "nextToken", &[])).text(), "c");
        assert!(!ok(method(&t, "hasMoreElements", &[])).truth());
        assert!(matches!(method(&t, "nextToken", &[]), Some(Err(_))));
        let d = ok(construct("StringTokenizer", &[s("1+2"), s("+"), V::Bool(true)]));
        let all: Vec<String> = (0..3).map(|_| ok(method(&d, "nextToken", &[])).text()).collect();
        assert_eq!(all, ["1", "+", "2"]);
        let spaces = ok(construct("StringTokenizer", &[s("  one two\tthree ")]));
        assert_eq!(ok(method(&spaces, "countTokens", &[])).int(), 3);
    }

    #[test]
    fn a_joiner_joins_with_prefix_suffix_and_empty_value() {
        let j = ok(construct("StringJoiner", &[s(", "), s("["), s("]")]));
        assert_eq!(ok(method(&j, "toString", &[])).text(), "[]");
        ok(method(&j, "setEmptyValue", &[s("EMPTY")]));
        assert_eq!(ok(method(&j, "toString", &[])).text(), "EMPTY");
        ok(method(&j, "add", &[s("a")]));
        ok(method(&j, "add", &[V::Null]));
        assert_eq!(ok(method(&j, "toString", &[])).text(), "[a, null]");
        assert_eq!(ok(method(&j, "length", &[])).int(), 9);
    }

    #[test]
    fn a_bitset_sets_finds_and_combines_bits() {
        let b = ok(construct("BitSet", &[]));
        ok(method(&b, "set", &[V::Int(3)]));
        ok(method(&b, "set", &[V::Int(70), V::Int(72)]));
        assert!(ok(method(&b, "get", &[V::Int(71)])).truth());
        assert_eq!(ok(method(&b, "cardinality", &[])).int(), 3);
        assert_eq!(ok(method(&b, "nextSetBit", &[V::Int(4)])).int(), 70);
        assert_eq!(ok(method(&b, "nextClearBit", &[V::Int(70)])).int(), 72);
        assert_eq!(ok(method(&b, "length", &[])).int(), 72);
        assert_eq!(ok(method(&b, "toString", &[])).text(), "{3, 70, 71}");
        let c = ok(construct("BitSet", &[V::Int(8)]));
        ok(method(&c, "set", &[V::Int(3)]));
        ok(method(&b, "and", &[c.clone()]));
        assert_eq!(ok(method(&b, "toString", &[])).text(), "{3}");
        assert!(ok(method(&b, "equals", &[c])).truth());
        assert!(matches!(method(&b, "get", &[V::Int(-1)]), Some(Err(_))));
    }

    #[test]
    fn numbers_and_characters_answer_as_java() {
        // A lone surrogate half isn't a Rust char: jrt carries one as its number.
        assert_eq!(ok(static_call("Character", "toCodePoint", &[V::Int(0xD83D), V::Int(0xDE00)])).int(), 0x1F600);
        assert_eq!(ok(static_call("Integer", "rotateLeft", &[V::Int(1), V::Int(33)])).int(), 2);
        assert_eq!(ok(static_call("Integer", "reverseBytes", &[V::Int(0x01020304)])).int(), 0x04030201);
        assert_eq!(ok(static_call("Long", "rotateRight", &[V::Long(1), V::Int(1)])).long(), i64::MIN);
        assert_eq!(ok(static_call("Math", "rint", &[V::Double(2.5)])).double(), 2.0);
        assert_eq!(ok(static_call("Math", "rint", &[V::Double(3.5)])).double(), 4.0);
        assert!((ok(static_call("Math", "tanh", &[V::Double(1.0)])).double() - 0.7615941559557649).abs() < 1e-12);
        assert_eq!(ok(static_call("Character", "forDigit", &[V::Int(11), V::Int(16)])).text(), "b");
    }

    #[test]
    fn deep_arrays_print_compare_and_hash() {
        let inner = arr(vec![V::Int(1), V::Int(2)]);
        let outer = arr(vec![inner.clone(), V::Null]);
        assert_eq!(ok(static_call("Arrays", "deepToString", &[outer.clone()])).text(), "[[1, 2], null]");
        let again = arr(vec![arr(vec![V::Int(1), V::Int(2)]), V::Null]);
        assert!(ok(static_call("Arrays", "deepEquals", &[outer.clone(), again.clone()])).truth());
        assert_eq!(ok(static_call("Arrays", "deepHashCode", &[outer])).int(), ok(static_call("Arrays", "deepHashCode", &[again])).int());
    }

    #[test]
    fn an_iterator_answers_as_an_enumeration_and_text_as_a_sequence() {
        let items = list(vec![s("x"), s("y")]);
        let it = call(&items, "iterator", &[]).unwrap_or_else(|e| panic!("threw {}", e.text()));
        assert!(ok(method(&it, "hasMoreElements", &[])).truth());
        assert_eq!(ok(method(&it, "nextElement", &[])).text(), "x");
        assert_eq!(ok(method(&s("hello"), "subSequence", &[V::Int(1), V::Int(3)])).text(), "el");
    }

    #[test]
    fn a_stack_trace_has_frames_with_names() {
        let thread = call_static("Thread", "currentThread", &[]).unwrap_or_else(|e| panic!("threw {}", e.text()));
        let trace = ok(method(&thread, "getStackTrace", &[]));
        let frames = trace.items().unwrap_or_else(|e| panic!("threw {}", e.text()));
        assert!(frames.len() >= 3);
        assert_eq!(ok(method(&frames[2], "getMethodName", &[])).text(), "caller");
    }

    #[test]
    fn a_reference_array_sets_swaps_and_updates() {
        let r = ok(construct("AtomicReferenceArray", &[V::Int(3)]));
        assert_eq!(ok(method(&r, "length", &[])).int(), 3);
        ok(method(&r, "set", &[V::Int(1), s("a")]));
        assert!(ok(method(&r, "compareAndSet", &[V::Int(1), s("a"), s("b")])).truth());
        assert!(!ok(method(&r, "compareAndSet", &[V::Int(1), s("a"), s("c")])).truth());
        assert_eq!(ok(method(&r, "getAndSet", &[V::Int(1), s("d")])).text(), "b");
        assert_eq!(ok(method(&r, "toString", &[])).text(), "[null, d, null]");
        assert!(matches!(method(&r, "get", &[V::Int(3)]), Some(Err(_))));
    }

    #[test]
    fn a_method_handle_calls_what_it_names() {
        let lookup = ok(static_call("MethodHandles", "lookup", &[]));
        let upper = ok(method(&lookup, "findVirtual", &[V::Class("String"), s("toUpperCase"), V::Null]));
        assert_eq!(ok(method(&upper, "invoke", &[s("abc")])).text(), "ABC");
        let bound = ok(method(&upper, "bindTo", &[s("xy")]));
        assert_eq!(ok(method(&bound, "invokeWithArguments", &[arr(vec![])])).text(), "XY");
        let max = ok(method(&lookup, "findStatic", &[V::Class("Math"), s("max"), V::Null]));
        assert_eq!(ok(method(&max, "invoke", &[V::Int(3), V::Int(7)])).int(), 7);
    }

    #[test]
    fn method_handle_combinators_work_as_pistons_converters_use_them() {
        let lookup = ok(static_call("MethodHandles", "lookup", &[]));
        let to_string = ok(static_call("MethodType", "methodType", &[V::Class("Integer"), V::Class("String")]));
        // Integer.valueOf(String), typed.
        let value_of = ok(method(&lookup, "findStatic", &[V::Class("Integer"), s("valueOf"), to_string]));
        let t = ok(method(&value_of, "type", &[]));
        assert_eq!(ok(method(&t, "parameterType", &[V::Int(0)])).text(), V::Class("String").text());
        // A second argument the converter takes and ignores.
        let dropped = ok(static_call("MethodHandles", "dropArguments", &[value_of, V::Int(1), V::Class("InjectedValueAccess")]));
        assert_eq!(ok(method(&ok(method(&dropped, "type", &[])), "parameterCount", &[])).int(), 2);
        // Wrap the result, and turn a thrown exception into a value.
        let wrap = ok(method(&lookup, "findStatic", &[V::Class("String"), s("valueOf"), V::Null]));
        let filtered = ok(static_call("MethodHandles", "filterReturnValue", &[dropped, wrap]));
        let failed = ok(static_call("MethodHandles", "constant", &[V::Class("String"), s("failed")]));
        let guarded = ok(static_call("MethodHandles", "catchException", &[filtered, V::Class("Throwable"), failed]));
        // LambdaMetafactory over an invoker: a function of the handle, giving a converter that calls it.
        let invoker = ok(static_call("MethodHandles", "invoker", &[V::Null]));
        let site = ok(static_call("LambdaMetafactory", "metafactory", &[lookup, s("convert"), V::Null, V::Null, invoker, V::Null]));
        let make = ok(method(&site, "dynamicInvoker", &[]));
        let converter = ok(method(&make, "invokeExact", &[guarded]));
        assert_eq!(ok(Some(call(&converter, "convert", &[s("42"), V::Null]))).text(), "42");
        assert_eq!(ok(Some(call(&converter, "convert", &[s("x"), V::Null]))).text(), "failed");
    }

    #[test]
    fn a_case_insensitive_sorted_map_finds_keys_whatever_their_case() {
        let insensitive = crate::other_field("String", "CASE_INSENSITIVE_ORDER");
        let b = ok(static_call("ImmutableSortedMap", "orderedBy", &[insensitive]));
        for (k, v) in [("north", 1), ("East", 2), ("south", 3)] {
            ok(method(&b, "put", &[s(k), V::Int(v)]));
        }
        let map = ok(method(&b, "build", &[]));
        assert_eq!(ok(method(&map, "get", &[s("NORTH")])).int(), 1);
        assert_eq!(ok(method(&map, "get", &[s("east")])).int(), 2);
        assert!(ok(method(&map, "get", &[s("up")])).is_null());
        assert_eq!(ok(method(&map, "keySet", &[])).text(), "[East, north, south]");
    }

    #[test]
    fn a_guava_table_keeps_a_value_per_row_and_column() {
        let b = ok(static_call("ImmutableTable", "builder", &[]));
        ok(method(&b, "put", &[s("part"), s("load"), V::Int(1)]));
        ok(method(&b, "put", &[s("part"), s("save"), V::Int(2)]));
        ok(method(&b, "put", &[s("other"), s("load"), V::Int(3)]));
        let t = ok(method(&b, "build", &[]));
        assert_eq!(ok(method(&t, "get", &[s("part"), s("save")])).int(), 2);
        assert_eq!(ok(method(&t, "row", &[s("part")])).text(), "{load=1, save=2}");
        assert_eq!(ok(method(&t, "column", &[s("load")])).text(), "{part=1, other=3}");
        assert_eq!(ok(method(&t, "size", &[])).int(), 3);
    }

    #[test]
    fn small_stoppers_answer() {
        let builder = ok(construct("Locale.Builder", &[]));
        ok(method(&builder, "setLanguage", &[s("EN")]));
        ok(method(&builder, "setRegion", &[s("us")]));
        assert_eq!(ok(method(&builder, "build", &[])).text(), "en_US");
        let library = ok(static_call("System", "mapLibraryName", &[s("jansi")])).text();
        assert!(library.contains("jansi"));
        let fs = ok(static_call("FileSystems", "getDefault", &[]));
        let watch = ok(method(&fs, "newWatchService", &[]));
        assert!(ok(method(&watch, "poll", &[])).is_null());
        ok(method(&watch, "close", &[]));
        assert!(matches!(method(&watch, "take", &[]), Some(Err(_))));
        let sb = crate::new_other("StringBuilder", &[]).unwrap_or_else(|e| panic!("threw {}", e.text()));
        ok(method(&sb, "append", &[s("hello"), V::Int(1), V::Int(4)]));
        assert_eq!(sb.text(), "ell");
    }

    #[test]
    fn privileged_actions_run_and_addresses_resolve() {
        let action = crate::func(|_| Ok(s("ran")));
        assert_eq!(ok(static_call("AccessController", "doPrivileged", &[action])).text(), "ran");
        let loopback = ok(static_call("InetAddress", "getLoopbackAddress", &[]));
        assert!(loopback.native::<Mutex<std::net::IpAddr>>().is_some());
        let parsed = ok(static_call("InetAddress", "getByName", &[s("10.0.0.7")]));
        assert_eq!(parsed.native::<Mutex<std::net::IpAddr>>().map(|ip| *lock(ip)), Some("10.0.0.7".parse().unwrap()));
        let bytes = arr(vec![V::Int(192), V::Int(168), V::Int(0), V::Int(1)]);
        assert!(ok(static_call("InetAddress", "getByAddress", &[bytes])).native::<Mutex<std::net::IpAddr>>().is_some());
    }

    #[test]
    fn an_identity_map_keeps_two_equal_lists_apart() {
        let map = ok(construct("IdentityHashMap", &[]));
        let (one, two) = (list(vec![s("a")]), list(vec![s("a")]));
        ok(method(&map, "put", &[one.clone(), s("first")]));
        ok(method(&map, "put", &[two.clone(), s("second")]));
        assert_eq!(ok(method(&map, "size", &[])).int(), 2);
        assert_eq!(ok(method(&map, "get", &[one])).text(), "first");
        assert_eq!(ok(method(&map, "get", &[two])).text(), "second");
        assert!(ok(method(&map, "get", &[list(vec![s("a")])])).is_null());
    }

    #[test]
    fn a_manifest_reads_its_main_attributes_and_long_lines() {
        let m = read_manifest(b"Manifest-Version: 1.0\r\nWorldEdit-Version: 7.4\r\n .6\r\nFAWE-Plugin-Jar-Type: mojang\r\n\r\nName: a/b\r\nX: y\r\n");
        let main = native(JAttributes(m.main.clone()));
        assert_eq!(ok(method(&main, "getValue", &[s("worldedit-version")])).text(), "7.4.6");
        assert_eq!(ok(method(&main, "getValue", &[s("FAWE-Plugin-Jar-Type")])).text(), "mojang");
        assert_eq!(m.sections.len(), 1);
    }

    #[test]
    fn hex_format_writes_and_reads_bytes() {
        let hex = ok(static_call("HexFormat", "of", &[]));
        let bytes = arr(vec![V::Int(-1), V::Int(10)]);
        assert_eq!(ok(method(&hex, "formatHex", &[bytes])).text(), "ff0a");
        let back = ok(method(&hex, "parseHex", &[s("ff0a")])).items().unwrap_or_else(|e| panic!("threw {}", e.text()));
        assert_eq!(back.iter().map(V::int).collect::<Vec<_>>(), [-1, 10]);
    }
}
