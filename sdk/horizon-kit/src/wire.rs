//! The packets sent most (text changes, icon turns, removals) made here as the bytes the client reads, so a tick's
//! hundreds of them cost no JSON: the engine hands the bytes to each player as they are. Packet ids come from the
//! engine once (`packet_ids`); an engine without it gets the JSON packets as before.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub struct Ids {
    pub entity_data: i32,
    pub teleport: i32,
    pub remove: i32,
    /// AddEntity, with the entity types' network ids (none: spawns go as JSON).
    pub add_entity: Option<i32>,
    pub entity_types: HashMap<String, i32>,
    pub items: HashMap<String, i32>,
    pub glint: Option<i32>,
}

fn registry(name: &str) -> HashMap<String, i32> {
    let answer = horizon_bukkit::horizon_mod::query(json!({ "op": "core_registry", "registry": name }));
    answer.as_array().into_iter().flatten().enumerate().filter_map(|(i, n)| Some((n.as_str()?.to_owned(), i as i32))).collect()
}

/// The engine's ids for the packets made here, `None` when it can't take them. Asked first on the main thread
/// (`on_enable`): the registries are a game query.
pub fn ids() -> Option<&'static Ids> {
    static IDS: OnceLock<Option<Ids>> = OnceLock::new();
    IDS.get_or_init(|| {
        if std::env::var("KIT_JSON_PACKETS").is_ok() {
            return None;
        }
        let answer = horizon_bukkit::horizon_mod::query(json!({ "op": "packet_ids", "names": ["SetEntityData", "TeleportEntity", "RemoveEntities", "AddEntity"] }));
        let id = |n: &str| answer[n].as_i64().map(|i| i as i32);
        let entity_types = registry("minecraft:entity_type");
        let items = registry("minecraft:item");
        let raw_spawns = std::env::var("KIT_JSON_SPAWNS").is_err() && entity_types.contains_key("minecraft:armor_stand") && items.contains_key("minecraft:stone");
        Some(Ids {
            entity_data: id("SetEntityData")?,
            teleport: id("TeleportEntity")?,
            remove: id("RemoveEntities")?,
            add_entity: id("AddEntity").filter(|_| raw_spawns),
            glint: registry("minecraft:data_component_type").get("minecraft:enchantment_glint_override").copied(),
            entity_types,
            items,
        })
    })
    .as_ref()
}

/// AddEntity: `None` when the engine has to make it (an unknown type, or an engine without the ids).
pub fn add_entity(entity: i32, uuid: u128, kind: &str, pos: [f64; 3], yaw: i8) -> Option<Vec<u8>> {
    let ids = ids()?;
    let packet = ids.add_entity?;
    let kind = *ids.entity_types.get(kind)?;
    let mut w = W::default();
    w.varint(entity);
    w.0.extend_from_slice(&uuid.to_be_bytes());
    w.varint(kind);
    for p in pos {
        w.f64(p);
    }
    // Velocity (LpVec3): none.
    w.u8(0);
    w.u8(0);
    w.u8(yaw as u8);
    w.u8(yaw as u8);
    w.varint(0);
    Some(framed(packet, &w))
}

/// A stack as the client reads it, when it is a plain item (or one with only the enchantment glint): the rest
/// (heads' profiles, damage, model data) the engine makes.
fn item_stack(w: &mut W, v: &Value) -> bool {
    let Some(ids) = ids() else { return false };
    if v.is_null() {
        w.varint(0);
        return true;
    }
    let Some(item) = v["item"].as_str().and_then(|n| ids.items.get(n)) else { return false };
    let components = v["components"].as_object();
    let glint = match components {
        None => false,
        Some(c) if c.is_empty() => false,
        Some(c) if c.len() == 1 && c.get("minecraft:enchantment_glint_override").and_then(Value::as_str) == Some("true") => true,
        Some(_) => return false,
    };
    w.varint(v["count"].as_i64().unwrap_or(1) as i32);
    w.varint(*item);
    if glint {
        let Some(kind) = ids.glint else { return false };
        w.varint(1);
        w.varint(0);
        w.varint(kind);
        w.bool(true);
    } else {
        w.varint(0);
        w.varint(0);
    }
    true
}

#[derive(Default)]
pub struct W(pub Vec<u8>);

