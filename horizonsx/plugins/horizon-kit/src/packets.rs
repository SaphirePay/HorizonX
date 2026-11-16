//! Entities only their viewers' clients know about (holograms, markers, NPCs): the server sends each viewer the packets that
//! make, change and remove them, and nothing of them exists in the world. Every packet of a tick goes out in one
//! engine query (`packets_send`), each packet built once for all the players it's for.

use serde_json::{Value, json};
use std::cell::RefCell;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

/// Entity data serializers (the game's numbers).
pub const BYTE: i32 = 0;
pub const INT: i32 = 1;
pub const FLOAT: i32 = 3;
pub const COMPONENT: i32 = 5;
pub const OPT_COMPONENT: i32 = 6;
pub const ITEM: i32 = 7;
pub const BOOL: i32 = 8;
pub const BLOCK_STATE: i32 = 14;
pub const VECTOR3: i32 = 39;
pub const QUATERNION: i32 = 40;

/// Entity data slots.
pub const FLAGS: u8 = 0;
pub const CUSTOM_NAME: u8 = 2;
pub const NAME_VISIBLE: u8 = 3;
pub const SILENT: u8 = 4;
pub const NO_GRAVITY: u8 = 5;
pub const ARMOR_STAND_FLAGS: u8 = 15;
pub const ITEM_STACK: u8 = 8;

