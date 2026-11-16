//! ajLeaderboards 2.11 for Horizon, written again in Rust (the Java it follows is in java/). Same
//! `/ajleaderboards` (`/ajlb`), `config.yml`, `messages.yml`, and the `%ajlb_...%` placeholders (lb, position,
//! value, extra, rel, total, size, reset), served to every other plugin through horizon-kit hooks.
//!
//! A board is any placeholder (`statistic_player_kills`, `mcmmo_power_level`, `vault_eco_balance`...). Every
//! `stat-refresh` ticks the server thread reads each board's value for the online players (the only game work)
//! and hands the batch to the board thread, which updates the totals and the hourly/daily/weekly/monthly/yearly
//! deltas, re-sorts, and writes `cache.json` at most once a minute. Placeholder lookups read the sorted lists
//! from memory: no database query per placeholder, as Java's cache does with its fetch threads.

mod store;

use horizon_bukkit::event::{PlayerJoinEvent, PlayerQuitEvent};
use horizon_bukkit::{CommandSender, Player, Plugin, bukkit_plugin};
use horizon_kit::{Config, hooks};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use store::{Snapshot, TYPES, Update};

const DEF_CONFIG: &str = include_str!("../data/config.yml");
const DEF_MESSAGES: &str = r#"signs:
  top:
    default:
      - "&7&m       &r #{POSITION} &7&m       "
      - "&6{NAME}"
      - "&e{VALUE} {VALUENAME}"
      - "&7&m                   "
formatted:
  k: "k"
  m: "m"
  b: "b"
  t: "t"
time:
  w: "w "
  d: "d "
  h: "h "
  m: "m "
  s: "s"
noperm: "You don't have permission to do this!"
commands:
  reload:
    success: "&aConfigs reloaded!"
    fail: "&cAn error occurred while reloading one of your configs. Check the console for more info."
  export:
    fileexists: "&cThe file &f{FILE}&c already exists!"
    success: "&aThe cache has been exported to the file &f{FILE}&a!"
  import:
    nofile: "&cThe file &f{FILE}&c doesnt exist!"
    success: "&aThe cache has been imported from the file &f{FILE}&a!"
no-data:
  lb:
    name: "---"
    value: "---"
  extra: "---"
  rel:
    name: "---"
    value: "---"
loading:
  short: "-?-"
  position: ".."
"#;

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static SETTINGS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);
static SIGNS: Mutex<Vec<SignDisplay>> = Mutex::new(Vec::new());

fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Settings {
    pub stat_refresh: u64,
    pub sign_update: u64,
    pub update_on_join: bool,
    pub update_on_leave: bool,
    pub extras: Vec<String>,
    pub reverse: Vec<String>,
    pub comma: String,
    pub decimal: String,
    pub round: usize,
    pub value_names: HashMap<String, String>,
    pub dont_add_zero: Vec<String>,
    pub require_zero_validation: bool,
    pub weekly_on: chrono::Weekday,
    pub no_name: String,
    pub no_value: String,
    pub dontupdate_perm: bool,
    pub only_update: Vec<String>,
    pub msgs: Config,
}

fn settings() -> Arc<Settings> {
    SETTINGS.read().unwrap_or_else(|e| e.into_inner()).clone().expect("ajLeaderboards settings")
}

