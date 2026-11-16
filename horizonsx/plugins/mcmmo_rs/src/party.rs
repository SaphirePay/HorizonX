//! Parties (`flatfile/parties.yml`, mcMMO's own format): create/join/invite/accept/quit/kick/disband/owner/lock/
//! password/rename, party levels and their unlocks, XP share (EQUAL among the members near), party chat and
//! `/ptp`. Lock order: SESSIONS, STORE, then PARTIES.

use crate::locale::get as l;
use crate::mmo::{self, Chat, Out, Session};
use horizon_bukkit::{CommandSender, Player};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};

#[derive(Clone)]
pub struct Party {
    pub name: String,
    pub leader: (String, String),
    pub password: Option<String>,
    pub locked: bool,
    pub level: i32,
    pub xp: f32,
    pub ally: String,
    pub xp_share: String,
    pub item_share: String,
    pub item_types: Vec<(String, bool)>,
    pub members: Vec<(String, String)>,
}

#[derive(Default)]
pub struct Parties {
    pub by_name: HashMap<String, Party>,
    pub member_of: HashMap<String, String>,
    dirty: bool,
}

impl Parties {
    pub fn of(&self, uuid: &str) -> Option<&Party> {
        self.member_of.get(uuid).and_then(|n| self.by_name.get(n))
    }
    fn of_mut(&mut self, uuid: &str) -> Option<&mut Party> {
        let n = self.member_of.get(uuid)?.clone();
        self.dirty = true;
        self.by_name.get_mut(&n)
    }
    fn reindex(&mut self) {
        self.member_of.clear();
        for (key, p) in &self.by_name {
            for (u, _) in &p.members {
                self.member_of.insert(u.clone(), key.clone());
            }
        }
        self.dirty = true;
    }
}

pub static PARTIES: LazyLock<Mutex<Parties>> = LazyLock::new(|| Mutex::new(Parties::default()));

pub fn parties() -> MutexGuard<'static, Parties> {
    PARTIES.lock().unwrap_or_else(|e| e.into_inner())
}

static FILE: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

pub fn load(folder: &std::path::Path) {
    let path = folder.join("flatfile").join("parties.yml");
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.clone());
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let doc: serde_yaml::Value = horizon_kit::yaml::parse(&text).unwrap_or(serde_yaml::Value::Null);
    let mut ps = Parties::default();
    if let Some(m) = doc.as_mapping() {
        for (k, v) in m {
            let Some(name) = k.as_str() else { continue };
            let s = |key: &str| v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_owned();
            let split = |e: &str| {
                let mut it = e.splitn(2, '|');
                (it.next().unwrap_or_default().to_ascii_lowercase(), it.next().unwrap_or_default().to_owned())
            };
            let leader = split(&s("Leader"));
            let members: Vec<(String, String)> = v.get("Members").and_then(|m| m.as_sequence()).map(|seq| seq.iter().filter_map(|e| e.as_str()).map(split).collect()).unwrap_or_default();
            let item_types = v.get("ItemShareType").and_then(|m| m.as_mapping()).map(|m| m.iter().filter_map(|(k, v)| Some((k.as_str()?.to_owned(), v.as_bool().unwrap_or(true)))).collect()).unwrap_or_default();
            let pw = s("Password");
            ps.by_name.insert(
                name.to_ascii_lowercase(),
                Party {
                    name: name.to_owned(),
                    leader,
                    password: if pw.is_empty() { None } else { Some(pw) },
                    locked: v.get("Locked").and_then(|x| x.as_bool()).unwrap_or(false),
                    level: v.get("Level").and_then(|x| x.as_i64()).unwrap_or(0) as i32,
                    xp: v.get("Xp").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
                    ally: s("Ally"),
                    xp_share: v.get("ExpShareMode").and_then(|x| x.as_str()).unwrap_or("NONE").to_owned(),
                    item_share: v.get("ItemShareMode").and_then(|x| x.as_str()).unwrap_or("NONE").to_owned(),
                    item_types,
                    members,
                },
            );
        }
    }
    ps.reindex();
    ps.dirty = false;
    *parties() = ps;
}

