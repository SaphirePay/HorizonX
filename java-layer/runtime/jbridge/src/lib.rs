//! Where Bukkit's Java shape and the SDK's differ: `getConfig()` hands back one live object, a title is
//! a value, `getLogger()` and `getCommand()` are objects, schedulers take a `Runnable`. Written once, used
//! by every converted plugin. Everything else is bound straight from the SDK's `/// bukkit:` tags.
use horizon_bukkit::config::{self, YamlConfiguration};
use horizon_bukkit::{CommandSender, Component, Player, Plugin, World, Location, mini_message, scheduler};
pub mod component;
pub mod registries;
pub mod snakeyaml;
pub mod messenger;
pub mod lifecycle;
pub mod dialogs;
pub mod xplugin;
pub mod protocollib;
pub mod packetevents;
pub mod generator;
pub mod bungee;
pub mod horizon_chunks;
pub mod objstream;

use jrt::javaio::{JBytes, JDuration};
use jrt::{R, V};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

/// An event, valid while its listeners run.
pub struct Ev<T>(pub *mut T);
unsafe impl<T> Send for Ev<T> {}
unsafe impl<T> Sync for Ev<T> {}

/// An event handed to a Java listener. The game owns the event only while the listener runs; Java code
/// that keeps it (a field, a delayed task) and reads it later gets an exception instead of freed memory,
/// which once took a whole server down ("memory allocation of <an address> bytes failed").
pub struct EvL<T> {
    pub ptr: *mut T,
    pub id: u64,
    /// What its getters answered while the listener ran: a task that reads the event later (Java keeps it
    /// alive) gets the same answers, as a Java event would give.
    pub memo: Mutex<HashMap<String, V>>,
}
unsafe impl<T> Send for EvL<T> {}
unsafe impl<T> Sync for EvL<T> {}

static LIVE_EVENTS: Mutex<Vec<u64>> = Mutex::new(Vec::new());
static NEXT_EVENT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The event's Java handle, and its id for `ev_close` once the listener returned.
pub fn ev_open<T: 'static>(event: &mut T) -> (V, u64) {
    let id = NEXT_EVENT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    jrt::lock(&LIVE_EVENTS).push(id);
    (jrt::native(EvL { ptr: std::ptr::from_mut(event), id, memo: Mutex::new(HashMap::new()) }), id)
}

/// A getter answered while the event was live, kept for later readers.
pub fn ev_remember(memo: &Mutex<HashMap<String, V>>, name: &str, a: &[V], out: &Option<R>) {
    if a.is_empty() && (name.starts_with("get") || name.starts_with("is") || name.starts_with("has")) {
        if let Some(Ok(value)) = out {
            jrt::lock(memo).insert(name.to_owned(), value.clone());
        }
    }
}

/// A call on an event after its listener returned: a getter it already answered gives the same answer,
/// a never-read getter or a change can't reach the game any more and throws.
pub fn ev_late(memo: &Mutex<HashMap<String, V>>, name: &str, a: &[V]) -> R {
    if a.is_empty() {
        if let Some(value) = jrt::lock(memo).get(name) {
            return Ok(value.clone());
        }
    }
    Err(ev_gone())
}

pub fn ev_close(id: u64) {
    jrt::lock(&LIVE_EVENTS).retain(|live| *live != id);
}

pub fn ev_live(id: u64) -> bool {
    jrt::lock(&LIVE_EVENTS).contains(&id)
}

/// Paper's timings, in short: what each listener and task of the plugin costs the tick. Every 30 s, the
/// ones that took more than 1 % of that time (300 ms) go to the log, the costliest first.
static TIMINGS: Mutex<Option<(std::time::Instant, HashMap<&'static str, (u64, std::time::Duration)>)>> = Mutex::new(None);

pub fn timing(what: &'static str, started: std::time::Instant) {
    let spent = started.elapsed();
    let mut held = jrt::lock(&TIMINGS);
    let (since, table) = held.get_or_insert_with(|| (std::time::Instant::now(), HashMap::new()));
    let row = table.entry(what).or_insert((0, std::time::Duration::ZERO));
    row.0 += 1;
    row.1 += spent;
    if since.elapsed() >= std::time::Duration::from_secs(30) {
        let mut rows: Vec<_> = table.drain().filter(|(_, (_, total))| *total >= std::time::Duration::from_millis(300)).collect();
        *since = std::time::Instant::now();
        drop(held);
        rows.sort_by(|a, b| b.1.1.cmp(&a.1.1));
        for (name, (count, total)) in rows.into_iter().take(5) {
            the_plugin().warning(&format!("[timings] {name}: {:.0} ms in 30 s over {count} calls ({:.2} ms each)", total.as_secs_f64() * 1000.0, total.as_secs_f64() * 1000.0 / count as f64));
        }
    }
}

/// A text parameter of the SDK (`Tag.is_tagged(id: &str)`): Java may pass a Material object where the SDK
/// reads its name, as `Tag.isTagged(stack.getType())` does.
pub fn text_like(v: &V) -> bool {
    matches!(v, V::Str(_) | V::Sb(_)) || v.native::<Mutex<horizon_bukkit::Material>>().is_some()
}

pub fn text_arg(v: &V) -> String {
    match held::<horizon_bukkit::Material>(v) {
        Some(material) => material.key().to_owned(),
        None => v.text(),
    }
}

pub fn ev_gone() -> V {
    jrt::exc("IllegalStateException", "this event was kept after its listener returned; Horizon no longer holds it".to_string())
}

pub struct CommandH(pub String);
/// Bukkit's CommandMap, as Paper's getCommandMap() or the server's reflected commandMap field gives it.
pub struct CommandMapH;
/// The server class's commandMap field, reached by reflection.
pub struct CommandMapFieldH;
pub struct SchedulerH;
pub struct LoggerH;
/// `getDescription()` / `getPluginMeta()`: the plugin's own plugin.yml.
pub struct DescH(YamlConfiguration);
/// The `Command` a plugin's own command class extends.
pub struct CmdBaseH(Mutex<CmdFields>);
pub struct CmdFields {
    name: String,
    description: String,
    usage: String,
    aliases: Vec<String>,
    permission: Option<String>,
    message: Option<String>,
}

fn cmd_base_method(c: &CmdBaseH, name: &str, a: &[V]) -> Option<R> {
    let mut f = jrt::lock(&c.0);
    let opt = |o: &Option<String>| o.as_deref().map_or(V::Null, jrt::s);
    Some(Ok(match (name, a.len()) {
        ("getName" | "getLabel", 0) => jrt::s(&f.name),
        ("toString", 0) => jrt::s(&format!("Command({})", f.name)),
        ("getDescription", 0) => jrt::s(&f.description),
        ("getUsage", 0) => jrt::s(&f.usage),
        ("getAliases", 0) => jrt::list(f.aliases.iter().map(|x| jrt::s(x)).collect()),
        ("getPermission", 0) => opt(&f.permission),
        ("getPermissionMessage", 0) => opt(&f.message),
        ("setName" | "setLabel", 1) => { f.name = a[0].text(); V::Bool(true) }
        ("setDescription", 1) => { f.description = a[0].text(); V::Null }
        ("setUsage", 1) => { f.usage = a[0].text(); V::Null }
        ("setAliases", 1) => { f.aliases = a[0].items().unwrap_or_default().iter().map(V::text).collect(); V::Null }
        ("setPermission", 1) => { f.permission = (!a[0].is_null()).then(|| a[0].text()); V::Null }
        ("setPermissionMessage", 1) => { f.message = (!a[0].is_null()).then(|| a[0].text()); V::Null }
        ("isRegistered", 0) | ("register" | "unregister", 1) => V::Bool(true),
        ("testPermissionSilent" | "testPermission", 1) => {
            let Some(permission) = f.permission.clone() else { return Some(Ok(V::Bool(true))) };
            drop(f);
            let allowed = match jrt::call(&a[0], "hasPermission", &[jrt::s(&permission)]) { Ok(v) => v.truth(), Err(e) => return Some(Err(e)) };
            if !allowed && name == "testPermission" {
                let _ = jrt::call(&a[0], "sendMessage", &[jrt::s("§cI'm sorry, but you do not have permission to perform this command. Please contact the server administrators if you believe that this is in error.")]);
            }
            V::Bool(allowed)
        }
        _ => return None,
    }))
}
pub struct MiniH;
pub struct TaskH(pub u32);
/// An Adventure `Sound`: what `/playsound` needs.
pub struct SoundH {
    pub key: String,
    pub source: String,
    pub volume: f32,
    pub pitch: f32,
}
pub struct TimesH(pub u32, pub u32, pub u32);
pub struct TitleH {
    pub title: Component,
    pub subtitle: Component,
    pub times: (u32, u32, u32),
}
pub struct OptionsH(pub V);

pub struct State {
    pub plugin: Plugin,
    pub main: V,
    pub config: V,
    pub resources: &'static [(&'static str, &'static [u8])],
    pub listeners: fn(&V, &Plugin),
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

// ---- Questions from other threads: asked on the main thread, at the next tick ----

type Waiting = (Value, std::sync::mpsc::Sender<Value>);
static HOPS: Mutex<Vec<Waiting>> = Mutex::new(Vec::new());
static SEEN: Mutex<Option<HashMap<String, (std::time::Instant, Value)>>> = Mutex::new(None);
static MAIN_THREAD: std::sync::OnceLock<std::thread::ThreadId> = std::sync::OnceLock::new();
static LAST_TICK: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Answers what's queued. Only where the game is lent: a tick's task, or the main thread inside an event.
/// Answers the queries other threads are waiting on, now: for a host whose main thread is busy a long while
/// between ticks (the Java layer enabling plugins one after another, as Paper's start does).
pub fn pump_hops() {
    pump();
}

fn pump() {
    if MAIN_THREAD.get() != Some(&std::thread::current().id()) {
        return;
    }
    let waiting = std::mem::take(&mut *jrt::lock(&HOPS));
    for (request, reply) in waiting {
        let op = request["op"].as_str().unwrap_or_default().to_owned();
        let answer = horizon_bukkit::query_from_main(request);
        if op.starts_with("box") {
            crate::horizon_chunks::debug_line(&format!("pump ran {op}: call {} changed {}", answer["call"], answer["changed"]));
        }
        let _ = reply.send(answer);
    }
}

fn hop(request: Value) -> Value {
    if MAIN_THREAD.get() == Some(&std::thread::current().id()) {
        return Value::Null;
    }
    // Each answer costs a tick, and a permission plugin's commands ask thousands of times.
    if request["op"] == "core_perm_grant" {
        jrt::lock(&SEEN).get_or_insert_default().clear();
    }
    let keep_ms = match request["op"].as_str() {
        Some("player" | "players") => 1_000,
        Some("core_perm_check") => 30_000,
        _ => 0,
    };
    let key = (keep_ms > 0).then(|| request.to_string());
    if let Some(key) = &key {
        if let Some((at, value)) = jrt::lock(&SEEN).get_or_insert_default().get(key) {
            if at.elapsed().as_millis() < keep_ms {
                return value.clone();
            }
        }
    }
    if request["op"].as_str().is_some_and(|op| op.starts_with("box")) {
        crate::horizon_chunks::debug_line(&format!("hop queued {} from {:?}", request["op"], std::thread::current().id()));
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    jrt::lock(&HOPS).push((request, sender));
    // Before the first tick the server is starting (plugins being enabled): the answer comes at the next pump,
    // which may be a while; Paper reads its registries from any thread without waiting at all.
    let wait = if jrt::lock(&LAST_TICK).is_some() { 5 } else { 120 };
    let Ok(answer) = receiver.recv_timeout(std::time::Duration::from_secs(wait)) else {
        return Value::Null;
    };
    if let Some(key) = key {
        jrt::lock(&SEEN).get_or_insert_default().insert(key, (std::time::Instant::now(), answer.clone()));
    }
    answer
}

fn tick() {
    *jrt::lock(&LAST_TICK) = Some(std::time::Instant::now());
    pump();
}

/// A server paused while empty runs no tasks, so a command that went on to another thread would wait
/// for a tick that isn't coming. Paper's scheduler keeps beating through the pause: here the command
/// that started the work answers its questions until they stop.
fn finish_while_paused() {
    use std::time::{Duration, Instant};
    if jrt::lock(&LAST_TICK).is_some_and(|at| at.elapsed() < Duration::from_millis(500)) {
        return;
    }
    let (started, mut quiet) = (Instant::now(), Instant::now());
    while started.elapsed() < Duration::from_secs(8) && quiet.elapsed() < Duration::from_millis(300) {
        if jrt::lock(&HOPS).is_empty() {
            std::thread::sleep(Duration::from_millis(1));
        } else {
            pump();
            quiet = Instant::now();
        }
    }
}
static DEFAULTS: Mutex<Option<HashMap<usize, Value>>> = Mutex::new(None);
/// Simple-YAML's `YamlFile`: a configuration that knows its own file (`new YamlFile(file).load()`).
static YAML_FILES: Mutex<Option<HashMap<usize, std::path::PathBuf>>> = Mutex::new(None);

fn yaml_file_path(this: &V) -> Option<std::path::PathBuf> {
    jrt::lock(&YAML_FILES).as_ref()?.get(&identity(this)).cloned()
}

/// What a `YamlFile` adds to a configuration: loading and saving its own file.
fn yaml_file_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    let path = || yaml_file_path(this).ok_or_else(|| jrt::exc("IllegalArgumentException", "This configuration file is null!"));
    let load = |path: &std::path::Path| -> R {
        if !path.is_file() {
            return Err(jrt::exc("FileNotFoundException", format!("{} (The system cannot find the file specified)", path.display())));
        }
        *held::<YamlConfiguration>(this).ok_or_else(|| jrt::exc("NullPointerException", "configuration"))? = YamlConfiguration::load_configuration(path);
        Ok(V::Null)
    };
    let create = |path: &std::path::Path| -> Result<bool, V> {
        if path.exists() {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, b"").map(|()| true).map_err(|e| jrt::exc("IOException", e.to_string()))
    };
    Some(match (name, a.len()) {
        ("load" | "loadWithComments", 0) => path().and_then(|p| load(&p)),
        ("createOrLoad" | "createOrLoadWithComments", 0) => path().and_then(|p| create(&p).and_then(|_| load(&p))),
        ("createNewFile", 0 | 1) => path().and_then(|p| {
            if a.first().is_some_and(V::truth) && p.exists() {
                let _ = std::fs::remove_file(&p);
            }
            create(&p).map(|_| V::Null)
        }),
        ("exists", 0) => path().map(|p| V::Bool(p.exists())),
        ("deleteFile", 0) => path().and_then(|p| std::fs::remove_file(&p).map(|()| V::Null).map_err(|e| jrt::exc("IOException", e.to_string()))),
        ("save", 0) => path().and_then(|p| {
            let cfg = held::<YamlConfiguration>(this).ok_or_else(|| jrt::exc("NullPointerException", "configuration"))?;
            cfg.save(Some(&p)).map(|()| V::Null).map_err(|e| jrt::exc("IOException", e.to_string()))
        }),
        ("getConfigurationFile", 0) => path().map(jrt::javaio::file),
        ("getFilePath", 0) => path().map(|p| jrt::s(&p.display().to_string())),
        ("setConfigurationFile", 1) => {
            let Some(p) = jrt::javaio::path_of(&a[0]) else { return Some(Err(jrt::exc("IllegalArgumentException", "File cannot be null"))) };
            jrt::lock(&YAML_FILES).get_or_insert_default().insert(identity(this), p);
            Ok(V::Null)
        }
        _ => return None,
    })
}

fn state() -> MutexGuard<'static, Option<State>> {
    jrt::lock(&STATE)
}

pub fn the_plugin() -> Plugin {
    state().as_ref().expect("the plugin is enabled").plugin.clone()
}

/// What an object of the game answers that its own bindings don't: an entity as its own class (`entity_class_call`),
/// then the Server and a World as an Adventure audience of their players.
thread_local! {
    static IN_TILE_CLASS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A block's state used as its own class: `((Sign) state).isWaxed()`, `((Furnace) state).getCookTime()`. A Java cast
/// does nothing here and `block.getState()` is a TileState, so each class's view of it is tried, and what that view
/// changed goes back into the state for `update()` (as meta_class_call does for item metas).
fn tile_class_call(this: &V, name: &str, a: &[V]) -> Option<R> {
    use horizon_bukkit::block_data::TileState;
    if IN_TILE_CLASS.get() {
        return None;
    }
    let state = held::<TileState>(this)?.clone();
    macro_rules! try_as {
        ($($class:path),*) => {$(
            let as_class = wrap($class(state.clone()));
            IN_TILE_CLASS.set(true);
            let answer = jrt::try_native(&as_class, name, a);
            IN_TILE_CLASS.set(false);
            if let Some(answer) = answer {
                if let Some(changed) = held::<$class>(&as_class).map(|view| view.0.clone()) {
                    if let Some(mut mine) = held::<TileState>(this) {
                        *mine = changed;
                    }
                }
                return Some(answer);
            }
        )*};
    }
    try_as!(horizon_bukkit::blocks::Sign, horizon_bukkit::fam_items::Chest, horizon_bukkit::blocks::Furnace, horizon_bukkit::blocks::Hopper,
        horizon_bukkit::fam_items::ShulkerBox, horizon_bukkit::fam_items::Dispenser, horizon_bukkit::blocks::Banner, horizon_bukkit::fam_items::Skull,
        horizon_bukkit::blocks::BrewingStand, horizon_bukkit::blocks::Beacon, horizon_bukkit::blocks::Beehive, horizon_bukkit::blocks::Jukebox,
        horizon_bukkit::blocks::Lectern, horizon_bukkit::blocks::CommandBlock, horizon_bukkit::blocks::Crafter, horizon_bukkit::blocks::BrushableBlock,
        horizon_bukkit::blocks::EndGateway, horizon_bukkit::blocks::SculkSensor, horizon_bukkit::blocks::Structure, horizon_bukkit::blocks::TrialSpawner,
        horizon_bukkit::blocks::Vault, horizon_bukkit::fam_items::Bed, horizon_bukkit::fam_items::Bell, horizon_bukkit::fam_items::Campfire,
        horizon_bukkit::fam_items::ChiseledBookshelf, horizon_bukkit::fam_items::Shelf, horizon_bukkit::fam_items::SculkShrieker);
    None
}

fn bridge_fallback(this: &V, name: &str, a: &[V]) -> Option<R> {
    objstream::method(this, name, a).or_else(|| tile_class_call(this, name, a)).or_else(|| entity_class_call(this, name, a)).or_else(|| meta_class_call(this, name, a)).or_else(|| forward_to_players(this, name, a)).or_else(|| {
        // An inventory of the game whose holder nothing here knows (a GUI's view on close, a type of inventory without
        // its own binding): no holder, as Java's `getHolder()` gives null, so `instanceof MyHolder` checks are false
        // instead of the listener throwing (CustomEvents, DGCommandItems on close).
        (name == "getHolder" && a.is_empty()).then(|| Ok(V::Null))
    }).or_else(|| (name == "getHelpMap" && a.is_empty()).then(|| Ok(jrt::native(HelpMapH)))).or_else(|| old_scheduler(this, name, a))
}

/// Bukkit's deprecated scheduler methods (`scheduleSyncRepeatingTask` ...), which return the task's id: the
/// `runTask...` they became, and its id.
fn old_scheduler(this: &V, name: &str, a: &[V]) -> Option<R> {
    let (now, args) = match (name, a.len()) {
        ("scheduleSyncDelayedTask", 2) => ("runTask", a.to_vec()),
        ("scheduleSyncDelayedTask", 3) => ("runTaskLater", a.to_vec()),
        ("scheduleAsyncDelayedTask", 2) => ("runTaskAsynchronously", a.to_vec()),
        ("scheduleAsyncDelayedTask", 3) => ("runTaskLaterAsynchronously", a.to_vec()),
        ("scheduleSyncRepeatingTask", 4) => ("runTaskTimer", a.to_vec()),
        ("scheduleAsyncRepeatingTask", 4) => ("runTaskTimerAsynchronously", a.to_vec()),
        _ => return None,
    };
    Some(jrt::call(this, now, &args).and_then(|task| jrt::call(&task, "getTaskId", &[])))
}

/// `Server.getHelpMap()`: Bukkit's /help index. Horizon's /help lists commands itself, so topics added here are
/// accepted and none is looked up.
pub struct HelpMapH;

fn help_map_method(name: &str) -> Option<R> {
    Some(Ok(match name {
        "getHelpTopic" => V::Null,
        "getHelpTopics" | "getIgnoredPlugins" => jrt::list(Vec::new()),
        "addTopic" | "clear" | "registerHelpTopicFactory" => V::Null,
        _ => return None,
    }))
}

thread_local! {
    static IN_META_CLASS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// An item's meta asked for what only one class of meta has: `((SkullMeta) item.getItemMeta()).setOwningPlayer(p)`.
/// A Java cast changes nothing here, so the meta (the stack, as `getItemMeta()` gives it) is tried as each class of
/// meta the SDK has, and what that class's method changed goes back into the meta the plugin holds, which it then
/// hands to `setItemMeta` as on Paper (FluxMarket's player heads).
fn meta_class_call(this: &V, name: &str, a: &[V]) -> Option<R> {
    use horizon_bukkit::meta::*;
    if IN_META_CLASS.get() {
        return None;
    }
    let stack = held::<horizon_bukkit::ItemStack>(this)?.clone();
    macro_rules! try_as {
        ($($class:ident),*) => {$(
            let as_class = wrap($class(stack.clone()));
            IN_META_CLASS.set(true);
            let answer = jrt::try_native(&as_class, name, a);
            IN_META_CLASS.set(false);
            if let Some(answer) = answer {
                if let Some(changed) = held::<$class>(&as_class).map(|m| m.0.clone()) {
                    if let Some(mut mine) = held::<horizon_bukkit::ItemStack>(this) {
                        *mine = changed;
                    }
                }
                return Some(answer);
            }
        )*};
    }
    try_as!(SkullMeta, BookMeta, WritableBookMeta, PotionMeta, LeatherArmorMeta, ColorableArmorMeta, FireworkMeta, FireworkEffectMeta,
        CompassMeta, MapMeta, EnchantmentStorageMeta, BannerMeta, SpawnEggMeta, ArmorMeta, CrossbowMeta, BundleMeta, BlockStateMeta,
        BlockDataMeta, SuspiciousStewMeta, AxolotlBucketMeta, TropicalFishBucketMeta, MusicInstrumentMeta, ShieldMeta,
        OminousBottleMeta, KnowledgeBookMeta);
    None
}

/// Adventure's `Audience` methods, and Bukkit's `PluginMessageRecipient.sendPluginMessage`: on Paper the Server is a
/// ForwardingAudience of every online player and a World of its players, so each player gets the call.
const AUDIENCE_METHODS: &[&str] = &[
    "sendMessage", "sendRichMessage", "sendPlainMessage", "sendActionBar", "sendPlayerListHeader", "sendPlayerListFooter",
    "sendPlayerListHeaderAndFooter", "showTitle", "clearTitle", "resetTitle", "sendTitlePart", "showBossBar", "hideBossBar",
    "playSound", "stopSound", "openBook", "sendPluginMessage",
];

fn forward_to_players(this: &V, name: &str, a: &[V]) -> Option<R> {
    if !AUDIENCE_METHODS.contains(&name) {
        return None;
    }
    let players = if held::<horizon_bukkit::fam_core::Server>(this).is_some() {
        jrt::call_static("Bukkit", "getOnlinePlayers", &[]).ok()?.items().ok()?
    } else if held::<World>(this).is_some() {
        jrt::call(this, "getPlayers", &[]).ok()?.items().ok()?
    } else {
        return None;
    };
    for player in players {
        if let Err(thrown) = jrt::call(&player, name, a) {
            return Some(Err(thrown));
        }
    }
    Some(Ok(V::Null))
}

/// An `OfflinePlayer` argument: one held as such, or a `Player` (every Player is an OfflinePlayer in Bukkit), so
/// `skullMeta.setOwningPlayer(player)` works as on Paper (FluxMarket).
pub fn offline_of(value: &V) -> Option<horizon_bukkit::OfflinePlayer> {
    if let Some(offline) = held::<horizon_bukkit::OfflinePlayer>(value) {
        return Some(offline.clone());
    }
    held::<Player>(value).map(|player| horizon_bukkit::get_offline_player(&player.get_name()))
}

pub fn main_object() -> V {
    state().as_ref().map_or(V::Null, |s| s.main.clone())
}

pub fn start(plugin: Plugin, resources: &'static [(&'static str, &'static [u8])], listeners: fn(&V, &Plugin)) {
    *state() = Some(State { plugin, main: V::Null, config: V::Null, resources, listeners });
    jrt::set_reporter(|line| the_plugin().severe(line));
    let _ = MAIN_THREAD.set(std::thread::current().id());
    jrt::stack_watch::mark_server_thread();
    horizon_bukkit::horizon_mod::on_query_without_game(hop);
    jrt::set_pump(pump);
    jrt::set_fallback(bridge_fallback);
    jrt::set_native_label(native_label);
    scheduler::run_task_timer(1, 1, tick);
    xplugin::open_channel(&the_plugin());
    jrt::set_elsewhere(xplugin::static_remote, xplugin::construct_remote);
    jrt::set_class_elsewhere(xplugin::has_class);
}

/// `onEnable` threw. Paper logs it and disables the plugin: its onDisable runs, its tasks and listeners
/// stop, and its commands answer the error line. A gap of the converter's (`... is not converted`) leaves
/// it running instead, since Paper wouldn't have thrown there.
pub fn enable_failed(thrown: &V) {
    if thrown.text().contains("is not converted") {
        return;
    }
    let name = the_plugin().get_name().to_owned();
    horizon_bukkit::fam_core::get_plugin_manager().disable_plugin(&name);
}

pub fn set_main(main: V) {
    if let Some(state) = state().as_mut() {
        state.main = main;
    }
}

/// The plugin's own event classes (`class MeteorSpawnEvent extends Event`) and who listens to them: the
/// listener, the event's class, the `@EventHandler` method, its priority (LOWEST 0 .. MONITOR 5), ignoreCancelled.
static OWN_EVENT_LISTENERS: Mutex<Vec<(V, &'static str, &'static str, u8, bool)>> = Mutex::new(Vec::new());

/// `registerEvents(listener, plugin)` for a handler of one of the plugin's own events.
pub fn own_listener(listener: &V, event: &'static str, method: &'static str, priority: u8, ignore_cancelled: bool) {
    jrt::lock(&OWN_EVENT_LISTENERS).push((listener.clone(), event, method, priority, ignore_cancelled));
}

/// `getPluginManager().callEvent(myEvent)` with one of the plugin's own events: its handlers, by priority.
fn call_own_event(event: &V) -> R {
    let mut handlers: Vec<_> = jrt::lock(&OWN_EVENT_LISTENERS).iter().filter(|row| jrt::instance_of(event, row.1)).cloned().collect();
    handlers.sort_by_key(|row| row.3);
    for (listener, _, method, _, ignore_cancelled) in handlers {
        if ignore_cancelled && jrt::call(event, "isCancelled", &[]).is_ok_and(|c| c.truth()) {
            continue;
        }
        let _ = jrt::report(method, jrt::call(&listener, method, std::slice::from_ref(event)));
    }
    Ok(V::Null)
}

/// `registerEvent(EventClass, listener, priority, executor, plugin, ignoreCancelled)`, which the
/// converted plugin answers for the events it names: event's simple name, listener, priority,
/// ignoreCancelled, executor. False for an event it doesn't know.
pub type DynamicEvents = fn(&str, &V, &str, bool, &V, &Plugin) -> bool;
static DYNAMIC: std::sync::OnceLock<DynamicEvents> = std::sync::OnceLock::new();

pub fn set_dynamic(register: DynamicEvents) {
    let _ = DYNAMIC.set(register);
}

fn register_dynamic(a: &[V]) -> Option<R> {
    let event = match &a[0] {
        V::Class(name) => (*name).to_owned(),
        other => other.text(),
    };
    let event = event.rsplit(['.', '$']).next().unwrap_or_default().to_owned();
    let priority = if a[2].is_null() { "NORMAL".to_owned() } else { a[2].text() };
    let ignore = a.get(5).is_some_and(V::truth);
    let register = DYNAMIC.get()?;
    register(&event, &a[1], &priority, ignore, &a[3], &the_plugin()).then_some(Ok(V::Null))
}

fn resource(name: &str) -> Option<&'static [u8]> {
    // A leading `/` is the jar's root (`Class.getResourceAsStream("/lang/strings.json")`): the same file.
    let name = name.trim_start_matches('/');
    state().as_ref()?.resources.iter().find(|(file, _)| *file == name).map(|(_, bytes)| *bytes)
}

pub(crate) fn resource_names() -> Vec<&'static str> {
    state().as_ref().map_or_else(Vec::new, |s| s.resources.iter().map(|(name, _)| *name).collect())
}

pub(crate) fn resource_text(name: &str) -> Option<String> {
    resource(name).map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

/// What a command's executor answered. Thrown, Paper logs it and tells the sender "An unexpected error
/// occurred while trying to execute that command", with no usage line after.
fn command_outcome(sender: &horizon_bukkit::CommandSender, what: &str, out: R) -> bool {
    match out {
        Ok(answer) => answer.truth(),
        Err(thrown) => {
            let _ = jrt::report(what, Err(thrown));
            sender.send_message("§cAn unexpected error occurred while trying to execute that command");
            true
        }
    }
}

/// Every object of the game is held behind a lock, since Java may change one through any reference.
fn register_commands(fallback: &str, commands: Vec<V>) -> R {
    let plugin = the_plugin();
    let mut all = true;
    for object in commands {
        let name = jrt::call(&object, "getName", &[])?.text();
        let mut command = horizon_bukkit::pure2::Command::new(&name);
        let aliases: Vec<String> = jrt::call(&object, "getAliases", &[]).ok().and_then(|v| v.items().ok()).unwrap_or_default().iter().map(V::text).collect();
        command.set_aliases(&aliases.iter().map(String::as_str).collect::<Vec<_>>());
        if let Ok(description) = jrt::call(&object, "getDescription", &[]) {
            command.set_description(&description.text());
        }
        if let Ok(permission) = jrt::call(&object, "getPermission", &[]) {
            if !permission.is_null() {
                command.set_permission(&permission.text());
            }
        }
        all &= horizon_bukkit::fam_core::get_command_map().register(&plugin, fallback, &command);
        let runs = object.clone();
        plugin.set_executor(&name, move |sender, label, args| {
            let list = jrt::arr(args.iter().map(|x| jrt::s(x)).collect());
            let done = command_outcome(sender, "execute", jrt::call(&runs, "execute", &[wrap(sender.clone()), jrt::s(label), list]));
            finish_while_paused();
            done
        });
        let completes = object.clone();
        plugin.set_tab_completer(&name, move |sender, alias, args| {
            let list = jrt::arr(args.iter().map(|x| jrt::s(x)).collect());
            jrt::call(&completes, "tabComplete", &[wrap(sender.clone()), jrt::s(alias), list]).ok().and_then(|v| v.items().ok()).unwrap_or_default().iter().map(V::text).collect()
        });
    }
    Ok(V::Bool(all))
}

/// An item's `PersistentDataContainer`: its `minecraft:custom_data` component, under `PublicBukkitValues`
/// as Bukkit keeps it. Holds the meta (a stack) it belongs to, so `setItemMeta` carries the values.
pub struct ItemDataH(V);

fn key_text(key: &V) -> String {
    match held::<horizon_bukkit::persistence::NamespacedKey>(key) {
        Some(key) => format!("{}:{}", key.get_namespace(), key.get_key()),
        None => key.text(),
    }
}

fn item_data(meta: &V, name: &str, a: &[V]) -> Option<R> {
    let mut stack = held::<horizon_bukkit::ItemStack>(meta)?;
    let mut data = stack.get_component("minecraft:custom_data").cloned().unwrap_or_else(|| serde_json::json!({}));
    if !data.is_object() {
        data = serde_json::json!({});
    }
    let mut values = data.get("PublicBukkitValues").and_then(Value::as_object).cloned().unwrap_or_default();
    let read = |value: &Value, kind: &str| -> V {
        match (kind, value) {
            (_, Value::Null) => V::Null,
            ("STRING", other) => jrt::s(&other.as_str().map_or_else(|| other.to_string(), str::to_owned)),
            ("INTEGER" | "SHORT" | "BYTE", other) => V::Int(other.as_i64().or_else(|| other.as_bool().map(i64::from)).unwrap_or(0) as i32),
            ("LONG", other) => V::Long(other.as_i64().unwrap_or(0)),
            ("DOUBLE" | "FLOAT", other) => V::Double(other.as_f64().unwrap_or(0.0)),
            ("BOOLEAN", other) => V::Bool(other.as_bool().unwrap_or_else(|| other.as_i64().unwrap_or(0) != 0)),
            (_, Value::String(text)) => jrt::s(text),
            (_, other) => jrt::javaio::from_json_map(&other.as_object().cloned().unwrap_or_default()),
        }
    };
    let kind = |at: usize| a.get(at).map(|k| k.text().rsplit('.').next().unwrap_or("").to_ascii_uppercase()).unwrap_or_default();
    match (name, a.len()) {
        ("set", 3) => {
            let value = match &a[2] {
                V::Bool(b) => serde_json::json!(*b as i32),
                other => jrt::javaio::to_json(other),
            };
            values.insert(key_text(&a[0]), value);
        }
        ("remove", 1) => {
            values.remove(&key_text(&a[0]));
        }
        ("get", 2) => return Some(Ok(values.get(&key_text(&a[0])).map_or(V::Null, |v| read(v, &kind(1))))),
        ("getOrDefault", 3) => return Some(Ok(values.get(&key_text(&a[0])).map_or_else(|| a[2].clone(), |v| read(v, &kind(1))))),
        ("has", 1 | 2) => return Some(Ok(V::Bool(values.contains_key(&key_text(&a[0]))))),
        ("isEmpty", 0) => return Some(Ok(V::Bool(values.is_empty()))),
        ("getKeys", 0) => return Some(Ok(jrt::list(values.keys().filter_map(|k| horizon_bukkit::persistence::NamespacedKey::from_string(k).map(wrap)).collect()))),
        ("getAdapterContext", 0) => return Some(Ok(jrt::native(jrt::javautil::JStub))),
        _ => return None,
    }
    if values.is_empty() {
        if let Some(object) = data.as_object_mut() {
            object.remove("PublicBukkitValues");
        }
    } else {
        data["PublicBukkitValues"] = Value::Object(values);
    }
    if data.as_object().is_some_and(serde_json::Map::is_empty) {
        stack.remove_component("minecraft:custom_data");
    } else {
        stack.set_component("minecraft:custom_data", data);
    }
    Some(Ok(V::Null))
}

/// A component as MiniMessage writes it: `<red><bold>text</bold></red>`, children inside their parent's tags.
fn mini_serialize(json: &Value) -> String {
    if let Some(text) = json.as_str() {
        return text.replace('<', r"\<");
    }
    let mut open = Vec::new();
    if let Some(color) = json.get("color").and_then(Value::as_str) {
        open.push(color.to_owned());
    }
    for decoration in ["bold", "italic", "underlined", "strikethrough", "obfuscated"] {
        if json.get(decoration).and_then(Value::as_bool) == Some(true) {
            open.push(decoration.to_owned());
        }
    }
    let mut out: String = open.iter().map(|tag| format!("<{tag}>")).collect();
    out.push_str(&json.get("text").and_then(Value::as_str).unwrap_or("").replace('<', r"\<"));
    for child in json.get("extra").and_then(Value::as_array).into_iter().flatten() {
        out.push_str(&mini_serialize(child));
    }
    for tag in open.iter().rev() {
        out.push_str(&format!("</{tag}>"));
    }
    out
}

/// MiniMessage placeholders (`Placeholder.parsed/unparsed/component`, `TagResolver.resolver(...)`): each
/// `<key>` and the MiniMessage text that stands in for it before the text is read.
pub struct ResolverH(pub Vec<(String, String)>);

fn resolvers_of(values: &[V], out: &mut Vec<(String, String)>) {
    for value in values {
        if let Some(found) = value.native::<ResolverH>() {
            out.extend(found.0.iter().cloned());
        } else if matches!(value, V::Arr(_) | V::List(_) | V::Set(_)) {
            if let Ok(items) = value.items() {
                resolvers_of(&items, out);
            }
        }
    }
}

/// `deserialize(text, resolvers...)`: `<key>` (and `<key/>`) replaced by each placeholder's text.
fn mini_with(text: &str, resolvers: &[(String, String)]) -> String {
    let mut out = text.to_owned();
    for (key, value) in resolvers {
        let key = key.to_ascii_lowercase();
        for tag in [format!("<{key}>"), format!("<{key}/>")] {
            out = out.replace(&tag, value);
        }
    }
    out
}

/// MiniMessage text without its tags; an escaped `\<` stays as text.
fn strip_mini_tags(text: &str) -> String {
    let mut out = String::new();
    let (mut depth, mut escaped) = (0, false);
    for c in text.chars() {
        match c {
            '\\' if !escaped => escaped = true,
            '<' if !escaped => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => {
                out.push(c);
                escaped = false;
            }
            _ => escaped = false,
        }
    }
    out
}

/// GUIs made with a holder, and that holder.
static HOLDERS: Mutex<Vec<(horizon_bukkit::Inventory, V)>> = Mutex::new(Vec::new());

/// A title or message as legacy `§` text, from a String or an Adventure component.
fn legacy_text(value: &V) -> String {
    component::static_method("LegacyComponentSerializer", "legacySection", &[])
        .and_then(Result::ok)
        .and_then(|serializer| jrt::call(&serializer, "serialize", std::slice::from_ref(value)).ok())
        .map_or_else(|| value.text(), |text| text.text())
}

/// A Material from either form it travels in: the game's wrapped one, or its name (`Material.STONE`).
pub fn material(value: &V) -> Option<horizon_bukkit::Material> {
    match value {
        V::Str(name) if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':')) => Some(horizon_bukkit::Material::of(name)),
        other => held::<horizon_bukkit::Material>(other).map(|m| m.clone()),
    }
}

