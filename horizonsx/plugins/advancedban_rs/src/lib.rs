//! AdvancedBan 2.3.0 rewritten in Rust for Horizon: same plugin name, data folder, config.yml, Messages.yml,
//! Layouts.yml, commands and permissions. Storage is SQLite (`data/storage.db`, the Java plugin's tables) instead of
//! HSQLDB/MySQL: the active punishments live in memory, every write and every history read runs on a database
//! thread. Logins, chat and commands are checked from memory (no database on any game thread).

pub mod commands;
pub mod store;

use horizon_bukkit::prelude::*;
use horizon_kit::Config;
use std::sync::{Mutex, RwLock};
use store::{PType, Punishment};

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);

pub struct Files {
    pub config: Config,
    pub messages: Config,
    pub layouts: Config,
}

static FILES: RwLock<Option<Files>> = RwLock::new(None);
const CONFIG: &str = include_str!("../data/config.yml");
const MESSAGES: &str = include_str!("../data/Messages.yml");
const LAYOUTS: &str = include_str!("../data/Layouts.yml");

pub fn reload() {
    let Some(p) = *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) else { return };
    let folder = p.get_data_folder();
    *FILES.write().unwrap_or_else(|e| e.into_inner()) = Some(Files {
        config: Config::load(&folder, "config.yml", CONFIG),
        messages: Config::load(&folder, "Messages.yml", MESSAGES),
        layouts: Config::load(&folder, "Layouts.yml", LAYOUTS),
    });
}

pub fn files<R>(f: impl FnOnce(&Files) -> R) -> R {
    let held = FILES.read().unwrap_or_else(|e| e.into_inner());
    match held.as_ref() {
        Some(x) => f(x),
        None => f(&Files { config: Config::parse(CONFIG), messages: Config::parse(MESSAGES), layouts: Config::parse(LAYOUTS) }),
    }
}

pub fn log(text: &str) {
    horizon_bukkit::horizon_mod::log(&format!("[AdvancedBan] {text}"));
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

/// TimeManager.getTime: now plus `TimeDiff` hours, in milliseconds.
pub fn now() -> i64 {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    ms + files(|f| f.config.int("TimeDiff", 0)) * 3_600_000
}

/// "30m", "2h", "1mo" -> milliseconds (-1 when not a duration).
pub fn to_millis(s: &str) -> i64 {
    let s = s.to_ascii_lowercase();
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    let Ok(n) = digits.parse::<i64>() else { return -1 };
    match &s[digits.len()..] {
        "s" => n * 1000,
        "m" => n * 60_000,
        "h" => n * 3_600_000,
        "d" => n * 86_400_000,
        "w" => n * 604_800_000,
        "mo" => n * 2_592_000_000,
        _ => -1,
    }
}

/// `SimpleDateFormat` for the usual letters (dd MM yyyy yy HH mm ss), in UTC shifted by TimeDiff already.
pub fn format_date(ms: i64) -> String {
    let pattern = files(|f| f.config.str("DateFormat", "dd.MM.yyyy-HH:mm"));
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
    let (hh, mi, ss) = (rem / 3600, rem / 60 % 60, rem % 60);
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        let pad = |v: i64, w: usize| format!("{v:0w$}");
        match c {
            'd' => out.push_str(&pad(d, n.max(1).min(2))),
            'M' => out.push_str(&pad(m, n.max(1).min(2))),
            'y' if n == 2 => out.push_str(&pad(y % 100, 2)),
            'y' => out.push_str(&y.to_string()),
            'H' => out.push_str(&pad(hh, n.min(2))),
            'h' => out.push_str(&pad(if hh % 12 == 0 { 12 } else { hh % 12 }, n.min(2))),
            'm' => out.push_str(&pad(mi, n.min(2))),
            's' => out.push_str(&pad(ss, n.min(2))),
            'a' => out.push_str(if hh < 12 { "AM" } else { "PM" }),
            '\'' => {}
            _ => out.extend(std::iter::repeat_n(c, n)),
        }
        i += n;
    }
    out
}

// ---- Messages ----

fn replace(text: &str, pairs: &[(&str, &str)]) -> String {
    let mut t = text.to_owned();
    for (k, v) in pairs {
        t = t.replace(&format!("%{k}%"), v);
    }
    t
}

pub fn prefix() -> String {
    files(|f| if f.config.bool("Disable Prefix", false) { String::new() } else { horizon_kit::text::colorize(&f.messages.str("General.Prefix", "")) })
}