pub static DEBUG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A fresh entity id, far above the ones the world hands out.
pub fn next_id() -> i32 {
    static NEXT: AtomicI32 = AtomicI32::new(1_600_000_000);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn uuid() -> String {
    static N: AtomicU64 = AtomicU64::new(1);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    format!("{:016x}{:016x}", t ^ 0xD3C3_A1B2_0000_0000, n.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 0x4000)
}

enum Entry {
    Json(Value),
    /// Packets made here (length, id, body after each other) for these players.
    Raw(Vec<String>, Vec<u8>),
}

thread_local! {
    static BATCH: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

/// The engine takes players as 32 hex digits (no dashes).
fn players(to: &[String]) -> Vec<String> {
    to.iter().map(|u| if u.contains('-') { u.replace('-', "") } else { u.clone() }).collect()
}

/// A packet for these players, sent with the rest of the tick's.
pub fn queue(to: &[String], name: &str, fields: Value) {
    if to.is_empty() {
        return;
    }
    let to = players(to);
    BATCH.with(|b| b.borrow_mut().push(Entry::Json(json!({ "to": to, "name": name, "fields": fields }))));
}

/// A packet made here, in order with the rest; one after another for the same players go as one.
pub fn queue_raw(to: &[String], packet: Vec<u8>) {
    if to.is_empty() {
        return;
    }
    let to = players(to);
    BATCH.with(|b| {
        let mut b = b.borrow_mut();
        if let Some(Entry::Raw(last_to, bytes)) = b.last_mut()
            && *last_to == to
        {
            bytes.extend_from_slice(&packet);
            return;
        }
        b.push(Entry::Raw(to, packet));
    });
}

/// What the hologram thread made that needs the game (spawns, items) or a player the engine doesn't know there yet,
/// with everything after it (order kept): sent by the main thread's next tick.
static OUTBOX: std::sync::Mutex<Vec<Entry>> = std::sync::Mutex::new(Vec::new());

thread_local! {
    static WORKER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn set_worker() {
    WORKER.with(|w| w.set(true));
}

fn outbox() -> std::sync::MutexGuard<'static, Vec<Entry>> {
    OUTBOX.lock().unwrap_or_else(|e| e.into_inner())
}

fn to_json(entries: Vec<Entry>) -> Vec<Value> {
    entries
        .into_iter()
        .map(|e| match e {
            Entry::Json(v) => v,
            Entry::Raw(to, bytes) => json!({ "to": to, "raw": crate::wire::hex(&bytes) }),
        })
        .collect()
}

/// Everything queued, out in one query: from the hologram thread straight to the players' connections when it's
/// all bytes made here, else (and from the main thread) through the game, in order.
pub fn flush() {
    let entries = BATCH.with(|b| std::mem::take(&mut *b.borrow_mut()));
    let started = std::time::Instant::now();
    if WORKER.with(std::cell::Cell::get) {
        if entries.is_empty() {
            return;
        }
        let mut entries = entries;
        let mut out = outbox();
        if !out.is_empty() {
            out.extend(entries);
            return;
        }
        // The bytes before the first packet the game has to make go now; that one and all after it, in order,
        // with the main thread's next tick.
        let tail = match entries.iter().position(|e| matches!(e, Entry::Json(_))) {
            Some(0) => {
                out.extend(entries);
                return;
            }
            Some(i) => entries.split_off(i),
            None => Vec::new(),
        };
        drop(out);
        let raws: Vec<(Vec<String>, Vec<u8>)> = entries.into_iter().filter_map(|e| if let Entry::Raw(to, b) = e { Some((to, b)) } else { None }).collect();
        let packets: Vec<Value> = raws.iter().map(|(to, b)| json!({ "to": to, "raw": crate::wire::hex(b) })).collect();
        let answer = horizon_bukkit::horizon_mod::query(json!({ "op": "packets_free", "packets": packets }));
        let missing: Vec<String> = match answer["missing"].as_array() {
            Some(m) => m.iter().filter_map(|u| u.as_str().map(str::to_owned)).collect(),
            // An engine without it: everything through the game.
            None => raws.iter().flat_map(|(to, _)| to.iter().cloned()).collect(),
        };
        if !missing.is_empty() || !tail.is_empty() {
            let mut out = outbox();
            for (to, bytes) in raws {
                let to: Vec<String> = to.into_iter().filter(|u| missing.contains(u)).collect();
                if !to.is_empty() {
                    out.push(Entry::Raw(to, bytes));
                }
            }
            out.extend(tail);
        }
        crate::stats::count(answer["sent"].as_u64().unwrap_or(0) as usize);
        crate::stats::time("packets sent off the server thread", started.elapsed());
        return;
    }
    let mut all = std::mem::take(&mut *outbox());
    all.extend(entries);
    if all.is_empty() {
        return;
    }
    let n = all.len();
    let answer = horizon_bukkit::horizon_mod::query(json!({ "op": "packets_send", "packets": to_json(all) }));
    crate::stats::count(answer.as_u64().unwrap_or(0) as usize);
    if std::env::var("DH_DEBUG").is_ok() || DEBUG.load(Ordering::Relaxed) {
        horizon_bukkit::horizon_mod::log(&format!("packets_send {n} entries -> {answer}"));
    }
}

fn angle(deg: f32) -> i64 {
    ((deg * 256.0 / 360.0).floor() as i32 as i8) as i64
}

pub fn spawn(to: &[String], id: i32, kind: &str, pos: [f64; 3], yaw: f32) {
    if to.is_empty() {
        return;
    }
    let made = uuid();
    if let Ok(u) = u128::from_str_radix(&made, 16)
        && let Some(raw) = crate::wire::add_entity(id, u, kind, pos, angle(yaw) as i8)
    {
        return queue_raw(to, raw);
    }
    queue(
        to,
        "AddEntity",
        json!([["id", id], ["uuid", made], ["kind", kind], ["position", pos], ["velocity", [0.0, 0.0, 0.0]], ["pitch", 0], ["yaw", angle(yaw)], ["head_yaw", angle(yaw)], ["data", 0]]),
    );
}

/// Entity data: `(slot, serializer, value)`.
pub fn data(to: &[String], id: i32, values: Vec<Value>) {
    if values.is_empty() || to.is_empty() {
        return;
    }
    if let Some(raw) = crate::wire::entity_data(id, &values) {
        return queue_raw(to, raw);
    }
    queue(to, "SetEntityData", json!([["id", id], ["values", values]]));
}

pub fn entry(slot: u8, kind: i32, value: Value) -> Value {
    json!([slot, kind, value])
}

/// A stack on a slot (5: the head).
pub fn equip(to: &[String], id: i32, slot: u8, item: Value) {
    queue(to, "SetEquipment", json!([["id", id], ["slots", [[slot, item]]]]));
}

pub fn remove(to: &[String], ids: &[i32]) {
    if ids.is_empty() || to.is_empty() {
        return;
    }
    if let Some(raw) = crate::wire::remove(ids) {
        return queue_raw(to, raw);
    }
    queue(to, "RemoveEntities", json!([["ids", ids]]));
}

pub fn teleport(to: &[String], id: i32, pos: [f64; 3], yaw: f32) {
    if to.is_empty() {
        return;
    }
    if let Some(raw) = crate::wire::teleport(id, pos, yaw) {
        return queue_raw(to, raw);
    }
    queue(to, "TeleportEntity", json!([["id", id], ["position", pos], ["delta", [0.0, 0.0, 0.0]], ["yaw", yaw], ["pitch", 0.0], ["relatives", 0], ["on_ground", false]]));
}

/// An invisible marker armor stand's data (name shown when there is one).
pub fn stand_data(name: Option<&Value>, small: bool, marker: bool) -> Vec<Value> {
    let mut stand = 0x08;
    if small {
        stand |= 0x01;
    }
    if marker {
        stand |= 0x10;
    }
    let mut v = vec![entry(FLAGS, BYTE, json!(0x20)), entry(NO_GRAVITY, BOOL, json!(true)), entry(ARMOR_STAND_FLAGS, BYTE, json!(stand))];
    if let Some(n) = name {
        v.push(entry(CUSTOM_NAME, OPT_COMPONENT, n.clone()));
        v.push(entry(NAME_VISIBLE, BOOL, json!(true)));
    }
    v
}

/// A text line's new text (a legacy string), shown or not.
pub fn name(to: &[String], id: i32, legacy: &str, visible: bool) {
    if to.is_empty() {
        return;
    }
    if let Some(raw) = crate::wire::name(id, legacy, visible) {
        return queue_raw(to, raw);
    }
    data(to, id, name_data(&crate::text::component(legacy), visible));
}

/// A name's data alone (a text line's update).
pub fn name_data(name: &Value, visible: bool) -> Vec<Value> {
    vec![entry(CUSTOM_NAME, OPT_COMPONENT, name.clone()), entry(NAME_VISIBLE, BOOL, json!(visible))]
}
