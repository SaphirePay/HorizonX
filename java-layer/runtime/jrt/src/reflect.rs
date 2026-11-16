//! `getDeclaredMethod` on one of the plugin's own classes, and `Method.invoke`: what dynamic event
//! registration (`registerEvent` with an executor that invokes a handler by name) does.
use crate::{R, V, exc, native, s};

/// A method of a plugin class, by name and parameter count.
pub struct JMethod {
    class: &'static str,
    name: String,
    count: usize,
    /// From the converter's method table when the method is in it: its parameter types and modifiers.
    params: Option<&'static [&'static str]>,
    flags: u8,
    ret: Option<&'static str>,
}

/// Each plugin class's declared methods as the converter wrote them: (class, method, 1 public | 8 static, parameter
/// types). `getMethods()` / `getDeclaredMethods()` answer from it, so annotation scanners (WorldEdit's event bus,
/// command frameworks) find the plugin's methods.
type MethodRow = (&'static str, &'static str, u8, &'static [&'static str], &'static str);
static METHOD_TABLE: std::sync::OnceLock<&'static [MethodRow]> = std::sync::OnceLock::new();

pub fn set_method_table(rows: &'static [MethodRow]) {
    let _ = METHOD_TABLE.set(rows);
}

/// The parameter types of every overload of `class.name` with `count` parameters, as the converter wrote them.
/// Indexed once (a lookup runs on every call that passes a plugin object: a scan of the whole table timed out
/// FAWE's enable).
pub fn overload_params(class: &str, name: &str, count: usize) -> &'static [&'static [&'static str]] {
    type Index = std::collections::HashMap<&'static str, std::collections::HashMap<&'static str, Vec<Vec<&'static [&'static str]>>>>;
    static INDEX: std::sync::OnceLock<Index> = std::sync::OnceLock::new();
    let Some(rows) = METHOD_TABLE.get() else { return &[] };
    let index = INDEX.get_or_init(|| {
        let mut index = Index::new();
        for (c, n, _, p, _) in rows.iter() {
            let by_count = index.entry(*c).or_default().entry(*n).or_default();
            if by_count.len() <= p.len() {
                by_count.resize(p.len() + 1, Vec::new());
            }
            by_count[p.len()].push(*p);
        }
        index
    });
    index.get(class).and_then(|m| m.get(name)).and_then(|by_count| by_count.get(count)).map_or(&[], Vec::as_slice)
}

fn table_row(class: &str, name: &str, count: usize) -> Option<&'static MethodRow> {
    METHOD_TABLE.get()?.iter().find(|(c, n, _, p, _)| *c == class && *n == name && p.len() == count)
}

/// `clazz.getDeclaredMethods()` (`declared`) or `clazz.getMethods()`: the class's own, or the public ones along its
/// parents and interfaces too, as Java gives them.
pub fn methods_of(class: &'static str, declared: bool) -> V {
    let Some(rows) = METHOD_TABLE.get() else { return crate::arr(Vec::new()) };
    let mut chain: Vec<&'static str> = vec![class];
    if !declared {
        let mut at = 0;
        while at < chain.len() {
            if let Some(k) = crate::plugin_class(chain[at]) {
                if let Some(parent) = k.parent {
                    chain.push(parent().name);
                }
                for interface in k.interfaces {
                    let name = interface().name;
                    if !chain.contains(&name) {
                        chain.push(name);
                    }
                }
            }
            at += 1;
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for owner in chain {
        for (c, n, flags, params, ret) in rows.iter().filter(|row| row.0 == owner) {
            if !declared && flags & 1 == 0 {
                continue;
            }
            if !seen.insert((*n, params.len())) {
                continue;
            }
            out.push(native(JMethod { class: c, name: (*n).to_owned(), count: params.len(), params: Some(params), flags: *flags, ret: Some(ret) }));
        }
    }
    crate::arr(out)
}

/// `Foo.class.getDeclaredMethod(name, types...)`: found when the class has made an object here and has
/// a method of that name taking that many arguments; `NoSuchMethodException` otherwise, as Java says.
/// The game API's methods the plugin may look up by reflection, as `Class.method` of the SDK's tags:
/// `OfflinePlayer.class.getMethod("getPlayerProfile")` finds one Paper has, as a feature probe expects.
static API_METHODS: std::sync::OnceLock<&'static [&'static str]> = std::sync::OnceLock::new();

pub fn set_api_methods(methods: &'static [&'static str]) {
    let _ = API_METHODS.set(methods);
}