/// A game object's kind, for `JRT_PROFILE=1`.
fn native_label(v: &V) -> &'static str {
    use horizon_bukkit as hb;
    if v.native::<Mutex<hb::Material>>().is_some() { return "Material"; }
    if v.native::<Mutex<hb::Block>>().is_some() { return "Block"; }
    if v.native::<Mutex<hb::BlockState>>().is_some() { return "BlockState"; }
    if v.native::<Mutex<hb::block_data::TileState>>().is_some() { return "TileState"; }
    if v.native::<Mutex<hb::block_data::BlockData>>().is_some() { return "BlockData"; }
    if v.native::<Mutex<hb::Location>>().is_some() { return "Location"; }
    if v.native::<Mutex<hb::World>>().is_some() { return "World"; }
    if v.native::<Mutex<hb::ItemStack>>().is_some() { return "ItemStack"; }
    if v.native::<Mutex<Player>>().is_some() { return "Player"; }
    if v.native::<Mutex<hb::entity::Entity>>().is_some() { return "Entity"; }
    if v.native::<Mutex<hb::Inventory>>().is_some() { return "Inventory"; }
    "game"
}

fn material_fast(this: &V, name: &str, a: &[V]) -> Option<R> {
    if !matches!((name, a.len()), ("name" | "toString", 0) | ("equals", 1)) {
        return None;
    }
    let mine = held::<horizon_bukkit::Material>(this)?.name();
    if name != "equals" {
        return Some(Ok(jrt::s(&mine)));
    }
    if let (V::Native(x), V::Native(y)) = (this, &a[0])
        && std::sync::Arc::ptr_eq(x, y)
    {
        return Some(Ok(V::Bool(true)));
    }
    // `type.equals(Material.WATER)`: the plugin's constant is its name here (jrt::same's rule), so it was a miss
    // through every stage, 235k times in 30 s of CoreProtect's water logging.
    if let V::Str(constant) = &a[0] {
        return Some(Ok(V::Bool(!constant.is_empty() && constant.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_') && **constant == *mine)));
    }
    if a[0].is_null() {
        return Some(Ok(V::Bool(false)));
    }
    let other = held::<horizon_bukkit::Material>(&a[0])?.name();
    Some(Ok(V::Bool(mine == other)))
}

const MATERIAL_ONLY: &[&str] = &[
    "isBlock", "isItem", "isSolid", "isAir", "isEdible", "isFlammable", "isBurnable", "isTransparent", "isOccluding", "isInteractable",
    "hasGravity", "getMaxStackSize", "getMaxDurability", "getKey", "getHardness", "getBlastResistance", "isFuel", "isRecord", "isCollidable",
    "getTranslationKey", "getItemTranslationKey", "getBlockTranslationKey", "asItemType", "asBlockType", "getEquipmentSlot", "getCreativeCategory",
    "isLegacy", "getSlipperiness", "getCraftingRemainingItem", "getCompostChance", "isCompostable", "getItemRarity", "createBlockData", "key",
];

/// A constant's name (`DUST`, `ZOMBIE`): upper case, digits and underscores.
fn entity_type_like(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

pub fn wrap<T: std::any::Any + Send>(value: T) -> V {
    let held: Box<dyn std::any::Any> = Box::new(value);
    match held.downcast::<CommandSender>() {
        Ok(sender) => match *sender {
            CommandSender::Player(player) => jrt::native(Mutex::new(player)),
            other => jrt::native(Mutex::new(other)),
        },
        // `Player.getScoreboard()`: the board the client shows, as the type that board's methods are on.
        Err(other) => match other.downcast::<horizon_bukkit::fam_entity::ShownScoreboard>() {
            Ok(shown) => match *shown {
                horizon_bukkit::fam_entity::ShownScoreboard::Game(board) => jrt::native(Mutex::new(board)),
                horizon_bukkit::fam_entity::ShownScoreboard::Plugin(board) => jrt::native(Mutex::new(board)),
            },
            // An event's entity that may be a player (`EntityDamageByEntityEvent.getEntity()`): the player or the
            // entity itself, which is what has the methods.
            Err(other) => match other.downcast::<horizon_bukkit::fam_ev_entity::Actor>() {
                Ok(actor) => match *actor {
                    horizon_bukkit::fam_ev_entity::Actor::Player(player) => jrt::native(Mutex::new(player)),
                    horizon_bukkit::fam_ev_entity::Actor::Entity(entity) => jrt::native(Mutex::new(entity)),
                },
                // The same, maybe absent (`FoodLevelChangeEvent.getEntity()` gave an object of no Bukkit class, and
                // EssentialsX's hunger listener failed casting it to HumanEntity): the player or entity, or null.
                Err(other) => match other.downcast::<Option<horizon_bukkit::fam_ev_entity::Actor>>() {
                    Ok(actor) => match *actor {
                        Some(actor) => wrap(actor),
                        None => V::Null,
                    },
                    Err(other) => jrt::native(Mutex::new(*other.downcast::<T>().expect("the same type"))),
                },
            },
        },
    }
}

pub fn held<T: 'static>(value: &V) -> Option<MutexGuard<'_, T>> {
    value.native::<Mutex<T>>().map(jrt::lock)
}

/// `FileConfiguration`, from the file, with the jar's copy as its defaults.
fn config_object() -> V {
    if let Some(state) = state().as_ref() {
        if !state.config.is_null() {
            return state.config.clone();
        }
    }
    let loaded = wrap(load_config());
    if let Some(state) = state().as_mut() {
        state.config = loaded.clone();
    }
    loaded
}

fn load_config() -> YamlConfiguration {
    let mut loaded = the_plugin().get_config();
    if let Some(text) = resource_text("config.yml") {
        loaded.set_defaults(config::parse(&text));
    }
    loaded
}

fn run_of(task: V) -> impl FnMut() + Send + 'static {
    move || {
        let _ = jrt::report("task", jrt::call(&task, "run", &[]));
    }
}

fn run_once(task: V) -> impl FnOnce() + Send + 'static {
    move || {
        let _ = jrt::report("task", jrt::call(&task, "run", &[]));
    }
}

/// `BukkitScheduler.runTask*(plugin, task, ...)` and `BukkitRunnable.runTask*(plugin, ...)`.
fn schedule(name: &str, task: V, times: &[V]) -> Option<R> {
    let n = |i: usize| times.get(i).map_or(0, |v| v.long().max(0) as u64);
    let id = match name {
        "runTask" | "scheduleSyncDelayedTask" if times.is_empty() => scheduler::run_task(run_of(task.clone())),
        "runTaskLater" | "scheduleSyncDelayedTask" => scheduler::run_task_later(n(0).max(1), run_of(task.clone())),
        "runTaskTimer" | "scheduleSyncRepeatingTask" => scheduler::run_task_timer(n(0).max(1), n(1).max(1), run_of(task.clone())),
        "runTaskAsynchronously" | "runTaskLaterAsynchronously" | "runTaskTimerAsynchronously" => {
            match name {
                "runTaskAsynchronously" => {
                    scheduler::run_task_asynchronously(run_once(task.clone()));
                }
                "runTaskLaterAsynchronously" => {
                    let (delay, task) = (n(0), task.clone());
                    scheduler::run_task_asynchronously(move || {
                        std::thread::sleep(std::time::Duration::from_millis(delay * 50));
                        run_once(task)();
                    });
                }
                _ => {
                    let (delay, period, task) = (n(0), n(1).max(1), task.clone());
                    scheduler::run_task_asynchronously(move || {
                        std::thread::sleep(std::time::Duration::from_millis(delay * 50));
                        while !task.get("$cancelled").is_ok_and(|c| c.truth()) {
                            let _ = jrt::report("task", jrt::call(&task, "run", &[]));
                            std::thread::sleep(std::time::Duration::from_millis(period * 50));
                        }
                    });
                }
            }
            0
        }
        _ => return None,
    };
    let _ = task.set("$task", V::Int(id as i32));
    Some(Ok(if name.starts_with("schedule") { V::Int(id as i32) } else { jrt::native(TaskH(id)) }))
}

/// What the plugin's own classes get from `BukkitRunnable` and from `Object`.
fn object_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(found) = xplugin::expansion_method(this, name, a) {
        return Some(found);
    }
    if name.starts_with("runTask") {
        return schedule(name, this.clone(), a.get(1..).unwrap_or_default());
    }
    match name {
        "cancel" => {
            let _ = this.set("$cancelled", V::Bool(true));
            if let Ok(id) = this.get("$task") {
                if !id.is_null() {
                    scheduler::cancel_task(id.int() as u32);
                }
            }
            Some(Ok(V::Null))
        }
        "isCancelled" => Some(this.get("$cancelled").map(|c| V::Bool(c.truth()))),
        "getTaskId" => Some(this.get("$task")),
        _ => None,
    }
}

fn plugin_method(this: &V, plugin: &Plugin, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        // `getFile()`: the plugin's file in `plugins/`, as Paper names its jar.
        ("getFile", 0) => jrt::javaio::file(format!("plugins/{}.jar", plugin.get_name())),
        // `super.onCommand(...)` / `super.onTabComplete(...)`: JavaPlugin's own, which do nothing.
        ("onCommand", 4) => V::Bool(false),
        ("onTabComplete", 4) => V::Null,
        ("onLoad" | "onEnable" | "onDisable", 0) => V::Null,
        ("getCommand", 1) => jrt::native(CommandH(a[0].text())),
        // The plugin's class loader: what it is asked for here is the jar's resources.
        ("getClassLoader", 0) => jrt::native(jrt::javautil::JPlain),
        ("getConfig", 0) => config_object(),
        ("saveConfig", 0) => {
            let object = config_object();
            let current = held::<YamlConfiguration>(&object)?;
            plugin.save_config(&current);
            V::Null
        }
        ("reloadConfig", 0) => {
            let object = config_object();
            *held::<YamlConfiguration>(&object)? = load_config();
            V::Null
        }
        ("saveDefaultConfig", 0) => {
            if let Some(text) = resource_text("config.yml") {
                plugin.save_default_config(&text);
            } else if !plugin.get_data_folder().join("config.yml").exists() {
                // Paper's saveResource throws when the jar has none, and the plugin then fails to enable.
                return Some(Err(jrt::exc("IllegalArgumentException", "The embedded resource 'config.yml' cannot be found")));
            }
            V::Null
        }
        ("saveResource", 2) => {
            let name = a[0].text().replace('\\', "/");
            let Some(bytes) = resource(&name) else {
                return Some(Err(jrt::exc("IllegalArgumentException", format!("The embedded resource '{name}' cannot be found"))));
            };
            let path = plugin.get_data_folder().join(&name);
            if !path.exists() || a[1].truth() {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(path, bytes);
            }
            V::Null
        }
        ("getResource", 1) => resource(&a[0].text()).map_or(V::Null, |bytes| jrt::javaio::stream(bytes.to_vec())),
        ("getLogger" | "getSLF4JLogger" | "getComponentLogger", 0) => jrt::native(LoggerH),
        ("getDescription" | "getPluginMeta", 0) => {
            let _ = this;
            let text = resource_text("paper-plugin.yml").or_else(|| resource_text("plugin.yml")).unwrap_or_default();
            let mut out = YamlConfiguration::new();
            if let Value::Object(map) = config::parse(&text) {
                for (key, value) in map {
                    out.set(&key, value);
                }
            }
            jrt::native(DescH(out))
        }
        _ => return None,
    }))
}

