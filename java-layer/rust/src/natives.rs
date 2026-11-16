//! What Java calls: `horizon.javalayer.Native`'s methods.
use crate::conv::{self, from_array, from_java, to_java};
use jni::JNIEnv;
use jni::NativeMethod;
use jni::objects::{JClass, JObject, JObjectArray, JString};
use jni::sys::{jboolean, jint, jlong, jobject};
use jrt::{R, V};
use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

static MAIN_THREAD: OnceLock<std::thread::ThreadId> = OnceLock::new();

pub fn register(env: &mut JNIEnv, class: &JClass) -> jni::errors::Result<()> {
    let _ = MAIN_THREAD.set(std::thread::current().id());
    const CALL: &str = "(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;";
    const STATIC: &str = "(Ljava/lang/String;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;";
    let method = |name: &str, sig: &str, f: *mut c_void| NativeMethod { name: name.into(), sig: sig.into(), fn_ptr: f };
    // One at a time: a jar older than this DLL (a method it lacks) costs that method, never the whole layer.
    let all = [
            method("call", CALL, call as *mut c_void),
            method("tryCall", CALL, try_call as *mut c_void),
            method("callStatic", STATIC, call_static as *mut c_void),
            method("tryStatic", STATIC, try_static as *mut c_void),
            method("construct", "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;", construct as *mut c_void),
            method("release", "(J)V", release as *mut c_void),
            method("log", "(ILjava/lang/String;)V", log as *mut c_void),
            method("registerCommand", "(Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z", register_command as *mut c_void),
            method("wantEvent", "(Ljava/lang/String;I)Z", want_event as *mut c_void),
            method("addPermission", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V", add_permission as *mut c_void),
            method("isMainThread", "()Z", is_main_thread as *mut c_void),
            method("pumpQueries", "()V", pump_queries as *mut c_void),
            method("handles", "()I", handles as *mut c_void),
            method("eventClasses", "()[Ljava/lang/String;", event_classes as *mut c_void),
            method("field", "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/Object;", static_field as *mut c_void),
            method("tag", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/Object;", tag_query as *mut c_void),
            method("query", "(Ljava/lang/String;)Ljava/lang/String;", engine_query as *mut c_void),
            method("valueJson", "(Ljava/lang/Object;)Ljava/lang/String;", value_json as *mut c_void),
            method("setComponentJson", "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/String;)Z", set_component_json as *mut c_void),
            method("profileProperties", "(Ljava/lang/Object;)Ljava/lang/String;", profile_properties as *mut c_void),
            method("itemToJson", "(Ljava/lang/Object;)Ljava/lang/String;", item_to_json as *mut c_void),
            method("itemFromJson", "(Ljava/lang/String;)Ljava/lang/Object;", item_from_json as *mut c_void),
            method("componentJson", "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/String;", component_json as *mut c_void),
            method("addRecipeJson", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/Object;)Z", add_recipe_json as *mut c_void),
    ];
    let mut missing = Vec::new();
    for one in all {
        let name = one.name.to_str().map(str::to_owned).unwrap_or_default();
        if env.register_native_methods(class, std::slice::from_ref(&one)).is_err() {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
            missing.push(name);
        }
    }
    if !missing.is_empty() {
        eprintln!("[JavaLayer] horizon-javalayer.jar is older than this DLL (no {}): rebuild java-layer/lib", missing.join(", "));
    }
    Ok(())
}

/// Java's exception for what the SDK threw. A call the SDK doesn't have says so plainly.
fn throw(env: &mut JNIEnv, thrown: &V) {
    let (class, message) = match thrown {
        V::Exc(t) => (t.class.clone(), t.message.clone().unwrap_or_default()),
        other => ("RuntimeException".to_owned(), other.text()),
    };
    if env.exception_check().unwrap_or(false) {
        return;
    }
    let java = match class.as_str() {
        "IllegalArgumentException" | "IllegalStateException" | "NullPointerException" | "UnsupportedOperationException" | "IndexOutOfBoundsException"
        | "ArrayIndexOutOfBoundsException" | "NumberFormatException" | "ClassCastException" | "ArithmeticException" | "StringIndexOutOfBoundsException" => format!("java/lang/{class}"),
        "IOException" | "FileNotFoundException" => format!("java/io/{class}"),
        "NoSuchMethodError" => {
            let _ = env.throw_new("java/lang/UnsupportedOperationException", format!("Horizon's Java layer can't do this yet: {}", message.replace(" is not converted", "")));
            return;
        }
        _ => "java/lang/RuntimeException".to_owned(),
    };
    let text = if java == "java/lang/RuntimeException" { format!("{class}: {message}") } else { message };
    let _ = env.throw_new(java, text);
}

fn missing(thrown: &V) -> bool {
    matches!(thrown, V::Exc(t) if t.class == "NoSuchMethodError" && t.message.as_deref().is_some_and(|m| m.ends_with("is not converted")))
}

/// A native's body: a panic or a JNI error becomes Java's exception, never a crashed server.
fn guard<'l>(env: &mut JNIEnv<'l>, body: impl FnOnce(&mut JNIEnv<'l>) -> jni::errors::Result<JObject<'l>>) -> jobject {
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(env)));
    match out {
        Ok(Ok(value)) => value.into_raw(),
        Ok(Err(jni::errors::Error::JavaException)) => std::ptr::null_mut(),
        Ok(Err(error)) => {
            if !env.exception_check().unwrap_or(false) {
                let _ = env.throw_new("java/lang/RuntimeException", format!("Horizon Java layer: {error}"));
            }
            std::ptr::null_mut()
        }
        Err(panic) => {
            let text = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned())).unwrap_or_default();
            if !env.exception_check().unwrap_or(false) {
                let _ = env.throw_new("java/lang/RuntimeException", format!("Horizon Java layer panicked: {text}"));
            }
            std::ptr::null_mut()
        }
    }
}

