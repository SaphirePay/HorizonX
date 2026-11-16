//! Caffeine's caches and Configurate's trees: two libraries plugins shade rather than a part of Java.
use super::*;
use serde_json::{Map, Value};
use std::path::PathBuf;

// ---- Caffeine: entries stay, which is what an unbounded cache does ----

pub struct JCacheBuilder {
    ttl: Mutex<Option<u64>>,
}

pub struct JCache {
    map: V,
    loader: Option<V>,
    ttl: Option<u64>,
    stamps: Mutex<BTreeMap<String, (V, std::time::Instant)>>,
}

/// `cache.asMap()`, and a set made from it: both still expire.
pub struct JCacheMap(V);
pub struct JCacheSet(V);

fn span_ms(a: &[V]) -> Option<u64> {
    match a {
        [duration] => duration.native::<crate::javaio::JDuration>().map(|d| d.0.max(0) as u64),
        [amount, unit] => call(unit, "toMillis", std::slice::from_ref(amount)).ok().map(|ms| ms.long().max(0) as u64),
        _ => None,
    }
}

impl JCache {
    fn purge(&self) {
        let Some(ttl) = self.ttl else { return };
        let mut stamps = lock(&self.stamps);
        let old: Vec<String> = stamps.iter().filter(|(_, (_, at))| at.elapsed().as_millis() as u64 >= ttl).map(|(k, _)| k.clone()).collect();
        for key in old {
            if let Some((value, _)) = stamps.remove(&key) {
                let _ = call(&self.map, "remove", &[value]);
            }
        }
    }
    fn stamp(&self, key: &V) {
        if self.ttl.is_some() {
            lock(&self.stamps).insert(key.key(), (key.clone(), std::time::Instant::now()));
        }
    }
}

fn cache_method(cache: &JCache, this: &V, name: &str, a: &[V]) -> Option<R> {
    cache.purge();
    let get = |key: &V| call(&cache.map, "get", std::slice::from_ref(key));
    let put = |key: &V, value: &V| {
        cache.stamp(key);
        call(&cache.map, "put", &[key.clone(), value.clone()])
    };
    Some(match (name, a.len()) {
        // Guava's LoadingCache: `getUnchecked(key)` / `apply(key)` load as `get(key)` does.
        ("get", 1 | 2) | ("getUnchecked" | "apply", 1) => {
            let found = match get(&a[0]) {
                Ok(found) => found,
                Err(error) => return Some(Err(error)),
            };
            if !found.is_null() {
                return Some(Ok(found));
            }
            let made = match a.get(1).or(cache.loader.as_ref()) {
                Some(loader) => call(loader, "$invoke", &a[..1]),
                None => Ok(V::Null),
            };
            made.and_then(|value| {
                if !value.is_null() {
                    put(&a[0], &value)?;
                }
                Ok(value)
            })
        }
        ("getIfPresent", 1) => get(&a[0]),
        ("put", 2) => put(&a[0], &a[1]).map(|_| V::Null),
        ("invalidate" | "refresh", 1) => call(&cache.map, "remove", a).map(|_| V::Null),
        ("invalidateAll", 0) => call(&cache.map, "clear", &[]).map(|_| V::Null),
        ("asMap", 0) => Ok(native(JCacheMap(this.clone()))),
        ("estimatedSize", 0) => call(&cache.map, "size", &[]),
        ("cleanUp", 0) => Ok(V::Null),
        _ => return None,
    })
}

fn cache_of(value: &V) -> Option<&JCache> {
    value.native::<JCache>()
}

fn cache_view(holder: &V, name: &str, a: &[V]) -> Option<R> {
    let cache = cache_of(holder)?;
    cache.purge();
    if name == "put" || name == "putIfAbsent" {
        cache.stamp(&a[0]);
    }
    Some(call(&cache.map, name, a))
}

fn cache_set(holder: &V, name: &str, a: &[V]) -> Option<R> {
    let cache = cache_of(holder)?;
    cache.purge();
    Some(match (name, a.len()) {
        ("add", 1) => match call(&cache.map, "containsKey", a) {
            Ok(seen) if seen.truth() => Ok(V::Bool(false)),
            Ok(_) => {
                cache.stamp(&a[0]);
                call(&cache.map, "put", &[a[0].clone(), V::Bool(true)]).map(|_| V::Bool(true))
            }
            Err(error) => Err(error),
        },
        ("contains", 1) => call(&cache.map, "containsKey", a),
        ("remove", 1) => call(&cache.map, "remove", a).map(|old| V::Bool(!old.is_null())),
        ("size" | "isEmpty" | "clear", 0) => call(&cache.map, name, a),
        _ => call(&call(&cache.map, "keySet", &[]).ok()?, name, a),
    })
}

// ---- Configurate 3: a node is a place in a tree, which needn't exist yet ----

#[derive(Clone)]
pub struct JNode {
    root: Arc<Mutex<Value>>,
    path: Vec<Value>,
}

pub struct JNodeLoader(pub PathBuf);

fn node(root: Arc<Mutex<Value>>, path: Vec<Value>) -> V {
    native(JNode { root, path })
}

