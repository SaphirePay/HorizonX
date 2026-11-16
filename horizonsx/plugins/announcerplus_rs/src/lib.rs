//! AnnouncerPlus 1.4.8 for Horizon, written again in Rust (from jpenilla/AnnouncerPlus, MIT). Same files
//! (`plugins/AnnouncerPlus/main.conf`, `first-join.conf`, `message-configs/*.conf`, `join-quit-configs/*.conf`),
//! same `/announcerplus` (`/ap`), same permissions.
//!
//! Timed broadcasts are one Bukkit timer per message config; join/quit features run on their events; titles,
//! action bars, boss bars and toasts share one server-thread tick ([`display`]).

mod display;

use display::{ActionBar, Bar, Title, Toast};
use horizon_bukkit::adventure::Component;
use horizon_bukkit::event::{PlayerJoinEvent, PlayerQuitEvent};
use horizon_bukkit::fam_core::BukkitTask;
use horizon_bukkit::{CommandSender, Player, Plugin, bukkit_plugin};
use horizon_kit::{hocon, hooks, mini};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static CONFIG: RwLock<Option<Arc<Configs>>> = RwLock::new(None);
static TASKS: Mutex<Vec<BukkitTask>> = Mutex::new(Vec::new());

pub(crate) fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

fn configs() -> Arc<Configs> {
    CONFIG.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

// ---- Settings ----

#[derive(Clone, Debug)]
struct Sound {
    name: String,
    source: String,
    volume: f32,
    pitch: f32,
}

#[derive(Clone, Debug, Default)]
struct Message {
    text: Vec<String>,
    action_bar: Option<ActionBar>,
    boss_bar: Option<Bar>,
    title: Option<Title>,
    toast: Option<Toast>,
    sounds: Vec<Sound>,
    random_sounds: bool,
    commands: Vec<String>,
    per_player: Vec<String>,
    as_player: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct MessageConfig {
    name: String,
    messages: Vec<Message>,
    commands: Vec<String>,
    per_player: Vec<String>,
    as_player: Vec<String>,
    interval: u64,
    interval_text: String,
    delay: u64,
    random: bool,
}

#[derive(Clone, Debug, Default)]
struct JoinQuit {
    name: Option<String>,
    permission: String,
    random_broadcast_sound: bool,
    broadcast_sounds: Vec<Sound>,
    random_sound: bool,
    sounds: Vec<Sound>,
    messages: Vec<String>,
    broadcasts: Vec<String>,
    commands: Vec<String>,
    as_player: Vec<String>,
    title: Option<Title>,
    action_bar: Option<ActionBar>,
    boss_bar: Option<Bar>,
    toast: Option<Toast>,
    quit_random_sound: bool,
    quit_sounds: Vec<Sound>,
    quit_broadcasts: Vec<String>,
    quit_commands: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct Configs {
    placeholders: Vec<(String, String)>,
    broadcasts: bool,
    join: bool,
    quit: bool,
    first_join_enabled: bool,
    random_join: Vec<(String, Vec<(String, f64)>)>,
    random_quit: Vec<(String, Vec<(String, f64)>)>,
    first_join: JoinQuit,
    messages: BTreeMap<String, MessageConfig>,
    join_quit: BTreeMap<String, JoinQuit>,
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_owned)).collect()
}
fn s(v: &Value, d: &str) -> String {
    v.as_str().map_or_else(|| d.to_owned(), str::to_owned)
}
fn i(v: &Value, d: i64) -> i64 {
    v.as_i64().or_else(|| v.as_str().and_then(|t| t.parse().ok())).unwrap_or(d)
}
fn b(v: &Value, d: bool) -> bool {
    v.as_bool().unwrap_or(d)
}
fn f(v: &Value, d: f64) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|t| t.parse().ok())).unwrap_or(d)
}

fn sounds(v: &Value) -> Vec<Sound> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| {
            Some(Sound { name: x["name"].as_str()?.to_owned(), source: s(&x["source"], "master"), volume: f(&x["volume"], 1.0) as f32, pitch: f(&x["pitch"], 1.0) as f32 })
        })
        .collect()
}

fn action_bar(v: &Value) -> Option<ActionBar> {
    let a = ActionBar { seconds: i(&v["duration-seconds"], 6), fade: b(&v["enable-fade-out"], false), text: s(&v["text"], "") };
    a.enabled().then_some(a)
}
fn title(v: &Value) -> Option<Title> {
    let t = Title {
        fade_in: i(&v["fade-in-seconds"], 1),
        stay: i(&v["duration-seconds"], 5),
        fade_out: i(&v["fade-out-seconds"], 1),
        title: s(&v["title"], ""),
        subtitle: s(&v["subtitle"], ""),
    };
    t.enabled().then_some(t)
}
fn boss_bar(v: &Value) -> Option<Bar> {
    let b = Bar {
        seconds: i(&v["duration-seconds"], 12),
        text: s(&v["text"], ""),
        color: s(&v["color"], "YELLOW"),
        overlay: s(&v["overlay"], "PROGRESS"),
        fill: s(&v["fill-mode"], "DRAIN"),
    };
    b.enabled().then_some(b)
}
fn toast(v: &Value) -> Option<Toast> {
    let t = Toast {
        icon: s(&v["icon"], "DIAMOND"),
        enchanted: b(&v["icon-enchanted"], false),
        model_data: i(&v["icon-custom-model-data"], -1),
        item_model: s(&v["icon-item-model"], "announcerplus:disabled"),
        header: s(&v["header"], ""),
        footer: s(&v["footer"], ""),
        frame: s(&v["frame"], "GOAL"),
    };
    t.enabled().then_some(t)
}