impl W {
    pub fn varint(&mut self, v: i32) {
        let mut v = v as u32;
        loop {
            if v & !0x7F == 0 {
                self.0.push(v as u8);
                return;
            }
            self.0.push((v & 0x7F | 0x80) as u8);
            v >>= 7;
        }
    }
    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub fn bool(&mut self, v: bool) {
        self.0.push(u8::from(v));
    }
    pub fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    pub fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    /// An NBT string: u16 length, then Java's modified UTF-8.
    fn nbt_string(&mut self, s: &str) {
        let mut bytes = Vec::with_capacity(s.len());
        for c in s.chars() {
            let n = c as u32;
            if n != 0 && n < 0x80 {
                bytes.push(n as u8);
            } else if n < 0x800 {
                bytes.push((0xC0 | (n >> 6)) as u8);
                bytes.push((0x80 | (n & 0x3F)) as u8);
            } else if n < 0x10000 {
                bytes.extend_from_slice(&[(0xE0 | (n >> 12)) as u8, (0x80 | ((n >> 6) & 0x3F)) as u8, (0x80 | (n & 0x3F)) as u8]);
            } else {
                // Two UTF-16 halves, each written as three bytes.
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units).iter() {
                    let u = u32::from(*u);
                    bytes.extend_from_slice(&[(0xE0 | (u >> 12)) as u8, (0x80 | ((u >> 6) & 0x3F)) as u8, (0x80 | (u & 0x3F)) as u8]);
                }
            }
        }
        let len = bytes.len().min(65535);
        self.0.extend_from_slice(&(len as u16).to_be_bytes());
        self.0.extend_from_slice(&bytes[..len]);
    }
    /// A JSON value's NBT payload (no type byte): objects as compounds, text, `true` as 1b, numbers as ints or
    /// doubles, lists of one kind.
    fn nbt_payload(&mut self, v: &Value) {
        match v {
            Value::Object(map) => {
                for (k, item) in map {
                    self.u8(nbt_type(item));
                    self.nbt_string(k);
                    self.nbt_payload(item);
                }
                self.u8(0);
            }
            Value::String(s) => self.nbt_string(s),
            Value::Bool(b) => self.bool(*b),
            Value::Number(n) => match n.as_i64() {
                Some(i) => self.i32(i as i32),
                None => self.0.extend_from_slice(&n.as_f64().unwrap_or(0.0).to_be_bytes()),
            },
            Value::Array(items) => {
                self.u8(items.first().map_or(0, nbt_type));
                self.i32(items.len() as i32);
                for item in items {
                    self.nbt_payload(item);
                }
            }
            Value::Null => {}
        }
    }
    /// A network NBT tag (type byte, no name, payload).
    pub fn nbt(&mut self, v: &Value) {
        self.u8(nbt_type(v));
        self.nbt_payload(v);
    }
}

fn nbt_type(v: &Value) -> u8 {
    match v {
        Value::Object(_) => 10,
        Value::String(_) => 8,
        Value::Bool(_) => 1,
        Value::Number(n) if n.as_i64().is_some() => 3,
        Value::Number(_) => 6,
        Value::Array(_) => 9,
        Value::Null => 0,
    }
}

/// A legacy `§` text's component as network NBT, made once per text (animations come round again).
pub fn text_nbt(legacy: &str) -> Arc<Vec<u8>> {
    static CACHE: Mutex<Option<HashMap<String, Arc<Vec<u8>>>>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    if let Some(hit) = map.get(legacy) {
        return hit.clone();
    }
    let mut w = W::default();
    w.nbt(&crate::text::component(legacy));
    let bytes = Arc::new(w.0);
    if map.len() > 8192 {
        map.clear();
    }
    map.insert(legacy.to_owned(), bytes.clone());
    bytes
}

/// One entity data value as the client reads it, `false` for kinds made by the engine (items, block states).
fn data_value(w: &mut W, kind: i32, v: &Value) -> bool {
    match kind {
        crate::packets::BYTE => w.u8(v.as_i64().unwrap_or(0) as u8),
        crate::packets::INT => w.varint(v.as_i64().unwrap_or(0) as i32),
        crate::packets::FLOAT => w.f32(v.as_f64().unwrap_or(0.0) as f32),
        crate::packets::BOOL => w.bool(v.as_bool().unwrap_or(false)),
        crate::packets::COMPONENT => w.nbt(v),
        crate::packets::OPT_COMPONENT => {
            w.bool(!v.is_null());
            if !v.is_null() {
                w.nbt(v);
            }
        }
        crate::packets::ITEM => return item_stack(w, v),
        crate::packets::VECTOR3 => {
            for i in 0..3 {
                w.f32(v[i].as_f64().unwrap_or(0.0) as f32);
            }
        }
        // x, y, z, w (displays' left and right rotations).
        crate::packets::QUATERNION => {
            for i in 0..4 {
                w.f32(v[i].as_f64().unwrap_or(if i == 3 { 1.0 } else { 0.0 }) as f32);
            }
        }
        _ => return false,
    }
    true
}

