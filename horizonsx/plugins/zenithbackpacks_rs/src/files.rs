//! The Java plugin's files, same layout and format, so an existing server's backpacks and vaults open as they were:
//! `players/<uuid>.yml` (rows + contents), `chests/<id>.yml` (pages, owner, last-opened, contents) and
//! `placed-chests.yml`. Items are Bukkit's `ItemStack.serialize` maps (`==`, DataVersion, id, count, components as
//! SNBT). Writes go to one I/O thread (beside, then renamed); `flush` writes what's left at once (disable).

use horizon_bukkit::{ItemStack, data};
use serde_json::{Map, Value};
use serde_yaml::{Mapping, Value as Yaml};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};

static DATA_VERSION: Mutex<Option<i64>> = Mutex::new(None);

/// The game's data version, asked once (on the server thread) and kept.
pub fn remember_data_version() {
    let v = ItemStack::new(horizon_bukkit::Material::of("STONE"), 1).serialize().get("DataVersion").and_then(Value::as_i64);
    *DATA_VERSION.lock().unwrap_or_else(|e| e.into_inner()) = v;
}

/// `ItemStack.serialize` without a query per stack: Bukkit's map, components as SNBT.
pub fn item_yaml(stack: &ItemStack) -> Yaml {
    let mut m = Mapping::new();
    m.insert("==".into(), "org.bukkit.inventory.ItemStack".into());
    let version = DATA_VERSION.lock().unwrap_or_else(|e| e.into_inner()).unwrap_or(4903);
    m.insert("DataVersion".into(), version.into());
    m.insert("id".into(), stack.get_type().key().into());
    m.insert("count".into(), (stack.get_amount() as i64).into());
    let mut comps = Mapping::new();
    for (id, value) in stack.all_components() {
        comps.insert(id.as_str().into(), data::snbt(value).into());
    }
    if !comps.is_empty() {
        m.insert("components".into(), Yaml::Mapping(comps));
    }
    m.insert("schema_version".into(), 1.into());
    Yaml::Mapping(m)
}

fn to_json(v: &Yaml) -> Value {
    match v {
        Yaml::Null => Value::Null,
        Yaml::Bool(b) => Value::Bool(*b),
        Yaml::Number(n) => n.as_i64().map(Value::from).or_else(|| n.as_f64().map(Value::from)).unwrap_or(Value::Null),
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Sequence(s) => Value::Array(s.iter().map(to_json).collect()),
        Yaml::Mapping(m) => Value::Object(m.iter().filter_map(|(k, v)| Some((horizon_kit::yaml::key_str(k)?, to_json(v)))).collect::<Map<_, _>>()),
        Yaml::Tagged(t) => to_json(&t.value),
    }
}

/// A saved stack back (`ItemStack.deserialize`), None for air or junk.
pub fn item_of(v: &Yaml) -> Option<ItemStack> {
    let Value::Object(o) = to_json(v) else { return None };
    let map: BTreeMap<String, Value> = o.into_iter().collect();
    ItemStack::deserialize(&map).filter(|s| !s.is_empty() && !s.get_type().is_air())
}

/// `contents:` of a file into `len` slots.
fn contents_of(doc: &Yaml, len: usize) -> Vec<Option<ItemStack>> {
    let mut out = vec![None; len];
    if let Some(m) = horizon_kit::yaml::at(doc, "contents").and_then(Yaml::as_mapping) {
        for (k, v) in m {
            if let Some(i) = horizon_kit::yaml::key_str(k).and_then(|k| k.parse::<usize>().ok()).filter(|i| *i < len) {
                out[i] = item_of(v);
            }
        }
    }
    out
}

fn contents_yaml(items: &[Option<ItemStack>], upto: usize) -> Yaml {
    let mut m = Mapping::new();
    for (i, s) in items.iter().enumerate().take(upto) {
        if let Some(s) = s.as_ref().filter(|s| !s.is_empty()) {
            m.insert(i.to_string().into(), item_yaml(s));
        }
    }
    Yaml::Mapping(m)
}

fn read(path: &Path) -> Option<Yaml> {
    let text = std::fs::read_to_string(path).ok()?;
    horizon_kit::yaml::parse(&text).ok()
}

// ---- Backpacks ----

pub struct Backpack {
    pub rows: i32,
    pub items: Vec<Option<ItemStack>>,
}

pub fn backpack_path(folder: &Path, uuid: &str) -> PathBuf {
    folder.join("players").join(format!("{uuid}.yml"))
}

/// A player's backpack (`rows` clamped to 1..=max, a missing file = `starting` rows, empty).
pub fn load_backpack(folder: &Path, uuid: &str, starting: i32, max: i32) -> Backpack {
    let doc = read(&backpack_path(folder, uuid));
    let rows = doc.as_ref().map_or(starting as i64, |d| horizon_kit::yaml::int(d, "rows", starting as i64)).clamp(1, max as i64) as i32;
    let items = doc.as_ref().map_or_else(|| vec![None; super::conf::BACKPACK_CAP], |d| contents_of(d, super::conf::BACKPACK_CAP));
    Backpack { rows, items }
}

pub fn backpack_text(b: &Backpack, max_rows: i32) -> String {
    let mut m = Mapping::new();
    m.insert("rows".into(), (b.rows as i64).into());
    m.insert("contents".into(), contents_yaml(&b.items, (max_rows as usize * 9).min(super::conf::BACKPACK_CAP)));
    serde_yaml::to_string(&Yaml::Mapping(m)).unwrap_or_default()
}

// ---- Vault chests ----

