//! Plan (Player Analytics) 5.8 rewritten in Rust for Horizon: same plugin name, data folder, config.yml,
//! ServerInfoFile.yml, database.db (Plan's tables) and `/plan` commands; the web pages are a light read-only page
//! made here instead of Plan's React app.
//!
//! What it records: sessions (join/leave, AFK, per-world and per-gamemode time, deaths, mob and player kills,
//! ping), new players and names, and once a minute TPS, MSPT, players online, entities and chunks.
//! Performance: the server thread only keeps a few numbers per online player and samples once a minute (one
//! `players` query for every player's place); no move listener (AFK is found from the minute samples, chat and
//! commands); the database is written on its own thread and the web server reads on its own threads.

pub mod db;
pub mod web;

use horizon_bukkit::prelude::*;
use horizon_kit::{Config, Ctx, Res, Router, Sub};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, RwLock};

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static CONFIG: RwLock<Option<Config>> = RwLock::new(None);
const DEFAULT_CONFIG: &str = include_str!("../data/assets/plan/config.yml");

pub fn log(text: &str) {
    horizon_bukkit::horizon_mod::log(&format!("[Plan] {text}"));
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn on_main(f: impl FnOnce() + Send + 'static) {
    let mut f = Some(f);
    horizon_bukkit::scheduler::run_task(move || {
        if let Some(f) = f.take() {
            f();
        }
    });
}

fn cfg<R>(f: impl FnOnce(&Config) -> R) -> R {
    let held = CONFIG.read().unwrap_or_else(|e| e.into_inner());
    match held.as_ref() {
        Some(c) => f(c),
        None => f(&Config::parse(DEFAULT_CONFIG)),
    }
}

pub fn server_name() -> String {
    cfg(|c| c.str("Server.ServerName", "Plan"))
}

/// A `Time:`/`Unit:` pair as milliseconds.
fn duration_ms(path: &str, def_ms: i64) -> i64 {
    cfg(|c| {
        let t = c.int(&format!("{path}.Time"), -1);
        if t < 0 {
            return def_ms;
        }
        let unit = c.str(&format!("{path}.Unit"), "MILLISECONDS").to_ascii_uppercase();
        t * match unit.as_str() {
            "SECONDS" => 1000,
            "MINUTES" => 60_000,
            "HOURS" => 3_600_000,
            "DAYS" => 86_400_000,
            _ => 1,
        }
    })
}

/// Formatting.Dates.TimeZone: 'GMT+2' / 'GMT-04:30' (server and UTC: 0).
pub fn tz_offset_ms() -> i64 {
    let tz = cfg(|c| c.str("Formatting.Dates.TimeZone", "server"));
    let Some(rest) = tz.strip_prefix("GMT").or_else(|| tz.strip_prefix("UTC")) else { return 0 };
    let sign = if rest.starts_with('-') { -1 } else { 1 };
    let rest = rest.trim_start_matches(['+', '-']);
    let (h, m) = rest.split_once(':').map_or((rest, "0"), |(h, m)| (h, m));
    sign * (h.parse::<i64>().unwrap_or(0) * 3_600_000 + m.parse::<i64>().unwrap_or(0) * 60_000)
}

fn address() -> String {
    let port = cfg(|c| c.int("Webserver.Port", 8804));
    if cfg(|c| c.bool("Webserver.Alternative_IP.Enabled", false)) {
        let a = cfg(|c| c.str("Webserver.Alternative_IP.Address", "your.domain.here:%port%")).replace("%port%", &port.to_string());
        return if a.starts_with("http") { a } else { format!("http://{a}") };
    }
    let props = std::fs::read_to_string(horizon_bukkit::server::server_folder().join("server.properties")).unwrap_or_default();
    let ip = props.lines().find_map(|l| l.strip_prefix("server-ip=")).map(str::trim).filter(|s| !s.is_empty()).unwrap_or("localhost").to_owned();
    format!("http://{ip}:{port}")
}

// ---- Sessions in memory ----

struct Live {
    uuid: String,
    name: String,
    start: i64,
    world: String,
    mode: usize,
    segment: i64,
    worlds: HashMap<String, [i64; 4]>,
    deaths: i64,
    mob_kills: i64,
    kills: Vec<(String, String, i64)>,
    afk: i64,
    last_active: i64,
    last_pos: [f64; 5],
    ping: Option<(i32, i32, i64, i64)>,
}

static LIVE: Mutex<Option<HashMap<String, Live>>> = Mutex::new(None);
/// Totals from the database for online players (placeholders): playtime, afk, sessions, kills, deaths, mob kills, registered.
static TOTALS: Mutex<Option<HashMap<String, [i64; 7]>>> = Mutex::new(None);

fn live_map() -> std::sync::MutexGuard<'static, Option<HashMap<String, Live>>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

fn afk_threshold() -> i64 {
    duration_ms("Time.Thresholds.AFK_threshold", 180_000)
}

fn mode_index(mode: &str) -> usize {
    match mode.to_ascii_uppercase().as_str() {
        "CREATIVE" => 1,
        "ADVENTURE" => 2,
        "SPECTATOR" => 3,
        _ => 0,
    }
}

impl Live {
    fn close_segment(&mut self, at: i64) {
        let e = self.worlds.entry(self.world.clone()).or_insert([0; 4]);
        e[self.mode] += (at - self.segment).max(0);
        self.segment = at;
    }
    fn active(&mut self, at: i64, threshold: i64) {
        if at - self.last_active >= threshold {
            self.afk += at - self.last_active;
        }
        self.last_active = at;
    }
    fn afk_now(&self, at: i64, threshold: i64) -> i64 {
        self.afk + if at - self.last_active >= threshold { at - self.last_active } else { 0 }
    }
}

/// The sessions going on: (uuid, name, start, afk so far), for the pages (plain data: any thread).
pub fn live() -> Vec<(String, String, i64, i64)> {
    let t = now();
    let th = afk_threshold();
    live_map().as_ref().map(|m| m.values().map(|l| (l.uuid.clone(), l.name.clone(), l.start, l.afk_now(t, th))).collect()).unwrap_or_default()
}

fn dashed(uuid: &str) -> String {
    let h: String = uuid.chars().filter(|c| *c != '-').collect();
    if h.len() != 32 {
        return uuid.to_owned();
    }
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn start_session(p: &Player) {
    let t = now();
    let uuid = dashed(&p.get_unique_id());
    let world = p.get_world().get_name().to_owned();
    let l = p.get_location();
    let live = Live { uuid: uuid.clone(), name: p.get_name().to_owned(), start: t, world, mode: mode_index(p.get_game_mode()), segment: t, worlds: HashMap::new(), deaths: 0, mob_kills: 0, kills: Vec::new(), afk: 0, last_active: t, last_pos: [l.x, l.y, l.z, l.yaw as f64, l.pitch as f64], ping: None };
    live_map().get_or_insert_with(HashMap::new).insert(uuid.clone(), live);
    db::send(db::Job::Register(uuid.clone(), p.get_name().to_owned(), t, p.is_op()));
    // Totals for the placeholders, read off the server thread.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Some(t) = db::totals(&uuid) {
            TOTALS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(uuid, t);
        }
    });
}