fn answer<'l>(env: &mut JNIEnv<'l>, out: R, soft: bool) -> jni::errors::Result<JObject<'l>> {
    match out {
        Ok(value) => to_java(env, &value),
        Err(thrown) if soft && missing(&thrown) => env.new_local_ref(conv::cache().missing.as_obj()),
        Err(thrown) => {
            throw(env, &thrown);
            Ok(JObject::null())
        }
    }
}

fn text(env: &mut JNIEnv, s: &JString) -> jni::errors::Result<String> {
    if s.is_null() {
        return Ok(String::new());
    }
    Ok(env.get_string(s)?.into())
}

/// HORIZON_JAVA_PROFILE=<file>: every Java->native call counted and timed by its method name (and the receiver's
/// Rust type), written to that file as JSON every 2000 calls and read by test/jcalls.py. Off (no cost but one load)
/// without the variable.
static PROFILE: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
static CALLS: Mutex<Option<std::collections::HashMap<String, (u64, u128)>>> = Mutex::new(None);

fn profiled<T>(label: impl FnOnce() -> String, work: impl FnOnce() -> T) -> T {
    let Some(path) = PROFILE.get_or_init(|| std::env::var_os("HORIZON_JAVA_PROFILE").map(std::path::PathBuf::from)) else {
        return work();
    };
    let started = std::time::Instant::now();
    let out = work();
    let took = started.elapsed().as_nanos();
    let mut all = CALLS.lock().unwrap_or_else(|e| e.into_inner());
    let all = all.get_or_insert_with(Default::default);
    let row = all.entry(label()).or_insert((0, 0));
    row.0 += 1;
    row.1 += took;
    let total: u64 = all.values().map(|r| r.0).sum();
    if total % 2000 == 0 {
        let rows: serde_json::Map<String, serde_json::Value> = all.iter().map(|(k, (n, ns))| (k.clone(), serde_json::json!([n, *ns as f64 / 1e6]))).collect();
        let _ = std::fs::write(path, serde_json::Value::Object(rows).to_string());
    }
    out
}

fn on_object<'l>(env: &mut JNIEnv<'l>, this: JObject<'l>, name: JString<'l>, args: JObjectArray<'l>, soft: bool) -> jobject {
    // Other threads' calls waiting on the main thread are answered at each of its own: a long stretch of main-thread
    // work (a plugin's big onEnable, a slow command) doesn't leave them waiting out their time, as on Paper, where
    // they need no main thread at all.
    jbridge::pump_hops();
    guard(env, |env| {
        let this = from_java(env, &this)?;
        let name = text(env, &name)?;
        let args = from_array(env, &args)?;
        if this.is_null() {
            return answer(env, Err(jrt::exc("NullPointerException", format!("{name} on a released object"))), false);
        }
        let this = unlocked(&this);
        let out = profiled(|| format!("{}.{name}/{}", jrt::class_of(&this), args.len()), || jrt::call(&this, &name, &args));
        answer(env, out, soft)
    })
}

