//! NPCs and npcs.yml (FancyNpcs' own format: `npcs.<id>.name/displayName/type/location.../skin/actions/...`).
//! Each NPC keeps its whole YAML section, so keys this port doesn't know survive a save.

use serde_yaml::{Mapping, Value as Y};
use std::collections::{HashMap, HashSet};

pub const TRIGGERS: [&str; 4] = ["ANY_CLICK", "LEFT_CLICK", "RIGHT_CLICK", "CUSTOM"];

#[derive(Clone, Debug)]
pub struct Action {
    pub order: i32,
    pub action: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Item {
    /// minecraft:diamond_sword
    pub id: String,
    pub count: i32,
    /// The stack as Bukkit wrote it (kept as it was).
    pub yaml: Y,
}

#[derive(Clone, Debug, Default)]
pub struct SkinRef {
    pub identifier: String,
    /// AUTO / SLIM
    pub variant: String,
}

#[derive(Clone, Debug)]
pub struct Data {
    pub id: String,
    pub name: String,
    pub creator: String,
    pub display_name: String,
    /// The Bukkit entity type name (PLAYER, VILLAGER...).
    pub kind: String,
    pub world: String,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub show_in_tab: bool,
    pub spawn_entity: bool,
    pub collidable: bool,
    pub glowing: bool,
    pub glowing_color: String,
    pub turn_to_player: bool,
    pub turn_to_player_distance: i32,
    pub interaction_cooldown: f32,
    pub scale: f32,
    pub visibility_distance: i32,
    /// ALL / PERMISSION_REQUIRED / MANUAL
    pub visibility: String,
    pub skin: Option<SkinRef>,
    pub mirror_skin: bool,
    /// Slot name (MAINHAND, OFFHAND, HEAD, CHEST, LEGS, FEET, BODY) -> item.
    pub equipment: Vec<(String, Item)>,
    pub attributes: Vec<(String, String)>,
    pub actions: HashMap<String, Vec<Action>>,
    pub raw: Mapping,
}

impl Data {
    pub fn new(name: &str, creator: &str, world: &str, pos: [f64; 3], yaw: f32, pitch: f32) -> Data {
        Data {
            id: crate::new_uuid_string(),
            name: name.to_owned(),
            creator: creator.to_owned(),
            display_name: name.to_owned(),
            kind: "PLAYER".into(),
            world: world.to_owned(),
            pos,
            yaw,
            pitch,
            show_in_tab: false,
            spawn_entity: true,
            collidable: true,
            glowing: false,
            glowing_color: "white".into(),
            turn_to_player: false,
            turn_to_player_distance: -1,
            interaction_cooldown: 0.0,
            scale: 1.0,
            visibility_distance: -1,
            visibility: "ALL".into(),
            skin: None,
            mirror_skin: false,
            equipment: Vec::new(),
            attributes: Vec::new(),
            actions: HashMap::new(),
            raw: Mapping::new(),
        }
    }
    pub fn is_player(&self) -> bool {
        self.kind.eq_ignore_ascii_case("PLAYER")
    }
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn actions_of(&self, trigger: &str) -> Vec<Action> {
        let mut list = self.actions.get(trigger).cloned().unwrap_or_default();
        list.sort_by_key(|a| a.order);
        list
    }
}

/// What the server knows of one NPC on top of its data.
pub struct Npc {
    pub data: Data,
    pub entity_id: i32,
    pub uuid: u128,
    /// The profile name: eight invisible colour codes (the shown name is the team prefix).
    pub local_name: String,
    pub team: String,
    pub viewers: HashSet<String>,
    pub teams_sent: HashSet<String>,
    /// Viewers this NPC is turned to.
    pub looking: HashSet<String>,
    pub last_interaction: HashMap<String, std::time::Instant>,
    /// The display name as text JSON (placeholders filled without a player), made on the server thread.
    pub name_json: serde_json::Value,
    pub dirty: bool,
}

impl Npc {
    pub fn new(data: Data) -> Npc {
        const CHARS: &[u8] = b"0123456789abcdefklmnor";
        let mut local = String::new();
        for _ in 0..8 {
            local.push('§');
            local.push(CHARS[crate::rand_u32() as usize % CHARS.len()] as char);
        }
        let id = horizon_kit::packets::next_id();
        Npc {
            data,
            entity_id: id,
            uuid: crate::rand_u128() & !(0xF000u128 << 64) | (0x4000u128 << 64),
            local_name: local,
            team: format!("npc-{:012x}", crate::rand_u128() & 0xFFFF_FFFF_FFFF),
            viewers: HashSet::new(),
            teams_sent: HashSet::new(),
            looking: HashSet::new(),
            last_interaction: HashMap::new(),
            name_json: serde_json::Value::Null,
            dirty: true,
        }
    }
}

// ---- YAML ----

fn s(m: &Mapping, k: &str) -> Option<String> {
    m.get(k).and_then(|v| match v {
        Y::String(s) => Some(s.clone()),
        Y::Number(n) => Some(n.to_string()),
        Y::Bool(b) => Some(b.to_string()),
        _ => None,
    })
}
fn b(m: &Mapping, k: &str, d: bool) -> bool {
    m.get(k).and_then(Y::as_bool).unwrap_or(d)
}
fn f(m: &Mapping, k: &str, d: f64) -> f64 {
    m.get(k).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(d)
}
fn sub<'a>(m: &'a Mapping, k: &str) -> Option<&'a Mapping> {
    m.get(k).and_then(Y::as_mapping)
}