fn end_session(uuid: &str) {
    let t = now();
    let th = afk_threshold();
    let Some(mut l) = live_map().as_mut().and_then(|m| m.remove(uuid)) else { return };
    l.close_segment(t);
    l.active(t, th);
    let ping = l.ping.map(|(min, max, sum, n)| (min, max, sum as f64 / n.max(1) as f64));
    if let Some(totals) = TOTALS.lock().unwrap_or_else(|e| e.into_inner()).as_mut()
        && let Some(x) = totals.get_mut(uuid)
    {
        x[0] += t - l.start;
        x[1] += l.afk;
        x[2] += 1;
        x[3] += l.kills.len() as i64;
        x[4] += l.deaths;
        x[5] += l.mob_kills;
    }
    db::send(db::Job::Session(Box::new(db::Session { uuid: l.uuid, name: l.name, start: l.start, end: t, deaths: l.deaths, mob_kills: l.mob_kills, afk: l.afk, worlds: l.worlds, kills: l.kills, ping })));
}

fn with_live(uuid: &str, f: impl FnOnce(&mut Live)) {
    if let Some(l) = live_map().as_mut().and_then(|m| m.get_mut(&dashed(uuid))) {
        f(l);
    }
}

/// The minute sample: TPS, MSPT, players, entities, chunks; and each player's place (AFK) and ping.
fn sample() {
    let t = now();
    let th = afk_threshold();
    let answer = horizon_bukkit::horizon_mod::query(json!({ "op": "players" }));
    let rows = answer.as_array().cloned().unwrap_or_default();
    let ping_on = cfg(|c| c.bool("Data_gathering.Ping", true));
    {
        let mut held = live_map();
        let map = held.get_or_insert_with(HashMap::new);
        for p in &rows {
            let Some(uuid) = p["uuid"].as_str() else { continue };
            let Some(l) = map.get_mut(&dashed(uuid)) else { continue };
            let pos = [p["position"][0].as_f64().unwrap_or(0.0), p["position"][1].as_f64().unwrap_or(0.0), p["position"][2].as_f64().unwrap_or(0.0), p["yaw"].as_f64().unwrap_or(0.0), p["pitch"].as_f64().unwrap_or(0.0)];
            let moved = pos.iter().zip(l.last_pos.iter()).any(|(a, b)| (a - b).abs() > 0.01);
            if moved {
                l.active(t, th);
                l.last_pos = pos;
            }
        }
    }
    if ping_on {
        for p in get_online_players() {
            let ping = p.get_ping();
            with_live(&p.get_unique_id(), |l| {
                l.ping = Some(match l.ping {
                    Some((min, max, sum, n)) => (min.min(ping), max.max(ping), sum + ping as i64, n + 1),
                    None => (ping, ping, ping as i64, 1),
                });
            });
        }
    }
    let worlds = get_worlds();
    let entities: i64 = worlds.iter().map(|w| w.get_entity_count() as i64).sum();
    let chunks: i64 = worlds.iter().map(|w| w.get_chunk_count() as i64).sum();
    let tps = horizon_bukkit::server::get_tps()[0].min(20.0);
    let mspt = horizon_bukkit::server::get_average_tick_time();
    db::send(db::Job::Tps(db::Tps { date: t, tps, players: rows.len() as i64, entities, chunks, mspt }));
}

