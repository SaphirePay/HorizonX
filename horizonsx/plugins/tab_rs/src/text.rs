//! TAB's text: `%placeholders%` (its own, `%animation:..%`, `%condition:..%`, and any other plugin's through
//! horizon-kit hooks), output replacements, conditions, and colours (`&`, `&#hex`, `<#a>..</#b>` gradients,
//! MiniMessage) into a text component.
//!
//! Every placeholder value is cached per player (and per server for the server-wide ones) for its refresh
//! interval (`placeholder-refresh-intervals`, default 500 ms): a header, a sidebar and a name tag using `%ping%`
//! cost one lookup between them, not three a refresh.

use horizon_bukkit::Player;
use horizon_kit::{hooks, mini, text};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[derive(Clone, Debug, Default)]
pub struct Animation {
    pub interval: u64,
    pub texts: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct NamedCondition {
    pub conditions: Vec<String>,
    pub or: bool,
    pub yes: String,
    pub no: String,
}

#[derive(Clone, Debug, Default)]
pub struct TextSettings {
    pub animations: HashMap<String, Animation>,
    pub conditions: HashMap<String, NamedCondition>,
    pub replacements: HashMap<String, Vec<(String, String)>>,
    pub intervals: HashMap<String, u64>,
    pub default_interval: u64,
    pub date_format: String,
    pub time_format: String,
    pub time_offset_hours: f64,
    pub minimessage: bool,
}

/// Per-player and server-wide placeholder values with their expiry.
#[derive(Default)]
struct Cache {
    players: HashMap<String, HashMap<String, (String, u64)>>,
    server: HashMap<String, (String, u64)>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

pub fn forget(uuid: &str) {
    if let Some(c) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        c.players.remove(uuid);
    }
}

pub fn clear() {
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// What the TAB placeholders need beyond the player: the group TAB assigned them (`%group%`).
pub struct Who<'a> {
    pub player: &'a Player,
    pub uuid: &'a str,
    pub group: &'a str,
}

const SERVER_WIDE: [&str; 11] = ["online", "staffonline", "nonstaffonline", "memory-used", "memory-max", "memory-used-gb", "memory-max-gb", "tps", "date", "time", "server"];

fn memory_mb() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        let rss = std::fs::read_to_string("/proc/self/status").ok().and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())).unwrap_or(0);
        let total = std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| s.lines().find(|l| l.starts_with("MemTotal:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())).unwrap_or(0);
        (rss / 1024, total / 1024)
    }
    #[cfg(not(target_os = "linux"))]
    {
        (0, 0)
    }
}

/// Java `SimpleDateFormat` for the letters TAB's defaults use (`dd.MM.yyyy`, `HH:mm:ss`, `h:mm a`, `EEE`, `MMM`).
pub fn format_date(pattern: &str, offset_hours: f64) -> String {
    use chrono::{Datelike, Duration, Local, Timelike};
    let t = Local::now() + Duration::milliseconds((offset_hours * 3_600_000.0) as i64);
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                out.push(chars[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            out.push(c);
            i += 1;
            continue;
        }
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        let pad = |v: u32| if n >= 2 { format!("{v:0n$}") } else { v.to_string() };
        let piece = match c {
            'y' => {
                if n == 2 {
                    format!("{:02}", t.year() % 100)
                } else {
                    t.year().to_string()
                }
            }
            'M' if n >= 4 => t.format("%B").to_string(),
            'M' if n == 3 => t.format("%b").to_string(),
            'M' => pad(t.month()),
            'd' => pad(t.day()),
            'H' => pad(t.hour()),
            'h' => pad(if t.hour() % 12 == 0 { 12 } else { t.hour() % 12 }),
            'm' => pad(t.minute()),
            's' => pad(t.second()),
            'a' => (if t.hour() < 12 { "AM" } else { "PM" }).to_owned(),
            'E' if n >= 4 => t.format("%A").to_string(),
            'E' => t.format("%a").to_string(),
            'S' => format!("{:03}", t.timestamp_subsec_millis()),
            other => other.to_string().repeat(n),
        };
        out.push_str(&piece);
        i += n;
    }
    out
}

