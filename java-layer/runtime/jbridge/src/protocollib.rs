//! ProtocolLib on Horizon: its API over the engine's packet hook. A listener hears the play packets going
//! to a player (`pl_send`), reads their fields by kind (`getIntegers`, `getBlockPositionModifier`, ...),
//! changes them or cancels the packet; a plugin builds packets (`createPacket`) and sends them.
//!
//! Horizon names packets as it models them (`BlockUpdate`); ProtocolLib's names (`BLOCK_CHANGE`) map here.
//! A field reads by Horizon's order of the packet's fields, which follows the protocol's.

use horizon_bukkit::horizon_mod::{query, Answer, Host, Value};
use jrt::{R, V};
use serde_json::json;
use std::sync::{Mutex, OnceLock};

/// ProtocolLib's server packet names, as Horizon calls them.
const SERVER: &[(&str, &str)] = &[
    ("SPAWN_ENTITY", "AddEntity"), ("ANIMATION", "Animate"), ("STATISTIC", "AwardStats"), ("BLOCK_BREAK_ANIMATION", "BlockDestruction"),
    ("TILE_ENTITY_DATA", "BlockEntityData"), ("BLOCK_ACTION", "BlockEvent"), ("BLOCK_CHANGE", "BlockUpdate"), ("BOSS", "BossEvent"),
    ("SERVER_DIFFICULTY", "ChangeDifficulty"), ("CLEAR_TITLES", "ClearTitles"), ("COMMANDS", "Commands"), ("CLOSE_WINDOW", "ContainerClose"),
    ("WINDOW_ITEMS", "ContainerSetContent"), ("WINDOW_DATA", "ContainerSetData"), ("SET_SLOT", "ContainerSetSlot"), ("SET_COOLDOWN", "Cooldown"),
    ("CUSTOM_PAYLOAD", "CustomPayload"), ("KICK_DISCONNECT", "Disconnect"), ("ENTITY_STATUS", "EntityEvent"), ("UNLOAD_CHUNK", "ForgetLevelChunk"),
    ("GAME_STATE_CHANGE", "GameEvent"), ("HURT_ANIMATION", "HurtAnimation"), ("DAMAGE_EVENT", "DamageEvent"), ("KEEP_ALIVE", "KeepAlive"),
    ("MAP_CHUNK", "LevelChunkWithLight"), ("WORLD_EVENT", "LevelEvent"), ("LIGHT_UPDATE", "LightUpdate"), ("LOGIN", "Login"), ("MAP", "MapItemData"),
    ("REL_ENTITY_MOVE", "MoveEntityPos"), ("REL_ENTITY_MOVE_LOOK", "MoveEntityPosRot"), ("ENTITY_LOOK", "MoveEntityRot"),
    ("OPEN_WINDOW_MERCHANT", "MerchantOffers"), ("OPEN_WINDOW", "OpenScreen"), ("OPEN_BOOK", "OpenBook"), ("OPEN_SIGN_EDITOR", "OpenSignEditor"),
    ("PING", "Ping"), ("ABILITIES", "PlayerAbilities"), ("CHAT", "PlayerChat"), ("SYSTEM_CHAT", "SystemChat"), ("PLAYER_INFO_REMOVE", "PlayerInfoRemove"),
    ("PLAYER_INFO", "PlayerInfoUpdate"), ("LOOK_AT", "PlayerLookAt"), ("POSITION", "PlayerPosition"), ("ENTITY_DESTROY", "RemoveEntities"),
    ("REMOVE_ENTITY_EFFECT", "RemoveMobEffect"), ("RESPAWN", "Respawn"), ("ENTITY_HEAD_ROTATION", "RotateHead"), ("MULTI_BLOCK_CHANGE", "SectionBlocksUpdate"),
    ("SERVER_DATA", "ServerData"), ("SET_ACTION_BAR_TEXT", "SetActionBarText"), ("CAMERA", "SetCamera"), ("VIEW_CENTRE", "SetChunkCacheCenter"),
    ("SPAWN_POSITION", "SetDefaultSpawnPosition"), ("SCOREBOARD_DISPLAY_OBJECTIVE", "SetDisplayObjective"), ("ENTITY_METADATA", "SetEntityData"),
    ("ENTITY_VELOCITY", "SetEntityMotion"), ("ATTACH_ENTITY", "SetEntityLink"), ("ENTITY_EQUIPMENT", "SetEquipment"), ("EXPERIENCE", "SetExperience"),
    ("UPDATE_HEALTH", "SetHealth"), ("HELD_ITEM_SLOT", "SetHeldSlot"), ("SCOREBOARD_OBJECTIVE", "SetObjective"), ("MOUNT", "SetPassengers"),
    ("SCOREBOARD_TEAM", "SetPlayerTeam"), ("SCOREBOARD_SCORE", "SetScore"), ("SET_SUBTITLE_TEXT", "SetSubtitleText"), ("UPDATE_TIME", "SetTime"),
    ("SET_TITLE_TEXT", "SetTitleText"), ("SET_TITLES_ANIMATION", "SetTitlesAnimation"), ("NAMED_SOUND_EFFECT", "Sound"), ("ENTITY_SOUND", "SoundEntity"),
    ("STOP_SOUND", "StopSound"), ("PLAYER_LIST_HEADER_FOOTER", "TabList"), ("COLLECT", "TakeItemEntity"), ("ENTITY_TELEPORT", "TeleportEntity"),
    ("ADVANCEMENTS", "UpdateAdvancements"), ("UPDATE_ATTRIBUTES", "UpdateAttributes"), ("WORLD_PARTICLES", "LevelParticles"), ("EXPLOSION", "Explode"),
    ("ENTITY_EFFECT", "UpdateMobEffect"), ("TAGS", "UpdateTags"),
    // PacketEvents' names for the same packets.
    ("CHUNK_DATA", "LevelChunkWithLight"), ("DESTROY_ENTITIES", "RemoveEntities"), ("ENTITY_RELATIVE_MOVE", "MoveEntityPos"),
    ("ENTITY_RELATIVE_MOVE_AND_ROTATION", "MoveEntityPosRot"), ("ENTITY_ROTATION", "MoveEntityRot"), ("ENTITY_HEAD_LOOK", "RotateHead"),
    ("TIME_UPDATE", "SetTime"), ("SOUND_EFFECT", "Sound"), ("PARTICLE", "LevelParticles"), ("CHAT_MESSAGE", "PlayerChat"),
    ("SYSTEM_CHAT_MESSAGE", "SystemChat"), ("PLAYER_INFO_UPDATE", "PlayerInfoUpdate"), ("DISCONNECT", "Disconnect"), ("EFFECT", "LevelEvent"),
    ("PLAYER_POSITION_AND_LOOK", "PlayerPosition"), ("BLOCK_ENTITY_DATA", "BlockEntityData"), ("UPDATE_LIGHT", "LightUpdate"),
    ("JOIN_GAME", "Login"), ("WINDOW_PROPERTY", "ContainerSetData"), ("OPEN_SIGN_EDITOR", "OpenSignEditor"),
];