/// MessageManager.getMessage.
pub fn message(path: &str, pairs: &[(&str, &str)]) -> String {
    let raw = files(|f| f.messages.at(path).and_then(serde_yaml::Value::as_str).map(str::to_owned));
    match raw {
        Some(r) => horizon_kit::text::colorize(&replace(&r, pairs)),
        None => "Failed! See console for details!".into(),
    }
}

/// MessageManager.sendMessage (with the prefix when asked and not disabled).
pub fn send(to: &CommandSender, path: &str, with_prefix: bool, pairs: &[(&str, &str)]) {
    let text = message(path, pairs);
    if text.is_empty() {
        return;
    }
    let p = if with_prefix { prefix() } else { String::new() };
    let full = if p.is_empty() { text } else { format!("{p} {text}") };
    for line in full.split('\n') {
        to.send_message(line);
    }
}

/// MessageManager.getLayout: a list (or one line) from Messages.yml or Layouts.yml.
pub fn layout(from_layouts: bool, path: &str, pairs: &[(&str, &str)]) -> Vec<String> {
    let lines = files(|f| {
        let c = if from_layouts { &f.layouts } else { &f.messages };
        match c.at(path) {
            Some(serde_yaml::Value::Sequence(_)) | Some(serde_yaml::Value::String(_)) => Some(c.list(path)),
            _ => None,
        }
    });
    match lines {
        Some(l) => l.into_iter().map(|s| horizon_kit::text::colorize(&replace(&s, pairs))).collect(),
        None => vec!["Failed! See console for details!".into()],
    }
}

pub fn has_layout(from_layouts: bool, path: &str) -> bool {
    files(|f| if from_layouts { f.layouts.at(path).is_some() } else { f.messages.at(path).is_some() })
}

/// Universal.hasPerms (with `.all` nodes when EnableAllPermissionNodes is on).
pub fn has_perms(sender: &CommandSender, perm: &str) -> bool {
    let perm = perm.to_ascii_lowercase();
    if sender.has_permission(&perm) {
        return true;
    }
    if files(|f| f.config.bool("EnableAllPermissionNodes", false)) {
        let mut p = perm.as_str();
        while let Some(at) = p.rfind('.') {
            p = &p[..at];
            if sender.has_permission(&format!("{p}.all")) {
                return true;
            }
        }
    }
    false
}

pub fn player_has(p: &Player, perm: &str) -> bool {
    has_perms(&CommandSender::Player(p.clone()), perm)
}

/// Sends `lines` to every online player with `perm`.
pub fn notify(perm: &str, lines: &[String]) {
    for p in get_online_players() {
        if player_has(&p, perm) {
            for l in lines {
                p.send_message(l);
            }
        }
    }
}

// ---- A punishment's texts ----

pub fn reason_of(p: &Punishment) -> String {
    p.reason.clone().unwrap_or_else(|| files(|f| f.config.str("DefaultReason", "none"))).replace('\'', "")
}

pub fn duration_of(p: &Punishment, from_start: bool) -> String {
    if !p.kind.is_temp() {
        return "permanent".into();
    }
    let base = if from_start { p.start } else { now() };
    let diff = -((-(p.end - base)).div_euclid(1000));
    let two = |v: i64| format!("{v:02}");
    let (d, h, m, s) = (diff / 86_400, diff / 3600 % 24, diff / 60 % 60, diff % 60);
    if diff > 86_400 {
        message("General.TimeLayoutD", &[("D", &d.to_string()), ("H", &h.to_string()), ("M", &m.to_string()), ("S", &s.to_string()), ("DD", &two(d)), ("HH", &two(h)), ("MM", &two(m)), ("SS", &two(s))])
    } else if diff > 3600 {
        let h = diff / 3600;
        message("General.TimeLayoutH", &[("H", &h.to_string()), ("M", &m.to_string()), ("S", &s.to_string()), ("HH", &two(h)), ("MM", &two(m)), ("SS", &two(s))])
    } else if diff > 60 {
        let m = diff / 60;
        message("General.TimeLayoutM", &[("M", &m.to_string()), ("S", &s.to_string()), ("MM", &two(m)), ("SS", &two(s))])
    } else {
        message("General.TimeLayoutS", &[("S", &diff.to_string()), ("SS", &two(diff))])
    }
}

pub fn current_warns(uuid: &str) -> usize {
    store::of(uuid, PType::Warning, now()).len()
}

/// Punishment.getLayout: what the punished player is shown (the kick screen for bans).
pub fn layout_of(p: &Punishment) -> Vec<String> {
    layout_with(p, None)
}