/// `FixedMetadataValue`: its plugin and the value itself, with `MetadataValueAdapter`'s conversions.
pub struct FixedMetaH {
    plugin: V,
    value: V,
}

fn fixed_meta_method(meta: &FixedMetaH, name: &str, a: &[V]) -> Option<R> {
    let value = &meta.value;
    let number = || match value {
        V::Int(_) | V::Long(_) | V::Double(_) | V::Char(_) => Some(value.double()),
        V::Str(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    Some(Ok(match (name, a.len()) {
        ("value", 0) => value.clone(),
        ("getOwningPlugin", 0) => meta.plugin.clone(),
        ("invalidate", 0) => V::Null,
        ("asString", 0) => if value.is_null() { jrt::s("") } else { jrt::s(&jrt::call(value, "toString", &[]).map(|t| t.text()).unwrap_or_else(|_| value.text())) },
        ("asBoolean", 0) => V::Bool(match value {
            V::Bool(b) => *b,
            V::Str(text) => text.eq_ignore_ascii_case("true"),
            V::Null => false,
            _ => number().map_or(true, |n| n != 0.0),
        }),
        ("asInt" | "asShort" | "asByte", 0) => V::Int(number().unwrap_or(0.0) as i32),
        ("asLong", 0) => V::Long(number().unwrap_or(0.0) as i64),
        ("asDouble" | "asFloat", 0) => V::Double(number().unwrap_or(0.0)),
        _ => return None,
    }))
}

/// Bukkit's `Metadatable` on players, entities, blocks and worlds: values kept as Java handed them over,
/// by holder (its UUID, else its text) and key, one per plugin.
static METADATA: Mutex<std::collections::BTreeMap<(String, String), Vec<(String, V)>>> = Mutex::new(std::collections::BTreeMap::new());

fn metadata_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if !matches!((name, a.len()), ("setMetadata", 2) | ("getMetadata" | "hasMetadata", 1) | ("removeMetadata", 2)) || !matches!(this, V::Native(_)) {
        return None;
    }
    let holder = match jrt::call(this, "getUniqueId", &[]) {
        Ok(id) if !id.is_null() => id.text(),
        _ => match jrt::call(this, "getLocation", &[]) {
            Ok(at) if !at.is_null() => at.text(),
            _ => this.text(),
        },
    };
    let key = (holder, a[0].text());
    let owner = |v: &V| jrt::call(v, "getOwningPlugin", &[]).map(|p| p.text()).unwrap_or_default();
    let mut all = jrt::lock(&METADATA);
    Some(Ok(match name {
        "setMetadata" => {
            let plugin = owner(&a[1]);
            let values = all.entry(key).or_default();
            values.retain(|(known, _)| *known != plugin);
            values.push((plugin, a[1].clone()));
            V::Null
        }
        "getMetadata" => jrt::list(all.get(&key).map(|v| v.iter().map(|(_, x)| x.clone()).collect()).unwrap_or_default()),
        "hasMetadata" => V::Bool(all.get(&key).is_some_and(|v| !v.is_empty())),
        _ => {
            let plugin = a[1].text();
            if let Some(values) = all.get_mut(&key) {
                values.retain(|(known, _)| *known != plugin && !plugin.contains(known.as_str()));
            }
            V::Null
        }
    }))
}

fn desc_method(d: &DescH, name: &str, a: &[V]) -> Option<R> {
    let y = &d.0;
    let text = |k: &str| y.get_string(k).map_or(V::Null, |s| jrt::s(&s));
    let list = |k: &str| jrt::list(y.get_string_list(k).iter().map(|s| jrt::s(s)).collect());
    if !a.is_empty() {
        return None;
    }
    Some(Ok(match name {
        "getName" | "getRawName" => text("name"),
        "getVersion" => text("version"),
        "getDescription" => text("description"),
        "getMain" | "getMainClass" => text("main"),
        "getWebsite" => text("website"),
        "getPrefix" | "getLoggerPrefix" => text("prefix"),
        "getAPIVersion" | "getApiVersion" => text("api-version"),
        "getFullName" | "getDisplayName" => jrt::s(&format!("{} v{}", y.get_string("name").unwrap_or_default(), y.get_string("version").unwrap_or_default())),
        "getAuthors" => {
            let mut all = y.get_string_list("authors");
            if let Some(one) = y.get_string("author") {
                all.insert(0, one);
            }
            jrt::list(all.iter().map(|s| jrt::s(s)).collect())
        }
        "getContributors" => list("contributors"),
        "getDepend" | "getPluginDependencies" => list("depend"),
        "getSoftDepend" | "getPluginSoftDependencies" => list("softdepend"),
        "getLoadBefore" | "getLoadBeforePlugins" => list("loadbefore"),
        "getProvides" | "getProvidedPlugins" => list("provides"),
        "getLibraries" => list("libraries"),
        "getCommands" => {
            let fresh = || jrt::new_other("LinkedHashMap", &[]).unwrap_or(V::Null);
            let map = fresh();
            for key in y.get_keys("commands") {
                let _ = jrt::call(&map, "put", &[jrt::s(&key), fresh()]);
            }
            map
        }
        "toString" => jrt::s(&format!("{} v{}", y.get_string("name").unwrap_or_default(), y.get_string("version").unwrap_or_default())),
        _ => return None,
    }))
}

fn command_method(handle: &CommandH, name: &str, a: &[V]) -> Option<R> {
    let plugin = the_plugin();
    Some(Ok(match name {
        "setExecutor" => {
            let (executor, command) = (a[0].clone(), handle.0.clone());
            plugin.set_executor(&handle.0, move |sender, label, args| {
                let list = jrt::arr(args.iter().map(|x| jrt::s(x)).collect());
                let out = jrt::call(&executor, "onCommand", &[wrap(sender.clone()), jrt::native(CommandH(command.clone())), jrt::s(label), list]);
                let done = command_outcome(sender, "onCommand", out);
                finish_while_paused();
                done
            });
            V::Null
        }
        "setTabCompleter" => {
            let (completer, command) = (a[0].clone(), handle.0.clone());
            plugin.set_tab_completer(&handle.0, move |sender, label, args| {
                let list = jrt::arr(args.iter().map(|x| jrt::s(x)).collect());
                let out = jrt::call(&completer, "onTabComplete", &[wrap(sender.clone()), jrt::native(CommandH(command.clone())), jrt::s(label), list]);
                jrt::report("onTabComplete", out).items().unwrap_or_default().iter().map(V::text).collect()
            });
            V::Null
        }
        "getName" | "getLabel" | "toString" => jrt::s(&handle.0),
        "getExecutor" | "getTabCompleter" | "getPermission" | "getUsage" | "getDescription" => V::Null,
        "getAliases" => jrt::list(Vec::new()),
        "setDescription" | "setUsage" | "setPermission" | "setAliases" | "setLabel" | "setPermissionMessage" => V::Null,
        _ => return None,
    }))
}

fn logger_method(name: &str, a: &[V]) -> Option<R> {
    let plugin = the_plugin();
    let say = |level: &str, text: &str| match level {
        "SEVERE" => plugin.severe(text),
        "WARNING" => plugin.warning(text),
        "FINE" | "FINER" | "FINEST" | "CONFIG" | "OFF" => {}
        _ => plugin.info(text),
    };
    let text = |v: &V| if matches!(v, V::Func(_)) { jrt::call(v, "get", &[]).map_or_else(|_| String::new(), |t| t.text()) } else { v.text() };
    match name {
        "info" | "warning" | "warn" | "severe" | "error" | "fine" | "finer" | "finest" | "config" | "debug" | "trace" => {
            let level = match name {
                "info" => "INFO",
                "warning" | "warn" => "WARNING",
                "severe" | "error" => "SEVERE",
                _ => "FINE",
            };
            // SLF4J's `info("{} joined", name)`: each `{}` takes the next argument.
            let mut line = text(a.first()?);
            // SLF4J / Log4j: a last argument that is a throwable and no `{}` takes is printed after the line
            // (FAWE's "Error performing chunk call at chunk {},{}" carried its exception and showed nothing).
            let thrown = a.last().filter(|v| a.len() > 1 && a.len() - 1 > line.matches("{}").count() && (matches!(v, V::Exc(_)) || jrt::instance_of(v, "Throwable")));
            let thrown_text = thrown.map(|t| format!("\n  {}  at {}", jrt::throwable_text(t), jrt::take_trace()));
            if a.len() > 1 && line.contains("{}") {
                let mut rest = a[1..].iter().flat_map(|v| if matches!(v, V::Arr(_)) { v.items().unwrap_or_default() } else { vec![v.clone()] });
                let mut out = String::new();
                let mut pieces = line.split("{}").peekable();
                while let Some(piece) = pieces.next() {
                    out.push_str(piece);
                    if pieces.peek().is_some() {
                        out.push_str(&rest.next().map_or_else(|| "{}".to_owned(), |v| v.text()));
                    }
                }
                line = out;
            }
            if let Some(extra) = thrown_text {
                line.push_str(&extra);
            }
            say(level, &line);
        }
        // `log(record)`: a `LogRecord`'s level and its message with the parameters filled in.
        "log" if a.len() == 1 && a[0].native::<jrt::javautil::JLogRecord>().is_some() => {
            let record = a[0].native::<jrt::javautil::JLogRecord>()?;
            say(&record.level(), &record.text());
        }
        "log" => match a.get(2) {
            Some(thrown) if !thrown.is_null() => say(&a[0].text(), &format!("{}: {} at {}", text(a.get(1)?), thrown.text(), jrt::take_trace())),
            _ => say(&a[0].text(), &text(a.get(1)?)),
        },
        // Log4j's `catching(t)` (an error with its trace) and `throwing(t)` (the same, giving t back).
        "catching" | "throwing" if !a.is_empty() => {
            let thrown = a.last()?;
            say("SEVERE", &format!("Catching {}  at {}", jrt::throwable_text(thrown), jrt::take_trace()));
            if name == "throwing" {
                return Some(Ok(thrown.clone()));
            }
        }
        "getName" => return Some(Ok(jrt::s(&the_plugin().get_name().to_owned()))),
        // java.util.logging's handler plumbing (WorldEdit's command log): taken, and lines still go to the console.
        "setLevel" | "setParent" | "setUseParentHandlers" | "addHandler" | "removeHandler" | "setFilter" | "setResourceBundle" => {}
        "getHandlers" => return Some(Ok(jrt::arr(Vec::new()))),
        "getUseParentHandlers" => return Some(Ok(V::Bool(true))),
        "getParent" | "getLevel" | "getFilter" | "getResourceBundle" => return Some(Ok(V::Null)),
        "isEnabled" => return Some(Ok(V::Bool(true))),
        "isLoggable" => return Some(Ok(V::Bool(true))),
        // SLF4J's level questions: debug and trace lines are dropped here, the rest are written.
        "isInfoEnabled" | "isWarnEnabled" | "isErrorEnabled" => return Some(Ok(V::Bool(true))),
        "isDebugEnabled" | "isTraceEnabled" => return Some(Ok(V::Bool(false))),
        _ => return None,
    }
    Some(Ok(V::Null))
}

fn config_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    Some(Ok(match (name, a.len()) {
        ("getKeys", 1) => {
            let cfg = held::<YamlConfiguration>(this)?;
            let keys = if a[0].truth() { deep_keys(&cfg) } else { cfg.get_keys("") };
            jrt::list(keys.iter().map(|k| jrt::s(k)).collect())
        }
        ("getValues", 1) => {
            let values = held::<YamlConfiguration>(this)?.get_values();
            jrt::javaio::from_json_map(&if a[0].truth() { deep_values(&Value::Object(values)) } else { values })
        }
        ("setDefaults", 1) => {
            let values = match held::<YamlConfiguration>(&a[0]) {
                Some(other) => Value::Object(other.get_values()),
                None => jrt::javaio::to_json(&a[0]),
            };
            jrt::lock(&DEFAULTS).get_or_insert_default().insert(identity(this), values.clone());
            held::<YamlConfiguration>(this)?.set_defaults(values);
            V::Null
        }
        ("options", 0) => jrt::native(OptionsH(this.clone())),
        // `contains(path, ignoreDefault)`: this file's own value, defaults aside.
        ("contains", 2) => V::Bool(held::<YamlConfiguration>(this)?.contains(&a[0].text())),
        // `loadFromString(text)`: the text replaces what it held.
        ("loadFromString", 1) => {
            let mut fresh = YamlConfiguration::new();
            if let Value::Object(map) = config::parse(&a[0].text()) {
                for (key, value) in map {
                    fresh.set(&key, value);
                }
            }
            *held::<YamlConfiguration>(this)? = fresh;
            V::Null
        }
        // `load(reader)`: the stream's text, as `loadFromString` (a file goes to `load` below).
        ("load", 1) if jrt::javaio::path_of(&a[0]).is_none() => {
            let text = jrt::javaio::bytes_of(&a[0]).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
            return config_method(this, "loadFromString", &[jrt::s(&text)]);
        }
        ("saveToString", 0) => jrt::s(&held::<YamlConfiguration>(this)?.save_to_string()),
        // What `setDefaults` gave it, or for the plugin's own config the jar's config.yml: null when neither.
        ("getDefaults", 0) => {
            let stored = jrt::lock(&DEFAULTS).get_or_insert_default().get(&identity(this)).cloned();
            let main = state().as_ref().is_some_and(|s| identity(&s.config) == identity(this));
            let values = stored.or_else(|| if main { resource_text("config.yml").map(|text| config::parse(&text)) } else { None });
            match values {
                Some(Value::Object(map)) => {
                    let mut out = YamlConfiguration::new();
                    for (key, value) in map {
                        out.set(&key, value);
                    }
                    wrap(out)
                }
                _ => V::Null,
            }
        }
        // `getLong(path, default)` and the rest: the default when the path says nothing.
        ("getLong" | "getDouble" | "getInt" | "getBoolean" | "getString" | "getList" | "getItemStack" | "getLocation" | "getColor" | "getVector" | "get", 2) => {
            let present = held::<YamlConfiguration>(this)?.contains(&a[0].text());
            return Some(if present { jrt::call(this, name, &a[..1]) } else { Ok(a[1].clone()) });
        }
        ("getConfigurationSection", 1) => {
            let path = a[0].text();
            if held::<YamlConfiguration>(this)?.is_configuration_section(&path) { section(this, path) } else { V::Null }
        }
        ("createSection", 1 | 2) => {
            let path = a[0].text();
            let values = a.get(1).map_or_else(|| Value::Object(serde_json::Map::new()), jrt::javaio::to_json);
            held::<YamlConfiguration>(this)?.set(&path, values);
            section(this, path)
        }
        ("getParent", 0) => V::Null,
        ("getRoot", 0) => this.clone(),
        ("getName" | "getCurrentPath", 0) => jrt::s(""),
        ("set", 2) => {
            held::<YamlConfiguration>(this)?.set(&a[0].text(), jrt::javaio::to_json(&a[1]));
            V::Null
        }
        ("load", 1) => {
            let path = jrt::javaio::path_of(&a[0])?;
            *held::<YamlConfiguration>(this)? = YamlConfiguration::load_configuration(&path);
            V::Null
        }
        _ => return None,
    }))
}

