//! TAB 6.2.1 for Horizon, written again in Rust (the Java it follows is in java/). Same files (`plugins/TAB/
//! config.yml`, `groups.yml`, `users.yml`, `animations.yml`, `messages.yml`, `playerdata.yml`), same `/tab`.
//!
//! Features: header/footer designs, tab list names (tabprefix/customtabname/tabsuffix), name tags and tab sorting
//! (per-viewer teams: tagprefix/tagsuffix, colour, collision, sorting types), the player-list (yellow number) and
//! below-name objectives with fancy values, boss bars, sidebars (`||` right-hand text, no red numbers), animations,
//! conditions, per-world/per-group/per-user properties, toggles remembered.
//!
//! Cost: one server-thread timer. Players refresh in turn (spread over the refresh interval), placeholder values
//! are cached for their own intervals, and only what changed is sent: a name tag that stays the same costs no
//! packet. Packets for the tick go out in one engine query (`horizon_kit::packets::flush`); scores and boss bars
//! are bytes made here.

mod props;
mod text;

use horizon_bukkit::adventure::Component;
use horizon_bukkit::event::PlayerQuitEvent;
use horizon_bukkit::{CommandSender, Player, Plugin, bukkit_plugin};
use horizon_kit::{Config, packets, wire};
use props::Props;
use serde_json::{Value, json};
use serde_yaml::Value as Yaml;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use text::{TextSettings, Who, now_ms};

const DEF_CONFIG: &str = include_str!("../data/config/config.yml");
const DEF_GROUPS: &str = include_str!("../data/config/groups.yml");
const DEF_USERS: &str = include_str!("../data/config/users.yml");
const DEF_ANIMATIONS: &str = include_str!("../data/config/animations.yml");
const DEF_MESSAGES: &str = include_str!("../data/config/messages.yml");

const OBJ_LIST: &str = "TAB-PlayerList";
const OBJ_BELOW: &str = "TAB-BelowName";
const OBJ_SIDEBAR: &str = "TAB-Scoreboard";

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static CFG: RwLock<Option<Arc<Cfg>>> = RwLock::new(None);
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}
fn cfg() -> Arc<Cfg> {
    CFG.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}
fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE.lock().unwrap_or_else(|e| e.into_inner()))
}

// ---- Settings ----

#[derive(Clone, Debug)]
struct Design {
    cond: String,
    header: Vec<String>,
    footer: Vec<String>,
}

#[derive(Clone, Debug)]
enum Sort {
    Groups(Vec<Vec<String>>),
    Permissions(Vec<String>),
    Values(String, Vec<String>),
    AToZ(String),
    ZToA(String),
    LowToHigh(String),
    HighToLow(String),
}

#[derive(Clone, Debug)]
struct BarDef {
    name: String,
    style: String,
    color: String,
    progress: String,
    text: String,
    cond: String,
    announcement: bool,
}