fn step<'a>(value: &'a Value, key: &Value) -> Option<&'a Value> {
    match key {
        Value::Number(n) => value.get(n.as_u64()? as usize),
        Value::String(k) => value.get(k.as_str()),
        _ => None,
    }
}

impl JNode {
    fn read(&self) -> Value {
        let root = lock(&self.root);
        self.path.iter().try_fold(&*root, |value, key| step(value, key)).cloned().unwrap_or(Value::Null)
    }
    fn write(&self, new: Value) {
        let mut root = lock(&self.root);
        let Some((last, parents)) = self.path.split_last() else {
            *root = new;
            return;
        };
        let mut here = &mut *root;
        for key in parents {
            here = match key {
                Value::Number(n) => {
                    let at = n.as_u64().unwrap_or(0) as usize;
                    if !here.is_array() {
                        *here = Value::Array(Vec::new());
                    }
                    let list = here.as_array_mut().expect("an array");
                    while list.len() <= at {
                        list.push(Value::Null);
                    }
                    &mut list[at]
                }
                other => {
                    if !here.is_object() {
                        *here = Value::Object(Map::new());
                    }
                    here.as_object_mut().expect("an object").entry(other.as_str().unwrap_or("").to_owned()).or_insert(Value::Null)
                }
            };
        }
        match last {
            Value::Number(n) => {
                let at = n.as_u64().unwrap_or(0) as usize;
                if !here.is_array() {
                    *here = Value::Array(Vec::new());
                }
                let list = here.as_array_mut().expect("an array");
                if new.is_null() {
                    if at < list.len() {
                        list.remove(at);
                    }
                } else {
                    while list.len() <= at {
                        list.push(Value::Null);
                    }
                    list[at] = new;
                }
            }
            other => {
                let key = other.as_str().unwrap_or("").to_owned();
                if new.is_null() {
                    if let Some(map) = here.as_object_mut() {
                        map.remove(&key);
                    }
                } else {
                    if !here.is_object() {
                        *here = Value::Object(Map::new());
                    }
                    here.as_object_mut().expect("an object").insert(key, new);
                }
            }
        }
    }
    fn child(&self, key: Value) -> Self {
        let mut path = self.path.clone();
        path.push(key);
        Self { root: self.root.clone(), path }
    }
}

fn key_of(value: &V) -> Value {
    match value {
        V::Int(n) => Value::from(*n),
        V::Long(n) => Value::from(*n),
        other => Value::String(other.text()),
    }
}

fn json_of(value: &V) -> Value {
    match value.native::<JNode>() {
        Some(other) => other.read(),
        None => crate::javaio::to_json(value),
    }
}

fn key_value(key: &Value) -> V {
    crate::javaio::from_json(key)
}

fn node_method(n: &JNode, this: &V, name: &str, a: &[V]) -> Option<R> {
    let value = n.read();
    let or = |found: Option<V>| found.unwrap_or_else(|| a.first().cloned().unwrap_or(V::Null));
    Some(Ok(match (name, a.len()) {
        ("getNode", _) | ("node", _) => {
            let keys: Vec<V> = match a {
                [one] if matches!(one, V::Arr(_)) => one.items().unwrap_or_default(),
                _ => a.to_vec(),
            };
            let mut at = n.clone();
            for key in &keys {
                at = at.child(key_of(key));
            }
            native(at)
        }
        ("setValue" | "set", 1) => {
            n.write(json_of(&a[0]));
            this.clone()
        }
        ("setValue" | "set", 2) => {
            n.write(json_of(&a[1]));
            this.clone()
        }
        ("mergeValuesFrom", 1) => {
            if value.is_null() {
                n.write(json_of(&a[0]));
            }
            this.clone()
        }
        ("getValue", 1) if matches!(a[0], V::Func(_)) => return Some(call(&a[0], "$invoke", &[crate::javaio::from_json(&value)])),
        ("getValue" | "raw", 0 | 1) => or((!value.is_null()).then(|| crate::javaio::from_json(&value))),
        ("getString" | "string", 0 | 1) => or(match &value {
            Value::Null | Value::Object(_) | Value::Array(_) => None,
            Value::String(text) => Some(s(text)),
            other => Some(s(&other.to_string())),
        }),
        ("getBoolean", 0) => V::Bool(value.as_bool().unwrap_or(false)),
        ("getBoolean", 1) => value.as_bool().map_or_else(|| a[0].clone(), V::Bool),
        ("getInt", 0) => V::Int(value.as_i64().unwrap_or(0) as i32),
        ("getInt", 1) => value.as_i64().map_or_else(|| a[0].clone(), |v| V::Int(v as i32)),
        ("getLong", 0) => V::Long(value.as_i64().unwrap_or(0)),
        ("getLong", 1) => value.as_i64().map_or_else(|| a[0].clone(), V::Long),
        ("getDouble", 0) => V::Double(value.as_f64().unwrap_or(0.0)),
        ("getDouble", 1) => value.as_f64().map_or_else(|| a[0].clone(), V::Double),
        ("getChildrenMap" | "childrenMap", 0) => {
            let mut out = JMap::new();
            for key in value.as_object().into_iter().flatten().map(|(k, _)| k) {
                let k = s(key);
                out.insert(k.key(), (k, native(n.child(Value::String(key.clone())))));
            }
            V::Map(Arc::new(Mutex::new(out)))
        }
        ("getChildrenList" | "childrenList", 0) => list((0..value.as_array().map_or(0, Vec::len)).map(|i| native(n.child(Value::from(i)))).collect()),
        ("getList", 1 | 2) => {
            let items = value.as_array().cloned().unwrap_or_default();
            let mapped: Result<Vec<V>, V> = items.iter().map(|item| if matches!(a[0], V::Func(_)) { call(&a[0], "$invoke", &[crate::javaio::from_json(item)]) } else { Ok(crate::javaio::from_json(item)) }).collect();
            match mapped {
                Ok(mapped) => list(mapped),
                Err(error) => return Some(Err(error)),
            }
        }
        ("appendListNode", 0) => native(n.child(Value::from(value.as_array().map_or(0, Vec::len)))),
        ("removeChild", 1) => {
            let had = !n.child(key_of(&a[0])).read().is_null();
            n.child(key_of(&a[0])).write(Value::Null);
            V::Bool(had)
        }
        ("getKey" | "key", 0) => n.path.last().map_or(V::Null, key_value),
        ("getParent" | "parent", 0) => n.path.split_last().map_or(V::Null, |(_, parent)| node(n.root.clone(), parent.to_vec())),
        ("getPath" | "path", 0) => list(n.path.iter().map(key_value).collect()),
        ("getRoot", 0) => node(n.root.clone(), Vec::new()),
        ("isVirtual" | "virtual", 0) => V::Bool(value.is_null()),
        ("isEmpty" | "empty", 0) => V::Bool(value.is_null() || value.as_object().is_some_and(Map::is_empty) || value.as_array().is_some_and(Vec::is_empty)),
        ("isList", 0) => V::Bool(value.is_array()),
        ("isMap", 0) => V::Bool(value.is_object()),
        ("hasChild", _) => V::Bool(!n.child(key_of(&a[0])).read().is_null()),
        ("toString", 0) => s(&value.to_string()),
        _ => return None,
    }))
}