/// `SimpleDuration`: "3 minutes", "10m", "30 sec", "2hrs" -> ticks.
fn duration(text: &str) -> Option<u64> {
    let t = text.trim();
    let split = t.find(|c: char| !c.is_ascii_digit())?;
    let n: u64 = t[..split].parse().ok()?;
    let unit = t[split..].trim().to_ascii_lowercase();
    let per = match unit.as_str() {
        "s" | "second" | "seconds" | "sec" | "secs" => 20,
        "m" | "minute" | "minutes" | "min" | "mins" => 1200,
        "h" | "hour" | "hours" | "hr" | "hrs" => 72000,
        _ => return None,
    };
    Some(n * per)
}

fn message_config(name: &str, v: &Value) -> MessageConfig {
    let messages = v["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| Message {
            text: strs(&m["message-text"]),
            action_bar: action_bar(&m["action-bar"]),
            boss_bar: boss_bar(&m["boss-bar"]),
            title: title(&m["title"]),
            toast: toast(&m["toast"]),
            sounds: sounds(&m["sounds"]),
            random_sounds: b(&m["sounds-randomized"], true),
            commands: strs(&m["commands"]),
            per_player: strs(&m["per-player-commands"]),
            as_player: strs(&m["as-player-commands"]),
        })
        .collect();
    let interval_text = s(&v["interval-time"], "3 minutes");
    MessageConfig {
        name: name.to_owned(),
        messages,
        commands: strs(&v["every-broadcast-commands"]),
        per_player: strs(&v["every-broadcast-per-player-commands"]),
        as_player: strs(&v["every-broadcast-as-player-commands"]),
        interval: duration(&interval_text).unwrap_or(3600).max(1),
        interval_text,
        delay: v["startup-delay"].as_str().and_then(duration).unwrap_or(0),
        random: b(&v["random-message-order"], false),
    }
}

fn join_quit(name: Option<&str>, v: &Value) -> JoinQuit {
    let j = &v["join-section"];
    let q = &v["quit-section"];
    JoinQuit {
        name: name.map(str::to_owned),
        permission: s(&v["visible-permission"], ""),
        random_broadcast_sound: b(&j["randomize-join-broadcast-sounds"], true),
        broadcast_sounds: sounds(&j["join-broadcast-sounds"]),
        random_sound: b(&j["randomize-join-sounds"], true),
        sounds: sounds(&j["join-sounds"]),
        messages: strs(&j["join-messages"]),
        broadcasts: strs(&j["join-broadcasts"]),
        commands: strs(&j["join-commands"]),
        as_player: strs(&j["as-player-join-commands"]),
        title: title(&j["title-settings"]),
        action_bar: action_bar(&j["action-bar-settings"]),
        boss_bar: boss_bar(&j["boss-bar-settings"]),
        toast: toast(&j["toast-settings"]),
        quit_random_sound: b(&q["randomize-quit-sounds"], true),
        quit_sounds: sounds(&q["quit-sounds"]),
        quit_broadcasts: strs(&q["quit-broadcasts"]),
        quit_commands: strs(&q["quit-commands"]),
    }
}

// ---- Default files ----

fn snd(name: &str) -> Value {
    json!({ "name": name, "source": "master", "pitch": 1.0, "volume": 1.0 })
}

fn element_defaults() -> (Value, Value, Value, Value) {
    (
        json!({ "duration-seconds": 6, "enable-fade-out": false, "text": "" }),
        json!({ "color": "YELLOW", "duration-seconds": 12, "fill-mode": "DRAIN", "overlay": "PROGRESS", "text": "" }),
        json!({ "duration-seconds": 5, "fade-in-seconds": 1, "fade-out-seconds": 1, "subtitle": "", "title": "" }),
        json!({ "footer": "", "frame": "GOAL", "header": "", "icon": "DIAMOND", "icon-custom-model-data": -1, "icon-enchanted": false, "icon-item-model": "announcerplus:disabled" }),
    )
}

fn message_default(text: &[&str], set: impl FnOnce(&mut Value)) -> Value {
    let (a, bb, t, to) = element_defaults();
    let mut m = json!({
        "action-bar": a, "as-player-commands": [], "boss-bar": bb, "commands": [], "message-text": text,
        "per-player-commands": [], "sounds": [], "sounds-randomized": true, "title": t, "toast": to,
    });
    set(&mut m);
    m
}