/// The layout with `%COUNT%` given (a new warning counts itself).
pub fn layout_with(p: &Punishment, warns: Option<usize>) -> Vec<String> {
    let reason = reason_of(p);
    let is_layout = reason.starts_with('@') || reason.starts_with('~');
    let first = reason.split(' ').next().unwrap_or("");
    let shown_reason = if is_layout { reason.get(first.len() + 1..).unwrap_or("").to_owned() } else { reason.clone() };
    let count = if p.kind.basic() == PType::Warning { warns.unwrap_or_else(|| current_warns(&p.uuid) + 1).to_string() } else { "0".into() };
    let path = if is_layout { format!("Message.{}", &first[1..]) } else { format!("{}.Layout", p.kind.name()) };
    let pre = prefix();
    layout(is_layout, &path, &[("OPERATOR", &p.operator), ("PREFIX", &pre), ("DURATION", &duration_of(p, false)), ("REASON", &shown_reason), ("HEXID", &format!("{:X}", p.id)), ("ID", &p.id.to_string()), ("DATE", &format_date(p.start)), ("COUNT", &count)])
}

pub fn layout_bsn(p: &Punishment) -> String {
    layout_of(p).join("\n")
}

fn announce(p: &Punishment, warns: usize) {
    let pre = prefix();
    let lines = layout(false, &format!("{}.Notification", p.kind.name()), &[("OPERATOR", &p.operator), ("PREFIX", &pre), ("DURATION", &duration_of(p, true)), ("REASON", &reason_of(p)), ("NAME", &p.name), ("ID", &p.id.to_string()), ("HEXID", &format!("{:X}", p.id)), ("DATE", &format_date(p.start)), ("COUNT", &warns.to_string())]);
    notify(&format!("ab.notify.{}", p.kind.name()), &lines);
}

fn online(name: &str) -> Option<Player> {
    get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name))
}

/// Punishment.create: stored, announced, the player told or kicked, warn actions run.
pub fn create(name: &str, target: &str, reason: Option<String>, operator: &str, kind: PType, end: i64, calculation: &str, silent: bool) {
    let kind = if end == -1 { kind.permanent() } else { kind };
    let warns = if kind.basic() == PType::Warning { current_warns(target) + 1 } else { 0 };
    let p = Punishment { id: -1, name: name.to_owned(), uuid: target.to_owned(), reason, operator: operator.to_owned(), kind, start: now(), end, calculation: calculation.to_owned() };
    let p = store::add(p);
    let shown = layout_with(&p, Some(warns));
    if !silent {
        announce(&p, warns);
    }
    if let Some(player) = online(name) {
        if kind.basic() != PType::Ban && kind != PType::Kick {
            if kind.basic() != PType::Note {
                for line in shown {
                    player.send_message(&line);
                }
            }
        } else {
            let screen = shown.join("\n");
            horizon_bukkit::scheduler::run_task(move || player.kick_player(&screen));
        }
    }
    if kind.basic() == PType::Warning {
        let cmd = files(|f| (1..=warns).rev().find_map(|i| f.config.at(&format!("WarnActions.{i}")).and_then(serde_yaml::Value::as_str).map(str::to_owned)));
        if let Some(cmd) = cmd {
            let cmd = cmd.replace("%PLAYER%", name).replace("%COUNT%", &warns.to_string()).replace("%REASON%", &reason_of(&p));
            horizon_bukkit::scheduler::run_task(move || {
                horizon_bukkit::dispatch_command(&cmd);
                log(&format!("Executing command: {cmd}"));
            });
        }
    }
}

/// Punishment.delete: gone, with the undo notification when `who` is given.
pub fn delete(p: &Punishment, who: Option<&str>) {
    if p.kind == PType::Kick {
        return;
    }
    store::remove(p.id);
    if let Some(who) = who {
        let basic = p.kind.basic().name();
        let pre = prefix();
        let text = message(&format!("Un{basic}.Notification"), &[("OPERATOR", who), ("NAME", &p.name)]);
        notify(&format!("ab.undoNotify.{basic}"), &[if pre.is_empty() { text } else { format!("{pre} {text}") }]);
    }
}

// ---- Names / uuids ----

/// A name's uuid (no dashes): an online player's, else the server's record of the name.
pub fn uuid_of(name: &str) -> Option<String> {
    if let Some(p) = online(name) {
        return Some(p.get_unique_id().replace('-', ""));
    }
    let off = horizon_bukkit::get_offline_player(name);
    let id = off.get_unique_id()?.replace('-', "");
    (!id.is_empty()).then_some(id)
}

pub fn is_ip(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()))
}