/// A whole packet: its length, id and body.
fn framed(id: i32, body: &W) -> Vec<u8> {
    let mut inner = W::default();
    inner.varint(id);
    inner.0.extend_from_slice(&body.0);
    let mut out = W::default();
    out.varint(inner.0.len() as i32);
    out.0.extend_from_slice(&inner.0);
    out.0
}

/// SetEntityData from `(slot, serializer, value)` entries, `None` when one is the engine's to make.
pub fn entity_data(entity: i32, values: &[Value]) -> Option<Vec<u8>> {
    let ids = ids()?;
    let mut w = W::default();
    w.varint(entity);
    for e in values {
        w.u8(e[0].as_u64()? as u8);
        let kind = e[1].as_i64()? as i32;
        w.varint(kind);
        if !data_value(&mut w, kind, &e[2]) {
            return None;
        }
    }
    w.u8(0xFF);
    Some(framed(ids.entity_data, &w))
}

/// A text line's name (and whether it shows): the packet sent most.
pub fn name(entity: i32, legacy: &str, visible: bool) -> Option<Vec<u8>> {
    let ids = ids()?;
    let mut w = W::default();
    w.varint(entity);
    w.u8(crate::packets::CUSTOM_NAME);
    w.varint(crate::packets::OPT_COMPONENT);
    w.bool(true);
    w.0.extend_from_slice(&text_nbt(legacy));
    w.u8(crate::packets::NAME_VISIBLE);
    w.varint(crate::packets::BOOL);
    w.bool(visible);
    w.u8(0xFF);
    Some(framed(ids.entity_data, &w))
}

pub fn teleport(entity: i32, pos: [f64; 3], yaw: f32) -> Option<Vec<u8>> {
    let ids = ids()?;
    let mut w = W::default();
    w.varint(entity);
    for p in pos {
        w.f64(p);
    }
    for _ in 0..3 {
        w.f64(0.0);
    }
    w.f32(yaw);
    w.f32(0.0);
    w.i32(0);
    w.bool(false);
    Some(framed(ids.teleport, &w))
}

pub fn remove(entities: &[i32]) -> Option<Vec<u8>> {
    let ids = ids()?;
    let mut w = W::default();
    w.varint(entities.len() as i32);
    for e in entities {
        w.varint(*e);
    }
    Some(framed(ids.remove, &w))
}

pub fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[usize::from(b >> 4)]);
        out.push(HEX[usize::from(b & 15)]);
    }
    String::from_utf8(out).unwrap_or_default()
}

impl W {
    /// A protocol string: VarInt length, then UTF-8.
    pub fn str(&mut self, s: &str) {
        self.varint(s.len() as i32);
        self.0.extend_from_slice(s.as_bytes());
    }
}

/// Any packet's id by its Mojang name without `Clientbound`/`Packet` (`SetScore`, `SetObjective`, ...), asked of
/// the engine once (`packet_ids`). `None` on an engine that can't name it.
pub fn packet_id(name: &str) -> Option<i32> {
    static IDS: Mutex<Option<HashMap<String, Option<i32>>>> = Mutex::new(None);
    if let Some(hit) = IDS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(name).copied()) {
        return hit;
    }
    let answer = horizon_bukkit::horizon_mod::query(json!({ "op": "packet_ids", "names": [name] }));
    let id = answer[name].as_i64().map(|i| i as i32);
    IDS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(name.to_owned(), id);
    id
}

/// A score's number format (1.20.3+): none (the objective's), blank (no number), or fixed text.
pub enum NumberFormat<'a> {
    Default,
    Blank,
    Fixed(&'a Value),
}

/// SetScore with what JSON packets can't carry: a display name for the line and a number format (sidebars
/// with no red numbers, right-hand text, fancy tab and below-name values). `display` and the fixed text are
/// text components (JSON). `None` when the engine can't name the packet.
pub fn set_score(owner: &str, objective: &str, score: i32, display: Option<&Value>, format: NumberFormat) -> Option<Vec<u8>> {
    let id = packet_id("SetScore")?;
    let mut w = W::default();
    w.str(owner);
    w.str(objective);
    w.varint(score);
    match display {
        Some(d) => {
            w.bool(true);
            w.nbt(d);
        }
        None => w.bool(false),
    }
    match format {
        NumberFormat::Default => w.bool(false),
        NumberFormat::Blank => {
            w.bool(true);
            w.varint(0);
        }
        NumberFormat::Fixed(text) => {
            w.bool(true);
            w.varint(2);
            w.nbt(text);
        }
    }
    Some(framed(id, &w))
}

