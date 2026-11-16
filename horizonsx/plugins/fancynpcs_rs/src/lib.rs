//! FancyNpcs 2.12.1 rewritten in Rust for Horizon: same plugin name, data folder, config.yml, npcs.yml,
//! languages/default.yml, `/npc` and `/fancynpcs` commands and permissions.
//!
//! NPCs exist only as packets their viewers' clients get. Performance: the server thread reads the players once a
//! few ticks (one `players` query, plus the `.see` permissions and placeholder names the NPCs need) and wakes the
//! NPC thread, which decides who sees what, turns NPCs to players and sends the packets as bytes straight to the
//! players' connections. Skins are fetched on threads of their own; npcs.yml is written on a writer thread.

pub mod actions;
pub mod commands;
pub mod net;
pub mod npc;
pub mod skins;
pub mod view;

use horizon_bukkit::prelude::*;
use horizon_kit::Config;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Instant;

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static WORKER: horizon_kit::worker::Worker = horizon_kit::worker::Worker::new();
pub static FORCE_VISIBILITY: AtomicBool = AtomicBool::new(false);
static DIRTY: AtomicBool = AtomicBool::new(false);
/// Players who joined and when (NPCs come after spawn_delay_on_join).
static JOINED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

pub fn folder() -> Option<std::path::PathBuf> {
    PLUGIN.lock().unwrap_or_else(|e| e.into_inner()).map(|p| p.get_data_folder())
}

pub fn log(text: &str) {
    horizon_bukkit::horizon_mod::log(&format!("[FancyNpcs] {text}"));
}

/// Runs `f` on the server thread at the next tick (the SDK scheduler only queues: safe from any thread).
pub fn on_main(f: impl FnOnce() + Send + 'static) {
    let mut f = Some(f);
    horizon_bukkit::scheduler::run_task(move || {
        if let Some(f) = f.take() {
            f();
        }
    });
}

// ---- Randomness / ids ----

pub fn rand_u64() -> u64 {
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut s = STATE.load(Ordering::Relaxed);
    if s == 0 {
        s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x9E37_79B9, |d| d.as_nanos() as u64) | 1;
    }
    s ^= s << 13;
    s ^= s >> 7;
    s ^= s << 17;
    STATE.store(s, Ordering::Relaxed);
    s
}
pub fn rand_u32() -> u32 {
    rand_u64() as u32
}
pub fn rand_u128() -> u128 {
    (u128::from(rand_u64()) << 64) | u128::from(rand_u64())
}
pub fn uuid_string(u: u128) -> String {
    let h = format!("{u:032x}");
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}
pub fn new_uuid_string() -> String {
    uuid_string(rand_u128() & !(0xF000u128 << 64) | (0x4000u128 << 64))
}

// ---- config.yml and the language file ----

pub struct Files {
    pub config: Config,
    pub lang: Config,
}

static FILES: RwLock<Option<Files>> = RwLock::new(None);
const CONFIG: &str = include_str!("../data/config.yml");
const LANG: &str = include_str!("../data/languages/default.yml");

pub fn load_files() {
    let Some(folder) = folder() else { return };
    let config = Config::load(&folder, "config.yml", CONFIG);
    let lang_name = config.str("language", "default");
    let lang_dir = folder.join("languages");
    let _ = Config::load(&lang_dir, "default.yml", LANG);
    let lang = Config::load(&lang_dir, &format!("{lang_name}.yml"), LANG);
    *FILES.write().unwrap_or_else(|e| e.into_inner()) = Some(Files { config, lang });
}

pub fn files<R>(f: impl FnOnce(&Files) -> R) -> R {
    let held = FILES.read().unwrap_or_else(|e| e.into_inner());
    match held.as_ref() {
        Some(x) => f(x),
        None => f(&Files { config: Config::parse(CONFIG), lang: Config::parse(LANG) }),
    }
}