/// A game object that is only a name for something the engine holds (a player, an entity, a world, a block): the
/// call runs on a copy, so its lock is never held across the call. A call that fires an event (teleport) whose
/// listener asks the same player something would otherwise wait on itself forever.
fn unlocked(this: &V) -> V {
    macro_rules! copy {
        ($($t:ty),*) => {$(
            if let Some(held) = this.native::<std::sync::Mutex<$t>>() {
                if let Ok(inner) = held.try_lock() {
                    return jrt::native(std::sync::Mutex::new(inner.clone()));
                }
            }
        )*};
    }
    copy!(horizon_bukkit::Player, horizon_bukkit::entity::Entity, horizon_bukkit::World, horizon_bukkit::Block, horizon_bukkit::OfflinePlayer);
    this.clone()
}

extern "system" fn call<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, this: JObject<'l>, name: JString<'l>, args: JObjectArray<'l>) -> jobject {
    on_object(&mut env, this, name, args, false)
}

extern "system" fn try_call<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, this: JObject<'l>, name: JString<'l>, args: JObjectArray<'l>) -> jobject {
    on_object(&mut env, this, name, args, true)
}

fn on_class<'l>(env: &mut JNIEnv<'l>, class: JString<'l>, name: JString<'l>, args: JObjectArray<'l>, soft: bool) -> jobject {
    jbridge::pump_hops();
    guard(env, |env| {
        let class = text(env, &class)?;
        let name = text(env, &name)?;
        let args = from_array(env, &args)?;
        let out = profiled(|| format!("static {class}.{name}/{}", args.len()), || jrt::call_static(&class, &name, &args));
        answer(env, out, soft)
    })
}

extern "system" fn call_static<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, class: JString<'l>, name: JString<'l>, args: JObjectArray<'l>) -> jobject {
    on_class(&mut env, class, name, args, false)
}

extern "system" fn try_static<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, class: JString<'l>, name: JString<'l>, args: JObjectArray<'l>) -> jobject {
    on_class(&mut env, class, name, args, true)
}

extern "system" fn construct<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, class: JString<'l>, args: JObjectArray<'l>) -> jobject {
    guard(&mut env, |env| {
        let class = text(env, &class)?;
        let args = from_array(env, &args)?;
        let out = jrt::new_other(&class, &args);
        answer(env, out, true)
    })
}

extern "system" fn release(_env: JNIEnv, _class: JClass, id: jlong) {
    conv::release(id as u64);
}

extern "system" fn handles(_env: JNIEnv, _class: JClass) -> jint {
    conv::live_handles() as jint
}

/// The queries other threads wait on, answered now (the start enables plugins for a long while between ticks).
extern "system" fn pump_queries(_env: JNIEnv, _class: JClass) {
    jbridge::pump_hops();
}

extern "system" fn is_main_thread(_env: JNIEnv, _class: JClass) -> jboolean {
    u8::from(MAIN_THREAD.get() == Some(&std::thread::current().id()))
}

extern "system" fn log<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, level: jint, line: JString<'l>) {
    let Ok(line) = text(&mut env, &line) else { return };
    let Some(java) = crate::jvm::java() else {
        eprintln!("{line}");
        return;
    };
    match level {
        0 => java.plugin.info(&line),
        1 => java.plugin.warning(&line),
        _ => java.plugin.severe(&line),
    }
}

fn strings(env: &mut JNIEnv, array: &JObjectArray) -> jni::errors::Result<Vec<String>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let length = env.get_array_length(array)?;
    let mut out = Vec::new();
    for i in 0..length {
        let item = env.get_object_array_element(array, i)?;
        out.push(text(env, &JString::from(item))?);
    }
    Ok(out)
}

