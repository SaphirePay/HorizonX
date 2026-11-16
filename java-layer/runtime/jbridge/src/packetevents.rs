//! PacketEvents on Horizon: its API over the engine's packet hooks (the same as ProtocolLib's). A listener
//! hears the play packets going to a player (`pl_send`) and coming from one (`pl_receive`), reads them through
//! PacketEvents' wrappers, cancels them; a plugin builds packets with the wrappers (a text display only one
//! player sees: spawn, entity data, destroy) and sends them.
//!
//! Which packets are listened to comes from the plugin's own code (`plugin_packet_types`, which jport writes
//! from the `PacketType.Play.Server/Client.X` it names): a packet no plugin names is never decoded.
use crate::protocollib::{packet_type, packet_type_of, type_horizon};
use horizon_bukkit::Player;
use horizon_bukkit::horizon_mod::{Answer, Host, query};
use jrt::{R, V};
use serde_json::{Value, json};
use std::sync::{Mutex, OnceLock};

/// `PacketEvents.getAPI()` and the managers it hands out.
pub struct ApiH(&'static str);
/// `ServerVersion`: `V_1_20_2`, or the server's own (newer than any constant).
pub struct VersionH(Option<(u32, u32, u32)>);
/// A packet event: who, which packet, its fields, cancelled.
pub struct EventH {
    uuid: String,
    name: String,
    packet: String,
    fields: Vec<(String, String, Value)>,
    cancelled: Mutex<bool>,
    receive: bool,
}
/// A wrapper read from an event, or built to be sent: Horizon's packet name and its fields.
pub struct WrapperH {
    packet: String,
    fields: Mutex<Vec<(String, Value)>>,
    /// For a client interaction: attack (from `Attack`) or interact.
    attack: bool,
}
/// `new EntityData(index, EntityDataTypes.X, value)`.
pub struct EntityDataH(u8, i32, V);
/// PacketEvents' `Vector3d`/`Vector3f`/`Vector3i` and JOML's: three numbers.
pub struct Vec3H(f64, f64, f64);
/// PacketEvents' `Location`: a position and a facing.
pub struct LocationH(f64, f64, f64, f32, f32);
/// `ChunkData`'s column: where.
pub struct ColumnH(i32, i32);
/// A player's connection (`User`).
pub struct UserH(String, String);

static LISTENERS: Mutex<Vec<V>> = Mutex::new(Vec::new());
static HOOKED: OnceLock<()> = OnceLock::new();
static TYPES: OnceLock<&'static [&'static str]> = OnceLock::new();

/// The packet types the plugin's code names (`Server.CHUNK_DATA`, `Client.INTERACT_ENTITY`), from jport.
pub fn set_types(types: &'static [&'static str]) {
    let _ = TYPES.set(types);
}

fn vec3(v: &V) -> Option<(f64, f64, f64)> {
    if let Some(p) = v.native::<Vec3H>() {
        return Some((p.0, p.1, p.2));
    }
    if let Ok(items) = v.items() {
        if items.len() == 3 {
            return Some((items[0].double(), items[1].double(), items[2].double()));
        }
    }
    None
}

fn version_of(name: &str) -> Option<(u32, u32, u32)> {
    let mut parts = name.trim_start_matches("V_").split('_').map(|p| p.parse::<u32>().unwrap_or(0));
    Some((parts.next()?, parts.next().unwrap_or(0), parts.next().unwrap_or(0)))
}

fn packed_degrees(v: f64) -> i64 {
    ((v * 256.0 / 360.0).floor() as i64).rem_euclid(256) as i8 as i64
}

/// `EntityDataTypes.X`: the serializer's id in the game.
fn serializer(name: &str) -> Option<i32> {
    Some(match name {
        "BYTE" => 0,
        "INT" => 1,
        "LONG" => 2,
        "FLOAT" => 3,
        "STRING" => 4,
        "COMPONENT" | "ADV_COMPONENT" => 5,
        "OPTIONAL_COMPONENT" | "OPTIONAL_ADV_COMPONENT" => 6,
        "ITEMSTACK" => 7,
        "BOOLEAN" => 8,
        "ROTATION" => 9,
        "BLOCK_POSITION" => 10,
        "OPTIONAL_BLOCK_POSITION" => 11,
        "BLOCK_FACE" => 12,
        "OPTIONAL_UUID" => 13,
        "BLOCK_STATE" => 14,
        "OPTIONAL_BLOCK_STATE" => 15,
        "PARTICLE" => 16,
        "PARTICLES" => 17,
        "VILLAGER_DATA" => 18,
        "OPTIONAL_INT" => 19,
        "ENTITY_POSE" => 20,
        "VECTOR3F" => 39,
        "QUATERNION" => 40,
        _ => return None,
    })
}