#[derive(Clone, Debug)]
struct BoardDef {
    name: String,
    title: String,
    cond: String,
    lines: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct Objective {
    enabled: bool,
    value: String,
    fancy: String,
    title: String,
    hearts: bool,
    disable: String,
}

#[derive(Default)]
struct Cfg {
    text: TextSettings,
    hf: bool,
    designs: Vec<Design>,
    names: bool,
    names_disable: String,
    teams: bool,
    collision: bool,
    invisible_tags: bool,
    see_invisibles: bool,
    case_sensitive: bool,
    sorting: Vec<Sort>,
    teams_disable: String,
    list: Objective,
    below: Objective,
    bars: bool,
    bar_toggle: String,
    bar_remember: bool,
    bar_hidden: bool,
    bar_defs: Vec<BarDef>,
    boards: bool,
    board_toggle: String,
    board_remember: bool,
    board_hidden: bool,
    board_delay_ms: u64,
    board_defs: Vec<BoardDef>,
    by_perms: bool,
    finding: Vec<String>,
    groups: Props,
    users: Props,
    msgs: Config,
    refresh_ticks: u64,
}

fn yaml_lines(v: Option<&Yaml>) -> Vec<String> {
    match v {
        Some(Yaml::Sequence(s)) => s.iter().map(|x| props::scalar(x).unwrap_or_default()).collect(),
        Some(other) => props::scalar(other).map(|s| vec![s]).unwrap_or_default(),
        None => Vec::new(),
    }
}

fn map_entries(v: Option<&Yaml>) -> Vec<(String, Yaml)> {
    v.and_then(Yaml::as_mapping).map(|m| m.iter().filter_map(|(k, v)| Some((props::scalar(k)?, v.clone()))).collect()).unwrap_or_default()
}

fn load() -> Result<Cfg, String> {
    let p = plugin().ok_or("not enabled")?;
    let dir = p.get_data_folder();
    let c = Config::load(&dir, "config.yml", DEF_CONFIG);
    let anim = Config::load(&dir, "animations.yml", DEF_ANIMATIONS);
    let msgs = Config::load(&dir, "messages.yml", DEF_MESSAGES);
    let groups = Props::load(&dir.join("groups.yml"), DEF_GROUPS)?;
    let users = Props::load(&dir.join("users.yml"), DEF_USERS)?;
    let b = |k: &str, d: bool| c.bool(k, d);
    let s = |k: &str, d: &str| c.str(k, d);

    let mut ts = TextSettings {
        default_interval: c.int("placeholder-refresh-intervals.default-refresh-interval", 500).max(50) as u64,
        date_format: s("placeholders.date-format", "dd.MM.yyyy"),
        time_format: s("placeholders.time-format", "[HH:mm:ss / h:mm a]"),
        time_offset_hours: c.num("placeholders.time-offset", 0.0),
        minimessage: b("components.minimessage-support", true),
        ..Default::default()
    };
    for (k, v) in map_entries(c.at("placeholder-refresh-intervals")) {
        if k.starts_with('%')
            && let Some(n) = v.as_i64()
        {
            ts.intervals.insert(k, n.max(50) as u64);
        }
    }
    for (k, v) in map_entries(c.at("placeholder-output-replacements")) {
        let list = map_entries(Some(&v)).into_iter().map(|(from, to)| (from, props::scalar(&to).unwrap_or_default())).collect();
        ts.replacements.insert(k, list);
    }
    for (k, v) in map_entries(c.at("conditions")) {
        let conds = yaml_lines(v.get("conditions"));
        let or = v.get("type").and_then(Yaml::as_str).is_some_and(|t| t.eq_ignore_ascii_case("OR"));
        let yes = v.get("true").or_else(|| v.get(Yaml::Bool(true))).and_then(props::scalar).unwrap_or_else(|| "true".into());
        let no = v.get("false").or_else(|| v.get(Yaml::Bool(false))).and_then(props::scalar).unwrap_or_else(|| "false".into());
        ts.conditions.insert(k, text::NamedCondition { conditions: conds, or, yes, no });
    }
    let mut min_anim = u64::MAX;
    for k in anim.keys("") {
        let interval = anim.int(&format!("{k}.change-interval"), 1000).max(50) as u64;
        min_anim = min_anim.min(interval);
        ts.animations.insert(k.clone(), text::Animation { interval, texts: anim.list(&format!("{k}.texts")) });
    }

    let designs = map_entries(c.at("header-footer.designs"))
        .into_iter()
        .map(|(_, v)| Design { cond: v.get("display-condition").and_then(props::scalar).unwrap_or_default(), header: yaml_lines(v.get("header")), footer: yaml_lines(v.get("footer")) })
        .collect();
    let sorting = c
        .list("scoreboard-teams.sorting-types")
        .into_iter()
        .filter_map(|t| {
            let (kind, rest) = t.split_once(':')?;
            Some(match kind.to_ascii_uppercase().as_str() {
                "GROUPS" => Sort::Groups(rest.split(',').map(|g| g.split('|').map(|x| x.trim().to_ascii_lowercase()).collect()).collect()),
                "PERMISSIONS" => Sort::Permissions(rest.split(',').map(|x| x.trim().to_owned()).collect()),
                "PLACEHOLDER" => {
                    let (ph, vals) = rest.rsplit_once(':')?;
                    Sort::Values(ph.to_owned(), vals.split(',').map(|x| x.trim().to_owned()).collect())
                }
                "PLACEHOLDER_A_TO_Z" => Sort::AToZ(rest.to_owned()),
                "PLACEHOLDER_Z_TO_A" => Sort::ZToA(rest.to_owned()),
                "PLACEHOLDER_LOW_TO_HIGH" => Sort::LowToHigh(rest.to_owned()),
                "PLACEHOLDER_HIGH_TO_LOW" => Sort::HighToLow(rest.to_owned()),
                _ => return None,
            })
        })
        .collect();
    let objective = |base: &str, value: &str| Objective {
        enabled: b(&format!("{base}.enabled"), false),
        value: s(&format!("{base}.value"), value),
        fancy: s(&format!("{base}.fancy-value"), ""),
        title: s(&format!("{base}.title"), ""),
        hearts: s(&format!("{base}.render-type"), "INTEGER").eq_ignore_ascii_case("HEARTS"),
        disable: s(&format!("{base}.disable-condition"), ""),
    };
    let bar_defs = map_entries(c.at("bossbar.bars"))
        .into_iter()
        .map(|(name, v)| BarDef {
            name,
            style: v.get("style").and_then(props::scalar).unwrap_or_else(|| "PROGRESS".into()),
            color: v.get("color").and_then(props::scalar).unwrap_or_else(|| "PURPLE".into()),
            progress: v.get("progress").and_then(props::scalar).unwrap_or_else(|| "100".into()),
            text: v.get("text").and_then(props::scalar).unwrap_or_default(),
            cond: v.get("display-condition").and_then(props::scalar).unwrap_or_default(),
            announcement: v.get("announcement-bar").and_then(Yaml::as_bool).unwrap_or(false),
        })
        .collect();
    let board_defs = map_entries(c.at("scoreboard.scoreboards"))
        .into_iter()
        .map(|(name, v)| BoardDef { name, title: v.get("title").and_then(props::scalar).unwrap_or_default(), cond: v.get("display-condition").and_then(props::scalar).unwrap_or_default(), lines: yaml_lines(v.get("lines")) })
        .collect();
    let refresh = ts.default_interval.min(min_anim);
    Ok(Cfg {
        hf: b("header-footer.enabled", true),
        designs,
        names: b("tablist-name-formatting.enabled", true),
        names_disable: s("tablist-name-formatting.disable-condition", ""),
        teams: b("scoreboard-teams.enabled", true),
        collision: b("scoreboard-teams.enable-collision", true),
        invisible_tags: b("scoreboard-teams.invisible-nametags", false),
        see_invisibles: b("scoreboard-teams.can-see-friendly-invisibles", false),
        case_sensitive: b("scoreboard-teams.case-sensitive-sorting", true),
        sorting,
        teams_disable: s("scoreboard-teams.disable-condition", ""),
        list: objective("playerlist-objective", "%ping%"),
        below: objective("belowname-objective", "%health%"),
        bars: b("bossbar.enabled", false),
        bar_toggle: s("bossbar.toggle-command", "/bossbar"),
        bar_remember: b("bossbar.remember-toggle-choice", false),
        bar_hidden: b("bossbar.hidden-by-default", false),
        bar_defs,
        boards: b("scoreboard.enabled", false),
        board_toggle: s("scoreboard.toggle-command", "/sb"),
        board_remember: b("scoreboard.remember-toggle-choice", false),
        board_hidden: b("scoreboard.hidden-by-default", false),
        board_delay_ms: c.int("scoreboard.delay-on-join-milliseconds", 0).max(0) as u64,
        board_defs,
        by_perms: b("assign-groups-by-permissions", false),
        finding: c.list("primary-group-finding-list"),
        groups,
        users,
        msgs,
        refresh_ticks: (refresh / 50).clamp(1, 20),
        text: ts,
    })
}

fn msg(key: &str, args: &[(&str, &str)]) -> String {
    let mut m = cfg().msgs.str(key, key);
    for (k, v) in args {
        m = m.replace(&format!("%{k}%"), v);
    }
    horizon_bukkit::chat_color::translate_alternate_color_codes('&', &m)
}

// ---- Per-player state ----

#[derive(Default)]
struct Bar {
    id: u128,
    text: Value,
    colour: i32,
    overlay: i32,
    progress: f32,
}

type Board = (String, Value, Vec<(Value, Option<Value>)>);

#[derive(Default)]
struct Pl {
    uuid: String,
    name: String,
    group: String,
    joined_ms: u64,
    team: Option<String>,
    team_params: Option<Value>,
    tab_name: Option<Value>,
    header: Option<(Value, Value)>,
    list_score: Option<(i32, Value)>,
    below_score: Option<(i32, Value)>,
    board: Option<Board>,
    board_off: bool,
    board_forced: Option<(String, u64)>,
    bars_off: bool,
    bars: BTreeMap<String, Bar>,
    announced: HashMap<String, u64>,
    tag_hidden: bool,
    collision: Option<bool>,
    has_list: bool,
    has_below: bool,
}

#[derive(Default)]
struct State {
    players: BTreeMap<String, Pl>,
    tick: u64,
    /// Team name -> owner uuid (unique names).
    team_owner: HashMap<String, String>,
    bar_announce: HashMap<String, u64>,
    board_announce: Option<(String, u64)>,
    board_off_saved: HashSet<String>,
    bar_off_saved: HashSet<String>,
}

fn hex_of(uuid: &str) -> String {
    uuid.replace('-', "")
}


/// One of the scoreboard / tab-list packets to these players, as bytes made here (`packets::queue` takes the
/// engine's packet names with field lists, not these): `tab_list`, `sb_objective`, `sb_display`, `sb_reset`,
/// `sb_score`.
fn jp(to: &[String], name: &str, f: Value) {
    let empty = json!({ "text": "" });
    let bytes = match name {
        "tab_list" => wire::tab_list(f.get("header").unwrap_or(&empty), f.get("footer").unwrap_or(&empty)),
        "sb_objective" => wire::set_objective(f["name"].as_str().unwrap_or_default(), f["method"].as_i64().unwrap_or(0) as i8, f.get("display").unwrap_or(&empty), f["render"].as_i64().unwrap_or(0) as i32),
        "sb_display" => wire::display_objective(f["slot"].as_i64().unwrap_or(1) as i32, f["objective"].as_str().unwrap_or_default()),
        "sb_reset" => wire::reset_score(f["owner"].as_str().unwrap_or_default(), f["objective"].as_str()),
        "sb_score" => wire::set_score(f["owner"].as_str().unwrap_or_default(), f["objective"].as_str().unwrap_or_default(), f["score"].as_i64().unwrap_or(0) as i32, None, wire::NumberFormat::Default),
        _ => None,
    };
    if let Some(b) = bytes {
        packets::queue_raw(to, b);
    }
}

// ---- Teams ----

fn base36(mut n: u64, width: usize) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = vec![b'0'; width];
    for i in (0..width).rev() {
        out[i] = D[(n % 36) as usize];
        n /= 36;
    }
    String::from_utf8(out).unwrap_or_default()
}