extern "system" fn register_command<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, plugin: JString<'l>, name: JString<'l>, aliases: JObjectArray<'l>, description: JString<'l>, usage: JString<'l>, permission: JString<'l>) -> jboolean {
    let read = (|| -> jni::errors::Result<_> {
        Ok((text(&mut env, &plugin)?, text(&mut env, &name)?, strings(&mut env, &aliases)?, text(&mut env, &description)?, text(&mut env, &usage)?, text(&mut env, &permission)?))
    })();
    let Ok((owner, name, aliases, description, usage, permission)) = read else { return 0 };
    let Some(java) = crate::jvm::java() else { return 0 };
    let host = java.plugin.clone();
    let mut command = horizon_bukkit::pure2::Command::new(&name);
    command.set_aliases(&aliases.iter().map(String::as_str).collect::<Vec<_>>());
    if !description.is_empty() {
        command.set_description(&description);
    }
    if !usage.is_empty() {
        command.set_usage(&usage);
    }
    // The permission stays Java's: Bukkit's PluginCommand.execute and the command map's tab completion test it with
    // the player's Permissible (children of `default: op` parents, attachments, a permission plugin's own). The
    // server's check came first and knew only each node's own default: an op was refused PvPManager's commands
    // (`pvpmanager.command.announce` is default false, given to ops by `pvpmanager.*`).
    let _ = permission;
    let registered = horizon_bukkit::fam_core::get_command_map().register(&host, &owner.to_lowercase(), &command);
    let (run_owner, run_name) = (owner.clone(), name.clone());
    host.set_executor(&name, move |sender, label, args| crate::events::command(&run_owner, &run_name, sender, label, args));
    let (tab_owner, tab_name) = (owner, name);
    host.set_tab_completer(&tab_name.clone(), move |sender, alias, args| crate::events::tab(&tab_owner, &tab_name, sender, alias, args));
    u8::from(registered)
}

static WANTED: Mutex<Option<HashSet<(String, u8)>>> = Mutex::new(None);

extern "system" fn want_event<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, class: JString<'l>, slot: jint) -> jboolean {
    let Ok(class) = text(&mut env, &class) else { return 0 };
    let slot = slot.clamp(0, 5) as u8;
    let mut wanted = WANTED.lock().unwrap_or_else(|e| e.into_inner());
    let set = wanted.get_or_insert_default();
    if set.contains(&(class.clone(), slot)) {
        return 1;
    }
    let Some(java) = crate::jvm::java() else { return 0 };
    let known = crate::generated::register_event(&class, slot, &java.plugin);
    if known {
        set.insert((class, slot));
    }
    u8::from(known)
}

extern "system" fn add_permission<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, name: JString<'l>, default: JString<'l>, description: JString<'l>) {
    let (Ok(name), Ok(default), Ok(description)) = (text(&mut env, &name), text(&mut env, &default), text(&mut env, &description)) else { return };
    let permission = horizon_bukkit::config2::Permission::new(&name, &default, &description);
    horizon_bukkit::fam_core::get_plugin_manager().add_permission(&permission);
}

// ---- the SDK calling back into Java: a Java object it was handed (a Runnable, a listener, a holder) ----

/// A call on a Java object the SDK holds: `Bridge.invokeRef(id, name, args)`; "*" is its one abstract method.
pub fn call_java(id: i64, name: &str, args: &[V]) -> R {
    let Some(java) = crate::jvm::java() else { return Err(jrt::exc("IllegalStateException", "Java is not running")) };
    let Ok(mut env) = java.vm.attach_current_thread_permanently() else { return Err(jrt::exc("IllegalStateException", "no Java thread")) };
    let class: &JClass = java.bridge.as_obj().into();
    let out = env.with_local_frame(32, |env| -> jni::errors::Result<R> {
        let name = env.new_string(name)?;
        let list = conv::array(env, args)?;
        let result = env.call_static_method(class, "invokeRef", "(JLjava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;", &[jni::objects::JValue::Long(id), jni::objects::JValue::Object(&name), jni::objects::JValue::Object(&list)]);
        match result {
            Ok(value) => {
                let object = value.l()?;
                Ok(Ok(from_java(env, &object)?))
            }
            Err(jni::errors::Error::JavaException) => {
                let thrown = env.exception_occurred()?;
                env.exception_clear()?;
                let text = env.call_method(&thrown, "toString", "()Ljava/lang/String;", &[])?.l()?;
                let text: String = env.get_string(&JString::from(text))?.into();
                Ok(Err(jrt::exc("RuntimeException", text)))
            }
            Err(other) => Err(other),
        }
    });
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    out.unwrap_or_else(|e| Err(jrt::exc("RuntimeException", e.to_string())))
}