pub fn find_method(class: &'static str, a: &[V]) -> R {
    let name = a.first().map(V::text).unwrap_or_default();
    let count = match a {
        [_, V::Arr(types)] => crate::lock(types).len(),
        _ => a.len().saturating_sub(1),
    };
    let api = crate::class_named(class).is_none() && API_METHODS.get().is_some_and(|all| all.contains(&format!("{class}.{name}").as_str()));
    let found = api || crate::class_named(class).is_some_and(|k| crate::method_of(k, &name, &vec![V::Null; count]).is_some());
    let row = table_row(class, &name, count);
    if found { Ok(native(JMethod { class, name, count, params: row.map(|r| r.3), flags: row.map_or(9, |r| r.2), ret: row.map(|r| r.4) })) } else { Err(exc("NoSuchMethodException", name)) }
}

/// An annotation of the plugin's own type on one of its classes, `@PerkInfo(name = "x")`: each element a method.
pub struct JAnnotation {
    kind: &'static str,
    values: Vec<(&'static str, V)>,
}

pub fn annotation(kind: &'static str, values: Vec<(&'static str, V)>) -> V {
    native(JAnnotation { kind, values })
}

static ANNOTATIONS: std::sync::OnceLock<fn(&str, &str) -> Option<R>> = std::sync::OnceLock::new();

/// What the converted plugin answers `Foo.class.getAnnotation(Bar.class)` with: its class annotations, by
/// the class's and the annotation's simple names.
/// The annotation type a value is.
pub fn annotation_kind(found: &JAnnotation) -> &str {
    found.kind
}

pub fn set_annotations(find: fn(&str, &str) -> Option<R>) {
    let _ = ANNOTATIONS.set(find);
}

static PARAM_ANNOTATIONS: std::sync::OnceLock<&'static [(&'static str, usize, &'static str)]> = std::sync::OnceLock::new();

/// Which parameters of which methods (`Owner.method`, index) carry which annotations: the converter's index.
pub fn set_param_annotations(rows: &'static [(&'static str, usize, &'static str)]) {
    let _ = PARAM_ANNOTATIONS.set(rows);
}

/// `method.getParameterAnnotations()`: per parameter, the annotations on it.
fn parameter_annotations(class: &str, method: &str, count: usize) -> R {
    let holder = format!("{}.{method}", class.rsplit(['.', '$']).next().unwrap_or(class));
    let rows = PARAM_ANNOTATIONS.get().copied().unwrap_or_default();
    let width = rows.iter().filter(|(h, _, _)| *h == holder).map(|(_, i, _)| i + 1).max().unwrap_or(0).max(count);
    let mut out = Vec::with_capacity(width);
    for index in 0..width {
        let mut here = Vec::new();
        for (_, _, kind) in rows.iter().filter(|(h, i, _)| *h == holder && *i == index) {
            let found = class_annotation(&format!("{holder}#{index}"), &V::Class(kind))?;
            if !found.is_null() {
                here.push(found);
            }
        }
        out.push(crate::arr(here));
    }
    Ok(crate::arr(out))
}

type FieldTypes = fn(&str) -> Option<&'static [(&'static str, &'static str)]>;
static FIELD_TYPES: std::sync::OnceLock<FieldTypes> = std::sync::OnceLock::new();

/// The plugin's table of instance field types (`jport` writes it), for Gson.
pub fn set_field_types(find: FieldTypes) {
    let _ = FIELD_TYPES.set(find);
}

/// The field named `field` in `class` or a parent of it: its name as the table keeps it, and its type.
fn field_type(class: &'static crate::Class, field: &str) -> Option<(&'static str, &'static str)> {
    let find = FIELD_TYPES.get()?;
    let mut at = Some(class);
    while let Some(k) = at {
        if let Some(found) = find(k.name).and_then(|rows| rows.iter().find(|(n, _)| *n == field)) {
            return Some(*found);
        }
        at = k.parent.map(|p| p());
    }
    None
}