pub struct Chest {
    pub owner: String,
    pub pages: i32,
    pub last_opened: i64,
    pub items: Vec<Option<ItemStack>>,
}

pub fn chest_dir(folder: &Path) -> PathBuf {
    folder.join("chests")
}

pub fn chest_path(folder: &Path, id: &str) -> PathBuf {
    chest_dir(folder).join(format!("{id}.yml"))
}

pub fn load_chest(folder: &Path, id: &str, owner_fallback: &str) -> Chest {
    let doc = read(&chest_path(folder, id));
    let at = |k: &str, d: i64| doc.as_ref().map_or(d, |doc| horizon_kit::yaml::int(doc, k, d));
    Chest {
        owner: doc.as_ref().and_then(|d| horizon_kit::yaml::string(d, "owner")).filter(|o| o.len() == 36).unwrap_or_else(|| owner_fallback.to_owned()),
        pages: super::conf::clamp_pages(at("pages", 1) as i32),
        last_opened: at("last-opened", horizon_kit::time::now_ms() as i64),
        items: doc.as_ref().map_or_else(|| vec![None; super::conf::CHEST_CAPACITY], |d| contents_of(d, super::conf::CHEST_CAPACITY)),
    }
}

pub fn chest_text(c: &Chest) -> String {
    let mut m = Mapping::new();
    m.insert("pages".into(), (c.pages as i64).into());
    m.insert("owner".into(), c.owner.as_str().into());
    m.insert("last-opened".into(), c.last_opened.into());
    m.insert("contents".into(), contents_yaml(&c.items, super::conf::CHEST_CAPACITY));
    serde_yaml::to_string(&Yaml::Mapping(m)).unwrap_or_default()
}

/// `last-opened` of a chest file without reading its items (`chestpurge`).
pub fn last_opened(path: &Path) -> i64 {
    read(path).map_or(0, |d| horizon_kit::yaml::int(&d, "last-opened", 0))
}

// ---- Where chests are placed ----

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Spot {
    pub world: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Spot {
    pub fn of(b: &horizon_bukkit::Block) -> Spot {
        Spot { world: b.get_world().get_name().to_owned(), x: b.get_x(), y: b.get_y(), z: b.get_z() }
    }
}

pub fn load_placed(folder: &Path) -> HashMap<String, Spot> {
    let mut out = HashMap::new();
    let Some(doc) = read(&folder.join("placed-chests.yml")) else { return out };
    for id in horizon_kit::yaml::keys(&doc, "placed") {
        let get = |k: &str| horizon_kit::yaml::int(&doc, &format!("placed.{id}.{k}"), 0) as i32;
        if let Some(world) = horizon_kit::yaml::string(&doc, &format!("placed.{id}.world")).filter(|w| !w.is_empty()) {
            out.insert(id.clone(), Spot { world, x: get("x"), y: get("y"), z: get("z") });
        }
    }
    out
}

pub fn placed_text(placed: &HashMap<String, Spot>) -> String {
    let mut all = Mapping::new();
    let mut ids: Vec<&String> = placed.keys().collect();
    ids.sort();
    for id in ids {
        let s = &placed[id];
        let mut m = Mapping::new();
        m.insert("world".into(), s.world.as_str().into());
        m.insert("x".into(), (s.x as i64).into());
        m.insert("y".into(), (s.y as i64).into());
        m.insert("z".into(), (s.z as i64).into());
        all.insert(id.as_str().into(), Yaml::Mapping(m));
    }
    let mut root = Mapping::new();
    root.insert("placed".into(), Yaml::Mapping(all));
    serde_yaml::to_string(&Yaml::Mapping(root)).unwrap_or_default()
}

// ---- The writer ----

struct Writer {
    pending: Mutex<HashMap<PathBuf, String>>,
    wake: Condvar,
}

fn writer() -> &'static Writer {
    static W: OnceLock<Writer> = OnceLock::new();
    W.get_or_init(|| {
        std::thread::Builder::new()
            .name("zenithbackpacks-io".into())
            .spawn(|| {
                loop {
                    let jobs = {
                        let w = writer();
                        let mut p = w.pending.lock().unwrap_or_else(|e| e.into_inner());
                        while p.is_empty() {
                            p = w.wake.wait(p).unwrap_or_else(|e| e.into_inner());
                        }
                        std::mem::take(&mut *p)
                    };
                    for (path, text) in jobs {
                        write_now(&path, &text);
                    }
                }
            })
            .ok();
        Writer { pending: Mutex::new(HashMap::new()), wake: Condvar::new() }
    })
}

fn write_now(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("yml.tmp");
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::write(path, text);
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Written on the I/O thread (a newer text for the same file replaces one not yet written).
pub fn save(path: PathBuf, text: String) {
    let w = writer();
    w.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(path, text);
    w.wake.notify_one();
}

/// Everything not yet written, written now (disable).
pub fn flush() {
    let jobs = std::mem::take(&mut *writer().pending.lock().unwrap_or_else(|e| e.into_inner()));
    for (path, text) in jobs {
        write_now(&path, &text);
    }
}

#[allow(dead_code)]
pub fn as_json(v: &Yaml) -> Value {
    to_json(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placed_round_trip() {
        let mut p = HashMap::new();
        p.insert("b2dfc112-1b94-444a-a1c7-ddd7b596dd09".to_owned(), Spot { world: "world".into(), x: 176, y: 65, z: -145 });
        let dir = std::env::temp_dir().join("zbp_test_placed");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("placed-chests.yml"), placed_text(&p)).unwrap();
        assert_eq!(load_placed(&dir), p);

    }
}