fn register_events(plugin: &Plugin) {
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut PlayerJoinEvent| start_session(e.get_player()));
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut PlayerQuitEvent| {
        let uuid = dashed(&e.get_player().get_unique_id());
        end_session(&uuid);
        if let Some(m) = TOTALS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            m.remove(&uuid);
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerKickEvent| {
        let Some(p) = e.get_player() else { return };
        db::send(db::Job::Kicked(dashed(&p.get_unique_id())));
    });
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut PlayerChangedWorldEvent| {
        let Some(p) = e.get_player() else { return };
        let world = p.get_world().get_name().to_owned();
        let t = now();
        let th = afk_threshold();
        with_live(&p.get_unique_id(), |l| {
            l.close_segment(t);
            l.world = world;
            l.active(t, th);
        });
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerGameModeChangeEvent| {
        let mode = mode_index(&e.get_new_game_mode());
        let t = now();
        let Some(p) = e.get_player() else { return };
        with_live(&p.get_unique_id(), |l| {
            l.close_segment(t);
            l.mode = mode;
        });
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut AsyncPlayerChatEvent| {
        let t = now();
        let th = afk_threshold();
        with_live(&e.get_player().get_unique_id(), |l| l.active(t, th));
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerCommandPreprocessEvent| {
        let p = e.get_player();
        if e.get_message().to_ascii_lowercase().starts_with("/afk") {
            return;
        }
        let t = now();
        let th = afk_threshold();
        with_live(&p.get_unique_id(), |l| l.active(t, th));
    });
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut PlayerDeathEvent| {
        with_live(&e.get_player().get_unique_id(), |l| l.deaths += 1);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut EntityDeathEvent| {
        let Some(killer) = e.get_killer().and_then(|a| a.as_player().cloned()) else { return };
        let kind = e.get_entity_type().trim_start_matches("minecraft:").to_ascii_lowercase();
        if kind == "player" {
            let victim = dashed(&e.get_entity().get_unique_id());
            let weapon = killer.get_inventory().get_item_in_main_hand().map_or_else(|| "AIR".to_owned(), |i| i.get_type().name());
            let t = now();
            with_live(&killer.get_unique_id(), |l| l.kills.push((victim, weapon, t)));
        } else {
            with_live(&killer.get_unique_id(), |l| l.mob_kills += 1);
        }
    });
}

// ---- %plan_...% ----

fn fmt_time(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (d, h, m, sec) = (s / 86400, s % 86400 / 3600, s % 3600 / 60, s % 60);
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m {sec}s")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}

fn placeholder(uuid: &str, params: &str) -> Option<String> {
    let key = params.to_ascii_lowercase();
    match key.as_str() {
        "server_name" => return Some(server_name()),
        "server_uuid" => return Some(db::SERVER_UUID.read().unwrap_or_else(|e| e.into_inner()).clone()),
        "server_online" | "server_players_online" => return Some(live().len().to_string()),
        "server_players_registered_total" => return Some(db::registered_count().to_string()),
        _ => {}
    }
    let uuid = dashed(uuid);
    let t = now();
    let th = afk_threshold();
    let base = TOTALS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&uuid).copied()).unwrap_or([0; 7]);
    let (cur, cur_afk, cur_kills, cur_deaths, cur_mobs) = live_map().as_ref().and_then(|m| m.get(&uuid)).map_or((0, 0, 0, 0, 0), |l| (t - l.start, l.afk_now(t, th), l.kills.len() as i64, l.deaths, l.mob_kills));
    let total = base[0] + cur;
    let afk = base[1] + cur_afk;
    Some(match key.as_str() {
        "player_time_total" | "player_playtime" => fmt_time(total),
        "player_time_active" => fmt_time(total - afk),
        "player_time_afk" => fmt_time(afk),
        "player_time_total_raw" => (total / 1000).to_string(),
        "player_time_active_raw" => ((total - afk) / 1000).to_string(),
        "player_sessions_count" | "player_session_count" => (base[2] + i64::from(cur > 0)).to_string(),
        "player_kill_count" | "player_player_kill_count" => (base[3] + cur_kills).to_string(),
        "player_death_count" => (base[4] + cur_deaths).to_string(),
        "player_mob_kill_count" => (base[5] + cur_mobs).to_string(),
        "player_current_session_length" => fmt_time(cur),
        _ => return None,
    })
}