/// A live part of a configuration: `ConfigurationSection`.
pub struct SectionH {
    root: V,
    path: String,
}

fn section(root: &V, path: String) -> V {
    jrt::native(SectionH { root: root.clone(), path })
}

fn section_method(sec: &SectionH, this: &V, name: &str, a: &[V]) -> Option<R> {
    let full = |tail: &str| format!("{}.{tail}", sec.path);
    Some(match (name, a.len()) {
        ("getKeys", 1) => {
            let cfg = held::<YamlConfiguration>(&sec.root)?;
            let keys: Vec<String> = if a[0].truth() {
                let prefix = full("");
                deep_keys(&cfg).into_iter().filter_map(|k| k.strip_prefix(&prefix).map(str::to_owned)).collect()
            } else {
                cfg.get_keys(&sec.path)
            };
            Ok(jrt::list(keys.iter().map(|k| jrt::s(k)).collect()))
        }
        ("getValues", 1) => Ok(held::<YamlConfiguration>(&sec.root)?.get(&sec.path).map_or_else(|| jrt::javaio::from_json_map(&serde_json::Map::new()), |v| if a[0].truth() { jrt::javaio::from_json_map(&deep_values(&v)) } else { jrt::javaio::from_json(&v) })),
        ("getName", 0) => Ok(jrt::s(sec.path.rsplit('.').next().unwrap_or_default())),
        ("getCurrentPath", 0) => Ok(jrt::s(&sec.path)),
        ("getRoot", 0) => Ok(sec.root.clone()),
        ("getParent", 0) => Ok(match sec.path.rsplit_once('.') {
            Some((parent, _)) => section(&sec.root, parent.to_owned()),
            None => sec.root.clone(),
        }),
        ("hashCode", 0) | ("equals", 1) | ("toString", 0) => return None,
        _ if !a.is_empty() && matches!(a[0], V::Str(_)) => {
            let mut args = a.to_vec();
            args[0] = jrt::s(&full(&a[0].text()));
            jrt::call(&sec.root, name, &args)
        }
        _ => {
            let _ = this;
            return None;
        }
    })
}

fn deep_keys(cfg: &YamlConfiguration) -> Vec<String> {
    fn walk(prefix: &str, value: &Value, out: &mut Vec<String>) {
        if let Some(map) = value.as_object() {
            for (key, inner) in map {
                let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                out.push(path.clone());
                walk(&path, inner, out);
            }
        }
    }
    let mut out = Vec::new();
    walk("", &Value::Object(cfg.get_values()), &mut out);
    out
}

/// `getValues(true)`: every path under `section`, a section before what's in it, each with its value (a
/// section's is its map), as Bukkit's deep `getValues` lists them.
fn deep_values(section: &Value) -> serde_json::Map<String, Value> {
    fn walk(prefix: &str, value: &Value, out: &mut serde_json::Map<String, Value>) {
        if let Some(map) = value.as_object() {
            for (key, inner) in map {
                let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                out.insert(path.clone(), inner.clone());
                walk(&path, inner, out);
            }
        }
    }
    let mut out = serde_json::Map::new();
    walk("", section, &mut out);
    out
}

fn identity(value: &V) -> usize {
    match value {
        V::Native(native) => std::sync::Arc::as_ptr(native).cast::<()>() as usize,
        _ => 0,
    }
}

/// UniversalScheduler's `TaskScheduler` (a library plugins fetch at run time for Folia): Bukkit's scheduler
/// with the plugin already given. Its region and entity variants (`runTask(entity, task)`) run on the server
/// thread, as it does on Paper.
pub struct UniSchedH(V);

fn uni_scheduler_method(uni: &UniSchedH, name: &str, a: &[V]) -> Option<R> {
    let bukkit = jrt::native(SchedulerH);
    // A leading entity or location says where on Folia; on Paper it is the main thread all the same.
    let rest = match a.first() {
        Some(first) if matches!(first, V::Native(_)) && !matches!(name, "cancelTask") => &a[1..],
        _ => a,
    };
    let task = |bukkit_name: &str, args: &[V]| {
        let mut full = vec![uni.0.clone()];
        full.extend_from_slice(args);
        jrt::call(&bukkit, bukkit_name, &full)
    };
    Some(match name {
        "isGlobalThread" | "isTickThread" | "isEntityThread" | "isRegionThread" => Ok(V::Bool(horizon_bukkit::is_primary_thread())),
        "runTask" | "runTaskLater" | "runTaskTimer" | "runTaskAsynchronously" | "runTaskLaterAsynchronously" | "runTaskTimerAsynchronously" => task(name, rest),
        "execute" => task("runTask", rest),
        "scheduleSyncDelayedTask" => task("runTaskLater", rest),
        "scheduleSyncRepeatingTask" => task("runTaskTimer", rest),
        "callSyncMethod" => jrt::call(&bukkit, "callSyncMethod", &[uni.0.clone(), rest.first().cloned().unwrap_or(V::Null)]),
        "cancelTasks" => jrt::call(&bukkit, "cancelTasks", &[uni.0.clone()]),
        "cancelTask" => jrt::call(&bukkit, "cancelTask", a),
        _ => return None,
    })
}

/// Config headers and footers by config and kind (`<identity>#header`).
static HEADERS: Mutex<std::collections::BTreeMap<String, Vec<V>>> = Mutex::new(std::collections::BTreeMap::new());

fn options_method(options: &OptionsH, this: &V, name: &str, a: &[V]) -> Option<R> {
    match (name, a.len()) {
        // The getter: whether defaults are copied, which Bukkit starts as false.
        ("copyDefaults", 0) => Some(Ok(V::Bool(false))),
        ("copyDefaults", 1) if a[0].truth() => {
            let defaults = jrt::lock(&DEFAULTS).get_or_insert_default().get(&identity(&options.0)).cloned();
            if let Some(defaults) = defaults {
                held::<YamlConfiguration>(&options.0)?.copy_defaults(&defaults);
            }
            Some(Ok(this.clone()))
        }
        // The header and footer a plugin sets, read back as it left them (Paper: `List<String>`; the old `header()`: one text).
        ("setHeader" | "setFooter" | "header", 1) => {
            let key = format!("{}#{}", identity(&options.0), if name == "setFooter" { "footer" } else { "header" });
            let lines = match &a[0] {
                V::Null => Vec::new(),
                V::Str(text) => text.lines().map(jrt::s).collect(),
                other => other.items().unwrap_or_default(),
            };
            jrt::lock(&HEADERS).insert(key, lines);
            Some(Ok(this.clone()))
        }
        ("getHeader" | "getFooter" | "header" | "footer", 0) => {
            let key = format!("{}#{}", identity(&options.0), if name.ends_with("ooter") { "footer" } else { "header" });
            let lines = jrt::lock(&HEADERS).get(&key).cloned().unwrap_or_default();
            Some(Ok(if name == "header" { jrt::s(&lines.iter().map(V::text).collect::<Vec<_>>().join("\n")) } else { jrt::list(lines) }))
        }
        ("copyDefaults" | "parseComments" | "pathSeparator" | "indent" | "width" | "codePointLimit" | "copyHeader", 1) => Some(Ok(this.clone())),
        // The getters, at Bukkit's defaults.
        ("indent", 0) => Some(Ok(V::Int(2))),
        ("width", 0) => Some(Ok(V::Int(80))),
        ("codePointLimit", 0) => Some(Ok(V::Int(3 * 1024 * 1024))),
        _ => None,
    }
}