fn sort_key(c: &Cfg, who: &Who, now: u64) -> String {
    let mut key = String::new();
    for s in &c.sorting {
        match s {
            Sort::Groups(list) => {
                let g = who.group.to_ascii_lowercase();
                let i = list.iter().position(|alts| alts.contains(&g)).unwrap_or(list.len());
                key.push_str(&base36(i as u64, 2));
            }
            Sort::Permissions(list) => {
                let i = list.iter().position(|p| who.player.has_permission(p)).unwrap_or(list.len());
                key.push_str(&base36(i as u64, 2));
            }
            Sort::Values(ph, vals) => {
                let v = horizon_kit::text::strip(&text::fill(ph, Some(who), &c.text, now));
                let i = vals.iter().position(|x| *x == v).unwrap_or(vals.len());
                key.push_str(&base36(i as u64, 2));
            }
            Sort::AToZ(ph) | Sort::ZToA(ph) => {
                let mut v = horizon_kit::text::strip(&text::fill(ph, Some(who), &c.text, now));
                if !c.case_sensitive {
                    v = v.to_lowercase();
                }
                let mut chunk: String = v.chars().filter(|ch| ch.is_ascii_graphic()).take(4).collect();
                while chunk.len() < 4 {
                    chunk.push(' ');
                }
                if matches!(s, Sort::ZToA(_)) {
                    chunk = chunk.bytes().map(|b| char::from(126 - (b.clamp(32, 126) - 32))).collect();
                }
                key.push_str(&chunk);
            }
            Sort::LowToHigh(ph) | Sort::HighToLow(ph) => {
                let v = horizon_kit::text::strip(&text::fill(ph, Some(who), &c.text, now)).trim().replace(',', "").parse::<f64>().unwrap_or(0.0);
                let n = v.clamp(0.0, 1_679_615.0) as u64;
                let n = if matches!(s, Sort::HighToLow(_)) { 1_679_615 - n } else { n };
                key.push_str(&base36(n, 4));
            }
        }
    }
    key
}

/// A unique team name: sort key, then the player's name, at most 16 characters.
fn team_name(st: &State, key: &str, name: &str, uuid: &str) -> String {
    let mut t: String = format!("{key}{name}").chars().take(16).collect();
    let mut n = 0;
    while st.team_owner.get(&t).is_some_and(|o| o != uuid) {
        n += 1;
        let mut base: String = t.chars().take(15).collect();
        base.push(char::from(b'0' + (n % 10) as u8));
        t = base;
    }
    t
}

/// One team packet to one viewer (`core_board_team`: the engine encodes it for the client's version).
fn send_team(viewer_uuid: &str, name: &str, method: i32, params: &Value, players: &[String]) {
    let mut q = json!({ "op": "core_board_team", "uuid": hex_of(viewer_uuid), "name": name, "method": method, "players": players });
    if let Some(o) = params.as_object() {
        for (k, v) in o {
            q[k] = v.clone();
        }
    }
    horizon_bukkit::horizon_mod::query(q);
}

// ---- Refresh ----

fn group_of(c: &Cfg, player: &Player, uuid: &str) -> String {
    if c.by_perms {
        return c.finding.iter().find(|g| player.has_permission(&format!("tab.group.{g}"))).cloned().unwrap_or_else(|| c.finding.last().cloned().unwrap_or_else(|| "default".into()));
    }
    let primary = horizon_kit::hooks::chat::primary_group(uuid);
    // groups.yml keys keep their case (`Owner`); LuckPerms names are lower case.
    c.groups.find_group(&primary).unwrap_or(primary)
}

fn number(v: &str) -> i32 {
    horizon_kit::text::strip(v).trim().replace(',', "").parse::<f64>().map_or(0, |f| f.round() as i32)
}

fn header_footer(c: &Cfg, who: &Who, prop: &dyn Fn(&str) -> Option<String>, now: u64) {
    let ts = &c.text;
    let own = (prop("header"), prop("footer"));
    let (h, f) = if own.0.is_some() || own.1.is_some() {
        (own.0.map(|h| vec![h]).unwrap_or_default(), own.1.map(|f| vec![f]).unwrap_or_default())
    } else {
        c.designs.iter().find(|d| text::condition(&d.cond, Some(who), ts, now)).map(|d| (d.header.clone(), d.footer.clone())).unwrap_or_default()
    };
    let pair = (text::lines(&h, Some(who), ts, now), text::lines(&f, Some(who), ts, now));
    let uuid = who.uuid.to_owned();
    if state(|st| st.players.get(&uuid).and_then(|p| p.header.as_ref()) != Some(&pair)) {
        jp(std::slice::from_ref(&uuid), "tab_list", json!({ "header": pair.0, "footer": pair.1 }));
        state(|st| st.players.get_mut(&uuid).map(|p| p.header = Some(pair)));
    }
}

fn tab_name(c: &Cfg, who: &Who, prop: &dyn Fn(&str) -> Option<String>, now: u64) {
    let ts = &c.text;
    let on = c.names_disable.is_empty() || !text::condition(&c.names_disable, Some(who), ts, now);
    let name = on.then(|| {
        let full = format!("{}{}{}", prop("tabprefix").unwrap_or_default(), prop("customtabname").unwrap_or_else(|| "%player%".into()), prop("tabsuffix").unwrap_or_default());
        text::component(&text::fill(&full, Some(who), ts, now), ts)
    });
    let uuid = who.uuid.to_owned();
    if state(|st| st.players.get(&uuid).map(|p| p.tab_name.as_ref()) != Some(name.as_ref())) {
        who.player.set_player_list_name_component(name.as_ref().map(|v| Component(v.clone())).as_ref());
        state(|st| st.players.get_mut(&uuid).map(|p| p.tab_name = name));
    }
}