/// parties.yml written (from the saver thread or at stop): the text made under the lock, written after.
pub fn save() {
    let Some(path) = FILE.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
    let text = {
        let mut ps = parties();
        if !ps.dirty {
            return;
        }
        ps.dirty = false;
        let mut root = serde_yaml::Mapping::new();
        for p in ps.by_name.values() {
            let mut m = serde_yaml::Mapping::new();
            let y = |s: &str| serde_yaml::Value::String(s.to_owned());
            m.insert(y("Leader"), y(&format!("{}|{}", p.leader.0, p.leader.1)));
            m.insert(y("Password"), p.password.as_deref().map_or(serde_yaml::Value::Null, y));
            m.insert(y("Locked"), serde_yaml::Value::Bool(p.locked));
            m.insert(y("Level"), serde_yaml::Value::Number(p.level.into()));
            m.insert(y("Xp"), serde_yaml::Value::Number((p.xp as i64).into()));
            m.insert(y("Ally"), y(&p.ally));
            m.insert(y("ExpShareMode"), y(&p.xp_share));
            m.insert(y("ItemShareMode"), y(&p.item_share));
            let mut types = serde_yaml::Mapping::new();
            for t in ["LOOT", "MINING", "HERBALISM", "WOODCUTTING", "MISC"] {
                let on = p.item_types.iter().find(|(k, _)| k == t).map(|(_, v)| *v).unwrap_or(true);
                types.insert(y(t), serde_yaml::Value::Bool(on));
            }
            m.insert(y("ItemShareType"), serde_yaml::Value::Mapping(types));
            m.insert(y("Members"), serde_yaml::Value::Sequence(p.members.iter().map(|(u, n)| y(&format!("{u}|{n}"))).collect()));
            root.insert(y(&p.name), serde_yaml::Value::Mapping(m));
        }
        serde_yaml::to_string(&serde_yaml::Value::Mapping(root)).unwrap_or_default()
    };
    let _ = std::fs::write(path, text);
}

fn unlocked(p: &Party, feature: usize) -> bool {
    p.level >= crate::settings::get().party_unlock[feature]
}

/// Who an EQUAL share goes to: the gainer and the members near (None: no share, the gainer keeps it all).
pub fn share_targets(sessions: &HashMap<String, Session>, uuid: &str) -> Option<Vec<String>> {
    let s = crate::settings::get();
    if !s.party_enabled {
        return None;
    }
    let ps = parties();
    let p = ps.of(uuid)?;
    if !p.xp_share.eq_ignore_ascii_case("EQUAL") || !unlocked(p, 4) {
        return None;
    }
    let me = sessions.get(uuid)?.pos.clone()?;
    let range2 = s.party_range * s.party_range;
    let mut out = vec![uuid.to_owned()];
    for (u, _) in &p.members {
        if u == uuid {
            continue;
        }
        if let Some(other) = sessions.get(u).and_then(|o| o.pos.clone())
            && other.0 == me.0
            && (other.1 - me.1).powi(2) + (other.2 - me.2).powi(2) + (other.3 - me.3).powi(2) <= range2
        {
            out.push(u.clone());
        }
    }
    if out.len() < 2 { None } else { Some(out) }
}

/// The party's own XP from a member's gain (Party.applyXpGain).
pub fn party_xp(sessions: &HashMap<String, Session>, out: &mut Out, uuid: &str, xp: f32) {
    let s = crate::settings::get();
    if !s.party_enabled {
        return;
    }
    let mut ps = parties();
    let Some(key) = ps.member_of.get(uuid).cloned() else { return };
    let online: Vec<String> = ps.by_name.get(&key).map(|p| p.members.iter().filter(|(u, _)| sessions.contains_key(u)).map(|(u, _)| u.clone()).collect()).unwrap_or_default();
    if s.party_near_needed && share_targets_nolock(sessions, &ps, uuid).is_none() {
        return;
    }
    let Some(p) = ps.by_name.get_mut(&key) else { return };
    if s.party_level_cap > 0 && p.level >= s.party_level_cap {
        return;
    }
    p.xp += xp;
    let mut gained = 0;
    loop {
        let need = s.xp_to_next(p.level) * (online.len() as i64 + s.party_curve as i64);
        if (p.xp as f64) < need as f64 {
            break;
        }
        if s.party_level_cap > 0 && p.level >= s.party_level_cap {
            p.xp = 0.0;
            break;
        }
        p.xp -= need as f32;
        p.level += 1;
        gained += 1;
    }
    ps.dirty = true;
    if gained > 0 {
        let p = &ps.by_name[&key];
        let text = l("Party.LevelUp", &[&gained.to_string(), &p.level.to_string()]);
        for u in &online {
            if let Some(sess) = sessions.get(u) {
                out.notify(sess, "PartyMessage", text.clone());
            }
        }
    }
}