/// The type arguments of `List<A>` / `Map<K, V>`, split at the top level.
fn type_arguments(written: &str) -> Vec<&str> {
    let Some(open) = written.find('<') else { return Vec::new() };
    let inner = &written[open + 1..written.len().saturating_sub(1)];
    let (mut depth, mut start, mut out) = (0, 0, Vec::new());
    for (i, c) in inner.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(inner[start..].trim());
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

/// A JSON value as Gson makes it into a field of type `written`: numbers by the field's kind, a plugin class
/// as one of its objects, lists and maps of them, a plugin enum by its constant.
fn gson_value(value: &serde_json::Value, written: &str) -> R {
    let base = written.split('<').next().unwrap_or(written).trim_end_matches("[]").rsplit('.').next().unwrap_or(written);
    if value.is_null() {
        return Ok(V::Null);
    }
    match base {
        "int" | "Integer" | "short" | "Short" | "byte" | "Byte" => return Ok(V::Int(value.as_f64().map_or(0, |n| n as i32))),
        "long" | "Long" => return Ok(V::Long(value.as_f64().map_or(0, |n| n as i64))),
        "double" | "Double" | "float" | "Float" => return Ok(V::Double(value.as_f64().unwrap_or(0.0))),
        "boolean" | "Boolean" => return Ok(V::Bool(value.as_bool().unwrap_or(false))),
        "String" | "CharSequence" => return Ok(crate::s(&value.as_str().map_or_else(|| value.to_string(), str::to_owned))),
        "char" | "Character" => return Ok(V::Char(value.as_str().and_then(|t| t.chars().next()).unwrap_or('\0'))),
        _ => {}
    }
    let arguments = type_arguments(written);
    if written.ends_with("[]") {
        let element = &written[..written.len() - 2];
        let items = value.as_array().map(|items| items.iter().map(|v| gson_value(v, element)).collect::<Result<Vec<_>, _>>()).transpose()?.unwrap_or_default();
        return Ok(crate::arr(items));
    }
    if matches!(base, "List" | "ArrayList" | "LinkedList" | "Collection" | "Iterable" | "Set" | "HashSet" | "LinkedHashSet" | "TreeSet" | "Queue" | "Deque") {
        let element = arguments.first().copied().unwrap_or("Object");
        let items = value.as_array().map(|items| items.iter().map(|v| gson_value(v, element)).collect::<Result<Vec<_>, _>>()).transpose()?.unwrap_or_default();
        return if base.contains("Set") { crate::new_builtin("HashSet", &[crate::list(items)]).unwrap_or_else(|| Ok(V::Null)) } else { Ok(crate::list(items)) };
    }
    if matches!(base, "Map" | "HashMap" | "LinkedHashMap" | "TreeMap" | "ConcurrentHashMap") {
        let element = arguments.get(1).copied().unwrap_or("Object");
        let key_kind = arguments.first().copied().unwrap_or("String");
        let mut map = crate::JMap::new();
        for (k, v) in value.as_object().into_iter().flatten() {
            let key = gson_value(&serde_json::Value::String(k.clone()), key_kind).or_else(|_| Ok::<V, V>(crate::s(k)))?;
            let key = match (key_kind, &key) {
                ("Integer" | "int", V::Int(0)) | ("Long" | "long", V::Long(0)) => k.parse::<i64>().map_or(key.clone(), |n| if key_kind.starts_with('L') || key_kind == "long" { V::Long(n) } else { V::Int(n as i32) }),
                _ => key,
            };
            map.insert(key.key(), (key, gson_value(v, element)?));
        }
        return Ok(V::Map(std::sync::Arc::new(std::sync::Mutex::new(map))));
    }
    if crate::plugin_class(base).is_some() {
        if let serde_json::Value::String(constant) = value {
            return crate::call_static(base, "valueOf", &[crate::s(constant)]).or_else(|_| Ok(crate::s(constant)));
        }
        return gson_bind(value, base);
    }
    Ok(crate::javaio::from_json(value))
}

/// `gson.fromJson(text, Settings.class)` for a class of the plugin: an object made as Gson makes one (its no
/// argument constructor when it has one, so defaults stay), each JSON member put in the field of its name.
pub fn gson_bind(value: &serde_json::Value, class: &str) -> R {
    let Some(own) = crate::plugin_class(class) else {
        return Ok(crate::javaio::from_json(value));
    };
    let serde_json::Value::Object(members) = value else {
        return gson_value(value, "Object");
    };
    let object = match crate::new_obj(own, &[], &[]) {
        Ok(made) => made,
        Err(_) => V::Obj(std::sync::Arc::new(crate::Object { class: own, fields: std::sync::Mutex::new(Default::default()) })),
    };
    for (name, member) in members {
        let field = crate::reflect::serialized_field(own, name).unwrap_or_else(|| name.clone());
        // A member the class doesn't declare: Gson skips it. `expires_at` is `expiresAt` too (Gson's snake_case
        // naming policy, which Tebex's API answers in).
        let Some((name, written)) = field_type(own, &field).or_else(|| field_type(own, &camel(&field))) else {
            continue;
        };
        object.set(name, gson_value(member, written)?)?;
    }
    Ok(object)
}

fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in name.chars() {
        if c == '_' {
            up = true;
        } else if up {
            out.push(c.to_ascii_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The field a JSON member names: its own name, or the field whose `@SerializedName` says it.
fn serialized_field(class: &'static crate::Class, member: &str) -> Option<String> {
    let find = FIELD_TYPES.get()?;
    let mut at = Some(class);
    while let Some(k) = at {
        for (name, _) in find(k.name).unwrap_or_default() {
            let annotation = ANNOTATIONS.get().and_then(|annotations| annotations(&format!("{}.{name}", k.name), "SerializedName")).and_then(Result::ok);
            if annotation.is_some_and(|a| crate::call(&a, "value", &[]).is_ok_and(|v| v.text() == member)) {
                return Some((*name).to_owned());
            }
        }
        at = k.parent.map(|p| p());
    }
    None
}

/// `getAnnotation(Bar.class)` of a class: the annotation, or null when it carries none (as Java).
pub fn class_annotation(class: &str, wanted: &V) -> R {
    let V::Class(kind) = wanted else { return Ok(V::Null) };
    annotation_of(class, kind)
}

/// One annotation of a holder (`Class`, `Class.method`, `Class.field`), made once: Java hands back the same object
/// each time, and WorldEdit's bindings compare `annotations[0] == binding`.
fn annotation_of(holder: &str, kind: &str) -> R {
    static MADE: std::sync::Mutex<Option<std::collections::HashMap<String, V>>> = std::sync::Mutex::new(None);
    let key = format!("{holder}|{kind}");
    if let Some(found) = crate::lock(&MADE).get_or_insert_default().get(&key) {
        return Ok(found.clone());
    }
    let made = ANNOTATIONS.get().and_then(|find| find(holder, kind)).unwrap_or(Ok(V::Null))?;
    crate::lock(&MADE).get_or_insert_default().insert(key, made.clone());
    Ok(made)
}

/// Each holder's annotation kinds as written, for `getAnnotations()`.
static ANNOTATION_KINDS: std::sync::OnceLock<&'static [(&'static str, &'static [&'static str])]> = std::sync::OnceLock::new();

pub fn set_annotation_kinds(rows: &'static [(&'static str, &'static [&'static str])]) {
    let _ = ANNOTATION_KINDS.set(rows);
}

