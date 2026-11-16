//! Plugins using plugins: Bukkit's `ServicesManager` across converted plugins and hand-written ports.
//!
//! Each plugin is a library of its own, so an object can't be handed from one to another. A service is
//! an SDK service (`horizon_bukkit::persistence`): a name, and a call by method name with JSON both ways.
//! The JSON is plain, as the hand-written ports already read it (Essentials' Vault economy): numbers,
//! text, lists, a player by name (Vault's String overloads take it), a record as an object of its fields.

use horizon_bukkit::persistence::{provide_service, service_registrations, ServiceProvider};
use jrt::{R, V};
use serde_json::{json, Map, Value};

/// A service by Bukkit's class name: Vault's three by the full names their providers register.
pub fn service_name(class: &str) -> String {
    match class.rsplit('.').next().unwrap_or(class) {
        "Economy" => "net.milkbowl.vault.economy.Economy".to_owned(),
        "Permission" => "net.milkbowl.vault.permission.Permission".to_owned(),
        "Chat" => "net.milkbowl.vault.chat.Chat".to_owned(),
        _ => class.to_owned(),
    }
}

fn class_of(v: &V) -> String {
    match v {
        V::Class(name) => (*name).to_owned(),
        other => other.text(),
    }
}

/// Everyone providing a service, by its full or simple name, the highest priority first.
fn providers(class: &str) -> Vec<ServiceProvider> {
    let full = service_name(class);
    let mut found = service_registrations(&full);
    if found.is_empty() && full == class {
        let simple = class.rsplit('.').next().unwrap_or(class);
        for name in horizon_bukkit::persistence::service_names() {
            if name == simple || name.ends_with(&format!(".{simple}")) {
                found.extend(service_registrations(&name));
            }
        }
    }
    found
}

/// `getProvider()` of a registration: its methods are calls into the plugin that provides it.
pub struct ProxyH(ServiceProvider);
/// `RegisteredServiceProvider`.
pub struct RegistrationH(ServiceProvider);
/// A record another plugin answered with: its getters and fields read the object's keys.
pub struct JsonH(Value);

/// What a service in any plugin reads.
pub fn to_plain(v: &V) -> Value {
    to_plain_at(v, 0)
}

fn to_plain_at(v: &V, depth: u8) -> Value {
    if depth > 6 {
        return Value::Null;
    }
    match v {
        V::Null => Value::Null,
        V::Bool(b) => json!(b),
        V::Int(i) => json!(i),
        V::Long(l) => json!(l),
        V::Double(d) => json!(d),
        V::Char(c) => json!(c.to_string()),
        V::Str(text) => json!(&**text),
        V::List(_) | V::Arr(_) | V::Set(_) => Value::Array(v.items().unwrap_or_default().iter().map(|x| to_plain_at(x, depth + 1)).collect()),
        V::Map(map) => {
            let mut out = Map::new();
            for (key, value) in jrt::lock(map).values() {
                out.insert(key.text(), to_plain_at(value, depth + 1));
            }
            Value::Object(out)
        }
        // A record of the plugin's own (Vault's EconomyResponse): its fields.
        V::Obj(object) => {
            let mut out = Map::new();
            for (key, value) in jrt::lock(&object.fields).iter() {
                if !key.starts_with("__") && !key.starts_with("this$") {
                    out.insert((*key).to_owned(), to_plain_at(value, depth + 1));
                }
            }
            // `type == SUCCESS` is how Vault's EconomyResponse says it worked; Essentials says "success".
            if let Some(Value::String(kind)) = out.get("type") {
                let ok = kind == "SUCCESS";
                out.entry("success").or_insert(json!(ok));
            }
            Value::Object(out)
        }
        _ => {
            if let Some(player) = crate::held::<horizon_bukkit::Player>(v) {
                return json!(player.get_name());
            }
            if let Some(json) = v.native::<JsonH>() {
                return json.0.clone();
            }
            json!(v.text())
        }
    }
}