fn share_targets_nolock(sessions: &HashMap<String, Session>, ps: &Parties, uuid: &str) -> Option<()> {
    let s = crate::settings::get();
    let p = ps.of(uuid)?;
    let me = sessions.get(uuid)?.pos.clone()?;
    let range2 = s.party_range * s.party_range;
    p.members.iter().any(|(u, _)| u != uuid && sessions.get(u).and_then(|o| o.pos.clone()).is_some_and(|o| o.0 == me.0 && (o.1 - me.1).powi(2) + (o.2 - me.2).powi(2) + (o.3 - me.3).powi(2) <= range2)).then_some(())
}

pub fn same_party(a: &str, b: &str) -> bool {
    let ps = parties();
    matches!((ps.member_of.get(a), ps.member_of.get(b)), (Some(x), Some(y)) if x == y)
}

pub fn party_name(uuid: &str) -> Option<String> {
    parties().of(uuid).map(|p| p.name.clone())
}

fn online(uuid: &str) -> Option<Player> {
    horizon_bukkit::get_online_players().into_iter().find(|p| p.get_unique_id().eq_ignore_ascii_case(uuid))
}

fn tell(sender: &CommandSender, text: &str) {
    for line in text.split('\n') {
        sender.send_message(line);
    }
}

fn members_say(members: &[(String, String)], text: &str) {
    for (u, _) in members {
        if let Some(p) = online(u) {
            p.send_message(text);
        }
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 24 && !name.contains('.') && !name.contains('|')
}

/// `/party ...`
pub fn command(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    let s = crate::settings::get();
    let Some(player) = sender.as_player() else {
        tell(sender, &l("Commands.NoConsole", &[]));
        return true;
    };
    if !s.party_enabled {
        tell(sender, &l("Party.Forbidden", &[]));
        return true;
    }
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let name = player.get_name().to_owned();
    let sub = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    match sub.as_str() {
        "" | "info" => info(sender, &uuid),
        "help" | "?" => {
            for i in 0..=10 {
                let key = format!("Party.Help.{i}");
                if crate::locale::has(&key) {
                    tell(sender, &l(&key, &[&format!("/{label} join"), &format!("/{label} quit")]));
                }
            }
        }
        "create" => {
            let Some(pname) = args.get(1) else {
                tell(sender, &l("Commands.Usage.2", &[label, "create", "<name>"]));
                return true;
            };
            if !valid_name(pname) {
                tell(sender, &l("Party.InvalidName", &[]));
                return true;
            }
            let mut ps = parties();
            if ps.by_name.contains_key(&pname.to_ascii_lowercase()) {
                tell(sender, &l("Commands.Party.AlreadyExists", &[pname]));
                return true;
            }
            leave_locked(&mut ps, &uuid, &name);
            ps.by_name.insert(
                pname.to_ascii_lowercase(),
                Party { name: (*pname).to_owned(), leader: (uuid.clone(), name.clone()), password: args.get(2).map(|p| (*p).to_owned()), locked: false, level: 0, xp: 0.0, ally: String::new(), xp_share: "NONE".into(), item_share: "NONE".into(), item_types: Vec::new(), members: vec![(uuid.clone(), name.clone())] },
            );
            ps.reindex();
            tell(sender, &l("Commands.Party.Create", &[pname]));
        }
        "join" => {
            let Some(target) = args.get(1) else {
                tell(sender, &l("Commands.Usage.2", &[label, "join", "<player>"]));
                return true;
            };
            let Some(tp) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(target)) else {
                tell(sender, &l("Party.NotOnline", &[target]));
                return true;
            };
            let tu = tp.get_unique_id().to_ascii_lowercase();
            if tu == uuid {
                tell(sender, &l("Party.Join.Self", &[]));
                return true;
            }
            let mut ps = parties();
            let Some(key) = ps.member_of.get(&tu).cloned() else {
                tell(sender, &l("Party.PlayerNotInParty", &[target]));
                return true;
            };
            let p = &ps.by_name[&key];
            if let Some(pw) = &p.password {
                match args.get(2) {
                    None => {
                        tell(sender, &l("Party.Password.None", &[]));
                        return true;
                    }
                    Some(given) if given != pw => {
                        tell(sender, &l("Party.Password.Incorrect", &[]));
                        return true;
                    }
                    _ => {}
                }
            }
            if p.locked {
                tell(sender, &l("Party.Locked", &[]));
                return true;
            }
            join_locked(&mut ps, &key, &uuid, &name, sender);
        }
        "accept" => {
            let invite = {
                let mut ss = mmo::sessions();
                ss.get_mut(&uuid).and_then(|s| s.invite.take())
            };
            let Some((pname, _from)) = invite else {
                tell(sender, &l("Commands.Party.None", &[]));
                return true;
            };
            let mut ps = parties();
            let key = pname.to_ascii_lowercase();
            if !ps.by_name.contains_key(&key) {
                tell(sender, &l("Party.Specify", &[]));
                return true;
            }
            join_locked(&mut ps, &key, &uuid, &name, sender);
            tell(sender, &l("Commands.Party.Invite.Accepted", &[&pname]));
        }
        "invite" => {
            let Some(target) = args.get(1) else {
                tell(sender, &l("Commands.Usage.2", &[label, "invite", "<player>"]));
                return true;
            };
            let Some(tp) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(target)) else {
                tell(sender, &l("Party.NotOnline", &[target]));
                return true;
            };
            let tu = tp.get_unique_id().to_ascii_lowercase();
            if tu == uuid {
                tell(sender, &l("Party.Invite.Self", &[]));
                return true;
            }
            let (pname, ok) = {
                let ps = parties();
                let Some(p) = ps.of(&uuid) else {
                    tell(sender, &l("Commands.Party.None", &[]));
                    return true;
                };
                if p.members.iter().any(|(u, _)| *u == tu) {
                    tell(sender, &l("Party.Player.InSameParty", &[tp.get_name()]));
                    return true;
                }
                if p.locked && p.leader.0 != uuid {
                    tell(sender, &l("Party.Locked", &[]));
                    return true;
                }
                if s.party_max > 0 && p.members.len() as i32 >= s.party_max {
                    tell(sender, &l("Commands.Party.PartyFull.Invite", &[tp.get_name(), &p.name, &p.members.len().to_string()]));
                    return true;
                }
                (p.name.clone(), true)
            };
            if ok {
                if let Some(sess) = mmo::sessions().get_mut(&tu) {
                    sess.invite = Some((pname.clone(), name.clone()));
                }
                tp.send_message(&l("Commands.Party.Invite.0", &[&pname, &name]));
                tp.send_message(&l("Commands.Party.Invite.1", &[]));
                tell(sender, &l("Commands.Invite.Success", &[]));
            }
        }
        "quit" | "leave" | "q" => {
            let mut ps = parties();
            if ps.of(&uuid).is_none() {
                tell(sender, &l("Commands.Party.None", &[]));
                return true;
            }
            leave_locked(&mut ps, &uuid, &name);
            tell(sender, &l("Commands.Party.Leave", &[]));
        }
        "kick" => {
            let Some(target) = args.get(1) else {
                tell(sender, &l("Commands.Usage.2", &[label, "kick", "<player>"]));
                return true;
            };
            let mut ps = parties();
            let Some(p) = ps.of(&uuid) else {
                tell(sender, &l("Commands.Party.None", &[]));
                return true;
            };
            if p.leader.0 != uuid {
                tell(sender, &l("Party.NotOwner", &[]));
                return true;
            }
            let Some((tu, tn)) = p.members.iter().find(|(_, n)| n.eq_ignore_ascii_case(target)).cloned() else {
                tell(sender, &l("Party.NotInYourParty", &[target]));
                return true;
            };
            let pname = p.name.clone();
            leave_locked(&mut ps, &tu, &tn);
            if let Some(tp) = online(&tu) {
                tp.send_message(&l("Commands.Party.Kick", &[&pname]));
            }
        }
        "disband" => {
            let mut ps = parties();
            let Some(p) = ps.of(&uuid).cloned() else {
                tell(sender, &l("Commands.Party.None", &[]));
                return true;
            };
            if p.leader.0 != uuid {
                tell(sender, &l("Party.NotOwner", &[]));
                return true;
            }
            ps.by_name.remove(&p.name.to_ascii_lowercase());
            ps.reindex();
            members_say(&p.members, &l("Party.Disband", &[]));
        }
        "owner" => {
            let Some(target) = args.get(1) else {
                tell(sender, &l("Commands.Usage.2", &[label, "owner", "<player>"]));
                return true;
            };
            let mut ps = parties();
            let Some(p) = ps.of_mut(&uuid) else {
                tell(sender, &l("Commands.Party.None", &[]));
                return true;
            };
            if p.leader.0 != uuid {
                tell(sender, &l("Party.NotOwner", &[]));
                return true;
            }
            let Some(m) = p.members.iter().find(|(_, n)| n.eq_ignore_ascii_case(target)).cloned() else {
                tell(sender, &l("Party.NotInYourParty", &[target]));
                return true;
            };
            p.leader = m.clone();
            let members = p.members.clone();
            members_say(&members, &l("Party.Owner.New", &[&m.1]));
        }
        "lock" | "unlock" => {
            let lock = sub == "lock" && !args.get(1).is_some_and(|a| a.eq_ignore_ascii_case("off") || a.eq_ignore_ascii_case("false"));
            let mut ps = parties();
            let Some(p) = ps.of_mut(&uuid) else {
                tell(sender, &l("Commands.Party.None", &[]));
                return true;
            };
            if p.leader.0 != uuid {
                tell(sender, &l("Party.NotOwner", &[]));
                return true;
            }
            if p.locked == lock {
                tell(sender, &l(if lock { "Party.IsLocked" } else { "Party.IsntLocked" }, &[]));
                return true;
            }
            p.locked = lock;
            tell(sender, &l(if lock { "Party.Locked" } else { "Party.Unlocked" }, &[]));
        }
        "password" => {
            let mut ps = parties();
            let Some(p) = ps.of_mut(&uuid) else {
                tell(sender, &l("Commands.Party.None", &[]));
                return true;
            };
            if p.leader.0 != uuid {
                tell(sender, &l("Party.NotOwner", &[]));
                return true;
            }
            match args.get(1) {
                Some(pw) if !pw.eq_ignore_ascii_case("clear") && !pw.eq_ignore_ascii_case("reset") => {
                    p.password = Some((*pw).to_owned());
                    tell(sender, &l("Party.Password.Set", &[pw]));
                }
                _ => {
                    p.password = None;
                    tell(sender, &l("Party.Password.Removed", &[]));
                }
            }
        }
        "rename" => {
            let Some(new) = args.get(1) else {
                tell(sender, &l("Commands.Usage.2", &[label, "rename", "<name>"]));
                return true;
            };
            if !valid_name(new) {
                tell(sender, &l("Party.InvalidName", &[]));
                return true;
            }
            let mut ps = parties();
            let Some(mut p) = ps.of(&uuid).cloned() else {
                tell(sender, &l("Commands.Party.None", &[]));
                return true;
            };
            if p.leader.0 != uuid {
                tell(sender, &l("Party.NotOwner", &[]));
                return true;
            }
            if p.name.eq_ignore_ascii_case(new) {
                tell(sender, &l("Party.Rename.Same", &[]));
                return true;
            }
            if ps.by_name.contains_key(&new.to_ascii_lowercase()) {
                tell(sender, &l("Commands.Party.AlreadyExists", &[new]));
                return true;
            }
            ps.by_name.remove(&p.name.to_ascii_lowercase());
            p.name = (*new).to_owned();
            let members = p.members.clone();
            ps.by_name.insert(new.to_ascii_lowercase(), p);
            ps.reindex();
            members_say(&members, &l("Party.InformedOnNameChange", &[&name, new]));
        }
        "chat" | "c" => {
            drop(s);
            return chat_command(sender, &[]);
        }
        "xpshare" | "shareexp" | "sharexp" => {
            let mut ps = parties();
            let Some(p) = ps.of_mut(&uuid) else {
                tell(sender, &l("Commands.Party.None", &[]));
                return true;
            };
            if p.leader.0 != uuid {
                tell(sender, &l("Party.NotOwner", &[]));
                return true;
            }
            if !unlocked(p, 4) {
                tell(sender, &l("Party.Feature.Disabled.5", &[]));
                return true;
            }
            let mode = match args.get(1).map(|a| a.to_ascii_lowercase()).as_deref() {
                Some("none" | "off" | "false") => "NONE",
                Some("equal" | "even" | "on" | "true") => "EQUAL",
                _ => {
                    tell(sender, &l("Commands.Usage.2", &[label, "xpshare", "<NONE | EQUAL>"]));
                    return true;
                }
            };
            p.xp_share = mode.to_owned();
            let members = p.members.clone();
            members_say(&members, &format!("{} {}", l("Commands.Party.ShareMode", &[]), l(&format!("Party.ShareMode.{}", if mode == "EQUAL" { "Equal" } else { "None" }), &[])));
        }
        "itemshare" | "shareitem" | "shareitems" => {
            let mut ps = parties();
            let Some(p) = ps.of_mut(&uuid) else {
                tell(sender, &l("Commands.Party.None", &[]));
                return true;
            };
            if !unlocked(p, 3) {
                tell(sender, &l("Party.Feature.Disabled.4", &[]));
                return true;
            }
            if let Some(m) = args.get(1) {
                p.item_share = m.to_ascii_uppercase();
            }
            tell(sender, &l("Commands.Party.ItemShare", &[&p.item_share]));
        }
        "teleport" | "tp" => {
            drop(s);
            return ptp(sender, &args[1..]);
        }
        "alliance" | "ally" => tell(sender, &l("Commands.Party.Alliance.None", &[])),
        _ => {
            // `/party <name> [password]` makes or joins it, as in mcMMO.
            let pname = args[0];
            let mut ps = parties();
            let key = pname.to_ascii_lowercase();
            if ps.by_name.contains_key(&key) {
                let p = &ps.by_name[&key];
                if p.password.as_deref().is_some_and(|pw| args.get(1) != Some(&pw)) {
                    tell(sender, &l("Party.Password.Incorrect", &[]));
                    return true;
                }
                if p.locked {
                    tell(sender, &l("Party.Locked", &[]));
                    return true;
                }
                join_locked(&mut ps, &key, &uuid, &name, sender);
            } else if valid_name(pname) {
                leave_locked(&mut ps, &uuid, &name);
                ps.by_name.insert(key, Party { name: pname.to_owned(), leader: (uuid.clone(), name.clone()), password: args.get(1).map(|p| (*p).to_owned()), locked: false, level: 0, xp: 0.0, ally: String::new(), xp_share: "NONE".into(), item_share: "NONE".into(), item_types: Vec::new(), members: vec![(uuid.clone(), name.clone())] });
                ps.reindex();
                tell(sender, &l("Commands.Party.Create", &[pname]));
            } else {
                tell(sender, &l("Party.InvalidName", &[]));
            }
        }
    }
    true
}