/// An entity data value as the engine's JSON: a component's JSON, a vector's numbers.
fn data_json(kind: i32, value: &V) -> Value {
    match kind {
        5 | 6 => crate::held::<horizon_bukkit::Component>(value).map_or_else(|| if value.is_null() { Value::Null } else { json!({ "text": value.text() }) }, |c| c.0.clone()),
        39 => vec3(value).map_or(json!([0.0, 0.0, 0.0]), |(x, y, z)| json!([x, y, z])),
        40 => value.items().ok().map_or(json!([0.0, 0.0, 0.0, 1.0]), |q| json!(q.iter().map(V::double).collect::<Vec<_>>())),
        8 => json!(value.truth()),
        3 => json!(value.double()),
        4 => json!(value.text()),
        _ => match value {
            V::Bool(b) => json!(i64::from(*b)),
            V::Char(c) => json!(*c as i64),
            other => json!(other.long()),
        },
    }
}

/// A packet built by a wrapper, sent to a player past the listeners.
fn send(player_uuid: &str, wrapper: &WrapperH) -> V {
    let fields: Vec<Value> = jrt::lock(&wrapper.fields).iter().map(|(n, v)| json!([n, v])).collect();
    let uuid = player_uuid.replace('-', "");
    V::Bool(query(json!({ "op": "packet_send", "name": wrapper.packet, "uuid": uuid, "fields": fields })).as_bool().unwrap_or(false))
}

fn player_uuid(v: &V) -> Option<String> {
    if let Some(user) = v.native::<UserH>() {
        return Some(user.0.clone());
    }
    // The lock on the player is let go before asking it.
    let is_player = crate::held::<Player>(v).is_some();
    is_player.then(|| jrt::call(v, "getUniqueId", &[]).map(|u| u.text()).unwrap_or_default())
}

/// A packet on its way, as each listener of this plugin hears it.
fn heard(value: &Value, receive: bool) -> Answer {
    let listeners = jrt::lock(&LISTENERS).clone();
    if listeners.is_empty() {
        return Answer::Pass;
    }
    let packet = value["packet"].as_str().unwrap_or_default().to_owned();
    let fields = value["fields"].as_array().into_iter().flatten().map(|row| (row[0].as_str().unwrap_or_default().to_owned(), row[1].as_str().unwrap_or_default().to_owned(), row[2].clone())).collect();
    let event = jrt::native(EventH {
        uuid: value["uuid"].as_str().unwrap_or_default().to_owned(),
        name: value["player"].as_str().unwrap_or_default().to_owned(),
        packet,
        fields,
        cancelled: Mutex::new(false),
        receive,
    });
    for listener in listeners {
        // `PacketListenerAbstract.onPacketSend/Receive`, or `SimplePacketListenerAbstract.onPacketPlaySend/Receive`.
        let names: [&str; 2] = if receive { ["onPacketReceive", "onPacketPlayReceive"] } else { ["onPacketSend", "onPacketPlaySend"] };
        // A listener that doesn't override it has PacketListenerAbstract's, which does nothing (AntiMap's
        // MinimapDetector hears only what clients send). A lambda listener answers either.
        let name = match names.into_iter().find(|n| jrt::defines(&listener, n, 1)) {
            Some(name) => name,
            None if matches!(listener, V::Obj(_)) => continue,
            None => names[0],
        };
        let _ = jrt::report(name, jrt::call(&listener, name, std::slice::from_ref(&event)));
    }
    let cancelled = event.native::<EventH>().is_some_and(|e| *jrt::lock(&e.cancelled));
    if cancelled { Answer::Fail } else { Answer::Pass }
}

