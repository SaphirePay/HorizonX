//! HorizonBoard: a sidebar scoreboard, a tab list (header, footer, ranked and sorted player names, the rank over
//! each head) and the chat format for a Horizon server, written natively on the SDK and horizon-kit.
//!
//! Once a second (`update-seconds`) every online player's board is worked out on the server thread and only what
//! changed goes out, as the bytes the client reads: a sidebar line whose value changed is one packet, an unchanged
//! board is none. Each viewer is sent every player's tab name and team itself, so someone who joins later sees the
//! same list as everyone else. Ranks come from LuckPerms (primary group), balances from the server's economy
//! (Essentials). Chat is formatted from the ranks the last refresh worked out, so a chat line never waits on a
//! lookup.

use chrono::Local;
use horizon_bukkit::adventure::Component;
use horizon_bukkit::event::PlayerQuitEvent;
use horizon_bukkit::fam_ev_player::AsyncChatEvent;
use horizon_bukkit::{CommandSender, Player, Plugin, bukkit_plugin};
use horizon_kit::wire::W;
use horizon_kit::{Config, app, hooks, mini, packets, wire, yaml};
use serde_json::{Value, json};
use serde_yaml::Value as Yaml;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

const DEFAULT_CONFIG: &str = include_str!("../data/config.yml");
const SIDEBAR: &str = "horizonboard";

/// One rank of config.yml: its group, badge and name colour.
#[derive(Clone, Default)]
struct Rank {
    group: String,
    badge: String,
    colour: String,
}

/// config.yml, read.
#[derive(Clone, Default)]
struct Settings {
    words: Vec<(String, String)>,
    update_ticks: u64,
    sidebar: bool,
    title: String,
    lines: Vec<String>,
    tablist: bool,
    header: Vec<String>,
    footer: Vec<String>,
    name: String,
    chat: bool,
    chat_format: String,
    /// In sort order.
    ranks: Vec<Rank>,
    operator_rank: String,
}

impl Settings {
    fn load() -> Settings {
        let c = Config::load(&app::folder(), "config.yml", DEFAULT_CONFIG);
        let mut ranks = Vec::new();
        if let Some(Yaml::Mapping(m)) = c.at("ranks") {
            for (k, v) in m {
                let Some(group) = yaml::key_str(k) else { continue };
                // `owner: "<badge>"` (the first config) or `owner: { badge, name }`.
                let (badge, colour) = match v {
                    Yaml::Mapping(_) => (yaml::string(v, "badge").unwrap_or_default(), yaml::string(v, "name").unwrap_or_else(|| "<white>".into())),
                    other => (yaml::scalar(other).unwrap_or_default(), "<white>".into()),
                };
                ranks.push(Rank { group: group.to_ascii_lowercase(), badge, colour });
            }
        }
        Settings {
            words: vec![("%server%".into(), c.str("server-name", "Nyxora")), ("%website%".into(), c.str("website", "")), ("%store%".into(), c.str("store", ""))],
            update_ticks: (c.num("update-seconds", 1.0).max(0.25) * 20.0).round() as u64,
            sidebar: c.bool("sidebar.enabled", true),
            title: c.str("sidebar.title", "%server%"),
            lines: c.list("sidebar.lines").into_iter().take(15).collect(),
            tablist: c.bool("tablist.enabled", true),
            header: c.list("tablist.header"),
            footer: c.list("tablist.footer"),
            name: c.str("tablist.name", "%rank%  %name%"),
            chat: c.bool("chat.enabled", true),
            chat_format: c.str("chat.format", "%rank%  %name% <#52525b>» <#e4e4e7>%message%"),
            ranks,
            operator_rank: c.str("operator-rank", "owner").to_ascii_lowercase(),
        }
    }
}

/// What one player was last sent, and their rank as last worked out.
#[derive(Default)]
struct Seen {
    name: String,
    board_title: Option<Value>,
    board_lines: Vec<(Value, Option<Value>)>,
    tab: Option<(Value, Value)>,
    /// The tab names this viewer was sent, by the named player's uuid.
    list_names: HashMap<String, Value>,
    /// This player's own team (name, settings), as everyone has it.
    team: Option<(String, Value)>,
    /// The teams this viewer was sent, by team name.
    teams_known: HashSet<String>,
    hidden: bool,
    /// The chat format worked out at the last refresh (legacy text with %1$s / %2$s), for the chat event.
    chat_format: Option<Value>,
}

