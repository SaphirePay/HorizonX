//! BetterRTP 3.6.13 rewritten in Rust for Horizon: same plugin name, data folder, config.yml, economy.yml,
//! effects.yml, locations.yml, signs.yml, placeholders.yml, lang files, data/database.db, commands and permissions.
//!
//! Performance: nothing per tick. A teleport's chunk loads happen off the tick (the engine loads it; the server
//! thread only checks it arrived), the safety check is a few block reads (one chunk snapshot in the nether), and the
//! SQLite cooldown database and log.txt are written on threads of their own.

pub mod commands;
pub mod db;
pub mod effects;
pub mod rtp;
pub mod settings;
pub mod text;

use horizon_bukkit::prelude::*;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, RwLock};

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static SETTINGS: RwLock<Option<Arc<settings::Settings>>> = RwLock::new(None);

pub fn folder() -> Option<std::path::PathBuf> {
    PLUGIN.lock().unwrap_or_else(|e| e.into_inner()).map(|p| p.get_data_folder())
}

pub fn settings() -> Arc<settings::Settings> {
    if let Some(s) = SETTINGS.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return s.clone();
    }
    let s = Arc::new(settings::Settings::load(&folder().unwrap_or_else(std::env::temp_dir)));
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
    s
}

/// Every file read again (`/rtp reload`, an edit).
pub fn reload() {
    let Some(folder) = folder() else { return };
    let s = Arc::new(settings::Settings::load(&folder));
    let worlds: Vec<String> = if s.cooldown_enabled && s.per_world { get_worlds().into_iter().map(|w| w.get_name().to_owned()).filter(|w| !s.disabled.contains(w)).collect() } else { Vec::new() };
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(s);
    if !worlds.is_empty() {
        db::send(db::Job::Tables(worlds));
    }
}

pub fn log(text: &str) {
    horizon_bukkit::horizon_mod::log(&format!("[BetterRTP] {text}"));
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

/// Message_RTP.sms: the prefix, placeholders, colours.
pub fn sms(to: &CommandSender, text: &str) {
    if text.is_empty() {
        return;
    }
    let s = settings();
    let prefix = s.lang.str("Messages.Prefix", "");
    let full = format!("{prefix}{text}");
    let full = match to {
        CommandSender::Player(p) => horizon_kit::hooks::placeholders(&full, Some(p)),
        _ => full,
    };
    for line in full.split('\n') {
        to.send_message(&horizon_kit::text::colorize(line));
    }
}

fn fill(text: &str, pairs: &[(&str, &str)]) -> String {
    let mut t = text.to_owned();
    for (k, v) in pairs {
        t = t.replace(&format!("%{k}%"), v);
    }
    t
}

/// `Messages.<key>` with `%name%` placeholders, prefixed.
pub fn send(to: &CommandSender, key: &str, pairs: &[(&str, &str)]) {
    let s = settings();
    let raw = s.lang.str(&format!("Messages.{key}"), "");
    sms(to, &fill(&raw, pairs));
}

/// A key outside `Messages.` (usage lines), prefixed.
pub fn send_raw(to: &CommandSender, key: &str, pairs: &[(&str, &str)]) {
    let s = settings();
    let raw = s.lang.str(key, "");
    sms(to, &fill(&raw, pairs));
}

// ---- log.txt (Settings.Logger) ----

static LOGGER: Mutex<Option<Sender<String>>> = Mutex::new(None);

fn start_logger(path: std::path::PathBuf) {
    let (tx, rx) = channel::<String>();
    let _ = std::thread::Builder::new().name("BetterRTP logger".into()).spawn(move || {
        use std::io::Write;
        while let Ok(first) = rx.recv() {
            let mut lines = vec![first];
            lines.extend(rx.try_iter());
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                for l in lines {
                    let _ = writeln!(f, "{l}");
                }
            }
        }
    });
    *LOGGER.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
}

fn stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