fn load_settings() -> Settings {
    let dir = plugin().map(|p| p.get_data_folder()).unwrap_or_default();
    let c = Config::load(&dir, "config.yml", DEF_CONFIG);
    let msgs = Config::load(&dir, "messages.yml", DEF_MESSAGES);
    let weekly_on = match c.str("reset-weekly-on", "sunday").to_ascii_lowercase().as_str() {
        "monday" => chrono::Weekday::Mon,
        "tuesday" => chrono::Weekday::Tue,
        "wednesday" => chrono::Weekday::Wed,
        "thursday" => chrono::Weekday::Thu,
        "friday" => chrono::Weekday::Fri,
        "saturday" => chrono::Weekday::Sat,
        _ => chrono::Weekday::Sun,
    };
    Settings {
        stat_refresh: c.int("stat-refresh", 1200).max(20) as u64,
        sign_update: c.int("sign-update", 20).max(1) as u64,
        update_on_join: c.bool("update-on-join", true),
        update_on_leave: c.bool("update-on-leave", true),
        extras: c.list("extras"),
        reverse: c.list("reverse-sort"),
        comma: c.str("comma", ","),
        decimal: c.str("decimal", "."),
        round: c.int("round-decimal-places", 2).clamp(0, 10) as usize,
        value_names: c.list("value-names").iter().filter_map(|v| v.split_once('%').map(|(b, n)| (b.to_owned(), n.to_owned()))).collect(),
        dont_add_zero: c.list("dont-add-zero"),
        require_zero_validation: c.bool("require-zero-validation", true),
        weekly_on,
        no_name: c.str("no-data-name", "---"),
        no_value: c.str("no-data-score", "---"),
        dontupdate_perm: c.bool("enable-dontupdate-permission", true),
        only_update: c.list("only-update"),
        msgs,
    }
}

fn msg(key: &str) -> String {
    horizon_bukkit::chat_color::translate_alternate_color_codes('&', &settings().msgs.str(key, key))
}

// ---- Formatting ----

/// `addCommas`: grouping and decimals as configured, up to `round-decimal-places`.
pub fn commas(n: f64, s: &Settings) -> String {
    let neg = n < 0.0;
    let fixed = format!("{:.*}", s.round, n.abs());
    let (int, frac) = fixed.split_once('.').map_or((fixed.as_str(), ""), |(a, b)| (a, b));
    let frac = frac.trim_end_matches('0');
    let mut out = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push_str(&s.comma);
        }
        out.push(ch);
    }
    if !frac.is_empty() {
        out.push_str(&s.decimal);
        out.push_str(frac);
    }
    if neg { format!("-{out}") } else { out }
}