#[derive(Default)]
struct State {
    settings: Settings,
    players: HashMap<String, Seen>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut held = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(held.get_or_insert_with(State::default))
}

// ---- Values ----

/// The live values a player's lines are filled with.
struct Facts {
    uuid: String,
    player: String,
    rank: String,
    colour: String,
    rank_index: usize,
    balance: String,
    /// The balance as a plain number (`1,234`), for lines that put the word after it.
    balance_num: String,
    ping: String,
    world: String,
}

fn ping_text(ms: i32) -> String {
    let colour = if ms < 80 { "<#4ade80>" } else if ms < 150 { "<#facc15>" } else { "<#f87171>" };
    format!("{colour}{ms}ms</#>")
}

fn money(amount: f64) -> String {
    let whole = amount.trunc() as i64;
    let digits = whole.abs().to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let cents = ((amount.fract().abs() * 100.0).round() as i64).min(99);
    let sign = if amount < 0.0 { "-" } else { "" };
    if cents == 0 { format!("{sign}${grouped}") } else { format!("{sign}${grouped}.{cents:02}") }
}

fn facts(s: &Settings, p: &Player) -> Facts {
    let uuid = p.get_unique_id();
    let mut group = hooks::chat::primary_group(&uuid).to_ascii_lowercase();
    if group == "default" && p.is_op() {
        group = s.operator_rank.clone();
    }
    let rank_index = s.ranks.iter().position(|r| r.group == group).unwrap_or(s.ranks.len());
    let (rank, colour) = match s.ranks.get(rank_index) {
        Some(r) => (r.badge.clone(), r.colour.clone()),
        None => {
            // A group config.yml doesn't list: LuckPerms' own prefix, else the group's name.
            let prefix = hooks::chat::prefix(&uuid);
            (if prefix.trim().is_empty() { format!("<#71717a><bold>{}</bold>", group.to_uppercase()) } else { prefix }, "<white>".into())
        }
    };
    let amount = hooks::economy::enabled().then(|| hooks::economy::balance(p.get_name()));
    let balance = amount.map_or_else(|| "-".into(), money);
    let balance_num = amount.map_or_else(|| "-".into(), |a| money(a).replacen('$', "", 1));
    Facts { uuid, player: p.get_name().to_owned(), rank, colour, rank_index, balance, balance_num, ping: ping_text(p.get_ping()), world: p.get_world().get_name().to_owned() }
}

/// The values every line shares this refresh.
struct Shared {
    online: String,
    max: String,
    tps: String,
    date: String,
    /// `11/06/26`.
    date_short: String,
    time: String,
}

fn shared(online: usize) -> Shared {
    let tps = horizon_bukkit::server::get_tps()[0].min(20.0);
    let colour = if tps >= 19.5 { "<#4ade80>" } else if tps >= 15.0 { "<#facc15>" } else { "<#f87171>" };
    let now = Local::now();
    Shared {
        online: online.to_string(),
        max: horizon_bukkit::server::get_max_players().to_string(),
        tps: format!("{colour}{tps:.1}</#>"),
        date: now.format("%m/%d/%Y").to_string(),
        date_short: now.format("%m/%d/%y").to_string(),
        time: now.format("%H:%M").to_string(),
    }
}

fn fill(s: &Settings, raw: &str, f: &Facts, sh: &Shared) -> String {
    let mut out = raw.to_owned();
    for (k, v) in &s.words {
        out = out.replace(k, v);
    }
    let name = format!("{}{}", f.colour, f.player);
    for (k, v) in [("%player%", &f.player), ("%name%", &name), ("%rank%", &f.rank), ("%balance%", &f.balance), ("%balance_num%", &f.balance_num), ("%ping%", &f.ping), ("%world%", &f.world), ("%online%", &sh.online), ("%max%", &sh.max), ("%tps%", &sh.tps), ("%date%", &sh.date), ("%date_short%", &sh.date_short), ("%time%", &sh.time)] {
        out = out.replace(k, v);
    }
    out
}

fn component(raw: &str) -> Value {
    mini::parse_mixed(raw)
}