/// What a plugin's Java reads of plain JSON.
pub fn from_plain(value: &Value) -> V {
    match value {
        Value::Null => V::Null,
        Value::Bool(b) => V::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) if i32::try_from(i).is_ok() && !n.is_f64() => V::Int(i as i32),
            Some(i) if !n.is_f64() => V::Long(i),
            _ => V::Double(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(text) => jrt::s(text),
        Value::Array(items) => jrt::list(items.iter().map(from_plain).collect()),
        Value::Object(_) => jrt::native(JsonH(value.clone())),
    }
}

/// `ServicesManager` and its registrations: `None` leaves the call to the SDK's own answers.
pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(proxy) = this.native::<ProxyH>() {
        if matches!(name, "hashCode" | "equals" | "toString") {
            return Some(Ok(match name {
                "toString" => jrt::s(&format!("{} from {}", proxy.0.get_service(), proxy.0.get_plugin_name())),
                "equals" => V::Bool(a[0].native::<ProxyH>().is_some_and(|other| other.0.get_service() == proxy.0.get_service())),
                _ => V::Int(0),
            }));
        }
        let answer = proxy.0.call(name, &Value::Array(a.iter().map(to_plain).collect()));
        if let Some(problem) = answer.get("exception").and_then(Value::as_str) {
            return Some(Err(jrt::exc("RuntimeException", problem.to_owned())));
        }
        return Some(Ok(from_plain(&answer)));
    }
    if let Some(registration) = this.native::<RegistrationH>() {
        return Some(Ok(match name {
            "getProvider" => provider_of(&registration.0),
            "getPriority" => jrt::s(&registration.0.get_priority().to_ascii_uppercase()),
            "getService" => jrt::s(registration.0.get_service()),
            "getPlugin" => jrt::s(registration.0.get_plugin_name()),
            _ => return None,
        }));
    }
    if let Some(record) = this.native::<JsonH>() {
        return Some(Ok(record_answer(&record.0, name, a)));
    }
    if this.native::<std::sync::Mutex<horizon_bukkit::persistence::ServicesManager>>().is_some() {
        return services_manager(name, a);
    }
    None
}

fn record_answer(record: &Value, name: &str, a: &[V]) -> V {
    let key = if name == "$field" { a.first().map(V::text).unwrap_or_default() } else { name.to_owned() };
    let lower_first = |s: &str| {
        let mut chars = s.chars();
        chars.next().map_or_else(String::new, |c| c.to_ascii_lowercase().to_string() + chars.as_str())
    };
    let wanted = match key.as_str() {
        "transactionSuccess" => "success".to_owned(),
        other => other.strip_prefix("get").or_else(|| other.strip_prefix("is")).map(lower_first).unwrap_or_else(|| other.to_owned()),
    };
    match (key.as_str(), record.get(&wanted).or_else(|| record.get(&key))) {
        ("toString", _) => jrt::s(&record.to_string()),
        (_, Some(value)) => from_plain(value),
        _ => V::Null,
    }
}

/// A service's provider: the object itself when a converted plugin provides it (LuckPerms' API, whose
/// methods hand out more objects), else a proxy that passes plain JSON (Essentials' Vault economy).
fn provider_of(provider: &ServiceProvider) -> V {
    let own = provider.call("$provider", &Value::Null);
    if own.get("ref").is_some() {
        return from_wire(&own);
    }
    jrt::native(ProxyH(provider.clone()))
}

fn services_manager(name: &str, a: &[V]) -> Option<R> {
    let class = a.first().map(class_of).unwrap_or_default();
    Some(Ok(match (name, a.len()) {
        // register(service class, provider, plugin, priority)
        ("register", 4) => {
            let provider = a[1].clone();
            let priority = {
                let text = a[3].text();
                let mut chars = text.chars();
                chars.next().map_or_else(|| "Normal".to_owned(), |c| c.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase())
            };
            provide_service(&crate::the_plugin(), &service_name(&class), &priority, move |method, args| {
                // Another converted plugin asks for the object itself: a reference, its calls run here.
                if method == "$provider" {
                    return to_wire(&provider);
                }
                let args: Vec<V> = args.as_array().map(|items| items.iter().map(from_plain).collect()).unwrap_or_default();
                match jrt::call(&provider, method, &args) {
                    Ok(answer) => to_plain(&answer),
                    Err(thrown) => json!({ "exception": thrown.text() }),
                }
            });
            V::Null
        }
        ("getRegistration", 1) => match providers(&class).into_iter().next() {
            Some(p) => jrt::native(RegistrationH(p)),
            None => {
                crate::the_plugin().warning(&format!("[services] no {} among {:?}", service_name(&class), horizon_bukkit::persistence::service_names()));
                V::Null
            }
        },
        ("load", 1) => providers(&class).into_iter().next().map_or(V::Null, |p| provider_of(&p)),
        ("getRegistrations", 1) if matches!(a[0], V::Class(_)) => jrt::list(providers(&class).into_iter().map(|p| jrt::native(RegistrationH(p))).collect()),
        ("isProvidedFor", 1) => V::Bool(!providers(&class).is_empty()),
        ("unregister", 2) | ("unregisterAll", 1) => {
            horizon_bukkit::persistence::withdraw_service(&crate::the_plugin(), &service_name(&class));
            V::Null
        }
        _ => return None,
    }))
}