fn demo_config() -> Value {
    json!({
        "every-broadcast-as-player-commands": [],
        "every-broadcast-commands": [],
        "every-broadcast-per-player-commands": [],
        "interval-time": "3 minutes",
        "messages": [
            message_default(&["<center><rainbow>Test AnnouncerPlus broadcast!"], |_| {}),
            message_default(&[], |m| m["boss-bar"] = json!({ "color": "{animate:flash:YELLOW:PURPLE:40}", "duration-seconds": 25, "fill-mode": "DRAIN", "overlay": "PROGRESS",
                "text": "<green>-| <white>{animate:scrolltext:Hello this is an example Boss Bar announcement:20:3}</white> |-" })),
            message_default(&[
                "{prefix1} 1. <gradient:blue:green:blue>Multi-line test AnnouncerPlus broadcast",
                "{prefix1} 2. <gradient:red:gold:red>Line number two of three",
                "{prefix1} 3. <bold><rainbow>this is the last line (line 3)",
            ], |m| m["toast"] = json!({ "footer": "<rainbow>This is a Toast message!", "frame": "CHALLENGE", "header": "<gradient:green:blue><bold><italic>AnnouncerPlus",
                "icon": "NETHER_STAR", "icon-custom-model-data": -1, "icon-enchanted": false, "icon-item-model": "announcerplus:disabled" })),
            message_default(&["{prefix1} Test <gradient:blue:aqua>AnnouncerPlus</gradient> broadcast with sound<green>!"], |m| {
                m["sounds"] = json!([snd("minecraft:entity.strider.happy"), snd("minecraft:entity.villager.ambient"), snd("minecraft:block.note_block.cow_bell")]);
            }),
            message_default(&["{prefix1} Use <click:run_command:/ap about><hover:show_text:'<rainbow>Click to run!'><rainbow>/ap about</rainbow></hover></click> to check the plugin version"], |m| {
                m["action-bar"] = json!({ "duration-seconds": 15, "enable-fade-out": true,
                    "text": "<{animate:randomcolor:pulse:25}>-| <white>{animate:scrolltext:Hello there this is some very long text being displayed in a scrolling window!! =):20:3}</white> |-" });
            }),
            message_default(&["<bold><italic>Hello,</italic></bold> {nick} {prefix1} {r}!!!!!!!!!{rc}"], |m| {
                m["title"] = json!({ "duration-seconds": 13, "fade-in-seconds": 1, "fade-out-seconds": 2,
                    "subtitle": "<{animate:pulse:red:blue:10}>{animate:type:This is a test... typing...:6}",
                    "title": "<gradient:green:blue:green:{animate:scroll:0.1}>||||||||||||||||||||||||||||||||||||||||||||" });
            }),
            message_default(&["<center><gradient:red:blue>Centered text Example"], |m| {
                m["boss-bar"] = json!({ "color": "PINK", "duration-seconds": 25, "fill-mode": "DRAIN", "overlay": "PROGRESS", "text": "<bold>This is an example <italic><gradient:blue:light_purple>Boss Bar" });
            }),
        ],
        "random-message-order": false,
        "remove-disabled-message-elements": false,
        "remove-duplicate-comments": true,
        "startup-delay": "0s",
        "version": 1,
    })
}

fn join_quit_default() -> Value {
    let (_, bb, _, _) = element_defaults();
    json!({
        "join-section": {
            "action-bar-settings": { "duration-seconds": 8, "enable-fade-out": false, "text": "<gradient:green:blue:green:{animate:scroll:0.1}>|||||||||||||||||||||||||||||||||||||||</gradient>" },
            "as-player-join-commands": [],
            "boss-bar-settings": bb,
            "join-broadcast-sounds": [snd("minecraft:entity.enderman.teleport")],
            "join-broadcasts": ["<hover:show_text:'<yellow>Username</yellow><gray>:</gray> {user}'>{nick}</hover> <yellow>joined the game"],
            "join-commands": [],
            "join-messages": [
                "<hover:show_text:'<yellow>Username</yellow><gray>:</gray> {user}'>{nick}</hover> <yellow>joined the game",
                "<center><rainbow><italic>Welcome,</rainbow> {user}<yellow>!",
                "<center><gradient:black:white:black>------------------------------------</gradient>",
                "This server is using <blue>Announcer<italic>Plus<reset>!",
                "<gradient:green:white>Configure these messages by editing the config files!",
            ],
            "join-sounds": [snd("minecraft:entity.strider.happy"), snd("minecraft:entity.villager.ambient"), snd("minecraft:block.note_block.cow_bell")],
            "randomize-join-broadcast-sounds": true,
            "randomize-join-sounds": true,
            "title-settings": { "duration-seconds": 7, "fade-in-seconds": 1, "fade-out-seconds": 1,
                "subtitle": "<{animate:pulse:red:blue:yellow:green:10}>{user}",
                "title": "<bold><italic><gradient:green:blue:green:{animate:scroll:0.1}>Welcome</gradient><yellow>{animate:flash:!:!!:!!!:10}" },
            "toast-settings": { "footer": "<rainbow>Welcome to the server!", "frame": "CHALLENGE", "header": "<gradient:green:blue><bold><italic>AnnouncerPlus",
                "icon": "DIAMOND", "icon-custom-model-data": -1, "icon-enchanted": false, "icon-item-model": "announcerplus:disabled" },
        },
        "quit-section": {
            "quit-broadcasts": ["<hover:show_text:'<yellow>Username</yellow><gray>:</gray> {user}'>{nick}</hover> <yellow>left the game"],
            "quit-commands": [],
            "quit-sounds": [snd("minecraft:entity.enderman.teleport")],
            "randomize-quit-sounds": true,
        },
        "remove-duplicate-comments": true,
        "version": 1,
        "visible-permission": "",
    })
}

fn main_default() -> Value {
    json!({
        "check-for-updates": false,
        "custom-placeholders": { "nick": "%essentials_nickname%", "prefix1": "<bold><blue>[<green>!</green>]</blue></bold>", "r": "<rainbow>", "rc": "</rainbow>", "user": "%player_name%" },
        "enable-broadcasts": true,
        "first-join-config-enabled": false,
        "join-features": true,
        "quit-features": true,
        "random-join-configs": { "demo": [{ "config-name": "config_a", "weight": 0.1 }, { "config-name": "config_b", "weight": 0.2 }] },
        "random-quit-configs": { "demo": [{ "config-name": "config_a", "weight": 0.2 }, { "config-name": "config_b", "weight": 0.1 }] },
    })
}