fn lines(s: &Settings, raw: &[String], f: &Facts, sh: &Shared, p: &Player) -> Value {
    // Other plugins' placeholders too (%network_online%, %horizonjobs_...%), as the sidebar does.
    component(&raw.iter().map(|l| hooks::placeholders(&fill(s, l, f, sh), Some(p))).collect::<Vec<_>>().join("\n"))
}

/// Where the message goes in a chat line.
const MARK: char = '\u{E000}';

/// A player's chat line as a component, the message's place held by `MARK` (styled like text the format wrote
/// there, so the message takes the format's colour).
fn chat_format(s: &Settings, f: &Facts, sh: &Shared) -> Value {
    component(&fill(s, &s.chat_format, f, sh).replace("%message%", &MARK.to_string()))
}

/// The chat line with the message in its place: the text holding `MARK` becomes that text before it, the
/// message, and the text after it, all in that text's style.
fn splice(line: &mut Value, message: &Value) -> bool {
    match line {
        Value::Object(o) => {
            if let Some(Value::String(t)) = o.get("text")
                && let Some((before, after)) = t.split_once(MARK)
            {
                let mut extra = Vec::new();
                if !before.is_empty() {
                    extra.push(json!({ "text": before }));
                }
                extra.push(message.clone());
                if !after.is_empty() {
                    extra.push(json!({ "text": after }));
                }
                if let Some(Value::Array(rest)) = o.remove("extra") {
                    extra.extend(rest);
                }
                o.insert("text".into(), json!(""));
                o.insert("extra".into(), Value::Array(extra));
                return true;
            }
            o.get_mut("extra").is_some_and(|e| splice(e, message))
        }
        Value::Array(a) => a.iter_mut().any(|v| splice(v, message)),
        Value::String(t) if t.contains(MARK) => {
            let (before, after) = t.split_once(MARK).unwrap_or_default();
            *line = json!({ "text": before, "extra": [message.clone(), { "text": after }] });
            true
        }
        _ => false,
    }
}

// ---- Packets ----

fn send(to: &str, bytes: Option<Vec<u8>>) {
    if let Some(b) = bytes {
        packets::queue_raw(&[to.to_owned()], b);
    }
}

fn info_update_id() -> Option<i32> {
    static ID: OnceLock<Option<i32>> = OnceLock::new();
    *ID.get_or_init(|| horizon_bukkit::horizon_mod::query(json!({ "op": "packet_ids", "names": ["PlayerInfoUpdate"] }))["PlayerInfoUpdate"].as_i64().map(|i| i as i32))
}

/// PlayerInfoUpdate with UPDATE_DISPLAY_NAME only: how one player is named in this viewer's tab list.
fn list_name_packet(uuid: &str, name: &Value) -> Option<Vec<u8>> {
    let id = info_update_id()?;
    let bits = u128::from_str_radix(&uuid.replace('-', ""), 16).ok()?;
    let mut body = W::default();
    body.u8(0x20);
    body.varint(1);
    body.0.extend_from_slice(&bits.to_be_bytes());
    body.bool(true);
    body.nbt(name);
    let mut inner = W::default();
    inner.varint(id);
    inner.0.extend_from_slice(&body.0);
    let mut out = W::default();
    out.varint(inner.0.len() as i32);
    out.0.extend_from_slice(&inner.0);
    Some(out.0)
}

/// The sidebar: made the first time, then only lines that changed.
fn sidebar(uuid: &str, seen: &mut Seen, title: Value, rows: Vec<(Value, Option<Value>)>) {
    let rebuild = seen.board_title.is_none() || seen.board_lines.len() != rows.len();
    if rebuild {
        if seen.board_title.is_some() {
            send(uuid, wire::set_objective(SIDEBAR, 1, &json!({ "text": "" }), 0));
        }
        send(uuid, wire::set_objective(SIDEBAR, 0, &title, 0));
        send(uuid, wire::display_objective(1, SIDEBAR));
    } else if seen.board_title.as_ref() != Some(&title) {
        send(uuid, wire::set_objective(SIDEBAR, 2, &title, 0));
    }
    let n = rows.len();
    for (i, row) in rows.iter().enumerate() {
        if !rebuild && seen.board_lines.get(i) == Some(row) {
            continue;
        }
        // Each line's holder is an invisible colour code, never shown: its text is the line's display.
        let owner = format!("§{:x}§r", i % 16);
        let format = match &row.1 {
            Some(value) => wire::NumberFormat::Fixed(value),
            None => wire::NumberFormat::Blank,
        };
        send(uuid, wire::set_score(&owner, SIDEBAR, (n - i) as i32, Some(&row.0), format));
    }
    seen.board_title = Some(title);
    seen.board_lines = rows;
}