pub fn log_rtp(player: &str, at: &Location, attempts: u32) {
    let s = settings();
    if !s.config.bool("Settings.Logger.Enabled", true) {
        return;
    }
    let line = format!("[{}] {player} rtp'd to {} x={} y={} z={} ({attempts} attempts)", stamp(), at.world.get_name(), at.x.floor(), at.y.floor(), at.z.floor());
    if s.config.bool("Settings.Logger.LogToConsole", false) {
        log(&line);
    }
    if let Some(tx) = LOGGER.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(line);
    }
}

// ---- Events ----

fn register_events(plugin: &Plugin) {
    use std::sync::atomic::Ordering;
    plugin.register_event(|e: &mut PlayerMoveEvent| {
        if !rtp::ANY_DELAY.load(Ordering::Acquire) || !e.has_changed_block() {
            return;
        }
        let p = e.get_player().clone();
        if settings().cancel_on_move && rtp::in_delay(&p.get_unique_id()) {
            rtp::cancel_delay(&p, true);
        }
    });
    plugin.register_event_with(enums::event_priority::NORMAL, true, |e: &mut EntityDamageEvent| {
        let delays = rtp::ANY_DELAY.load(Ordering::Acquire);
        let invincible = rtp::ANY_INVINCIBLE.load(Ordering::Acquire);
        if !delays && !invincible {
            return;
        }
        let Some(en) = e.get_entity() else { return };
        let uuid = en.get_unique_id();
        if invincible && rtp::is_invincible(&uuid) {
            e.set_cancelled(true);
            return;
        }
        if delays
            && settings().cancel_on_damage
            && rtp::in_delay(&uuid)
            && let Some(p) = get_online_players().into_iter().find(|p| p.get_unique_id() == uuid)
        {
            rtp::cancel_delay(&p, true);
        }
    });
    plugin.register_event(|e: &mut PlayerJoinEvent| {
        let p = e.get_player().clone();
        let s = settings();
        let worlds: Vec<String> = if s.per_world { get_worlds().into_iter().map(|w| w.get_name().to_owned()).collect() } else { Vec::new() };
        db::send(db::Job::Load(p.get_unique_id(), worlds));
        if let Some((world, _)) = s.first_join.clone()
            && !p.has_played_before()
        {
            horizon_bukkit::scheduler::run_task_later(5, move || {
                if horizon_bukkit::get_world(&world).is_some() {
                    rtp::tp(&p, &horizon_bukkit::get_console_sender(), Some(world.clone()), None, rtp::RtpType::Join, None, rtp::Info::new(false, false, false));
                }
            });
        }
    });
    plugin.register_event(|e: &mut PlayerQuitEvent| {
        let uuid = e.get_player().get_unique_id();
        rtp::forget(&uuid);
        db::forget(&uuid);
    });
    plugin.register_event(|e: &mut PlayerRespawnEvent| {
        let p = e.get_player().clone();
        let s = settings();
        let world = e.get_respawn_location().world.get_name().to_owned();
        let t = rtp::target_for(&s, Some(&p), &CommandSender::Player(p.clone()), &world, None, None);
        if t.s.rtp_on_death {
            horizon_bukkit::scheduler::run_task_later(2, move || {
                rtp::tp(&p, &horizon_bukkit::get_console_sender(), Some(world.clone()), None, rtp::RtpType::Respawn, None, rtp::Info::new(false, false, false));
            });
        }
    });
    plugin.register_event_with(enums::event_priority::NORMAL, true, |e: &mut SignChangeEvent| {
        let s = settings();
        if !s.signs_enabled {
            return;
        }
        let Some(p) = e.get_player() else { return };
        if !rtp::has_p(&p, "betterrtp.sign") {
            return;
        }
        let first = e.get_line(0).unwrap_or("").to_owned();
        if first.eq_ignore_ascii_case(&s.sign_title) || first.eq_ignore_ascii_case("[RTP]") {
            let colored = horizon_kit::text::colorize(&s.sign_title);
            e.set_line(0, &colored);
            let cmd = sign_command(&e.get_lines()[1..]);
            send(&CommandSender::Player(p), "Sign", &[("command", &cmd)]);
        }
    });
    plugin.register_event(|e: &mut PlayerInteractEvent| {
        let s = settings();
        if !s.signs_enabled || e.get_action() != "RIGHT_CLICK_BLOCK" {
            return;
        }
        let Some(block) = e.get_clicked_block() else { return };
        if !block.get_type().name().ends_with("SIGN") {
            return;
        }
        let sign = horizon_bukkit::blocks::Sign(block.tile());
        let lines = sign.get_lines();
        if lines.first().map(String::as_str) != Some(horizon_kit::text::colorize(&s.sign_title).as_str()) {
            return;
        }
        let cmd = sign_command(&lines[1..]);
        let p = e.get_player().clone();
        let words: Vec<&str> = cmd.split_whitespace().collect();
        let sender = CommandSender::Player(p);
        if words.is_empty() || words[0].eq_ignore_ascii_case("rtp") {
            commands::run(&sender, "rtp", &[]);
        } else {
            commands::run(&sender, "rtp", &words);
        }
    });
}