pub fn cfg_int(path: &str, def: i64) -> i64 {
    files(|f| f.config.int(path, def))
}
pub fn cfg_bool(path: &str, def: bool) -> bool {
    files(|f| f.config.bool(path, def))
}
pub fn cfg_str(path: &str, def: &str) -> String {
    files(|f| f.config.str(path, def))
}

const PRIMARY: &str = "#ffcc24";
const SECONDARY: &str = "gray";
const SUCCESS: &str = "#81E366";
const WARNING: &str = "#E3CA66";
const ERROR: &str = "#E36666";
const PREFIX: &str = "<color:#ba8813>[</color><gradient:#ffae00:#fffb00:#ffae00>FancyNpcs</gradient><color:#ba8813>]</color> <gray>";

/// The colour names of the language files filled in.
pub fn colors(text: &str) -> String {
    text.replace("{primaryColor}", &format!("<color:{PRIMARY}>"))
        .replace("{secondaryColor}", &format!("<color:{SECONDARY}>"))
        .replace("{successColor}", &format!("<color:{SUCCESS}>"))
        .replace("{warningColor}", &format!("<color:{WARNING}>"))
        .replace("{errorColor}", &format!("<color:{ERROR}>"))
}

/// A message (`messages.<key>`, a line or a list) with `{name}` values, as MiniMessage text.
pub fn tr(key: &str, pairs: &[(&str, &str)]) -> String {
    let raw = files(|f| match f.lang.at(&format!("messages.{key}")) {
        Some(serde_yaml::Value::Sequence(lines)) => lines.iter().filter_map(|l| l.as_str()).collect::<Vec<_>>().join("<newline>"),
        Some(serde_yaml::Value::String(s)) => s.clone(),
        _ => format!("<red>{key}"),
    });
    let mut text = raw;
    for (k, v) in pairs {
        text = text.replace(&format!("{{{k}}}"), v).replace(&format!("%{k}%"), v);
    }
    colors(&text)
}

/// Sends MiniMessage text (with the FancyNpcs prefix when asked).
pub fn send_mini(to: &CommandSender, mini: &str, prefix: bool) {
    let text = if prefix { format!("{PREFIX}{mini}") } else { format!("<{SECONDARY}>{mini}") };
    to.send_component(&Component(horizon_kit::mini::parse_mixed(&text)));
}

pub fn say(to: &CommandSender, key: &str, pairs: &[(&str, &str)]) {
    send_mini(to, &tr(key, pairs), true);
}

pub fn say_plain(to: &CommandSender, key: &str, pairs: &[(&str, &str)]) {
    send_mini(to, &tr(key, pairs), false);
}

/// A display name as text JSON (MiniMessage and legacy codes, placeholders filled for `player` or none).
pub fn name_json(display: &str, player: Option<&Player>) -> serde_json::Value {
    let filled = if display.contains('%') { horizon_kit::hooks::placeholders(display, player) } else { display.to_owned() };
    horizon_kit::mini::parse_mixed(&filled)
}

// ---- npcs.yml ----

pub fn load_npcs() {
    let Some(folder) = folder() else { return };
    let text = std::fs::read_to_string(folder.join("npcs.yml")).unwrap_or_default();
    let doc: serde_yaml::Value = horizon_kit::yaml::parse(&text).unwrap_or(serde_yaml::Value::Null);
    let mut list = Vec::new();
    if let Some(serde_yaml::Value::Mapping(m)) = doc.get("npcs") {
        for (k, v) in m {
            let (Some(id), Some(section)) = (k.as_str(), v.as_mapping()) else { continue };
            if let Some(data) = npc::from_yaml(id, section) {
                let mut n = npc::Npc::new(data);
                n.name_json = name_json(&n.data.display_name, None);
                n.dirty = false;
                list.push(n);
            }
        }
    }
    let count = list.len();
    let mut all = view::npcs();
    for n in all.iter_mut() {
        view::remove_all(n);
    }
    *all = list;
    drop(all);
    FORCE_VISIBILITY.store(true, Ordering::Release);
    log(&format!("Loaded {count} NPCs."));
}