fn hide_sidebar(uuid: &str, seen: &mut Seen) {
    if seen.board_title.take().is_some() {
        send(uuid, wire::set_objective(SIDEBAR, 1, &json!({ "text": "" }), 0));
    }
    seen.board_lines.clear();
}

/// One team packet to one viewer (the engine encodes it for the client): created (0), removed (1) or changed (2).
fn team(viewer: &str, name: &str, method: i32, params: &Value, players: &[String]) {
    let mut q = json!({ "op": "core_board_team", "uuid": viewer.replace('-', ""), "name": name, "method": method, "players": players });
    if let Some(o) = params.as_object() {
        for (k, v) in o {
            q[k] = v.clone();
        }
    }
    horizon_bukkit::horizon_mod::query(q);
}

/// A player's team: sorts them by rank in everyone's tab list and puts the badge before their name over their head.
fn team_of(f: &Facts) -> (String, Value) {
    let name: String = format!("{:02}{}", f.rank_index.min(99), f.player).chars().take(16).collect();
    let params = json!({
        "display": { "text": "" },
        "prefix": component(&format!("{} ", f.rank)),
        "suffix": { "text": "" },
        "visibility": 0,
        "collision": 0,
        "color": null,
        "options": 0,
    });
    (name, params)
}

// ---- The refresh ----

struct Wanted {
    uuid: String,
    player: String,
    board: Option<(Value, Vec<(Value, Option<Value>)>)>,
    tab: Option<(Value, Value)>,
    list_name: Option<Value>,
    team: Option<(String, Value)>,
    chat_format: Option<Value>,
}

fn refresh() {
    let online = horizon_bukkit::get_online_players();
    let settings = with(|st| st.settings.clone());
    let sh = shared(online.len());
    // Everyone's board, names and team, worked out first (the lookups), then sent with the state held.
    let wanted: Vec<Wanted> = online
        .iter()
        .map(|p| {
            let f = facts(&settings, p);
            let board = settings.sidebar.then(|| {
                // Our words first, then any other plugin's (%horizonsmp_playtime%, %horizonjobs_job_display%...).
                let title = component(&hooks::placeholders(&fill(&settings, &settings.title, &f, &sh), Some(p)));
                let rows = settings
                    .lines
                    .iter()
                    .map(|l| {
                        let filled = hooks::placeholders(&fill(&settings, l, &f, &sh), Some(p));
                        match filled.split_once("||") {
                            Some((left, right)) => (component(left), Some(component(right))),
                            None => (component(&filled), None),
                        }
                    })
                    .collect();
                (title, rows)
            });
            Wanted {
                uuid: f.uuid.clone(),
                player: f.player.clone(),
                board,
                tab: settings.tablist.then(|| (lines(&settings, &settings.header, &f, &sh, p), lines(&settings, &settings.footer, &f, &sh, p))),
                list_name: settings.tablist.then(|| component(&fill(&settings, &settings.name, &f, &sh))),
                team: settings.tablist.then(|| team_of(&f)),
                chat_format: settings.chat.then(|| chat_format(&settings, &f, &sh)),
            }
        })
        .collect();
    let mut teams_out: Vec<(String, String, i32, Value, Vec<String>)> = Vec::new();
    with(|st| {
        // Who left: their team goes from everyone's list.
        let here: HashSet<&String> = wanted.iter().map(|w| &w.uuid).collect();
        let gone: Vec<String> = st.players.keys().filter(|u| !here.contains(u)).cloned().collect();
        for u in gone {
            if let Some(seen) = st.players.remove(&u)
                && let Some((t, _)) = seen.team
            {
                for (viewer, vs) in st.players.iter_mut() {
                    if vs.teams_known.remove(&t) {
                        teams_out.push((viewer.clone(), t.clone(), 1, Value::Null, Vec::new()));
                    }
                }
            }
        }
        // Each player's own: sidebar, header and footer, chat format, team.
        for w in &wanted {
            let seen = st.players.entry(w.uuid.clone()).or_default();
            seen.name = w.player.clone();
            seen.chat_format = w.chat_format.clone();
            match &w.board {
                Some((title, rows)) if !seen.hidden => sidebar(&w.uuid, seen, title.clone(), rows.clone()),
                _ => hide_sidebar(&w.uuid, seen),
            }
            if let Some(pair) = &w.tab
                && seen.tab.as_ref() != Some(pair)
            {
                send(&w.uuid, wire::tab_list(&pair.0, &pair.1));
                seen.tab = Some(pair.clone());
            }
            if seen.team.as_ref() != w.team.as_ref() {
                // A changed team: everyone who has the old one loses it, then all get the new one below.
                if let Some((old, _)) = seen.team.take() {
                    for vs in st.players.values_mut() {
                        vs.teams_known.remove(&old);
                    }
                    for v in &wanted {
                        teams_out.push((v.uuid.clone(), old.clone(), 1, Value::Null, Vec::new()));
                    }
                }
                if let Some(s) = st.players.get_mut(&w.uuid) {
                    s.team = w.team.clone();
                }
            }
        }
        // Every viewer has every player's team and tab name: the ones they lack or that changed are sent.
        for v in &wanted {
            for w in &wanted {
                let Some((tname, params)) = &w.team else { continue };
                let viewer = st.players.get_mut(&v.uuid).expect("added above");
                if viewer.teams_known.insert(tname.clone()) {
                    teams_out.push((v.uuid.clone(), tname.clone(), 0, params.clone(), vec![w.player.clone()]));
                }
                if let Some(name) = &w.list_name
                    && viewer.list_names.get(&w.uuid) != Some(name)
                {
                    send(&v.uuid, list_name_packet(&w.uuid, name));
                    viewer.list_names.insert(w.uuid.clone(), name.clone());
                }
            }
        }
    });
    for (viewer, name, method, params, players) in teams_out {
        team(&viewer, &name, method, &params, &players);
    }
    packets::flush();
}

