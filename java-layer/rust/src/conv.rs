//! Values between Java and the SDK.
//!
//! Plain values travel as themselves (String, boxed numbers). A game object stays here, in [`HANDLES`], and Java
//! gets an `H` (its id, the Bukkit class it is, a key that names it for `equals`). Small value classes
//! (Location, Vector, Component, Material, NamespacedKey) and collections travel as a `Pack`, which
//! `horizon.javalayer.Conv` turns into the real Bukkit object on the Java side.
use horizon_bukkit::{Component, Location, Material, World};
use jni::JNIEnv;
use jni::errors::Result;
use jni::objects::{GlobalRef, JObject, JObjectArray, JString, JValue};
use jrt::V;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

pub const LOC: i32 = 1;
pub const VEC: i32 = 2;
pub const KEY: i32 = 3;
pub const JSON: i32 = 4;
pub const MAT: i32 = 5;
pub const LIST: i32 = 6;
pub const SET: i32 = 7;
pub const ARR: i32 = 8;
pub const MAP: i32 = 9;
pub const REF: i32 = 10;

pub struct Cache {
    pub h: GlobalRef,
    pub pack: GlobalRef,
    pub string: GlobalRef,
    pub integer: GlobalRef,
    pub long: GlobalRef,
    pub double: GlobalRef,
    pub float: GlobalRef,
    pub short: GlobalRef,
    pub byte: GlobalRef,
    pub boolean: GlobalRef,
    pub character: GlobalRef,
    pub object: GlobalRef,
    pub missing: GlobalRef,
}

static CACHE: OnceLock<Cache> = OnceLock::new();

pub fn cache() -> &'static Cache {
    CACHE.get().expect("conv::init ran")
}

pub fn init(env: &mut JNIEnv) -> Result<()> {
    let mut class = |name: &str| -> Result<GlobalRef> {
        let found = env.find_class(name)?;
        env.new_global_ref(found)
    };
    let cache = Cache {
        h: class("horizon/javalayer/H")?,
        pack: class("horizon/javalayer/Pack")?,
        string: class("java/lang/String")?,
        integer: class("java/lang/Integer")?,
        long: class("java/lang/Long")?,
        double: class("java/lang/Double")?,
        float: class("java/lang/Float")?,
        short: class("java/lang/Short")?,
        byte: class("java/lang/Byte")?,
        boolean: class("java/lang/Boolean")?,
        character: class("java/lang/Character")?,
        object: class("java/lang/Object")?,
        missing: {
            let native = env.find_class("horizon/javalayer/Native")?;
            let value = env.get_static_field(&native, "MISSING", "Ljava/lang/Object;")?.l()?;
            env.new_global_ref(value)?
        },
    };
    let _ = CACHE.set(cache);
    Ok(())
}

// ---- the objects Java holds ----

static HANDLES: Mutex<Option<HashMap<u64, V>>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

pub fn put(value: V) -> u64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    HANDLES.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().insert(id, value);
    id
}

pub fn get(id: u64) -> Option<V> {
    HANDLES.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(&id).cloned()
}

pub fn release(id: u64) {
    if let Some(table) = HANDLES.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        table.remove(&id);
    }
}

pub fn live_handles() -> usize {
    HANDLES.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(0, HashMap::len)
}

/// A Java object handed to the SDK (a plugin's `InventoryHolder`): Java keeps it by this id, the SDK keeps the id.
pub struct JavaRef(pub i64);

// ---- what each SDK type is in Bukkit ----

static KIND_MAP: OnceLock<HashMap<std::any::TypeId, &'static str>> = OnceLock::new();

pub fn kind_of(value: &V) -> Option<&'static str> {
    let V::Native(native) = value else {
        return match value {
            V::Obj(_) => Some("java.lang.Object"),
            _ => None,
        };
    };
    let map = KIND_MAP.get_or_init(|| crate::generated::KINDS.iter().map(|(java, id)| (id(), *java)).collect());
    let id = (&**native as &dyn std::any::Any).type_id();
    map.get(&id).copied()
}

fn text_of(value: jrt::R) -> Option<String> {
    match value {
        Ok(V::Null) | Err(_) => None,
        Ok(v) => Some(v.text()),
    }
}