// ---- /plan ----

fn tell_later(sender: &CommandSender, lines: Vec<String>) {
    let s = sender.clone();
    on_main(move || {
        for l in lines {
            s.send_message(&horizon_kit::text::colorize(&l));
        }
    });
}

fn bare(c: &Ctx) -> Res {
    c.tell("&2> &2Player Analytics");
    c.tell(&format!("&2  Server page: &f{}", address()));
    c.tell("&7  /plan help for the commands.");
    Ok(())
}

fn server_cmd(c: &Ctx) -> Res {
    c.tell("&2> &2Server");
    c.tell(&format!("&2Server page: &f{}/server", address()));
    let to = c.sender.clone();
    std::thread::spawn(move || {
        let o = db::overview(now());
        tell_later(&to, vec![
            format!("&2  Players: &f{} &7(+{} this week)", o["players"], o["new_week"]),
            format!("&2  Unique this week: &f{}", o["unique_week"]),
            format!("&2  Playtime: &f{}", fmt_time(o["playtime"].as_i64().unwrap_or(0))),
            format!("&2  Average TPS (7 days): &f{:.2}", o["tps_week"].as_f64().unwrap_or(0.0)),
        ]);
    });
    Ok(())
}

fn players_cmd(c: &Ctx) -> Res {
    c.tell("&2> &2Players");
    c.tell(&format!("&2Players page: &f{}/players", address()));
    Ok(())
}

fn player_cmd(c: &Ctx) -> Res {
    let name = match c.arg(0) {
        Some(n) => {
            if !n.eq_ignore_ascii_case(c.sender.get_name()) && !c.sender.has_permission("plan.player.other") {
                return Err("&cYou don't have the permission (plan.player.other).".into());
            }
            n.to_owned()
        }
        None => c.player()?.get_name().to_owned(),
    };
    c.tell(&format!("&2Player page: &f{}/player/{}", address(), name));
    Ok(())
}