fn listen() {
    HOOKED.get_or_init(|| {
        for written in TYPES.get().copied().unwrap_or_default() {
            let (side, name) = written.split_once('.').unwrap_or(("Server", written));
            let server = side == "Server";
            let horizon = type_horizon(server, name);
            let names = if !server && horizon == "Interact" { vec!["Interact", "Attack"] } else { vec![horizon.as_str()] };
            for h in names {
                let listened = if server { h.to_owned() } else { format!("in:{h}") };
                query(json!({ "op": "packet_listen", "name": listened }));
            }
        }
        Host.on("pl_send", |v| heard(v, false));
        Host.on("pl_receive", |v| heard(v, true));
    });
}

pub fn field(class: &str, name: &str) -> Option<V> {
    Some(match class {
        "ServerVersion" => jrt::native(VersionH(version_of(name))),
        "EntityTypes" if name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') => jrt::s(&format!("minecraft:{}", name.to_ascii_lowercase())),
        "EntityDataTypes" => V::Int(serializer(name)?),
        "PacketListenerPriority" => jrt::s(name),
        "InteractAction" => jrt::s(name),
        _ => return None,
    })
}

pub fn static_method(class: &str, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (class, name) {
        ("PacketEvents", "getAPI") => jrt::native(ApiH("api")),
        ("PacketEvents", "setAPI") => V::Null,
        ("SpigotPacketEventsBuilder", "build" | "buildNoCache") => jrt::native(ApiH("api")),
        // The engine's PacketEvents is built once, with the server: nothing to forget.
        ("SpigotPacketEventsBuilder", "clearBuildCache") => V::Null,
        ("ServerVersion", "getLatest") => jrt::native(VersionH(None)),
        ("ServerVersion", "valueOf") => jrt::native(VersionH(version_of(&a.first().map(V::text).unwrap_or_default()))),
        _ => return None,
    }))
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    let int = |at: usize| a.get(at).map_or(0, V::long);
    let event = || a.first().and_then(|e| e.native::<EventH>());
    let wrapper = |packet: &str, fields: Vec<(String, Value)>| Some(Ok(jrt::native(WrapperH { packet: packet.to_owned(), fields: Mutex::new(fields), attack: false })));
    match class {
        "Vector3d" | "Vector3f" | "Vector3i" if a.len() == 3 => Some(Ok(jrt::native(Vec3H(a[0].double(), a[1].double(), a[2].double())))),
        "Vector3d" | "Vector3f" | "Vector3i" if a.is_empty() => Some(Ok(jrt::native(Vec3H(0.0, 0.0, 0.0)))),
        // JOML's `Quaternionf()` (no rotation) or `(x, y, z, w)`; `AxisAngle4f(angle, x, y, z)` as its quaternion.
        "Quaternionf" | "Quaterniond" => Some(Ok(jrt::arr(if a.len() == 4 { a.iter().map(|v| V::Double(v.double())).collect() } else { vec![V::Double(0.0), V::Double(0.0), V::Double(0.0), V::Double(1.0)] }))),
        "AxisAngle4f" | "AxisAngle4d" if a.len() == 4 => {
            let (angle, x, y, z) = (a[0].double(), a[1].double(), a[2].double(), a[3].double());
            let length = (x * x + y * y + z * z).sqrt().max(f64::EPSILON);
            let half = (angle / 2.0).sin() / length;
            Some(Ok(jrt::arr(vec![V::Double(x * half), V::Double(y * half), V::Double(z * half), V::Double((angle / 2.0).cos())])))
        }
        "AxisAngle4f" | "AxisAngle4d" => Some(Ok(jrt::arr(vec![V::Double(0.0), V::Double(0.0), V::Double(0.0), V::Double(1.0)]))),
        "Location" if a.len() == 3 && vec3(&a[0]).is_some() => {
            let (x, y, z) = vec3(&a[0])?;
            Some(Ok(jrt::native(LocationH(x, y, z, a[1].double() as f32, a[2].double() as f32))))
        }
        "Location" if a.len() == 5 && a.iter().all(|v| matches!(v, V::Int(_) | V::Long(_) | V::Double(_))) => {
            Some(Ok(jrt::native(LocationH(a[0].double(), a[1].double(), a[2].double(), a[3].double() as f32, a[4].double() as f32))))
        }
        "EntityData" if a.len() == 3 => Some(Ok(jrt::native(EntityDataH(int(0) as u8, a[1].int(), a[2].clone())))),
        // Read from an event: its packet's fields, by Horizon's names.
        "WrapperPlayServerChunkData" | "WrapperPlayServerUnloadChunk" | "WrapperPlayClientInteractEntity" | "WrapperPlayServerEntityMetadata" | "WrapperPlayServerSpawnEntity" | "WrapperPlayServerDestroyEntities" if event().is_some() => {
            let e = event()?;
            let fields = e.fields.iter().map(|(n, _, v)| (n.clone(), v.clone())).collect();
            Some(Ok(jrt::native(WrapperH { packet: e.packet.clone(), fields: Mutex::new(fields), attack: e.packet == "Attack" })))
        }
        // `new WrapperPlayServerSpawnEntity(id, uuid, type, location, headYaw, data, velocity)`.
        "WrapperPlayServerSpawnEntity" if a.len() >= 4 => {
            let uuid = match &a[1] {
                V::Null => String::new(),
                other => jrt::call(other, "orElse", &[V::Null]).map(|u| u.text()).unwrap_or_else(|_| other.text()),
            };
            let (x, y, z, yaw, pitch) = a[3].native::<LocationH>().map(|l| (l.0, l.1, l.2, l.3, l.4)).or_else(|| vec3(&a[3]).map(|(x, y, z)| (x, y, z, 0.0, 0.0))).unwrap_or_default();
            let head_yaw = a.get(4).map_or(f64::from(yaw), V::double);
            let velocity = a.get(6).and_then(|v| vec3(&jrt::call(v, "orElse", &[V::Null]).unwrap_or_else(|_| v.clone()))).unwrap_or_default();
            wrapper("AddEntity", vec![
                ("id".into(), json!(int(0))),
                ("uuid".into(), json!(if uuid.is_empty() { format!("{:032x}", rand_uuid()) } else { uuid.replace('-', "") })),
                ("kind".into(), json!(a[2].text())),
                ("position".into(), json!([x, y, z])),
                ("velocity".into(), json!([velocity.0, velocity.1, velocity.2])),
                ("pitch".into(), json!(packed_degrees(f64::from(pitch)))),
                ("yaw".into(), json!(packed_degrees(f64::from(yaw)))),
                ("head_yaw".into(), json!(packed_degrees(head_yaw))),
                ("data".into(), json!(a.get(5).map_or(0, V::long))),
            ])
        }
        // `new WrapperPlayServerEntityMetadata(id, List<EntityData>)`.
        "WrapperPlayServerEntityMetadata" if a.len() == 2 => {
            let values: Vec<Value> = a[1].items().unwrap_or_default().iter().filter_map(|d| d.native::<EntityDataH>().map(|d| json!([d.0, d.1, data_json(d.1, &d.2)]))).collect();
            wrapper("SetEntityData", vec![("id".into(), json!(int(0))), ("values".into(), Value::Array(values))])
        }
        // `new WrapperPlayServerDestroyEntities(ids...)`.
        "WrapperPlayServerDestroyEntities" => {
            let ids: Vec<i64> = a.iter().flat_map(|v| v.items().map(|i| i.iter().map(V::long).collect()).unwrap_or_else(|_| vec![v.long()])).collect();
            wrapper("RemoveEntities", vec![("ids".into(), json!(ids))])
        }
        _ => None,
    }
}

