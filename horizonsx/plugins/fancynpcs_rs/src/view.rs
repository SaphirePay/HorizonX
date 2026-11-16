//! Who sees which NPC, and NPCs turning to players: the NPC thread's work. The server thread only reads the
//! players (one `players` query for all of them, plus the permission and placeholder values the NPCs need) and
//! hands them over; spawning, updating, turning and removing are packets made here and sent from here.

use crate::net;
use crate::npc::Npc;
use horizon_kit::packets::queue_raw;
use horizon_kit::wire::W;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Viewer {
    pub uuid: String,
    pub name: String,
    pub world: String,
    pub pos: [f64; 3],
    /// NPC names this player may see (`fancynpcs.npc.<name>.see`, for PERMISSION_REQUIRED ones).
    pub sees: HashSet<String>,
    /// Display names with placeholders, filled for this player (npc id -> text JSON).
    pub names: HashMap<String, Value>,
}

pub struct Snapshot {
    pub viewers: Vec<Viewer>,
    pub visibility: bool,
    pub turn: bool,
    pub settings: Settings,
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub visibility_distance: i32,
    pub turn_distance: i32,
    pub reset_direction: bool,
    pub skip_invisible: bool,
    pub tab_remove_ms: u64,
    pub swing_arm: bool,
}

pub static INBOX: Mutex<Option<Snapshot>> = Mutex::new(None);
pub static NPCS: Mutex<Vec<Npc>> = Mutex::new(Vec::new());
/// Player-list entries to take away again (NPCs not shown in tab): when, the NPC's uuid, for whom.
static TAB_REMOVALS: Mutex<Vec<(Instant, u128, String)>> = Mutex::new(Vec::new());
/// Per (npc id, viewer): the name last sent, so a placeholder change sends one team update.
static SENT_NAMES: Mutex<Option<HashMap<(String, String), Value>>> = Mutex::new(None);

pub fn npcs() -> std::sync::MutexGuard<'static, Vec<Npc>> {
    NPCS.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- Packets for one NPC and one viewer ----

const FLAGS: u8 = 0;
const CUSTOM_NAME: u8 = 2;
const NAME_VISIBLE: u8 = 3;
const SILENT: u8 = 4;
const POSE: u8 = 6;
const FROZEN: u8 = 7;
const HANDS: u8 = 8;
const SKIN_PARTS: u8 = 16;

enum Data {
    Byte(u8),
    Int(i32),
    Bool(bool),
    OptComponent(Option<Value>),
    Pose(i32),
}

fn entity_data(entity: i32, values: &[(u8, Data)]) -> Option<Vec<u8>> {
    let packet = horizon_kit::wire::ids()?.entity_data;
    let mut w = W::default();
    w.varint(entity);
    for (slot, v) in values {
        w.u8(*slot);
        match v {
            Data::Byte(b) => {
                w.varint(0);
                w.u8(*b);
            }
            Data::Int(i) => {
                w.varint(1);
                w.varint(*i);
            }
            Data::Bool(b) => {
                w.varint(8);
                w.bool(*b);
            }
            Data::OptComponent(c) => {
                w.varint(6);
                w.bool(c.is_some());
                if let Some(c) = c {
                    w.nbt(c);
                }
            }
            Data::Pose(p) => {
                w.varint(20);
                w.varint(*p);
            }
        }
    }
    w.u8(0xFF);
    let mut inner = W::default();
    inner.varint(packet);
    inner.0.extend_from_slice(&w.0);
    let mut out = W::default();
    out.varint(inner.0.len() as i32);
    out.0.extend_from_slice(&inner.0);
    Some(out.0)
}