// ---- Converted plugins using each other's classes: LuckPermsProvider.get(), PlaceholderAPI.setPlaceholders(..) ----
//
// Each converted plugin answers on `jport.plugin.<Name>`: whether a class is its own, a static call, a `new`,
// and a call on an object it handed out. What isn't plain data goes as a reference to the plugin that holds
// it, so a method called on it runs there.

/// Objects this plugin handed to others, by number.
static EXPORTS: std::sync::Mutex<Vec<V>> = std::sync::Mutex::new(Vec::new());
/// Which plugin owns a class another plugin's code named: `None` when none does (asked once).
static OWNERS: std::sync::Mutex<Option<std::collections::HashMap<String, Option<String>>>> = std::sync::Mutex::new(None);
static OWN_NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// An object of another converted plugin.
pub struct RemoteH {
    plugin: String,
    id: u64,
}

fn channel(plugin: &str) -> String {
    format!("jport.plugin.{plugin}")
}

/// At start: this plugin answers the others.
pub fn open_channel(plugin: &horizon_bukkit::Plugin) {
    let name = plugin.get_name().to_owned();
    let _ = OWN_NAME.set(name.clone());
    provide_service(plugin, &channel(&name), "Normal", answer);
    // Vault, as on Paper: an economy from the moment Vault enables, which hands each call to the economy
    // plugin (Essentials) when it's there. A plugin that checks for an economy before Essentials has
    // started (plugins start in name order) finds one; Essentials' own, registered higher, comes first.
    if name == "Vault" {
        provide_service(plugin, "net.milkbowl.vault.economy.Economy", "Lowest", |method, args| {
            let economy = service_registrations("net.milkbowl.vault.economy.Economy").into_iter().find(|p| p.get_plugin_name() != "Vault");
            match economy {
                Some(economy) => economy.call(method, args),
                None if method == "isEnabled" => json!(false),
                None if method == "getName" => json!("Vault"),
                None => json!({ "exception": "no economy plugin is enabled" }),
            }
        });
    }
}

fn own_name() -> &'static str {
    OWN_NAME.get().map_or("", String::as_str)
}

fn export(v: &V) -> u64 {
    let mut all = jrt::lock(&EXPORTS);
    if let Some(at) = all.iter().position(|known| jrt::same(known, v)) {
        return at as u64;
    }
    all.push(v.clone());
    (all.len() - 1) as u64
}

/// A value on the way to another plugin.
fn to_wire(v: &V) -> Value {
    match v {
        V::Null => Value::Null,
        V::Bool(b) => json!({ "b": b }),
        V::Int(i) => json!({ "i": i }),
        V::Long(l) => json!({ "l": l }),
        V::Double(d) => json!({ "d": d }),
        V::Char(c) => json!({ "c": c.to_string() }),
        V::Str(text) => json!(&**text),
        V::Class(name) => json!({ "class": name }),
        V::List(_) | V::Set(_) => json!({ "list": v.items().unwrap_or_default().iter().map(to_wire).collect::<Vec<_>>() }),
        V::Arr(_) => json!({ "arr": v.items().unwrap_or_default().iter().map(to_wire).collect::<Vec<_>>() }),
        _ => {
            if let Some(player) = crate::held::<horizon_bukkit::Player>(v) {
                return json!({ "player": player.get_unique_id() });
            }
            if let Some(remote) = v.native::<RemoteH>() {
                return json!({ "ref": [remote.plugin, remote.id] });
            }
            json!({ "ref": [own_name(), export(v)] })
        }
    }
}