fn name_tag(c: &Cfg, who: &Who, prop: &dyn Fn(&str) -> Option<String>, others: &[String], now: u64) {
    let ts = &c.text;
    let uuid = who.uuid.to_owned();
    let disabled = !c.teams_disable.is_empty() && text::condition(&c.teams_disable, Some(who), ts, now);
    let key = sort_key(c, who, now);
    let (hidden, collision, name, old_team, old_params) =
        state(|st| st.players.get(&uuid).map(|p| (p.tag_hidden, p.collision, p.name.clone(), p.team.clone(), p.team_params.clone())).unwrap_or_default());
    let params = if disabled {
        json!({ "display": { "text": "" }, "prefix": { "text": "" }, "suffix": { "text": "" }, "visibility": 0, "collision": 0, "color": null, "options": 0 })
    } else {
        let prefix = text::fill(&prop("tagprefix").unwrap_or_default(), Some(who), ts, now);
        let suffix = text::fill(&prop("tagsuffix").unwrap_or_default(), Some(who), ts, now);
        json!({
            "display": { "text": "" },
            "prefix": text::component(&prefix, ts),
            "suffix": text::component(&suffix, ts),
            "visibility": if hidden || c.invisible_tags { 1 } else { 0 },
            "collision": if collision.unwrap_or(c.collision) { 0 } else { 1 },
            "color": text::last_color(&prefix),
            "options": if c.see_invisibles { 2 } else { 0 },
        })
    };
    let team = state(|st| team_name(st, &key, &name, &uuid));
    if old_team.as_deref() != Some(team.as_str()) {
        if let Some(old) = &old_team {
            for v in others {
                send_team(v, old, 1, &Value::Null, &[]);
            }
        }
        for v in others {
            send_team(v, &team, 0, &params, std::slice::from_ref(&name));
        }
        state(|st| {
            if let Some(old) = &old_team {
                st.team_owner.remove(old);
            }
            st.team_owner.insert(team.clone(), uuid.clone());
            if let Some(p) = st.players.get_mut(&uuid) {
                p.team = Some(team.clone());
                p.team_params = Some(params.clone());
            }
        });
    } else if old_params.as_ref() != Some(&params) {
        for v in others {
            send_team(v, &team, 2, &params, &[]);
        }
        state(|st| st.players.get_mut(&uuid).map(|p| p.team_params = Some(params)));
    }
}

fn objectives(c: &Cfg, who: &Who, others: &[String], now: u64) {
    let ts = &c.text;
    let uuid = who.uuid.to_owned();
    let pl_name = who.player.get_name().to_owned();
    for (obj, is_list, name) in [(&c.list, true, OBJ_LIST), (&c.below, false, OBJ_BELOW)] {
        if !obj.enabled {
            continue;
        }
        let off = !obj.disable.is_empty() && text::condition(&obj.disable, Some(who), ts, now);
        let value = if off { 0 } else { number(&text::fill(&obj.value, Some(who), ts, now)) };
        let fancy_text = if obj.fancy.is_empty() { value.to_string() } else { text::fill(&obj.fancy, Some(who), ts, now) };
        let pair = (value, text::component(&fancy_text, ts));
        let old = state(|st| st.players.get(&uuid).and_then(|p| if is_list { p.list_score.clone() } else { p.below_score.clone() }));
        if old.as_ref() == Some(&pair) {
            continue;
        }
        match wire::set_score(&pl_name, name, pair.0, None, wire::NumberFormat::Fixed(&pair.1)) {
            Some(bytes) => packets::queue_raw(others, bytes),
            None => jp(others, "sb_score", json!({ "owner": pl_name, "objective": name, "score": pair.0 })),
        }
        state(|st| {
            if let Some(p) = st.players.get_mut(&uuid) {
                if is_list {
                    p.list_score = Some(pair);
                } else {
                    p.below_score = Some(pair);
                }
            }
        });
    }
}

fn boss_bars(c: &Cfg, who: &Who, now: u64) {
    let ts = &c.text;
    let uuid = who.uuid.to_owned();
    let (off, mut announced) = state(|st| {
        let mut a = st.bar_announce.clone();
        let off = st.players.get(&uuid).is_some_and(|p| {
            a.extend(p.announced.clone());
            p.bars_off
        });
        (off, a)
    });
    announced.retain(|_, until| *until > now);
    let mut want: BTreeMap<String, Bar> = BTreeMap::new();
    if !off {
        for b in &c.bar_defs {
            let shown = announced.contains_key(&b.name) || (!b.announcement && text::condition(&b.cond, Some(who), ts, now));
            if !shown {
                continue;
            }
            want.insert(
                b.name.clone(),
                Bar {
                    id: 0,
                    text: text::component(&text::fill(&b.text, Some(who), ts, now), ts),
                    colour: wire::boss::colour_of(&text::fill(&b.color, Some(who), ts, now)),
                    overlay: wire::boss::overlay_of(&text::fill(&b.style, Some(who), ts, now)),
                    progress: text::fill(&b.progress, Some(who), ts, now).trim().parse::<f32>().unwrap_or(100.0) / 100.0,
                },
            );
        }
    }
    let me = [uuid.clone()];
    state(|st| {
        let Some(p) = st.players.get_mut(&uuid) else { return };
        let gone: Vec<String> = p.bars.keys().filter(|k| !want.contains_key(*k)).cloned().collect();
        for k in gone {
            if let Some(b) = p.bars.remove(&k)
                && let Some(bytes) = wire::boss::remove(b.id)
            {
                packets::queue_raw(&me, bytes);
            }
        }
        for (k, mut w) in want {
            match p.bars.get_mut(&k) {
                None => {
                    w.id = wire::boss::new_id();
                    if let Some(bytes) = wire::boss::add(w.id, &w.text, w.progress, w.colour, w.overlay) {
                        packets::queue_raw(&me, bytes);
                    }
                    p.bars.insert(k, w);
                }
                Some(have) => {
                    if have.text != w.text
                        && let Some(bytes) = wire::boss::title(have.id, &w.text)
                    {
                        packets::queue_raw(&me, bytes);
                        have.text = w.text;
                    }
                    if (have.colour, have.overlay) != (w.colour, w.overlay)
                        && let Some(bytes) = wire::boss::style(have.id, w.colour, w.overlay)
                    {
                        packets::queue_raw(&me, bytes);
                        (have.colour, have.overlay) = (w.colour, w.overlay);
                    }
                    if (have.progress - w.progress).abs() > 0.001
                        && let Some(bytes) = wire::boss::progress(have.id, w.progress)
                    {
                        packets::queue_raw(&me, bytes);
                        have.progress = w.progress;
                    }
                }
            }
        }
    });
}

