//! Player meta (`[meta]`, `/dm meta`, `has meta`, `%deluxemenus_meta_..%`): Java keeps it in each player's
//! PersistentDataContainer under `deluxemenus:<key>`; here it is one map in memory, written to `meta.json` in the
//! data folder by a background thread a few seconds after it changes (and on disable).

use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};

/// uuid -> key -> (type, value as text).
type Store = HashMap<String, HashMap<String, (String, String)>>;

static STORE: Mutex<Option<Store>> = Mutex::new(None);
static DIRTY: AtomicBool = AtomicBool::new(false);
static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static SAVER: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static STOP: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

pub const TYPES: [&str; 5] = ["DOUBLE", "INTEGER", "LONG", "STRING", "BOOLEAN"];

fn lock() -> std::sync::MutexGuard<'static, Option<Store>> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Read the file (the caller's thread) and start the saver.
pub fn start(path: PathBuf) {
    let mut store = Store::new();
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(Value::Object(players)) = serde_json::from_str::<Value>(&text)
    {
        for (uuid, keys) in players {
            let entry = store.entry(uuid).or_default();
            for (k, v) in keys.as_object().into_iter().flatten() {
                if let (Some(t), Some(val)) = (v["type"].as_str(), v["value"].as_str()) {
                    entry.insert(k.clone(), (t.to_owned(), val.to_owned()));
                }
            }
        }
    }
    *lock() = Some(store);
    *PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    *STOP.0.lock().unwrap_or_else(|e| e.into_inner()) = false;
    let handle = std::thread::Builder::new()
        .name("deluxemenus-meta".into())
        .spawn(|| {
            loop {
                let stop = STOP.0.lock().unwrap_or_else(|e| e.into_inner());
                let (stop, _) = STOP.1.wait_timeout(stop, std::time::Duration::from_secs(5)).unwrap_or_else(|e| e.into_inner());
                let quit = *stop;
                drop(stop);
                save_if_dirty();
                if quit {
                    return;
                }
            }
        })
        .ok();
    *SAVER.lock().unwrap_or_else(|e| e.into_inner()) = handle;
}

fn save_if_dirty() {
    if !DIRTY.swap(false, Ordering::AcqRel) {
        return;
    }
    let doc = {
        let held = lock();
        let Some(store) = held.as_ref() else { return };
        let mut out = Map::new();
        for (uuid, keys) in store {
            let m: Map<String, Value> = keys.iter().map(|(k, (t, v))| (k.clone(), json!({ "type": t, "value": v }))).collect();
            if !m.is_empty() {
                out.insert(uuid.clone(), Value::Object(m));
            }
        }
        Value::Object(out)
    };
    if let Some(path) = PATH.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(&doc).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// Stop the saver after one last write.
pub fn stop() {
    *STOP.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
    STOP.1.notify_all();
    if let Some(h) = SAVER.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = h.join();
    }
    save_if_dirty();
}

/// `getKey`: a valid namespaced-key path (`[a-z0-9._-]`).
pub fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-' | '/'))
}

fn norm(kind: &str) -> Option<&'static str> {
    TYPES.iter().find(|t| t.eq_ignore_ascii_case(kind)).copied()
}

/// The value of `key` stored as `kind`.
pub fn get(uuid: &str, key: &str, kind: &str) -> Option<String> {
    let kind = norm(kind)?;
    let held = lock();
    let (t, v) = held.as_ref()?.get(uuid)?.get(&key.to_ascii_lowercase())?;
    (t == kind || (kind == "BOOLEAN" && t == "STRING") || (kind == "STRING" && t == "BOOLEAN")).then(|| v.clone())
}

pub fn has(uuid: &str, key: &str, kind: Option<&str>) -> bool {
    match kind {
        Some(k) => get(uuid, key, k).is_some(),
        None => lock().as_ref().and_then(|s| s.get(uuid)).is_some_and(|m| m.contains_key(&key.to_ascii_lowercase())),
    }
}

/// Every key of a type, sorted.
pub fn list(uuid: &str, kind: &str) -> Vec<(String, String)> {
    let Some(kind) = norm(kind) else { return Vec::new() };
    let held = lock();
    let mut out: Vec<(String, String)> = held.as_ref().and_then(|s| s.get(uuid)).into_iter().flatten().filter(|(_, (t, _))| t == kind).map(|(k, (_, v))| (k.clone(), v.clone())).collect();
    out.sort();
    out
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Success,
    InvalidSyntax,
    InvalidType,
    NewValueIsDifferentType,
    ExistentValueIsDifferentType,
    ValueNotFound,
}