/// The handle Java gets: id, Bukkit class, key (names the object for `equals`), extra (an entity's type, an
/// item's material, "console").
fn handle<'l>(env: &mut JNIEnv<'l>, value: &V) -> Result<JObject<'l>> {
    let kind = kind_of(value).unwrap_or("");
    let mut key = String::new();
    let mut extra = String::new();
    if kind == "org.bukkit.entity.Player" || kind == "org.bukkit.OfflinePlayer" {
        if let Some(uuid) = text_of(jrt::call(value, "getUniqueId", &[])) {
            key = format!("p:{uuid}");
        }
        extra = "PLAYER".to_owned();
    } else if kind.starts_with("org.bukkit.entity.") {
        if let Some(uuid) = text_of(jrt::call(value, "getUniqueId", &[])) {
            key = format!("e:{uuid}");
        }
        extra = text_of(jrt::call(value, "getType", &[])).unwrap_or_default();
        // A player met as a plain entity (getNearbyEntities, a chunk's entities): Java gets the Player itself.
        if extra.is_empty()
            && let Some(entity) = jbridge::held::<horizon_bukkit::entity::Entity>(value).map(|entity| entity.clone())
            && let Some(player) = horizon_bukkit::get_online_players().into_iter().find(|player| player.get_unique_id().replace('-', "") == entity.get_unique_id().replace('-', ""))
        {
            return handle(env, &jbridge::wrap(player));
        }
    } else if kind == "org.bukkit.World" {
        if let Some(name) = text_of(jrt::call(value, "getName", &[])) {
            key = format!("w:{name}");
        }
    } else if kind == "org.bukkit.inventory.ItemStack" {
        if let Ok(material) = jrt::call(value, "getType", &[]) {
            extra = match jbridge::held::<Material>(&material) {
                Some(m) => m.name(),
                None => material.text(),
            };
        }
    } else if kind == "org.bukkit.command.CommandSender" {
        extra = "console".to_owned();
    }
    let id = put(value.clone());
    let kind = env.new_string(kind)?;
    let key = env.new_string(key)?;
    let extra = env.new_string(extra)?;
    let class: &jni::objects::JClass = cache().h.as_obj().into();
    env.new_object(class, "(JLjava/lang/String;Ljava/lang/String;Ljava/lang/String;)V", &[JValue::Long(id as i64), JValue::Object(&kind), JValue::Object(&key), JValue::Object(&extra)])
}

#[allow(clippy::too_many_arguments)]
fn pack<'l>(env: &mut JNIEnv<'l>, tag: i32, o: &JObject, x: f64, y: f64, z: f64, f1: f32, f2: f32, s: Option<&str>, items: Option<&[V]>, vals: Option<&[V]>, id: i64) -> Result<JObject<'l>> {
    let text = match s {
        Some(s) => JObject::from(env.new_string(s)?),
        None => JObject::null(),
    };
    let list = match items {
        Some(items) => JObject::from(array(env, items)?),
        None => JObject::null(),
    };
    let second = match vals {
        Some(vals) => JObject::from(array(env, vals)?),
        None => JObject::null(),
    };
    let class: &jni::objects::JClass = cache().pack.as_obj().into();
    env.new_object(
        class,
        "(ILjava/lang/Object;DDDFFLjava/lang/String;[Ljava/lang/Object;[Ljava/lang/Object;J)V",
        &[JValue::Int(tag), JValue::Object(o), JValue::Double(x), JValue::Double(y), JValue::Double(z), JValue::Float(f1), JValue::Float(f2), JValue::Object(&text), JValue::Object(&list), JValue::Object(&second), JValue::Long(id)],
    )
}

pub fn array<'l>(env: &mut JNIEnv<'l>, items: &[V]) -> Result<JObjectArray<'l>> {
    let class: &jni::objects::JClass = cache().object.as_obj().into();
    let out = env.new_object_array(items.len() as i32, class, JObject::null())?;
    for (i, item) in items.iter().enumerate() {
        let value = to_java(env, item)?;
        env.set_object_array_element(&out, i as i32, &value)?;
        env.delete_local_ref(value)?;
    }
    Ok(out)
}

fn boxed<'l>(env: &mut JNIEnv<'l>, class: &GlobalRef, sig: &str, value: JValue) -> Result<JObject<'l>> {
    let class: &jni::objects::JClass = class.as_obj().into();
    env.call_static_method(class, "valueOf", sig, &[value])?.l()
}