fn sidebar(c: &Cfg, who: &Who, now: u64) {
    let ts = &c.text;
    let uuid = who.uuid.to_owned();
    let (off, forced, joined) = state(|st| st.players.get(&uuid).map(|p| (p.board_off, p.board_forced.clone(), p.joined_ms)).unwrap_or_default());
    let announced = state(|st| st.board_announce.clone()).filter(|(_, until)| *until > now).map(|(n, _)| n);
    let forced = forced.filter(|(_, until)| *until > now).map(|(n, _)| n);
    let def = if off || now < joined + c.board_delay_ms {
        None
    } else if let Some(n) = forced.or(announced) {
        c.board_defs.iter().find(|b| b.name == n)
    } else {
        c.board_defs.iter().find(|b| text::condition(&b.cond, Some(who), ts, now))
    };
    let want: Option<Board> = def.map(|d| {
        let title = text::component(&text::fill(&d.title, Some(who), ts, now), ts);
        let lines = d
            .lines
            .iter()
            .take(15)
            .map(|l| {
                let filled = text::fill(l, Some(who), ts, now);
                match filled.split_once("||") {
                    Some((left, right)) => (text::component(left, ts), Some(text::component(right, ts))),
                    None => (text::component(&filled, ts), None),
                }
            })
            .collect();
        (d.name.clone(), title, lines)
    });
    let have = state(|st| st.players.get(&uuid).and_then(|p| p.board.clone()));
    if have == want {
        return;
    }
    let me = [uuid.clone()];
    let rebuild = have.as_ref().map(|h| (&h.0, h.2.len())) != want.as_ref().map(|w| (&w.0, w.2.len()));
    if rebuild && have.is_some() {
        jp(&me, "sb_objective", json!({ "name": OBJ_SIDEBAR, "method": 1 }));
    }
    if let Some((_, title, lines)) = &want {
        if rebuild {
            jp(&me, "sb_objective", json!({ "name": OBJ_SIDEBAR, "method": 0, "display": title, "render": 0 }));
            jp(&me, "sb_display", json!({ "slot": 1, "objective": OBJ_SIDEBAR }));
        } else if have.as_ref().map(|h| &h.1) != Some(title) {
            jp(&me, "sb_objective", json!({ "name": OBJ_SIDEBAR, "method": 2, "display": title, "render": 0 }));
        }
        let n = lines.len();
        for (i, line) in lines.iter().enumerate() {
            if !rebuild && have.as_ref().and_then(|h| h.2.get(i)) == Some(line) {
                continue;
            }
            // Hidden holder names, one per line.
            let owner = format!("§{:x}§r", i % 16);
            let fmt = match &line.1 {
                Some(right) => wire::NumberFormat::Fixed(right),
                None => wire::NumberFormat::Blank,
            };
            match wire::set_score(&owner, OBJ_SIDEBAR, (n - i) as i32, Some(&line.0), fmt) {
                Some(bytes) => packets::queue_raw(&me, bytes),
                None => jp(&me, "sb_score", json!({ "owner": horizon_kit::mini::to_legacy(&line.0), "objective": OBJ_SIDEBAR, "score": (n - i) as i32 })),
            }
        }
    }
    state(|st| st.players.get_mut(&uuid).map(|p| p.board = want));
}

/// Everything about one player worked out again and the changes sent.
fn refresh(player: &Player, everyone: &[Player], c: &Cfg, now: u64) {
    let uuid = player.get_unique_id();
    let world = player.get_world().get_name().to_owned();
    let group = group_of(c, player, &uuid);
    let name = state(|st| {
        let pl = st.players.entry(uuid.clone()).or_default();
        pl.group = group.clone();
        pl.name.clone()
    });
    if name.is_empty() {
        return;
    }
    let who = Who { player, uuid: &uuid, group: &group };
    let prop = |p: &str| c.users.user(&name, &uuid, &world, p).or_else(|| c.groups.group(&group, &world, p));
    let others: Vec<String> = everyone.iter().map(Player::get_unique_id).collect();
    if c.hf {
        header_footer(c, &who, &prop, now);
    }
    if c.names {
        tab_name(c, &who, &prop, now);
    }
    if c.teams {
        name_tag(c, &who, &prop, &others, now);
    }
    objectives(c, &who, &others, now);
    if c.bars {
        boss_bars(c, &who, now);
    }
    if c.boards {
        sidebar(c, &who, now);
    }
}

/// A joining viewer gets everyone's teams and the shared objectives with everyone's values.
fn introduce(viewer: &Player, everyone: &[Player], c: &Cfg) {
    let v = viewer.get_unique_id();
    let me = [v.clone()];
    type Row = (String, Option<String>, Option<Value>, Option<(i32, Value)>, Option<(i32, Value)>);
    let rows: Vec<Row> = state(|st| {
        everyone
            .iter()
            .filter(|p| p.get_unique_id() != v)
            .filter_map(|p| {
                let pl = st.players.get(&p.get_unique_id())?;
                Some((pl.name.clone(), pl.team.clone(), pl.team_params.clone(), pl.list_score.clone(), pl.below_score.clone()))
            })
            .collect()
    });
    if c.teams {
        for (name, team, params, _, _) in &rows {
            if let (Some(team), Some(params)) = (team, params) {
                send_team(&v, team, 0, params, std::slice::from_ref(name));
            }
        }
    }
    for (obj, slot, oname, is_list) in [(&c.list, 0, OBJ_LIST, true), (&c.below, 2, OBJ_BELOW, false)] {
        if !obj.enabled {
            continue;
        }
        let title = text::component(&obj.title, &c.text);
        jp(&me, "sb_objective", json!({ "name": oname, "method": 0, "display": title, "render": i32::from(obj.hearts) }));
        jp(&me, "sb_display", json!({ "slot": slot, "objective": oname }));
        for (name, _, _, list, below) in &rows {
            if let Some((value, fancy)) = if is_list { list } else { below } {
                match wire::set_score(name, oname, *value, None, wire::NumberFormat::Fixed(fancy)) {
                    Some(bytes) => packets::queue_raw(&me, bytes),
                    None => jp(&me, "sb_score", json!({ "owner": name, "objective": oname, "score": value })),
                }
            }
        }
        state(|st| {
            if let Some(p) = st.players.get_mut(&v) {
                if is_list {
                    p.has_list = true;
                } else {
                    p.has_below = true;
                }
            }
        });
    }
}

fn tick() {
    let c = cfg();
    let now = now_ms();
    let t = state(|st| {
        st.tick += 1;
        st.tick
    });
    let everyone = horizon_bukkit::get_online_players();
    // New players: their objectives first (so their own scores land), then their refresh.
    let new: Vec<Player> = state(|st| everyone.iter().filter(|p| !st.players.get(&p.get_unique_id()).is_some_and(|pl| !pl.name.is_empty())).cloned().collect());
    for p in &new {
        let uuid = p.get_unique_id();
        state(|st| {
            let (board_off, bar_off) = (st.board_off_saved.contains(p.get_name()), st.bar_off_saved.contains(p.get_name()));
            let pl = st.players.entry(uuid.clone()).or_default();
            pl.uuid = uuid.clone();
            pl.name = p.get_name().to_owned();
            pl.joined_ms = now;
            pl.board_off = if c.board_remember && board_off { true } else { c.board_hidden };
            pl.bars_off = if c.bar_remember && bar_off { true } else { c.bar_hidden };
        });
        introduce(p, &everyone, &c);
    }
    for p in &new {
        refresh(p, &everyone, &c, now);
    }
    // Everyone else in turn, spread over the refresh period.
    let period = c.refresh_ticks.max(1);
    for (i, p) in everyone.iter().enumerate() {
        if !new.contains(p) && (t + i as u64) % period == 0 {
            refresh(p, &everyone, &c, now);
        }
    }
    packets::flush();
}