/// The packets a player sends, by ProtocolLib's and PacketEvents' names.
const CLIENT: &[(&str, &str)] = &[
    ("INTERACT_ENTITY", "Interact"), ("USE_ENTITY", "Interact"), ("CHAT_MESSAGE", "Chat"), ("CHAT", "Chat"), ("CHAT_COMMAND", "ChatCommand"),
    ("PLAYER_DIGGING", "PlayerAction"), ("BLOCK_DIG", "PlayerAction"), ("PLAYER_BLOCK_PLACEMENT", "UseItemOn"), ("USE_ITEM_ON", "UseItemOn"),
    ("USE_ITEM", "UseItem"), ("BLOCK_PLACE", "UseItem"), ("ANIMATION", "Swing"), ("ARM_ANIMATION", "Swing"), ("CLICK_WINDOW", "ContainerClick"),
    ("WINDOW_CLICK", "ContainerClick"), ("CLOSE_WINDOW", "ContainerClose"), ("PLAYER_POSITION", "MovePlayerPos"), ("POSITION", "MovePlayerPos"),
    ("PLAYER_POSITION_AND_ROTATION", "MovePlayerPosRot"), ("POSITION_LOOK", "MovePlayerPosRot"), ("PLAYER_ROTATION", "MovePlayerRot"),
    ("LOOK", "MovePlayerRot"), ("PLAYER_FLYING", "MovePlayerStatusOnly"), ("FLYING", "MovePlayerStatusOnly"), ("ENTITY_ACTION", "PlayerCommand"),
    ("HELD_ITEM_CHANGE", "SetCarriedItem"), ("HELD_ITEM_SLOT", "SetCarriedItem"), ("KEEP_ALIVE", "KeepAlive"), ("PLUGIN_MESSAGE", "CustomPayload"),
    ("CUSTOM_PAYLOAD", "CustomPayload"), ("TAB_COMPLETE", "CommandSuggestion"), ("UPDATE_SIGN", "SignUpdate"), ("PLAYER_INPUT", "PlayerInput"),
    ("STEER_VEHICLE", "PlayerInput"), ("CLIENT_STATUS", "ClientCommand"), ("CLIENT_SETTINGS", "ClientInformation"), ("SETTINGS", "ClientInformation"),
    ("PONG", "Pong"), ("TELEPORT_CONFIRM", "AcceptTeleportation"), ("CREATIVE_INVENTORY_ACTION", "SetCreativeModeSlot"), ("SET_CREATIVE_SLOT", "SetCreativeModeSlot"),
];