/// `getAnnotations()` of a holder: each of its annotations, in the order written.
pub fn annotations_of(holder: &str) -> R {
    let kinds = ANNOTATION_KINDS.get().and_then(|rows| rows.iter().find(|(h, _)| *h == holder)).map_or(&[][..], |(_, k)| *k);
    let mut out = Vec::new();
    for kind in kinds {
        let found = annotation_of(holder, kind)?;
        if !found.is_null() {
            out.push(found);
        }
    }
    Ok(crate::arr(out))
}

fn annotation_method(found: &JAnnotation, name: &str, a: &[V]) -> Option<R> {
    if let Some((_, value)) = found.values.iter().find(|(key, _)| *key == name).filter(|_| a.is_empty()) {
        return Some(Ok(value.clone()));
    }
    Some(Ok(match name {
        "annotationType" => V::Class(found.kind),
        "toString" => s(&format!("@{}", found.kind)),
        "hashCode" => V::Int(found.kind.len() as i32),
        "equals" => V::Bool(a.first().and_then(|o| o.native::<JAnnotation>()).is_some_and(|o| o.kind == found.kind)),
        _ => return None,
    }))
}

/// `getGenericSuperclass()` of a plugin class: its parent with erased arguments, each `Object`. What Gson's
/// `new TypeToken<List<X>>() {}` reads; Gson here makes Java's maps and lists of any JSON anyway.
pub struct JParamType {
    raw: &'static str,
}