fn quit(player: &Player) {
    let uuid = player.get_unique_id();
    text::forget(&uuid);
    let removed = state(|st| {
        let pl = st.players.remove(&uuid)?;
        if let Some(t) = &pl.team {
            st.team_owner.remove(t);
        }
        Some(pl)
    });
    let Some(pl) = removed else { return };
    let others: Vec<String> = horizon_bukkit::get_online_players().iter().map(Player::get_unique_id).filter(|u| *u != uuid).collect();
    if let Some(team) = &pl.team {
        for v in &others {
            send_team(v, team, 1, &Value::Null, &[]);
        }
    }
    for obj in [OBJ_LIST, OBJ_BELOW] {
        jp(&others, "sb_reset", json!({ "owner": pl.name, "objective": obj }));
    }
}

/// Everything TAB sent taken back (disable, reload).
fn undo_all() {
    let players: Vec<Pl> = state(|st| {
        st.team_owner.clear();
        std::mem::take(&mut st.players).into_values().collect()
    });
    let online = horizon_bukkit::get_online_players();
    let ids: Vec<String> = online.iter().map(Player::get_unique_id).collect();
    for pl in &players {
        if let Some(team) = &pl.team {
            for v in &ids {
                send_team(v, team, 1, &Value::Null, &[]);
            }
        }
        let me = [pl.uuid.clone()];
        for b in pl.bars.values() {
            if let Some(bytes) = wire::boss::remove(b.id) {
                packets::queue_raw(&me, bytes);
            }
        }
        if pl.board.is_some() {
            jp(&me, "sb_objective", json!({ "name": OBJ_SIDEBAR, "method": 1 }));
        }
        for (has, obj) in [(pl.has_list, OBJ_LIST), (pl.has_below, OBJ_BELOW)] {
            if has {
                jp(&me, "sb_objective", json!({ "name": obj, "method": 1 }));
            }
        }
        if pl.header.is_some() {
            jp(&me, "tab_list", json!({ "header": { "text": "" }, "footer": { "text": "" } }));
        }
        if pl.tab_name.is_some()
            && let Some(p) = online.iter().find(|p| p.get_unique_id() == pl.uuid)
        {
            p.set_player_list_name_component(None);
        }
    }
    packets::flush();
    text::clear();
}

// ---- playerdata.yml (remembered toggles) ----

fn save_playerdata() {
    let Some(p) = plugin() else { return };
    let (b, s) = state(|st| (st.bar_off_saved.clone(), st.board_off_saved.clone()));
    let mut b: Vec<String> = b.into_iter().collect();
    let mut s: Vec<String> = s.into_iter().collect();
    b.sort();
    s.sort();
    let yaml = serde_yaml::to_string(&json!({ "bossbar-off": b, "scoreboard-off": s })).unwrap_or_default();
    let path = p.get_data_folder().join("playerdata.yml");
    std::thread::spawn(move || {
        let _ = std::fs::write(path, yaml);
    });
}

fn load_playerdata() {
    let Some(p) = plugin() else { return };
    let c = Config::load(&p.get_data_folder(), "playerdata.yml", "bossbar-off: []\nscoreboard-off: []\n");
    let (b, s) = (c.list("bossbar-off"), c.list("scoreboard-off"));
    state(|st| {
        st.bar_off_saved = b.into_iter().collect();
        st.board_off_saved = s.into_iter().collect();
    });
}

// ---- Commands ----

fn find_online(name: &str) -> Option<Player> {
    horizon_bukkit::get_player(name).filter(|p| p.get_name().eq_ignore_ascii_case(name))
}

fn set_toggle(target: &Player, board: bool, on: Option<bool>, silent: bool) {
    let c = cfg();
    let uuid = target.get_unique_id();
    let name = target.get_name().to_owned();
    let now_off = state(|st| {
        let p = st.players.get_mut(&uuid)?;
        let cur = if board { &mut p.board_off } else { &mut p.bars_off };
        *cur = on.map_or(!*cur, |v| !v);
        let off = *cur;
        let set = if board { &mut st.board_off_saved } else { &mut st.bar_off_saved };
        if off {
            set.insert(name.clone());
        } else {
            set.remove(&name);
        }
        Some(off)
    });
    let Some(off) = now_off else { return };
    if (board && c.board_remember) || (!board && c.bar_remember) {
        save_playerdata();
    }
    if !silent {
        let key = match (board, off) {
            (true, false) => "scoreboard-toggle-on",
            (true, true) => "scoreboard-toggle-off",
            (false, false) => "bossbar-toggle-on",
            (false, true) => "bossbar-toggle-off",
        };
        target.send_message(&msg(key, &[]));
    }
    refresh(target, &horizon_bukkit::get_online_players(), &c, now_ms());
    packets::flush();
}

fn toggle_command(sender: &CommandSender, a: &[&str], board: bool) {
    let c = cfg();
    if (board && !c.boards) || (!board && !c.bars) {
        sender.send_message(&msg(if board { "scoreboard-feature-not-enabled" } else { "bossbar-feature-not-enabled" }, &[]));
        return;
    }
    let kind = if board { "scoreboard" } else { "bossbar" };
    let silent = a.contains(&"-s");
    let (on, rest) = match a.first().map(|m| m.to_ascii_lowercase()).as_deref() {
        Some("on") => (Some(true), &a[1..]),
        Some("off") => (Some(false), &a[1..]),
        Some("toggle") => (None, &a[1..]),
        _ => (None, a),
    };
    let target = match rest.first().filter(|t| **t != "-s") {
        Some(name) => {
            if !sender.has_permission(&format!("tab.{kind}.toggle.other")) {
                return sender.send_message(&msg("no-permission", &[]));
            }
            match find_online(name) {
                Some(p) => p,
                None => return sender.send_message(&msg("player-not-online", &[("player", name)])),
            }
        }
        None => {
            if !sender.has_permission(&format!("tab.{kind}.toggle")) {
                return sender.send_message(&msg("no-permission", &[]));
            }
            match sender.as_player() {
                Some(p) => p.clone(),
                None => return sender.send_message(&msg("command-only-from-game", &[])),
            }
        }
    };
    set_toggle(&target, board, on, silent);
}