fn tab_value(key: &str, who: Option<&Who>, s: &TextSettings) -> Option<String> {
    let online = || horizon_bukkit::get_online_players();
    let server = Some(match key {
        "online" => online().len().to_string(),
        "staffonline" => online().iter().filter(|p| p.has_permission("tab.staff")).count().to_string(),
        "nonstaffonline" => online().iter().filter(|p| !p.has_permission("tab.staff")).count().to_string(),
        "memory-used" => memory_mb().0.to_string(),
        "memory-max" => memory_mb().1.to_string(),
        "memory-used-gb" => format!("{:.1}", memory_mb().0 as f64 / 1024.0),
        "memory-max-gb" => format!("{:.1}", memory_mb().1 as f64 / 1024.0),
        "tps" => format!("{:.1}", horizon_bukkit::server::get_tps()[0].min(20.0)),
        "date" => format_date(&s.date_format, s.time_offset_hours),
        "time" => format_date(&s.time_format, s.time_offset_hours),
        "server" => "Horizon".to_owned(),
        _ => String::new(),
    });
    if SERVER_WIDE.contains(&key) {
        return server;
    }
    let w = who?;
    let p = w.player;
    Some(match key {
        "player" => p.get_name().to_owned(),
        "uuid" => p.get_unique_id(),
        "displayname" => {
            let d = p.get_display_name();
            if d.is_empty() { p.get_name().to_owned() } else { d.to_owned() }
        }
        "world" => p.get_world().get_name().to_owned(),
        "worldonline" => {
            let world = p.get_world().get_name().to_owned();
            online().iter().filter(|o| o.get_world().get_name() == world).count().to_string()
        }
        "ping" => p.get_ping().to_string(),
        "health" => (p.get_health().ceil() as i64).to_string(),
        "group" => w.group.to_owned(),
        "gamemode" => p.get_game_mode().to_ascii_lowercase(),
        "luckperms-prefix" | "vault-prefix" => hooks::chat::prefix(w.uuid),
        "luckperms-suffix" | "vault-suffix" => hooks::chat::suffix(w.uuid),
        "player-version-id" => "9999".to_owned(),
        "player-version" => "latest".to_owned(),
        "bedrock" => "false".to_owned(),
        "vanished" => "false".to_owned(),
        "beautifiedgroup" | "rank" => {
            let mut c = w.group.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
        _ => return None,
    })
}

fn interval_of(key: &str, s: &TextSettings) -> u64 {
    s.intervals.get(&format!("%{key}%")).copied().unwrap_or(s.default_interval)
}

/// One placeholder's value (without `%`), cached.
fn value(key: &str, who: Option<&Who>, s: &TextSettings, now: u64) -> Option<String> {
    if let Some(name) = key.strip_prefix("animation:") {
        let a = s.animations.get(name).or_else(|| s.animations.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, a)| a))?;
        if a.texts.is_empty() {
            return Some(String::new());
        }
        let frame = (now / a.interval.max(1)) as usize % a.texts.len();
        return Some(a.texts[frame].clone());
    }
    if let Some(name) = key.strip_prefix("condition:") {
        let c = s.conditions.get(name)?.clone();
        let ok = check_all(&c.conditions, c.or, who, s, now);
        return Some(if ok { c.yes } else { c.no });
    }
    let server_wide = SERVER_WIDE.contains(&key);
    let uuid = who.map(|w| w.uuid.to_owned()).unwrap_or_default();
    {
        let held = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = held.as_ref() {
            let hit = if server_wide { c.server.get(key) } else { c.players.get(&uuid).and_then(|m| m.get(key)) };
            if let Some((v, until)) = hit
                && *until > now
            {
                return Some(v.clone());
            }
        }
    }
    let raw = tab_value(key, who, s).or_else(|| hooks::placeholder(key, who.map(|w| w.player)))?;
    let replaced = match s.replacements.get(&format!("%{key}%")) {
        Some(list) => list
            .iter()
            .find(|(from, _)| from == &raw)
            .or_else(|| list.iter().find(|(from, _)| from == "else"))
            .map_or_else(|| raw.clone(), |(_, to)| to.replace(&format!("%{key}%"), &raw)),
        None => raw,
    };
    let until = now + interval_of(key, s);
    let mut held = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let c = held.get_or_insert_with(Cache::default);
    if server_wide {
        c.server.insert(key.to_owned(), (replaced.clone(), until));
    } else {
        c.players.entry(uuid).or_default().insert(key.to_owned(), (replaced.clone(), until));
    }
    Some(replaced)
}

/// `%...%` filled (twice: animations and conditions may hold placeholders).
pub fn fill(textin: &str, who: Option<&Who>, s: &TextSettings, now: u64) -> String {
    let mut out = textin.to_owned();
    for _ in 0..3 {
        if !out.contains('%') {
            break;
        }
        let next = fill_once(&out, who, s, now);
        if next == out {
            break;
        }
        out = next;
    }
    out
}