pub fn generic_superclass(class: &str) -> V {
    match crate::plugin_class(class).and_then(|k| k.parent) {
        Some(parent) => native(JParamType { raw: parent().name }),
        None => V::Class("Object"),
    }
}

fn param_type_method(t: &JParamType, name: &str) -> Option<R> {
    Some(Ok(match name {
        "getRawType" => V::Class(t.raw),
        "getActualTypeArguments" => crate::arr(vec![V::Class("Object")]),
        "getOwnerType" => V::Null,
        "getTypeName" | "toString" => s(&format!("{}<java.lang.Object>", t.raw)),
        "hashCode" => V::Int(t.raw.len() as i32),
        _ => return None,
    }))
}

pub fn is_param_type(value: &V) -> bool {
    value.native::<JParamType>().is_some()
}

/// A constructor of a plugin class: `Foo.class.getConstructor(types...)`, then `newInstance(args...)`.
pub struct JCtor {
    class: &'static crate::Class,
    count: Option<usize>,
}

/// `getConstructor(types...)`/`getDeclaredConstructor(...)` of a plugin class; `NoSuchMethodException` for
/// any other class, as Java says when it isn't there.
pub fn find_constructor(class: &str, a: &[V]) -> R {
    let count = match a {
        [V::Arr(types)] => Some(crate::lock(types).len()),
        rest => Some(rest.len()),
    };
    match crate::plugin_class(class) {
        Some(k) => Ok(native(JCtor { class: k, count })),
        None => Err(exc("NoSuchMethodException", format!("{class}.<init>"))),
    }
}

/// `Foo.class.newInstance()` and `Constructor.newInstance(args)`: a new object, its constructor run.
pub fn instantiate(class: &'static crate::Class, args: &[V]) -> R {
    crate::new_obj(class, args, &[])
}

fn ctor_method(c: &JCtor, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match name {
        "newInstance" => {
            let args = match a {
                [V::Arr(items)] if c.count != Some(1) || crate::lock(items).len() == 1 => crate::lock(items).clone(),
                rest => rest.to_vec(),
            };
            return Some(instantiate(c.class, &args));
        }
        "setAccessible" | "trySetAccessible" => V::Null,
        "canAccess" | "isAccessible" => V::Bool(true),
        "getParameterCount" => V::Int(c.count.unwrap_or(0) as i32),
        "getDeclaringClass" => V::Class(c.class.name),
        "getName" | "toString" => s(c.class.name),
        "getModifiers" => V::Int(1),
        "isAnnotationPresent" | "isVarArgs" | "isSynthetic" => V::Bool(false),
        "getAnnotation" | "getDeclaredAnnotation" => V::Null,
        "getAnnotations" | "getDeclaredAnnotations" | "getParameterTypes" | "getExceptionTypes" => crate::arr(Vec::new()),
        _ => return None,
    }))
}

/// A field of a plugin class, as `Class.getField` hands it out: static ones live in the class's statics, the others
/// in each object.
pub struct JPluginField {
    owner: &'static crate::Class,
    name: &'static str,
    kind: &'static str,
    statics: Option<&'static crate::Statics>,
}

/// `getField(name)` of a plugin class (or a parent of it). Finding the class runs its static initialiser, as the
/// first use of a class does in Java, so a static field holds its value when it's read.
pub fn plugin_field(class: &str, name: &str) -> Option<V> {
    let mut at = Some(crate::plugin_class(class)?);
    // The class as asked (its full name, when its short one is shared), then each parent by its own name.
    let mut asked = Some(class);
    while let Some(k) = at {
        let statics = asked.take().and_then(crate::statics_of).or_else(|| crate::statics_of(k.name));
        if let Some(statics) = statics {
            if crate::lock(&statics.0).contains_key(name) {
                let kind = crate::class_of(&statics.get(name));
                return Some(crate::native(JPluginField { owner: k, name: crate::intern(name), kind, statics: Some(statics) }));
            }
        }
        if let Some((field, kind)) = FIELD_TYPES.get().and_then(|find| find(k.name)).and_then(|rows| rows.iter().find(|(n, _)| *n == name)) {
            return Some(crate::native(JPluginField { owner: k, name: field, kind, statics: None }));
        }
        at = k.parent.map(|p| p());
    }
    None
}