/// `formatDouble`: 1.23k, 4.5m, ...
pub fn short(n: f64, s: &Settings) -> String {
    let units = [(1e12, "t"), (1e9, "b"), (1e6, "m"), (1e3, "k")];
    for (div, key) in units {
        if n.abs() >= div {
            let v = n / div;
            let t = format!("{v:.2}");
            let t = t.trim_end_matches('0').trim_end_matches('.');
            return format!("{t}{}", s.msgs.str(&format!("formatted.{key}"), key));
        }
    }
    let t = format!("{n:.2}");
    t.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `TimeUtils.formatTimeSeconds`.
pub fn time(secs: i64, s: &Settings) -> String {
    let unit = |k: &str| s.msgs.str(&format!("time.{k}"), k);
    let mut left = secs.max(0);
    let mut out = String::new();
    for (size, key) in [(604_800, "w"), (86_400, "d"), (3600, "h"), (60, "m")] {
        if left >= size {
            out.push_str(&format!("{}{}", left / size, unit(key)));
            left %= size;
        }
    }
    if left > 0 || out.is_empty() {
        out.push_str(&format!("{left}{}", unit("s")));
    }
    out.trim_end().to_owned()
}

/// A placeholder's text as a number: commas and colours off, `k`/`m` suffixes read.
pub fn parse_number(raw: &str) -> Option<f64> {
    let t = horizon_kit::text::strip(raw).trim().replace([',', ' '], "");
    if t.is_empty() {
        return None;
    }
    let (body, mul) = match t.chars().last()?.to_ascii_lowercase() {
        'k' => (&t[..t.len() - 1], 1e3),
        'm' => (&t[..t.len() - 1], 1e6),
        'b' => (&t[..t.len() - 1], 1e9),
        _ => (t.as_str(), 1.0),
    };
    body.parse::<f64>().ok().map(|v| v * mul)
}

// ---- Collecting values (server thread) ----

fn read_player(player: &Player, boards: &[String], s: &Settings) -> Option<Update> {
    if s.dontupdate_perm && player.has_permission("ajleaderboards.dontupdate") && !player.is_op() {
        return None;
    }
    let mut values = Vec::new();
    for b in boards {
        if !s.only_update.is_empty() && !s.only_update.iter().any(|o| o.eq_ignore_ascii_case(b)) {
            continue;
        }
        let Some(raw) = hooks::placeholder(b, Some(player)) else { continue };
        if let Some(v) = parse_number(&raw) {
            values.push((b.clone(), v));
        }
    }
    let uuid = player.get_unique_id();
    let meta = hooks::chat::all(&uuid);
    let extras = s.extras.iter().filter_map(|e| hooks::placeholder(e, Some(player)).map(|v| (e.clone(), v))).collect();
    Some(Update {
        uuid,
        name: player.get_name().to_owned(),
        display: player.get_display_name().to_owned(),
        prefix: meta.prefix,
        suffix: meta.suffix,
        values,
        extras,
    })
}

fn update_players(players: &[Player]) {
    let s = settings();
    let boards = store::boards();
    if boards.is_empty() && s.extras.is_empty() {
        return;
    }
    let batch: Vec<Update> = players.iter().filter_map(|p| read_player(p, &boards, &s)).collect();
    store::send(batch);
}

// ---- Placeholders ----

fn color_of(prefix: &str) -> String {
    let legacy = horizon_kit::text::colorize(prefix);
    let mut last = String::new();
    let chars: Vec<char> = legacy.chars().collect();
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '§' {
            let c = chars[i + 1].to_ascii_lowercase();
            if c.is_ascii_hexdigit() || c == 'r' {
                last = format!("§{c}");
            } else if "lonmk".contains(c) {
                last.push_str(&format!("§{c}"));
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    last
}

/// `lb_<board>_<n>_<type>_<what>` and the rest, as the Java expansion's regexes split them. Board names may
/// hold `_`, so the type is found by its name from the right.
pub fn answer(uuid: &str, params: &str) -> Option<String> {
    let s = settings();
    let snap = store::snapshot();
    let p = params.to_ascii_lowercase();
    let typed = |rest: &str| -> Option<(String, String, String)> {
        // "<board>_<type>[_suffix]" -> (board, type, suffix)
        for t in TYPES {
            for suffix in ["_formatted", "_raw", "_time", ""] {
                let tail = format!("_{t}{suffix}");
                if let Some(board) = rest.strip_suffix(&tail) {
                    let board = snap.board_name(board)?;
                    return Some((board, t.to_owned(), suffix.trim_start_matches('_').to_owned()));
                }
            }
        }
        None
    };
    if let Some(rest) = p.strip_prefix("lb_") {
        // lb_<board>_<n>_<type>_<what>
        let whats = ["value_formatted", "skin_uuid", "displayname", "rawvalue", "prefix", "suffix", "color", "value", "name", "uuid", "time", "skin"];
        let (what, head) = whats.iter().find_map(|w| rest.strip_suffix(&format!("_{w}")).map(|h| (w.to_string(), h.to_owned())))
            .or_else(|| rest.rsplit_once("_extra_").map(|(h, e)| (format!("extra_{e}"), h.to_owned())))?;
        let (head, t) = TYPES.iter().find_map(|t| head.strip_suffix(&format!("_{t}")).map(|h| (h.to_owned(), t.to_string())))?;
        let (board, n) = head.rsplit_once('_')?;
        let n: usize = n.parse().ok().filter(|n| *n >= 1)?;
        let board = snap.board_name(board)?;
        let entry = snap.top(&board, &t, n - 1);
        return Some(match entry {
            None => match what.as_str() {
                "name" | "displayname" => s.no_name.clone(),
                "value" | "value_formatted" | "rawvalue" | "time" => s.no_value.clone(),
                _ => String::new(),
            },
            Some(e) => match what.as_str() {
                "name" => e.name.clone(),
                "displayname" => if e.display.is_empty() { e.name.clone() } else { e.display.clone() },
                "uuid" | "skin_uuid" => e.uuid.clone(),
                "skin" => e.name.clone(),
                "prefix" => e.prefix.clone(),
                "suffix" => e.suffix.clone(),
                "color" => color_of(&e.prefix),
                "value" => commas(e.value, &s),
                "value_formatted" => short(e.value, &s),
                "rawvalue" => format!("{}", e.value),
                "time" => time(e.value.round() as i64, &s),
                w => {
                    let extra = w.strip_prefix("extra_")?;
                    snap.extra(&e.uuid, extra).unwrap_or_else(|| s.msgs.str("no-data.extra", "---"))
                }
            },
        });
    }
    if let Some(rest) = p.strip_prefix("position_") {
        let (board, t, suffix) = typed(rest)?;
        let pos = snap.position(&board, &t, uuid);
        return Some(match pos {
            Some(n) if suffix == "formatted" => commas(n as f64, &s),
            Some(n) => n.to_string(),
            None => s.msgs.str("loading.position", ".."),
        });
    }
    if let Some(rest) = p.strip_prefix("value_") {
        let (board, t, suffix) = typed(rest)?;
        let v = snap.value(&board, &t, uuid);
        return Some(match (v, suffix.as_str()) {
            (None, _) => s.no_value.clone(),
            (Some(v), "formatted") => short(v, &s),
            (Some(v), "raw") => format!("{v}"),
            (Some(v), "time") => time(v.round() as i64, &s),
            (Some(v), _) => commas(v, &s),
        });
    }
    if let Some(rest) = p.strip_prefix("total_") {
        let (board, t, suffix) = typed(rest)?;
        let total = snap.total(&board, &t);
        return Some(match suffix.as_str() {
            "formatted" => short(total, &s),
            "raw" => format!("{total}"),
            _ => commas(total, &s),
        });
    }
    if let Some(board) = p.strip_prefix("size_") {
        let board = snap.board_name(board)?;
        return Some(snap.size(&board).to_string());
    }
    if let Some(rest) = p.strip_prefix("reset_") {
        let (board, t) = TYPES.iter().find_map(|t| rest.strip_suffix(&format!("_{t}")).map(|b| (b.to_owned(), t.to_string())))?;
        let _ = snap.board_name(&board)?;
        let secs = store::next_reset(&t, s.weekly_on).map_or(0, |at| at - chrono::Local::now().timestamp());
        return Some(time(secs, &s));
    }
    if let Some(extra) = p.strip_prefix("extra_") {
        return Some(snap.extra(uuid, extra).unwrap_or_else(|| s.msgs.str("no-data.extra", "---")));
    }
    if let Some(rest) = p.strip_prefix("rel_") {
        // rel_<board>_<type>_<+|-><n>_<what>
        let (head, what) = rest.rsplit_once('_')?;
        let (head, offset) = head.rsplit_once('_')?;
        let sign: i64 = if offset.starts_with('-') { -1 } else { 1 };
        let off: i64 = offset.trim_start_matches(['+', '-']).parse().ok()?;
        let (board, t) = TYPES.iter().find_map(|t| head.strip_suffix(&format!("_{t}")).map(|b| (b.to_owned(), t.to_string())))?;
        let board = snap.board_name(&board)?;
        let mine = snap.position(&board, &t, uuid)? as i64;
        let target = mine + sign * off;
        if target < 1 {
            return Some(s.msgs.str(&format!("no-data.rel.{what}"), "---"));
        }
        let e = snap.top(&board, &t, (target - 1) as usize);
        return Some(match (e, what) {
            (_, "position") => target.to_string(),
            (None, _) => s.msgs.str(&format!("no-data.rel.{what}"), "---"),
            (Some(e), "name") => e.name.clone(),
            (Some(e), "displayname") => e.display.clone(),
            (Some(e), "prefix") => e.prefix.clone(),
            (Some(e), "suffix") => e.suffix.clone(),
            (Some(e), "color") => color_of(&e.prefix),
            (Some(e), "rawvalue") => format!("{}", e.value),
            (Some(e), "time") => time(e.value.round() as i64, &s),
            (Some(e), _) => commas(e.value, &s),
        });
    }
    None
}

// ---- Signs ----

#[derive(Clone, Debug)]
struct SignDisplay {
    world: String,
    x: i32,
    y: i32,
    z: i32,
    board: String,
    position: usize,
    kind: String,
    last: Vec<String>,
}

fn signs_path() -> std::path::PathBuf {
    plugin().map(|p| p.get_data_folder()).unwrap_or_default().join("displays.json")
}

fn load_signs() {
    let list: Vec<SignDisplay> = std::fs::read_to_string(signs_path())
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["signs"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|s| {
            Some(SignDisplay {
                world: s["world"].as_str()?.to_owned(),
                x: s["x"].as_i64()? as i32,
                y: s["y"].as_i64()? as i32,
                z: s["z"].as_i64()? as i32,
                board: s["board"].as_str()?.to_owned(),
                position: s["position"].as_u64()? as usize,
                kind: s["type"].as_str().unwrap_or("alltime").to_owned(),
                last: Vec::new(),
            })
        })
        .collect();
    *SIGNS.lock().unwrap_or_else(|e| e.into_inner()) = list;
}

fn save_signs() {
    let list: Vec<serde_json::Value> = SIGNS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|s| serde_json::json!({ "world": s.world, "x": s.x, "y": s.y, "z": s.z, "board": s.board, "position": s.position, "type": s.kind }))
        .collect();
    let text = serde_json::to_string_pretty(&serde_json::json!({ "signs": list })).unwrap_or_default();
    let path = signs_path();
    std::thread::spawn(move || {
        let _ = std::fs::write(path, text);
    });
}

fn update_signs() {
    let s = settings();
    let snap = store::snapshot();
    let mut signs = SIGNS.lock().unwrap_or_else(|e| e.into_inner());
    for sign in signs.iter_mut() {
        let e = snap.top(&sign.board, &sign.kind, sign.position.saturating_sub(1));
        let (name, value) = e.map_or_else(|| (s.no_name.clone(), s.no_value.clone()), |e| (e.name.clone(), commas(e.value, &s)));
        let value_name = s.value_names.get(&sign.board).cloned().unwrap_or_default();
        let lines: Vec<String> = s
            .msgs
            .list("signs.top.default")
            .iter()
            .take(4)
            .map(|l| horizon_bukkit::chat_color::translate_alternate_color_codes('&', &l.replace("{POSITION}", &sign.position.to_string()).replace("{NAME}", &name).replace("{VALUE}", &value).replace("{VALUENAME}", &value_name)))
            .collect();
        if lines == sign.last {
            continue;
        }
        let Some(world) = horizon_bukkit::get_world(&sign.world) else { continue };
        if !world.is_chunk_loaded(sign.x >> 4, sign.z >> 4) {
            continue;
        }
        let block = world.get_block_at(sign.x, sign.y, sign.z);
        if !block.get_type().name().ends_with("SIGN") {
            continue;
        }
        let mut state: horizon_bukkit::blocks::Sign = horizon_bukkit::block_data::StateClass::of(block.tile());
        for (i, l) in lines.iter().enumerate() {
            state.set_line(i, l);
        }
        state.0.update();
        sign.last = lines;
    }
}

// ---- Commands ----

fn types_list() -> String {
    TYPES.join(", ")
}

fn command(sender: &CommandSender, args: &[&str]) -> bool {
    let sub = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let a = args.get(1..).unwrap_or(&[]);
    if !sub.is_empty() && !sender.has_permission(&format!("ajleaderboards.commands.{sub}")) && !sender.has_permission("ajleaderboards.use") {
        sender.send_message(&msg("noperm"));
        return true;
    }
    let say = |t: &str| sender.send_message(&horizon_bukkit::chat_color::translate_alternate_color_codes('&', t));
    match sub.as_str() {
        "add" => match a.first() {
            Some(ph) => {
                let ph = ph.trim_matches('%').to_owned();
                if let Some(p) = sender.as_player()
                    && hooks::placeholder(&ph, Some(p)).and_then(|v| parse_number(&v)).is_none()
                {
                    say(&format!("&cThe placeholder &f%{ph}%&c does not give a number for you (is the plugin that provides it installed?). Adding it anyway."));
                }
                if store::add_board(&ph) {
                    say(&format!("&aAdded leaderboard for &f{ph}&a!"));
                    update_players(&horizon_bukkit::get_online_players());
                } else {
                    say(&format!("&cThe board &f{ph}&c already exists."));
                }
            }
            None => say("&cPlease provide a placeholder to track.\n&7Usage: /ajlb add <placeholder>"),
        },
        "remove" => match a.first() {
            Some(b) if a.get(1).is_some_and(|c| c.eq_ignore_ascii_case("confirm")) => {
                if store::remove_board(b) {
                    say(&format!("&aRemoved the &f{b}&a leaderboard!"));
                } else {
                    say(&format!("&cThe board &f{b}&c does not exist."));
                }
            }
            Some(b) => say(&format!("&eAre you sure you want to remove the &f{b}&e leaderboard and all its data? Run &f/ajlb remove {b} confirm&e to confirm.")),
            None => say("&cUsage: /ajlb remove <board>"),
        },
        "list" => {
            let boards = store::boards();
            say(&format!("&6Boards ({}):", boards.len()));
            for b in boards {
                say(&format!("&7- &e{b} &7({} players)", store::snapshot().size(&b)));
            }
        }
        "update" => match (a.first(), a.get(1)) {
            (Some(_), Some(name)) => match horizon_bukkit::get_player(name) {
                Some(p) => {
                    update_players(&[p]);
                    say(&format!("&aUpdated &f{name}&a."));
                }
                None => say(&format!("&cPlayer &f{name}&c is not online.")),
            },
            _ => {
                update_players(&horizon_bukkit::get_online_players());
                say("&aUpdated all online players.");
            }
        },
        "updateplayer" => match a.get(1).or(a.first()).and_then(|n| horizon_bukkit::get_player(n)) {
            Some(p) => {
                update_players(&[p]);
                say("&aUpdated that player.");
            }
            None => say("&cUsage: /ajlb updateplayer <board> <player> (the player must be online)"),
        },
        "removeplayer" => match a.first() {
            Some(name) => {
                let n = store::remove_player(name, a.get(1).copied());
                say(&format!("&aRemoved &f{name}&a from {n} board(s)."));
            }
            None => say("&cUsage: /ajlb removeplayer <player> [board]"),
        },
        "reset" => match (a.first(), a.get(1).map(|t| t.to_ascii_lowercase())) {
            (Some(b), Some(t)) if TYPES.contains(&t.as_str()) => {
                if store::reset(b, &t) {
                    say(&format!("&aReset the &f{t}&a values of &f{b}&a."));
                } else {
                    say(&format!("&cThe board &f{b}&c does not exist."));
                }
            }
            _ => say(&format!("&cUsage: /ajlb reset <board> <type> &7(types: {})", types_list())),
        },
        "reload" => {
            *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(load_settings()));
            hooks::refresh();
            say(&msg("commands.reload.success"));
        }
        "export" => {
            let file = a.first().map_or_else(|| "export.json".to_owned(), |f| (*f).to_owned());
            let path = plugin().map(|p| p.get_data_folder()).unwrap_or_default().join(&file);
            if path.exists() {
                say(&msg("commands.export.fileexists").replace("{FILE}", &file));
            } else {
                store::export(&path);
                say(&msg("commands.export.success").replace("{FILE}", &file));
            }
        }
        "import" => {
            let file = a.first().map_or_else(|| "export.json".to_owned(), |f| (*f).to_owned());
            let path = plugin().map(|p| p.get_data_folder()).unwrap_or_default().join(&file);
            if !path.exists() {
                say(&msg("commands.import.nofile").replace("{FILE}", &file));
            } else if store::import(&path) {
                say(&msg("commands.import.success").replace("{FILE}", &file));
            } else {
                say(&msg("commands.import.fail"));
            }
        }
        "signs" => match (a.first().map(|x| x.to_ascii_lowercase()).as_deref(), sender.as_player()) {
            (Some("add"), Some(p)) => {
                let (Some(board), Some(pos)) = (a.get(1), a.get(2).and_then(|n| n.parse::<usize>().ok()).filter(|n| *n >= 1)) else {
                    say("&cUsage: /ajlb signs add <board> <position> [type]");
                    return true;
                };
                let kind = a.get(3).map(|t| t.to_ascii_lowercase()).filter(|t| TYPES.contains(&t.as_str())).unwrap_or_else(|| "alltime".into());
                let Some(board) = store::snapshot().board_name(&board.to_ascii_lowercase()) else {
                    say(&format!("&cThe board &f{board}&c does not exist."));
                    return true;
                };
                let Some(block) = p.get_target_block_exact(10, "NEVER") else {
                    say("&cLook at a sign first.");
                    return true;
                };
                if !block.get_type().name().ends_with("SIGN") {
                    say("&cLook at a sign first.");
                    return true;
                }
                let l = block.get_location();
                let sign = SignDisplay { world: p.get_world().get_name().to_owned(), x: l.get_block_x(), y: l.get_block_y(), z: l.get_block_z(), board, position: pos, kind, last: Vec::new() };
                let mut signs = SIGNS.lock().unwrap_or_else(|e| e.into_inner());
                signs.retain(|s| !(s.world == sign.world && s.x == sign.x && s.y == sign.y && s.z == sign.z));
                signs.push(sign);
                drop(signs);
                save_signs();
                say("&aSign added!");
            }
            (Some("remove"), Some(p)) => {
                let Some(block) = p.get_target_block_exact(10, "NEVER") else {
                    say("&cLook at a sign first.");
                    return true;
                };
                let l = block.get_location();
                let world = p.get_world().get_name().to_owned();
                let mut signs = SIGNS.lock().unwrap_or_else(|e| e.into_inner());
                let before = signs.len();
                signs.retain(|s| !(s.world == world && s.x == l.get_block_x() && s.y == l.get_block_y() && s.z == l.get_block_z()));
                let removed = before != signs.len();
                drop(signs);
                save_signs();
                say(if removed { "&aSign removed." } else { "&cThat isn't an ajLeaderboards sign." });
            }
            (Some("list"), _) => {
                for s in SIGNS.lock().unwrap_or_else(|e| e.into_inner()).iter() {
                    say(&format!("&7- &e{} &7#{} {} &8@ {} {} {} {}", s.board, s.position, s.kind, s.world, s.x, s.y, s.z));
                }
            }
            (_, None) => say("&cThis must be run in game."),
            _ => say("&cUsage: /ajlb signs <add|remove|list>"),
        },
        "version" => say(&format!("&aajLeaderboards &f{} &7(Rust, Horizon)", env!("CARGO_PKG_VERSION"))),
        _ => {
            for l in [
                "&6ajLeaderboards &7commands:",
                "&e/ajlb add <placeholder> &7- start tracking a placeholder",
                "&e/ajlb remove <board> &7- delete a board",
                "&e/ajlb list &7- list boards",
                "&e/ajlb update [board] [player] &7- update now",
                "&e/ajlb removeplayer <player> [board] &7- remove a player",
                "&e/ajlb reset <board> <type> &7- reset a timed board",
                "&e/ajlb signs <add|remove|list> &7- sign displays",
                "&e/ajlb export|import [file] &7- move the cache",
                "&e/ajlb reload &7- reload configs",
            ] {
                say(l);
            }
        }
    }
    true
}

fn complete(args: &[&str]) -> Vec<String> {
    let typed = args.last().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let opts: Vec<String> = match (args.len(), args.first().map(|a| a.to_ascii_lowercase()).as_deref()) {
        (0 | 1, _) => ["add", "remove", "list", "update", "updateplayer", "removeplayer", "reset", "signs", "export", "import", "reload", "version"].iter().map(|s| (*s).to_owned()).collect(),
        (2, Some("remove" | "reset" | "update" | "updateplayer")) => store::boards(),
        (2, Some("signs")) => vec!["add".into(), "remove".into(), "list".into()],
        (3, Some("signs")) => store::boards(),
        (3, Some("reset")) | (5, Some("signs")) => TYPES.iter().map(|s| (*s).to_owned()).collect(),
        _ => Vec::new(),
    };
    opts.into_iter().filter(|o| o.to_ascii_lowercase().starts_with(&typed)).collect()
}

// ---- Enable / disable ----

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    let s = load_settings();
    let (refresh, sign_every) = (s.stat_refresh, s.sign_update);
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(s));
    store::start(&plugin.get_data_folder());
    load_signs();
    hooks::provide_placeholders(&plugin, "ajlb", |uuid, _name, params| answer(uuid, params));
    plugin.register_event_with("MONITOR", false, |e: &mut PlayerJoinEvent| {
        if settings().update_on_join {
            let p = e.get_player().clone();
            if let Some(pl) = crate::plugin() {
                pl.run_task_later(40, move || update_players(std::slice::from_ref(&p)));
            }
        }
    });
    plugin.register_event_with("MONITOR", false, |e: &mut PlayerQuitEvent| {
        if settings().update_on_leave {
            update_players(&[e.get_player().clone()]);
        }
    });
    plugin.set_executor("ajleaderboards", |sender, _, args| command(sender, args));
    plugin.set_tab_completer("ajleaderboards", |_, _, args| complete(args));
    plugin.run_task_timer(100, refresh, || update_players(&horizon_bukkit::get_online_players()));
    plugin.run_task_timer(40, sign_every, update_signs);
    // Timed boards roll over on the hour (the board thread checks; this wakes it).
    plugin.run_task_timer(1200, 1200, || store::send(Vec::new()));
}