fn loader_method(loader: &JNodeLoader, name: &str, a: &[V]) -> Option<R> {
    Some(match name {
        "load" => {
            let value = std::fs::read_to_string(&loader.0).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Object(Map::new()));
            Ok(node(Arc::new(Mutex::new(value)), Vec::new()))
        }
        "save" => {
            if let Some(parent) = loader.0.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let text = serde_json::to_string_pretty(&json_of(&a[0])).unwrap_or_default();
            std::fs::write(&loader.0, text).map(|()| V::Null).map_err(|e| exc("IOException", e.to_string()))
        }
        "createEmptyNode" | "createNode" => Ok(node(Arc::new(Mutex::new(Value::Object(Map::new()))), Vec::new())),
        _ => return None,
    })
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    (class == "JNodeLoader").then(|| Ok(native(JNodeLoader(crate::javaio::path_of(&a[0]).unwrap_or_default()))))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(builder) = this.native::<JCacheBuilder>() {
        return Some(Ok(match name {
            "build" | "buildAsync" => native(JCache { map: V::Map(Arc::new(Mutex::new(JMap::new()))), loader: a.first().cloned(), ttl: *lock(&builder.ttl), stamps: Mutex::new(BTreeMap::new()) }),
            "expireAfterWrite" | "expireAfterAccess" => {
                *lock(&builder.ttl) = span_ms(a);
                this.clone()
            }
            _ => this.clone(),
        }));
    }
    if let Some(cache) = this.native::<JCache>() {
        return cache_method(cache, this, name, a);
    }
    if let Some(view) = this.native::<JCacheMap>() {
        return cache_view(&view.0, name, a);
    }
    if let Some(set) = this.native::<JCacheSet>() {
        return cache_set(&set.0, name, a);
    }
    if let Some(n) = this.native::<JNode>() {
        return node_method(n, this, name, a);
    }
    if let Some(loader) = this.native::<JNodeLoader>() {
        return loader_method(loader, name, a);
    }
    None
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    match (class, name) {
        ("Caffeine", "newBuilder") => Some(Ok(native(JCacheBuilder { ttl: Mutex::new(None) }))),
        // Guava's `CacheLoader.from(function)`: the function is the loader, called with the key.
        ("CacheLoader", "from") if a.len() == 1 => Some(Ok(a[0].clone())),
        ("Collections", "newSetFromMap") if a[0].native::<JCacheMap>().is_some() => Some(Ok(native(JCacheSet(a[0].native::<JCacheMap>()?.0.clone())))),
        ("ConfigurationNode", "root") => {
            let root = node(Arc::new(Mutex::new(Value::Object(Map::new()))), Vec::new());
            if let Some(setup) = a.first() {
                if let Err(error) = call(setup, "$invoke", std::slice::from_ref(&root)) {
                    return Some(Err(error));
                }
            }
            Some(Ok(root))
        }
        _ => None,
    }
}