fn property_command(sender: &CommandSender, a: &[&str], is_group: bool) {
    let properties = "§3Properties: §ftabprefix, tabsuffix, tagprefix, tagsuffix, customtabname, header, footer §3(§f<name> remove§3 clears all)";
    let (Some(name), Some(prop)) = (a.first(), a.get(1).map(|p| p.to_ascii_lowercase())) else {
        sender.send_message(properties);
        return;
    };
    let Some(p) = plugin() else { return };
    let file = p.get_data_folder().join(if is_group { "groups.yml" } else { "users.yml" });
    if prop == "remove" {
        if !(sender.has_permission("tab.remove") || sender.has_permission("tab.admin")) {
            return sender.send_message(&msg("no-permission", &[]));
        }
        props::remove_holder(&file, name);
        sender.send_message(&msg(if is_group { "group-data-removed" } else { "user-data-removed" }, &[("group", name), ("player", name)]));
    } else {
        let base = if is_group { "tab.change.group" } else { "tab.change.player" };
        if !(sender.has_permission(&format!("{base}.{prop}")) || sender.has_permission("tab.change.*") || sender.has_permission("tab.admin")) {
            return sender.send_message(&msg("no-permission", &[]));
        }
        let mut rest: Vec<&str> = a[2..].to_vec();
        let mut world: Option<String> = None;
        if let Some(i) = rest.iter().position(|x| *x == "-w") {
            world = rest.get(i + 1).map(|w| (*w).to_owned());
            rest.truncate(i);
        }
        let value = rest.join(" ").trim_matches('"').to_owned();
        props::set_property(&file, name, &prop, (!value.is_empty()).then_some(value.as_str()), world.as_deref());
        let (assigned, removed) = if is_group { ("group-value-assigned", "group-value-removed") } else { ("user-value-assigned", "user-value-removed") };
        if value.is_empty() {
            sender.send_message(&msg(removed, &[("property", &prop), ("group", name), ("player", name)]));
        } else {
            sender.send_message(&msg(assigned, &[("property", &prop), ("value", &value), ("group", name), ("player", name)]));
        }
    }
    if let Err(e) = reload() {
        sender.send_message(&format!("§c{e}"));
    }
}

fn need(sender: &CommandSender, perm: &str) -> bool {
    let ok = sender.has_permission(perm) || sender.has_permission("tab.admin");
    if !ok {
        sender.send_message(&msg("no-permission", &[]));
    }
    ok
}

fn board_or_bar(sender: &CommandSender, a: &[&str], board: bool) {
    match a.first().map(|x| x.to_ascii_lowercase()).as_deref() {
        Some("show") if board => {
            if !need(sender, "tab.scoreboard.show") {
                return;
            }
            let Some(name) = a.get(1) else { return sender.send_message(&msg("scoreboard-show-usage", &[])) };
            if !cfg().board_defs.iter().any(|b| b.name == *name) {
                return sender.send_message(&msg("scoreboard-not-found", &[("name", name)]));
            }
            let target = match a.get(2) {
                Some(t) => {
                    if !need(sender, "tab.scoreboard.show.other") {
                        return;
                    }
                    find_online(t)
                }
                None => sender.as_player().cloned(),
            };
            match target {
                Some(t) => {
                    let uuid = t.get_unique_id();
                    state(|st| st.players.get_mut(&uuid).map(|p| p.board_forced = Some((name.to_string(), u64::MAX))));
                }
                None => sender.send_message(&msg("player-not-online", &[("player", a.get(2).copied().unwrap_or(""))])),
            }
        }
        Some("announce") => {
            if !need(sender, if board { "tab.announce.scoreboard" } else { "tab.announce.bar" }) {
                return;
            }
            let (Some(name), Some(secs)) = (a.get(1), a.get(2).and_then(|s| s.parse::<u64>().ok())) else {
                return sender.send_message(&msg(if board { "scoreboard-announce-command-usage" } else { "bossbar-announce-command-usage" }, &[]));
            };
            let c = cfg();
            let until = now_ms() + secs * 1000;
            if board {
                if !c.board_defs.iter().any(|b| b.name == *name) {
                    return sender.send_message(&msg("scoreboard-not-found", &[("name", name)]));
                }
                state(|st| st.board_announce = Some((name.to_string(), until)));
            } else {
                if !c.bar_defs.iter().any(|b| b.name == *name) {
                    return sender.send_message(&msg("bossbar-not-found", &[("name", name)]));
                }
                state(|st| st.bar_announce.insert(name.to_string(), until));
                sender.send_message(&msg("bossbar-announcement-success", &[("bossbar", name), ("length", &secs.to_string())]));
            }
        }
        _ => toggle_command(sender, a, board),
    }
}