fn rand_uuid() -> u128 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    (nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835) & !(0xf << 76)) | (0x4 << 76)
}

fn field_of<'a>(fields: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(api) = this.native::<ApiH>() {
        return Some(Ok(match (api.0, name) {
            (_, "getEventManager") => jrt::native(ApiH("events")),
            (_, "getPlayerManager") => jrt::native(ApiH("players")),
            (_, "getServerManager") => jrt::native(ApiH("server")),
            (_, "getProtocolManager" | "getSettings" | "getInjector") => this.clone(),
            (_, "isLoaded" | "isInitialized" | "isTerminated") => V::Bool(name != "isTerminated"),
            (_, "load" | "init" | "terminate" | "checkForUpdates" | "reEncodeByDefault" | "debug" | "bStats" | "kickOnPacketException") => this.clone(),
            ("server", "getVersion") => jrt::native(VersionH(None)),
            ("events", "registerListener" | "registerListeners") => {
                listen();
                for listener in a.iter().filter(|v| matches!(v, V::Obj(_))) {
                    jrt::lock(&LISTENERS).push(listener.clone());
                }
                a.first().cloned().unwrap_or(V::Null)
            }
            ("events", "unregisterListener" | "unregisterListeners") => {
                jrt::lock(&LISTENERS).retain(|l| !a.iter().any(|x| jrt::same(l, x)));
                V::Null
            }
            ("events", "unregisterAllListeners") => {
                jrt::lock(&LISTENERS).clear();
                V::Null
            }
            ("players", "sendPacket" | "sendPacketSilently" | "writePacket" | "writePacketSilently") if a.len() == 2 => {
                let (Some(uuid), Some(wrapper)) = (player_uuid(&a[0]), a[1].native::<WrapperH>()) else {
                    return Some(Ok(V::Null));
                };
                send(&uuid, wrapper);
                V::Null
            }
            ("players", "getUser") => {
                let uuid = player_uuid(&a[0]).unwrap_or_default();
                let name = jrt::call(&a[0], "getName", &[]).map(|n| n.text()).unwrap_or_default();
                jrt::native(UserH(uuid, name))
            }
            ("players", "getClientVersion") => jrt::native(VersionH(None)),
            ("players", "getPing") => V::Int(0),
            _ => return None,
        }));
    }
    if let Some(v) = this.native::<VersionH>() {
        let newest = (u32::MAX, 0, 0);
        let mine = v.0.unwrap_or(newest);
        let other = || a.first().and_then(|o| o.native::<VersionH>()).map_or(newest, |o| o.0.unwrap_or(newest));
        return Some(Ok(match name {
            "isNewerThanOrEquals" => V::Bool(mine >= other()),
            "isNewerThan" => V::Bool(mine > other()),
            "isOlderThan" => V::Bool(mine < other()),
            "isOlderThanOrEquals" => V::Bool(mine <= other()),
            "getReleaseName" | "toString" | "name" => jrt::s(&v.0.map_or_else(|| "26.3".to_owned(), |(a, b, c)| if c == 0 { format!("{a}.{b}") } else { format!("{a}.{b}.{c}") })),
            "getProtocolVersion" => V::Int(777),
            "equals" => V::Bool(mine == other()),
            "hashCode" => V::Int(mine.1 as i32 * 100 + mine.2 as i32),
            _ => return None,
        }));
    }
    if let Some(e) = this.native::<EventH>() {
        return Some(Ok(match name {
            "getPacketType" => {
                let horizon = if e.packet == "Attack" { "Interact" } else { e.packet.as_str() };
                packet_type_of(!e.receive, horizon)
            }
            "getPlayer" if e.uuid.is_empty() => V::Null,
            "getPlayer" => crate::wrap(Player::of_ids(&e.uuid, &e.name)),
            "getUser" => jrt::native(UserH(e.uuid.clone(), e.name.clone())),
            "isCancelled" => V::Bool(*jrt::lock(&e.cancelled)),
            "setCancelled" => {
                *jrt::lock(&e.cancelled) = a.first().is_some_and(V::truth);
                V::Null
            }
            "getConnectionState" => jrt::s("PLAY"),
            "getServerVersion" => jrt::native(VersionH(None)),
            "markForReEncode" | "setLastUsedWrapper" | "cleanUp" => V::Null,
            "getPacketId" => V::Int(0),
            "getPacketName" => jrt::s(&e.packet),
            _ => return None,
        }));
    }
    if let Some(w) = this.native::<WrapperH>() {
        let fields = jrt::lock(&w.fields).clone();
        let get = |n: &str| field_of(&fields, n).cloned().unwrap_or(Value::Null);
        return Some(Ok(match (w.packet.as_str(), name) {
            ("LevelChunkWithLight", "getColumn") => jrt::native(ColumnH(get("x").as_i64().unwrap_or(0) as i32, get("z").as_i64().unwrap_or(0) as i32)),
            ("ForgetLevelChunk", "getChunkX") => V::Int(get("pos")[0].as_i64().unwrap_or(0) as i32),
            ("ForgetLevelChunk", "getChunkZ") => V::Int(get("pos")[1].as_i64().unwrap_or(0) as i32),
            ("Interact" | "Attack", "getEntityId") => V::Int(get("id").as_i64().unwrap_or(0) as i32),
            ("Interact" | "Attack", "getAction") => jrt::s(if w.attack { "ATTACK" } else { "INTERACT" }),
            ("Interact" | "Attack", "isSneaking") => jrt::call_static("Optional", "of", &[V::Bool(get("secondary").as_bool().unwrap_or(false))]).unwrap_or(V::Null),
            ("Interact" | "Attack", "getHand") => jrt::s(if get("hand").as_i64() == Some(1) { "OFF_HAND" } else { "MAIN_HAND" }),
            ("Interact" | "Attack", "getTarget") => {
                let at = get("location");
                jrt::call_static("Optional", "ofNullable", &[if at.is_null() { V::Null } else { jrt::native(Vec3H(at[0].as_f64().unwrap_or(0.0), at[1].as_f64().unwrap_or(0.0), at[2].as_f64().unwrap_or(0.0))) }]).unwrap_or(V::Null)
            }
            ("AddEntity" | "SetEntityData" | "RemoveEntities", "getEntityId") => V::Int(get("id").as_i64().unwrap_or(0) as i32),
            ("RemoveEntities", "getEntityIds") => jrt::arr(get("ids").as_array().into_iter().flatten().map(|v| V::Int(v.as_i64().unwrap_or(0) as i32)).collect()),
            (_, "getPacketTypeData" | "write" | "read" | "copy") => this.clone(),
            (_, "toString") => jrt::s(&format!("Wrapper[{}]", w.packet)),
            _ => return None,
        }));
    }
    if let Some(c) = this.native::<ColumnH>() {
        return Some(Ok(match name {
            "getX" => V::Int(c.0),
            "getZ" => V::Int(c.1),
            "isFullChunk" => V::Bool(true),
            _ => return None,
        }));
    }
    if let Some(user) = this.native::<UserH>() {
        return Some(Ok(match name {
            "getUUID" | "getProfile" => jrt::s(&user.0),
            "getName" => jrt::s(&user.1),
            "sendPacket" | "sendPacketSilently" | "writePacket" => {
                if let Some(wrapper) = a.first().and_then(|w| w.native::<WrapperH>()) {
                    send(&user.0, wrapper);
                }
                V::Null
            }
            "getClientVersion" => jrt::native(VersionH(None)),
            "getConnectionState" => jrt::s("PLAY"),
            _ => return None,
        }));
    }
    if let Some(d) = this.native::<EntityDataH>() {
        return Some(Ok(match name {
            "getIndex" => V::Int(i32::from(d.0)),
            "getValue" => d.2.clone(),
            "getType" => V::Int(d.1),
            _ => return None,
        }));
    }
    if let Some(p) = this.native::<Vec3H>() {
        return Some(Ok(match (name, a.len()) {
            ("getX" | "x", 0) => V::Double(p.0),
            ("getY" | "y", 0) => V::Double(p.1),
            ("getZ" | "z", 0) => V::Double(p.2),
            // JOML's public fields, read as `v.x`.
            ("$field", 1) => match a[0].text().as_str() {
                "x" => V::Double(p.0),
                "y" => V::Double(p.1),
                "z" => V::Double(p.2),
                _ => return None,
            },
            ("add" | "subtract" | "multiply" | "mul" | "sub", 1) => {
                let (x, y, z) = vec3(&a[0]).unwrap_or((a[0].double(), a[0].double(), a[0].double()));
                jrt::native(match name {
                    "add" => Vec3H(p.0 + x, p.1 + y, p.2 + z),
                    "subtract" | "sub" => Vec3H(p.0 - x, p.1 - y, p.2 - z),
                    _ => Vec3H(p.0 * x, p.1 * y, p.2 * z),
                })
            }
            ("length", 0) => V::Double((p.0 * p.0 + p.1 * p.1 + p.2 * p.2).sqrt()),
            ("toString", 0) => jrt::s(&format!("({}, {}, {})", p.0, p.1, p.2)),
            _ => return None,
        }));
    }
    if let Some(l) = this.native::<LocationH>() {
        return Some(Ok(match name {
            "getX" => V::Double(l.0),
            "getY" => V::Double(l.1),
            "getZ" => V::Double(l.2),
            "getYaw" => V::Double(f64::from(l.3)),
            "getPitch" => V::Double(f64::from(l.4)),
            "getPosition" => jrt::native(Vec3H(l.0, l.1, l.2)),
            _ => return None,
        }));
    }
    // `PacketType.Play.Server.X == event.getPacketType()` uses one object per type: see protocollib.
    let _ = packet_type;
    None
}