/// The fields of a plugin class: its instance fields in the order written, then its static ones; with
/// `inherited`, its parents' after (`getFields`), else its own (`getDeclaredFields`).
pub fn plugin_fields(class: &str, inherited: bool) -> Option<V> {
    let mut at = Some(crate::plugin_class(class)?);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while let Some(k) = at {
        for (name, kind) in FIELD_TYPES.get().and_then(|find| find(k.name)).unwrap_or_default() {
            if seen.insert(*name) {
                out.push(crate::native(JPluginField { owner: k, name, kind, statics: None }));
            }
        }
        if let Some(statics) = crate::statics_of(k.name) {
            let names: Vec<(&'static str, V)> = crate::lock(&statics.0).iter().map(|(n, v)| (*n, v.clone())).collect();
            for (name, value) in names {
                // jport's own entries (`$...`) are no field of the class.
                if !name.starts_with('$') && seen.insert(name) {
                    out.push(crate::native(JPluginField { owner: k, name, kind: crate::class_of(&value), statics: Some(statics) }));
                }
            }
        }
        if !inherited {
            break;
        }
        at = k.parent.map(|p| p());
    }
    Some(crate::arr(out))
}

/// A field's declared type as written (`List<String>`, `QUEUE`, `int[]`): the class, without its arguments.
fn erased(kind: &str) -> &'static str {
    let raw = kind.split('<').next().unwrap_or(kind).trim();
    let suffix = if kind.trim_end().ends_with("[]") && !raw.ends_with("[]") { "[]" } else { "" };
    // The class by its own name, as a class literal is written here (`Settings.QUEUE` and `QUEUE.class` are one
    // class; `Config.ConfigBlock` is `ConfigBlock`), so `field.getType() == Foo.class` holds as in Java.
    let raw = raw.rsplit(['.', '$']).next().unwrap_or(raw);
    crate::intern(&format!("{raw}{suffix}"))
}

/// `field.getGenericType()` of a field written with type arguments (`ConfigBlock<Limit>`): the raw class and the
/// arguments as classes.
pub struct JGenericType {
    raw: &'static str,
    args: Vec<&'static str>,
}

fn generic_type_method(t: &JGenericType, name: &str) -> Option<R> {
    Some(Ok(match name {
        "getRawType" => V::Class(t.raw),
        "getActualTypeArguments" => crate::arr(t.args.iter().map(|a| V::Class(a)).collect()),
        "getOwnerType" => V::Null,
        "getTypeName" | "toString" => s(&format!("{}<{}>", crate::java_class_name(t.raw), t.args.iter().map(|a| crate::java_class_name(a)).collect::<Vec<_>>().join(", "))),
        "hashCode" => V::Int(t.raw.len() as i32),
        _ => return None,
    }))
}

fn generic_of(kind: &'static str) -> V {
    let Some((raw, rest)) = kind.split_once('<') else { return V::Class(erased(kind)) };
    let inner = rest.strip_suffix('>').unwrap_or(rest);
    let written = format!("x<{inner}>");
    let args = type_arguments(&written).into_iter().map(erased).collect();
    crate::native(JGenericType { raw: erased(raw), args })
}