fn join_locked(ps: &mut Parties, key: &str, uuid: &str, name: &str, sender: &CommandSender) {
    let s = crate::settings::get();
    let Some(p) = ps.by_name.get(key) else { return };
    if s.party_max > 0 && p.members.len() as i32 >= s.party_max {
        tell(sender, &l("Commands.Party.PartyFull.InviteAccept", &[&p.name, &p.members.len().to_string()]));
        return;
    }
    leave_locked(ps, uuid, name);
    let Some(p) = ps.by_name.get_mut(key) else { return };
    let members = p.members.clone();
    p.members.push((uuid.to_owned(), name.to_owned()));
    let pname = p.name.clone();
    ps.reindex();
    members_say(&members, &l("Party.InformedOnJoin", &[name]));
    tell(sender, &l("Commands.Party.Join", &[&pname]));
}

fn leave_locked(ps: &mut Parties, uuid: &str, name: &str) {
    let Some(key) = ps.member_of.get(uuid).cloned() else { return };
    let Some(p) = ps.by_name.get_mut(&key) else { return };
    p.members.retain(|(u, _)| u != uuid);
    if p.members.is_empty() {
        ps.by_name.remove(&key);
    } else {
        if p.leader.0 == uuid {
            p.leader = p.members[0].clone();
        }
        let members = p.members.clone();
        members_say(&members, &l("Party.InformedOnQuit", &[name]));
    }
    ps.reindex();
}