fn ingame_cmd(c: &Ctx) -> Res {
    let name = match c.arg(0) {
        Some(n) => {
            if !n.eq_ignore_ascii_case(c.sender.get_name()) && !c.sender.has_permission("plan.ingame.other") {
                return Err("&cYou don't have the permission (plan.ingame.other).".into());
            }
            n.to_owned()
        }
        None => c.player()?.get_name().to_owned(),
    };
    let to = c.sender.clone();
    std::thread::spawn(move || {
        let p = db::player(&name);
        if p.is_null() {
            return tell_later(&to, vec![format!("&cPlayer '{name}' was not found.")]);
        }
        tell_later(&to, vec![
            format!("&2> &2Player: &f{}", p["name"].as_str().unwrap_or(&name)),
            format!("&2  Registered: &f{}", fmt_date(p["registered"].as_i64().unwrap_or(0))),
            format!("&2  Last Seen: &f{}", fmt_date(p["last_seen"].as_i64().unwrap_or(0))),
            format!("&2  Playtime: &f{}", fmt_time(p["playtime"].as_i64().unwrap_or(0))),
            format!("&2  Active: &f{}", fmt_time(p["playtime"].as_i64().unwrap_or(0) - p["afk"].as_i64().unwrap_or(0))),
            format!("&2  Sessions: &f{}", p["sessions_count"]),
            format!("&2  Player Kills: &f{}", p["player_kills"]),
            format!("&2  Mob Kills: &f{}", p["mob_kills"]),
            format!("&2  Deaths: &f{}", p["deaths"]),
            format!("&2  Times Kicked: &f{}", p["times_kicked"]),
        ]);
    });
    Ok(())
}

fn fmt_date(ms: i64) -> String {
    if ms <= 0 {
        return "-".into();
    }
    let ms = ms + tz_offset_ms();
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {d} {y}, {:02}:{:02}", MONTHS[(m - 1) as usize], rem / 3600, rem / 60 % 60)
}

fn search_cmd(c: &Ctx) -> Res {
    let text = c.rest(0);
    if text.trim().is_empty() {
        return Err("&cThe search string can not be empty".into());
    }
    let to = c.sender.clone();
    std::thread::spawn(move || {
        let rows = db::players(&text, 20);
        let names: Vec<String> = rows.as_array().into_iter().flatten().filter_map(|r| r["name"].as_str().map(str::to_owned)).collect();
        let mut lines = vec![format!("&2> &2{} Results for &f{}&2:", names.len(), text)];
        lines.push(format!("&f  {}", names.join(", ")));
        tell_later(&to, lines);
    });
    Ok(())
}

fn info_cmd(c: &Ctx) -> Res {
    c.tell("&2> &2Player Analytics");
    c.tell("&2  Version: &f5.8 build 3638 (Rust, Horizon)");
    c.tell(&format!("&2  Webserver: &f{}", if web_running() { address() } else { "off".into() }));
    c.tell("&2  Database: &fSQLite (database.db)");
    c.tell(&format!("&2  Sessions now: &f{}", live().len()));
    Ok(())
}

fn reload_cmd(c: &Ctx) -> Res {
    stop_all();
    start_all();
    c.tell("&aReload Complete");
    Ok(())
}

fn register_cmd(c: &Ctx) -> Res {
    c.tell("&eThis Plan's pages are read-only and need no web user: open them at");
    c.tell(&format!("&f{}", address()));
    Ok(())
}

fn servers_cmd(c: &Ctx) -> Res {
    c.tell("&2> &2Servers");
    c.tell("&7id::name::uuid::version");
    c.tell(&format!("&f1::{}::{}::5.8 build 3638", server_name(), db::SERVER_UUID.read().unwrap_or_else(|e| e.into_inner())));
    Ok(())
}

fn network_cmd(c: &Ctx) -> Res {
    c.tell("&eServer is not connected to a network. The link redirects to server page.");
    c.tell(&format!("&2Server page: &f{}", address()));
    Ok(())
}

fn help_cmd(c: &Ctx) -> Res {
    PLAN.help(c.sender, c.label);
    Ok(())
}

fn tab_players(_: &Ctx) -> Vec<String> {
    get_online_players().iter().map(|p| p.get_name().to_owned()).collect()
}