fn command(sender: &CommandSender, args: &[&str]) -> bool {
    let sub = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let a = args.get(1..).unwrap_or(&[]);
    match sub.as_str() {
        "reload" => {
            if need(sender, "tab.reload") {
                match reload() {
                    Ok(()) => sender.send_message(&msg("reload-success", &[])),
                    Err(e) => sender.send_message(&msg("reload-fail-file", &[("file", &e)])),
                }
            }
        }
        "parse" => {
            if !need(sender, "tab.parse") {
                return true;
            }
            let (Some(name), true) = (a.first(), a.len() > 1) else {
                sender.send_message(&msg("parse-command-usage", &[]));
                return true;
            };
            let input = a[1..].join(" ");
            let c = cfg();
            let now = now_ms();
            let target = if *name == "me" { sender.as_player().cloned() } else { find_online(name) };
            let out = match target {
                Some(p) => {
                    let uuid = p.get_unique_id();
                    let group = group_of(&c, &p, &uuid);
                    text::fill(&input, Some(&Who { player: &p, uuid: &uuid, group: &group }), &c.text, now)
                }
                None => text::fill(&input, None, &c.text, now),
            };
            sender.send_message(&format!("§6Replacing placeholder §e{input}§6: §r{}", horizon_kit::text::colorize(&out)));
            sender.send_message(&format!("§6Raw: §r{}", out.replace('§', "&")));
        }
        "group" | "player" => property_command(sender, a, sub == "group"),
        "grouplist" => {
            if need(sender, "tab.grouplist") {
                sender.send_message(&format!("§3Configured groups: §f{}", cfg().groups.holders().join(", ")));
            }
        }
        "groupinfo" => {
            if need(sender, "tab.groupinfo") {
                let rows: Vec<(String, String)> = state(|st| st.players.values().map(|p| (p.name.clone(), p.group.clone())).collect());
                let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for (n, g) in rows {
                    by.entry(g).or_default().push(n);
                }
                for (g, names) in by {
                    sender.send_message(&format!("§f{g}§7: {}", names.join(", ")));
                }
            }
        }
        "scoreboard" => board_or_bar(sender, a, true),
        "bossbar" => board_or_bar(sender, a, false),
        "announce" => {
            let board = a.first().is_some_and(|k| k.eq_ignore_ascii_case("scoreboard"));
            let mut rest = vec!["announce"];
            rest.extend_from_slice(a.get(1..).unwrap_or(&[]));
            board_or_bar(sender, &rest, board);
        }
        "send" => {
            if !need(sender, "tab.send.bar") {
                return true;
            }
            let (Some(kind), Some(name), Some(bar), Some(secs)) = (a.first(), a.get(1), a.get(2), a.get(3).and_then(|s| s.parse::<u64>().ok())) else {
                sender.send_message(&msg("send-command-usage", &[]));
                return true;
            };
            if !kind.eq_ignore_ascii_case("bar") {
                sender.send_message(&msg("send-command-usage", &[]));
                return true;
            }
            let Some(target) = find_online(name) else {
                sender.send_message(&msg("player-not-online", &[("player", name)]));
                return true;
            };
            if !cfg().bar_defs.iter().any(|b| b.name == *bar) {
                sender.send_message(&msg("bossbar-not-found", &[("name", bar)]));
                return true;
            }
            let uuid = target.get_unique_id();
            state(|st| st.players.get_mut(&uuid).map(|p| p.announced.insert(bar.to_string(), now_ms() + secs * 1000)));
            sender.send_message(&msg("bossbar-send-success", &[("bossbar", bar), ("player", target.get_name()), ("length", &secs.to_string())]));
        }
        "nametag" => {
            let mode = a.first().map(|x| x.to_ascii_lowercase()).unwrap_or_else(|| "toggle".into());
            let target = match a.get(1) {
                Some(n) => {
                    if !need(sender, "tab.nametag.toggle.other") {
                        return true;
                    }
                    find_online(n)
                }
                None => {
                    if !need(sender, "tab.nametag.toggle") {
                        return true;
                    }
                    sender.as_player().cloned()
                }
            };
            match target {
                Some(t) => {
                    let uuid = t.get_unique_id();
                    state(|st| {
                        if let Some(p) = st.players.get_mut(&uuid) {
                            p.tag_hidden = match mode.as_str() {
                                "hide" => true,
                                "show" => false,
                                _ => !p.tag_hidden,
                            };
                        }
                    });
                }
                None => sender.send_message(&msg("player-not-online", &[("player", a.get(1).copied().unwrap_or(""))])),
            }
        }
        "setcollision" => {
            if !need(sender, "tab.setcollision") {
                return true;
            }
            match (a.first().and_then(|n| find_online(n)), a.get(1).and_then(|v| v.parse::<bool>().ok())) {
                (Some(t), Some(v)) => {
                    let uuid = t.get_unique_id();
                    state(|st| st.players.get_mut(&uuid).map(|p| p.collision = Some(v)));
                }
                (None, Some(_)) => sender.send_message(&msg("player-not-online", &[("player", a.first().copied().unwrap_or(""))])),
                _ => sender.send_message(&msg("collision-command-usage", &[])),
            }
        }
        "cpu" => {
            if need(sender, "tab.cpu") {
                let n = state(|st| st.players.len());
                sender.send_message(&format!("§3TAB (Rust): §f{n}§3 players, a refresh every §f{}§3 ticks each, packets in one batch a tick.", cfg().refresh_ticks));
            }
        }
        "debug" => {
            if need(sender, "tab.debug") {
                let target = a.first().and_then(|n| find_online(n)).or_else(|| sender.as_player().cloned());
                if let Some(t) = target {
                    let uuid = t.get_unique_id();
                    let info = state(|st| st.players.get(&uuid).map(|p| format!("group §f{}§3, team §f{}", p.group, p.team.clone().unwrap_or_default())));
                    sender.send_message(&format!("§3{}: {}", t.get_name(), info.unwrap_or_else(|| "not loaded".into())));
                }
            }
        }
        _ => {
            if sender.has_permission("tab.admin") {
                for l in cfg().msgs.list("help-menu") {
                    sender.send_message(&horizon_bukkit::chat_color::translate_alternate_color_codes('&', &l));
                }
            } else {
                sender.send_message(&format!("§3TAB v{} (Rust) by NEZNAMY", env!("CARGO_PKG_VERSION")));
            }
        }
    }
    true
}

fn complete(args: &[&str]) -> Vec<String> {
    let typed = args.last().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let names = || horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).collect::<Vec<_>>();
    let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let opts: Vec<String> = match (args.len(), args.first().map(|a| a.to_ascii_lowercase()).as_deref()) {
        (0 | 1, _) => words(&["announce", "bossbar", "cpu", "debug", "group", "groupinfo", "grouplist", "nametag", "parse", "player", "reload", "scoreboard", "send", "setcollision"]),
        (2, Some("scoreboard")) => words(&["on", "off", "toggle", "show", "announce"]),
        (2, Some("bossbar")) => words(&["on", "off", "toggle", "announce"]),
        (2, Some("group")) => cfg().groups.holders(),
        (2, Some("player" | "parse" | "setcollision" | "debug")) => names(),
        (3, Some("group" | "player")) => words(&["tabprefix", "tabsuffix", "tagprefix", "tagsuffix", "customtabname", "header", "footer", "remove"]),
        (3, Some("scoreboard")) => cfg().board_defs.iter().map(|b| b.name.clone()).collect(),
        (3, Some("bossbar")) => cfg().bar_defs.iter().map(|b| b.name.clone()).collect(),
        (2, Some("nametag")) => words(&["toggle", "show", "hide"]),
        _ => Vec::new(),
    };
    opts.into_iter().filter(|o| o.to_ascii_lowercase().starts_with(&typed)).collect()
}

/// `/sb` and `/bossbar` (the configured toggle commands): registered once.
fn register_toggles(p: Plugin) {
    static DONE: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let c = cfg();
    for (cmd, board, on) in [(c.board_toggle.clone(), true, c.boards), (c.bar_toggle.clone(), false, c.bars)] {
        let name = cmd.trim_start_matches('/').to_ascii_lowercase();
        if !on || name.is_empty() || DONE.lock().unwrap_or_else(|e| e.into_inner()).contains(&name) {
            continue;
        }
        let command = horizon_bukkit::pure2::Command::new(&name);
        if horizon_bukkit::fam_core::get_command_map().register(&p, "tab", &command) {
            p.set_executor(&name, move |sender, _, args| {
                toggle_command(sender, args, board);
                true
            });
            DONE.lock().unwrap_or_else(|e| e.into_inner()).push(name);
        }
    }
}

fn reload() -> Result<(), String> {
    let c = load()?;
    undo_all();
    *CFG.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(c));
    horizon_kit::hooks::refresh();
    load_playerdata();
    if let Some(p) = plugin() {
        register_toggles(p);
    }
    Ok(())
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    if let Err(e) = reload() {
        plugin.severe(&format!("Failed to load the configuration: {e}"));
    }
    plugin.register_event_with("MONITOR", false, |e: &mut PlayerQuitEvent| quit(e.get_player()));
    plugin.set_executor("tab", |sender, _, args| command(sender, args));
    plugin.set_tab_completer("tab", |_, _, args| complete(args));
    plugin.run_task_timer(1, 1, tick);
    plugin.info("Enabled (Rust): header/footer, tab names, name tags, objectives, boss bars, sidebars");
}

fn on_disable(_plugin: Plugin) {
    undo_all();
    *CFG.write().unwrap_or_else(|e| e.into_inner()) = None;
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sort_parts() {
        assert_eq!(base36(0, 2), "00");
        assert_eq!(base36(37, 2), "11");
        let st = State::default();
        assert_eq!(team_name(&st, "00", "Notch_With_A_Long_Name", "u"), "00Notch_With_A_L");
        assert_eq!(number("&c12.6"), 13);
    }
}