pub fn is_mute_command(cmd: &str) -> bool {
    let cmd = cmd.trim_start_matches('/').to_ascii_lowercase();
    let mut words = cmd.split_whitespace();
    let Some(first) = words.next() else { return false };
    let first = first.rsplit(':').next().unwrap_or(first);
    let rest: Vec<&str> = words.collect();
    let full = if rest.is_empty() { first.to_owned() } else { format!("{first} {}", rest.join(" ")) };
    files(|f| f.config.list("MuteCommands")).iter().any(|m| {
        let m = m.to_ascii_lowercase();
        full == m || full.starts_with(&format!("{m} "))
    })
}

fn register_events(plugin: &Plugin) {
    plugin.register_event_with(enums::event_priority::HIGH, false, |e: &mut AsyncPlayerPreLoginEvent| {
        if e.get_login_result() != "ALLOWED" {
            return;
        }
        let name = e.get_name().to_ascii_lowercase();
        let uuid = e.get_unique_id().replace('-', "");
        let ip = e.get_address().map(|a| a.to_string());
        if let Some(ip) = &ip {
            store::mem().ips.insert(name.clone(), ip.clone());
        }
        let mut targets: Vec<&str> = vec![&uuid];
        if let Some(ip) = &ip {
            targets.push(ip);
        }
        if let Some(ban) = store::find(&targets, PType::Ban, now()) {
            e.disallow("KICK_BANNED", &horizon_bukkit::adventure::legacy(&layout_bsn(&ban)));
        }
    });
    plugin.register_event_with(enums::event_priority::HIGHEST, false, |e: &mut AsyncPlayerChatEvent| {
        let p = e.get_player().clone();
        let uuid = p.get_unique_id().replace('-', "");
        if let Some(mute) = store::find(&[&uuid], PType::Mute, now()) {
            e.set_cancelled(true);
            on_main(move || {
                for line in layout_of(&mute) {
                    p.send_message(&line);
                }
            });
        }
    });
    plugin.register_event_with(enums::event_priority::HIGHEST, false, |e: &mut PlayerCommandPreprocessEvent| {
        if !is_mute_command(e.get_message()) {
            return;
        }
        let p = e.get_player().clone();
        let uuid = p.get_unique_id().replace('-', "");
        if let Some(mute) = store::find(&[&uuid], PType::Mute, now()) {
            e.set_cancelled(true);
            for line in layout_of(&mute) {
                p.send_message(&line);
            }
        }
    });
    plugin.register_event(|e: &mut PlayerJoinEvent| {
        // A player who joined without the pre-login event (an IP known only from the connection).
        let p = e.get_player().clone();
        if let Some(ip) = p.get_address() {
            let ip = ip.split(':').next().unwrap_or(&ip).trim_start_matches('/').to_owned();
            store::mem().ips.insert(p.get_name().to_ascii_lowercase(), ip);
        }
    });
}

fn placeholder(name: &str, params: &str) -> Option<String> {
    let uuid = uuid_of(name)?;
    let t = now();
    Some(match params.to_ascii_lowercase().as_str() {
        "banned" | "isbanned" => store::find(&[&uuid], PType::Ban, t).is_some().to_string(),
        "muted" | "ismuted" => store::find(&[&uuid], PType::Mute, t).is_some().to_string(),
        "warns" | "warnings" => current_warns(&uuid).to_string(),
        "notes" => store::of(&uuid, PType::Note, t).len().to_string(),
        "mute_reason" => store::find(&[&uuid], PType::Mute, t).map(|p| reason_of(&p)).unwrap_or_default(),
        "mute_duration" => store::find(&[&uuid], PType::Mute, t).map(|p| duration_of(&p, false)).unwrap_or_default(),
        _ => return None,
    })
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    reload();
    let folder = plugin.get_data_folder();
    if files(|f| f.config.bool("UseMySQL", false)) {
        log("UseMySQL is on, but this build keeps punishments in SQLite (data/storage.db).");
    }
    store::start(folder.join("data").join("storage.db"), now());
    commands::register(&plugin);
    register_events(&plugin);
    horizon_kit::hooks::provide_placeholders(&plugin, "advancedban", |_uuid, name, params| placeholder(name, params));
    for p in get_online_players() {
        if let Some(ip) = p.get_address() {
            store::mem().ips.insert(p.get_name().to_ascii_lowercase(), ip.split(':').next().unwrap_or(&ip).trim_start_matches('/').to_owned());
        }
    }
    let count = store::mem().active.len();
    if files(|f| f.config.bool("DetailedEnableMessage", true)) {
        log(&format!("AdvancedBan v2.3.0 (Rust) enabled: SQLite storage, {count} active punishments loaded."));
    }
}

fn on_disable(_plugin: Plugin) {
    store::stop();
    if files(|f| f.config.bool("DetailedDisableMessage", true)) {
        log("AdvancedBan disabled.");
    }
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