/// Horizon's name of a packet a plugin names (`Server.CHUNK_DATA`, `Client.INTERACT_ENTITY`).
pub(crate) fn type_horizon(server: bool, name: &str) -> String {
    if server {
        return horizon_name(name);
    }
    CLIENT.iter().find(|(p, _)| *p == name).map_or_else(|| name.split('_').map(|word| word[..1].to_owned() + &word[1..].to_ascii_lowercase()).collect(), |(_, h)| (*h).to_owned())
}

/// One object per packet type, so `event.getPacketType() == PacketType.Play.Server.X` holds as in Java.
static TYPES: Mutex<Vec<((bool, String), V)>> = Mutex::new(Vec::new());

/// A packet type as a plugin names it.
pub(crate) fn packet_type(server: bool, name: &str) -> V {
    let horizon = type_horizon(server, name);
    shared_type(server, horizon, name.to_owned())
}

/// A packet type by Horizon's name (what a heard packet is).
pub(crate) fn packet_type_of(server: bool, horizon: &str) -> V {
    let protocol = if server { protocol_name(horizon) } else { CLIENT.iter().find(|(_, h)| *h == horizon).map_or_else(|| horizon.to_owned(), |(p, _)| (*p).to_owned()) };
    shared_type(server, horizon.to_owned(), protocol)
}

fn shared_type(server: bool, horizon: String, protocol: String) -> V {
    let mut all = jrt::lock(&TYPES);
    if let Some((_, v)) = all.iter().find(|((s, h), _)| *s == server && *h == horizon) {
        return v.clone();
    }
    let made = jrt::native(TypeH { horizon: horizon.clone(), protocol, server });
    all.push(((server, horizon), made.clone()));
    made
}

/// Horizon's name of a ProtocolLib server packet; an unknown one by its words (`SET_TIME` -> `SetTime`).
fn horizon_name(protocol: &str) -> String {
    SERVER.iter().find(|(p, _)| *p == protocol).map_or_else(
        || protocol.split('_').map(|word| word[..1].to_owned() + &word[1..].to_ascii_lowercase()).collect(),
        |(_, h)| (*h).to_owned(),
    )
}

fn protocol_name(horizon: &str) -> String {
    SERVER.iter().find(|(_, h)| *h == horizon).map_or_else(|| horizon.to_owned(), |(p, _)| (*p).to_owned())
}