fn truthy(v: Option<&str>) -> bool {
    v.is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

fn empty_name(n: &Npc) -> bool {
    n.data.display_name.eq_ignore_ascii_case("<empty>")
}

fn shown_name(n: &Npc, viewer: Option<&Viewer>) -> Value {
    viewer.and_then(|v| v.names.get(&n.data.id).cloned()).unwrap_or_else(|| n.name_json.clone())
}

/// The data, team, name, equipment, scale and place of an NPC for one viewer (NPC.update).
pub fn update_bytes(n: &Npc, viewer: Option<&Viewer>, create_team: bool, swing: bool) -> Vec<u8> {
    let d = &n.data;
    let mut out = Vec::new();
    let name = shown_name(n, viewer);
    let hidden_name = empty_name(n);
    // Team: the player NPC's shown name is its prefix; the glow colour and collisions for every kind.
    let member = if d.is_player() { n.local_name.clone() } else { crate::uuid_string(n.uuid) };
    let prefix = if d.is_player() && !hidden_name { name.clone() } else { json!({ "text": "" }) };
    out.extend(net::team(&n.team, create_team, &prefix, net::color_id(&d.glowing_color), !hidden_name, d.collidable, &member).unwrap_or_default());
    if d.is_player() {
        out.extend(net::info_display(n.uuid, d.show_in_tab, (!hidden_name).then_some(&name)).unwrap_or_default());
    }
    let mut flags = 0u8;
    if truthy(d.attribute("on_fire")) {
        flags |= 0x01;
    }
    if d.attribute("pose").is_some_and(|p| p.eq_ignore_ascii_case("crouching")) {
        flags |= 0x02;
    }
    if truthy(d.attribute("invisible")) {
        flags |= 0x20;
    }
    if d.glowing {
        flags |= 0x40;
    }
    let mut values = vec![(FLAGS, Data::Byte(flags)), (SILENT, Data::Bool(truthy(d.attribute("silent"))))];
    if truthy(d.attribute("shaking")) {
        values.push((FROZEN, Data::Int(140)));
    }
    if d.is_player() {
        values.push((CUSTOM_NAME, Data::OptComponent(None)));
        values.push((NAME_VISIBLE, Data::Bool(false)));
        values.push((SKIN_PARTS, Data::Byte(127)));
    } else {
        values.push((CUSTOM_NAME, Data::OptComponent((!hidden_name).then_some(name))));
        values.push((NAME_VISIBLE, Data::Bool(!hidden_name)));
    }
    if let Some(pose) = d.attribute("pose") {
        let p = match pose.to_ascii_lowercase().as_str() {
            "sleeping" => 2,
            "swimming" => 3,
            "crouching" => 5,
            _ => 0,
        };
        values.push((POSE, Data::Pose(p)));
    }
    if let Some(hand) = d.attribute("use_item") {
        let b = match hand.to_ascii_lowercase().as_str() {
            "main_hand" => 0x01,
            "off_hand" => 0x03,
            _ => 0,
        };
        values.push((HANDS, Data::Byte(b)));
    }
    out.extend(entity_data(n.entity_id, &values).unwrap_or_default());
    let slots: Vec<(u8, String, i32)> = d.equipment.iter().filter_map(|(slot, item)| slot_id(slot).map(|s| (s, item.id.clone(), item.count))).collect();
    out.extend(net::equipment(n.entity_id, &slots).unwrap_or_default());
    out.extend(net::scale(n.entity_id, d.scale as f64).unwrap_or_default());
    if d.spawn_entity {
        out.extend(net::look(n.entity_id, d.pos, d.yaw, d.pitch));
        if swing && d.is_player() {
            out.extend(net::swing(n.entity_id).unwrap_or_default());
        }
    }
    out
}

pub fn slot_id(name: &str) -> Option<u8> {
    Some(match name.to_ascii_uppercase().as_str() {
        "MAINHAND" | "MAIN_HAND" | "HAND" => 0,
        "OFFHAND" | "OFF_HAND" => 1,
        "FEET" | "BOOTS" => 2,
        "LEGS" | "LEGGINGS" => 3,
        "CHEST" | "CHESTPLATE" => 4,
        "HEAD" | "HELMET" => 5,
        "BODY" => 6,
        "SADDLE" => 7,
        _ => return None,
    })
}

/// NPC.spawn for one viewer: the player-list entry with the skin, the entity, then its update.
pub fn spawn_bytes(n: &Npc, viewer: &Viewer) -> Vec<u8> {
    let d = &n.data;
    let mut out = Vec::new();
    if d.is_player() {
        let skin_id = if d.mirror_skin { Some(viewer.name.clone()) } else { d.skin.as_ref().map(|s| s.identifier.clone()) };
        let tex = skin_id.and_then(|id| crate::skins::get(&id));
        let name = shown_name(n, Some(viewer));
        let hidden = empty_name(n);
        out.extend(net::add_player(n.uuid, &n.local_name, tex.as_ref().map(|t| net::Skin { value: &t.value, signature: &t.signature }), d.show_in_tab, (!hidden).then_some(&name)).unwrap_or_default());
    }
    let kind = format!("minecraft:{}", d.kind.to_ascii_lowercase());
    if let Some(add) = horizon_kit::wire::add_entity(n.entity_id, n.uuid, &kind, d.pos, net::angle(d.yaw) as i8) {
        out.extend(add);
    }
    out
}

pub fn remove_bytes(n: &Npc) -> Vec<u8> {
    let mut out = Vec::new();
    if n.data.is_player() {
        out.extend(net::info_remove(&[n.uuid]).unwrap_or_default());
    }
    out.extend(horizon_kit::wire::remove(&[n.entity_id]).unwrap_or_default());
    out
}

/// Spawn + update for a viewer, with the player-list entry taken off later when not shown in tab.
fn show(n: &mut Npc, v: &Viewer, s: &Settings) {
    let mut bytes = spawn_bytes(n, v);
    let create_team = n.teams_sent.insert(v.uuid.clone());
    bytes.extend(update_bytes(n, Some(v), create_team, s.swing_arm));
    queue_raw(std::slice::from_ref(&v.uuid), bytes);
    n.viewers.insert(v.uuid.clone());
    SENT_NAMES.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert((n.data.id.clone(), v.uuid.clone()), shown_name(n, Some(v)));
    if n.data.is_player() && !n.data.show_in_tab && s.tab_remove_ms > 0 {
        TAB_REMOVALS.lock().unwrap_or_else(|e| e.into_inner()).push((Instant::now() + Duration::from_millis(s.tab_remove_ms), n.uuid, v.uuid.clone()));
    }
}

fn hide(n: &mut Npc, uuid: &str) {
    queue_raw(&[uuid.to_owned()], remove_bytes(n));
    n.viewers.remove(uuid);
    n.looking.remove(uuid);
}

fn should_see(n: &Npc, v: &Viewer, s: &Settings) -> bool {
    let d = &n.data;
    match d.visibility.as_str() {
        "PERMISSION_REQUIRED" if !v.sees.contains(&d.name) => return false,
        "MANUAL" => return false,
        _ => {}
    }
    let dist = if d.visibility_distance > -1 { d.visibility_distance } else { s.visibility_distance };
    if dist == 0 || !d.spawn_entity || v.world != d.world {
        return false;
    }
    if dist != i32::MAX {
        let (dx, dy, dz) = (d.pos[0] - v.pos[0], d.pos[1] - v.pos[1], d.pos[2] - v.pos[2]);
        if dx * dx + dy * dy + dz * dz > (dist as f64) * (dist as f64) {
            return false;
        }
    }
    !s.skip_invisible || !truthy(d.attribute("invisible")) || d.glowing || !d.equipment.is_empty()
}

/// The yaw and pitch from an NPC's eyes to a player's.
fn look_at(n: &Npc, v: &Viewer) -> (f32, f32) {
    let eye = if n.data.is_player() { 1.62 } else { 1.0 } * n.data.scale as f64;
    let (dx, dy, dz) = (v.pos[0] - n.data.pos[0], (v.pos[1] + 1.62) - (n.data.pos[1] + eye), v.pos[2] - n.data.pos[2]);
    let horizontal = (dx * dx + dz * dz).sqrt();
    let yaw = (-dx).atan2(dz).to_degrees() as f32;
    let pitch = (-dy).atan2(horizontal).to_degrees() as f32;
    (yaw, pitch)
}

/// One NPC-thread tick.
pub fn tick() {
    let Some(snap) = INBOX.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    let s = snap.settings;
    let online: HashSet<&str> = snap.viewers.iter().map(|v| v.uuid.as_str()).collect();
    let mut all = npcs();
    for n in all.iter_mut() {
        // Gone players.
        n.viewers.retain(|u| online.contains(u.as_str()));
        n.teams_sent.retain(|u| online.contains(u.as_str()));
        n.looking.retain(|u| online.contains(u.as_str()));
        if snap.visibility {
            for v in &snap.viewers {
                let should = should_see(n, v, &s);
                let seen = n.viewers.contains(&v.uuid);
                if should && !seen {
                    show(n, v, &s);
                } else if !should && seen {
                    hide(n, &v.uuid);
                }
            }
        }
        // Placeholder names that changed for a viewer: one team / name update.
        if n.data.display_name.contains('%') {
            for v in snap.viewers.iter().filter(|v| n.viewers.contains(&v.uuid)) {
                let now = shown_name(n, Some(v));
                let key = (n.data.id.clone(), v.uuid.clone());
                let mut sent = SENT_NAMES.lock().unwrap_or_else(|e| e.into_inner());
                let map = sent.get_or_insert_with(HashMap::new);
                if map.get(&key) != Some(&now) {
                    map.insert(key, now);
                    drop(sent);
                    queue_raw(std::slice::from_ref(&v.uuid), update_bytes(n, Some(v), false, false));
                }
            }
        }
        if snap.turn && n.data.turn_to_player {
            let reach = if n.data.turn_to_player_distance > -1 { n.data.turn_to_player_distance } else { s.turn_distance } as f64;
            for v in snap.viewers.iter().filter(|v| n.viewers.contains(&v.uuid)) {
                let (dx, dy, dz) = (n.data.pos[0] - v.pos[0], n.data.pos[1] - v.pos[1], n.data.pos[2] - v.pos[2]);
                if dx * dx + dy * dy + dz * dz <= reach * reach {
                    let (yaw, pitch) = look_at(n, v);
                    queue_raw(std::slice::from_ref(&v.uuid), net::look(n.entity_id, n.data.pos, yaw, pitch));
                    n.looking.insert(v.uuid.clone());
                } else if n.looking.remove(&v.uuid) && s.reset_direction {
                    queue_raw(std::slice::from_ref(&v.uuid), net::look(n.entity_id, n.data.pos, n.data.yaw, n.data.pitch));
                }
            }
        }
    }
    drop(all);
    let now = Instant::now();
    let due: Vec<(Instant, u128, String)> = {
        let mut list = TAB_REMOVALS.lock().unwrap_or_else(|e| e.into_inner());
        let (due, keep): (Vec<_>, Vec<_>) = list.drain(..).partition(|(at, _, _)| *at <= now);
        *list = keep;
        due
    };
    for (_, uuid, viewer) in due {
        if online.contains(viewer.as_str()) {
            queue_raw(&[viewer], net::info_remove(&[uuid]).unwrap_or_default());
        }
    }
}

// ---- From the server thread (commands) ----

/// The NPC's update sent to everyone who sees it (and the packets out now).
pub fn update_all(n: &mut Npc, swing: bool) {
    if let Some(map) = SENT_NAMES.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        map.retain(|(id, _), _| *id != n.data.id);
    }
    let viewers: Vec<String> = n.viewers.iter().cloned().collect();
    for v in viewers {
        let create = n.teams_sent.insert(v.clone());
        queue_raw(std::slice::from_ref(&v), update_bytes(n, None, create, swing));
    }
    horizon_kit::packets::flush();
}

/// Taken away from everyone (the next visibility pass shows it again: a new type, skin or place).
pub fn respawn_all(n: &mut Npc) {
    let viewers: Vec<String> = n.viewers.drain().collect();
    for v in &viewers {
        queue_raw(std::slice::from_ref(v), remove_bytes(n));
    }
    n.looking.clear();
    horizon_kit::packets::flush();
    crate::FORCE_VISIBILITY.store(true, std::sync::atomic::Ordering::Release);
}

pub fn remove_all(n: &mut Npc) {
    let viewers: Vec<String> = n.viewers.drain().collect();
    for v in &viewers {
        queue_raw(std::slice::from_ref(v), remove_bytes(n));
    }
    let teams: Vec<String> = n.teams_sent.drain().collect();
    for v in &teams {
        queue_raw(std::slice::from_ref(v), net::team_remove(&n.team).unwrap_or_default());
    }
    horizon_kit::packets::flush();
}
