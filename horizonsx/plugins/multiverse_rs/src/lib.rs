//! Multiverse-Core 5.8.2 rewritten in Rust for Horizon. Same plugin name, data folder, worlds.yml, config.yml,
//! anchors.yml, messages, commands (`/mv ...` and the old `/mvtp`-style names) and permissions as the Java plugin.
//!
//! Performance: nothing runs per tick. Worlds load on enable; worlds.yml/anchors.yml are written on a writer thread
//! (a burst of changes is one write); world folders are copied (clone) and deleted (delete, regen) on threads of
//! their own, the result coming back to the server thread through the scheduler.

pub mod commands;
pub mod dest;
pub mod util;
pub mod worlds;

use horizon_bukkit::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};
use util::{log, m};

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);

pub fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn folder() -> Option<PathBuf> {
    plugin().map(|p| p.get_data_folder())
}

/// config.yml.
pub mod config {
    use horizon_kit::Config;
    use std::sync::RwLock;

    pub static CONFIG: RwLock<Option<Config>> = RwLock::new(None);
    pub const DEFAULT: &str = include_str!("../data/config.yml");

    pub fn load(folder: &std::path::Path) {
        *CONFIG.write().unwrap_or_else(|e| e.into_inner()) = Some(Config::load(folder, "config.yml", DEFAULT));
    }
    fn with<R>(f: impl FnOnce(&Config) -> R) -> R {
        let held = CONFIG.read().unwrap_or_else(|e| e.into_inner());
        match held.as_ref() {
            Some(c) => f(c),
            None => f(&Config::parse(DEFAULT)),
        }
    }
    pub fn b(path: &str, def: bool) -> bool {
        with(|c| c.bool(path, def))
    }
    pub fn s(path: &str, def: &str) -> String {
        with(|c| c.str(path, def))
    }
    pub fn i(path: &str, def: i64) -> i64 {
        with(|c| c.int(path, def))
    }
    fn leaves(v: &serde_yaml::Value, prefix: &str, out: &mut Vec<String>) {
        if let serde_yaml::Value::Mapping(m) = v {
            for (k, v) in m {
                let Some(k) = k.as_str() else { continue };
                let path = if prefix.is_empty() { k.to_owned() } else { format!("{prefix}.{k}") };
                if v.is_mapping() {
                    leaves(v, &path, out);
                } else if path != "version" {
                    out.push(path);
                }
            }
        }
    }
    pub fn leaf_paths() -> Vec<String> {
        let mut out = Vec::new();
        with(|c| leaves(&c.doc, "", &mut out));
        out
    }
    /// The names `/mv config` takes: the last part of each path.
    pub fn leaf_names() -> Vec<String> {
        leaf_paths().into_iter().map(|p| p.rsplit('.').next().unwrap_or(&p).to_owned()).collect()
    }
    pub fn find_path(name: &str) -> Option<String> {
        leaf_paths().into_iter().find(|p| p.eq_ignore_ascii_case(name) || p.rsplit('.').next().is_some_and(|l| l.eq_ignore_ascii_case(name)))
    }
    /// A value set from text (true/false, numbers kept as numbers) and the file written on the writer thread.
    pub fn set(path: &str, value: &str) {
        let v: serde_yaml::Value = match value {
            "true" | "false" => serde_yaml::Value::Bool(value == "true"),
            _ => value.parse::<i64>().map(Into::into).or_else(|_| value.parse::<f64>().map(Into::into)).unwrap_or_else(|_| value.into()),
        };
        let mut held = CONFIG.write().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = held.as_mut() {
            c.set(path, v);
            if let Ok(text) = serde_yaml::to_string(&c.doc) {
                crate::util::write_file(c.path.clone(), text);
            }
        }
    }
}

/// Config, messages, anchors and worlds.yml read again; new auto-load worlds loaded.
pub fn reload_all() {
    let Some(folder) = folder() else { return };
    config::load(&folder);
    util::load_messages(&folder, &config::s("messaging.default-locale", "en"));
    dest::load_anchors(&folder);
    worlds::load_file(&folder);
    worlds::load_all();
}