fn from_wire(value: &Value) -> V {
    match value {
        Value::Null => V::Null,
        Value::String(text) => jrt::s(text),
        Value::Object(map) => {
            if let Some(b) = map.get("b") {
                return V::Bool(b.as_bool().unwrap_or(false));
            }
            if let Some(i) = map.get("i") {
                return V::Int(i.as_i64().unwrap_or(0) as i32);
            }
            if let Some(l) = map.get("l") {
                return V::Long(l.as_i64().unwrap_or(0));
            }
            if let Some(d) = map.get("d") {
                return V::Double(d.as_f64().unwrap_or(0.0));
            }
            if let Some(c) = map.get("c") {
                return V::Char(c.as_str().and_then(|s| s.chars().next()).unwrap_or('\0'));
            }
            if let Some(class) = map.get("class").and_then(Value::as_str) {
                return V::Class(Box::leak(class.to_owned().into_boxed_str()));
            }
            if let Some(items) = map.get("list").and_then(Value::as_array) {
                return jrt::list(items.iter().map(from_wire).collect());
            }
            if let Some(items) = map.get("arr").and_then(Value::as_array) {
                return jrt::arr(items.iter().map(from_wire).collect());
            }
            if let Some(uuid) = map.get("player").and_then(Value::as_str) {
                return jrt::call_static("Bukkit", "getPlayer", &[jrt::s(uuid)]).unwrap_or(V::Null);
            }
            if let Some(pair) = map.get("ref").and_then(Value::as_array) {
                let (plugin, id) = (pair[0].as_str().unwrap_or_default(), pair[1].as_u64().unwrap_or(0));
                if plugin == own_name() {
                    return jrt::lock(&EXPORTS).get(id as usize).cloned().unwrap_or(V::Null);
                }
                return jrt::native(RemoteH { plugin: plugin.to_owned(), id });
            }
            from_plain(value)
        }
        other => from_plain(other),
    }
}

fn reply(result: R) -> Value {
    match result {
        Ok(v) => json!({ "ok": to_wire(&v) }),
        Err(thrown) => json!({ "exc": ["RuntimeException", thrown.text()] }),
    }
}

/// The other end: another plugin asking this one.
fn answer(method: &str, call: &Value) -> Value {
    let args: Vec<V> = call["args"].as_array().map(|items| items.iter().map(from_wire).collect()).unwrap_or_default();
    let class = call["class"].as_str().unwrap_or_default();
    match method {
        "has" => json!(jrt::plugin_class(class).is_some()),
        "static" => {
            let Some(owner) = jrt::plugin_class(class) else {
                return json!({ "exc": ["ClassNotFoundException", class] });
            };
            let name = call["method"].as_str().unwrap_or_default();
            reply(match jrt::method_of(owner, name, &args) {
                Some(found) => found(&V::Null, &args),
                None => jrt::inherited_static(owner, name, &args).unwrap_or_else(|| Err(jrt::exc("NoSuchMethodError", format!("{class}.{name}")))),
            })
        }
        "new" => match jrt::plugin_class(class) {
            Some(owner) => reply(jrt::new_obj(owner, &args, &[])),
            None => json!({ "exc": ["ClassNotFoundException", class] }),
        },
        "call" => {
            let target = jrt::lock(&EXPORTS).get(call["id"].as_u64().unwrap_or(u64::MAX) as usize).cloned();
            match target {
                Some(target) => reply(jrt::call(&target, call["method"].as_str().unwrap_or_default(), &args)),
                None => json!({ "exc": ["IllegalStateException", "that object is gone"] }),
            }
        }
        "is" => {
            let target = jrt::lock(&EXPORTS).get(call["id"].as_u64().unwrap_or(u64::MAX) as usize).cloned();
            match target {
                Some(target) => json!({ "ok": to_wire(&V::Bool(jrt::instance_of(&target, call["type"].as_str().unwrap_or_default()))) }),
                None => json!({ "exc": ["IllegalStateException", "that object is gone"] }),
            }
        }
        _ => Value::Null,
    }
}