fn parse(kind: &str, value: Option<&str>) -> Option<String> {
    let v = value?.trim();
    Some(match kind {
        "DOUBLE" => v.parse::<f64>().ok()?.to_string(),
        "INTEGER" => v.parse::<i32>().ok()?.to_string(),
        "LONG" => v.parse::<i64>().ok()?.to_string(),
        "BOOLEAN" => match v.to_ascii_lowercase().as_str() {
            "true" | "false" => v.to_ascii_lowercase(),
            _ => return None,
        },
        _ => v.to_owned(),
    })
}

fn put(uuid: &str, key: &str, kind: &str, value: Option<String>) {
    let mut held = lock();
    let store = held.get_or_insert_with(Store::new);
    let entry = store.entry(uuid.to_owned()).or_default();
    match value {
        Some(v) => {
            entry.insert(key.to_owned(), (kind.to_owned(), v));
        }
        None => {
            entry.remove(key);
        }
    }
    DIRTY.store(true, Ordering::Release);
}

fn current(uuid: &str, key: &str) -> Option<(String, String)> {
    lock().as_ref()?.get(uuid)?.get(key).cloned()
}

/// `set|remove|add|subtract <key> <type> [value]`, `switch <key>`.
pub fn apply(uuid: &str, action: &str, key: &str, kind: Option<&str>, value: Option<&str>) -> Outcome {
    let key = key.to_ascii_lowercase();
    if !valid_key(&key) {
        return Outcome::InvalidSyntax;
    }
    let action = action.to_ascii_lowercase();
    if action == "switch" {
        return match current(uuid, &key) {
            None => {
                put(uuid, &key, "BOOLEAN", Some("true".into()));
                Outcome::Success
            }
            Some((t, v)) if t == "BOOLEAN" || (t == "STRING" && (v == "true" || v == "false")) => {
                put(uuid, &key, "BOOLEAN", Some(if v == "true" { "false".into() } else { "true".into() }));
                Outcome::Success
            }
            Some(_) => Outcome::ExistentValueIsDifferentType,
        };
    }
    let Some(kind) = kind.and_then(norm) else { return Outcome::InvalidSyntax };
    let parsed = parse(kind, value);
    match action.as_str() {
        "set" => match parsed {
            Some(v) => {
                if let Some((t, _)) = current(uuid, &key)
                    && t != kind
                {
                    return Outcome::ExistentValueIsDifferentType;
                }
                put(uuid, &key, kind, Some(v));
                Outcome::Success
            }
            None => Outcome::NewValueIsDifferentType,
        },
        "remove" => match current(uuid, &key) {
            Some((t, _)) if t == kind => {
                put(uuid, &key, kind, None);
                Outcome::Success
            }
            Some(_) => Outcome::ExistentValueIsDifferentType,
            None => Outcome::ValueNotFound,
        },
        "add" | "subtract" => {
            if !matches!(kind, "DOUBLE" | "INTEGER" | "LONG") {
                return Outcome::InvalidType;
            }
            let Some(delta) = parsed.and_then(|p| p.parse::<f64>().ok()) else { return Outcome::NewValueIsDifferentType };
            let delta = if action == "subtract" { -delta } else { delta };
            let old = match current(uuid, &key) {
                Some((t, v)) if t == kind => v.parse::<f64>().unwrap_or(0.0),
                Some(_) => return Outcome::ExistentValueIsDifferentType,
                None => 0.0,
            };
            let new = old + delta;
            let text = if kind == "DOUBLE" { new.to_string() } else { (new as i64).to_string() };
            put(uuid, &key, kind, Some(text));
            Outcome::Success
        }
        _ => Outcome::InvalidSyntax,
    }
}

/// `[meta] <action> <key> <type> <value>` as `parseAndExecuteMetaActionFromString`.
pub fn run_action(uuid: &str, line: &str) -> Outcome {
    let args: Vec<&str> = line.splitn(4, ' ').collect();
    if args.len() < 2 {
        return Outcome::InvalidSyntax;
    }
    if args[0].eq_ignore_ascii_case("switch") {
        return apply(uuid, "switch", args[1], None, None);
    }
    if args.len() < 3 {
        return Outcome::InvalidSyntax;
    }
    apply(uuid, args[0], args[1], Some(args[2]), args.get(3).copied())
}