/// A player's game mode and flight set from the world they are in (enforce-gamemode / enforce-flight).
pub fn apply_player(p: &Player, w: &worlds::MvWorld) {
    if config::b("world.enforce-gamemode", true) && !p.has_permission(&format!("mv.bypass.gamemode.{}", w.name)) {
        let gm = w.gamemode();
        if !p.get_game_mode().eq_ignore_ascii_case(&gm) {
            p.set_game_mode(&gm);
        }
    }
    if config::b("world.enforce-flight", true) {
        let mode = p.get_game_mode();
        if !mode.eq_ignore_ascii_case("CREATIVE") && !mode.eq_ignore_ascii_case("SPECTATOR") {
            let allow = w.b("allow-flight", false);
            if !allow && p.is_flying() {
                p.set_flying(false);
            }
            p.set_allow_flight(allow);
        }
    }
}

/// Each online player's world alias, for the chat prefix (the chat event runs off the server thread).
static CHAT_WORLD: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

fn note_world(p: &Player) {
    let alias = worlds::find(p.get_world().get_name()).map_or_else(|| p.get_world().get_name().to_owned(), |w| w.alias_or_name());
    CHAT_WORLD.write().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(p.get_unique_id(), alias);
}

fn default_spawn() -> Option<Location> {
    worlds::find("world").and_then(|w| w.spawn())
}

/// The overworld of a nether/end world (`x_nether` -> `x`).
fn overworld_of(name: &str) -> String {
    let nether = config::s("world.world-name-format.nether", "%overworld%_nether").replace("%overworld%", "");
    let end = config::s("world.world-name-format.end", "%overworld%_the_end").replace("%overworld%", "");
    name.strip_suffix(&nether).or_else(|| name.strip_suffix(&end)).unwrap_or(name).to_owned()
}