fn fill_once(input: &str, who: Option<&Who>, s: &TextSettings, now: u64) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let key = &after[..end];
        if key.is_empty() || key.contains(' ') {
            out.push('%');
            rest = after;
            continue;
        }
        match value(key, who, s, now) {
            Some(v) => out.push_str(&v),
            None => {
                out.push('%');
                out.push_str(key);
                out.push('%');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// One condition: `permission:x`, `a=b`, `a!=b`, `a>=b`, `a<=b`, `a>b`, `a<b` (numbers), `a<-b` (contains),
/// `a|-b` (starts with), `a-|b` (ends with); placeholders filled on both sides.
pub fn check(cond: &str, who: Option<&Who>, s: &TextSettings, now: u64) -> bool {
    let cond = cond.trim();
    if let Some(node) = cond.strip_prefix("permission:") {
        return who.is_some_and(|w| w.player.has_permission(node.trim()));
    }
    for op in ["!=", ">=", "<=", "<-", "|-", "-|", "=", ">", "<"] {
        if let Some((l, r)) = cond.split_once(op) {
            let (l, r) = (fill(l, who, s, now), fill(r, who, s, now));
            let (l, r) = (text::strip(&l), text::strip(&r));
            let num = |v: &str| v.trim().replace(',', "").parse::<f64>().ok();
            return match op {
                "=" => l == r,
                "!=" => l != r,
                "<-" => l.contains(&r),
                "|-" => l.starts_with(&r),
                "-|" => l.ends_with(&r),
                _ => match (num(&l), num(&r)) {
                    (Some(a), Some(b)) => match op {
                        ">=" => a >= b,
                        "<=" => a <= b,
                        ">" => a > b,
                        _ => a < b,
                    },
                    _ => false,
                },
            };
        }
    }
    // A named condition used inline.
    s.conditions.get(cond).is_some_and(|c| check_all(&c.conditions, c.or, who, s, now))
}

fn check_all(list: &[String], or: bool, who: Option<&Who>, s: &TextSettings, now: u64) -> bool {
    if or { list.iter().any(|c| check(c, who, s, now)) } else { list.iter().all(|c| check(c, who, s, now)) }
}

/// A `display-condition`/`disable-condition`: `a;b` all of them, `a|b` any; empty = true.
pub fn condition(text_cond: &str, who: Option<&Who>, s: &TextSettings, now: u64) -> bool {
    let t = text_cond.trim();
    if t.is_empty() {
        return true;
    }
    if t.contains('|') && !t.contains("|-") {
        return t.split('|').any(|c| condition(c, who, s, now));
    }
    t.split(';').all(|c| check(c, who, s, now))
}

/// Coloured text -> a component the NBT encoder takes (no bare strings in `extra`).
pub fn component(coloured: &str, s: &TextSettings) -> Value {
    let legacy = text::colorize(coloured);
    let v = if s.minimessage && legacy.contains('<') { mini::parse_mixed(&legacy) } else { horizon_bukkit::adventure::legacy(&hex_legacy(&legacy)).0 };
    normalize(v)
}

/// `§x§r§r§g§g§b§b` as `<#rrggbb>` so the legacy reader keeps hex colours.
fn hex_legacy(legacy: &str) -> String {
    if !legacy.contains("§x") {
        return legacy.to_owned();
    }
    mini::to_legacy(&mini::parse_mixed(legacy))
}

pub fn normalize(v: Value) -> Value {
    match v {
        Value::String(s) => json!({ "text": s }),
        Value::Object(mut o) => {
            if let Some(Value::Array(extra)) = o.remove("extra") {
                let extra: Vec<Value> = extra.into_iter().map(normalize).collect();
                if !extra.is_empty() {
                    o.insert("extra".into(), Value::Array(extra));
                }
            }
            if !o.contains_key("text") && !o.contains_key("translate") && !o.contains_key("keybind") {
                o.insert("text".into(), json!(""));
            }
            Value::Object(o)
        }
        other => json!({ "text": other.to_string() }),
    }
}

/// Lines joined as the tab list shows them.
pub fn lines(lines: &[String], who: Option<&Who>, s: &TextSettings, now: u64) -> Value {
    let filled: Vec<String> = lines.iter().map(|l| fill(l, who, s, now)).collect();
    let mut parts: Vec<Value> = Vec::new();
    for (i, l) in filled.iter().enumerate() {
        if i > 0 {
            parts.push(json!({ "text": "\n" }));
        }
        parts.push(component(l, s));
    }
    json!({ "text": "", "extra": parts })
}

/// The last colour of a coloured text, as a team colour index (0-15).
pub fn last_color(coloured: &str) -> Option<i32> {
    let legacy = text::colorize(coloured);
    let chars: Vec<char> = legacy.chars().collect();
    let mut last = None;
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '§' {
            let c = chars[i + 1].to_ascii_lowercase();
            if let Some(d) = c.to_digit(16) {
                last = Some(d as i32);
            } else if c == 'r' {
                last = None;
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> TextSettings {
        let mut s = TextSettings { default_interval: 500, date_format: "dd.MM.yyyy".into(), time_format: "HH:mm:ss".into(), minimessage: true, ..Default::default() };
        s.animations.insert("A".into(), Animation { interval: 100, texts: vec!["x".into(), "y".into()] });
        s
    }
    #[test]
    fn placeholders_and_conditions() {
        let s = settings();
        assert_eq!(fill("%animation:A%", None, &s, 50), "x");
        assert_eq!(fill("%animation:A%", None, &s, 150), "y");
        assert!(check("5>=3", None, &s, 0));
        assert!(check("abc|-ab", None, &s, 0));
        assert!(check("abc<-b", None, &s, 0));
        assert!(!check("a=b", None, &s, 0));
        assert!(condition("1=1;2=2", None, &s, 0));
        assert!(condition("1=2|2=2", None, &s, 0));
        assert_eq!(format_date("dd.MM.yyyy", 0.0).len(), 10);
        assert_eq!(last_color("&a[Admin] &c"), Some(12));
        let c = component("&cHi <bold>x", &s);
        assert!(c["extra"].as_array().is_some());
    }
}