fn info(sender: &CommandSender, uuid: &str) {
    let s = crate::settings::get();
    let ps = parties();
    let Some(p) = ps.of(uuid) else {
        tell(sender, &l("Commands.Party.None", &[]));
        return;
    };
    tell(sender, &l("Commands.Party.Header", &[]));
    let status = l(if p.locked { "Party.Status.Locked" } else { "Party.Status.Unlocked" }, &[]);
    tell(sender, &l("Commands.Party.Status", &[&p.name, &status, &p.level.to_string()]));
    let mut features = Vec::new();
    for (i, f) in ["Chat", "Teleport", "Alliance", "ItemShare", "XpShare"].iter().enumerate() {
        if p.level >= s.party_unlock[i] {
            features.push(l(&format!("Party.Feature.{f}"), &[]));
        } else {
            features.push(l(&format!("Party.Feature.Locked.{f}"), &[&s.party_unlock[i].to_string()]));
        }
    }
    tell(sender, &l("Commands.Party.UnlockedFeatures", &[&features.join(", ")]));
    tell(sender, &format!("{} {}", l("Commands.Party.ShareMode", &[]), l("Commands.Party.ExpShare", &[&p.xp_share])));
    tell(sender, &l("Commands.Party.Members.Header", &[]));
    let online_names: Vec<String> = horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).collect();
    let list: Vec<String> = p.members.iter().map(|(u, n)| {
        let on = online_names.iter().any(|o| o.eq_ignore_ascii_case(n));
        let lead = if *u == p.leader.0 { "§6" } else { "" };
        format!("{}{}{}", if on { "§a" } else { "§7" }, lead, n)
    }).collect();
    tell(sender, &list.join("§f, "));
}