fn register_events(plugin: &Plugin) {
    let teleport_priority: &'static str = match config::s("event-priority.player-teleport", "HIGHEST").to_ascii_uppercase().as_str() {
        "LOWEST" => enums::event_priority::LOWEST,
        "LOW" => enums::event_priority::LOW,
        "NORMAL" => enums::event_priority::NORMAL,
        "HIGH" => enums::event_priority::HIGH,
        "MONITOR" => enums::event_priority::MONITOR,
        _ => enums::event_priority::HIGHEST,
    };
    // Entry checks: access, player limit, blacklist, entry fee.
    plugin.register_event_with(teleport_priority, true, |e: &mut PlayerTeleportEvent| {
        if !config::b("teleport.teleport-intercept", true) {
            return;
        }
        let from = e.get_from().world.get_name().to_owned();
        let to = e.get_to().world.get_name().to_owned();
        if from == to {
            return;
        }
        let Some(w) = worlds::find(&to) else { return };
        let p = e.get_player().clone();
        if let Err(reason) = dest::can_enter(&p, Some(&from), &w) {
            e.set_cancelled(true);
            util::tell(&CommandSender::Player(p), &reason);
        }
    });
    plugin.register_event(|e: &mut PlayerChangedWorldEvent| {
        let Some(p) = e.get_player() else { return };
        note_world(&p);
        let Some(w) = worlds::find(p.get_world().get_name()) else { return };
        apply_player(&p, &w);
        dest::pay_entry(&p, &w);
    });
    plugin.register_event(|e: &mut PlayerJoinEvent| {
        let p = e.get_player().clone();
        note_world(&p);
        let first = !p.has_played_before();
        horizon_bukkit::scheduler::run_task(move || on_join(&p, first));
    });
    plugin.register_event(|e: &mut PlayerQuitEvent| {
        let uuid = e.get_player().get_unique_id();
        if let Some(map) = CHAT_WORLD.write().unwrap_or_else(|e| e.into_inner()).as_mut() {
            map.remove(&uuid);
        }
    });
    plugin.register_event_with(enums::event_priority::LOW, false, |e: &mut PlayerRespawnEvent| {
        let p = e.get_player().clone();
        let Some(w) = worlds::find(p.get_world().get_name()) else { return };
        if (e.is_bed_spawn() && w.b("bed-respawn", true)) || (e.is_anchor_spawn() && w.b("anchor-respawn", true)) {
            return;
        }
        let respawn = w.s("respawn-world", "");
        let target = if !respawn.is_empty() {
            worlds::find(&respawn).filter(worlds::MvWorld::is_loaded)
        } else if config::b("spawn.default-respawn-in-overworld", true) && w.environment() != "NORMAL" {
            worlds::find(&overworld_of(&w.name)).filter(worlds::MvWorld::is_loaded)
        } else if config::b("spawn.default-respawn-within-same-world", true) {
            Some(w)
        } else {
            None
        };
        if let Some(at) = target.and_then(|t| t.spawn()) {
            e.set_respawn_location(&at);
        }
    });
    plugin.register_event_with(enums::event_priority::NORMAL, true, |e: &mut FoodLevelChangeEvent| {
        let Some(p) = e.get_entity().and_then(|a| a.as_player().cloned()) else { return };
        if e.get_food_level() < p.get_food_level() && worlds::peek(p.get_world().get_name(), |w| !w.b("hunger", true)).unwrap_or(false) {
            e.set_cancelled(true);
        }
    });
    plugin.register_event_with(enums::event_priority::NORMAL, true, |e: &mut WeatherChangeEvent| {
        if e.to_weather_state() && worlds::peek(e.get_world().get_name(), |w| !w.b("allow-weather", true)).unwrap_or(false) {
            e.set_cancelled(true);
        }
    });
    plugin.register_event_with(enums::event_priority::NORMAL, true, |e: &mut EntityRegainHealthEvent| {
        if !matches!(e.get_regain_reason(), "SATIATED" | "REGEN") {
            return;
        }
        let Some(p) = e.get_entity().and_then(|a| a.as_player().cloned()) else { return };
        if worlds::peek(p.get_world().get_name(), |w| !w.b("auto-heal", true)).unwrap_or(false) {
            e.set_cancelled(true);
        }
    });
    plugin.register_event_with(enums::event_priority::NORMAL, true, |e: &mut PortalCreateEvent| {
        let Some(w) = worlds::find(e.get_world().get_name()) else { return };
        let form = w.s("portal-form", "ALL").to_ascii_uppercase();
        let end = e.get_reason().eq_ignore_ascii_case("END_PLATFORM");
        let blocked = match form.as_str() {
            "NONE" => true,
            "NETHER" => end,
            "END" => !end,
            _ => false,
        };
        if blocked {
            e.set_cancelled(true);
            if let Some(p) = e.get_entity().and_then(|en| horizon_bukkit::get_online_players().into_iter().find(|p| p.get_unique_id() == en.get_unique_id())) {
                p.send_message(&m(if end { "portalform.disabled.end" } else { "portalform.disabled.nether" }, &[]));
            }
        }
    });
    plugin.register_event_with(enums::event_priority::NORMAL, true, |e: &mut AsyncPlayerChatEvent| {
        if !config::b("messaging.enable-chat-prefix", false) {
            return;
        }
        let uuid = e.get_player().get_unique_id();
        let Some(alias) = CHAT_WORLD.read().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&uuid).cloned()) else { return };
        let fmt = config::s("messaging.chat-prefix-format", "[%world%]%chat%");
        let new = fmt.replace("%world%", &format!("{alias}§r")).replace("%chat%", e.get_format());
        e.set_format(&new);
    });
}

