//! The packets an NPC is made of, as the bytes the client reads (no JSON per packet): player-list entries with the
//! skin, the entity, its data, team (name tag, glow colour, collisions), equipment, scale, turns and removal. Packet
//! ids and registry numbers are asked of the engine once on the server thread (`init`); after that every builder
//! here is pure and runs on the NPC thread too.

use horizon_kit::wire::W;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::OnceLock;

pub struct Ids {
    pub info_update: i32,
    pub info_remove: i32,
    pub team: i32,
    pub rotate_head: i32,
    pub attributes: i32,
    pub equipment: i32,
    pub animate: i32,
    pub teleport: i32,
    pub scale_attribute: Option<i32>,
    pub items: HashMap<String, i32>,
}

static IDS: OnceLock<Option<Ids>> = OnceLock::new();

/// Asked once, on the server thread (packet ids and registries are game queries).
pub fn init() -> bool {
    let _ = horizon_kit::wire::ids();
    IDS.get_or_init(|| {
        let names = ["PlayerInfoUpdate", "PlayerInfoRemove", "SetPlayerTeam", "RotateHead", "UpdateAttributes", "SetEquipment", "Animate", "TeleportEntity"];
        let answer = horizon_bukkit::horizon_mod::query(json!({ "op": "packet_ids", "names": names }));
        let id = |n: &str| answer[n].as_i64().map(|i| i as i32);
        let registry = |name: &str| -> HashMap<String, i32> {
            let answer = horizon_bukkit::horizon_mod::query(json!({ "op": "core_registry", "registry": name }));
            answer.as_array().into_iter().flatten().enumerate().filter_map(|(i, n)| Some((n.as_str()?.to_owned(), i as i32))).collect()
        };
        let attributes = registry("minecraft:attribute");
        Some(Ids {
            info_update: id("PlayerInfoUpdate")?,
            info_remove: id("PlayerInfoRemove")?,
            team: id("SetPlayerTeam")?,
            rotate_head: id("RotateHead")?,
            attributes: id("UpdateAttributes")?,
            equipment: id("SetEquipment")?,
            animate: id("Animate")?,
            teleport: id("TeleportEntity")?,
            scale_attribute: attributes.get("minecraft:scale").copied(),
            items: registry("minecraft:item"),
        })
    })
    .is_some()
}

fn ids() -> Option<&'static Ids> {
    IDS.get().and_then(Option::as_ref)
}

fn framed(id: i32, body: &W) -> Vec<u8> {
    let mut inner = W::default();
    inner.varint(id);
    inner.0.extend_from_slice(&body.0);
    let mut out = W::default();
    out.varint(inner.0.len() as i32);
    out.0.extend_from_slice(&inner.0);
    out.0
}

pub fn angle(deg: f32) -> u8 {
    ((deg * 256.0 / 360.0).floor() as i32 as i8) as u8
}

pub struct Skin<'a> {
    pub value: &'a str,
    pub signature: &'a str,
}

/// PlayerInfoUpdate with ADD_PLAYER (profile + skin), UPDATE_LISTED, UPDATE_DISPLAY_NAME.
pub fn add_player(uuid: u128, profile_name: &str, skin: Option<Skin>, listed: bool, tab_name: Option<&Value>) -> Option<Vec<u8>> {
    let ids = ids()?;
    let mut w = W::default();
    // ADD_PLAYER | UPDATE_GAME_MODE | UPDATE_LISTED | UPDATE_DISPLAY_NAME
    w.u8(0x01 | 0x04 | 0x08 | 0x20);
    w.varint(1);
    w.0.extend_from_slice(&uuid.to_be_bytes());
    w.str(profile_name);
    match skin {
        Some(s) => {
            w.varint(1);
            w.str("textures");
            w.str(s.value);
            w.bool(!s.signature.is_empty());
            if !s.signature.is_empty() {
                w.str(s.signature);
            }
        }
        None => w.varint(0),
    }
    w.varint(0);
    w.bool(listed);
    match tab_name {
        Some(n) => {
            w.bool(true);
            w.nbt(n);
        }
        None => w.bool(false),
    }
    Some(framed(ids.info_update, &w))
}

/// PlayerInfoUpdate: the tab name and whether it's listed.
pub fn info_display(uuid: u128, listed: bool, tab_name: Option<&Value>) -> Option<Vec<u8>> {
    let ids = ids()?;
    let mut w = W::default();
    w.u8(0x08 | 0x20);
    w.varint(1);
    w.0.extend_from_slice(&uuid.to_be_bytes());
    w.bool(listed);
    match tab_name {
        Some(n) => {
            w.bool(true);
            w.nbt(n);
        }
        None => w.bool(false),
    }
    Some(framed(ids.info_update, &w))
}