thread_local! {
    static IN_ENTITY_CLASS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What the game's `Entity` alone doesn't answer goes to the class of what it is (`(ArmorStand)
/// world.spawn(..)`'s `setMarker`); and a class's value answers what every entity has through its `Entity`.
///
/// Then the interfaces it is (`entity_is`): a pig's `getAge` is `Ageable`'s, a wolf's `isTamed` `Tameable`'s
/// (CoreProtect reads them for every kill; the pig's kill threw "getAge is not converted").
macro_rules! entity_classes {
    (concrete: [$($class:ident),* $(,)?], interfaces: [$($iface:ident),* $(,)?] $(,)?) => {
        fn entity_class_call(this: &V, name: &str, a: &[V]) -> Option<R> {
            use horizon_bukkit::entity::{Entity, EntityClass};
            // Object's own: no entity class has them, and each class tried walks the whole dispatch again.
            if IN_ENTITY_CLASS.get() || matches!((name, a.len()), ("equals", 1) | ("hashCode", 0)) {
                return None;
            }
            let candidates: Vec<V> = if let Some(entity) = held::<Entity>(this).map(|e| e.clone()) {
                let kind = entity.get_type();
                let kind = kind.trim_start_matches("minecraft:");
                if kind.is_empty() {
                    return None;
                }
                let mut found = Vec::new();
                $(
                    if found.is_empty() && entity_type_of_class(stringify!($class)).eq_ignore_ascii_case(kind) {
                        found.push(wrap(<horizon_bukkit::entities::$class as EntityClass>::of(entity.clone())));
                    }
                )*
                $(
                    if entity_is(kind, stringify!($iface)) == Some(true) {
                        found.push(wrap(<horizon_bukkit::entities::$iface as EntityClass>::of(entity.clone())));
                    }
                )*
                found
            } else if let Some(player) = held::<Player>(this).map(|p| p.clone()) {
                // What a player has as a `LivingEntity` and the SDK keeps on `Entity` (`getMaximumNoDamageTicks`).
                let id = player.get_entity_id();
                if id <= 0 {
                    return None;
                }
                vec![wrap(Entity::of(id as u64, &player.get_world()))]
            } else {
                let mut found = None;
                $(
                    if found.is_none() {
                        if let Some(held) = held::<horizon_bukkit::entities::$class>(this) {
                            found = Some(held.0.clone());
                        }
                    }
                )*
                $(
                    if found.is_none() {
                        if let Some(held) = held::<horizon_bukkit::entities::$iface>(this) {
                            found = Some(held.0.clone());
                        }
                    }
                )*
                // A class's value: its Entity, which then also tries the other classes it is.
                vec![wrap(found?)]
            };
            for other in candidates {
                IN_ENTITY_CLASS.set(true);
                let answer = jrt::try_native(&other, name, a);
                IN_ENTITY_CLASS.set(false);
                if answer.is_some() {
                    return answer;
                }
            }
            None
        }
    };
}

entity_classes!(concrete: [
    Allay, Armadillo, ArmorStand, Axolotl, Bee, BlockDisplay, Bogged, Cat, Chicken, Cow, Creaking, Creeper, Dolphin,
    EnderCrystal, EnderSignal, Enderman, Horse, Painting, Slime, TropicalFish, Endermite, EvokerFangs, ExperienceOrb,
    FallingBlock, Firework, Fox, Frog, Ghast, GlowSquid, Goat, Hoglin, Interaction, IronGolem, Item, ItemDisplay,
    ItemFrame, Llama, Minecart, MushroomCow, OminousItemSpawner, Ocelot, Panda, Parrot, Phantom, Pig, Piglin, PufferFish,
    Rabbit, Ravager, Salmon, Sheep, Shulker, Snowman, SpectralArrow, Strider, TNTPrimed, Tadpole, TextDisplay, Trident,
    Turtle, Vex, Villager, Vindicator, WanderingTrader, WitherSkull, Wither, Wolf, Zombie, Zoglin, ZombieVillager,
    CommandMinecart, ExplosiveMinecart, HopperMinecart, PoweredMinecart, ThrownPotion, Bat, Husk, SkeletonHorse,
    CopperGolem,
], interfaces: [
    Mob, Ageable, Animals, Tameable, Sittable, AbstractHorse, ChestedHorse, AbstractArrow, AbstractCubeMob,
    Display, Explosive, PiglinAbstract, Projectile, Raider, SizedFireball, Spellcaster, Steerable,
    ThrowableProjectile,
]);

/// Bukkit's entity interface to its `EntityType`: `ArmorStand` is `ARMOR_STAND`, with the names that differ.
fn entity_type_of_class(class: &str) -> String {
    let simple = class.rsplit(['.', '$']).next().unwrap_or(class);
    let named = match simple {
        "TNTPrimed" => "TNT",
        "Firework" => "FIREWORK_ROCKET",
        "MushroomCow" => "MOOSHROOM",
        "Snowman" => "SNOW_GOLEM",
        "PigZombie" => "ZOMBIFIED_PIGLIN",
        "EnderSignal" => "EYE_OF_ENDER",
        "ThrownExpBottle" => "EXPERIENCE_BOTTLE",
        "LightningStrike" => "LIGHTNING_BOLT",
        "LeashHitch" => "LEASH_KNOT",
        "EnderCrystal" => "END_CRYSTAL",
        "FishHook" => "FISHING_BOBBER",
        "RideableMinecart" => "MINECART",
        "PufferFish" => "PUFFERFISH",
        "CommandMinecart" => "COMMAND_BLOCK_MINECART",
        "ExplosiveMinecart" => "TNT_MINECART",
        "HopperMinecart" => "HOPPER_MINECART",
        "PoweredMinecart" => "FURNACE_MINECART",
        "ThrownPotion" | "SplashPotion" => "SPLASH_POTION",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_owned();
    }
    let mut out = String::new();
    for (i, c) in simple.char_indices() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

fn player_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    // Paper's `sendActionBar(String)` in legacy `§` text, and `sendActionBar(char, String)` with its own marker.
    if name == "sendActionBar" && matches!(a, [V::Str(_) | V::Sb(_)] | [V::Char(_), V::Str(_) | V::Sb(_)]) {
        let text = match a {
            [marker, text] => translate_colors(marker.text().chars().next().unwrap_or('&'), &text.text()),
            _ => a[0].text(),
        };
        let bar = component::message_arg(&jrt::s(&text))?;
        held::<Player>(this)?.send_action_bar(&bar);
        return Some(Ok(V::Null));
    }
    // Adventure's `playSound(Sound.sound(key, source, volume, pitch))`: the game's `/playsound` at the player.
    if name == "playSound" && a.len() == 1 {
        if let Some(sound) = a[0].native::<SoundH>() {
            let who = held::<Player>(this)?.get_name().to_owned();
            horizon_bukkit::horizon_mod::run_command(&format!("playsound {} {} {who} ~ ~ ~ {} {}", sound.key, sound.source, sound.volume, sound.pitch));
            return Some(Ok(V::Null));
        }
    }
    // Paper's `sendTitlePart(TitlePart.TITLE | SUBTITLE | TIMES, value)`: one part each, as the game's `/title`.
    if name == "sendTitlePart" && a.len() == 2 {
        let who = held::<Player>(this)?.get_name().to_owned();
        let part = a[0].text().rsplit('.').next().unwrap_or("").to_ascii_uppercase();
        let command = match part.as_str() {
            "TITLE" | "SUBTITLE" => format!("title {who} {} {}", part.to_ascii_lowercase(), component::message_arg(&a[1])?.0),
            "TIMES" => {
                let times = a[1].native::<TimesH>()?;
                format!("title {who} times {} {} {}", times.0, times.1, times.2)
            }
            _ => return None,
        };
        horizon_bukkit::horizon_mod::run_command(&command);
        return Some(Ok(V::Null));
    }
    if name == "showTitle" && a.len() == 1 {
        let title = a[0].native::<TitleH>()?;
        held::<Player>(this)?.show_title(&title.title, &title.subtitle, title.times.0, title.times.1, title.times.2);
        return Some(Ok(V::Null));
    }
    None
}

fn ticks(value: &V) -> u32 {
    value.native::<JDuration>().map_or(0, |d| (d.0 / 50).max(0) as u32)
}

/// Before the SDK's own methods.
/// Java's `InetSocketAddress` and `InetAddress`, for the addresses the game hands over (`player.getAddress()`,
/// `event.getAddress()`): `player.getAddress().getAddress().getHostAddress()`.
fn address_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    use std::net::{IpAddr, SocketAddr};
    fn ip(this: &V) -> Option<IpAddr> {
        held::<IpAddr>(this).map(|ip| *ip).or_else(|| held::<std::net::Ipv4Addr>(this).map(|ip| IpAddr::V4(*ip))).or_else(|| held::<std::net::Ipv6Addr>(this).map(|ip| IpAddr::V6(*ip)))
    }
    fn ip_method(ip: IpAddr, name: &str, a: &[V]) -> Option<R> {
        Some(Ok(match (name, a.len()) {
            ("getHostAddress" | "getHostName" | "getCanonicalHostName", 0) => jrt::s(&ip.to_string()),
            ("toString", 0) => jrt::s(&format!("/{ip}")),
            ("getAddress", 0) => {
                let bytes = match ip {
                    IpAddr::V4(v4) => v4.octets().to_vec(),
                    IpAddr::V6(v6) => v6.octets().to_vec(),
                };
                jrt::arr(bytes.into_iter().map(|b| V::Int(i32::from(b as i8))).collect())
            }
            ("isLoopbackAddress", 0) => V::Bool(ip.is_loopback()),
            ("isAnyLocalAddress", 0) => V::Bool(ip.is_unspecified()),
            ("isSiteLocalAddress", 0) => V::Bool(matches!(ip, IpAddr::V4(v4) if v4.is_private())),
            ("isLinkLocalAddress", 0) => V::Bool(matches!(ip, IpAddr::V4(v4) if v4.is_link_local())),
            ("isMulticastAddress", 0) => V::Bool(ip.is_multicast()),
            ("hashCode", 0) => V::Int(ip.to_string().bytes().fold(0i32, |h, b| h.wrapping_mul(31).wrapping_add(i32::from(b)))),
            ("equals", 1) => V::Bool(crate::address_method(&a[0], "getHostAddress", &[]).and_then(Result::ok).is_some_and(|other| other.text() == ip.to_string())),
            _ => return None,
        }))
    }
    if let Some(socket) = held::<SocketAddr>(this).map(|s| *s) {
        return Some(Ok(match (name, a.len()) {
            ("getAddress", 0) => wrap(socket.ip()),
            ("getPort", 0) => V::Int(i32::from(socket.port())),
            ("getHostName" | "getHostString", 0) => jrt::s(&socket.ip().to_string()),
            ("isUnresolved", 0) => V::Bool(false),
            ("toString", 0) => jrt::s(&format!("/{socket}")),
            ("hashCode", 0) => V::Int(socket.to_string().bytes().fold(0i32, |h, b| h.wrapping_mul(31).wrapping_add(i32::from(b)))),
            ("equals", 1) => V::Bool(held::<SocketAddr>(&a[0]).is_some_and(|other| *other == socket)),
            _ => return ip_method(socket.ip(), name, a),
        }));
    }
    ip(this).and_then(|ip| ip_method(ip, name, a))
}

thread_local! {
    static IN_ADD_ITEM: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// One side of a sign (`sign.getSide(Side.FRONT)`), on the very state it came from, so what is set here is what
/// that state's `update()` writes.
pub struct SignSideH {
    state: V,
    front: bool,
}

fn sign_side_call(side: &SignSideH, name: &str, a: &[V]) -> Option<R> {
    let mut state = held::<horizon_bukkit::block_data::TileState>(&side.state)?;
    let mut sign = horizon_bukkit::blocks::Sign(state.clone());
    let changes = name.starts_with("set") || (name == "line" && a.len() == 2);
    let answer = {
        let mut face = sign.get_side(if side.front { "FRONT" } else { "BACK" });
        let legacy = |c: Option<Component>| c.map(|c| c.to_legacy()).unwrap_or_default();
        match (name, a.len()) {
            ("getLine", 1) => jrt::s(&legacy(face.line(a[0].int().max(0) as usize))),
            ("line", 1) => face.line(a[0].int().max(0) as usize).map_or(V::Null, wrap),
            ("getLines", 0) => jrt::arr((0..4).map(|i| jrt::s(&legacy(face.line(i)))).collect()),
            ("lines", 0) => jrt::list((0..4).map(|i| face.line(i).map_or(V::Null, wrap)).collect()),
            ("setLine", 2) | ("line", 2) => {
                let text = component::component_of(&a[1]).map_or_else(|| a[1].text(), |c| c.to_legacy());
                face.set_line(a[0].int().max(0) as usize, &text);
                V::Null
            }
            ("isGlowingText", 0) => V::Bool(face.is_glowing_text()),
            ("setGlowingText", 1) => {
                face.set_glowing_text(a[0].truth());
                V::Null
            }
            ("getColor", 0) => jrt::s(&face.get_color()),
            _ => return None,
        }
    };
    if changes {
        *state = sign.0;
    }
    Some(Ok(answer))
}

/// `inventory.addItem(stacks...)` / `removeItem(stacks...)` as Java has them: any number of stacks (an array, as
/// CoreProtect's `addItem(new ItemStack[]{stack})`), and back a HashMap of what didn't fit by the argument's index,
/// empty when all did (`addItem(..).size() == 0`, `.isEmpty()`). The SDK's bindings take one stack and give the
/// leftover or nothing; each stack goes through them.
fn add_items_java(this: &V, name: &str, a: &[V]) -> Option<R> {
    if !matches!(name, "addItem" | "removeItem") || a.is_empty() || IN_ADD_ITEM.get() || !matches!(this, V::Native(_)) {
        return None;
    }
    let stacks: Vec<V> = if a.len() == 1 && matches!(a[0], V::Arr(_) | V::List(_)) { a[0].items().ok()? } else { a.to_vec() };
    if !stacks.iter().all(|s| s.is_null() || held::<horizon_bukkit::ItemStack>(s).is_some()) {
        return None;
    }
    IN_ADD_ITEM.set(true);
    let mut leftovers = Vec::new();
    for (index, stack) in stacks.iter().enumerate() {
        if stack.is_null() {
            continue;
        }
        match jrt::call(this, name, std::slice::from_ref(stack)) {
            Ok(V::Null | V::Bool(true)) => {}
            Ok(V::Bool(false)) => leftovers.push((V::Int(index as i32), stack.clone())),
            Ok(left) if held::<horizon_bukkit::ItemStack>(&left).is_some_and(|l| l.get_amount() > 0 && !l.get_type().is_air()) => leftovers.push((V::Int(index as i32), left)),
            Ok(_) => {}
            Err(error) => {
                IN_ADD_ITEM.set(false);
                return Some(Err(error));
            }
        }
    }
    IN_ADD_ITEM.set(false);
    let map = jrt::new_other("HashMap", &[]).unwrap_or(V::Null);
    for (index, left) in leftovers {
        let _ = jrt::call(&map, "put", &[index, left]);
    }
    Some(Ok(map))
}

thread_local! {
    static IN_GET_TYPE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `entity.getType()` is Bukkit's EntityType, whose constants here are their names (`EntityType.TNT` is "TNT"): the
/// SDK's answer is the game's id (`minecraft:tnt`), so `getType() == EntityType.PIG` was false for every mob and
/// `getType().name()` gave the id (CoreProtect logged TNT as `#minecraft:tnt`). The name is the id's path, upper case,
/// as Bukkit's have been since 1.20.5; `getKey()` / `key()` on it give the id back.
fn entity_type_java(this: &V, name: &str, a: &[V]) -> Option<R> {
    if name != "getType" || !a.is_empty() || IN_GET_TYPE.get() || !matches!(this, V::Native(_)) {
        return None;
    }
    // A player is `EntityType.PLAYER` (CoreProtect's pickup listener asked `event.getEntityType()`).
    if this.native::<Mutex<Player>>().is_some() {
        return Some(Ok(jrt::s("PLAYER")));
    }
    // A block, its state or data, an item: no entity, and every `block.getType()` asked `instanceof Entity` of the
    // whole table first (CoreProtect asks it for each water flow).
    if !matches!(native_label(this), "game" | "Entity") || !jrt::instance_of(this, "Entity") {
        return None;
    }
    IN_GET_TYPE.set(true);
    let found = jrt::call(this, "getType", &[]);
    IN_GET_TYPE.set(false);
    Some(found.map(|kind| match &kind {
        V::Str(id) if id.starts_with("minecraft:") => jrt::s(&id["minecraft:".len()..].to_ascii_uppercase()),
        _ => kind,
    }))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    // A Material's `name()`, `toString()` and `equals`, first: plugins call them for every block (CoreProtect's
    // water and TNT logging ~200k times in 30 s), and each walked the whole dispatch.
    if let Some(found) = material_fast(this, name, a) {
        return Some(found);
    }
    if let Some(found) = add_items_java(this, name, a) {
        return Some(found);
    }
    if let Some(found) = entity_type_java(this, name, a) {
        return Some(found);
    }
    // `material.toString()` is its name in Java (`CHEST`): it was `java.lang.Object@…`, so a Material from the game
    // never matched the constants in a plugin's `Set<Material>` (CoreProtect's containers).
    if name == "toString" && a.is_empty() {
        if let Some(material) = held::<horizon_bukkit::Material>(this) {
            return Some(Ok(jrt::s(&material.name())));
        }
    }
    if this.native::<horizon_chunks::TagH>().is_some() {
        return horizon_chunks::tag_method(this, name, a);
    }
    if this.native::<Mutex<std::net::SocketAddr>>().is_some() || this.native::<Mutex<std::net::IpAddr>>().is_some() {
        if let Some(found) = address_method(this, name, a) {
            return Some(found);
        }
    }
    // `player.hideEntity(plugin, entity)` / `showEntity(plugin, entity)` / `canSee(entity)`: the SDK goes by the
    // entity's id.
    if matches!(name, "hideEntity" | "showEntity") && matches!(a.len(), 1 | 2) && this.native::<Mutex<Player>>().is_some() {
        let entity = a.last()?;
        let id = held::<horizon_bukkit::entity::Entity>(entity)
            .map(|e| e.get_entity_id() as i64)
            .or_else(|| held::<Player>(entity).map(|p| p.get_entity_id()))
            .or_else(|| jrt::call(entity, "getEntityId", &[]).ok().map(|id| id.long()))
            .filter(|id| *id > 0)?;
        let player = held::<Player>(this)?.clone();
        if name == "hideEntity" {
            player.hide_entity(&the_plugin(), id as u64);
        } else {
            player.show_entity(id as u64);
        }
        return Some(Ok(V::Null));
    }
    // Adventure's boss bars, on the SDK's (Bukkit's) bar: `player.showBossBar(bar)`, `bar.progress(0.5f)`, ...
    if matches!(name, "showBossBar" | "hideBossBar") && a.len() == 1 && a[0].native::<Mutex<horizon_bukkit::BossBar>>().is_some() {
        let player = held::<Player>(this).map(|p| p.clone());
        if let (Some(player), Some(bar)) = (player, held::<horizon_bukkit::BossBar>(&a[0])) {
            if name == "showBossBar" {
                bar.add_player(&player);
                bar.set_visible(true);
            } else {
                bar.remove_player(&player);
            }
            return Some(Ok(V::Null));
        }
    }
    if let Some(bar) = held::<horizon_bukkit::BossBar>(this) {
        let overlay_to_style = |o: &str| match o { "NOTCHED_6" => "SEGMENTED_6", "NOTCHED_10" => "SEGMENTED_10", "NOTCHED_12" => "SEGMENTED_12", "NOTCHED_20" => "SEGMENTED_20", _ => "SOLID" };
        let answer = match (name, a.len()) {
            ("name", 1) => {
                bar.set_title(&match &a[0] { V::Str(text) => text.to_string(), other => legacy_text(other) });
                Some(this.clone())
            }
            ("name", 0) => Some(wrap(Component(serde_json::json!({ "text": bar.get_title() })))),
            ("progress", 1) => {
                bar.set_progress(a[0].double().clamp(0.0, 1.0));
                Some(this.clone())
            }
            ("progress", 0) => Some(V::Double(bar.get_progress())),
            ("color", 1) => {
                bar.set_color(&a[0].text().to_ascii_uppercase());
                Some(this.clone())
            }
            ("color", 0) => Some(jrt::s(&bar.get_color())),
            ("overlay", 1) => {
                bar.set_style(overlay_to_style(&a[0].text().to_ascii_uppercase()));
                Some(this.clone())
            }
            ("overlay", 0) => Some(jrt::s(&bar.get_style().replace("SEGMENTED", "NOTCHED").replace("SOLID", "PROGRESS"))),
            _ => None,
        };
        if let Some(answer) = answer {
            return Some(Ok(answer));
        }
    }
    // Bukkit's `setResourcePack` overloads: (url), (url, hash), (url, hash, prompt|force), (url, hash, prompt,
    // force), (id, url, hash, prompt, force). The SDK takes the url, the SHA-1 as hex and whether it is required.
    if matches!(name, "setResourcePack" | "setTexturePack") && !a.is_empty() && this.native::<Mutex<Player>>().is_some() {
        let url = a.iter().find(|v| matches!(v, V::Str(_)) && v.text().contains("://")).or_else(|| a.iter().find(|v| matches!(v, V::Str(_)))).map(V::text).unwrap_or_default();
        let hash = a.iter().find_map(|v| match v {
            V::Arr(_) => jrt::javaio::bytes_of(v).map(|bytes| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()),
            V::Str(text) if text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit()) => Some(text.to_string()),
            _ => None,
        }).unwrap_or_default();
        let required = matches!(a.last(), Some(V::Bool(true)));
        held::<Player>(this)?.set_resource_pack(&url, &hash, required);
        return Some(Ok(V::Null));
    }
    // `EntityEvent.getEntityType()`, which every entity event inherits: an SDK event without its own is asked
    // for its entity's type.
    if name == "getEntityType" && a.is_empty() {
        if let Some(Ok(entity)) = jrt::try_native(this, "getEntity", &[]) {
            if !entity.is_null() {
                return Some(jrt::call(&entity, "getType", &[]));
            }
        }
    }
    if name == "createChunkData" || this.native::<generator::DataH>().is_some() {
        if let Some(found) = generator::method(this, name, a) {
            return Some(found);
        }
    }
    if matches!(this, V::Obj(_)) {
        return object_method(this, name, a);
    }
    if let Some(found) = bungee::method(this, name, a) {
        return Some(found);
    }
    // `world.getEntitiesByClass(ArmorStand.class)` (or `Classes`): the world's entities of that class.
    if matches!(name, "getEntitiesByClass" | "getEntitiesByClasses") && !a.is_empty() && a.iter().all(|c| matches!(c, V::Class(_)) || matches!(c, V::Arr(_))) && this.native::<Mutex<World>>().is_some() {
        let mut wanted: Vec<String> = Vec::new();
        for c in a {
            match c {
                V::Class(k) => wanted.push(k.rsplit('.').next().unwrap_or(k).to_owned()),
                other => wanted.extend(other.items().unwrap_or_default().iter().filter_map(|x| if let V::Class(k) = x { Some(k.to_string()) } else { None })),
            }
        }
        // The SDK's own list: the plugin may never call `getEntities`, so its binding may not be there.
        let all: Vec<V> = held::<World>(this)?.get_entities().into_iter().map(wrap).collect();
        return Some(Ok(jrt::list(all.into_iter().filter(|e| wanted.iter().any(|k| jrt::instance_of(e, k))).collect())));
    }
    // `world.setGameRule(GameRule.X, true)`: the SDK takes the value as the command writes it.
    // Paper's `world.getChunkAtAsync(x, z[, gen[, urgent]])` / `(location)` / `(block)` / `(x, z, callback)`: the
    // chunk now, as a done future (FAWE's preloader asks for the chunks of a selection this way).
    if name == "getChunkAtAsync" && !a.is_empty() && this.native::<Mutex<World>>().is_some() {
        let callback = a.last().filter(|v| matches!(v, V::Func(_) | V::Obj(_)) && a.len() == 3).cloned();
        let plain: Vec<V> = a.iter().filter(|v| !matches!(v, V::Bool(_))).take(2).cloned().collect();
        let chunk = match jrt::call(this, "getChunkAt", &plain) {
            Ok(chunk) => chunk,
            Err(thrown) => return Some(Err(thrown)),
        };
        if let Some(callback) = callback {
            return Some(jrt::call(&callback, "accept", &[chunk]).map(|_| V::Null));
        }
        return Some(jrt::call_static("CompletableFuture", "completedFuture", &[chunk]));
    }
    if name == "setGameRule" && a.len() == 2 && matches!(a[1], V::Bool(_) | V::Int(_) | V::Long(_)) && this.native::<Mutex<World>>().is_some() {
        let rule = a[0].text().trim_start_matches("minecraft:").to_lowercase();
        return Some(jrt::call(this, "setGameRule", &[jrt::s(&rule), jrt::s(&a[1].text())]));
    }
    // `registerNewObjective(name, criteria[, "display"])`: Paper's older forms, a display name as text.
    if name == "registerNewObjective" && (a.len() == 2 || (a.len() == 3 && matches!(a[2], V::Str(_)))) {
        let shown = a.get(2).map_or_else(|| a[0].text(), V::text);
        let criteria = a[1].text().to_lowercase();
        return Some(jrt::call(this, name, &[a[0].clone(), jrt::s(&criteria), wrap(Component(serde_json::json!({ "text": shown })))]));
    }
    // `Bukkit.getScoreboardManager().getNewScoreboard()`: the manager is the main board's handle here, and the
    // SDK's `get_new_scoreboard` has no receiver for the bindings to find.
    if name == "getNewScoreboard" && a.is_empty() && this.native::<Mutex<horizon_bukkit::scoreboard::Scoreboard>>().is_some() {
        return Some(Ok(wrap(horizon_bukkit::lead::get_new_scoreboard())));
    }
    // Paper's own classes, by the names plugins read to find the server's package
    // (`getServer().getClass().getName()` minus "CraftServer").
    if name == "getClass" && a.is_empty() {
        if this.native::<Mutex<horizon_bukkit::fam_core::Server>>().is_some() {
            return Some(Ok(V::Class("org.bukkit.craftbukkit.CraftServer")));
        }
        if this.native::<Mutex<Player>>().is_some() {
            return Some(Ok(V::Class("org.bukkit.craftbukkit.entity.CraftPlayer")));
        }
    }
    if this.native::<jrt::javautil::JPlain>().is_some() && matches!(name, "getResourceAsStream" | "getResource") {
        return Some(Ok(resource(&a[0].text()).map_or(V::Null, |bytes| jrt::javaio::stream(bytes.to_vec()))));
    }
    // `world.spawn(location, ArmorStand.class[, consumer])`: the class names the entity type; the consumer
    // gets the new entity, as Paper hands it over.
    if name == "spawn" && matches!(a.len(), 2 | 3) {
        if let V::Class(class) = &a[1] {
            let made = jrt::call(this, "spawn", &[a[0].clone(), jrt::s(&entity_type_of_class(class))]);
            if let (Ok(entity), Some(consumer)) = (&made, a.get(2)) {
                if !entity.is_null() && !consumer.is_null() {
                    if let Err(error) = jrt::call(consumer, "accept", std::slice::from_ref(entity)) {
                        return Some(Err(error));
                    }
                }
            }
            return Some(made);
        }
    }
    if let Some(found) = component::method(this, name, a) {
        return Some(found);
    }
    if let Some(found) = registries::method(this, name, a) {
        return Some(found);
    }
    if this.native::<HelpMapH>().is_some() {
        return help_map_method(name);
    }
    if let Some(found) = snakeyaml::method(this, name, a) {
        return Some(found);
    }
    if let Some(found) = messenger::method(this, name, a) {
        return Some(found);
    }
    // Adventure's BossBar setters on the SDK's bar: `name(...)`, `progress(...)`; its look (colour,
    // overlay, flags) stays as the bar was made.
    if a.len() == 1 && matches!(name, "name" | "progress" | "color" | "overlay" | "addFlag" | "removeFlag" | "flags") {
        if let Some(bar) = held::<horizon_bukkit::BossBar>(this) {
            match name {
                "name" => bar.set_title(&legacy_text(&a[0])),
                "progress" => bar.set_progress(a[0].double()),
                _ => {}
            }
            return Some(Ok(this.clone()));
        }
    }
    if let Some(found) = lifecycle::method(this, name, a) {
        return Some(found);
    }
    if let Some(found) = dialogs::method(this, name, a) {
        return Some(found);
    }
    // `Material.STONE` reads as its name: a Material's own question goes to the Material. Only names
    // java.lang.String doesn't have, so a real string keeps its own methods.
    if let V::Str(text) = this {
        // `DyeColor.RED.getColor()` / `getFireworkColor()`: a dye is its name here; its RGB as Bukkit's Color (a sign
        // side's `getColor().getColor().asRGB()` in CoreProtect).
        if a.is_empty() && matches!(name, "getColor" | "getFireworkColor") {
            let color = if name == "getColor" { horizon_bukkit::misc::dye_color(text) } else { horizon_bukkit::misc::dye_firework_color(text) };
            if let Some(color) = color {
                return Some(Ok(wrap(color)));
            }
        }
        // `Material.GLASS.createBlockData()`: the block's default state.
        if name == "createBlockData" && a.is_empty() && !text.is_empty() && text.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
            return Some(Ok(wrap(horizon_bukkit::block_data::BlockData::parse(&format!("minecraft:{}", text.to_ascii_lowercase())))));
        }
        if MATERIAL_ONLY.contains(&name) {
            return Some(jrt::call(&wrap(horizon_bukkit::Material::of(text)), name, a));
        }
        // `Particle.DUST.getDataType()`: the class of the data the particle takes, as Bukkit's (Void for none).
        if name == "getDataType" && a.is_empty() && entity_type_like(text) {
            return Some(Ok(V::Class(match &**text {
                "DUST" | "REDSTONE" => "Particle.DustOptions",
                "DUST_COLOR_TRANSITION" => "Particle.DustTransition",
                "ITEM" | "ITEM_CRACK" => "ItemStack",
                "BLOCK" | "BLOCK_CRACK" | "BLOCK_DUST" | "FALLING_DUST" | "BLOCK_MARKER" | "DUST_PILLAR" | "BLOCK_CRUMBLE" => "BlockData",
                "VIBRATION" => "Vibration",
                "SCULK_CHARGE" => "Float",
                "SHRIEK" => "Integer",
                "ENTITY_EFFECT" | "TINTED_LEAVES" => "Color",
                "TRAIL" => "Particle.Trail",
                "DRAGON_BREATH" | "FLASH" => "Float",
                _ => "Void",
            })));
        }
        // `EntityType.ZOMBIE.isAlive()`: an entity type is its name here; its own questions, by the game's id.
        let entity_type = || text.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') && !text.is_empty();
        if a.is_empty() && matches!(name, "isAlive" | "isSpawnable" | "getTypeId" | "hasDefaultAttributes" | "getEntityClass" | "getTranslationKey" | "isEnabledByFeature") && entity_type() {
            let id = format!("minecraft:{}", text.to_ascii_lowercase());
            return Some(Ok(match name {
                "isAlive" => V::Bool(horizon_bukkit::entities3::entity_type_is_alive(&id)),
                "isSpawnable" => V::Bool(horizon_bukkit::fam_entity::entity_type_is_spawnable(text)),
                "getTypeId" => V::Int(horizon_bukkit::fam_entity::entity_type_get_type_id(text)),
                "hasDefaultAttributes" => V::Bool(horizon_bukkit::fam_entity::entity_type_has_default_attributes(text)),
                "getEntityClass" => horizon_bukkit::fam_entity::entity_type_get_entity_class(text).map_or(V::Null, V::Class),
                "getTranslationKey" => jrt::s(&format!("entity.minecraft.{}", text.to_ascii_lowercase())),
                _ => V::Bool(true),
            }));
        }
        // A registry value held as its id or name (`PotionEffectType`, `Enchantment`...): `getName()`, which
        // String hasn't got, is its enum-style name, `minecraft:regeneration` -> "REGENERATION".
        if name == "getName" && a.is_empty() && !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.')) {
            return Some(Ok(jrt::s(&text.rsplit(':').next().unwrap_or(text).to_ascii_uppercase())));
        }
    }
    // Paper's `Bukkit.getCommandMap()` / `server.getCommandMap()`, and Spigot's way to it: the server class's private
    // `commandMap` field by reflection (`getServer().getClass().getDeclaredField("commandMap")`, setAccessible, `get(server)`),
    // as Tebex registers its /buy. Either way it's the command map whose `register` follows.
    if name == "getCommandMap" && a.is_empty() {
        return Some(Ok(jrt::native(CommandMapH)));
    }
    if matches!(name, "getDeclaredField" | "getField") && a.len() == 1 && a[0].text() == "commandMap" {
        return Some(Ok(jrt::native(CommandMapFieldH)));
    }
    if this.native::<CommandMapFieldH>().is_some() {
        return Some(Ok(match name {
            "get" => jrt::native(CommandMapH),
            "setAccessible" => V::Null,
            "trySetAccessible" | "canAccess" | "isAccessible" => V::Bool(true),
            "getName" => jrt::s("commandMap"),
            "getType" => V::Class("CommandMap"),
            _ => return None,
        }));
    }
    if this.native::<CommandMapH>().is_some() {
        match (name, a.len()) {
            ("getCommand", 1) => return Some(Ok(jrt::native(CommandH(a[0].text())))),
            ("getKnownCommands", 0) => return Some(Ok(V::Map(Default::default()))),
            ("clearCommands", 0) | ("setFallbackCommands", 0) => return Some(Ok(V::Null)),
            _ => {}
        }
    }
    // `getCommandMap().register(prefix, new MyCommand())` / `registerAll(prefix, list)`: the plugin's own
    // Command subclasses. Each becomes a command of this plugin whose `execute` and `tabComplete` answer.
    if matches!(name, "register" | "registerAll") && a.len() >= 2 {
        let commands = if name == "registerAll" { a[1].items().unwrap_or_default() } else { vec![a[a.len() - 1].clone()] };
        if !commands.is_empty() && commands.iter().all(|c| matches!(c, V::Obj(_))) {
            return Some(register_commands(&a[0].text(), commands).map(|ok| if name == "register" { ok } else { V::Null }));
        }
    }
    // ItemMeta is the stack's own components in the SDK: `getItemMeta()` is a detached copy, as in Java,
    // its setters work on that copy, and `setItemMeta(meta)` puts it back (keeping the stack's amount).
    if let Some(stack) = held::<horizon_bukkit::ItemStack>(this) {
        match (name, a.len()) {
            ("getItemMeta", 0) => return Some(Ok(wrap(stack.clone()))),
            // Paper's `stack.editMeta(meta -> ...)` (or `editMeta(SkullMeta.class, meta -> ...)`): the consumer
            // works on a copy that goes back on the stack, its amount kept. True, as a stack always has meta here.
            ("editMeta", 1 | 2) => {
                let (copy, amount) = (wrap(stack.clone()), stack.get_amount());
                drop(stack);
                if let Err(thrown) = jrt::call(&a[a.len() - 1], "accept", std::slice::from_ref(&copy)) {
                    return Some(Err(thrown));
                }
                let edited = held::<horizon_bukkit::ItemStack>(&copy)?.clone();
                let mut stack = held::<horizon_bukkit::ItemStack>(this)?;
                *stack = edited;
                stack.set_amount(amount);
                return Some(Ok(V::Bool(true)));
            }
            ("getPersistentDataContainer", 0) => return Some(Ok(jrt::native(ItemDataH(this.clone())))),
            // `meta.addEnchant(Enchantment.DURABILITY, 1, true)`: by Bukkit's old names too; the third
            // argument (ignore the level limit) is how the SDK always adds them.
            ("addEnchant", 3) | ("addEnchantment" | "addUnsafeEnchantment", 2) if matches!(a[0], V::Str(_)) => {
                let wanted = a[0].text();
                let key = registries::enchantment_key(&wanted);
                let mut stack = stack;
                stack.add_enchant(&key, a[1].long().max(0) as u32);
                return Some(Ok(if name == "addEnchant" { V::Bool(true) } else { V::Null }));
            }
            // Paper's Adventure setters on the meta.
            ("displayName", 1) => {
                drop(stack);
                let text = component::message_arg(&a[0]);
                if let (Some(name), Some(mut stack)) = (text, held::<horizon_bukkit::ItemStack>(this)) {
                    stack.set_display_name_component(&name);
                } else if a[0].is_null() {
                    held::<horizon_bukkit::ItemStack>(this)?.remove_component("minecraft:custom_name");
                }
                return Some(Ok(V::Null));
            }
            // `meta.lore(List<Component>)`: Adventure lines; null or an empty list clears the lore.
            ("lore", 1) if !matches!(a[0], V::Str(_)) => {
                drop(stack);
                let lines: Vec<_> = if a[0].is_null() { Vec::new() } else { a[0].items().ok()?.iter().filter_map(component::message_arg).collect() };
                held::<horizon_bukkit::ItemStack>(this)?.set_lore_components(&lines);
                return Some(Ok(V::Null));
            }
            ("hasItemMeta", 0) => return Some(Ok(V::Bool(true))),
            ("setItemMeta", 1) => {
                // The meta's components onto this stack: its amount AND its material stay (a meta made for another
                // item, as a saved `==: ItemMeta` is, never changes what the stack is - CraftItemStack.setItemMeta).
                let (amount, material) = (stack.get_amount(), stack.get_type().clone());
                drop(stack);
                if let Some(meta) = held::<horizon_bukkit::ItemStack>(&a[0]).map(|m| m.clone()) {
                    let mut stack = held::<horizon_bukkit::ItemStack>(this)?;
                    *stack = meta;
                    stack.set_amount(amount);
                    if *stack.get_type() != material {
                        stack.set_type(material);
                    }
                    return Some(Ok(V::Bool(true)));
                }
                return Some(Ok(V::Bool(a[0].is_null())));
            }
            _ => {}
        }
    }
    // The plugin's own event: `getPluginManager().callEvent(event)`, or Paper's `event.callEvent()` (true
    // unless a handler cancelled it).
    if name == "callEvent" && a.len() == 1 && matches!(a[0], V::Obj(_)) {
        return Some(call_own_event(&a[0]));
    }
    if name == "callEvent" && a.is_empty() && matches!(this, V::Obj(_)) {
        if let Err(thrown) = call_own_event(this) {
            return Some(Err(thrown));
        }
        return Some(Ok(V::Bool(!jrt::call(this, "isCancelled", &[]).is_ok_and(|c| c.truth()))));
    }
    // `profile.update()`: Paper's CompletableFuture, finished on the server thread when the lookup answers.
    if name == "update" && a.is_empty() {
        if let Some(profile) = held::<horizon_bukkit::player::PlayerProfile>(this).map(|p| p.clone()) {
            let (value, future) = jrt::javautil::pending_future();
            profile.update(move |updated| {
                future.complete_with(Ok(wrap(updated)));
            });
            return Some(Ok(value));
        }
    }
    // An entity's or player's container with Java's arity: `has(key, type)`, `get(key, type)`,
    // `getOrDefault(key, type, default)`, `set(key, type, value)`. The SDK's own take no type.
    if matches!((name, a.len()), ("has" | "get", 2) | ("getOrDefault" | "set", 3)) {
        if let (Some(pdc), Some(key)) = (held::<horizon_bukkit::persistence::PersistentDataContainer>(this), held::<horizon_bukkit::persistence::NamespacedKey>(&a[0]).map(|k| k.clone())) {
            let kind = a[1].text().rsplit('.').next().unwrap_or("").to_ascii_uppercase();
            let read = |value: Value| match (kind.as_str(), &value) {
                ("BOOLEAN", other) => V::Bool(other.as_bool().unwrap_or_else(|| other.as_i64().unwrap_or(0) != 0)),
                ("INTEGER" | "SHORT" | "BYTE", other) if other.is_number() => V::Int(other.as_i64().unwrap_or(0) as i32),
                ("LONG", other) if other.is_number() => V::Long(other.as_i64().unwrap_or(0)),
                ("DOUBLE" | "FLOAT", other) if other.is_number() => V::Double(other.as_f64().unwrap_or(0.0)),
                _ => jrt::javaio::from_json(&value),
            };
            return Some(Ok(match name {
                "has" => V::Bool(pdc.has(&key)),
                "get" => pdc.get(&key).map_or(V::Null, read),
                "getOrDefault" => pdc.get(&key).map_or_else(|| a[2].clone(), read),
                _ => {
                    pdc.set(&key, jrt::javaio::to_json(&a[2]));
                    V::Null
                }
            }));
        }
    }
    if let Some(data) = this.native::<ItemDataH>() {
        return item_data(&data.0, name, a);
    }
    // `for (ItemStack item : inventory)`: an Inventory is Iterable over its slots (`getContents()`, nulls too).
    if matches!(name, "iterator" | "spliterator") && a.is_empty() {
        let slots = |stacks: Vec<Option<horizon_bukkit::ItemStack>>| jrt::list(stacks.into_iter().map(|s| s.map_or(V::Null, wrap)).collect());
        let contents = if let Some(inventory) = held::<horizon_bukkit::Inventory>(this) {
            Some(slots(inventory.get_contents().to_vec()))
        } else {
            held::<horizon_bukkit::PlayerInventory>(this).map(|inventory| slots(inventory.get_contents()))
        };
        if let Some(contents) = contents {
            return Some(jrt::call(&contents, name, &[]));
        }
    }
    // `sender.getServer()`, `entity.getServer()`, `world.getServer()`: there is one server.
    if name == "getServer" && a.is_empty() && matches!(this, V::Native(_)) {
        return Some(Ok(wrap(the_plugin().get_server())));
    }
    // `getServer().getPluginCommand(name)` / `Bukkit.getPluginCommand(name)`: the same as `getCommand(name)`.
    if name == "getPluginCommand" && a.len() == 1 {
        return Some(Ok(jrt::native(CommandH(a[0].text()))));
    }
    if name == "getHolder" && matches!(a.len(), 0 | 1) {
        if let Some(inventory) = held::<horizon_bukkit::Inventory>(this) {
            let found = jrt::lock(&HOLDERS).iter().find(|(made, _)| made.same_as(&inventory)).map(|(_, holder)| holder.clone());
            // A GUI made without a holder (or one this bridge didn't make): no holder, as Java's `getHolder()` gives
            // null, so `getHolder() instanceof MyHolder` is false instead of the listener throwing (DGCommandItems).
            return Some(Ok(found.unwrap_or(V::Null)));
        }
        // An event's menu (`InventoryClickEvent.getInventory()`, a hopper's `getSource()`): a plugin's GUI is its holder
        // as above; a block's menu is the block's state (a Chest, a Hopper...), which `instanceof BlockInventoryHolder`
        // and `Hopper` tell apart (tile_state_is). It was null for every menu, so CoreProtect logged no chest use and
        // no hopper. `getHolder(useSnapshot)` is the same.
        if let Some(menu) = held::<horizon_bukkit::fam_ev_player::MenuInventory>(this).map(|m| m.clone()) {
            if let Some(gui) = menu.gui() {
                let found = jrt::lock(&HOLDERS).iter().find(|(made, _)| made.same_as(gui)).map(|(_, holder)| holder.clone());
                return Some(Ok(found.unwrap_or(V::Null)));
            }
            return Some(Ok(menu.get_location().map_or(V::Null, |location| wrap(location.get_block().tile()))));
        }
    }
    // `event.getView().getInventory(rawSlot)`: the menu above when the slot is one of its own, else the player's
    // inventory below (Bukkit's raw numbering has the top's slots first); `getTopInventory`, `getBottomInventory`.
    if matches!(name, "getInventory" | "getTopInventory" | "getBottomInventory") {
        if let Some(view) = held::<horizon_bukkit::fam_ev_player::MenuView>(this).map(|v| v.clone()) {
            let top = view.get_top_inventory().clone();
            let bottom = || view.get_player().map_or(V::Null, |player| wrap(player.get_inventory()));
            return match (name, a.first().map(V::int)) {
                ("getInventory", Some(raw)) if raw < 0 => Some(Ok(V::Null)),
                ("getInventory", Some(raw)) if (raw as usize) < top.get_size() => Some(Ok(wrap(top))),
                ("getInventory", Some(_)) | ("getBottomInventory", None) => Some(Ok(bottom())),
                ("getTopInventory", None) => Some(Ok(wrap(top))),
                _ => None,
            };
        }
    }
    // `sign.getSide(Side.FRONT|BACK)` on what `block.getState()` gave: one side of that sign, sharing its state so a
    // `setLine` there is what `sign.update()` writes (CoreProtect logs and restores sign text this way).
    if name == "getSide" && a.len() == 1 && held::<horizon_bukkit::block_data::TileState>(this).is_some() {
        return Some(Ok(jrt::native(SignSideH { state: this.clone(), front: !a[0].text().ends_with("BACK") })));
    }
    if let Some(side) = this.native::<SignSideH>() {
        if let Some(found) = sign_side_call(side, name, a) {
            return Some(found);
        }
    }
    // Paper's region, entity, global and async schedulers (Folia's API, on Paper too): `runNow`, `run`,
    // `execute`, `runDelayed`, `runAtFixedRate`. All on the main thread here, which is safe for each; delays in
    // ticks, or in the TimeUnit given. The task gets a handle whose `cancel()` stops a repeating one.
    if matches!(name, "runNow" | "runDelayed" | "runAtFixedRate" | "run" | "execute") && a.len() >= 2 && (a[0].native::<Mutex<Plugin>>().is_some() || (matches!(a[0], V::Obj(_)) && jrt::instance_of(&a[0], "JavaPlugin"))) {
        if let Some(task) = a.iter().skip(1).find(|v| matches!(v, V::Func(_) | V::Obj(_))).cloned() {
            let numbers: Vec<u64> = a.iter().skip(1).filter(|v| matches!(v, V::Int(_) | V::Long(_))).map(|v| v.long().max(0) as u64).collect();
            let unit = a.iter().skip(1).find(|v| matches!(v, V::Str(_))).map(V::text);
            let ticks = |n: u64| match unit.as_deref() {
                Some("MILLISECONDS") => (n / 50).max(1),
                Some("SECONDS") => n * 20,
                Some("MINUTES") => n * 1200,
                Some("HOURS") => n * 72000,
                Some("MICROSECONDS" | "NANOSECONDS") => 1,
                _ => n,
            };
            let id = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
            let (runs, seen) = (task.clone(), id.clone());
            let how = if name == "execute" { "run" } else { "accept" };
            let job = move || {
                let handle = jrt::native(TaskH(seen.load(std::sync::atomic::Ordering::Relaxed)));
                let args = if how == "run" { Vec::new() } else { vec![handle] };
                let _ = jrt::report("task", jrt::call(&runs, how, &args));
            };
            let made = match name {
                "runDelayed" => scheduler::run_task_later(ticks(numbers.last().copied().unwrap_or(0)).max(1), job),
                "runAtFixedRate" => {
                    let period = ticks(numbers.last().copied().unwrap_or(1)).max(1);
                    let first = ticks(numbers.iter().rev().nth(1).copied().unwrap_or(0)).max(1);
                    scheduler::run_task_timer(first, period, job)
                }
                "execute" if numbers.len() == 1 => scheduler::run_task_later(numbers[0].max(1), job),
                _ => scheduler::run_task(job),
            };
            id.store(made, std::sync::atomic::Ordering::Relaxed);
            return Some(Ok(jrt::native(TaskH(made))));
        }
    }
    // `getPluginManager().disablePlugin(this)`: the SDK goes by name, Java hands over the plugin.
    if matches!(name, "disablePlugin" | "isPluginEnabled") && a.len() == 1 && !matches!(a[0], V::Str(_) | V::Null) {
        let plugin_name = match held::<Plugin>(&a[0]) {
            Some(plugin) => plugin.get_name().to_string(),
            None if matches!(a[0], V::Obj(_)) => the_plugin().get_name().to_string(),
            None => return None,
        };
        let manager = horizon_bukkit::fam_core::get_plugin_manager();
        return Some(Ok(V::Bool(if name == "disablePlugin" { manager.disable_plugin(&plugin_name) } else { manager.is_plugin_enabled(&plugin_name) })).map(|done| if name == "disablePlugin" { V::Null } else { done }));
    }
    // `sendMessage(String[])`: Bukkit sends each line in turn.
    if name == "sendMessage" && a.len() == 1 && matches!(a[0], V::Arr(_)) {
        for line in a[0].items().unwrap_or_default() {
            if let Err(error) = jrt::call(this, "sendMessage", &[line]) {
                return Some(Err(error));
            }
        }
        return Some(Ok(V::Null));
    }
    if name == "registerEvent" && a.len() >= 4 {
        if let Some(done) = register_dynamic(a) {
            return Some(done);
        }
        // An event class the converted plugin has no binding for (one found by name at run time): Horizon
        // never fires it, so the listener is registered and never called, as on a server without that event.
        if matches!(a[0], V::Class(_)) {
            return Some(Ok(V::Null));
        }
    }
    if name == "registerEvents" {
        let state = state();
        let state = state.as_ref()?;
        (state.listeners)(&a[0], &state.plugin);
        return Some(Ok(V::Null));
    }
    if name == "getScheduler" {
        return Some(Ok(jrt::native(SchedulerH)));
    }
    if let Some(found) = metadata_method(this, name, a) {
        return Some(found);
    }
    if let Some(meta) = this.native::<FixedMetaH>() {
        return fixed_meta_method(meta, name, a);
    }
    if let Some(found) = player_by_id(name, a) {
        return Some(found);
    }
    if this.native::<Mutex<Plugin>>().is_some() {
        let plugin = the_plugin();
        if let Some(found) = plugin_method(this, &plugin, name, a) {
            return Some(found);
        }
    }
    if this.native::<Mutex<YamlConfiguration>>().is_some() {
        if yaml_file_path(this).is_some() || name == "setConfigurationFile" {
            if let Some(found) = yaml_file_method(this, name, a) {
                return Some(found);
            }
        }
        if let Some(found) = config_method(this, name, a) {
            return Some(found);
        }
    }
    if let Some(sec) = this.native::<SectionH>() {
        return section_method(sec, this, name, a);
    }
    if this.native::<Mutex<Player>>().is_some() {
        if let Some(found) = player_method(this, name, a) {
            return Some(found);
        }
    }
    if let Some(handle) = this.native::<CommandH>() {
        return command_method(handle, name, a);
    }
    if this.native::<LoggerH>().is_some() {
        return logger_method(name, a);
    }
    if let Some(found) = xplugin::remote_method(this, name, a) {
        return Some(found);
    }
    if let Some(found) = protocollib::method(this, name, a) {
        return Some(found);
    }
    if let Some(found) = packetevents::method(this, name, a) {
        return Some(found);
    }
    if let Some(found) = xplugin::method(this, name, a) {
        return Some(found);
    }
    if this.native::<AudiencesH>().is_some() {
        return audiences_method(this, name, a);
    }
    if let Some(group) = this.native::<AudienceGroupH>() {
        // An audience of many: each member hears it (`audiences.all().sendMessage(component)`).
        if matches!(name, "forEachAudience" | "audiences") {
            return Some(Ok(jrt::list(group.0.clone())));
        }
        for member in &group.0 {
            if let Err(thrown) = jrt::call(member, name, a) {
                return Some(Err(thrown));
            }
        }
        return Some(Ok(V::Null));
    }
    if let Some(d) = this.native::<DescH>() {
        return desc_method(d, name, a);
    }
    if let Some(c) = this.native::<CmdBaseH>() {
        return cmd_base_method(c, name, a);
    }
    if let Some(options) = this.native::<OptionsH>() {
        return options_method(options, this, name, a);
    }
    if let Some(uni) = this.native::<UniSchedH>() {
        return uni_scheduler_method(uni, name, a);
    }
    if this.native::<MiniH>().is_some() {
        return match name {
            "deserialize" if a.len() > 1 => {
                let mut resolvers = Vec::new();
                resolvers_of(&a[1..], &mut resolvers);
                Some(Ok(wrap(mini_message(&mini_with(&a[0].text(), &resolvers)))))
            }
            "deserialize" => Some(Ok(wrap(mini_message(&a[0].text())))),
            // A component back to MiniMessage text: colours, decorations and children as tags.
            "serialize" => {
                let json = held::<Component>(&a[0]).map(|c| c.0.clone()).unwrap_or_else(|| serde_json::json!({ "text": a[0].text() }));
                Some(Ok(jrt::s(&mini_serialize(&json))))
            }
            "stripTags" => Some(Ok(jrt::s(&strip_mini_tags(&a[0].text())))),
            "escapeTags" => Some(Ok(jrt::s(&a[0].text().replace('<', r"\<")))),
            _ => None,
        };
    }
    if this.native::<SchedulerH>().is_some() {
        if matches!(name, "cancelTask") {
            scheduler::cancel_task(a[0].int() as u32);
            return Some(Ok(V::Null));
        }
        // `cancelTasks(plugin)`: the SDK stops a plugin's tasks as it disables, which is when plugins ask.
        match name {
            "cancelTasks" => return Some(Ok(V::Null)),
            "isCurrentlyRunning" | "isQueued" => return Some(Ok(V::Bool(false))),
            "getPendingTasks" | "getActiveWorkers" => return Some(Ok(jrt::list(Vec::new()))),
            _ => {}
        }
        return schedule(name, a.get(1)?.clone(), a.get(2..).unwrap_or_default());
    }
    if let Some(task) = this.native::<TaskH>() {
        return Some(Ok(match name {
            "cancel" => {
                scheduler::cancel_task(task.0);
                V::Null
            }
            "getTaskId" => V::Int(task.0 as i32),
            "isCancelled" => V::Bool(false),
            _ => return None,
        }));
    }
    None
}