fn read_or_write(path: &Path, default: impl FnOnce() -> Value, header: &str) -> Result<Value, String> {
    if !path.exists() {
        let _ = std::fs::write(path, hocon::write(&default(), header));
    }
    hocon::read(path)
}

fn conf_files(dir: &Path) -> Vec<(String, std::path::PathBuf)> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "conf"))
        .map(|p| (p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), p))
        .collect();
    out.sort();
    out
}

fn weighted(v: &Value) -> Vec<(String, Vec<(String, f64)>)> {
    v.as_object()
        .into_iter()
        .flatten()
        .map(|(k, list)| (k.clone(), list.as_array().into_iter().flatten().map(|p| (s(&p["config-name"], "default"), f(&p["weight"], 0.1))).collect()))
        .collect()
}

fn add_perm(name: &str) {
    let pm = horizon_bukkit::fam_core::get_plugin_manager();
    pm.remove_permission(name);
    pm.add_permission(&horizon_bukkit::config2::Permission::new(name, "false", ""));
}

fn load(dir: &Path) -> Result<Configs, String> {
    let _ = std::fs::create_dir_all(dir);
    let main = read_or_write(&dir.join("main.conf"), main_default, "AnnouncerPlus (Horizon, Rust)")?;
    let first = read_or_write(
        &dir.join("first-join.conf"),
        || {
            let mut d = join_quit_default();
            if let Some(o) = d.as_object_mut() {
                o.remove("quit-section");
            }
            d
        },
        "If enabled in main.conf, this join config will be used when players join the server for the first time.\nAll other join configs will be skipped for first-join if this is enabled.",
    )?;
    let msg_dir = dir.join("message-configs");
    let _ = std::fs::create_dir_all(&msg_dir);
    if conf_files(&msg_dir).is_empty() {
        let _ = std::fs::write(
            msg_dir.join("demo.conf"),
            hocon::write(&demo_config(), "For a player to get these messages give them the announcerplus.messages.demo permission\n  If EssentialsX is installed, then giving a player the announcerplus.messages.demo.afk permission\n  will stop them from receiving these messages while afk"),
        );
    }
    let jq_dir = dir.join("join-quit-configs");
    let _ = std::fs::create_dir_all(&jq_dir);
    if conf_files(&jq_dir).is_empty() {
        let _ = std::fs::write(
            jq_dir.join("default.conf"),
            hocon::write(&join_quit_default(), "To give a player these join/quit messages give them the announcerplus.join.default\n  and announcerplus.quit.default permissions"),
        );
    }
    let mut messages = BTreeMap::new();
    for (name, path) in conf_files(&msg_dir) {
        let v = hocon::read(&path).map_err(|e| format!("Failed to load message config: {name}.conf. {e}"))?;
        add_perm(&format!("announcerplus.messages.{name}"));
        messages.insert(name.clone(), message_config(&name, &v));
    }
    let mut jq = BTreeMap::new();
    for (name, path) in conf_files(&jq_dir) {
        let v = hocon::read(&path).map_err(|e| format!("Failed to load join quit config: {name}.conf. {e}"))?;
        add_perm(&format!("announcerplus.join.{name}"));
        add_perm(&format!("announcerplus.quit.{name}"));
        jq.insert(name.clone(), join_quit(Some(&name), &v));
    }
    let random_join = weighted(&main["random-join-configs"]);
    let random_quit = weighted(&main["random-quit-configs"]);
    for (k, _) in &random_join {
        add_perm(&format!("announcerplus.randomjoin.{k}"));
    }
    for (k, _) in &random_quit {
        add_perm(&format!("announcerplus.randomquit.{k}"));
    }
    Ok(Configs {
        placeholders: main["custom-placeholders"].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), s(v, ""))).collect(),
        broadcasts: b(&main["enable-broadcasts"], true),
        join: b(&main["join-features"], true),
        quit: b(&main["quit-features"], true),
        first_join_enabled: b(&main["first-join-config-enabled"], false),
        random_join,
        random_quit,
        first_join: join_quit(None, &first),
        messages,
        join_quit: jq,
    })
}

// ---- Text ----

/// `ConfigManager.parse`: custom `{placeholders}`, no `§` codes, `%placeholders%` for a player, `<center>`.
pub(crate) fn parse(player: Option<&Player>, text: &str) -> String {
    let cfg = configs();
    let mut msg = text.to_owned();
    for (token, rep) in &cfg.placeholders {
        msg = msg.replace(&format!("{{{token}}}"), rep);
    }
    if msg.contains('§') {
        if let Some(p) = plugin() {
            p.warning(&format!("Legacy color codes have been detected in a message. This is not supported behavior. Message: '{}'", msg.replace('§', "&")));
        }
        msg = msg.replace('§', "");
    }
    if let Some(p) = player {
        // Values may carry § codes (nicknames): turned into tags so MiniMessage reads them.
        msg = hooks::placeholders(&msg, Some(p));
        if msg.contains('§') {
            msg = legacy_tags(&msg);
        }
    }
    if let Some(rest) = msg.strip_prefix("<center>") {
        let spaces = display::center_prefix(&mini::parse(rest));
        return format!("{spaces}{rest}");
    }
    msg
}