/// `/p [msg]` / `/partychat`: toggles party chat, or sends one message.
pub fn chat_command(sender: &CommandSender, args: &[&str]) -> bool {
    let Some(player) = sender.as_player() else {
        tell(sender, &l("Commands.NoConsole", &[]));
        return true;
    };
    let uuid = player.get_unique_id().to_ascii_lowercase();
    {
        let ps = parties();
        let Some(p) = ps.of(&uuid) else {
            tell(sender, &l("Commands.Party.None", &[]));
            return true;
        };
        if !unlocked(p, 0) {
            tell(sender, &l("Party.Feature.Disabled.1", &[]));
            return true;
        }
    }
    if args.is_empty() {
        let mut ss = mmo::sessions();
        if let Some(sess) = ss.get_mut(&uuid) {
            if sess.chat == Chat::Party {
                sess.chat = Chat::None;
                tell(sender, &l("Chat.Channel.Off", &[]));
            } else {
                sess.chat = Chat::Party;
                tell(sender, &l("Chat.Channel.On", &["Party"]));
            }
        }
        return true;
    }
    send_party_chat(player, &args.join(" "));
    true
}

/// One message to the sender's party (and spies, and the console).
pub fn send_party_chat(player: &Player, message: &str) {
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let (members, leader, pname) = {
        let ps = parties();
        let Some(p) = ps.of(&uuid) else { return };
        (p.members.clone(), p.leader.0 == uuid, p.name.clone())
    };
    let s = crate::settings::get();
    let who = if s.chat_display_names { player.get_display_name().to_owned() } else { player.get_name().to_owned() };
    let text = l(if leader { "Chat.Style.Party.Leader" } else { "Chat.Style.Party" }, &[&who, message]);
    let spy_text = l("Chat.Spy.Party", &[&who, message, &pname]);
    let spies: Vec<String> = mmo::sessions().values().filter(|s| s.chat_spy).map(|s| s.uuid.clone()).collect();
    for p in horizon_bukkit::get_online_players() {
        let u = p.get_unique_id().to_ascii_lowercase();
        if members.iter().any(|(m, _)| *m == u) {
            p.send_message(&text);
        } else if spies.contains(&u) {
            p.send_message(&spy_text);
        }
    }
    if s.chat_console {
        crate::log(&horizon_kit::text::strip(&text));
    }
}