fn sign_command(lines: &[String]) -> String {
    lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

// ---- %betterrtp_...% ----

fn placeholder(name: &str, params: &str) -> Option<String> {
    let p = horizon_bukkit::get_player(name)?;
    let s = settings();
    let req = params.to_ascii_lowercase();
    let (what, world) = ["canrtpcooldown", "canrtpprice", "canrtphunger", "cooldowntime", "cooldown", "canrtp", "price", "count"]
        .iter()
        .find(|w| req.starts_with(*w))
        .map(|w| (*w, params.get(w.len() + 1..).filter(|r| !r.is_empty() && params.as_bytes().get(w.len()) == Some(&b'_')).map(str::to_owned)))?;
    let world = world.unwrap_or_else(|| p.get_world().get_name().to_owned());
    let sender = CommandSender::Player(p.clone());
    let t = rtp::target_for(&s, Some(&p), &sender, &world, None, None);
    let ph = |k: &str, d: &str| horizon_kit::text::colorize(&s.placeholders.str(&format!("Config.CanRTP.{k}"), d));
    Some(match what {
        "count" => db::get(&p.get_unique_id()).map_or(0, |d| d.count).to_string(),
        "cooldown" => text::duration(rtp::cooldown_left(&p, &t).max(0)),
        "cooldowntime" => text::duration(t.s.cooldown * 1000),
        "price" => t.s.price.to_string(),
        "canrtpcooldown" => if rtp::is_cooling_down(&p, &t) { ph("Cooldown", "&fCooling Down") } else { ph("Success", "&aYes") },
        "canrtpprice" => if rtp::has_balance(&p, &t) { ph("Success", "&aYes") } else { ph("Price", "&cNo") },
        "canrtphunger" => if rtp::has_hunger(&p) { ph("Success", "&aYes") } else { ph("Hunger", "&cNo") },
        _ => {
            if !rtp::world_permission(&sender, &t.world).0 {
                ph("NoPermission", "&cNo Permission")
            } else if rtp::is_cooling_down(&p, &t) {
                ph("Cooldown", "&fCooling Down")
            } else if !rtp::has_balance(&p, &t) {
                ph("Price", "&cNo")
            } else if !rtp::has_hunger(&p) {
                ph("Hunger", "&cNo")
            } else {
                ph("Success", "&aYes")
            }
        }
    })
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    let folder = plugin.get_data_folder();
    db::start(folder.join("data").join("database.db"));
    start_logger(folder.join("log.txt"));
    reload();
    plugin.set_executor("betterrtp", commands::run);
    plugin.set_tab_completer("betterrtp", |s, _, a| commands::complete(s, a));
    register_events(&plugin);
    horizon_kit::hooks::provide_placeholders(&plugin, "betterrtp", |_uuid, name, params| placeholder(name, params));
    let worlds: Vec<String> = if settings().per_world { get_worlds().into_iter().map(|w| w.get_name().to_owned()).collect() } else { Vec::new() };
    for p in get_online_players() {
        db::send(db::Job::Load(p.get_unique_id(), worlds.clone()));
    }
    log("Enabled (Rust)");
}

fn on_disable(_plugin: Plugin) {
    db::stop();
    *LOGGER.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = None;
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