pub fn layer_call(r: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(java) = r.native::<conv::JavaRef>() {
        return Some(call_java(java.0, name, a));
    }
    // A Java MapRenderer added to / taken off a map (MapRenderers.java): drawn by the SDK's map loop.
    // A game recipe's key and data-pack JSON, for the Bukkit recipe classes Java makes of it (Recipes.java).
    if name == "$recipeJson" && let Some(recipe) = jbridge::held::<horizon_bukkit::core2::GameRecipe>(r).map(|g| g.clone()) {
        return Some(Ok(V::Arr(std::sync::Arc::new(Mutex::new(vec![jrt::s(&recipe.key), jrt::s(&recipe.data.to_string())])))));
    }
    // Enchantment.getPrimaryItems: null where the data names none (Paper), not an empty set.
    if name == "getPrimaryItems" && a.is_empty() && let Some(e) = jbridge::held::<horizon_bukkit::registry::Enchantment>(r).map(|e| e.clone()) {
        return Some(Ok(match e.primary_items() { Some(items) => jrt::list(items.iter().map(|x| jrt::s(x)).collect()), None => V::Null }));
    }
    if name == "$mapGameRendererOff" && let Some(map) = jbridge::held::<horizon_bukkit::fam_core::MapView>(r).map(|m| *m) {
        return Some(Ok(V::Bool(crate::adapters::game_renderer_off(&map))));
    }
    if (name == "$mapRendererAdd" || name == "$mapRendererRemove") && let Some(map) = jbridge::held::<horizon_bukkit::fam_core::MapView>(r).map(|m| *m) {
        if name == "$mapRendererAdd" && a.len() == 3 {
            crate::adapters::add_map_renderer(&map, a[0].clone(), a[1].long() as i32, a[2].truth());
            return Some(Ok(V::Null));
        }
        if a.len() == 1 {
            return Some(Ok(V::Bool(crate::adapters::remove_map_renderer(&map, a[0].long() as i32))));
        }
    }
    crate::generated::sdk_call(r, name, a)
}

/// `instanceof` on a Java object the SDK holds: Java answers (a class or interface name, simple or full).
pub fn layer_instance(v: &V, name: &str) -> bool {
    if let Some(java) = v.native::<conv::JavaRef>() {
        return call_java(java.0, "$instanceof", &[jrt::s(name)]).is_ok_and(|v| v.truth());
    }
    crate::generated::sdk_instance(v, name)
}

pub extern "system" fn event_classes<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>) -> jobject {
    let names: Vec<String> = crate::generated::EVENT_CLASSES.iter().map(|s| (*s).to_owned()).collect();
    match crate::jvm::strings(&mut env, &names) {
        Ok(array) => array.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// A static field of a class the SDK or a bridge stands for (`PacketType.Play.Server.MAP_CHUNK`); MISSING when none.
extern "system" fn static_field<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, class: JString<'l>, name: JString<'l>) -> jobject {
    guard(&mut env, |env| {
        let class = text(env, &class)?;
        let name = text(env, &name)?;
        match crate::generated::sdk_field(&class, &name) {
            Some(value) => to_java(env, &value),
            None => env.new_local_ref(conv::cache().missing.as_obj()),
        }
    })
}

/// The JSON of an SDK value object (a built data component, a dialog part), or null if it is none.
extern "system" fn value_json<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, object: JObject<'l>) -> jobject {
    guard(&mut env, |env| {
        let value = crate::conv::from_java(env, &object)?;
        match crate::components_gen::json_of(&value) {
            Some(json) => Ok(JObject::from(env.new_string(json.to_string())?)),
            None => Ok(JObject::null()),
        }
    })
}

/// Paper's `setData(type, value)` with the value as JSON (what `valueJson` gave, or a scalar's).
extern "system" fn set_component_json<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, item: JObject<'l>, id: JString<'l>, json: JString<'l>) -> jboolean {
    let done = (|| -> jni::errors::Result<bool> {
        let id = text(&mut env, &id)?;
        let json = text(&mut env, &json)?;
        let value = crate::conv::from_java(&mut env, &item)?;
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
        Ok(match jbridge::held::<horizon_bukkit::ItemStack>(&value) {
            Some(mut stack) => {
                stack.set_component(&id, parsed);
                true
            }
            None => false,
        })
    })();
    jboolean::from(done.unwrap_or(false))
}

/// A PlayerProfile's properties (name, value, signature) as JSON.
extern "system" fn profile_properties<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, profile: JObject<'l>) -> jobject {
    guard(&mut env, |env| {
        let value = crate::conv::from_java(env, &profile)?;
        let json = jbridge::held::<horizon_bukkit::player::PlayerProfile>(&value)
            .map(|p| serde_json::Value::Array(p.get_properties().iter().map(|(n, v, s)| serde_json::json!([n, v, s])).collect()).to_string());
        match json {
            Some(text) => Ok(JObject::from(env.new_string(text)?)),
            None => Ok(JObject::null()),
        }
    })
}