fn ask(plugin: &str, method: &str, call: Value) -> R {
    let Some(provider) = service_registrations(&channel(plugin)).into_iter().next() else {
        return Err(jrt::exc("IllegalStateException", format!("{plugin} is not running")));
    };
    let back = provider.call(method, &call);
    if let Some(problem) = back.get("exc").and_then(Value::as_array) {
        return Err(jrt::exc(problem[0].as_str().unwrap_or("RuntimeException"), problem[1].as_str().unwrap_or_default().to_owned()));
    }
    Ok(from_wire(&back["ok"]))
}

/// Whether another converted plugin has `class`.
pub fn has_class(class: &str) -> bool {
    owner_of(class).is_some()
}

/// The converted plugin that owns `class`, asked of each once.
fn owner_of(class: &str) -> Option<String> {
    if let Some(known) = jrt::lock(&OWNERS).as_ref().and_then(|all| all.get(class).cloned()) {
        return known;
    }
    let mut found = None;
    for name in horizon_bukkit::persistence::service_names() {
        if let Some(plugin) = name.strip_prefix("jport.plugin.") {
            if plugin == own_name() {
                continue;
            }
            let has = service_registrations(&name).into_iter().next().map(|p| p.call("has", &json!({ "class": class })));
            if has.is_some_and(|v| v.as_bool() == Some(true)) {
                found = Some(plugin.to_owned());
                break;
            }
        }
    }
    jrt::lock(&OWNERS).get_or_insert_default().insert(class.to_owned(), found.clone());
    found
}

/// A static call to a class of another converted plugin; `None` when no plugin has it.
pub fn static_remote(class: &str, name: &str, a: &[V]) -> Option<R> {
    let plugin = owner_of(class)?;
    Some(ask(&plugin, "static", json!({ "class": class, "method": name, "args": a.iter().map(to_wire).collect::<Vec<_>>() })))
}

/// `new` of a class of another converted plugin.
pub fn construct_remote(class: &str, a: &[V]) -> Option<R> {
    let plugin = owner_of(class)?;
    Some(ask(&plugin, "new", json!({ "class": class, "args": a.iter().map(to_wire).collect::<Vec<_>>() })))
}

/// A method on another plugin's object: it runs there.
pub fn remote_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    let remote = this.native::<RemoteH>()?;
    Some(ask(&remote.plugin, "call", json!({ "id": remote.id, "method": name, "args": a.iter().map(to_wire).collect::<Vec<_>>() })))
}

pub fn is_remote(v: &V) -> bool {
    v.native::<RemoteH>().is_some()
}