/// npcs.yml written on a thread of its own (made here).
pub fn save_npcs() {
    let Some(folder) = folder() else { return };
    let mut top = serde_yaml::Mapping::new();
    for n in view::npcs().iter_mut() {
        top.insert(n.data.id.clone().into(), serde_yaml::Value::Mapping(npc::to_yaml(&n.data)));
        n.dirty = false;
    }
    let mut doc = serde_yaml::Mapping::new();
    doc.insert("npcs".into(), serde_yaml::Value::Mapping(top));
    let Ok(text) = serde_yaml::to_string(&doc) else { return };
    let path = folder.join("npcs.yml");
    std::thread::spawn(move || {
        let tmp = path.with_extension("yml.tmp");
        if std::fs::write(&tmp, &text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    });
}

pub fn mark_dirty() {
    DIRTY.store(true, Ordering::Release);
}

// ---- The tick: players read here, the NPC thread does the rest ----

static TICKS: AtomicU64 = AtomicU64::new(0);

fn snapshot(visibility: bool, turn: bool) {
    let answer = horizon_bukkit::horizon_mod::query(serde_json::json!({ "op": "players" }));
    let delay = std::time::Duration::from_millis(cfg_int("spawn_delay_on_join", 40).max(0) as u64 * 50);
    let joined = JOINED.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default();
    let mut worlds: HashMap<String, String> = HashMap::new();
    let (perm_npcs, placeholder_npcs): (Vec<String>, Vec<(String, String)>) = {
        let all = view::npcs();
        (all.iter().filter(|n| n.data.visibility == "PERMISSION_REQUIRED").map(|n| n.data.name.clone()).collect(), all.iter().filter(|n| n.data.display_name.contains('%')).map(|n| (n.data.id.clone(), n.data.display_name.clone())).collect())
    };
    let mut viewers = Vec::new();
    for p in answer.as_array().into_iter().flatten() {
        let (Some(uuid), Some(name)) = (p["uuid"].as_str(), p["name"].as_str()) else { continue };
        if joined.get(uuid).is_some_and(|at| at.elapsed() < delay) {
            continue;
        }
        let dim = p["dimension"].as_str().unwrap_or("minecraft:overworld");
        let world = worlds.entry(dim.to_owned()).or_insert_with(|| World::of(dim).get_name().to_owned()).clone();
        let pos = [p["position"][0].as_f64().unwrap_or(0.0), p["position"][1].as_f64().unwrap_or(0.0), p["position"][2].as_f64().unwrap_or(0.0)];
        let mut v = view::Viewer { uuid: uuid.to_owned(), name: name.to_owned(), world, pos, sees: Default::default(), names: HashMap::new() };
        if visibility && (!perm_npcs.is_empty() || !placeholder_npcs.is_empty()) {
            let player = Player::of_ids(uuid, name);
            for n in &perm_npcs {
                if player.has_permission(&format!("fancynpcs.npc.{n}.see")) {
                    v.sees.insert(n.clone());
                }
            }
            for (id, display) in &placeholder_npcs {
                v.names.insert(id.clone(), name_json(display, Some(&player)));
            }
        }
        viewers.push(v);
    }
    let settings = view::Settings {
        visibility_distance: cfg_int("visibility_distance", 20) as i32,
        turn_distance: cfg_int("turn_to_player_distance", 5) as i32,
        reset_direction: cfg_bool("turn_to_player_reset_to_initial_direction", false),
        skip_invisible: cfg_bool("skip_invisible_npcs", true),
        tab_remove_ms: cfg_int("remove_npcs_from_playerlist_delay", 2000).max(0) as u64,
        swing_arm: cfg_bool("swing_arm_on_update", true),
    };
    *view::INBOX.lock().unwrap_or_else(|e| e.into_inner()) = Some(view::Snapshot { viewers, visibility, turn, settings });
    WORKER.wake();
}

fn tick() {
    let t = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    let vis_every = cfg_int("npc_update_visibility_interval", 20).max(1) as u64;
    let turn_every = cfg_int("turn_to_player_tracker_interval", 3).max(1) as u64;
    let forced = FORCE_VISIBILITY.swap(false, Ordering::AcqRel);
    let visibility = forced || t % vis_every == 0;
    let turn = t % turn_every == 0 && view::npcs().iter().any(|n| n.data.turn_to_player && !n.viewers.is_empty());
    if visibility || turn {
        snapshot(visibility, turn);
    }
    horizon_kit::packets::flush();
}

fn register_events(plugin: &Plugin) {
    plugin.register_event(|e: &mut PlayerJoinEvent| {
        let p = e.get_player().clone();
        let uuid = p.get_unique_id().replace('-', "");
        JOINED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(uuid, Instant::now());
        if view::npcs().iter().any(|n| n.data.mirror_skin) {
            let profile = p.get_player_profile();
            if let Some((_, value, sig)) = profile.get_properties().iter().find(|(n, _, _)| n == "textures") {
                skins::remember(p.get_name(), value, sig.as_deref().unwrap_or(""));
            }
        }
        let delay = cfg_int("spawn_delay_on_join", 40).max(0) as u64;
        horizon_bukkit::scheduler::run_task_later(delay + 1, || FORCE_VISIBILITY.store(true, Ordering::Release));
    });
    plugin.register_event(|e: &mut PlayerQuitEvent| {
        let uuid = e.get_player().get_unique_id().replace('-', "");
        if let Some(m) = JOINED.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            m.remove(&uuid);
        }
        for n in view::npcs().iter_mut() {
            n.viewers.remove(&uuid);
            n.teams_sent.remove(&uuid);
            n.looking.remove(&uuid);
        }
    });
    plugin.register_event(|_: &mut PlayerChangedWorldEvent| {
        FORCE_VISIBILITY.store(true, Ordering::Release);
    });
    plugin.register_event(|e: &mut PlayerUseUnknownEntityEvent| {
        if !e.get_hand().eq_ignore_ascii_case("HAND") {
            return;
        }
        let id = e.get_entity_id();
        let attack = e.is_attack();
        let Some(p) = e.get_player() else { return };
        actions::interact(&p, id, if attack { "LEFT_CLICK" } else { "RIGHT_CLICK" });
    });
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    load_files();
    if !net::init() {
        log("The engine does not name the NPC packets: NPCs will not show.");
    }
    skins::load();
    load_npcs();
    commands::register(&plugin);
    register_events(&plugin);
    WORKER.start("FancyNpcs", view::tick);
    plugin.run_task_timer(1, 1, tick);
    // Saves: a burst of changes is one write, a few seconds later; plus the autosave.
    plugin.run_task_timer(100, 100, || {
        if DIRTY.swap(false, Ordering::AcqRel) {
            save_npcs();
        }
    });
    if cfg_bool("enable_autosave", true) {
        let every = cfg_int("autosave_interval", 15).max(1) as u64 * 60 * 20;
        plugin.run_task_timer(every, every, || {
            if view::npcs().iter().any(|n| n.dirty) {
                save_npcs();
            }
        });
    }
    for p in get_online_players() {
        JOINED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(p.get_unique_id().replace('-', ""), Instant::now() - std::time::Duration::from_secs(3600));
    }
    log("FancyNpcs 2.12.1 (Rust) enabled.");
}

fn on_disable(_plugin: Plugin) {
    WORKER.stop();
    for n in view::npcs().iter_mut() {
        view::remove_all(n);
    }
    // The last save, written here (the writer threads may not outlive the plugin).
    if let Some(folder) = folder() {
        let mut top = serde_yaml::Mapping::new();
        for n in view::npcs().iter() {
            top.insert(n.data.id.clone().into(), serde_yaml::Value::Mapping(npc::to_yaml(&n.data)));
        }
        let mut doc = serde_yaml::Mapping::new();
        doc.insert("npcs".into(), serde_yaml::Value::Mapping(top));
        if let Ok(text) = serde_yaml::to_string(&doc) {
            let _ = std::fs::write(folder.join("npcs.yml"), text);
        }
    }
    view::npcs().clear();
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