// ---- Chat ----

fn on_chat(e: &mut AsyncChatEvent) {
    let uuid = e.get_player().get_unique_id();
    if let Some(line) = with(|st| st.players.get(&uuid).and_then(|s| s.chat_format.clone())) {
        // The same line for every viewer: the format with the message (as the earlier listeners left it) in place.
        e.renderer(move |_source, _name, message, _viewer| {
            let mut out = line.clone();
            if !splice(&mut out, &message.0) {
                out = json!({ "text": "", "extra": [out, " ", message.0.clone()] });
            }
            Component(out)
        });
    }
}

// ---- Commands ----

fn tell(to: &CommandSender, raw: &str) {
    to.send_component(&Component(component(raw)));
}

fn board_command(sender: &CommandSender) -> bool {
    let CommandSender::Player(p) = sender else {
        tell(sender, "<red>Only players have a scoreboard.");
        return true;
    };
    if !p.has_permission("horizonboard.toggle") {
        tell(sender, "<red>You don't have permission to do that.");
        return true;
    }
    let uuid = p.get_unique_id();
    let hidden = with(|st| {
        let seen = st.players.entry(uuid.clone()).or_default();
        seen.hidden = !seen.hidden;
        if seen.hidden {
            hide_sidebar(&uuid, seen);
        }
        seen.hidden
    });
    packets::flush();
    tell(sender, if hidden { "<#a1a1aa>Scoreboard <#f87171>hidden<#a1a1aa>. Type <white>/board</white> to show it again." } else { "<#a1a1aa>Scoreboard <#4ade80>shown<#a1a1aa>." });
    true
}

fn admin_command(sender: &CommandSender, args: &[&str]) -> bool {
    if !sender.has_permission("horizonboard.reload") {
        tell(sender, "<red>You don't have permission to do that.");
        return true;
    }
    if args.first().is_some_and(|a| a.eq_ignore_ascii_case("reload")) {
        with(|st| {
            st.settings = Settings::load();
            // Everyone's board, header and names are made again from the new file. The old sidebar is taken down
            // first: a board with fewer lines would otherwise keep the old one's extra lines.
            for (uuid, seen) in st.players.iter_mut() {
                hide_sidebar(uuid, seen);
                seen.tab = None;
                seen.list_names.clear();
            }
        });
        refresh();
        tell(sender, "<gradient:#a855f7:#22d3ee>HorizonBoard</gradient> <#a1a1aa>reloaded.");
    } else {
        tell(sender, "<#a1a1aa>Usage: <white>/horizonboard reload");
    }
    true
}