/// PlaceholderAPI's `PlaceholderExpansion`, which a plugin's own class extends: what the class doesn't
/// define. `register()` hands the object to the converted PlaceholderAPI (as a reference: PlaceholderAPI
/// calls back into it for each placeholder), as `PlaceholderAPIPlugin.getInstance().getLocalExpansionManager()
/// .register(this)` does on Paper.
pub fn expansion_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if !matches!(this, V::Obj(_)) || !jrt::instance_of(this, "PlaceholderExpansion") {
        return None;
    }
    let manager = || jrt::call(&jrt::call_static("PlaceholderAPIPlugin", "getInstance", &[])?, "getLocalExpansionManager", &[]);
    let identifier = || jrt::call(this, "getIdentifier", &[]).map(|id| id.text()).unwrap_or_default();
    Some(Ok(match (name, a.len()) {
        ("register", 0) => match manager().and_then(|m| jrt::call(&m, "register", std::slice::from_ref(this))) {
            Ok(done) => done,
            Err(thrown) => {
                crate::the_plugin().warning(&format!("[PlaceholderAPI] expansion {} not registered: {}", identifier(), thrown.text()));
                V::Bool(false)
            }
        },
        ("unregister", 0) => manager().and_then(|m| jrt::call(&m, "unregister", std::slice::from_ref(this))).unwrap_or(V::Bool(false)),
        ("isRegistered", 0) => jrt::call_static("PlaceholderAPI", "isRegistered", &[jrt::s(&identifier())]).unwrap_or(V::Bool(false)),
        ("canRegister", 0) => {
            let required = jrt::call(this, "getRequiredPlugin", &[]).unwrap_or(V::Null);
            V::Bool(required.is_null() || required.text().is_empty() || horizon_bukkit::persistence::service_names().iter().any(|s| s.eq_ignore_ascii_case(&format!("jport.plugin.{}", required.text()))) || jrt::call_static("Bukkit", "getPluginManager", &[]).and_then(|pm| jrt::call(&pm, "isPluginEnabled", &[required.clone()])).is_ok_and(|on| on.truth()))
        }
        ("getRequiredPlugin" | "getDescription" | "getPlugin" | "getLink", 0) => V::Null,
        // PlaceholderAPI marks it (INTERNAL, EXTERNAL) as it registers it.
        // Java's field starts as `Type.INTERNAL`; that initializer is PlaceholderAPI's, never run on a subclass made in
        // another plugin, so null went to `getExpansionType().name()` and every expansion failed to register.
        ("getExpansionType", 0) => match this.get("$expansionType") {
            Ok(V::Null) | Err(_) => jrt::s("INTERNAL"),
            Ok(set) => set,
        },
        ("setExpansionType", 1) => {
            let _ = this.set("$expansionType", a[0].clone());
            V::Null
        }
        // Its section of PlaceholderAPI's config: `expansions.<identifier>`.
        ("getConfigSection", 0 | 1) => {
            let section = jrt::call_static("PlaceholderAPIPlugin", "getInstance", &[])
                .and_then(|papi| jrt::call(&papi, "getConfig", &[]))
                .and_then(|config| jrt::call(&config, "getConfigurationSection", &[jrt::s(&format!("expansions.{}", identifier()))]))
                .unwrap_or(V::Null);
            match a.first() {
                Some(path) if !section.is_null() => jrt::call(&section, "getConfigurationSection", std::slice::from_ref(path)).unwrap_or(V::Null),
                _ => section,
            }
        }
        ("get" | "getInt" | "getLong" | "getDouble" | "getString" | "getBoolean", 2) | ("getStringList" | "configurationContains", 1) => {
            let section = jrt::call(this, "getConfigSection", &[]).unwrap_or(V::Null);
            match (section.is_null(), name) {
                (true, "getStringList") => jrt::list(Vec::new()),
                (true, "configurationContains") => V::Bool(false),
                (true, _) => a[1].clone(),
                (false, "configurationContains") => return Some(jrt::call(&section, "contains", &a[..1])),
                (false, _) => return Some(jrt::call(&section, name, a)),
            }
        }
        ("log", 2 | 3) | ("info" | "warning" | "severe", 1 | 2) => {
            let (level, text) = if name == "log" { (a[0].text(), a[1].text()) } else { (name.to_uppercase(), a[0].text()) };
            let line = format!("[{}] {text}", jrt::call(this, "getName", &[]).map(|n| n.text()).unwrap_or_default());
            match level.as_str() {
                "SEVERE" => crate::the_plugin().severe(&line),
                "WARNING" => crate::the_plugin().warning(&line),
                _ => crate::the_plugin().info(&line),
            }
            V::Null
        }
        ("persist", 0) => V::Bool(false),
        ("getName", 0) => jrt::s(&identifier()),
        ("getPlaceholders", 0) => jrt::list(Vec::new()),
        // PlaceholderAPI asks `onRequest(OfflinePlayer, params)`; an older expansion answers `onPlaceholderRequest(Player, params)`.
        ("onRequest", 2) => return Some(jrt::call(this, "onPlaceholderRequest", a)),
        ("onPlaceholderRequest", 2) => V::Null,
        ("getPlaceholderAPI", 0) => return Some(jrt::call_static("PlaceholderAPIPlugin", "getInstance", &[])),
        _ => return None,
    }))
}

/// `instanceof` of another plugin's object, as its own plugin answers it. An older plugin that doesn't
/// know the question is taken at its word: yes, as before.
pub fn remote_instance_of(v: &V, name: &str) -> bool {
    let Some(remote) = v.native::<RemoteH>() else {
        return false;
    };
    match ask(&remote.plugin, "is", json!({ "id": remote.id, "type": name })) {
        Ok(V::Bool(answer)) => answer,
        _ => true,
    }
}
