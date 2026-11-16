//! The auction data: listed, expired and purchased items, the sales history, money waiting for its seller and
//! players' options. All of it lives in memory; a background thread writes `storage.json` (atomically, through a
//! temporary file) a couple of seconds after it changes, and once more when the plugin stops.

use horizon_bukkit::{ItemStack, Material};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Listing {
    pub id: u64,
    pub seller: String,
    pub seller_name: String,
    pub item: Value,
    pub price: f64,
    pub economy: String,
    pub created: i64,
    /// When it expires (ms); 0 = never.
    pub expire: i64,
    #[serde(default)]
    pub buyer: Option<String>,
    #[serde(default)]
    pub buyer_name: Option<String>,
    #[serde(skip)]
    pub categories: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sale {
    pub id: u64,
    pub seller: String,
    pub buyer: String,
    pub buyer_name: String,
    pub item: Value,
    pub price: f64,
    pub economy: String,
    pub date: i64,
    #[serde(default)]
    pub notified: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Data {
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub listed: Vec<Listing>,
    /// Owned by their seller.
    #[serde(default)]
    pub expired: Vec<Listing>,
    /// Owned by their buyer.
    #[serde(default)]
    pub purchased: Vec<Listing>,
    #[serde(default)]
    pub history: Vec<Sale>,
    /// uuid -> economy -> money.
    #[serde(default)]
    pub pending: HashMap<String, HashMap<String, f64>>,
    /// uuid -> option -> value.
    #[serde(default)]
    pub options: HashMap<String, HashMap<String, bool>>,
}

impl Data {
    pub fn next(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

static DATA: Mutex<Option<Data>> = Mutex::new(None);
static DIRTY: AtomicBool = AtomicBool::new(false);
static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static SAVER: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static STOP: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

/// The data, changed or read; changes are saved soon after.
pub fn with<R>(f: impl FnOnce(&mut Data) -> R) -> R {
    let mut held = DATA.lock().unwrap_or_else(|e| e.into_inner());
    f(held.get_or_insert_with(Data::default))
}

pub fn changed() {
    DIRTY.store(true, Ordering::Release);
}

pub fn read(f: impl FnOnce(&Data) -> Option<()>) {
    let held = DATA.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = held.as_ref() {
        f(d);
    }
}

/// Reads `storage.json` (caller's thread: a background one) and starts the saver.
pub fn load(path: PathBuf) -> Data {
    let data: Data = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    *PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    data
}

pub fn install(data: Data) {
    *DATA.lock().unwrap_or_else(|e| e.into_inner()) = Some(data);
}

pub fn start_saver() {
    *STOP.0.lock().unwrap_or_else(|e| e.into_inner()) = false;
    let handle = std::thread::Builder::new()
        .name("zauctionhouse-save".into())
        .spawn(|| {
            loop {
                let stop = STOP.0.lock().unwrap_or_else(|e| e.into_inner());
                let (stop, _) = STOP.1.wait_timeout(stop, std::time::Duration::from_secs(2)).unwrap_or_else(|e| e.into_inner());
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
    let snapshot = DATA.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let (Some(data), Some(path)) = (snapshot, PATH.lock().unwrap_or_else(|e| e.into_inner()).clone()) else { return };
    let Ok(text) = serde_json::to_string(&data) else { return };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

pub fn stop() {
    *STOP.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
    STOP.1.notify_all();
    if let Some(h) = SAVER.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = h.join();
    }
    save_if_dirty();
}

/// A stack as stored: the item's id, count and every component (its whole data), no game call needed.
pub fn item_to_json(stack: &ItemStack) -> Value {
    let components: Map<String, Value> = stack.all_components().map(|(k, v)| (k.clone(), v.clone())).collect();
    json!({ "id": stack.get_type().key(), "count": stack.get_amount(), "components": components })
}

pub fn item_from_json(v: &Value) -> Option<ItemStack> {
    let mut stack = ItemStack::new(Material::of(v["id"].as_str()?), v["count"].as_i64().unwrap_or(1) as i32);
    for (k, c) in v["components"].as_object().into_iter().flatten() {
        stack.set_component(k, c.clone());
    }
    Some(stack)
}

pub fn item_amount(v: &Value) -> i64 {
    v["count"].as_i64().unwrap_or(1)
}

pub fn item_material(v: &Value) -> String {
    v["id"].as_str().unwrap_or("minecraft:air").trim_start_matches("minecraft:").to_ascii_uppercase()
}