fn on_enable(plugin: Plugin) {
    app::set(plugin);
    let _ = wire::ids();
    let _ = info_update_id();
    with(|st| st.settings = Settings::load());
    plugin.set_executor("board", |sender, _, _| board_command(sender));
    plugin.set_executor("horizonboard", |sender, _, args| admin_command(sender, args));
    plugin.set_tab_completer("horizonboard", |_, _, args| if args.len() <= 1 { vec!["reload".to_owned()] } else { Vec::new() });
    plugin.register_event(|e: &mut PlayerQuitEvent| {
        // The next refresh takes their team from everyone's list.
        let uuid = e.get_player().get_unique_id();
        with(|st| {
            for seen in st.players.values_mut() {
                seen.list_names.remove(&uuid);
            }
        });
    });
    // Last, so the format set here is the one the line goes out with.
    plugin.register_event_with("HIGHEST", false, on_chat);
    let ticks = with(|st| st.settings.update_ticks).max(5);
    plugin.run_task_timer(1, ticks, refresh);
    horizon_bukkit::horizon_mod::log("[HorizonBoard] Scoreboard, tab list and chat format ready");
}

fn on_disable(_plugin: Plugin) {
    // Everyone's sidebar and tab text taken down, the teams removed.
    let all: Vec<(String, Vec<String>)> = with(|st| st.players.iter().map(|(u, s)| (u.clone(), s.teams_known.iter().cloned().collect())).collect());
    for (uuid, teams) in &all {
        send(uuid, wire::set_objective(SIDEBAR, 1, &json!({ "text": "" }), 0));
        send(uuid, wire::tab_list(&json!({ "text": "" }), &json!({ "text": "" })));
        for t in teams {
            team(uuid, t, 1, &Value::Null, &[]);
        }
    }
    packets::flush();
    with(|st| st.players.clear());
    app::clear();
}

bukkit_plugin!(on_enable, on_disable);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_and_ping() {
        assert_eq!(money(1234567.0), "$1,234,567");
        assert_eq!(money(99750.5), "$99,750.50");
        assert_eq!(money(0.0), "$0");
        assert!(ping_text(42).contains("#4ade80"));
        assert!(ping_text(200).contains("#f87171"));
    }

    #[test]
    fn chat_format_takes_name_and_message() {
        let s = Settings { chat_format: "%rank%  %name% <#52525b>» <#e4e4e7>%message%".into(), ..Default::default() };
        let f = Facts { uuid: "u".into(), player: "Steve".into(), rank: "<#f87171><bold>OWNER</bold>".into(), colour: "<#fca5a5>".into(), rank_index: 0, balance: String::new(), balance_num: String::new(), ping: String::new(), world: String::new() };
        let sh = Shared { online: "1".into(), max: "20".into(), tps: "20".into(), date: String::new(), date_short: String::new(), time: String::new() };
        let mut out = chat_format(&s, &f, &sh);
        assert!(splice(&mut out, &json!({ "text": "100% hi" })), "{out}");
        let text = out.to_string();
        assert!(text.contains("Steve") && text.contains("OWNER") && text.contains("100% hi"), "{text}");
        assert!(!text.contains(MARK) && !text.contains("%message%"), "{text}");
        // The message sits inside the text styled #e4e4e7, so it takes that colour.
        let at = text.find("100% hi").unwrap();
        let colour = text[..at].rfind("\"color\"").map(|i| &text[i..i + 17]);
        assert_eq!(colour.map(str::to_lowercase).as_deref(), Some("\"color\":\"#e4e4e7\""), "{text}");
    }

    #[test]
    fn list_name_needs_the_engine() {
        // Without the engine there is no packet id: nothing is sent rather than a broken packet.
        assert!(list_name_packet("00000000-0000-0000-0000-000000000001", &json!({ "text": "x" })).is_none() || info_update_id().is_some());
    }
}