/// An item as the game's JSON (id, count, components).
extern "system" fn item_to_json<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, item: JObject<'l>) -> jobject {
    guard(&mut env, |env| {
        let value = crate::conv::from_java(env, &item)?;
        match jbridge::held::<horizon_bukkit::ItemStack>(&value).map(|s| s.to_json().to_string()) {
            Some(text) => Ok(JObject::from(env.new_string(text)?)),
            None => Ok(JObject::null()),
        }
    })
}

/// An item from the game's JSON: a server item handle, or null.
extern "system" fn item_from_json<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, json: JString<'l>) -> jobject {
    guard(&mut env, |env| {
        let text = text(env, &json)?;
        let value: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        match horizon_bukkit::ItemStack::read(&value) {
            Some(stack) => to_java(env, &jbridge::wrap(stack)),
            None => Ok(JObject::null()),
        }
    })
}

/// An item's data component as JSON text (null when the item doesn't have it): Paper's `getData`.
extern "system" fn component_json<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, item: JObject<'l>, id: JString<'l>) -> jobject {
    guard(&mut env, |env| {
        let id = text(env, &id)?;
        let value = crate::conv::from_java(env, &item)?;
        let json = jbridge::held::<horizon_bukkit::ItemStack>(&value).and_then(|stack| stack.get_component(&id).map(|v| v.to_string()));
        match json {
            Some(text) => Ok(JObject::from(env.new_string(text)?)),
            None => Ok(JObject::null()),
        }
    })
}

/// `Server.addRecipe` of any Bukkit recipe: its JSON made in Java, the result item from its handle.
extern "system" fn add_recipe_json<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, namespace: JString<'l>, key: JString<'l>, json: JString<'l>, result: JObject<'l>) -> jboolean {
    let done = (|| -> jni::errors::Result<bool> {
        let namespace = text(&mut env, &namespace)?;
        let key = text(&mut env, &key)?;
        let json = text(&mut env, &json)?;
        let value: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
        let item = if result.is_null() { None } else { crate::conv::from_java(&mut env, &result).ok() };
        let stack = item.as_ref().and_then(|v| jbridge::held::<horizon_bukkit::ItemStack>(v).map(|s| s.clone()));
        Ok(horizon_bukkit::extra::add_recipe_json(&namespace, &key, value, stack.as_ref()))
    })();
    jboolean::from(done.unwrap_or(false))
}

/// A query to the engine (`horizon_mod::query`), JSON in and out: what the NMS bridge and packet layers use for
/// things with no Bukkit name (raw packets to a player, packet ids, entity ids).
extern "system" fn engine_query<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, json: JString<'l>) -> jobject {
    guard(&mut env, |env| {
        let text = text(env, &json)?;
        let value: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        // The layer's own: the game's biome as WorldInfo.vanillaBiomeProvider gives it (while a chunk generates,
        // the one the game just asked about), and every biome it makes.
        let answer = match value["op"].as_str() {
            Some("layer_vanilla_biome") => {
                let n = |k: &str| value[k].as_i64().unwrap_or(0) as i32;
                serde_json::Value::from(horizon_bukkit::worlds::vanilla_biome_at(n("x"), n("y"), n("z")))
            }
            Some("layer_vanilla_biomes") => serde_json::Value::from(horizon_bukkit::worlds::vanilla_biomes()),
            _ => horizon_bukkit::horizon_mod::query(value),
        };
        Ok(JObject::from(env.new_string(answer.to_string())?))
    })
}

/// `Tag.isTagged(value)` (value given) or `Tag.getValues()` (value null) of the game's tag `name` in a folder
/// (`block`, `item`, `fluid`, `entity_type`, `game_event`).
extern "system" fn tag_query<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, kind: JString<'l>, name: JString<'l>, value: JString<'l>) -> jobject {
    guard(&mut env, |env| {
        let kind = text(env, &kind)?;
        let name = text(env, &name)?;
        let folder: &'static str = match kind.as_str() {
            "blocks" | "block" => "block",
            "items" | "item" => "item",
            "fluids" | "fluid" => "fluid",
            "entity_types" | "entity_type" => "entity_type",
            "game_events" | "game_event" => "game_event",
            _ => return Ok(JObject::null()),
        };
        let tag = horizon_bukkit::registry::Tag::get(folder, &name);
        if value.is_null() {
            let values = tag.get_values();
            return Ok(JObject::from(crate::jvm::strings(env, &values)?));
        }
        let value = text(env, &value)?;
        to_java(env, &V::Bool(tag.is_tagged(&value)))
    })
}