/// `PacketType.Play.Server.X`.
pub struct TypeH {
    horizon: String,
    protocol: String,
    server: bool,
}
/// The `PacketAdapter` a listener extends: what it listens to.
pub struct AdapterH {
    plugin: V,
    priority: String,
    types: Vec<String>,
}
pub struct ManagerH;
/// A packet: its fields by name, kind and value, and what a plugin changed.
pub struct ContainerH {
    name: String,
    fields: Mutex<Vec<(String, String, Value)>>,
    edits: Mutex<Vec<(String, Value)>>,
}
/// A packet's fields of one kind (`getIntegers()`): read and written by their order among that kind.
pub struct ModifierH {
    packet: V,
    kinds: &'static [&'static str],
}
pub struct EventH {
    /// The receiver's UUID, 32 hex digits as the engine writes it, and name.
    uuid: String,
    player: String,
    packet: V,
    cancelled: Mutex<bool>,
}
pub struct PositionH(i32, i32, i32);
pub struct BlockDataH(u32);

/// The plugin's packet listeners, by Horizon packet name.
static LISTENERS: Mutex<Vec<(String, V)>> = Mutex::new(Vec::new());
static HOOKED: OnceLock<()> = OnceLock::new();

fn type_of(v: &V) -> Option<String> {
    v.native::<TypeH>().map(|t| t.horizon.clone())
}

fn packet_v(name: &str, fields: Vec<(String, String, Value)>) -> V {
    jrt::native(ContainerH { name: name.to_owned(), fields: Mutex::new(fields), edits: Mutex::new(Vec::new()) })
}

fn fields_of(list: &Value) -> Vec<(String, String, Value)> {
    list.as_array()
        .into_iter()
        .flatten()
        .map(|row| (row[0].as_str().unwrap_or_default().to_owned(), row[1].as_str().unwrap_or_default().to_owned(), row[2].clone()))
        .collect()
}

/// A packet on its way to a player: every listener of this plugin hears it.
fn heard(value: &Value) -> Answer {
    let name = value["packet"].as_str().unwrap_or_default();
    let listeners: Vec<V> = jrt::lock(&LISTENERS).iter().filter(|(n, _)| n == name).map(|(_, l)| l.clone()).collect();
    if listeners.is_empty() {
        return Answer::Pass;
    }
    // Once per packet name, the log says the hook delivers it: how to tell a listener heard anything.
    {
        static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let mut seen = jrt::lock(&SEEN);
        if !seen.iter().any(|s| s == name) {
            seen.push(name.to_owned());
            crate::the_plugin().info(&format!("[ProtocolLib] first {} ({}) packet heard", protocol_name(name), name));
        }
    }
    let packet = packet_v(name, fields_of(&value["fields"]));
    let uuid = value["uuid"].as_str().filter(|uuid| uuid.trim_start_matches('0').len() > 0).unwrap_or_default().to_owned();
    let event = jrt::native(EventH { uuid, player: value["player"].as_str().unwrap_or_default().to_owned(), packet: packet.clone(), cancelled: Mutex::new(false) });
    for listener in listeners {
        let _ = jrt::report("onPacketSending", jrt::call(&listener, "onPacketSending", std::slice::from_ref(&event)));
    }
    let cancelled = event.native::<EventH>().is_some_and(|e| *jrt::lock(&e.cancelled));
    if let Some(container) = packet.native::<ContainerH>() {
        for (field, new) in jrt::lock(&container.edits).iter() {
            query(json!({ "op": "packet_edit", "field": field, "value": new }));
        }
    }
    if cancelled { Answer::Fail } else { Answer::Pass }
}

/// `PacketType.Play.Server.MAP_CHUNK`, `ListenerPriority.HIGHEST` and friends.
pub fn field(class: &str, name: &str) -> Option<V> {
    match class {
        "Server" => Some(packet_type(true, name)),
        "Client" => Some(packet_type(false, name)),
        _ => None,
    }
}

