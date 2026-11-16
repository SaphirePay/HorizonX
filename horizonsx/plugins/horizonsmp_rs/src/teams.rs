//! Teams: a group of up to `max-members` players with a leader, officers, a home, team chat and no friendly fire
//! (unless they switch it on). Members can build in each other's claims (claims.team-members-trusted).

use crate::{conf, data, ui};
use horizon_bukkit::prelude::*;
use horizon_kit::app;
use horizon_kit::store::File;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const SECTION: &str = "Teams";
pub const MEMBER: u8 = 0;
pub const OFFICER: u8 = 1;
pub const LEADER: u8 = 2;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Member {
    pub uuid: String,
    pub name: String,
    pub role: u8,
    pub joined: u64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Spot {
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
}

impl Spot {
    pub fn of(l: &Location) -> Spot {
        Spot { world: l.get_world().get_name().to_owned(), x: l.x, y: l.y, z: l.z, yaw: l.yaw, pitch: l.pitch }
    }
    pub fn location(&self) -> Option<Location> {
        horizon_bukkit::get_world(&self.world).map(|w| Location::new(w, self.x, self.y, self.z, self.yaw, self.pitch))
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Team {
    pub name: String,
    pub members: Vec<Member>,
    pub home: Option<Spot>,
    pub created: u64,
    pub friendly_fire: bool,
    pub description: String,
}

impl Team {
    pub fn leader(&self) -> Option<&Member> {
        self.members.iter().find(|m| m.role == LEADER)
    }
    pub fn role_of(&self, uuid: &str) -> Option<u8> {
        self.members.iter().find(|m| m.uuid == uuid).map(|m| m.role)
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Db {
    teams: BTreeMap<String, Team>,
}

struct Invite {
    team: String,
    by: String,
    until: u64,
}

#[derive(Default)]
struct State {
    db: Db,
    of: HashMap<String, String>,
    invites: HashMap<String, Vec<Invite>>,
}

impl State {
    fn rebuild(&mut self) {
        self.of.clear();
        for (key, t) in &self.db.teams {
            for m in &t.members {
                self.of.insert(m.uuid.clone(), key.clone());
            }
        }
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut held = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(held.get_or_insert_with(State::default))
}

fn file() -> &'static File<Db> {
    static FILE: OnceLock<File<Db>> = OnceLock::new();
    FILE.get_or_init(|| File::open(&app::folder().join("teams.json"), Duration::from_secs(3)))
}

fn changed(st: &mut State) {
    st.rebuild();
    let copy = st.db.clone();
    file().edit(|d| *d = copy);
}

pub fn save() {
    file().save_now();
}

pub fn load() {
    let db = file().get();
    with(|st| {
        st.db = db;
        st.rebuild();
    });
}

pub fn team_of(uuid: &str) -> Option<Team> {
    with(|st| st.of.get(uuid).and_then(|k| st.db.teams.get(k)).cloned())
}

pub fn team_name(uuid: &str) -> Option<String> {
    team_of(uuid).map(|t| t.name)
}

pub fn same_team(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && with(|st| matches!((st.of.get(a), st.of.get(b)), (Some(x), Some(y)) if x == y))
}

/// Whether two players in the same team may hurt each other.
pub fn friendly_fire(a: &str, b: &str) -> Option<bool> {
    with(|st| {
        let (x, y) = (st.of.get(a)?, st.of.get(b)?);
        if x != y {
            return None;
        }
        st.db.teams.get(x).map(|t| t.friendly_fire)
    })
}

fn edit_team<R>(key: &str, f: impl FnOnce(&mut Team) -> R) -> Option<R> {
    with(|st| {
        let t = st.db.teams.get_mut(key)?;
        let r = f(t);
        changed(st);
        Some(r)
    })
}

fn tell_team(t: &Team, text: &str) {
    for m in &t.members {
        if let Some(p) = horizon_bukkit::fam_ess::get_player_by_id(&m.uuid) {
            ui::say(&p, SECTION, text);
        }
    }
}

/// The most members a team may have: config, or its leader's `horizonsmp.team-size` LuckPerms meta.
pub fn team_max(t: &Team) -> usize {
    t.leader().and_then(|l| conf::meta_int(&l.uuid, "horizonsmp.team-size")).map_or(conf::get().team_max, |n| n.max(2) as usize)
}

fn role_name(r: u8) -> &'static str {
    match r {
        LEADER => "Leader",
        OFFICER => "Officer",
        _ => "Member",
    }
}

fn role_badge(r: u8) -> &'static str {
    match r {
        LEADER => "<#facc15>★",
        OFFICER => "<#22d3ee>✦",
        _ => "<#71717a>•",
    }
}

fn now() -> u64 {
    horizon_kit::time::now_ms()
}

fn valid_name(name: &str) -> Result<(), String> {
    let s = conf::get();
    let n = name.chars().count();
    if n < s.team_name_min || n > s.team_name_max {
        return Err(format!("A team name is {} to {} letters long.", s.team_name_min, s.team_name_max));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("A team name may only have letters, numbers and _.".into());
    }
    Ok(())
}

// ---- Actions ----

fn create(p: &Player, name: &str) {
    if !crate::allowed(p, "horizonsmp.teams.create") {
        return;
    }
    if let Err(e) = valid_name(name) {
        ui::say(p, SECTION, &format!("<#f87171>{e}"));
        return;
    }
    let uuid = p.get_unique_id();
    if team_of(&uuid).is_some() {
        ui::say(p, SECTION, "<#f87171>You're already in a team. Leave it first with <white>/team leave</white>.");
        return;
    }
    let key = name.to_ascii_lowercase();
    let made = with(|st| {
        if st.db.teams.contains_key(&key) {
            return false;
        }
        st.db.teams.insert(key.clone(), Team { name: name.to_owned(), members: vec![Member { uuid: uuid.clone(), name: p.get_name().to_owned(), role: LEADER, joined: now() }], home: None, created: now(), friendly_fire: false, description: String::new() });
        changed(st);
        true
    });
    if !made {
        ui::say(p, SECTION, "<#f87171>A team with that name already exists.");
        return;
    }
    ui::say(p, SECTION, &format!("<#4ade80>Team <white>{name}</white> created!</#4ade80> Invite friends with <click:suggest_command:/team invite ><#22d3ee><u>/team invite <player></u></#22d3ee></click>."));
    ui::title(p, &ui::grad(&format!("<bold>{}</bold>", name.to_ascii_uppercase())), "<#a1a1aa>your team is ready");
    ui::fanfare(p);
    horizon_bukkit::server::send_rich_message(&format!("<gradient:{}:{}><bold>Teams</bold></gradient> <#3f3f46>» <white>{}</white> <#a1a1aa>founded the team <#22d3ee>{name}</#22d3ee>.", ui::ACCENT, ui::ACCENT2, p.get_name()));
}

fn invite(p: &Player, target: &str) {
    let uuid = p.get_unique_id();
    let Some(t) = team_of(&uuid) else {
        ui::say(p, SECTION, "<#f87171>You're not in a team. <white>/team create <name></white>");
        return;
    };
    if t.role_of(&uuid).unwrap_or(0) < OFFICER {
        ui::say(p, SECTION, "<#f87171>Only the leader and officers can invite.");
        return;
    }
    let Some(other) = horizon_bukkit::get_online_players().into_iter().find(|o| o.get_name().eq_ignore_ascii_case(target)) else {
        ui::say(p, SECTION, &format!("<#f87171>{} isn't online.", ui::esc(target)));
        return;
    };
    let ou = other.get_unique_id();
    if ou == uuid {
        return;
    }
    if team_of(&ou).is_some() {
        ui::say(p, SECTION, &format!("<#f87171>{} is already in a team.", other.get_name()));
        return;
    }
    if t.members.len() >= team_max(&t) {
        ui::say(p, SECTION, &format!("<#f87171>Your team is full ({} players).", team_max(&t)));
        return;
    }
    let key = t.name.to_ascii_lowercase();
    with(|st| {
        let list = st.invites.entry(ou.clone()).or_default();
        list.retain(|i| i.team != key);
        list.push(Invite { team: key.clone(), by: p.get_name().to_owned(), until: now() + 120_000 });
    });
    ui::say(p, SECTION, &format!("Invited <white>{}</white>. The invite lasts 2 minutes.", other.get_name()));
    ui::say(&other, SECTION, &format!("<white>{}</white> invited you to the team <#22d3ee>{}</#22d3ee>. <click:run_command:/team accept {}><hover:show_text:'Join {}'><#4ade80><bold>[JOIN]</bold></#4ade80></hover></click> <click:run_command:/team deny {}><#f87171><bold>[NO]</bold></#f87171></click>", p.get_name(), t.name, t.name, t.name, t.name));
    ui::sound(&other, "BLOCK_NOTE_BLOCK_PLING:0.8:1.2");
}

fn accept(p: &Player, which: Option<&str>) {
    let uuid = p.get_unique_id();
    if team_of(&uuid).is_some() {
        ui::say(p, SECTION, "<#f87171>Leave your team first.");
        return;
    }
    let t = now();
    let inv = with(|st| {
        let list = st.invites.get_mut(&uuid)?;
        list.retain(|i| i.until > t);
        let at = match which {
            Some(w) => list.iter().position(|i| i.team.eq_ignore_ascii_case(w))?,
            None => list.len().checked_sub(1)?,
        };
        Some(list.remove(at))
    });
    let Some(inv) = inv else {
        ui::say(p, SECTION, "<#f87171>You have no invite to accept (they last 2 minutes).");
        return;
    };
    let max = with(|st| st.db.teams.get(&inv.team).cloned()).map_or(conf::get().team_max, |t| team_max(&t));
    let joined = edit_team(&inv.team, |team| {
        if team.members.len() >= max {
            return None;
        }
        team.members.push(Member { uuid: uuid.clone(), name: p.get_name().to_owned(), role: MEMBER, joined: now() });
        Some(team.clone())
    })
    .flatten();
    match joined {
        Some(team) => {
            tell_team(&team, &format!("<#4ade80>{}</#4ade80> joined the team (invited by {}).", p.get_name(), inv.by));
            ui::title(p, &ui::grad(&format!("<bold>{}</bold>", team.name.to_ascii_uppercase())), "<#a1a1aa>welcome to the team");
            ui::fanfare(p);
        }
        None => ui::say(p, SECTION, "<#f87171>That team is full or gone."),
    }
}

fn deny(p: &Player, which: Option<&str>) {
    let uuid = p.get_unique_id();
    with(|st| {
        if let Some(list) = st.invites.get_mut(&uuid) {
            match which {
                Some(w) => list.retain(|i| !i.team.eq_ignore_ascii_case(w)),
                None => list.clear(),
            }
        }
    });
    ui::say(p, SECTION, "Invite declined.");
}

fn leave(p: &Player) {
    let uuid = p.get_unique_id();
    let Some(t) = team_of(&uuid) else {
        ui::say(p, SECTION, "You're not in a team.");
        return;
    };
    let key = t.name.to_ascii_lowercase();
    if t.role_of(&uuid) == Some(LEADER) && t.members.len() > 1 {
        ui::say(p, SECTION, "<#f87171>You lead this team. Give it to someone with <white>/team transfer <player></white> or <white>/team disband</white> it.");
        return;
    }
    if t.members.len() <= 1 {
        disband_now(&key);
        ui::say(p, SECTION, &format!("You left, and the team <white>{}</white> is no more.", t.name));
        return;
    }
    edit_team(&key, |team| team.members.retain(|m| m.uuid != uuid));
    ui::say(p, SECTION, &format!("You left <white>{}</white>.", t.name));
    if let Some(t) = with(|st| st.db.teams.get(&key).cloned()) {
        tell_team(&t, &format!("<#f87171>{}</#f87171> left the team.", p.get_name()));
    }
}

fn disband_now(key: &str) {
    with(|st| {
        st.db.teams.remove(key);
        changed(st);
    });
}

fn kick(p: &Player, who: &str) {
    let uuid = p.get_unique_id();
    let Some(t) = team_of(&uuid) else { return ui::say(p, SECTION, "You're not in a team.") };
    let mine = t.role_of(&uuid).unwrap_or(0);
    let Some(target) = t.members.iter().find(|m| m.name.eq_ignore_ascii_case(who)).cloned() else {
        return ui::say(p, SECTION, &format!("<#f87171>{} isn't in your team.", ui::esc(who)));
    };
    if mine < OFFICER || target.role >= mine {
        return ui::say(p, SECTION, "<#f87171>You can't kick them.");
    }
    let key = t.name.to_ascii_lowercase();
    edit_team(&key, |team| team.members.retain(|m| m.uuid != target.uuid));
    if let Some(t2) = with(|st| st.db.teams.get(&key).cloned()) {
        tell_team(&t2, &format!("<#f87171>{}</#f87171> was removed from the team by {}.", target.name, p.get_name()));
    }
    if let Some(o) = horizon_bukkit::fam_ess::get_player_by_id(&target.uuid) {
        ui::say(&o, SECTION, &format!("<#f87171>You were removed from {}.", t.name));
    }
}

fn set_role(p: &Player, who: &str, up: bool) {
    let uuid = p.get_unique_id();
    let Some(t) = team_of(&uuid) else { return ui::say(p, SECTION, "You're not in a team.") };
    if t.role_of(&uuid) != Some(LEADER) {
        return ui::say(p, SECTION, "<#f87171>Only the leader can do that.");
    }
    let key = t.name.to_ascii_lowercase();
    let name = who.to_owned();
    let done = edit_team(&key, |team| {
        let m = team.members.iter_mut().find(|m| m.name.eq_ignore_ascii_case(&name) && m.role != LEADER)?;
        m.role = if up { OFFICER } else { MEMBER };
        Some((m.name.clone(), m.role))
    })
    .flatten();
    match done {
        Some((n, r)) => {
            if let Some(t2) = team_of(&uuid) {
                tell_team(&t2, &format!("<white>{n}</white> is now a{} <#22d3ee>{}</#22d3ee>.", if r == OFFICER { "n" } else { "" }, role_name(r)));
            }
        }
        None => ui::say(p, SECTION, "<#f87171>No such member."),
    }
}

fn transfer(p: &Player, who: &str) {
    let uuid = p.get_unique_id();
    let Some(t) = team_of(&uuid) else { return ui::say(p, SECTION, "You're not in a team.") };
    if t.role_of(&uuid) != Some(LEADER) {
        return ui::say(p, SECTION, "<#f87171>Only the leader can do that.");
    }
    let key = t.name.to_ascii_lowercase();
    let name = who.to_owned();
    let me = uuid.clone();
    let done = edit_team(&key, |team| {
        let target = team.members.iter().position(|m| m.name.eq_ignore_ascii_case(&name) && m.uuid != me)?;
        for m in &mut team.members {
            if m.uuid == me {
                m.role = OFFICER;
            }
        }
        team.members[target].role = LEADER;
        Some(team.members[target].name.clone())
    })
    .flatten();
    match done {
        Some(n) => {
            if let Some(t2) = team_of(&uuid) {
                tell_team(&t2, &format!("<#facc15>★ {n}</#facc15> now leads the team."));
            }
        }
        None => ui::say(p, SECTION, "<#f87171>No such member."),
    }
}

fn set_home(p: &Player) {
    let uuid = p.get_unique_id();
    let Some(t) = team_of(&uuid) else { return ui::say(p, SECTION, "You're not in a team.") };
    if t.role_of(&uuid).unwrap_or(0) < OFFICER {
        return ui::say(p, SECTION, "<#f87171>Only the leader and officers can set the home.");
    }
    let l = p.get_location();
    if let Some(c) = crate::claims::claim_at_loc(&l)
        && !crate::claims::may(p, &c, crate::claims::BUILD)
    {
        return ui::say(p, SECTION, "<#f87171>You can't set the team home in someone else's claim.");
    }
    let spot = Spot::of(&l);
    edit_team(&t.name.to_ascii_lowercase(), |team| team.home = Some(spot));
    if let Some(t2) = team_of(&uuid) {
        tell_team(&t2, &format!("<white>{}</white> set the team home at <white>{}, {}, {}</white>.", p.get_name(), l.get_block_x(), l.get_block_y(), l.get_block_z()));
    }
    ui::good(p);
}

fn go_home(p: &Player) {
    if !crate::allowed(p, "horizonsmp.teams.home") {
        return;
    }
    let uuid = p.get_unique_id();
    let Some(t) = team_of(&uuid) else { return ui::say(p, SECTION, "You're not in a team.") };
    let Some(home) = t.home.and_then(|h| h.location()) else {
        return ui::say(p, SECTION, "Your team has no home yet. An officer can set it with <white>/team sethome</white>.");
    };
    if crate::combat::tagged(&uuid) {
        return ui::deny(p, "You can't teleport while in combat.");
    }
    crate::warmup(p, conf::get().team_warmup, "the team home", home);
}

fn team_chat(p: &Player, msg: &str) {
    if !crate::allowed(p, "horizonsmp.teams.chat") {
        return;
    }
    let uuid = p.get_unique_id();
    let Some(t) = team_of(&uuid) else { return ui::say(p, SECTION, "You're not in a team.") };
    let r = t.role_of(&uuid).unwrap_or(0);
    let line = format!("<#22d3ee><bold>TEAM</bold></#22d3ee> {} <white>{}</white><#52525b>: <#e4e4e7>{}", role_badge(r), p.get_name(), ui::esc(msg));
    for m in &t.members {
        if let Some(o) = horizon_bukkit::fam_ess::get_player_by_id(&m.uuid) {
            ui::raw(&o, &line);
        }
    }
    app::info(&format!("[Team {}] {}: {}", t.name, p.get_name(), msg));
}

fn info(p: &Player, which: Option<&str>) {
    let t = match which {
        Some(n) => with(|st| st.db.teams.get(&n.to_ascii_lowercase()).cloned()),
        None => team_of(&p.get_unique_id()),
    };
    let Some(t) = t else {
        return ui::say(p, SECTION, if which.is_some() { "<#f87171>No team by that name." } else { "You're not in a team. <white>/team create <name></white>" });
    };
    let members: Vec<String> = t
        .members
        .iter()
        .map(|m| {
            let online = horizon_bukkit::fam_ess::get_player_by_id(&m.uuid).is_some();
            format!("{} {}{}", role_badge(m.role), if online { "<#4ade80>" } else { "<#71717a>" }, m.name)
        })
        .collect();
    let power = power(&t);
    ui::raw(p, &format!(
        "\n {}  <#71717a>{} member(s) • power {}\n {}\n <#a1a1aa>Home: <white>{}</white>   <#a1a1aa>Friendly fire: {}\n",
        ui::grad(&format!("<bold>{}</bold>", t.name.to_ascii_uppercase())),
        t.members.len(),
        ui::num(power),
        members.join("<#3f3f46>,  "),
        if t.home.is_some() { "set" } else { "none" },
        if t.friendly_fire { "<#f87171>on" } else { "<#4ade80>off" }
    ));
}

/// Kills × 10 + hours played, over the members: what /team top sorts by.
pub fn power(t: &Team) -> i64 {
    data::summaries().read(|m| t.members.iter().filter_map(|mem| m.get(&mem.uuid)).map(|s| i64::from(s.kills) * 10 + (s.playtime / 3600) as i64).sum())
}

fn list(p: &Player) {
    let mut teams: Vec<(i64, Team)> = with(|st| st.db.teams.values().cloned().collect::<Vec<_>>()).into_iter().map(|t| (power(&t), t)).collect();
    teams.sort_by(|a, b| b.0.cmp(&a.0));
    if teams.is_empty() {
        return ui::say(p, SECTION, "No teams yet. Be the first: <white>/team create <name></white>");
    }
    let mut lines = vec![String::new(), format!(" {}", ui::grad("<bold>TOP TEAMS</bold>"))];
    for (i, (pw, t)) in teams.iter().take(10).enumerate() {
        let medal = match i {
            0 => "<#facc15>",
            1 => "<#d4d4d8>",
            2 => "<#fb923c>",
            _ => "<#71717a>",
        };
        lines.push(format!(" {medal}#{}</#> <white>{}</white> <#71717a>• {} members • power {}", i + 1, t.name, t.members.len(), ui::num(*pw)));
    }
    lines.push(String::new());
    ui::raw(p, &lines.join("\n"));
}

// ---- Menu ----

pub fn open(p: &Player) {
    if !crate::allowed(p, "horizonsmp.teams") {
        return;
    }
    let uuid = p.get_unique_id();
    match team_of(&uuid) {
        None => open_none(p),
        Some(t) => open_team(p, &t),
    }
}

fn open_none(p: &Player) {
    let uuid = p.get_unique_id();
    let mut m = ui::frame(4, "⚑ Teams");
    m.button(11, ui::icon("WHITE_BANNER", "<#4ade80><bold>Create a team", &ui::lines(&["", "<#a1a1aa>Start your own team and", "<#a1a1aa>invite your friends.", "", "<#a1a1aa>Members share claims and", "<#a1a1aa>can't hurt each other.", "", "<#22d3ee>▸ Click, then type the name"])), |c| {
        c.close();
        ui::say(&c.player, SECTION, "Type <click:suggest_command:/team create ><#22d3ee><u>/team create <name></u></#22d3ee></click> to found your team.");
    });
    let t = now();
    let invites: Vec<(String, String)> = with(|st| st.invites.get(&uuid).map(|l| l.iter().filter(|i| i.until > t).map(|i| (i.team.clone(), i.by.clone())).collect()).unwrap_or_default());
    if invites.is_empty() {
        m.item(15, ui::icon("PAPER", "<#a1a1aa><bold>No invites", &ui::lines(&["", "<#71717a>When someone invites you,", "<#71717a>it shows here."])));
    } else {
        let (team, by) = invites.last().cloned().unwrap_or_default();
        m.button(15, ui::icon("MAP", &format!("<#22d3ee><bold>Invite: {team}"), &[String::new(), format!("<#a1a1aa>From <white>{by}"), String::new(), "<#4ade80>▸ Left-click to join".into(), "<#f87171>▸ Right-click to decline".into()]), move |c| {
            c.close();
            if c.click.is_right() { deny(&c.player, Some(&team)) } else { accept(&c.player, Some(&team)) }
        });
    }
    m.button(13, ui::icon("GOLDEN_HELMET", "<#facc15><bold>Top teams", &ui::lines(&["", "<#a1a1aa>The strongest teams.", "", "<#22d3ee>▸ Click to see"])), |c| {
        c.close();
        list(&c.player);
    });
    ui::back_button(&mut m, "the server menu", crate::menu::open);
    ui::close_button(&mut m);
    m.open(p);
}

fn open_team(p: &Player, t: &Team) {
    let uuid = p.get_unique_id();
    let me = t.role_of(&uuid).unwrap_or(0);
    let mut m = ui::frame(6, &format!("⚑ {}", t.name));
    let mut banner = ui::icon("PURPLE_BANNER", &ui::grad(&format!("<bold>{}</bold>", t.name.to_ascii_uppercase())), &[
        String::new(),
        format!("<#a1a1aa>Members  <white>{}/{}", t.members.len(), team_max(t)),
        format!("<#a1a1aa>Power      <white>{}", ui::num(power(t))),
        format!("<#a1a1aa>Your role  <white>{}", role_name(me)),
        format!("<#a1a1aa>Friendly fire {}", if t.friendly_fire { "<#f87171>on" } else { "<#4ade80>off" }),
    ]);
    ui::glint(&mut banner);
    m.item(4, banner);
    let mut members = t.members.clone();
    members.sort_by(|a, b| b.role.cmp(&a.role).then(a.name.cmp(&b.name)));
    for (slot, mem) in (19..26).chain(28..35).zip(members.iter()) {
        let online = horizon_bukkit::fam_ess::get_player_by_id(&mem.uuid).is_some();
        let s = data::summaries().read(|m| m.get(&mem.uuid).cloned()).unwrap_or_default();
        let mut lore = vec![
            String::new(),
            format!("<#a1a1aa>Role       {} <white>{}", role_badge(mem.role), role_name(mem.role)),
            format!("<#a1a1aa>Status    {}", if online { "<#4ade80>● online" } else { "<#71717a>● offline" }),
            format!("<#a1a1aa>Kills       <white>{}", s.kills),
            format!("<#a1a1aa>Playtime <white>{}", ui::duration(s.playtime)),
        ];
        if me == LEADER && mem.uuid != uuid {
            lore.push(String::new());
            lore.push("<#22d3ee>▸ Left-click: promote / demote".into());
            lore.push("<#f87171>▸ Shift-right-click: kick".into());
        }
        let name = mem.name.clone();
        let role = mem.role;
        m.button(slot, ui::head(&mem.name, &format!("{}<bold>{}", if online { "<white>" } else { "<#a1a1aa>" }, mem.name), &lore), move |c| {
            let my = team_of(&c.player.get_unique_id()).and_then(|t| t.role_of(&c.player.get_unique_id())).unwrap_or(0);
            if my != LEADER || name.eq_ignore_ascii_case(c.player.get_name()) {
                return;
            }
            if c.click == horizon_kit::gui::Click::ShiftRight {
                kick(&c.player, &name);
            } else if c.click.is_left() {
                set_role(&c.player, &name, role == MEMBER);
            }
            ui::click(&c.player);
            open(&c.player);
        });
    }
    m.button(47, ui::icon("RED_BED", "<#22d3ee><bold>Team home", &[String::new(), format!("<#a1a1aa>{}", if t.home.is_some() { "Teleport to your team's home." } else { "No home set yet." }), String::new(), "<#22d3ee>▸ Click to go".into()]), |c| {
        c.close();
        go_home(&c.player);
    });
    if me >= OFFICER {
        m.button(48, ui::icon("COMPASS", "<#facc15><bold>Set home here", &ui::lines(&["", "<#a1a1aa>Move the team home to", "<#a1a1aa>where you stand.", "", "<#22d3ee>▸ Click to set"])), |c| {
            c.close();
            set_home(&c.player);
        });
        m.button(50, ui::icon("WRITABLE_BOOK", "<#4ade80><bold>Invite a player", &ui::lines(&["", "<#a1a1aa>/team invite <player>", "", "<#22d3ee>▸ Click for the command"])), |c| {
            c.close();
            ui::say(&c.player, SECTION, "<click:suggest_command:/team invite ><#22d3ee><u>/team invite <player></u></#22d3ee></click>");
        });
    }
    if me == LEADER {
        let ff = t.friendly_fire;
        m.button(51, ui::icon(if ff { "IRON_SWORD" } else { "WOODEN_SWORD" }, "<#f87171><bold>Friendly fire", &[String::new(), format!("<#a1a1aa>Status  {}", if ff { "<#f87171>● On" } else { "<#4ade80>● Off" }), String::new(), "<#22d3ee>▸ Click to switch".into()]), |c| {
            if let Some(t) = team_of(&c.player.get_unique_id()) {
                edit_team(&t.name.to_ascii_lowercase(), |t| t.friendly_fire = !t.friendly_fire);
            }
            ui::click(&c.player);
            open(&c.player);
        });
    }
    m.button(46, ui::icon("OAK_SIGN", "<#a1a1aa><bold>Team chat", &ui::lines(&["", "<#a1a1aa>/tc <message>", "<#71717a>Only your team sees it."])), |c| {
        c.close();
        ui::say(&c.player, SECTION, "<click:suggest_command:/tc ><#22d3ee><u>/tc <message></u></#22d3ee></click> talks to your team.");
    });
    let leader = me == LEADER;
    m.button(52, ui::icon(if leader { "TNT" } else { "OAK_DOOR" }, if leader { "<#f87171><bold>Disband team" } else { "<#f87171><bold>Leave team" }, &ui::lines(&["", "<#f87171>▸ Click"])), move |c| {
        ui::click(&c.player);
        if leader {
            crate::claims::confirm(&c.player, "Disband the team?", |p| disband(p));
        } else {
            crate::claims::confirm(&c.player, "Leave the team?", |p| leave(p));
        }
    });
    ui::back_button(&mut m, "the server menu", crate::menu::open);
    ui::close_button(&mut m);
    m.open(p);
}

fn disband(p: &Player) {
    let uuid = p.get_unique_id();
    let Some(t) = team_of(&uuid) else { return };
    if t.role_of(&uuid) != Some(LEADER) {
        return ui::say(p, SECTION, "<#f87171>Only the leader can disband the team.");
    }
    tell_team(&t, &format!("<#f87171>The team {} was disbanded.", t.name));
    disband_now(&t.name.to_ascii_lowercase());
}

// ---- Commands ----

pub fn team_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    let Some(p) = sender.as_player() else {
        ui::tell(sender, SECTION, "Only players have teams.");
        return true;
    };
    let p = p.clone();
    if !conf::get().teams {
        ui::say(&p, SECTION, "<#f87171>Teams are turned off.");
        return true;
    }
    let sub = args.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    let a1 = args.get(1).copied();
    match sub.as_str() {
        "" | "menu" | "gui" => open(&p),
        "create" | "new" | "found" => match a1 {
            Some(n) => create(&p, n),
            None => ui::say(&p, SECTION, "Usage: <white>/team create <name>"),
        },
        "invite" | "add" => match a1 {
            Some(n) => invite(&p, n),
            None => ui::say(&p, SECTION, "Usage: <white>/team invite <player>"),
        },
        "accept" | "join" => accept(&p, a1),
        "deny" | "decline" => deny(&p, a1),
        "leave" | "quit" => leave(&p),
        "kick" | "remove" => match a1 {
            Some(n) => kick(&p, n),
            None => ui::say(&p, SECTION, "Usage: <white>/team kick <player>"),
        },
        "promote" => match a1 {
            Some(n) => set_role(&p, n, true),
            None => ui::say(&p, SECTION, "Usage: <white>/team promote <player>"),
        },
        "demote" => match a1 {
            Some(n) => set_role(&p, n, false),
            None => ui::say(&p, SECTION, "Usage: <white>/team demote <player>"),
        },
        "transfer" | "leader" => match a1 {
            Some(n) => transfer(&p, n),
            None => ui::say(&p, SECTION, "Usage: <white>/team transfer <player>"),
        },
        "disband" | "delete" => crate::claims::confirm(&p, "Disband the team?", |p| disband(p)),
        "sethome" => set_home(&p),
        "home" => go_home(&p),
        "chat" | "c" => team_chat(&p, &args[1..].join(" ")),
        "info" | "show" | "who" => info(&p, a1),
        "list" | "top" => list(&p),
        "ff" | "pvp" | "friendlyfire" => {
            let uuid = p.get_unique_id();
            match team_of(&uuid) {
                Some(t) if t.role_of(&uuid) == Some(LEADER) => {
                    let on = edit_team(&t.name.to_ascii_lowercase(), |t| {
                        t.friendly_fire = !t.friendly_fire;
                        t.friendly_fire
                    })
                    .unwrap_or(false);
                    if let Some(t) = team_of(&uuid) {
                        tell_team(&t, &format!("Friendly fire is now {}.", if on { "<#f87171>on" } else { "<#4ade80>off" }));
                    }
                }
                _ => ui::say(&p, SECTION, "<#f87171>Only the leader can do that."),
            }
        }
        _ => {
            ui::raw(&p, &[
                String::new(),
                format!(" {}  <#71717a>play together", ui::grad("<bold>TEAMS</bold>")),
                " <#22d3ee>/team</#22d3ee> <#71717a>your team's menu".into(),
                " <#22d3ee>/team create <name></#22d3ee> <#71717a>found a team".into(),
                " <#22d3ee>/team invite <player></#22d3ee> <#71717a>invite someone".into(),
                " <#22d3ee>/team accept • deny</#22d3ee> <#71717a>answer an invite".into(),
                " <#22d3ee>/team home • sethome</#22d3ee> <#71717a>your team's base".into(),
                " <#22d3ee>/tc <message></#22d3ee> <#71717a>team chat".into(),
                " <#22d3ee>/team info [team] • list</#22d3ee> <#71717a>teams".into(),
                " <#22d3ee>/team kick • promote • demote • transfer</#22d3ee>".into(),
                " <#22d3ee>/team leave • disband • ff</#22d3ee>".into(),
                String::new(),
            ].join("\n"));
        }
    }
    true
}

pub fn tc_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    if let Some(p) = sender.as_player() {
        if args.is_empty() {
            ui::say(p, SECTION, "Usage: <white>/tc <message>");
        } else {
            team_chat(&p.clone(), &args.join(" "));
        }
    }
    true
}

pub fn complete(sender: &CommandSender, args: &[&str]) -> Vec<String> {
    let subs = ["create", "invite", "accept", "deny", "leave", "kick", "promote", "demote", "transfer", "disband", "sethome", "home", "chat", "info", "list", "ff"];
    match args {
        [one] => subs.iter().filter(|s| s.starts_with(&one.to_ascii_lowercase())).map(|s| (*s).to_owned()).collect(),
        [sub, two] => {
            let two = two.to_ascii_lowercase();
            match *sub {
                "invite" => horizon_bukkit::get_online_players().into_iter().map(|p| p.get_name().to_owned()).filter(|n| n.to_ascii_lowercase().starts_with(&two)).collect(),
                "kick" | "promote" | "demote" | "transfer" => sender
                    .as_player()
                    .and_then(|p| team_of(&p.get_unique_id()))
                    .map(|t| t.members.into_iter().map(|m| m.name).filter(|n| n.to_ascii_lowercase().starts_with(&two)).collect())
                    .unwrap_or_default(),
                "info" | "accept" | "deny" => with(|st| st.db.teams.values().map(|t| t.name.clone()).filter(|n| n.to_ascii_lowercase().starts_with(&two)).collect()),
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

/// A player's name changed (renamed account): keep the member list right.
pub fn joined(p: &Player) {
    let uuid = p.get_unique_id();
    let name = p.get_name().to_owned();
    if let Some(t) = team_of(&uuid)
        && t.members.iter().any(|m| m.uuid == uuid && m.name != name)
    {
        edit_team(&t.name.to_ascii_lowercase(), |t| {
            for m in &mut t.members {
                if m.uuid == uuid {
                    m.name = name.clone();
                }
            }
        });
    }
}