const CHAT_COLORS: &[(&str, char)] = &[("BLACK", '0'), ("DARK_BLUE", '1'), ("DARK_GREEN", '2'), ("DARK_AQUA", '3'), ("DARK_RED", '4'), ("DARK_PURPLE", '5'), ("GOLD", '6'), ("GRAY", '7'), ("DARK_GRAY", '8'), ("BLUE", '9'), ("GREEN", 'a'), ("AQUA", 'b'), ("RED", 'c'), ("LIGHT_PURPLE", 'd'), ("YELLOW", 'e'), ("WHITE", 'f'), ("MAGIC", 'k'), ("BOLD", 'l'), ("STRIKETHROUGH", 'm'), ("UNDERLINE", 'n'), ("ITALIC", 'o'), ("RESET", 'r')];

/// Constants that are text in Java's hands.
pub fn field(class: &str, name: &str) -> Option<V> {
    if let Some(found) = packetevents::field(class, name) {
        return Some(found);
    }
    if let Some(found) = protocollib::field(class, name) {
        return Some(found);
    }
    if let Some(found) = registries::field(class, name) {
        return Some(found);
    }
    // `Tag.ITEMS_SPEARS`, `Tag.LOGS`: Bukkit's names for the game's tags, by its own rule (ITEMS_ item tags,
    // ENTITY_TYPES_ entity tags, FLUIDS_ fluid tags, the rest block tags). The name was left as text, so every
    // `isTagged` on it threw (NoSpears: one throw a spawn).
    if class == "Tag" && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
        let (kind, rest) = [("ITEMS_", "item"), ("ENTITY_TYPES_", "entity_type"), ("FLUIDS_", "fluid")]
            .iter()
            .find_map(|(prefix, kind)| name.strip_prefix(prefix).map(|rest| (*kind, rest)))
            .unwrap_or(("block", name));
        return Some(wrap(horizon_bukkit::registry::Tag::get(kind, &rest.to_ascii_lowercase())));
    }
    if matches!(class, "NamedTextColor" | "TextDecoration") {
        return Some(jrt::s(&name.to_lowercase()));
    }
    if class == "ChatColor" {
        if name == "COLOR_CHAR" {
            return Some(V::Char('\u{a7}'));
        }
        return CHAT_COLORS.iter().find(|(color, _)| *color == name).map(|(_, code)| jrt::s(&format!("\u{a7}{code}")));
    }
    None
}