pub fn static_method(class: &str, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (class, name, a.len()) {
        ("ProtocolLibrary", "getProtocolManager", 0) => jrt::native(ManagerH),
        ("ProtocolLibrary", "getPlugin", 0) => jrt::s("ProtocolLib"),
        ("BlockPosition", "getBlockPosition" | "fromVector", 1) => {
            let at = |m: &str| jrt::call(&a[0], m, &[]).map(|v| v.double().floor() as i32).unwrap_or(0);
            jrt::native(PositionH(at("getBlockX"), at("getBlockY"), at("getBlockZ")))
        }
        _ => return None,
    }))
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    match class {
        // new PacketAdapter(plugin, [priority,] types...) or (plugin, priority, Iterable<types>)
        "PacketAdapter" => {
            let mut types = Vec::new();
            let mut priority = "NORMAL".to_owned();
            for arg in &a[1.min(a.len())..] {
                if let Some(t) = type_of(arg) {
                    types.push(t);
                } else if let Ok(items) = arg.items() {
                    types.extend(items.iter().filter_map(type_of));
                } else if matches!(arg, V::Str(_)) {
                    priority = arg.text();
                }
            }
            Some(Ok(jrt::native(AdapterH { plugin: a.first().cloned().unwrap_or(V::Null), priority, types })))
        }
        "BlockPosition" if a.len() == 3 => Some(Ok(jrt::native(PositionH(a[0].int(), a[1].int(), a[2].int())))),
        "PacketContainer" if !a.is_empty() => {
            let name = type_of(&a[0])?;
            Some(Ok(blank(&name)))
        }
        _ => None,
    }
}

fn blank(name: &str) -> V {
    packet_v(name, fields_of(&query(json!({ "op": "packet_blank", "name": name }))))
}

/// The packet's listeners' types, from the `PacketAdapter` it extends.
fn adapter_types(listener: &V) -> Vec<String> {
    let parent = listener.get("__native").unwrap_or(V::Null);
    parent.native::<AdapterH>().map(|a| a.types.clone()).or_else(|| listener.native::<AdapterH>().map(|a| a.types.clone()))
        // A listener of the Java layer's (a real Java PacketAdapter): its native adapter, asked for by name.
        .or_else(|| matches!(listener, V::Native(_)).then(|| jrt::call(listener, "__adapter", &[]).ok()).flatten().and_then(|h| h.native::<AdapterH>().map(|a| a.types.clone())))
        .unwrap_or_default()
}