pub fn item_from_yaml(v: &Y) -> Option<Item> {
    let m = v.as_mapping()?;
    let id = s(m, "id").or_else(|| s(m, "type").map(|t| format!("minecraft:{}", t.to_ascii_lowercase())))?;
    let id = if id.contains(':') { id } else { format!("minecraft:{}", id.to_ascii_lowercase()) };
    let count = m.get("count").or_else(|| m.get("amount")).and_then(Y::as_i64).unwrap_or(1) as i32;
    Some(Item { id, count, yaml: v.clone() })
}

pub fn item_yaml(id: &str, count: i32) -> Y {
    let mut m = Mapping::new();
    m.insert("==".into(), "org.bukkit.inventory.ItemStack".into());
    m.insert("DataVersion".into(), 4440.into());
    m.insert("id".into(), id.into());
    m.insert("count".into(), count.into());
    m.insert("schema_version".into(), 1.into());
    Y::Mapping(m)
}

pub fn from_yaml(id: &str, m: &Mapping) -> Option<Data> {
    let loc = sub(m, "location")?;
    let world = s(loc, "world")?;
    let mut d = Data::new(&s(m, "name").unwrap_or_else(|| id.to_owned()), &s(m, "creator").unwrap_or_default(), &world, [f(loc, "x", 0.0), f(loc, "y", 0.0), f(loc, "z", 0.0)], f(loc, "yaw", 0.0) as f32, f(loc, "pitch", 0.0) as f32);
    d.id = id.to_owned();
    d.display_name = s(m, "displayName").unwrap_or_else(|| "<empty>".into());
    d.kind = s(m, "type").unwrap_or_else(|| "PLAYER".into()).to_ascii_uppercase();
    d.show_in_tab = b(m, "showInTab", false);
    d.spawn_entity = b(m, "spawnEntity", true);
    d.collidable = b(m, "collidable", true);
    d.glowing = b(m, "glowing", false);
    d.glowing_color = s(m, "glowingColor").unwrap_or_else(|| "white".into());
    d.turn_to_player = b(m, "turnToPlayer", false);
    d.turn_to_player_distance = f(m, "turnToPlayerDistance", -1.0) as i32;
    d.interaction_cooldown = f(m, "interactionCooldown", 0.0) as f32;
    d.scale = f(m, "scale", 1.0) as f32;
    d.visibility_distance = f(m, "visibility_distance", -1.0) as i32;
    d.visibility = s(m, "visibility").unwrap_or_else(|| "ALL".into()).to_ascii_uppercase();
    if let Some(skin) = sub(m, "skin") {
        let identifier = s(skin, "identifier").or_else(|| s(skin, "uuid")).unwrap_or_default();
        d.mirror_skin = b(skin, "mirrorSkin", false);
        if !identifier.is_empty() {
            d.skin = Some(SkinRef { identifier: identifier.clone(), variant: s(skin, "variant").unwrap_or_else(|| "AUTO".into()) });
        }
        // An old file's skin texture straight in npcs.yml: into the skin cache.
        if let (Some(value), Some(signature)) = (s(skin, "value"), s(skin, "signature"))
            && !value.is_empty()
        {
            crate::skins::remember(&identifier, &value, &signature);
        }
    }
    if let Some(eq) = sub(m, "equipment") {
        for (k, v) in eq {
            if let (Some(k), Some(item)) = (k.as_str(), item_from_yaml(v)) {
                d.equipment.push((k.to_ascii_uppercase(), item));
            }
        }
    }
    if let Some(attrs) = sub(m, "attributes") {
        for (k, v) in attrs {
            if let (Some(k), Some(v)) = (k.as_str(), v.as_str().map(str::to_owned).or_else(|| v.as_bool().map(|b| b.to_string()))) {
                d.attributes.push((k.to_owned(), v));
            }
        }
    }
    if let Some(actions) = sub(m, "actions") {
        for (trigger, list) in actions {
            let (Some(trigger), Some(list)) = (trigger.as_str(), list.as_mapping()) else { continue };
            let mut out = Vec::new();
            for (order, a) in list {
                let order = order.as_i64().or_else(|| order.as_str().and_then(|s| s.parse().ok()));
                let (Some(order), Some(a)) = (order, a.as_mapping()) else { continue };
                out.push(Action { order: order as i32, action: s(a, "action").unwrap_or_default(), value: s(a, "value") });
            }
            out.sort_by_key(|a| a.order);
            d.actions.insert(trigger.to_ascii_uppercase(), out);
        }
    }
    d.raw = m.clone();
    Some(d)
}