pub fn to_java<'l>(env: &mut JNIEnv<'l>, value: &V) -> Result<JObject<'l>> {
    let c = cache();
    match value {
        V::Null => Ok(JObject::null()),
        V::Bool(b) => boxed(env, &c.boolean, "(Z)Ljava/lang/Boolean;", JValue::Bool(u8::from(*b))),
        V::Char(ch) => boxed(env, &c.character, "(C)Ljava/lang/Character;", JValue::Char(*ch as u32 as u16)),
        V::Int(i) => boxed(env, &c.integer, "(I)Ljava/lang/Integer;", JValue::Int(*i)),
        V::Long(l) => boxed(env, &c.long, "(J)Ljava/lang/Long;", JValue::Long(*l)),
        V::Double(d) => boxed(env, &c.double, "(D)Ljava/lang/Double;", JValue::Double(*d)),
        V::Str(s) => Ok(JObject::from(env.new_string(&**s)?)),
        V::Sb(s) => {
            let text = jrt::lock(s).clone();
            Ok(JObject::from(env.new_string(text)?))
        }
        V::Class(name) => Ok(JObject::from(env.new_string(*name)?)),
        V::Opt(inner) => match &**inner {
            Some(v) => to_java(env, v),
            None => Ok(JObject::null()),
        },
        V::Arr(_) => {
            let items = value.items().unwrap_or_default();
            pack(env, ARR, &JObject::null(), 0.0, 0.0, 0.0, 0.0, 0.0, None, Some(&items), None, 0)
        }
        V::List(_) | V::Stream(_) | V::Iter(_) => {
            let items = value.items().unwrap_or_default();
            pack(env, LIST, &JObject::null(), 0.0, 0.0, 0.0, 0.0, 0.0, None, Some(&items), None, 0)
        }
        V::Set(_) => {
            let items = value.items().unwrap_or_default();
            pack(env, SET, &JObject::null(), 0.0, 0.0, 0.0, 0.0, 0.0, None, Some(&items), None, 0)
        }
        V::Map(map) => {
            let (keys, vals): (Vec<V>, Vec<V>) = jrt::lock(map).values().map(|(k, v)| (k.clone(), v.clone())).unzip();
            pack(env, MAP, &JObject::null(), 0.0, 0.0, 0.0, 0.0, 0.0, None, Some(&keys), Some(&vals), 0)
        }
        V::Entry(pair) => {
            let items = [pair.0.clone(), pair.1.clone()];
            pack(env, LIST, &JObject::null(), 0.0, 0.0, 0.0, 0.0, 0.0, None, Some(&items), None, 0)
        }
        V::Native(native) => {
            if let Some(java) = native.downcast_ref::<JavaRef>() {
                return pack(env, REF, &JObject::null(), 0.0, 0.0, 0.0, 0.0, 0.0, None, None, None, java.0);
            }
            // A file (World.getWorldFolder): its path; Java makes the File or Path the method returns.
            if let Some(file) = native.downcast_ref::<jrt::javaio::JFile>() {
                return Ok(JObject::from(env.new_string(file.0.to_string_lossy())?));
            }
            if let Some(location) = jbridge::held::<Location>(value).map(|l| l.clone()) {
                let world = handle(env, &jbridge::wrap(location.world.clone()))?;
                return pack(env, LOC, &world, location.x, location.y, location.z, location.yaw, location.pitch, None, None, None, 0);
            }
            if let Some(v) = jbridge::held::<horizon_bukkit::util::Vector>(value).map(|v| (v.x, v.y, v.z)) {
                return pack(env, VEC, &JObject::null(), v.0, v.1, v.2, 0.0, 0.0, None, None, None, 0);
            }
            if let Some(json) = jbridge::held::<Component>(value).map(|c| c.0.to_string()) {
                return pack(env, JSON, &JObject::null(), 0.0, 0.0, 0.0, 0.0, 0.0, Some(&json), None, None, 0);
            }
            if let Some(name) = jbridge::held::<Material>(value).map(|m| m.name()) {
                return pack(env, MAT, &JObject::null(), 0.0, 0.0, 0.0, 0.0, 0.0, Some(&name), None, None, 0);
            }
            if let Some(key) = jbridge::held::<horizon_bukkit::persistence::NamespacedKey>(value).map(|k| format!("{}:{}", k.get_namespace(), k.get_key())) {
                return pack(env, KEY, &JObject::null(), 0.0, 0.0, 0.0, 0.0, 0.0, Some(&key), None, None, 0);
            }
            handle(env, value)
        }
        V::Exc(thrown) => Ok(JObject::from(env.new_string(format!("{}: {}", thrown.class, thrown.message.clone().unwrap_or_default()))?)),
        _ => handle(env, value),
    }
}

fn field_f64(env: &mut JNIEnv, o: &JObject, name: &str) -> Result<f64> {
    env.get_field(o, name, "D")?.d()
}

fn field_text(env: &mut JNIEnv, o: &JObject, name: &str) -> Result<String> {
    let value = env.get_field(o, name, "Ljava/lang/String;")?.l()?;
    if value.is_null() {
        return Ok(String::new());
    }
    let text: String = env.get_string(&JString::from(value))?.into();
    Ok(text)
}