/// ProtocolLib as a plugin: the bridge is it, so `getPlugin("ProtocolLib")` finds it enabled.
pub struct PluginH;

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if matches!(name, "getPlugin" | "isPluginEnabled") && a.len() == 1 && matches!(&a[0], V::Str(text) if text.eq_ignore_ascii_case("ProtocolLib")) {
        return Some(Ok(if name == "getPlugin" { jrt::native(PluginH) } else { V::Bool(true) }));
    }
    if this.native::<PluginH>().is_some() {
        return Some(Ok(match name {
            "getName" | "toString" => jrt::s("ProtocolLib"),
            "isEnabled" => V::Bool(true),
            "getDescription" | "getPluginMeta" => this.clone(),
            "getVersion" => jrt::s("5.4.0"),
            "getFullName" => jrt::s("ProtocolLib v5.4.0"),
            _ => return None,
        }));
    }
    if this.native::<ManagerH>().is_some() {
        return Some(Ok(match name {
            "addPacketListener" => {
                let types = adapter_types(&a[0]);
                if types.is_empty() {
                    crate::the_plugin().warning("[ProtocolLib] a packet listener listens to no packet type");
                } else {
                    crate::the_plugin().info(&format!("[ProtocolLib] listening to {}", types.iter().map(|t| protocol_name(t)).collect::<Vec<_>>().join(", ")));
                }
                for t in &types {
                    query(json!({ "op": "packet_listen", "name": t }));
                    jrt::lock(&LISTENERS).push((t.clone(), a[0].clone()));
                }
                HOOKED.get_or_init(|| Host.on("pl_send", heard));
                V::Null
            }
            "removePacketListener" => {
                jrt::lock(&LISTENERS).retain(|(_, l)| !jrt::same(l, &a[0]));
                V::Null
            }
            "removePacketListeners" => {
                jrt::lock(&LISTENERS).clear();
                V::Null
            }
            "getPacketListeners" => jrt::list(jrt::lock(&LISTENERS).iter().map(|(_, l)| l.clone()).collect()),
            "createPacket" => match a.first().and_then(type_of) {
                Some(name) => blank(&name),
                None => V::Null,
            },
            // sendServerPacket(player, packet [, filters]): to that player, past the listeners.
            "sendServerPacket" => {
                let Some(packet) = a.get(1).and_then(|p| p.native::<ContainerH>()) else {
                    return Some(Err(jrt::exc("IllegalArgumentException", "not a packet".to_owned())));
                };
                let uuid = jrt::call(&a[0], "getUniqueId", &[]).map(|u| u.text().replace('-', "")).unwrap_or_default();
                let fields: Vec<Value> = jrt::lock(&packet.fields).iter().map(|(f, _, v)| json!([f, v])).collect();
                query(json!({ "op": "packet_send", "uuid": uuid, "name": packet.name, "fields": fields }));
                V::Null
            }
            "broadcastServerPacket" => {
                let players = jrt::call_static("Bukkit", "getOnlinePlayers", &[]).and_then(|p| p.items()).unwrap_or_default();
                for player in players {
                    let _ = method(this, "sendServerPacket", &[player, a[0].clone()]);
                }
                V::Null
            }
            "isClosed" => V::Bool(false),
            "getMinecraftVersion" => jrt::s("26.3"),
            _ => return None,
        }));
    }
    if let Some(adapter) = this.native::<AdapterH>() {
        return Some(Ok(match name {
            "getPlugin" => adapter.plugin.clone(),
            "getSendingWhitelist" | "getReceivingWhitelist" => jrt::list(adapter.types.iter().map(|t| packet_type_of(true, t)).collect()),
            "getPriority" => jrt::s(&adapter.priority),
            "onPacketSending" | "onPacketReceiving" => V::Null,
            _ => return None,
        }));
    }
    if let Some(event) = this.native::<EventH>() {
        return Some(Ok(match name {
            // The receiver, also while logging in (the first chunks go out before the join event).
            "getPlayer" if event.uuid.is_empty() => V::Null,
            "getPlayer" => crate::wrap(horizon_bukkit::Player::of_ids(&event.uuid, &event.player)),
            "getPacket" => event.packet.clone(),
            "getPacketType" => event.packet.native::<ContainerH>().map_or(V::Null, |p| packet_type_of(true, &p.name)),
            "isCancelled" => V::Bool(*jrt::lock(&event.cancelled)),
            "setCancelled" => {
                *jrt::lock(&event.cancelled) = a.first().is_some_and(V::truth);
                V::Null
            }
            "isServerPacket" => V::Bool(true),
            "setPacket" => V::Null,
            _ => return None,
        }));
    }
    if let Some(packet) = this.native::<ContainerH>() {
        let kinds: Option<&'static [&'static str]> = match name {
            "getIntegers" => Some(&["int"]),
            "getShorts" => Some(&["short"]),
            "getBytes" => Some(&["byte"]),
            "getBooleans" => Some(&["boolean"]),
            "getFloat" => Some(&["float"]),
            "getDoubles" => Some(&["double"]),
            "getLongs" => Some(&["long"]),
            "getStrings" => Some(&["string"]),
            "getUUIDs" => Some(&["uuid"]),
            "getBlockPositionModifier" | "getSectionPositions" => Some(&["position"]),
            "getModifier" => Some(&["int", "short", "byte", "boolean", "float", "double", "long", "string", "uuid", "position", "list", "optional"]),
            // BlockUpdate's state is a number on Horizon: block data reads it.
            "getBlockData" => Some(&["blockstate"]),
            _ => None,
        };
        if let Some(kinds) = kinds {
            return Some(Ok(jrt::native(ModifierH { packet: this.clone(), kinds })));
        }
        return Some(Ok(match name {
            "getType" => packet_type_of(true, &packet.name),
            "getHandle" | "shallowClone" | "deepClone" => this.clone(),
            "toString" => jrt::s(&format!("PacketContainer[{}]", packet.name)),
            _ => return None,
        }));
    }
    if let Some(modifier) = this.native::<ModifierH>() {
        let packet = modifier.packet.native::<ContainerH>()?;
        let block_state = modifier.kinds == ["blockstate"];
        // Which fields this modifier sees: by kind, or BlockUpdate's state for block data.
        let index: Vec<usize> = jrt::lock(&packet.fields)
            .iter()
            .enumerate()
            .filter(|(_, (field, kind, _))| if block_state { field == "state" } else { modifier.kinds.contains(&kind.as_str()) })
            .map(|(i, _)| i)
            .collect();
        let at = |n: &V| index.get(n.int().max(0) as usize).copied();
        let read = |i: usize| -> V {
            let (_, kind, value) = jrt::lock(&packet.fields)[i].clone();
            match kind.as_str() {
                _ if block_state => jrt::native(BlockDataH(value.as_u64().unwrap_or(0) as u32)),
                "position" => jrt::native(PositionH(value[0].as_i64().unwrap_or(0) as i32, value[1].as_i64().unwrap_or(0) as i32, value[2].as_i64().unwrap_or(0) as i32)),
                _ => crate::xplugin::from_plain(&value),
            }
        };
        return Some(Ok(match name {
            "size" => V::Int(index.len() as i32),
            "read" | "readSafely" => match a.first().and_then(at) {
                Some(i) => read(i),
                None if name == "readSafely" => V::Null,
                None => return Some(Err(jrt::exc("FieldAccessException", format!("no field {} of that kind", a.first().map(V::text).unwrap_or_default())))),
            },
            "getValues" => jrt::list(index.iter().map(|i| read(*i)).collect()),
            "write" | "writeSafely" => {
                let Some(i) = a.first().and_then(at) else {
                    return Some(Ok(this.clone()));
                };
                let new = match a.get(1) {
                    Some(v) if v.native::<PositionH>().is_some() => v.native::<PositionH>().map(|p| json!([p.0, p.1, p.2])).unwrap_or(Value::Null),
                    Some(v) if v.native::<BlockDataH>().is_some() => v.native::<BlockDataH>().map(|b| json!(b.0)).unwrap_or(Value::Null),
                    Some(v) => crate::xplugin::to_plain(v),
                    None => Value::Null,
                };
                let field = {
                    let mut fields = jrt::lock(&packet.fields);
                    fields[i].2 = new.clone();
                    fields[i].0.clone()
                };
                jrt::lock(&packet.edits).push((field, new));
                this.clone()
            }
            _ => return None,
        }));
    }
    if let Some(p) = this.native::<PositionH>() {
        return Some(Ok(match name {
            "getX" => V::Int(p.0),
            "getY" => V::Int(p.1),
            "getZ" => V::Int(p.2),
            "toString" => jrt::s(&format!("BlockPosition [x={}, y={}, z={}]", p.0, p.1, p.2)),
            "toLocation" => jrt::new_other("Location", &[a[0].clone(), V::Double(f64::from(p.0)), V::Double(f64::from(p.1)), V::Double(f64::from(p.2))]).unwrap_or(V::Null),
            _ => return None,
        }));
    }
    if let Some(data) = this.native::<BlockDataH>() {
        let state = || query(json!({ "op": "state_name", "state": data.0 })).as_str().unwrap_or("minecraft:air").to_owned();
        return Some(Ok(match name {
            // `Material` by its name: minecraft:stone[...] -> STONE.
            "getType" => jrt::s(&state().split('[').next().unwrap_or_default().trim_start_matches("minecraft:").to_ascii_uppercase()),
            "getData" => V::Int(0),
            "toString" => jrt::s(&state()),
            _ => return None,
        }));
    }
    if let Some(t) = this.native::<TypeH>() {
        return Some(Ok(match name {
            "name" | "toString" => jrt::s(&t.protocol),
            "isServer" => V::Bool(t.server),
            "isClient" => V::Bool(!t.server),
            "equals" => V::Bool(a[0].native::<TypeH>().is_some_and(|o| o.horizon == t.horizon)),
            "hashCode" => V::Int(t.horizon.len() as i32),
            _ => return None,
        }));
    }
    None
}