/// `/a [msg]`: admin chat.
pub fn admin_command(sender: &CommandSender, args: &[&str]) -> bool {
    if args.is_empty() {
        let Some(player) = sender.as_player() else {
            tell(sender, &l("Commands.NoConsole", &[]));
            return true;
        };
        let uuid = player.get_unique_id().to_ascii_lowercase();
        let mut ss = mmo::sessions();
        if let Some(sess) = ss.get_mut(&uuid) {
            if sess.chat == Chat::Admin {
                sess.chat = Chat::None;
                tell(sender, &l("Chat.Channel.Off", &[]));
            } else {
                sess.chat = Chat::Admin;
                tell(sender, &l("Chat.Channel.On", &["Admin"]));
            }
        }
        return true;
    }
    let who = match sender.as_player() {
        Some(p) => p.get_display_name().to_owned(),
        None => l("Chat.Identity.Console", &[]),
    };
    send_admin_chat(&who, &args.join(" "));
    true
}

pub fn send_admin_chat(who: &str, message: &str) {
    let text = l("Chat.Style.Admin", &[who, message]);
    let admins: Vec<String> = mmo::sessions().values().filter(|s| s.has(mmo::perm::ADMIN_CHAT)).map(|s| s.uuid.clone()).collect();
    for p in horizon_bukkit::get_online_players() {
        if admins.contains(&p.get_unique_id().to_ascii_lowercase()) {
            p.send_message(&text);
        }
    }
    crate::log(&horizon_kit::text::strip(&text));
}