fn items_of(env: &mut JNIEnv, o: &JObject, name: &str) -> Result<Vec<V>> {
    let value = env.get_field(o, name, "[Ljava/lang/Object;")?.l()?;
    if value.is_null() {
        return Ok(Vec::new());
    }
    from_array(env, &JObjectArray::from(value))
}

pub fn from_array(env: &mut JNIEnv, array: &JObjectArray) -> Result<Vec<V>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let length = env.get_array_length(array)?;
    let mut out = Vec::with_capacity(length as usize);
    for i in 0..length {
        let item = env.get_object_array_element(array, i)?;
        out.push(from_java(env, &item)?);
        env.delete_local_ref(item)?;
    }
    Ok(out)
}

fn default_world() -> Option<World> {
    horizon_bukkit::get_worlds().into_iter().next()
}

pub fn from_java(env: &mut JNIEnv, o: &JObject) -> Result<V> {
    if o.is_null() {
        return Ok(V::Null);
    }
    let c = cache();
    if env.is_instance_of(o, &c.string)? {
        let text: String = env.get_string(&JString::from(env.new_local_ref(o)?))?.into();
        return Ok(jrt::s(&text));
    }
    if env.is_instance_of(o, &c.h)? {
        let id = env.get_field(o, "id", "J")?.j()?;
        return Ok(get(id as u64).unwrap_or(V::Null));
    }
    if env.is_instance_of(o, &c.pack)? {
        let tag = env.get_field(o, "tag", "I")?.i()?;
        return match tag {
            LOC => {
                let world_obj = env.get_field(o, "o", "Ljava/lang/Object;")?.l()?;
                let world = from_java(env, &world_obj)?;
                let world = jbridge::held::<World>(&world).map(|w| w.clone()).or_else(default_world);
                let (x, y, z) = (field_f64(env, o, "x")?, field_f64(env, o, "y")?, field_f64(env, o, "z")?);
                let (yaw, pitch) = (env.get_field(o, "f1", "F")?.f()?, env.get_field(o, "f2", "F")?.f()?);
                Ok(match world {
                    Some(world) => jbridge::wrap(Location::new(world, x, y, z, yaw, pitch)),
                    None => V::Null,
                })
            }
            VEC => Ok(jbridge::wrap(horizon_bukkit::util::Vector::new(field_f64(env, o, "x")?, field_f64(env, o, "y")?, field_f64(env, o, "z")?))),
            KEY => {
                let text = field_text(env, o, "s")?;
                Ok(match horizon_bukkit::persistence::NamespacedKey::from_string(&text) {
                    Some(key) => jbridge::wrap(key),
                    None => jrt::s(&text),
                })
            }
            JSON => {
                let text = field_text(env, o, "s")?;
                Ok(jbridge::wrap(Component(serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)))))
            }
            MAT => Ok(jbridge::wrap(Material::of(&field_text(env, o, "s")?))),
            LIST | SET => Ok(jrt::list(items_of(env, o, "items")?)),
            ARR => Ok(jrt::arr(items_of(env, o, "items")?)),
            MAP => {
                let keys = items_of(env, o, "items")?;
                let vals = items_of(env, o, "vals")?;
                Ok(jrt::map_of(keys.into_iter().zip(vals).collect()))
            }
            REF => Ok(jrt::native(JavaRef(env.get_field(o, "id", "J")?.j()?))),
            _ => Ok(V::Null),
        };
    }
    if env.is_instance_of(o, &c.integer)? || env.is_instance_of(o, &c.short)? || env.is_instance_of(o, &c.byte)? {
        return Ok(V::Int(env.call_method(o, "intValue", "()I", &[])?.i()?));
    }
    if env.is_instance_of(o, &c.long)? {
        return Ok(V::Long(env.call_method(o, "longValue", "()J", &[])?.j()?));
    }
    if env.is_instance_of(o, &c.double)? || env.is_instance_of(o, &c.float)? {
        return Ok(V::Double(env.call_method(o, "doubleValue", "()D", &[])?.d()?));
    }
    if env.is_instance_of(o, &c.boolean)? {
        return Ok(V::Bool(env.call_method(o, "booleanValue", "()Z", &[])?.z()?));
    }
    if env.is_instance_of(o, &c.character)? {
        let ch = env.call_method(o, "charValue", "()C", &[])?.c()?;
        return Ok(V::Char(char::from_u32(u32::from(ch)).unwrap_or('?')));
    }
    Ok(V::Null)
}