/// Boss bars as packets (`BossEvent`): only the players sent them see them, nothing is saved in the world, the
/// title is a full text component, and an update is one small packet (the SDK's `BossBar` runs `/bossbar`
/// commands). `colour`: 0 pink, 1 blue, 2 red, 3 green, 4 yellow, 5 purple, 6 white. `overlay`: 0 progress,
/// 1-4 notched 6/10/12/20.
pub mod boss {
    use super::{W, framed, packet_id};
    use serde_json::Value;

    fn start(id: u128, action: i32) -> Option<(i32, W)> {
        let packet = packet_id("BossEvent")?;
        let mut w = W::default();
        w.0.extend_from_slice(&id.to_be_bytes());
        w.varint(action);
        Some((packet, w))
    }
    pub fn add(id: u128, title: &Value, progress: f32, colour: i32, overlay: i32) -> Option<Vec<u8>> {
        let (p, mut w) = start(id, 0)?;
        w.nbt(title);
        w.f32(progress.clamp(0.0, 1.0));
        w.varint(colour);
        w.varint(overlay);
        w.u8(0);
        Some(framed(p, &w))
    }
    pub fn remove(id: u128) -> Option<Vec<u8>> {
        let (p, w) = start(id, 1)?;
        Some(framed(p, &w))
    }
    pub fn progress(id: u128, progress: f32) -> Option<Vec<u8>> {
        let (p, mut w) = start(id, 2)?;
        w.f32(progress.clamp(0.0, 1.0));
        Some(framed(p, &w))
    }
    pub fn title(id: u128, title: &Value) -> Option<Vec<u8>> {
        let (p, mut w) = start(id, 3)?;
        w.nbt(title);
        Some(framed(p, &w))
    }
    pub fn style(id: u128, colour: i32, overlay: i32) -> Option<Vec<u8>> {
        let (p, mut w) = start(id, 4)?;
        w.varint(colour);
        w.varint(overlay);
        Some(framed(p, &w))
    }
    /// Bukkit `BarColor` / Adventure colour names -> the packet's number.
    pub fn colour_of(name: &str) -> i32 {
        match name.trim().to_ascii_uppercase().as_str() {
            "PINK" => 0,
            "BLUE" => 1,
            "RED" => 2,
            "GREEN" => 3,
            "YELLOW" => 4,
            "PURPLE" => 5,
            "WHITE" => 6,
            _ => 1,
        }
    }
    /// `PROGRESS`/`SOLID`, `NOTCHED_6`/`SEGMENTED_6`, ... -> the packet's number.
    pub fn overlay_of(name: &str) -> i32 {
        let n = name.trim().to_ascii_uppercase();
        match n.trim_start_matches("NOTCHED_").trim_start_matches("SEGMENTED_") {
            "6" => 1,
            "10" => 2,
            "12" => 3,
            "20" => 4,
            _ => 0,
        }
    }
    /// A fresh bar id.
    pub fn new_id() -> u128 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(1);
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        (u128::from(t) << 64) | u128::from(N.fetch_add(1, Ordering::Relaxed)) | (4u128 << 76)
    }
}

/// SetObjective: `method` 0 add, 1 remove, 2 change. `display` (text JSON) and `render` (0 integer, 1 hearts)
/// for 0 and 2. `None` when the engine can't name the packet.
pub fn set_objective(name: &str, method: i8, display: &Value, render: i32) -> Option<Vec<u8>> {
    let id = packet_id("SetObjective")?;
    let mut w = W::default();
    w.str(name);
    w.u8(method as u8);
    if method == 0 || method == 2 {
        w.nbt(display);
        w.varint(render);
        w.bool(false);
    }
    Some(framed(id, &w))
}

/// SetDisplayObjective: slot 0 list, 1 sidebar, 2 below name ("" clears).
pub fn display_objective(slot: i32, objective: &str) -> Option<Vec<u8>> {
    let id = packet_id("SetDisplayObjective")?;
    let mut w = W::default();
    w.varint(slot);
    w.str(objective);
    Some(framed(id, &w))
}

/// ResetScore: one objective's score of `owner`, or all of them (`None`).
pub fn reset_score(owner: &str, objective: Option<&str>) -> Option<Vec<u8>> {
    let id = packet_id("ResetScore")?;
    let mut w = W::default();
    w.str(owner);
    match objective {
        Some(o) => {
            w.bool(true);
            w.str(o);
        }
        None => w.bool(false),
    }
    Some(framed(id, &w))
}

/// TabList: the player list's header and footer (text JSON).
pub fn tab_list(header: &Value, footer: &Value) -> Option<Vec<u8>> {
    let id = packet_id("TabList")?;
    let mut w = W::default();
    w.nbt(header);
    w.nbt(footer);
    Some(framed(id, &w))
}