pub fn to_yaml(d: &Data) -> Mapping {
    let mut m = d.raw.clone();
    let mut put = |k: &str, v: Y| {
        m.insert(k.into(), v);
    };
    put("name", d.name.clone().into());
    put("creator", d.creator.clone().into());
    put("displayName", d.display_name.clone().into());
    put("type", d.kind.clone().into());
    let mut loc = Mapping::new();
    loc.insert("world".into(), d.world.clone().into());
    loc.insert("x".into(), d.pos[0].into());
    loc.insert("y".into(), d.pos[1].into());
    loc.insert("z".into(), d.pos[2].into());
    loc.insert("yaw".into(), (d.yaw as f64).into());
    loc.insert("pitch".into(), (d.pitch as f64).into());
    put("location", Y::Mapping(loc));
    put("showInTab", d.show_in_tab.into());
    put("spawnEntity", d.spawn_entity.into());
    put("collidable", d.collidable.into());
    put("glowing", d.glowing.into());
    put("glowingColor", d.glowing_color.clone().into());
    put("turnToPlayer", d.turn_to_player.into());
    put("turnToPlayerDistance", d.turn_to_player_distance.into());
    put("interactionCooldown", (d.interaction_cooldown as f64).into());
    put("scale", (d.scale as f64).into());
    put("visibility_distance", d.visibility_distance.into());
    put("visibility", d.visibility.clone().into());
    let mut skin = Mapping::new();
    if let Some(sk) = &d.skin {
        skin.insert("identifier".into(), sk.identifier.clone().into());
        skin.insert("variant".into(), sk.variant.clone().into());
    }
    skin.insert("mirrorSkin".into(), d.mirror_skin.into());
    put("skin", Y::Mapping(skin));
    if d.equipment.is_empty() {
        m.remove("equipment");
    } else {
        let mut eq = Mapping::new();
        for (slot, item) in &d.equipment {
            eq.insert(slot.clone().into(), item.yaml.clone());
        }
        m.insert("equipment".into(), Y::Mapping(eq));
    }
    let mut attrs = Mapping::new();
    for (k, v) in &d.attributes {
        attrs.insert(k.clone().into(), v.clone().into());
    }
    m.insert("attributes".into(), Y::Mapping(attrs));
    let mut actions = Mapping::new();
    for (trigger, list) in &d.actions {
        if list.is_empty() {
            continue;
        }
        let mut lm = Mapping::new();
        for a in list {
            let mut am = Mapping::new();
            am.insert("action".into(), a.action.clone().into());
            am.insert("value".into(), a.value.clone().map_or(Y::Null, Y::String));
            lm.insert(Y::Number(a.order.into()), Y::Mapping(am));
        }
        actions.insert(trigger.clone().into(), Y::Mapping(lm));
    }
    if actions.is_empty() {
        m.remove("actions");
    } else {
        m.insert("actions".into(), Y::Mapping(actions));
    }
    for gone in ["messages", "playerCommands", "serverCommands", "sendMessagesRandomly"] {
        m.remove(gone);
    }
    m
}