fn translate_colors(marker: char, text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < chars.len() {
        if chars[at] == marker && chars.get(at + 1).is_some_and(|c| "0123456789AaBbCcDdEeFfKkLlMmNnOoRrXx".contains(*c)) {
            out.push('\u{a7}');
            out.extend(chars[at + 1].to_lowercase());
            at += 2;
        } else {
            out.push(chars[at]);
            at += 1;
        }
    }
    out
}

fn strip_colors(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{a7}' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// `getPlayer(UUID)`: a UUID is its text here, and the SDK's `get_player` goes by name.
fn player_by_id(name: &str, a: &[V]) -> Option<R> {
    // The name and kind first: `id.text()` of any other one-argument call (`builder.add(component)`) ran the
    // argument's `toString` on every call: most of FAWE's 50 s start was rendering components for nothing.
    let [id] = a else { return None };
    if name != "getPlayer" || !matches!(id, V::Str(_)) {
        return None;
    }
    let text = id.text();
    if text.len() != 36 || text.matches('-').count() != 4 {
        return None;
    }
    Some(Ok(horizon_bukkit::get_online_players().into_iter().find(|player| player.get_unique_id() == text).map_or(V::Null, wrap)))
}

/// `BukkitAudiences`, and its builder.
pub struct AudiencesH;
/// An audience of several: the players, and maybe the console.
pub struct AudienceGroupH(Vec<V>);

fn audiences_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    let players = || jrt::call_static("Bukkit", "getOnlinePlayers", &[]).ok().and_then(|p| p.items().ok()).unwrap_or_default();
    let console = || jrt::call_static("Bukkit", "getConsoleSender", &[]).unwrap_or(V::Null);
    Some(Ok(match (name, a.len()) {
        ("player" | "sender", 1) => match &a[0] {
            // `player(uuid)`: the player online with that UUID, or no one.
            V::Str(_) => jrt::call_static("Bukkit", "getPlayer", a).unwrap_or(V::Null),
            other => other.clone(),
        },
        ("console", 0) => console(),
        ("players", 0) => jrt::native(AudienceGroupH(players())),
        ("all", 0) => jrt::native(AudienceGroupH(players().into_iter().chain(std::iter::once(console())).collect())),
        ("permission", 1) => {
            let node = a[0].text();
            jrt::native(AudienceGroupH(players().into_iter().filter(|p| jrt::call(p, "hasPermission", &[jrt::s(&node)]).is_ok_and(|ok| ok.truth())).collect()))
        }
        ("world", 1) => jrt::native(AudienceGroupH(players())),
        ("server", 1) => jrt::native(AudienceGroupH(players())),
        ("flattener" | "close" | "componentFlattener" | "partition" | "componentRenderer", _) => if name == "close" { V::Null } else { this.clone() },
        ("build", 0) => this.clone(),
        _ => return None,
    }))
}

pub fn static_method(class: &str, name: &str, a: &[V]) -> Option<R> {
    // `ConfigurationSerialization.deserializeObject(map[, class])` / `deserialize(map)` of what `ItemStack.serialize()`
    // or an ItemMeta's `serialize()` (an ItemMeta here is a copy of its stack) wrote: the stack again, which
    // `setItemMeta` takes as the meta (CoreProtect's rollback gives back named and enchanted items this way).
    if matches!(class, "ConfigurationSerialization") && matches!(name, "deserializeObject" | "deserialize") && !a.is_empty() {
        let saved = jrt::javaio::to_json(&a[0]);
        if let Some(fields) = saved.as_object().filter(|m| m.contains_key("id") || m.contains_key("type")) {
            let fields: std::collections::BTreeMap<String, serde_json::Value> = fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            return Some(Ok(horizon_bukkit::ItemStack::deserialize(&fields).map_or(V::Null, wrap)));
        }
    }
    // Paper's ClientOption constants as the SDK's, by name (the Java layer's ClientOptions passes Paper's own,
    // a class of its own, by its name): `getClientOption` of a configuration-phase connection takes the SDK's.
    if class == "ClientOption" && name == "$byName" && a.len() == 1 {
        use horizon_bukkit::fam_paper::ClientOption as O;
        let found = match a[0].text().as_str() {
            "SKIN_PARTS" => O::SKIN_PARTS,
            "CHAT_COLORS_ENABLED" => O::CHAT_COLORS_ENABLED,
            "CHAT_VISIBILITY" => O::CHAT_VISIBILITY,
            "LOCALE" => O::LOCALE,
            "MAIN_HAND" => O::MAIN_HAND,
            "VIEW_DISTANCE" => O::VIEW_DISTANCE,
            "TEXT_FILTERING_ENABLED" => O::TEXT_FILTERING_ENABLED,
            "ALLOW_SERVER_LISTINGS" => O::ALLOW_SERVER_LISTINGS,
            "PARTICLE_VISIBILITY" => O::PARTICLE_VISIBILITY,
            _ => return Some(Ok(V::Null)),
        };
        return Some(Ok(wrap(found)));
    }
    if let Some(found) = horizon_chunks::static_method(class, name, a) {
        return Some(found);
    }
    if let Some(found) = snakeyaml::static_call(class, name) {
        return Some(found);
    }
    if let Some(found) = packetevents::static_method(class, name, a) {
        return Some(found);
    }
    if let Some(found) = protocollib::static_method(class, name, a) {
        return Some(found);
    }
    // adventure-platform-bukkit's `BukkitAudiences.create(plugin)` (or `builder(plugin).build()`): on Paper
    // a player, the console and the server are audiences already, so its audiences are them.
    if class == "BukkitAudiences" && matches!(name, "create" | "builder") {
        return Some(Ok(jrt::native(AudiencesH)));
    }
    if let Some(found) = component::static_method(class, name, a) {
        return Some(found);
    }
    if let Some(found) = bungee::static_method(class, name, a) {
        return Some(found);
    }
    if let Some(found) = dialogs::static_method(class, name, a) {
        return Some(found);
    }
    if let Some(found) = lifecycle::static_method(class, name, a) {
        return Some(found);
    }
    // Adventure's `Sound.sound(key, source, volume, pitch)`: kept for `playSound`. Bukkit's Sound enum has no
    // `sound` method, so the name can't be mistaken for it.
    if class == "Sound" && name == "sound" && a.len() == 4 {
        let key = match held::<horizon_bukkit::persistence::NamespacedKey>(&a[0]) {
            Some(key) => format!("{}:{}", key.get_namespace(), key.get_key()),
            None => a[0].text(),
        };
        let source = a[1].text().rsplit('.').next().unwrap_or("master").to_ascii_lowercase();
        return Some(Ok(jrt::native(SoundH { key, source, volume: a[2].double() as f32, pitch: a[3].double() as f32 })));
    }
    // `JavaPlugin.getPlugin(Main.class)` / `getProvidingPlugin`: this plugin's main object. Bukkit's too.
    if matches!(name, "getPlugin" | "getProvidingPlugin") && a.len() == 1 && matches!(class, "JavaPlugin" | "Plugin") {
        return Some(Ok(main_object()));
    }
    if matches!(class, "Bukkit" | "Server") && name == "getPluginCommand" && a.len() == 1 {
        return Some(Ok(jrt::native(CommandH(a[0].text()))));
    }
    // `Bukkit.getEntity(uuid)`: a player or an entity of any world, as Paper looks it up.
    if matches!(class, "Bukkit" | "Server") && name == "getEntity" && a.len() == 1 {
        let uuid = a[0].text();
        if let Some(player) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_unique_id().eq_ignore_ascii_case(&uuid)) {
            return Some(Ok(wrap(player)));
        }
        let found = horizon_bukkit::get_worlds().iter().find_map(|world| world.get_entity(&uuid));
        return Some(Ok(found.map_or(V::Null, wrap)));
    }
    // `createBossBar(title, color, style, flags...)`: the SDK's bar takes the title and the colour.
    if matches!(class, "Bukkit" | "Server") && name == "createBossBar" && a.len() >= 2 && !a[0].native::<Mutex<horizon_bukkit::persistence::NamespacedKey>>().is_some() {
        let title = match &a[0] {
            V::Str(text) => text.to_string(),
            other => legacy_text(other),
        };
        return Some(Ok(wrap(horizon_bukkit::BossBar::create(&title, &a[1].text()))));
    }
    // Adventure's `BossBar.bossBar(name, progress, color, overlay)`: the SDK's bar, its colour by name.
    if class == "BossBar" && name == "bossBar" && a.len() >= 4 {
        let title = match &a[0] {
            V::Str(text) => text.to_string(),
            other => legacy_text(other),
        };
        let bar = horizon_bukkit::BossBar::create(&title, &a[2].text().to_ascii_uppercase());
        bar.set_progress(a[1].double());
        return Some(Ok(wrap(bar)));
    }
    // `addRecipe(shapedRecipe)`: the SDK takes the general Recipe, plugins pass the kind they built.
    if matches!(class, "Bukkit" | "Server") && name == "addRecipe" && a.len() == 1 {
        use horizon_bukkit::extra::{Recipe, ShapedRecipe, ShapelessRecipe};
        let recipe = held::<ShapedRecipe>(&a[0]).map(|r| Recipe::Shaped(r.clone())).or_else(|| held::<ShapelessRecipe>(&a[0]).map(|r| Recipe::Shapeless(r.clone())));
        if let Some(recipe) = recipe {
            return Some(Ok(V::Bool(horizon_bukkit::extra::add_recipe(&recipe))));
        }
    }
    // `createInventory(holder, size | type [, title])`: a GUI that remembers its holder, for
    // `event.getInventory().getHolder() instanceof MyMenu`.
    // The holder may be the plugin's own object or a game object (a player, an entity, a block's state).
    if matches!(class, "Bukkit" | "Server") && name == "createInventory" && a.len() >= 2 && matches!(a[0], V::Obj(_) | V::Null | V::Native(_)) {
        let size = match &a[1] {
            V::Int(n) => *n as usize,
            V::Long(n) => *n as usize,
            kind => match kind.text().as_str() {
                "HOPPER" => 5,
                "DISPENSER" | "DROPPER" => 9,
                "WORKBENCH" | "CRAFTING" => 10,
                _ => 27,
            },
        };
        let title = match a.get(2) {
            None | Some(V::Null) => "Chest".to_owned(),
            Some(V::Str(text)) => text.to_string(),
            Some(component) => legacy_text(component),
        };
        let inventory = horizon_bukkit::Inventory::create(size, &title);
        if !a[0].is_null() {
            jrt::lock(&HOLDERS).push((inventory.clone(), a[0].clone()));
        }
        return Some(Ok(wrap(inventory)));
    }
    if class == "Bukkit" {
        if let Some(found) = player_by_id(name, a) {
            return Some(found);
        }
    }
    match (class, name, a.len()) {
        ("ChatColor", "translateAlternateColorCodes", 2) => {
            if a[1].is_null() {
                return Some(Err(jrt::exc("NullPointerException", "Cannot translate null text")));
            }
            Some(Ok(jrt::s(&translate_colors(a[0].text().chars().next().unwrap_or('&'), &a[1].text()))))
        }
        ("ChatColor", "stripColor", 1) => Some(Ok(if a[0].is_null() { V::Null } else { jrt::s(&strip_colors(&a[0].text())) })),
        ("Bukkit", "getScheduler", 0) => Some(Ok(jrt::native(SchedulerH))),
        ("UniversalScheduler", "getScheduler", 1) => Some(Ok(jrt::native(UniSchedH(a[0].clone())))),
        ("Bukkit", "getMessenger", 0) => Some(Ok(jrt::native(messenger::MessengerH))),
        // A plugin's listeners stop with it: what onDisable asks for here is done already.
        ("HandlerList", "unregisterAll", _) => Some(Ok(V::Null)),
        // BungeeCord's `ChatColor.of("#ff8800")`: the hex colour as legacy text, `§x§f§f§8§8§0§0`.
        ("ChatColor", "of", 1) => {
            let text = a[0].text();
            Some(Ok(match text.strip_prefix('#') {
                Some(hex) if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) => jrt::s(&hex.to_ascii_lowercase().chars().fold("\u{a7}x".to_owned(), |mut out, c| {
                    out.push('\u{a7}');
                    out.push(c);
                    out
                })),
                _ => match CHAT_COLORS.iter().find(|(color, _)| color.eq_ignore_ascii_case(&text)) {
                    Some((_, code)) => jrt::s(&format!("\u{a7}{code}")),
                    None => return Some(Err(jrt::exc("IllegalArgumentException", format!("Could not parse ChatColor {text}")))),
                },
            }))
        }
        ("Bukkit", "getServer", 0) => Some(Ok(wrap(the_plugin().get_server()))),
        ("Bukkit", "getLogger", 0) | ("Logger", "getLogger", _) | ("LoggerFactory", "getLogger", _) | ("LogManager", "getLogger" | "getRootLogger" | "getFormatterLogger", _) | ("PaperPluginLogger", "getLogger", _) | ("ComponentLogger", "logger", _) => Some(Ok(jrt::native(LoggerH))),
        ("MiniMessage", "miniMessage", 0) => Some(Ok(jrt::native(MiniH))),
        ("Placeholder", "parsed", 2) => Some(Ok(jrt::native(ResolverH(vec![(a[0].text(), a[1].text())])))),
        ("Placeholder", "unparsed", 2) => Some(Ok(jrt::native(ResolverH(vec![(a[0].text(), a[1].text().replace('<', r"\<"))])))),
        ("Placeholder", "component", 2) => {
            let json = component::json_of(&a[1]).unwrap_or_else(|| serde_json::json!({ "text": a[1].text() }));
            Some(Ok(jrt::native(ResolverH(vec![(a[0].text(), mini_serialize(&json))]))))
        }
        ("TagResolver", "resolver", _) => {
            let mut resolvers = Vec::new();
            resolvers_of(a, &mut resolvers);
            Some(Ok(jrt::native(ResolverH(resolvers))))
        }
        ("TagResolver", "empty", 0) => Some(Ok(jrt::native(ResolverH(Vec::new())))),
        ("Times", "times", 3) => Some(Ok(jrt::native(TimesH(ticks(&a[0]), ticks(&a[1]), ticks(&a[2]))))),
        ("Title", "title", 2 | 3) => {
            let times = a.get(2).and_then(|t| t.native::<TimesH>()).map_or((10, 70, 20), |t| (t.0, t.1, t.2));
            let title = held::<Component>(&a[0])?.clone();
            let subtitle = held::<Component>(&a[1])?.clone();
            Some(Ok(jrt::native(TitleH { title, subtitle, times })))
        }
        ("YamlConfiguration", "loadConfiguration", 1) if a[0].native::<JBytes>().is_some() => {
            let text = String::from_utf8_lossy(&jrt::javaio::bytes_of(&a[0])?).into_owned();
            let mut out = YamlConfiguration::new();
            if let Value::Object(map) = config::parse(&text) {
                for (key, value) in map {
                    out.set(&key, value);
                }
            }
            Some(Ok(wrap(out)))
        }
        ("JavaPlugin", "getPlugin" | "getProvidingPlugin", _) => Some(Ok(main_object())),
        _ => None,
    }
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    if let Some(found) = objstream::construct(class, a) {
        return Some(found);
    }
    if let Some(found) = packetevents::construct(class, a) {
        return Some(found);
    }
    if let Some(found) = protocollib::construct(class, a) {
        return Some(found);
    }
    if let Some(found) = snakeyaml::construct(class, a) {
        return Some(found);
    }
    if let Some(found) = lifecycle::construct(class, a) {
        return Some(found);
    }
    if let Some(found) = bungee::construct(class, a) {
        return Some(found);
    }
    match (class, a.len()) {
        // `new AttributeModifier(uuid, name, amount, operation[, EquipmentSlot])`, the form before 1.21, whose
        // key Paper makes `minecraft:<uuid>`; `(key, amount, operation, EquipmentSlotGroup)`, the 1.21 form.
        ("AttributeModifier", 4 | 5) if !matches!(a[1], V::Double(_) | V::Int(_) | V::Long(_)) || a.len() == 5 => {
            let key = if a.len() >= 4 && matches!(a[1], V::Str(_)) && matches!(a[2], V::Double(_) | V::Int(_) | V::Long(_)) { format!("minecraft:{}", a[0].text().to_lowercase()) } else { key_text(&a[0]) };
            let (amount, operation) = if matches!(a[1], V::Str(_)) && a.len() >= 4 { (a[2].double(), a[3].text()) } else { (a[1].double(), a[2].text()) };
            let slot_at = if matches!(a[1], V::Str(_)) && a.len() >= 4 { 4 } else { 3 };
            let mut modifier = horizon_bukkit::scoreboard::AttributeModifier::new(&key, amount, &operation);
            if let Some(slot) = a.get(slot_at).filter(|v| !v.is_null()) {
                modifier.slot = match slot.text().rsplit('.').next().unwrap_or_default().to_uppercase().as_str() {
                    "HAND" | "MAINHAND" => "mainhand".to_owned(),
                    "OFF_HAND" | "OFFHAND" => "offhand".to_owned(),
                    other => other.to_lowercase(),
                };
            }
            Some(Ok(wrap(modifier)))
        }
        ("AttributeModifier", 4) => {
            let mut modifier = horizon_bukkit::scoreboard::AttributeModifier::new(&key_text(&a[0]), a[1].double(), &a[2].text());
            modifier.slot = a[3].text().rsplit('.').next().unwrap_or("any").to_lowercase();
            Some(Ok(wrap(modifier)))
        }
        // `new Location(null, 0, 0, 0)`: a location with no world yet (Shopkeepers keeps one to reuse), which
        // `setWorld` gives one later.
        ("Location", 4 | 6) => {
            let world = if a[0].is_null() { World::of("") } else { held::<World>(&a[0])?.clone() };
            let (yaw, pitch) = if a.len() == 6 { (a[4].double() as f32, a[5].double() as f32) } else { (0.0, 0.0) };
            Some(Ok(wrap(Location::new(world, a[1].double(), a[2].double(), a[3].double(), yaw, pitch))))
        }
        ("YamlConfiguration", 0) => Some(Ok(wrap(YamlConfiguration::new()))),
        // Simple-YAML's `new YamlFile(file or path)`: a configuration bound to its file, empty until `load()`.
        ("YamlFile", 0 | 1) => {
            let file = wrap(YamlConfiguration::new());
            if let Some(path) = a.first().and_then(jrt::javaio::path_of) {
                jrt::lock(&YAML_FILES).get_or_insert_default().insert(identity(&file), path);
            }
            Some(Ok(file))
        }
        // `new FixedMetadataValue(plugin, value)`: the value as Java handed it over (a task, an entity), not as JSON.
        ("FixedMetadataValue", 2) => Some(Ok(jrt::native(FixedMetaH { plugin: a[0].clone(), value: a[1].clone() }))),
        // `class X extends Command { X() { super(name, description, usage, aliases); } }`: what the parent keeps.
        ("Command" | "BukkitCommand", 1 | 4) => {
            let aliases = a.get(3).and_then(|x| x.items().ok()).unwrap_or_default().iter().map(V::text).collect();
            let text = |at: usize| a.get(at).map(V::text).unwrap_or_default();
            Some(Ok(jrt::native(CmdBaseH(Mutex::new(CmdFields { name: text(0), description: text(1), usage: if a.len() > 2 { text(2) } else { format!("/{}", text(0)) }, aliases, permission: None, message: None })))))
        }
        _ => None,
    }
}