fn on_join(p: &Player, first: bool) {
    if first && config::b("spawn.first-spawn-override", false) {
        let input = config::s("spawn.first-spawn-location", "");
        let done = if input.is_empty() { default_spawn().map(|at| dest::teleport_to(p, &at, true)) } else { dest::parse(&input).ok().map(|d| dest::teleport(p, &d, true)) };
        if let Some(Err(e)) = done {
            log(&format!("Could not send {} to the first spawn: {}", p.get_name(), horizon_kit::text::strip(&e)));
        }
    } else if config::b("spawn.enable-join-destination", false) && !p.has_permission("mv.bypass.joinlocation") {
        let input = config::s("spawn.join-destination", "");
        if let Ok(d) = dest::parse(&input) {
            let _ = dest::teleport(p, &d, true);
        }
    }
    let Some(w) = worlds::find(p.get_world().get_name()) else { return };
    if dest::can_stay(p, &w).is_err()
        && let Some(at) = default_spawn()
    {
        let _ = dest::teleport_to(p, &at, true);
        return;
    }
    apply_player(p, &w);
}

/// `%multiverse-core_<what>[_<world>]%`, as the Java plugin's PlaceholderAPI expansion.
fn placeholder(name: &str, params: &str) -> Option<String> {
    let (what, world) = match params.split_once('_') {
        Some((w, rest)) if worlds::find(rest).is_some() => (w.to_ascii_lowercase(), worlds::find(rest)),
        _ => (params.to_ascii_lowercase(), horizon_bukkit::get_player(name).and_then(|p| worlds::find(p.get_world().get_name()))),
    };
    let w = world?;
    let world = w.world();
    Some(match what.as_str() {
        "alias" => w.alias_or_name(),
        "name" => w.name.clone(),
        "animalspawn" => w.get("spawning.animal.spawn").and_then(serde_yaml::Value::as_bool).unwrap_or(true).to_string(),
        "monsterspawn" => w.get("spawning.monster.spawn").and_then(serde_yaml::Value::as_bool).unwrap_or(true).to_string(),
        "autoheal" => w.b("auto-heal", true).to_string(),
        "blacklist" => w.list("world-blacklist").join(", "),
        "currency" => w.s("entry-fee.currency", "@vault-economy"),
        "difficulty" => w.s("difficulty", "NORMAL").to_ascii_lowercase(),
        "entryfee" | "price" => w.f("entry-fee.amount", 0.0).to_string(),
        "environment" => w.environment().to_ascii_lowercase(),
        "flying" => w.b("allow-flight", false).to_string(),
        "gamemode" => w.gamemode().to_ascii_lowercase(),
        "generator" => w.generator(),
        "hunger" => w.b("hunger", true).to_string(),
        "playerlimit" => w.player_limit().to_string(),
        "pvp" => w.b("pvp", true).to_string(),
        "seed" => world.as_ref().map_or_else(|| w.seed().unwrap_or(0).to_string(), |x| x.get_seed().to_string()),
        "time" => world.as_ref().map_or(0, |x| x.get_time()).to_string(),
        "type" => world.as_ref().map_or("NORMAL", |x| x.get_world_type()).to_ascii_lowercase(),
        "weather" => if world.as_ref().is_some_and(|x| x.has_storm()) { "stormy".into() } else { "clear".into() },
        "playercount" => world.as_ref().map_or(0, |x| x.get_players().len()).to_string(),
        _ => return None,
    })
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    util::start_writer();
    let folder = plugin.get_data_folder();
    config::load(&folder);
    util::load_messages(&folder, &config::s("messaging.default-locale", "en"));
    dest::load_anchors(&folder);
    worlds::load_file(&folder);
    worlds::load_all();
    commands::register(&plugin);
    register_events(&plugin);
    if config::b("messaging.register-papi-hook", true) {
        horizon_kit::hooks::provide_placeholders(&plugin, "multiverse-core", |_uuid, name, params| placeholder(name, params));
    }
    for p in get_online_players() {
        note_world(&p);
    }
    if !config::b("misc.silent-start", false) {
        log("Version 5.8.2 (Rust) Enabled - By dumptruckman, Rigby, fernferret, lithium3141, main--, benwoo1110 and Zax71");
    }
}

fn on_disable(_plugin: Plugin) {
    worlds::save();
    util::stop_writer();
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