fn legacy_tags(text: &str) -> String {
    const NAMES: [&str; 16] = [
        "black", "dark_blue", "dark_green", "dark_aqua", "dark_red", "dark_purple", "gold", "gray", "dark_gray", "blue", "green", "aqua", "red", "light_purple", "yellow", "white",
    ];
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '§' && i + 1 < chars.len() {
            let c = chars[i + 1].to_ascii_lowercase();
            if c == 'x' && i + 13 < chars.len() {
                let hex: String = (0..6).map(|k| chars[i + 3 + k * 2]).collect();
                out.push_str(&format!("<#{hex}>"));
                i += 14;
                continue;
            }
            let tag = match c {
                d if d.is_ascii_hexdigit() => NAMES[d.to_digit(16).unwrap_or(15) as usize],
                'l' => "bold",
                'o' => "italic",
                'n' => "underlined",
                'm' => "strikethrough",
                'k' => "obfuscated",
                'r' => "reset",
                _ => "",
            };
            if !tag.is_empty() {
                out.push_str(&format!("<{tag}>"));
            }
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn render(player: Option<&Player>, text: &str) -> Component {
    Component(mini::parse(&parse(player, text)))
}

fn play(player: &Player, list: &[Sound], random: bool) {
    if list.is_empty() {
        return;
    }
    let one = |s: &Sound| {
        let cat = match s.source.to_ascii_uppercase().as_str() {
            "RECORD" => "RECORDS".to_owned(),
            other => other.to_owned(),
        };
        player.play_sound_in(&player.get_location(), &s.name, &cat, s.volume, s.pitch);
    };
    if random {
        one(&list[(display::rand_f() * list.len() as f64) as usize % list.len()]);
    } else {
        list.iter().for_each(one);
    }
}

fn console(line: &str) {
    horizon_bukkit::fam_core::dispatch_command_as(&horizon_bukkit::get_console_sender(), line);
}

fn show_elements(player: &Player, a: &Option<ActionBar>, bb: &Option<Bar>, t: &Option<Title>, to: &Option<Toast>) {
    if let Some(a) = a {
        display::show_action_bar(player, a);
    }
    if let Some(bb) = bb {
        display::show_bar(player, bb);
    }
    if let Some(t) = t {
        display::show_title(player, t);
    }
    if let Some(to) = to {
        display::show_toast(player, to);
    }
}

// ---- Timed broadcasts ----

fn broadcast(cfg: &MessageConfig, m: &Message) {
    let perm = format!("announcerplus.messages.{}", cfg.name);
    for player in horizon_bukkit::get_online_players() {
        if !player.has_permission(&perm) {
            continue;
        }
        for line in &m.text {
            player.send_component(&render(Some(&player), line));
        }
        play(&player, &m.sounds, m.random_sounds);
        show_elements(&player, &m.action_bar, &m.boss_bar, &m.title, &m.toast);
        for c in m.per_player.iter().chain(&cfg.per_player) {
            console(&parse(Some(&player), c));
        }
        for c in m.as_player.iter().chain(&cfg.as_player) {
            player.perform_command(&parse(Some(&player), c));
        }
    }
    for c in m.commands.iter().chain(&cfg.commands) {
        console(&parse(None, c));
    }
}

fn shuffled(cfg: &MessageConfig) -> VecDeque<usize> {
    let mut order: Vec<usize> = (0..cfg.messages.len()).collect();
    if cfg.random {
        for k in (1..order.len()).rev() {
            let j = (display::rand_f() * (k + 1) as f64) as usize % (k + 1);
            order.swap(k, j);
        }
    }
    order.into()
}

fn start_broadcasts() {
    let Some(p) = plugin() else { return };
    let cfg = configs();
    let mut tasks = TASKS.lock().unwrap_or_else(|e| e.into_inner());
    for t in tasks.drain(..) {
        t.cancel();
    }
    if !cfg.broadcasts {
        return;
    }
    for name in cfg.messages.keys() {
        let name = name.clone();
        let Some(mc) = cfg.messages.get(&name) else { continue };
        if mc.messages.is_empty() {
            continue;
        }
        let mut queue = shuffled(mc);
        let task = p.run_task_timer(mc.delay.max(1), mc.interval, move || {
            let cfg = configs();
            let Some(mc) = cfg.messages.get(&name) else { return };
            if queue.is_empty() {
                queue = shuffled(mc);
            }
            if let Some(m) = queue.pop_front().and_then(|i| mc.messages.get(i)) {
                broadcast(mc, m);
            }
        });
        tasks.push(task);
    }
}

// ---- Join / quit ----

fn weighted_pick(list: &[(String, f64)]) -> Option<String> {
    let total: f64 = list.iter().map(|(_, w)| w.max(0.0)).sum();
    if list.is_empty() || total <= 0.0 {
        return list.first().map(|(n, _)| n.clone());
    }
    let mut r = display::rand_f() * total;
    for (n, w) in list {
        r -= w.max(0.0);
        if r <= 0.0 {
            return Some(n.clone());
        }
    }
    list.last().map(|(n, _)| n.clone())
}

fn on_join_config(jq: &JoinQuit, player: &Player) {
    if let Some(name) = &jq.name
        && !player.has_permission(&format!("announcerplus.join.{name}"))
    {
        return;
    }
    for line in &jq.messages {
        player.send_component(&render(Some(player), line));
    }
    show_elements(player, &jq.action_bar, &jq.boss_bar, &jq.title, &jq.toast);
    play(player, &jq.sounds, jq.random_sound);
    let jq = jq.clone();
    let player = player.clone();
    if let Some(p) = plugin() {
        p.run_task_later(3, move || {
            if !player.is_online() {
                return;
            }
            let lines: Vec<Component> = jq.broadcasts.iter().map(|l| render(Some(&player), l)).collect();
            for other in horizon_bukkit::get_online_players() {
                if other.get_name() == player.get_name() || !(jq.permission.is_empty() || other.has_permission(&jq.permission)) {
                    continue;
                }
                for l in &lines {
                    other.send_component(l);
                }
                play(&other, &jq.broadcast_sounds, jq.random_broadcast_sound);
            }
            for c in &jq.commands {
                console(&parse(Some(&player), c));
            }
            for c in &jq.as_player {
                player.perform_command(&parse(Some(&player), c));
            }
        });
    }
}

fn on_quit_config(jq: &JoinQuit, player: &Player) {
    let Some(name) = &jq.name else { return };
    if !player.has_permission(&format!("announcerplus.quit.{name}")) {
        return;
    }
    let lines: Vec<Component> = jq.quit_broadcasts.iter().map(|l| render(Some(player), l)).collect();
    for other in horizon_bukkit::get_online_players() {
        if other.get_name() == player.get_name() || !(jq.permission.is_empty() || other.has_permission(&jq.permission)) {
            continue;
        }
        for l in &lines {
            other.send_component(l);
        }
        play(&other, &jq.quit_sounds, jq.quit_random_sound);
    }
    for c in &jq.quit_commands {
        console(&parse(Some(player), c));
    }
}

fn on_join(event: &mut PlayerJoinEvent) {
    let cfg = configs();
    if !cfg.join {
        return;
    }
    event.set_join_message_component(None);
    let player = event.get_player().clone();
    if cfg.first_join_enabled && !player.has_played_before() {
        on_join_config(&cfg.first_join, &player);
        return;
    }
    for (key, list) in &cfg.random_join {
        if key != "demo"
            && player.has_permission(&format!("announcerplus.randomjoin.{key}"))
            && let Some(jq) = weighted_pick(list).and_then(|n| cfg.join_quit.get(&n))
        {
            on_join_config(jq, &player);
        }
    }
    for jq in cfg.join_quit.values() {
        on_join_config(jq, &player);
    }
}

fn on_quit(event: &mut PlayerQuitEvent) {
    let player = event.get_player().clone();
    display::quit(&player.get_unique_id());
    let cfg = configs();
    if !cfg.quit {
        return;
    }
    event.set_quit_message_component(None);
    for (key, list) in &cfg.random_quit {
        if key != "demo"
            && player.has_permission(&format!("announcerplus.randomquit.{key}"))
            && let Some(jq) = weighted_pick(list).and_then(|n| cfg.join_quit.get(&n))
        {
            on_quit_config(jq, &player);
        }
    }
    for jq in cfg.join_quit.values() {
        on_quit_config(jq, &player);
    }
}

// ---- /announcerplus ----

const PREFIX: &str = "<white>[</white><gradient:#ff4f19:#f7ff19>AnnouncerPlus</gradient><white>]</white>";

fn send(sender: &CommandSender, text: &str) {
    sender.send_component(&Component(mini::parse(text)));
}

/// Words, with "quoted strings" kept whole (Cloud's quoted string arguments).
fn words(args: &[&str]) -> Vec<String> {
    let line = args.join(" ");
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut had = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                had = true;
            }
            ' ' if !quoted => {
                if !cur.is_empty() || had {
                    out.push(std::mem::take(&mut cur));
                }
                had = false;
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() || had {
        out.push(cur);
    }
    out
}

fn targets(prefix: &str, sender: &CommandSender, arg: Option<&str>) -> Result<Vec<Player>, String> {
    match prefix {
        "parse" => sender.as_player().map(|p| vec![p.clone()]).ok_or_else(|| "This command must be run by a player.".to_owned()),
        "send" => {
            let arg = arg.ok_or("Missing players")?;
            if arg == "@a" || arg == "*" {
                return Ok(horizon_bukkit::get_online_players());
            }
            let found: Vec<Player> = arg.split(',').filter_map(|n| horizon_bukkit::get_player(n.trim()).filter(|p| p.get_name().eq_ignore_ascii_case(n.trim()))).collect();
            if found.is_empty() { Err(format!("No player found for '{arg}'")) } else { Ok(found) }
        }
        _ => {
            let arg = arg.ok_or("Missing world")?;
            let all = horizon_bukkit::get_online_players();
            if arg == "all" || arg == "*" {
                return Ok(all);
            }
            Ok(all.into_iter().filter(|p| p.get_world().get_name().eq_ignore_ascii_case(arg)).collect())
        }
    }
}

fn element_command(sender: &CommandSender, prefix: &str, kind: &str, rest: &[String]) -> Result<(), String> {
    if !sender.has_permission(&format!("announcerplus.command.{prefix}.{kind}")) {
        return Err("I'm sorry, but you do not have permission to perform this command.".into());
    }
    let (target_arg, rest) = if prefix == "parse" { (None, rest) } else { (rest.first().map(String::as_str), rest.get(1..).unwrap_or(&[])) };
    let num = |i: usize, what: &str| rest.get(i).and_then(|x| x.parse::<i64>().ok()).ok_or_else(|| format!("Missing or invalid {what}"));
    match kind {
        "chat" => {
            let text = rest.join(" ");
            if text.is_empty() {
                return Err(format!("/ap {prefix} chat{} <message>", if prefix == "parse" { "" } else { " <target>" }));
            }
            if prefix == "parse" {
                sender.send_component(&render(sender.as_player(), &text));
            } else {
                for p in targets(prefix, sender, target_arg)? {
                    p.send_component(&render(Some(&p), &text));
                }
            }
        }
        "title" => {
            let t = Title {
                fade_in: num(0, "fade_in")?,
                stay: num(1, "stay")?,
                fade_out: num(2, "fade_out")?,
                title: rest.get(3).cloned().unwrap_or_default(),
                subtitle: rest.get(4).cloned().unwrap_or_default(),
            };
            for p in targets(prefix, sender, target_arg)? {
                display::show_title(&p, &t);
            }
        }
        "actionbar" => {
            let a = ActionBar { seconds: num(0, "seconds")?.max(1), fade: true, text: rest.get(1..).unwrap_or(&[]).join(" ") };
            for p in targets(prefix, sender, target_arg)? {
                display::show_action_bar(&p, &a);
            }
        }
        "bossbar" => {
            let bar = Bar {
                seconds: num(0, "seconds")?.max(1),
                overlay: rest.get(1).cloned().ok_or("Missing overlay")?,
                fill: rest.get(2).cloned().ok_or("Missing fillmode")?,
                color: rest.get(3).cloned().ok_or("Missing color")?,
                text: rest.get(4..).unwrap_or(&[]).join(" "),
            };
            for p in targets(prefix, sender, target_arg)? {
                display::show_bar(&p, &bar);
            }
        }
        "toast" => {
            let mut pos: Vec<&String> = Vec::new();
            let (mut enchanted, mut model_data, mut item_model) = (false, -1, "announcerplus:disabled".to_owned());
            let mut it = rest.iter();
            while let Some(w) = it.next() {
                match w.as_str() {
                    "--enchant" | "-e" => enchanted = true,
                    "--custom-model-data" | "-m" => model_data = it.next().and_then(|x| x.parse().ok()).unwrap_or(-1),
                    "--item-model" => item_model = it.next().cloned().unwrap_or(item_model),
                    _ => pos.push(w),
                }
            }
            let t = Toast {
                icon: pos.first().map(|s| (*s).clone()).ok_or("Missing icon")?,
                frame: pos.get(1).map(|s| (*s).clone()).ok_or("Missing frame")?,
                header: pos.get(2).map(|s| (*s).clone()).unwrap_or_default(),
                footer: pos.get(3).map(|s| (*s).clone()).unwrap_or_default(),
                enchanted,
                model_data,
                item_model,
            };
            for p in targets(prefix, sender, target_arg)? {
                display::show_toast(&p, &t);
            }
        }
        _ => return Err(format!("Unknown type '{kind}': chat, title, actionbar, bossbar, toast")),
    }
    Ok(())
}

fn help(sender: &CommandSender) {
    send(sender, &format!("{PREFIX} <white>AnnouncerPlus command help"));
    for (c, d) in [
        ("ap about", "Prints some information about AnnouncerPlus."),
        ("ap reload", "Reloads AnnouncerPlus configs."),
        ("ap messageconfig <config> [messages [page]]", "Shows info on a message config."),
        ("ap parse <chat|title|actionbar|bossbar|toast> ...", "Shows a message to yourself."),
        ("ap send <type> <players> ...", "Shows a message to the given players."),
        ("ap broadcast <type> <world|all> ...", "Shows a message to a world's players or everyone."),
        ("ap parseanimation <seconds> <message>", "Tests a message with animations."),
    ] {
        send(sender, &format!("<gray> - <white>/</white><#ff4f19>{}</#ff4f19>: <white>{}", c.replace('<', "\\<"), d));
    }
}

fn command(sender: &CommandSender, args: &[&str]) -> bool {
    let w = words(args);
    let sub = w.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    let deny = |sender: &CommandSender| send(sender, "<red>I'm sorry, but you do not have permission to perform this command.");
    let result: Result<(), String> = match sub.as_str() {
        "" | "help" => {
            help(sender);
            Ok(())
        }
        "about" => {
            send(sender, "<st><gradient:white:#ff4f19:white>                                       </gradient></st>");
            send(sender, &format!("<hover:show_text:'<rainbow>click me!'><click:open_url:'https://github.com/jpenilla/AnnouncerPlus'><white>AnnouncerPlus <gradient:#ff4f19:#f7ff19>{} (Rust)", env!("CARGO_PKG_VERSION")));
            send(sender, "<gray>By <white>jmp");
            send(sender, "<st><gradient:white:#ff4f19:white>                                       </gradient></st>");
            Ok(())
        }
        "reload" => {
            if !sender.has_permission("announcerplus.command.reload") {
                deny(sender);
                return true;
            }
            send(sender, &format!("{PREFIX} <italic><gradient:blue:green>Reloading AnnouncerPlus config..."));
            match reload() {
                Ok(()) => {
                    send(sender, &format!("{PREFIX} <green>Done."));
                    Ok(())
                }
                Err(e) => Err(format!("Failed to reload, see console. {e}")),
            }
        }
        "parse" | "send" | "broadcast" => match w.get(1) {
            Some(kind) => element_command(sender, &sub, &kind.to_ascii_lowercase(), w.get(2..).unwrap_or(&[])),
            None => Err(format!("/ap {sub} <chat|title|actionbar|bossbar|toast> ...")),
        },
        "parseanimation" => {
            if !sender.has_permission("announcerplus.command.parse.animation") {
                deny(sender);
                return true;
            }
            match (sender.as_player(), w.get(1).and_then(|x| x.parse::<i64>().ok())) {
                (Some(p), Some(secs)) => {
                    let text = w.get(2..).unwrap_or(&[]).join(" ");
                    display::show_title(&p, &Title { fade_in: 0, stay: secs, fade_out: 0, title: text.clone(), subtitle: text.clone() });
                    display::show_action_bar(&p, &ActionBar { seconds: secs, fade: false, text });
                    Ok(())
                }
                (None, _) => Err("This command must be run by a player.".into()),
                _ => Err("/ap parseanimation <seconds> <message>".into()),
            }
        }
        "messageconfig" => {
            if !sender.has_permission("announcerplus.command.messageconfig") {
                deny(sender);
                return true;
            }
            let cfg = configs();
            match w.get(1).and_then(|n| cfg.messages.get(n)) {
                None => Err(format!("No message config named '{}'. Configs: {}", w.get(1).cloned().unwrap_or_default(), cfg.messages.keys().cloned().collect::<Vec<_>>().join(", "))),
                Some(mc) => {
                    if w.get(2).is_some_and(|x| x.eq_ignore_ascii_case("messages")) {
                        let n = w.get(3).and_then(|x| x.parse::<usize>().ok()).unwrap_or(1).max(1);
                        match mc.messages.get(n - 1) {
                            Some(m) => {
                                send(sender, &format!("{PREFIX} <gray>Message <white>{n}</white>/<white>{}</white> of <white>{}", mc.messages.len(), mc.name));
                                for l in &m.text {
                                    sender.send_component(&render(sender.as_player(), l));
                                }
                                Ok(())
                            }
                            None => Err(format!("Message number must be 1-{}", mc.messages.len())),
                        }
                    } else {
                        send(sender, &format!("{PREFIX} <gray>Message config: <white>{}", mc.name));
                        send(sender, &format!("<white>  Permission</white><gray>: </gray>announcerplus.messages.{}", mc.name));
                        send(sender, &format!("<white>  Interval</white><gray>: </gray>{}", mc.interval_text));
                        send(sender, &format!("<white>  Random order</white><gray>: </gray>{}", mc.random));
                        send(sender, &format!("<white>  Messages</white><gray>: </gray><click:run_command:'/ap messageconfig {} messages'><hover:show_text:'Click to view'>{}</hover></click>", mc.name, mc.messages.len()));
                        Ok(())
                    }
                }
            }
        }
        _ => {
            help(sender);
            Ok(())
        }
    };
    if let Err(e) = result {
        send(sender, &format!("<red>{}", e.replace('<', "\\<")));
    }
    true
}

fn complete(args: &[&str]) -> Vec<String> {
    let typed = args.last().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let options: Vec<String> = match args.len() {
        0 | 1 => ["about", "help", "reload", "messageconfig", "parse", "send", "broadcast", "parseanimation"].iter().map(|s| (*s).to_owned()).collect(),
        2 if matches!(args[0], "parse" | "send" | "broadcast") => ["chat", "title", "actionbar", "bossbar", "toast"].iter().map(|s| (*s).to_owned()).collect(),
        2 if args[0] == "messageconfig" => configs().messages.keys().cloned().collect(),
        3 if args[0] == "send" => {
            let mut v: Vec<String> = horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).collect();
            v.push("@a".into());
            v
        }
        3 if args[0] == "broadcast" => {
            let mut v: Vec<String> = horizon_bukkit::get_worlds().iter().map(|w| w.get_name().to_owned()).collect();
            v.push("all".into());
            v
        }
        3 if args[0] == "messageconfig" => vec!["messages".into()],
        _ => Vec::new(),
    };
    options.into_iter().filter(|o| o.to_ascii_lowercase().starts_with(&typed)).collect()
}

// ---- Enable / disable ----

fn reload() -> Result<(), String> {
    let p = plugin().ok_or("not enabled")?;
    let cfg = load(&p.get_data_folder())?;
    display::stop_all();
    *CONFIG.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(cfg));
    hooks::refresh();
    start_broadcasts();
    Ok(())
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    if let Err(e) = reload() {
        plugin.severe(&format!("Failed to load AnnouncerPlus configs. This is likely due to misconfiguration. {e}"));
    }
    plugin.register_event_with("HIGHEST", false, on_join);
    plugin.register_event_with("HIGHEST", false, on_quit);
    plugin.set_executor("announcerplus", |sender, _, args| command(sender, args));
    plugin.set_tab_completer("announcerplus", |_, _, args| complete(args));
    plugin.run_task_timer(1, 1, display::tick);
}