/// What a `Block.getState()` is in Java, by its block: `Chest`, `Container`, `Sign`... A TileState here is one type
/// for all of them, and the bindings' table said every one was `CreatureSpawner` and none was a `Container` (so
/// CoreProtect never read a broken chest's items). A plain block's state is a BlockState and not a TileState.
fn tile_state_is(key: &str, name: &str) -> bool {
    let block = key.trim_start_matches("minecraft:");
    const CONTAINER: &[&str] = &["Container", "BlockInventoryHolder", "InventoryHolder", "Lockable", "Nameable"];
    let (own, extra): (&[&str], &[&str]) = if matches!(block, "chest" | "trapped_chest") || block.ends_with("copper_chest") {
        (&["Chest", "Lidded", "Lootable"], CONTAINER)
    } else if block == "barrel" {
        (&["Barrel", "Lidded", "Lootable"], CONTAINER)
    } else if block.ends_with("shulker_box") {
        (&["ShulkerBox", "Lidded", "Lootable"], CONTAINER)
    } else if block == "furnace" {
        (&["Furnace"], CONTAINER)
    } else if block == "blast_furnace" {
        (&["Furnace", "BlastFurnace"], CONTAINER)
    } else if block == "smoker" {
        (&["Furnace", "Smoker"], CONTAINER)
    } else if block == "hopper" {
        (&["Hopper", "Lootable"], CONTAINER)
    } else if block == "dispenser" {
        (&["Dispenser", "Lootable"], CONTAINER)
    } else if block == "dropper" {
        (&["Dropper", "Lootable"], CONTAINER)
    } else if block == "brewing_stand" {
        (&["BrewingStand"], CONTAINER)
    } else if block == "crafter" {
        (&["Crafter", "Lootable"], CONTAINER)
    } else if block == "chiseled_bookshelf" {
        (&["ChiseledBookshelf", "BlockInventoryHolder", "InventoryHolder"], &[])
    } else if block == "decorated_pot" {
        (&["DecoratedPot", "BlockInventoryHolder", "InventoryHolder", "Lootable"], &[])
    } else if block == "jukebox" {
        (&["Jukebox", "BlockInventoryHolder", "InventoryHolder"], &[])
    } else if block == "lectern" {
        (&["Lectern", "BlockInventoryHolder", "InventoryHolder"], &[])
    } else if block.ends_with("hanging_sign") {
        (&["Sign", "HangingSign"], &[])
    } else if block.ends_with("sign") {
        (&["Sign"], &[])
    } else if block.ends_with("banner") {
        (&["Banner"], &[])
    } else if block.ends_with("skull") || block.ends_with("head") {
        (&["Skull"], &[])
    } else if block == "spawner" {
        (&["CreatureSpawner"], &[])
    } else if block == "beacon" {
        (&["Beacon", "Lockable", "Nameable"], &[])
    } else if matches!(block, "beehive" | "bee_nest") {
        (&["Beehive", "EntityBlockStorage"], &[])
    } else if block.ends_with("bed") {
        (&["Bed"], &[])
    } else if matches!(block, "bell" | "campfire" | "soul_campfire" | "comparator" | "conduit" | "daylight_detector" | "enchanting_table" | "end_gateway"
        | "end_portal" | "ender_chest" | "structure_block" | "jigsaw" | "sculk_sensor" | "calibrated_sculk_sensor" | "sculk_catalyst" | "sculk_shrieker"
        | "trial_spawner" | "vault" | "command_block" | "chain_command_block" | "repeating_command_block" | "suspicious_sand" | "suspicious_gravel"
        | "creaking_heart" | "test_block" | "test_instance_block") {
        (&[], &[])
    } else {
        return matches!(name, "BlockState" | "Metadatable" | "Object");
    };
    matches!(name, "BlockState" | "TileState" | "Metadatable" | "PersistentDataHolder" | "Object") || own.contains(&name) || extra.contains(&name)
}

pub fn instance_of(value: &V, name: &str) -> Option<bool> {
    // `entity.getLastDamageCause()`: an EntityDamageEvent, and an EntityDamageByEntityEvent when something hit it
    // (CoreProtect casts it so to find who made a kill).
    if let Some(last) = held::<horizon_bukkit::fam_entity::LastDamage>(value) {
        return Some(matches!(name, "EntityDamageEvent" | "EntityEvent" | "Event" | "Cancellable") || (name == "EntityDamageByEntityEvent" && last.has_damager()));
    }
    if let Some(state) = held::<horizon_bukkit::block_data::TileState>(value) {
        let key = state.get_type().key().to_owned();
        drop(state);
        return Some(tile_state_is(&key, name));
    }
    // A game entity, by what it is: the bindings' table said every Entity was an ArmorStand, a Mob and a Vehicle (the
    // SDK's Entity carries those interfaces' methods), so CoreProtect took a killed pig for an armor stand and logged
    // no kill.
    if let Some(entity) = held::<horizon_bukkit::entity::Entity>(value) {
        let kind = entity.get_type();
        drop(entity);
        if let Some(answer) = entity_is(kind.trim_start_matches("minecraft:"), name) {
            return Some(answer);
        }
    }
    // The game's World, as Bukkit types it (the bindings' table had no row for it: `adapt(org.bukkit.World)` found no
    // overload once overloads told WorldEdit's World from Bukkit's).
    if held::<World>(value).is_some() && matches!(name, "World" | "RegionAccessor" | "WorldInfo" | "Keyed" | "PluginMessageRecipient" | "Metadatable" | "PersistentDataHolder" | "ForwardingAudience" | "Audience" | "Object") {
        return Some(true);
    }
    // Another plugin's object: its own plugin knows its types (PlaceholderAPI asks an expansion
    // `instanceof Configurable`, `Listener`, `Taskable` before it calls their methods).
    if xplugin::is_remote(value) {
        return Some(xplugin::remote_instance_of(value, name));
    }
    if value.native::<Mutex<std::net::SocketAddr>>().is_some() {
        return Some(matches!(name, "InetSocketAddress" | "SocketAddress" | "Serializable"));
    }
    if let Some(ip) = held::<std::net::IpAddr>(value).map(|ip| *ip) {
        return Some(matches!(name, "InetAddress" | "Serializable") || (name == "Inet4Address" && ip.is_ipv4()) || (name == "Inet6Address" && ip.is_ipv6()));
    }
    if let Some(held_value) = held::<Component>(value) {
        let translatable = held_value.0.get("translate").is_some();
        return Some(matches!(name, "Component" | "ComponentLike" | "BuildableComponent") || (name == "TextComponent" && !translatable) || (name == "TranslatableComponent" && translatable));
    }
    if value.native::<component::BuilderH>().is_some() {
        return Some(matches!(name, "ComponentLike" | "ComponentBuilder" | "Builder" | "Style"));
    }
    if value.native::<Mutex<Player>>().is_some() {
        return Some(matches!(name, "Player" | "HumanEntity" | "LivingEntity" | "Entity" | "CommandSender" | "Permissible" | "ServerOperator" | "OfflinePlayer" | "Audience" | "Damageable" | "Attributable" | "Nameable" | "Metadatable" | "ForwardingAudience" | "Sender" | "Identified" | "Conversable" | "InventoryHolder" | "AnimalTamer" | "Identity"));
    }
    if value.native::<SectionH>().is_some() || value.native::<Mutex<YamlConfiguration>>().is_some() {
        return Some(matches!(name, "ConfigurationSection" | "MemorySection") || (value.native::<SectionH>().is_none() && matches!(name, "Configuration" | "FileConfiguration" | "YamlConfiguration" | "MemoryConfiguration")));
    }
    if let Some(sender) = value.native::<Mutex<CommandSender>>() {
        let console = matches!(*jrt::lock(sender), CommandSender::Console);
        return Some(matches!(name, "CommandSender" | "Permissible" | "ServerOperator" | "Audience") || (console && name == "ConsoleCommandSender"));
    }
    None
}

const NOT_LIVING: &[&str] = &[
    "item", "experience_orb", "tnt", "falling_block", "area_effect_cloud", "lightning_bolt", "end_crystal", "marker",
    "evoker_fangs", "eye_of_ender", "leash_knot", "painting", "item_frame", "glow_item_frame", "firework_rocket",
    "fishing_bobber", "ominous_item_spawner", "interaction", "block_display", "item_display", "text_display",
];
const PROJECTILES: &[&str] = &[
    "arrow", "spectral_arrow", "trident", "snowball", "egg", "blue_egg", "brown_egg", "ender_pearl", "fireball",
    "small_fireball", "dragon_fireball", "wither_skull", "potion", "splash_potion", "lingering_potion",
    "experience_bottle", "llama_spit", "shulker_bullet", "firework_rocket", "wind_charge", "breeze_wind_charge",
    "fishing_bobber",
];
const ANIMALS: &[&str] = &[
    "cow", "mooshroom", "pig", "sheep", "chicken", "rabbit", "horse", "donkey", "mule", "skeleton_horse", "zombie_horse",
    "llama", "trader_llama", "camel", "camel_husk", "wolf", "cat", "ocelot", "parrot", "fox", "panda", "polar_bear",
    "turtle", "bee", "goat", "axolotl", "frog", "sniffer", "armadillo", "strider", "hoglin", "happy_ghast", "nautilus",
    "zombie_nautilus",
];
const MONSTERS: &[&str] = &[
    "zombie", "husk", "drowned", "zombie_villager", "zombified_piglin", "skeleton", "stray", "wither_skeleton", "bogged",
    "parched", "creeper", "spider", "cave_spider", "enderman", "endermite", "silverfish", "blaze", "witch", "guardian",
    "elder_guardian", "vex", "vindicator", "evoker", "illusioner", "pillager", "ravager", "piglin", "piglin_brute",
    "zoglin", "warden", "breeze", "creaking", "giant", "wither",
];
const ZOMBIES: &[&str] = &["zombie", "husk", "drowned", "zombie_villager", "zombified_piglin"];
const SKELETONS: &[&str] = &["skeleton", "stray", "wither_skeleton", "bogged", "parched"];
const HORSES: &[&str] = &["horse", "donkey", "mule", "skeleton_horse", "zombie_horse", "llama", "trader_llama", "camel", "camel_husk"];
const RAIDERS: &[&str] = &["evoker", "illusioner", "vindicator", "pillager", "ravager", "witch"];
const FISH: &[&str] = &["cod", "salmon", "pufferfish", "tropical_fish"];

/// `entity instanceof name` for a game entity of `kind` (`pig`): Some when the name is one of Bukkit's entity
/// interfaces or an entity type's own, None for anything else (the bindings' table answers those).
fn entity_is(kind: &str, name: &str) -> Option<bool> {
    if kind.is_empty() {
        return None;
    }
    let is = |list: &[&str]| list.contains(&kind);
    let minecart = kind.ends_with("minecart");
    let boat = kind.ends_with("_boat") || kind.ends_with("_raft");
    let living = !is(NOT_LIVING) && !is(PROJECTILES) && !minecart && !boat;
    let mob = living && kind != "armor_stand";
    Some(match name {
        "Entity" | "Nameable" | "Metadatable" | "PersistentDataHolder" | "PersistentDataContainer" | "Frictional"
        | "CommandSender" | "Permissible" | "ServerOperator" | "Audience" | "ForwardingAudience" | "Object" => true,
        "LivingEntity" | "Damageable" | "Attributable" | "ProjectileSource" => living,
        "Mob" => mob,
        "Creature" | "PathfinderMob" => mob && !matches!(kind, "ghast" | "happy_ghast" | "phantom" | "slime" | "magma_cube" | "ender_dragon" | "bat"),
        "Animals" | "Breedable" => is(ANIMALS),
        "Ageable" => is(ANIMALS) || matches!(kind, "villager" | "wandering_trader" | "piglin" | "zoglin") || is(ZOMBIES),
        "Monster" | "Enemy" => is(MONSTERS),
        "Tameable" | "OfflinePlayerOwnable" => is(HORSES) || matches!(kind, "wolf" | "cat" | "parrot"),
        "Sittable" => matches!(kind, "wolf" | "cat" | "parrot"),
        "AbstractHorse" => is(HORSES),
        "ChestedHorse" => matches!(kind, "donkey" | "mule" | "llama" | "trader_llama"),
        "Llama" => matches!(kind, "llama" | "trader_llama"),
        "Zombie" => is(ZOMBIES),
        "AbstractSkeleton" | "Skeleton" => is(SKELETONS),
        "Raider" => is(RAIDERS),
        "Illager" => matches!(kind, "evoker" | "illusioner" | "vindicator" | "pillager"),
        "Spellcaster" => matches!(kind, "evoker" | "illusioner"),
        "Fish" => is(FISH),
        "WaterMob" => is(FISH) || matches!(kind, "squid" | "glow_squid" | "dolphin" | "tadpole"),
        "Squid" => matches!(kind, "squid" | "glow_squid"),
        "Flying" => matches!(kind, "ghast" | "happy_ghast" | "phantom"),
        "Ambient" => kind == "bat",
        "Golem" => matches!(kind, "iron_golem" | "snow_golem" | "shulker" | "copper_golem"),
        "Snowman" => kind == "snow_golem",
        "MushroomCow" => kind == "mooshroom",
        "PigZombie" => kind == "zombified_piglin",
        "EnderCrystal" => kind == "end_crystal",
        "TNTPrimed" => kind == "tnt",
        "LightningStrike" => kind == "lightning_bolt",
        "ComplexLivingEntity" | "EnderDragon" => kind == "ender_dragon",
        "Boss" => matches!(kind, "ender_dragon" | "wither"),
        "AbstractVillager" | "Merchant" => matches!(kind, "villager" | "wandering_trader"),
        "Vehicle" => minecart || boat || is(HORSES) || matches!(kind, "pig" | "strider" | "happy_ghast" | "nautilus" | "zombie_nautilus"),
        "Minecart" => minecart,
        "Boat" => boat,
        "ChestBoat" => kind.ends_with("_chest_boat") || kind.ends_with("_chest_raft"),
        "Hanging" => matches!(kind, "painting" | "item_frame" | "glow_item_frame" | "leash_knot"),
        "ItemFrame" => matches!(kind, "item_frame" | "glow_item_frame"),
        "Projectile" => is(PROJECTILES),
        "AbstractArrow" => matches!(kind, "arrow" | "spectral_arrow" | "trident"),
        "ThrowableProjectile" => matches!(kind, "snowball" | "egg" | "blue_egg" | "brown_egg" | "ender_pearl" | "experience_bottle" | "potion" | "splash_potion" | "lingering_potion"),
        "ThrownPotion" => matches!(kind, "potion" | "splash_potion" | "lingering_potion"),
        "Fireball" | "SizedFireball" => matches!(kind, "fireball" | "small_fireball" | "dragon_fireball" | "wither_skull" | "wind_charge" | "breeze_wind_charge"),
        "Display" => kind.ends_with("_display"),
        "AbstractCubeMob" => matches!(kind, "slime" | "magma_cube"),
        "AbstractNautilus" => matches!(kind, "nautilus" | "zombie_nautilus"),
        "Explosive" => matches!(kind, "tnt" | "fireball" | "small_fireball" | "dragon_fireball" | "wither_skull" | "wind_charge" | "breeze_wind_charge"),
        "PiglinAbstract" => matches!(kind, "piglin" | "piglin_brute"),
        "Steerable" => matches!(kind, "pig" | "strider"),
        "Firework" => kind == "firework_rocket",
        "FishHook" => kind == "fishing_bobber",
        "LeashHitch" => kind == "leash_knot",
        "EnderSignal" => kind == "eye_of_ender",
        "PufferFish" => kind == "pufferfish",
        "Egg" => matches!(kind, "egg" | "blue_egg" | "brown_egg"),
        "Camel" => matches!(kind, "camel" | "camel_husk"),
        "Player" | "HumanEntity" | "OfflinePlayer" => false,
        // The type's own interface: `Pig`, `ArmorStand` (armor_stand), `GlowItemFrame`, `Item`, `ExperienceOrb`...
        other => {
            let mut snake = String::new();
            for (i, c) in other.chars().enumerate() {
                if c.is_ascii_uppercase() && i > 0 {
                    snake.push('_');
                }
                snake.push(c.to_ascii_lowercase());
            }
            if snake == kind {
                true
            } else if other.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !other.contains('.') && BUKKIT_ENTITY_NAMES.contains(&other) {
                false
            } else {
                return None;
            }
        }
    })
}

/// Bukkit entity interfaces named after one entity type: `instanceof` one of these is false for every other type.
const BUKKIT_ENTITY_NAMES: &[&str] = &[
    "ArmorStand", "Pig", "Cow", "Sheep", "Chicken", "Rabbit", "Horse", "Donkey", "Mule", "SkeletonHorse", "ZombieHorse",
    "Camel", "Wolf", "Cat", "Ocelot", "Parrot", "Fox", "Panda", "PolarBear", "Turtle", "Bee", "Goat", "Axolotl", "Frog",
    "Sniffer", "Armadillo", "Strider", "Hoglin", "Villager", "WanderingTrader", "Creeper", "Spider", "CaveSpider",
    "Enderman", "Endermite", "Silverfish", "Blaze", "Witch", "Guardian", "ElderGuardian", "Vex", "Vindicator", "Evoker",
    "Illusioner", "Pillager", "Ravager", "Piglin", "PiglinBrute", "Zoglin", "Warden", "Breeze", "Creaking", "Giant",
    "Wither", "Husk", "Drowned", "ZombieVillager", "Stray", "WitherSkeleton", "Bogged", "Slime", "MagmaCube", "Ghast",
    "Phantom", "Bat", "Dolphin", "Tadpole", "IronGolem", "Shulker", "Allay", "Item", "ExperienceOrb", "FallingBlock",
    "Painting", "GlowItemFrame", "LeashHitch", "Arrow", "SpectralArrow", "Trident", "Snowball", "Egg", "EnderPearl",
    "SmallFireball", "DragonFireball", "WitherSkull", "LlamaSpit", "ShulkerBullet", "Firework", "WindCharge",
    "FishHook", "AreaEffectCloud", "EvokerFangs", "EnderSignal", "Marker", "Interaction", "BlockDisplay",
    "ItemDisplay", "TextDisplay", "TropicalFish", "PufferFish", "Cod", "Salmon", "GlowSquid", "TraderLlama",
];

/// A game object as text: what `equals` and map keys go by.
pub fn describe(value: &V) -> Option<String> {
    if let Some(player) = held::<Player>(value) {
        return Some(format!("CraftPlayer{{name={}}}", player.get_name()));
    }
    if let Some(world) = held::<World>(value) {
        return Some(format!("CraftWorld{{name={}}}", world.get_name()));
    }
    if let Some(at) = held::<Location>(value) {
        return Some(format!("Location{{world={},x={},y={},z={}}}", at.get_world().get_name(), at.get_x(), at.get_y(), at.get_z()));
    }
    None
}

/// `JavaPlugin.onCommand`: what `plugin.yml` declares, for commands nothing took.
pub fn default_executors(main: &V) {
    let plugin = the_plugin();
    let Value::Object(commands) = plugin.get_commands() else { return };
    for name in commands.keys() {
        if plugin.get_executor(name).is_none() {
            let _ = command_method(&CommandH(name.clone()), "setExecutor", std::slice::from_ref(main));
        }
    }
}