pub fn info_remove(uuids: &[u128]) -> Option<Vec<u8>> {
    let ids = ids()?;
    let mut w = W::default();
    w.varint(uuids.len() as i32);
    for u in uuids {
        w.0.extend_from_slice(&u.to_be_bytes());
    }
    Some(framed(ids.info_remove, &w))
}

/// SetPlayerTeam create (method 0) or update (2): prefix (a player NPC's shown name), colour (the glow), name tag
/// and collisions. `member` joins when created.
pub fn team(name: &str, create: bool, prefix: &Value, color: Option<i32>, name_visible: bool, collidable: bool, member: &str) -> Option<Vec<u8>> {
    let ids = ids()?;
    let mut w = W::default();
    w.str(name);
    w.u8(if create { 0 } else { 2 });
    w.nbt(&json!({ "text": "" }));
    w.nbt(prefix);
    w.nbt(&json!({ "text": "" }));
    w.varint(if name_visible { 0 } else { 1 });
    w.varint(if collidable { 0 } else { 1 });
    match color {
        Some(c) => {
            w.bool(true);
            w.varint(c);
        }
        None => w.bool(false),
    }
    w.u8(0);
    if create {
        w.varint(1);
        w.str(member);
    }
    Some(framed(ids.team, &w))
}

pub fn team_remove(name: &str) -> Option<Vec<u8>> {
    let ids = ids()?;
    let mut w = W::default();
    w.str(name);
    w.u8(1);
    Some(framed(ids.team, &w))
}

pub fn teleport(entity: i32, pos: [f64; 3], yaw: f32, pitch: f32) -> Option<Vec<u8>> {
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
    w.f32(pitch);
    w.i32(0);
    w.bool(false);
    Some(framed(ids.teleport, &w))
}

pub fn rotate_head(entity: i32, yaw: f32) -> Option<Vec<u8>> {
    let ids = ids()?;
    let mut w = W::default();
    w.varint(entity);
    w.u8(angle(yaw));
    Some(framed(ids.rotate_head, &w))
}

/// A turn: the body (teleport in place) and the head.
pub fn look(entity: i32, pos: [f64; 3], yaw: f32, pitch: f32) -> Vec<u8> {
    let mut out = teleport(entity, pos, yaw, pitch).unwrap_or_default();
    out.extend(rotate_head(entity, yaw).unwrap_or_default());
    out
}

pub fn swing(entity: i32) -> Option<Vec<u8>> {
    let ids = ids()?;
    let mut w = W::default();
    w.varint(entity);
    w.u8(0);
    Some(framed(ids.animate, &w))
}

pub fn scale(entity: i32, scale: f64) -> Option<Vec<u8>> {
    let ids = ids()?;
    let attribute = ids.scale_attribute?;
    let mut w = W::default();
    w.varint(entity);
    w.varint(1);
    w.varint(attribute);
    w.f64(scale);
    w.varint(0);
    Some(framed(ids.attributes, &w))
}

/// SetEquipment: `(slot, item id like minecraft:diamond_sword, count)`; count 0 empties the slot.
pub fn equipment(entity: i32, slots: &[(u8, String, i32)]) -> Option<Vec<u8>> {
    let ids = ids()?;
    if slots.is_empty() {
        return None;
    }
    let mut w = W::default();
    w.varint(entity);
    for (i, (slot, item, count)) in slots.iter().enumerate() {
        let last = i + 1 == slots.len();
        w.u8(if last { *slot } else { *slot | 0x80 });
        match ids.items.get(item.as_str()).filter(|_| *count > 0) {
            Some(id) => {
                w.varint(*count);
                w.varint(*id);
                w.varint(0);
                w.varint(0);
            }
            None => w.varint(0),
        }
    }
    Some(framed(ids.equipment, &w))
}

/// The game's colour number of a named text colour (glow and team colour).
pub fn color_id(name: &str) -> Option<i32> {
    let names = ["black", "dark_blue", "dark_green", "dark_aqua", "dark_red", "dark_purple", "gold", "gray", "dark_gray", "blue", "green", "aqua", "red", "light_purple", "yellow", "white"];
    names.iter().position(|n| n.eq_ignore_ascii_case(name.trim())).map(|i| i as i32)
}

pub const COLORS: [&str; 16] = ["black", "dark_blue", "dark_green", "dark_aqua", "dark_red", "dark_purple", "gold", "gray", "dark_gray", "blue", "green", "aqua", "red", "light_purple", "yellow", "white"];