fn on_disable(_plugin: Plugin) {
    for t in TASKS.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
        t.cancel();
    }
    display::stop_all();
    *CONFIG.write().unwrap_or_else(|e| e.into_inner()) = None;
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configs_round_trip() {
        assert_eq!(duration("3 minutes"), Some(3600));
        assert_eq!(duration("10m"), Some(12000));
        assert_eq!(duration("30 sec"), Some(600));
        assert_eq!(duration("2hrs"), Some(144000));
        let demo = hocon::parse(&hocon::write(&demo_config(), "h")).unwrap();
        let mc = message_config("demo", &demo);
        assert_eq!(mc.messages.len(), 7);
        assert!(mc.messages[1].boss_bar.is_some());
        assert!(mc.messages[2].toast.is_some());
        assert_eq!(mc.messages[3].sounds.len(), 3);
        assert!(mc.messages[5].title.is_some());
        let jq = join_quit(Some("default"), &hocon::parse(&hocon::write(&join_quit_default(), "h")).unwrap());
        assert_eq!(jq.messages.len(), 5);
        assert!(jq.title.is_some() && jq.toast.is_some() && jq.boss_bar.is_none());
        assert_eq!(words(&["\"a b\"", "c", "\"\""]), vec!["a b", "c", ""]);
        assert_eq!(legacy_tags("§cHi"), "<red>Hi");
    }
}