fn on_disable(_plugin: Plugin) {
    store::stop();
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);

#[allow(dead_code)]
fn _snapshot_type_check(_: &Snapshot) {}

#[cfg(test)]
mod tests {
    use super::*;
    fn s() -> Settings {
        Settings {
            stat_refresh: 1200,
            sign_update: 20,
            update_on_join: true,
            update_on_leave: true,
            extras: vec![],
            reverse: vec![],
            comma: ",".into(),
            decimal: ".".into(),
            round: 2,
            value_names: HashMap::new(),
            dont_add_zero: vec![],
            require_zero_validation: true,
            weekly_on: chrono::Weekday::Sun,
            no_name: "---".into(),
            no_value: "---".into(),
            dontupdate_perm: true,
            only_update: vec![],
            msgs: Config::parse(DEF_MESSAGES),
        }
    }
    #[test]
    fn formats() {
        let s = s();
        assert_eq!(commas(1234567.891, &s), "1,234,567.89");
        assert_eq!(commas(12.0, &s), "12");
        assert_eq!(short(1234.0, &s), "1.23k");
        assert_eq!(short(5_000_000.0, &s), "5m");
        assert_eq!(time(3661, &s), "1h 1m 1s");
        assert_eq!(parse_number("&a1,234"), Some(1234.0));
        assert_eq!(parse_number("2.5k"), Some(2500.0));
        assert_eq!(color_of("&7[&6VIP&7] &e"), "§e");
    }
}