/// `/ptp <player> | accept | toggle | acceptany`
pub fn ptp(sender: &CommandSender, args: &[&str]) -> bool {
    let Some(player) = sender.as_player() else {
        tell(sender, &l("Commands.NoConsole", &[]));
        return true;
    };
    let uuid = player.get_unique_id().to_ascii_lowercase();
    {
        let ps = parties();
        let Some(p) = ps.of(&uuid) else {
            tell(sender, &l("Commands.Party.None", &[]));
            return true;
        };
        if !unlocked(p, 1) {
            tell(sender, &l("Party.Feature.Disabled.2", &[]));
            return true;
        }
    }
    let sub = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    match sub.as_str() {
        "" => tell(sender, &l("Commands.Usage.1", &["ptp", "<player>"])),
        "toggle" => {
            let mut ss = mmo::sessions();
            if let Some(s) = ss.get_mut(&uuid) {
                s.ptp_enabled = !s.ptp_enabled;
                tell(sender, &l(if s.ptp_enabled { "Commands.ptp.Enabled" } else { "Commands.ptp.Disabled" }, &[]));
            }
        }
        "acceptany" | "acceptall" => {
            let mut ss = mmo::sessions();
            if let Some(s) = ss.get_mut(&uuid) {
                s.ptp_any = !s.ptp_any;
                tell(sender, &l(if s.ptp_any { "Commands.ptp.AcceptAny.Enabled" } else { "Commands.ptp.AcceptAny.Disabled" }, &[]));
            }
        }
        "accept" => {
            let req = {
                let mut ss = mmo::sessions();
                ss.get_mut(&uuid).and_then(|s| s.ptp_request.take())
            };
            let Some((from, at)) = req.filter(|(_, at)| crate::profile::now_secs() - at <= 120) else {
                tell(sender, &l("Commands.ptp.NoRequests", &[]));
                return true;
            };
            let _ = at;
            match online(&from) {
                Some(fp) => {
                    fp.teleport(&player.get_location());
                    fp.send_message(&l("Party.Teleport.Player", &[player.get_name()]));
                    player.send_message(&l("Party.Teleport.Target", &[fp.get_name()]));
                }
                None => tell(sender, &l("Commands.ptp.NoRequests", &[])),
            }
        }
        _ => {
            let Some(target) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(args[0])) else {
                tell(sender, &l("Party.NotOnline", &[args[0]]));
                return true;
            };
            let tu = target.get_unique_id().to_ascii_lowercase();
            if tu == uuid {
                tell(sender, &l("Party.Teleport.Self", &[]));
                return true;
            }
            if !same_party(&uuid, &tu) {
                tell(sender, &l("Party.NotInYourParty", &[target.get_name()]));
                return true;
            }
            let (enabled, any, hurt) = {
                let mut ss = mmo::sessions();
                let hurt = ss.get(&uuid).map(|s| s.recently_hurt).unwrap_or(0);
                match ss.get_mut(&tu) {
                    Some(t) => {
                        let r = (t.ptp_enabled, t.ptp_any, hurt);
                        if t.ptp_enabled && !t.ptp_any {
                            t.ptp_request = Some((uuid.clone(), crate::profile::now_secs()));
                        }
                        r
                    }
                    None => (false, false, hurt),
                }
            };
            if !enabled {
                tell(sender, &l("Party.Teleport.Disabled", &[target.get_name()]));
                return true;
            }
            if crate::profile::now_ms() - hurt < 60_000 {
                tell(sender, &l("Party.Teleport.Hurt", &["60"]));
                return true;
            }
            if any {
                player.teleport(&target.get_location());
                player.send_message(&l("Party.Teleport.Player", &[target.get_name()]));
                target.send_message(&l("Party.Teleport.Target", &[player.get_name()]));
            } else {
                target.send_message(&l("Commands.ptp.Request1", &[player.get_name()]));
                target.send_message(&l("Commands.ptp.Request2", &["120"]));
                tell(sender, &l("Commands.Invite.Success", &[]));
            }
        }
    }
    true
}

/// A member's name kept current at join.
pub fn rename_member(uuid: &str, name: &str) {
    let mut ps = parties();
    let Some(key) = ps.member_of.get(uuid).cloned() else { return };
    if let Some(p) = ps.by_name.get_mut(&key) {
        for m in p.members.iter_mut() {
            if m.0 == uuid && m.1 != name {
                m.1 = name.to_owned();
            }
        }
        if p.leader.0 == uuid {
            p.leader.1 = name.to_owned();
        }
    }
    ps.dirty = true;
}