fn plugin_field_method(f: &JPluginField, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("get" | "getInt" | "getLong" | "getDouble" | "getFloat" | "getBoolean" | "getShort" | "getByte" | "getChar", 1) => match f.statics {
            Some(statics) => statics.get(f.name),
            None if a[0].is_null() => return Some(Err(crate::exc("NullPointerException", format!("Cannot get field {} of null", f.name)))),
            None => return Some(a[0].get(f.name)),
        },
        ("set" | "setInt" | "setLong" | "setDouble" | "setFloat" | "setBoolean" | "setShort" | "setByte" | "setChar", 2) => match f.statics {
            Some(statics) => {
                statics.set(f.name, a[1].clone());
                V::Null
            }
            None if a[0].is_null() => return Some(Err(crate::exc("NullPointerException", format!("Cannot set field {} of null", f.name)))),
            None => return Some(a[0].set(f.name, a[1].clone()).map(|_| V::Null)),
        },
        ("getName", 0) => crate::s(f.name),
        ("getType", 0) => V::Class(erased(f.kind)),
        ("getGenericType", 0) => generic_of(f.kind),
        // public (1), plus static (8) for a class field.
        ("getModifiers", 0) => V::Int(if f.statics.is_some() { 0x9 } else { 0x1 }),
        ("getDeclaringClass", 0) => V::Class(f.owner.name),
        ("setAccessible" | "trySetAccessible", _) => V::Bool(true),
        ("canAccess" | "isAccessible", _) => V::Bool(true),
        ("isSynthetic" | "isEnumConstant", 0) => V::Bool(false),
        ("getAnnotation" | "getDeclaredAnnotation", 1) => return Some(class_annotation(&format!("{}.{}", f.owner.name, f.name), &a[0])),
        ("isAnnotationPresent", 1) => return Some(class_annotation(&format!("{}.{}", f.owner.name, f.name), &a[0]).map(|found| V::Bool(!found.is_null()))),
        ("getAnnotations" | "getDeclaredAnnotations", 0) => crate::arr(Vec::new()),
        ("toString", 0) => crate::s(&format!("public {} {}.{}", f.kind, crate::java_class_name(f.owner.name), f.name)),
        _ => return None,
    }))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(f) = this.native::<JPluginField>() {
        return plugin_field_method(f, name, a);
    }
    if let Some(t) = this.native::<JGenericType>() {
        return generic_type_method(t, name);
    }
    if let Some(c) = this.native::<JCtor>() {
        return ctor_method(c, name, a);
    }
    if let Some(found) = this.native::<JAnnotation>() {
        return annotation_method(found, name, a);
    }
    if let Some(t) = this.native::<JParamType>() {
        return param_type_method(t, name);
    }
    let m = this.native::<JMethod>()?;
    Some(Ok(match name {
        "invoke" => {
            let target = a.first().cloned().unwrap_or(V::Null);
            let args = match &a[1.min(a.len())..] {
                [V::Arr(items)] if m.count != 1 || crate::lock(items).len() == 1 => crate::lock(items).clone(),
                rest => rest.to_vec(),
            };
            return Some(if target.is_null() { crate::call_static(m.class, &m.name, &args) } else { crate::call(&target, &m.name, &args) });
        }
        "getName" => s(&m.name),
        "getParameterCount" => V::Int(m.count as i32),
        "getDeclaringClass" => V::Class(m.class),
        "setAccessible" | "trySetAccessible" => V::Null,
        "canAccess" | "isAccessible" => V::Bool(true),
        // The plugin's annotations on the method (`Owner.method`, kept by the converter): WorldEdit's commands
        // read `@Command`, `@CommandPermissions` from each command method.
        "getAnnotation" | "getDeclaredAnnotation" if !a.is_empty() => return Some(class_annotation(&format!("{}.{}", m.class.rsplit(['.', '$']).next().unwrap_or(m.class), m.name), &a[0])),
        "getParameterAnnotations" => return Some(parameter_annotations(m.class, &m.name, m.count)),
        "isAnnotationPresent" if !a.is_empty() => return Some(class_annotation(&format!("{}.{}", m.class.rsplit(['.', '$']).next().unwrap_or(m.class), m.name), &a[0]).map(|found| V::Bool(!found.is_null()))),
        "isAnnotationPresent" | "isVarArgs" | "isDefault" | "isSynthetic" | "isBridge" => V::Bool(false),
        "getAnnotation" | "getDeclaredAnnotation" => V::Null,
        "getParameterTypes" | "getGenericParameterTypes" => crate::arr(m.params.unwrap_or_default().iter().map(|t| V::Class(t)).collect()),
        "getReturnType" | "getGenericReturnType" => V::Class(m.ret.unwrap_or("Object")),
        "getAnnotations" | "getDeclaredAnnotations" => return Some(annotations_of(&format!("{}.{}", m.class.rsplit(['.', '$']).next().unwrap_or(m.class), m.name))),
        "getExceptionTypes" => crate::arr(Vec::new()),
        // Public 1, static 8, from the converter's table (public static when the method isn't in it, as before).
        "getModifiers" => V::Int(i32::from(m.flags)),
        "hashCode" => V::Int(m.name.len() as i32),
        "equals" => V::Bool(a.first().and_then(|o| o.native::<JMethod>()).is_some_and(|o| o.name == m.name && o.class == m.class && o.count == m.count)),
        "toString" => s(&format!("{}.{}", m.class, m.name)),
        _ => return None,
    }))
}