static PLAN: LazyLock<Router> = LazyLock::new(|| {
    Router::new("plan")
        .bare(bare)
        .sub(Sub::new("help", help_cmd).about("Show command help"))
        .sub(Sub::new("server", server_cmd).alias("analyze").alias("analyse").alias("analysis").alias("a").perm("plan.server").usage("server [server]").about("View the server page"))
        .sub(Sub::new("servers", servers_cmd).alias("serverlist").alias("listservers").alias("sl").alias("ls").perm("plan.servers").about("List servers in the database"))
        .sub(Sub::new("network", network_cmd).alias("netw").perm("plan.network").about("View the network page"))
        .sub(Sub::new("player", player_cmd).alias("inspect").perm("plan.player.self").usage("player [name]").about("View a player page").tab(tab_players))
        .sub(Sub::new("players", players_cmd).alias("pl").alias("playerlist").alias("list").perm("plan.player.other").about("View the players page"))
        .sub(Sub::new("search", search_cmd).perm("plan.search").args(1).usage("search <name>").about("Search for a player name"))
        .sub(Sub::new("ingame", ingame_cmd).alias("qinspect").perm("plan.ingame.self").usage("ingame [name]").about("View player info in game").tab(tab_players))
        .sub(Sub::new("register", register_cmd).perm("plan.register.self").about("Register a user for the Plan website"))
        .sub(Sub::new("info", info_cmd).perm("plan.info").about("Information about the plugin"))
        .sub(Sub::new("reload", reload_cmd).perm("plan.reload").about("Restart Plan"))
});

// ---- Start / stop ----

static WEB: Mutex<bool> = Mutex::new(false);
static TASK: Mutex<Option<BukkitTask>> = Mutex::new(None);

fn web_running() -> bool {
    *WEB.lock().unwrap_or_else(|e| e.into_inner())
}

/// ServerInfoFile.yml: this server's uuid (made once).
fn server_uuid(folder: &std::path::Path) -> String {
    let path = folder.join("ServerInfoFile.yml");
    let mut c = Config::load(folder, "ServerInfoFile.yml", include_str!("../data/assets/plan/DefaultServerInfoFile.yml"));
    let uuid = c.str("Server.UUID", "");
    if uuid.len() >= 32 {
        return uuid;
    }
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
    let mut x = n ^ (std::process::id() as u128) << 64;
    x ^= x << 13;
    x ^= x >> 7;
    let u = (x & !(0xF000u128 << 64)) | (0x4000u128 << 64);
    let made = dashed(&format!("{u:032x}"));
    c.set("Server.UUID", made.clone().into());
    c.set("Server.ID", 1.into());
    c.save();
    let _ = path;
    made
}

fn start_all() {
    let Some(plugin) = *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) else { return };
    let folder = plugin.get_data_folder();
    *CONFIG.write().unwrap_or_else(|e| e.into_inner()) = Some(Config::load(&folder, "config.yml", DEFAULT_CONFIG));
    if !cfg(|c| c.str("Database.Type", "SQLite")).eq_ignore_ascii_case("SQLite") {
        log("Database.Type is MySQL: this build stores in SQLite (database.db).");
    }
    let uuid = server_uuid(&folder);
    db::start(folder.join("database.db"), uuid, server_name(), address());
    if !cfg(|c| c.bool("Webserver.Disable_Webserver", false)) {
        let port = cfg(|c| c.int("Webserver.Port", 8804)) as u16;
        let bind = cfg(|c| c.str("Webserver.Internal_IP", "0.0.0.0"));
        match web::start(&bind, port, 4) {
            Ok(()) => {
                *WEB.lock().unwrap_or_else(|e| e.into_inner()) = true;
                log(&format!("Webserver running on PORT {port} ( {} )", address()));
            }
            Err(e) => log(&format!("Webserver did not start on port {port}: {e}")),
        }
    }
    for p in get_online_players() {
        if live_map().as_ref().is_none_or(|m| !m.contains_key(&dashed(&p.get_unique_id()))) {
            start_session(&p);
        }
    }
    let task = plugin.run_task_timer(1200, 1200, sample);
    *TASK.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
}

fn stop_all() {
    if let Some(t) = TASK.lock().unwrap_or_else(|e| e.into_inner()).take() {
        t.cancel();
    }
    let online: Vec<String> = live_map().as_ref().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    for u in online {
        end_session(&u);
    }
    if web_running() {
        web::stop();
        *WEB.lock().unwrap_or_else(|e| e.into_inner()) = false;
    }
    db::stop();
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    start_all();
    plugin.set_executor("plan", |s, l, a| PLAN.run(s, l, a));
    plugin.set_tab_completer("plan", |s, _, a| PLAN.complete(s, a));
    register_events(&plugin);
    horizon_kit::hooks::provide_placeholders(&plugin, "plan", |uuid, _name, params| placeholder(uuid, params));
    log("Player Analytics (Rust) enabled.");
}

fn on_disable(_plugin: Plugin) {
    stop_all();
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
