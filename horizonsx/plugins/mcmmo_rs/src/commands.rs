//! mcMMO's commands, with its messages (the locale) and permissions.

use crate::locale::get as l;
use crate::mmo::{self, Out, Reason};
use crate::profile::{self, Profile};
use crate::settings;
use crate::skills::{ABILITIES, Skill};
use horizon_bukkit::{CommandSender, Player, Plugin};

fn tell(sender: &CommandSender, text: &str) {
    for line in text.split('\n') {
        sender.send_message(line);
    }
}

fn no_perm(sender: &CommandSender) {
    sender.send_message("§cYou don't have permission to do that.");
}

fn uuid_of(p: &Player) -> String {
    p.get_unique_id().to_ascii_lowercase()
}

fn online(name: &str) -> Option<Player> {
    horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name))
}

/// A profile by name (online or not), copied out.
fn profile_named(name: &str) -> Option<Profile> {
    let st = profile::store();
    st.by_name(name).cloned()
}

const GATHERING: [Skill; 5] = [Skill::Excavation, Skill::Fishing, Skill::Herbalism, Skill::Mining, Skill::Woodcutting];
const COMBAT: [Skill; 9] = [Skill::Axes, Skill::Archery, Skill::Crossbows, Skill::Maces, Skill::Spears, Skill::Swords, Skill::Taming, Skill::Tridents, Skill::Unarmed];
const MISC: [Skill; 5] = [Skill::Acrobatics, Skill::Alchemy, Skill::Repair, Skill::Salvage, Skill::Smelting];

fn skill_line(p: &Profile, skill: Skill) -> String {
    let s = settings::get();
    let label = format!("§e{} ", l(&format!("{}.Listener", skill.name()), &[]));
    if skill.is_child() {
        return l("Skills.ChildStats", &[&label, &p.level(skill).to_string()]);
    }
    let level = p.level[skill.idx()];
    let need = mmo::xp_to_level(&s, p, skill);
    if level >= s.cap(skill) {
        return l("Skills.Stats", &[&label, &level.to_string(), &(p.xp[skill.idx()] as i64).to_string(), &l("Skills.MaxXP", &[])]);
    }
    l("Skills.Stats", &[&label, &level.to_string(), &(p.xp[skill.idx()] as i64).to_string(), &need.to_string()])
}

fn print_stats(sender: &CommandSender, p: &Profile, perms: Option<u64>) {
    let has = |s: Skill| perms.is_none_or(|bits| bits & (1 << (mmo::perm::SKILL0 + s.idx())) != 0);
    for (header, skills) in [("Stats.Header.Gathering", &GATHERING[..]), ("Stats.Header.Combat", &COMBAT[..]), ("Stats.Header.Misc", &MISC[..])] {
        let lines: Vec<String> = skills.iter().filter(|s| has(**s)).map(|s| skill_line(p, *s)).collect();
        if lines.is_empty() {
            continue;
        }
        tell(sender, &l(header, &[]));
        for line in lines {
            tell(sender, &line);
        }
    }
    let s = settings::get();
    if s.power_cap > 0 {
        tell(sender, &l("Commands.PowerLevel.Capped", &[&p.power_level().to_string(), &s.power_cap.to_string()]));
    } else {
        tell(sender, &l("Commands.PowerLevel", &[&p.power_level().to_string()]));
    }
}

fn perms_of(uuid: &str) -> Option<u64> {
    mmo::sessions().get(uuid).map(|s| s.perms)
}

pub fn mcstats(sender: &CommandSender, _label: &str, _args: &[&str]) -> bool {
    let Some(player) = sender.as_player() else {
        tell(sender, &l("Commands.NoConsole", &[]));
        return true;
    };
    let uuid = uuid_of(player);
    let Some(p) = profile::store().get(&uuid).cloned() else {
        tell(sender, &l("Profile.PendingLoad", &[]));
        return true;
    };
    if crate::scoreboard::enabled() {
        crate::scoreboard::stats(player, &p, &l("Scoreboard.Header.PlayerStats", &[]));
        if !crate::scoreboard::prints("Stats") {
            return true;
        }
    }
    tell(sender, &l("Stats.Own.Stats", &[]));
    tell(sender, &l("mcMMO.NoSkillNote", &[]));
    print_stats(sender, &p, perms_of(&uuid));
    true
}

pub fn inspect(sender: &CommandSender, _label: &str, args: &[&str]) -> bool {
    let Some(name) = args.first() else {
        tell(sender, &l("Commands.Usage.1", &["inspect", &format!("<{}>", l("Commands.Usage.Player", &[]))]));
        return true;
    };
    let on = online(name);
    if on.is_none() && !sender.has_permission("mcmmo.commands.inspect.offline") {
        tell(sender, &l("Inspect.Offline", &[]));
        return true;
    }
    let Some(p) = profile_named(name) else {
        tell(sender, &l("Commands.DoesNotExist", &[]));
        return true;
    };
    if let (Some(me), Some(them)) = (sender.as_player(), on.as_ref())
        && !sender.has_permission("mcmmo.commands.inspect.far")
    {
        let (a, b) = (me.get_location(), them.get_location());
        if a.get_world().get_name() != b.get_world().get_name() || a.distance(&b) > 30.0 {
            tell(sender, &l("Inspect.TooFar", &[]));
            return true;
        }
    }
    if let Some(me) = sender.as_player()
        && crate::scoreboard::enabled()
    {
        crate::scoreboard::stats(me, &p, &l("Scoreboard.Header.PlayerInspect", &[&p.name]));
        if !crate::scoreboard::prints("Inspect") {
            return true;
        }
    }
    tell(sender, &l(if on.is_some() { "Inspect.Stats" } else { "Inspect.OfflineStats" }, &[&p.name]));
    print_stats(sender, &p, on.as_ref().and_then(|o| perms_of(&uuid_of(o))));
    true
}

pub fn mmopower(sender: &CommandSender, _label: &str, args: &[&str]) -> bool {
    let p = match (args.first(), sender.as_player()) {
        (Some(name), _) => profile_named(name),
        (None, Some(me)) => profile::store().get(&uuid_of(me)).cloned(),
        _ => {
            tell(sender, &l("Commands.NoConsole", &[]));
            return true;
        }
    };
    let Some(p) = p else {
        tell(sender, &l("Commands.DoesNotExist", &[]));
        return true;
    };
    let s = settings::get();
    if s.power_cap > 0 {
        tell(sender, &l("Commands.PowerLevel.Capped", &[&p.power_level().to_string(), &s.power_cap.to_string()]));
    } else {
        tell(sender, &l("Commands.PowerLevel", &[&p.power_level().to_string()]));
    }
    true
}

/// `/mining`, `/swords`... the skill's page.
pub fn skill_command(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    let Some(skill) = Skill::parse(label) else { return true };
    let Some(player) = sender.as_player() else {
        tell(sender, &l("Commands.NoConsole", &[]));
        return true;
    };
    if args.first() == Some(&"?") {
        let page: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(1).max(1);
        let key = format!("Guides.{}.Section.{}", skill.name(), page - 1);
        tell(sender, &l("Guides.Header", &[&mmo::skill_name(skill)]));
        if crate::locale::has(&key) {
            tell(sender, &l(&key, &[]));
        } else {
            tell(sender, &l("Guides.Page.Invalid", &[]));
        }
        return true;
    }
    let uuid = uuid_of(player);
    let Some(p) = profile::store().get(&uuid).cloned() else {
        tell(sender, &l("Profile.PendingLoad", &[]));
        return true;
    };
    let s = settings::get();
    tell(sender, &l("Skills.Overhaul.Header", &[&mmo::skill_name(skill)]));
    if skill.is_child() {
        tell(sender, &l("Commands.XPGain.Overhaul", &[&l("Commands.XPGain.Child", &[])]));
        let parents: Vec<String> = skill.parents().iter().map(|pp| l("Effects.Child.ParentList", &[&mmo::skill_name(*pp), &p.level[pp.idx()].to_string()])).collect();
        tell(sender, &l("Effects.Child.Overhaul", &[&p.level(skill).to_string(), &parents.join(" ")]));
    } else {
        tell(sender, &l("Commands.XPGain.Overhaul", &[&l(&format!("Commands.XPGain.{}", skill.name()), &[])]));
        let need = mmo::xp_to_level(&s, &p, skill);
        tell(sender, &l("Effects.Level.Overhaul", &[&p.level[skill.idx()].to_string(), &(p.xp[skill.idx()] as i64).to_string(), &need.to_string()]));
    }
    tell(sender, &l("Skills.Overhaul.Header", &[&l("Effects.SubSkills.Overhaul", &[])]));
    let level = p.level(skill);
    let prefix = format!("{}.", skill.name());
    let mut subs: Vec<(&String, &Vec<i32>)> = s.ranks.iter().filter(|(k, _)| k.starts_with(&prefix)).collect();
    subs.sort_by_key(|(k, _)| (*k).clone());
    for (key, levels) in subs {
        let sub = &key[prefix.len()..];
        let name_key = format!("{}.SubSkill.{sub}.Name", skill.name());
        let name = if crate::locale::has(&name_key) { l(&name_key, &[]) } else { sub.to_owned() };
        let rank = levels.iter().filter(|lv| level >= **lv).count();
        if rank == 0 {
            let need = levels.first().copied().unwrap_or(0);
            tell(sender, &l("Ability.Generic.Template.Lock", &[&format!("{name} (LOCKED UNTIL {need}+)")]));
            continue;
        }
        let section = format!("Skills.{}.{sub}", skill.name());
        let detail = if s.adv.contains_key(&format!("{section}.ChanceMax")) {
            format!("{:.2}%", s.chance(&section, level).min(100.0))
        } else {
            format!("Rank {rank}/{}", levels.len())
        };
        tell(sender, &l("Ability.Generic.Template", &[&name, &detail]));
    }
    if let Some(ab) = skill.super_ability() {
        let secs = {
            let capped = if s.ability_length_cap > 0 { level.min(s.ability_length_cap) } else { level };
            2 + capped / s.ability_length_step
        };
        tell(sender, &l("Ability.Generic.Template", &[&ab.display(), &format!("{secs}s")]));
    }
    tell(sender, &l("Guides.Available", &[&mmo::skill_name(skill), label]));
    true
}

/// `/mctop [skill] [page]`
pub fn mctop(sender: &CommandSender, _label: &str, args: &[&str]) -> bool {
    let (skill, page) = match args {
        [] => (None, 1),
        [a] => match a.parse::<usize>() {
            Ok(n) => (None, n),
            Err(_) => match Skill::parse(a) {
                Some(s) => (Some(s), 1),
                None => {
                    tell(sender, &l("Commands.Skill.Invalid", &[]));
                    return true;
                }
            },
        },
        [a, b, ..] => match Skill::parse(a) {
            Some(s) => (Some(s), b.parse().unwrap_or(1)),
            None => {
                tell(sender, &l("Commands.Skill.Invalid", &[]));
                return true;
            }
        },
    };
    if let Some(sk) = skill {
        if sk.is_child() {
            tell(sender, &l("Commands.Skill.ChildSkill", &[]));
            return true;
        }
        if !sender.has_permission(&format!("mcmmo.commands.mctop.{}", sk.lower())) && !sender.has_permission("mcmmo.commands.mctop") {
            no_perm(sender);
            return true;
        }
    }
    let boards = profile::boards();
    let board = match skill {
        Some(sk) => &boards.skills[sk.idx()],
        None => &boards.power,
    };
    let page = page.max(1);
    let rows: Vec<(String, i32)> = board.rows.iter().skip((page - 1) * 10).take(10).map(|r| (r.1.clone(), r.2)).collect();
    if let Some(me) = sender.as_player()
        && crate::scoreboard::enabled()
    {
        let title = match skill {
            Some(sk) => mmo::skill_name(sk),
            None => l("Scoreboard.Header.PowerLevel", &[]),
        };
        crate::scoreboard::show(me, "Top", &title, &rows);
        if !crate::scoreboard::prints("Top") {
            return true;
        }
    }
    match skill {
        Some(sk) => tell(sender, &l("Commands.Skill.Leaderboard", &[&mmo::skill_name(sk)])),
        None => tell(sender, &l("Commands.PowerLevel.Leaderboard", &[])),
    }
    for (i, (name, level)) in rows.iter().enumerate() {
        tell(sender, &format!("{:>2}. §a{} §f- §f{}", (page - 1) * 10 + i + 1, name, level));
    }
    tell(sender, &l("Commands.mctop.Tip", &[]));
    true
}

/// `/mcrank [player]`
pub fn mcrank(sender: &CommandSender, _label: &str, args: &[&str]) -> bool {
    let p = match (args.first(), sender.as_player()) {
        (Some(name), _) => {
            if !sender.has_permission("mcmmo.commands.mcrank.others") {
                no_perm(sender);
                return true;
            }
            profile_named(name)
        }
        (None, Some(me)) => profile::store().get(&uuid_of(me)).cloned(),
        _ => {
            tell(sender, &l("Commands.NoConsole", &[]));
            return true;
        }
    };
    let Some(p) = p else {
        tell(sender, &l("Commands.DoesNotExist", &[]));
        return true;
    };
    let boards = profile::boards();
    let mut rows = Vec::new();
    tell(sender, &l("Commands.mcrank.Heading", &[]));
    tell(sender, &l("Commands.mcrank.Player", &[&p.name]));
    for sk in Skill::non_child() {
        let place = boards.skills[sk.idx()].place.get(&p.uuid).copied();
        let text = place.map_or_else(|| l("Commands.mcrank.Unranked", &[]), |n| n.to_string());
        tell(sender, &l("Commands.mcrank.Skill", &[&mmo::skill_name(sk), &text]));
        rows.push((mmo::skill_name(sk), place.map(|n| n as i32).unwrap_or(0)));
    }
    let overall = boards.power.place.get(&p.uuid).copied();
    tell(sender, &l("Commands.mcrank.Overall", &[&overall.map_or_else(|| l("Commands.mcrank.Unranked", &[]), |n| n.to_string())]));
    if let Some(me) = sender.as_player() {
        rows.push((l("Scoreboard.Misc.Overall", &[]), overall.map(|n| n as i32).unwrap_or(0)));
        crate::scoreboard::show(me, "Rank", &l("Scoreboard.Header.PlayerRank", &[]), &rows);
    }
    true
}

/// `/addlevels`, `/addxp`, `/mmoedit`: `[player] <skill|all> <amount> [-s]`.
pub fn experience(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    let kind = match label.to_ascii_lowercase().as_str() {
        "addlevels" => 0,
        "addxp" => 1,
        _ => 2,
    };
    let silent = args.last() == Some(&"-s");
    let args: Vec<&str> = args.iter().copied().filter(|a| *a != "-s").collect();
    let (target, skill_arg, amount_arg) = match args.as_slice() {
        [skill, amount] => match sender.as_player() {
            Some(me) => (me.get_name().to_owned(), *skill, *amount),
            None => {
                tell(sender, &l("Commands.Usage.3", &[label, "<player>", "<skill>", "<amount>"]));
                return true;
            }
        },
        [player, skill, amount] => {
            if !sender.has_permission(&format!("mcmmo.commands.{}.others", label.to_ascii_lowercase())) {
                no_perm(sender);
                return true;
            }
            ((*player).to_owned(), *skill, *amount)
        }
        _ => {
            tell(sender, &l("Commands.Usage.3.XP", &[label, "<player>", "<skill>", "<amount>"]));
            return true;
        }
    };
    let Ok(amount) = amount_arg.parse::<i64>() else {
        tell(sender, &l("Commands.Usage.3", &[label, "<player>", "<skill>", "<amount>"]));
        return true;
    };
    if amount < 0 && kind != 2 {
        tell(sender, &l("Commands.NegativeNumberWarn", &[]));
        return true;
    }
    let all = skill_arg.eq_ignore_ascii_case("all");
    let skill = Skill::parse(skill_arg);
    if !all && skill.is_none() {
        tell(sender, &l("Commands.Skill.Invalid", &[]));
        return true;
    }
    if skill.is_some_and(Skill::is_child) {
        tell(sender, &l("Commands.Skill.ChildSkill", &[]));
        return true;
    }
    let skills: Vec<Skill> = if all { Skill::non_child().collect() } else { vec![skill.unwrap_or(Skill::Mining)] };
    let uuid = profile::store().uuid_of(&target);
    let Some(uuid) = uuid else {
        tell(sender, &l("Commands.DoesNotExist", &[]));
        return true;
    };
    let mut out = Out::default();
    {
        let mut ss = mmo::sessions();
        let mut st = profile::store();
        for sk in &skills {
            match kind {
                0 => {
                    if let Some(p) = st.get_mut(&uuid) {
                        p.level[sk.idx()] += amount as i32;
                        p.xp[sk.idx()] = 0.0;
                    }
                }
                1 => {
                    if ss.contains_key(&uuid) {
                        mmo::apply(&mut ss, &mut st, &mut out, &settings::get(), &uuid, *sk, amount as f32, Reason::Command);
                    } else if let Some(p) = st.get_mut(&uuid) {
                        p.xp[sk.idx()] += amount as f32;
                    }
                }
                _ => {
                    if let Some(p) = st.get_mut(&uuid) {
                        p.level[sk.idx()] = amount.max(0) as i32;
                        p.xp[sk.idx()] = 0.0;
                    }
                }
            }
        }
    }
    out.run();
    profile::refresh_boards();
    let skill_name = if all { "all".to_owned() } else { mmo::skill_name(skills[0]) };
    let amount_s = amount.to_string();
    if !silent && let Some(p) = online(&target) {
        let msg = match (kind, all) {
            (0, true) => l("Commands.addlevels.AwardAll.1", &[&amount_s]),
            (0, false) => l("Commands.addlevels.AwardSkill.1", &[&amount_s, &skill_name]),
            (1, true) => l("Commands.addxp.AwardAll", &[&amount_s]),
            (1, false) => l("Commands.addxp.AwardSkill", &[&amount_s, &skill_name]),
            (_, true) => l("Commands.mmoedit.AllSkills.1", &[&amount_s]),
            (_, false) => l("Commands.mmoedit.Modified.1", &[&skill_name, &amount_s]),
        };
        p.send_message(&msg);
    }
    if sender.as_player().is_none_or(|me| !me.get_name().eq_ignore_ascii_case(&target)) {
        let msg = if all { l("Commands.addlevels.AwardAll.2", &[&target]) } else { l("Commands.addlevels.AwardSkill.2", &[&skill_name, &target]) };
        tell(sender, &msg);
    }
    true
}

/// `/skillreset [player] <skill|all>`
pub fn skillreset(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    let (target, skill_arg) = match args {
        [skill] => match sender.as_player() {
            Some(me) => (me.get_name().to_owned(), *skill),
            None => {
                tell(sender, &l("Commands.Usage.2", &[label, "<player>", "<skill>"]));
                return true;
            }
        },
        [player, skill, ..] => {
            if !sender.has_permission("mcmmo.commands.skillreset.others") {
                no_perm(sender);
                return true;
            }
            ((*player).to_owned(), *skill)
        }
        _ => {
            tell(sender, &l("Commands.Usage.1", &[label, "<skill|all>"]));
            return true;
        }
    };
    let all = skill_arg.eq_ignore_ascii_case("all");
    let skill = Skill::parse(skill_arg);
    if !all && skill.is_none() {
        tell(sender, &l("Commands.Skill.Invalid", &[]));
        return true;
    }
    let Some(uuid) = profile::store().uuid_of(&target) else {
        tell(sender, &l("Commands.DoesNotExist", &[]));
        return true;
    };
    {
        let mut st = profile::store();
        if let Some(p) = st.get_mut(&uuid) {
            for sk in Skill::non_child() {
                if all || Some(sk) == skill {
                    p.level[sk.idx()] = 0;
                    p.xp[sk.idx()] = 0.0;
                }
            }
        }
    }
    profile::refresh_boards();
    if let Some(p) = online(&target) {
        p.send_message(&if all { l("Commands.Reset.All", &[]) } else { l("Commands.Reset.Single", &[&mmo::skill_name(skill.unwrap_or(Skill::Mining))]) });
    }
    true
}

fn toggle_for(sender: &CommandSender, args: &[&str], others_perm: &str, f: impl Fn(&mut mmo::Session) -> bool, on: &str, off: &str, toggled: &str) -> bool {
    let target = match args.first() {
        Some(name) => {
            if !sender.has_permission(others_perm) {
                no_perm(sender);
                return true;
            }
            match online(name) {
                Some(p) => p,
                None => {
                    tell(sender, &l("Commands.Offline", &[]));
                    return true;
                }
            }
        }
        None => match sender.as_player() {
            Some(p) => p.clone(),
            None => {
                tell(sender, &l("Commands.NoConsole", &[]));
                return true;
            }
        },
    };
    let state = {
        let mut ss = mmo::sessions();
        match ss.get_mut(&uuid_of(&target)) {
            Some(s) => f(s),
            None => {
                tell(sender, &l("Commands.NotLoaded", &[]));
                return true;
            }
        }
    };
    target.send_message(&l(if state { on } else { off }, &[]));
    if args.first().is_some() && !toggled.is_empty() {
        tell(sender, &l(toggled, &[target.get_name()]));
    }
    true
}

pub fn mcability(sender: &CommandSender, _label: &str, args: &[&str]) -> bool {
    toggle_for(
        sender,
        args,
        "mcmmo.commands.mcability.others",
        |s| {
            s.ability_use = !s.ability_use;
            s.ability_use
        },
        "Commands.Ability.On",
        "Commands.Ability.Off",
        "Commands.Ability.Toggle",
    )
}

pub fn mcgod(sender: &CommandSender, _label: &str, args: &[&str]) -> bool {
    toggle_for(
        sender,
        args,
        "mcmmo.commands.mcgod.others",
        |s| {
            s.god = !s.god;
            s.god
        },
        "Commands.GodMode.Enabled",
        "Commands.GodMode.Disabled",
        "Commands.GodMode.Toggle",
    )
}

pub fn mcchatspy(sender: &CommandSender, _label: &str, args: &[&str]) -> bool {
    toggle_for(
        sender,
        args,
        "mcmmo.commands.mcchatspy.others",
        |s| {
            s.chat_spy = !s.chat_spy;
            s.chat_spy
        },
        "Commands.AdminChatSpy.Enabled",
        "Commands.AdminChatSpy.Disabled",
        "Commands.AdminChatSpy.Toggle",
    )
}

pub fn mcnotify(sender: &CommandSender, _label: &str, _args: &[&str]) -> bool {
    toggle_for(
        sender,
        &[],
        "",
        |s| {
            s.notify = !s.notify;
            s.notify
        },
        "Commands.Notifications.On",
        "Commands.Notifications.Off",
        "",
    )
}

pub fn mclevelupsound(sender: &CommandSender, _label: &str, _args: &[&str]) -> bool {
    toggle_for(
        sender,
        &[],
        "",
        |s| {
            s.levelup_sound = !s.levelup_sound;
            s.levelup_sound
        },
        "Commands.LevelUpSound.On",
        "Commands.LevelUpSound.Off",
        "",
    )
}

pub fn mmodebug(sender: &CommandSender, _label: &str, _args: &[&str]) -> bool {
    let Some(p) = sender.as_player() else {
        tell(sender, &l("Commands.NoConsole", &[]));
        return true;
    };
    let on = {
        let mut ss = mmo::sessions();
        ss.get_mut(&uuid_of(p)).map(|s| {
            s.debug = !s.debug;
            s.debug
        })
    };
    if let Some(on) = on {
        tell(sender, &l("Commands.Mmodebug.Toggle", &[if on { "true" } else { "false" }]));
    }
    true
}

pub fn mcrefresh(sender: &CommandSender, _label: &str, args: &[&str]) -> bool {
    let target = match args.first() {
        Some(name) => {
            if !sender.has_permission("mcmmo.commands.mcrefresh.others") {
                no_perm(sender);
                return true;
            }
            online(name)
        }
        None => sender.as_player().cloned(),
    };
    let Some(target) = target else {
        tell(sender, &l("Commands.Offline", &[]));
        return true;
    };
    let uuid = uuid_of(&target);
    {
        let mut ss = mmo::sessions();
        let mut st = profile::store();
        if let Some(s) = ss.get_mut(&uuid) {
            s.active = [0; 8];
            s.informed = [true; 8];
            s.prepared = None;
            s.recently_hurt = 0;
        }
        if let Some(p) = st.get_mut(&uuid) {
            p.dats = [0; 13];
        }
    }
    target.send_message(&l("Ability.Generic.Refresh", &[]));
    if args.first().is_some() {
        tell(sender, &l("Commands.mcrefresh.Success", &[target.get_name()]));
    }
    true
}

pub fn mccooldown(sender: &CommandSender, _label: &str, _args: &[&str]) -> bool {
    let Some(player) = sender.as_player() else {
        tell(sender, &l("Commands.NoConsole", &[]));
        return true;
    };
    let uuid = uuid_of(player);
    let rows: Vec<(String, i64)> = {
        let ss = mmo::sessions();
        let st = profile::store();
        let (Some(sess), Some(p)) = (ss.get(&uuid), st.get(&uuid)) else {
            tell(sender, &l("Profile.PendingLoad", &[]));
            return true;
        };
        ABILITIES.iter().filter(|a| sess.has(mmo::perm::ABILITY0 + a.idx())).map(|a| (a.display(), mmo::cooldown_left(sess, p, *a))).collect()
    };
    if crate::scoreboard::enabled() {
        let board: Vec<(String, i32)> = rows.iter().map(|(n, s)| (n.clone(), (*s).max(0) as i32)).collect();
        crate::scoreboard::show(player, "Cooldown", &l("Scoreboard.Header.PlayerCooldowns", &[]), &board);
        if !crate::scoreboard::prints("Cooldown") {
            return true;
        }
    }
    tell(sender, &l("Commands.Cooldowns.Header", &[]));
    tell(sender, &l("mcMMO.NoSkillNote", &[]));
    for (name, left) in rows {
        if left <= 0 {
            tell(sender, &l("Commands.Cooldowns.Row.Y", &[&name]));
        } else {
            tell(sender, &l("Commands.Cooldowns.Row.N", &[&name, &left.to_string()]));
        }
    }
    true
}

pub fn mcremove(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    let Some(name) = args.first() else {
        tell(sender, &l("Commands.Usage.1", &[label, "<player>"]));
        return true;
    };
    let removed = {
        let mut st = profile::store();
        match st.uuid_of(name) {
            Some(u) => st.remove(&u),
            None => false,
        }
    };
    if removed {
        profile::refresh_boards();
        tell(sender, &l("Commands.mcremove.Success", &[name]));
    } else {
        tell(sender, &l("Commands.DoesNotExist", &[]));
    }
    true
}

pub fn mmoshowdb(sender: &CommandSender, _label: &str, _args: &[&str]) -> bool {
    tell(sender, &l("Commands.mmoshowdb", &["FlatFile"]));
    true
}

pub fn mcconvert(sender: &CommandSender, _label: &str, _args: &[&str]) -> bool {
    tell(sender, "§cThe Horizon mcMMO keeps its data in the FlatFile database (flatfile/mcmmo.users); conversion is not available.");
    true
}

pub fn mcpurge(sender: &CommandSender, _label: &str, _args: &[&str]) -> bool {
    let cutoff_months = horizon_kit::Config::load(&crate::folder(), "config.yml", include_str!("../data/config.yml")).int("Database_Purging.Old_User_Cutoff", 6);
    let now = profile::now_secs();
    let online: Vec<String> = horizon_bukkit::get_online_players().iter().map(uuid_of).collect();
    let removed = {
        let mut st = profile::store();
        let gone: Vec<String> = st.by_uuid.values().filter(|p| !online.contains(&p.uuid) && (p.power_level() == 0 || (cutoff_months > 0 && p.last_login > 0 && now - p.last_login > cutoff_months * 30 * 86400))).map(|p| p.uuid.clone()).collect();
        for u in &gone {
            st.remove(u);
        }
        gone.len()
    };
    profile::refresh_boards();
    tell(sender, &l("Commands.mcpurge.Success", &[]));
    tell(sender, &format!("§7({removed} removed)"));
    true
}

pub fn mcscoreboard(sender: &CommandSender, _label: &str, args: &[&str]) -> bool {
    let Some(player) = sender.as_player() else {
        tell(sender, &l("Commands.NoConsole", &[]));
        return true;
    };
    if !crate::scoreboard::enabled() {
        tell(sender, &l("Scoreboard.Disabled", &[]));
        return true;
    }
    match args.first().map(|a| a.to_ascii_lowercase()).as_deref() {
        Some("clear" | "reset") => {
            if crate::scoreboard::clear(player) {
                tell(sender, &l("Commands.Scoreboard.Clear", &[]));
            } else {
                tell(sender, &l("Commands.Scoreboard.NoBoard", &[]));
            }
        }
        Some("keep") => {
            if let Some(s) = mmo::sessions().get_mut(&uuid_of(player)) {
                s.board_keep = true;
            }
            tell(sender, &l("Commands.Scoreboard.Keep", &[]));
        }
        Some("time" | "timer") => {
            let n: i64 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(10);
            if let Some(s) = mmo::sessions().get_mut(&uuid_of(player)) {
                s.board_keep = false;
                s.board_until = profile::now_ms() + n * 1000;
            }
            tell(sender, &l("Commands.Scoreboard.Timer", &[&n.to_string()]));
        }
        _ => {
            for i in 0..4 {
                tell(sender, &l(&format!("Commands.Scoreboard.Help.{i}"), &[]));
            }
        }
    }
    true
}

pub fn mmoxpbar(sender: &CommandSender, _label: &str, args: &[&str]) -> bool {
    let Some(player) = sender.as_player() else {
        tell(sender, &l("Commands.NoConsole", &[]));
        return true;
    };
    let uuid = uuid_of(player);
    match args {
        [what] if what.eq_ignore_ascii_case("reset") => {
            if let Some(s) = mmo::sessions().get_mut(&uuid) {
                s.bar_hidden = [false; crate::skills::COUNT];
            }
            tell(sender, &l("Commands.XPBar.Reset", &[]));
        }
        [what] if what.eq_ignore_ascii_case("disable") => {
            let bars: Vec<horizon_bukkit::BossBar> = {
                let mut ss = mmo::sessions();
                match ss.get_mut(&uuid) {
                    Some(s) => {
                        s.bar_hidden = [true; crate::skills::COUNT];
                        s.bars.values().map(|b| b.bar.clone()).collect()
                    }
                    None => Vec::new(),
                }
            };
            for b in bars {
                b.set_visible(false);
            }
            tell(sender, &l("Commands.XPBar.DisableAll", &[]));
        }
        [skill, mode] => {
            let Some(sk) = Skill::parse(skill) else {
                tell(sender, &l("Commands.Skill.Invalid", &[]));
                return true;
            };
            let hide = mode.eq_ignore_ascii_case("hide");
            if let Some(s) = mmo::sessions().get_mut(&uuid) {
                s.bar_hidden[sk.idx()] = hide;
            }
            tell(sender, &l("Commands.XPBar.SettingChanged", &[&mmo::skill_name(sk), if hide { "hide" } else { "show" }]));
        }
        _ => tell(sender, &l("Commands.XPBar.Usage", &[])),
    }
    true
}

pub fn xprate(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    match args {
        [] => {
            if crate::xprate::active() {
                tell(sender, &l("Commands.xprate.current", &[&format!("{}", crate::xprate::global())]));
            } else {
                tell(sender, &l("Commands.xprate.none", &[]));
            }
        }
        [r] if r.eq_ignore_ascii_case("reset") || r.eq_ignore_ascii_case("clear") => {
            if !sender.has_permission("mcmmo.commands.xprate.reset") {
                no_perm(sender);
                return true;
            }
            crate::xprate::reset();
            for p in horizon_bukkit::get_online_players() {
                p.send_title(&l("Commands.Event.Stop", &[]), &l("Commands.Event.Stop.Subtitle", &[]), 10, 70, 20);
            }
        }
        [rate, rest @ ..] => {
            if !sender.has_permission("mcmmo.commands.xprate.set") {
                no_perm(sender);
                return true;
            }
            if let Some(sk) = Skill::parse(rate) {
                let Some(r) = rest.first().and_then(|r| r.parse::<f64>().ok()).filter(|r| *r > 0.0) else {
                    tell(sender, &l("Commands.xprate.invalid", &[]));
                    return true;
                };
                crate::xprate::set_skill(sk, r);
                horizon_bukkit::broadcast_message(&l("Commands.Event.XP.Skill", &[&mmo::skill_name(sk), &format!("{r}")]));
                return true;
            }
            let Some(r) = rate.parse::<f64>().ok().filter(|r| *r > 0.0) else {
                tell(sender, &l("Commands.Usage.2", &[label, "<rate>", "<true|false>"]));
                return true;
            };
            crate::xprate::set_global(r);
            if rest.first().is_some_and(|a| a.eq_ignore_ascii_case("true")) {
                for p in horizon_bukkit::get_online_players() {
                    p.send_title(&l("Commands.Event.Start", &[]), &l("Commands.Event.XP", &[&format!("{r}")]), 10, 70, 20);
                }
            }
            horizon_bukkit::broadcast_message(&l("Commands.Event.XP", &[&format!("{r}")]));
        }
    }
    true
}

pub fn mcmmo_cmd(sender: &CommandSender, _label: &str, args: &[&str]) -> bool {
    if args.first().is_some_and(|a| a.eq_ignore_ascii_case("reload")) && sender.has_permission("mcmmo.commands.reload") {
        crate::reload();
        tell(sender, "§amcMMO configs and locale reloaded.");
        return true;
    }
    if args.first().is_some_and(|a| a.eq_ignore_ascii_case("help") || a.eq_ignore_ascii_case("commands")) {
        tell(sender, &l("Commands.Other", &[]));
        for key in ["Commands.Stats", "Commands.SkillInfo", "Commands.Leaderboards", "Commands.Inspect", "Commands.ToggleAbility", "Commands.Party.Invite", "Commands.Party.Accept", "Commands.Party.Quit", "Commands.Party.Teleport", "Commands.mmoedit", "Commands.mcgod", "Commands.Reset"] {
            tell(sender, &l(key, &[]));
        }
        return true;
    }
    for line in l("mcMMO.Description", &[]).split(',') {
        tell(sender, line);
    }
    tell(sender, &l("mcMMO.Description.FormerDevs", &[]));
    tell(sender, "§6 - §aHorizon native rewrite (Rust), GPL-3.0");
    true
}

pub fn mmoinfo(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    let Some(name) = args.first() else {
        tell(sender, &l("Commands.Usage.1", &[label, "<subskill>"]));
        return true;
    };
    let s = settings::get();
    let wanted = name.to_ascii_lowercase().replace(['_', ' '], "");
    let found = s.ranks.keys().find(|k| k.split('.').nth(1).is_some_and(|sub| sub.to_ascii_lowercase() == wanted)).cloned();
    let Some(key) = found else {
        tell(sender, &l("Commands.MmoInfo.NoMatch", &[]));
        return true;
    };
    let (skill, sub) = key.split_once('.').unwrap_or_default();
    tell(sender, &l("Commands.MmoInfo.Header", &[]));
    let display = l(&format!("{skill}.SubSkill.{sub}.Name"), &[]);
    tell(sender, &l("Commands.MmoInfo.SubSkillHeader", &[&display]));
    tell(sender, &l("Commands.MmoInfo.DetailsHeader", &[]));
    let desc = format!("{skill}.SubSkill.{sub}.Description");
    if crate::locale::has(&desc) {
        tell(sender, &l(&desc, &[]));
    }
    if let Some(levels) = s.ranks.get(&key) {
        let ranks: Vec<String> = levels.iter().map(|x| x.to_string()).collect();
        tell(sender, &format!("§3Ranks unlock at: §a{}", ranks.join(", ")));
    }
    true
}

pub fn reloadlocale(sender: &CommandSender, _label: &str, _args: &[&str]) -> bool {
    crate::locale::load(&crate::folder());
    tell(sender, "§aLocale reloaded.");
    true
}

pub fn register(plugin: Plugin) {
    plugin.set_executor("mcstats", mcstats);
    plugin.set_executor("inspect", inspect);
    plugin.set_executor("mmopower", mmopower);
    plugin.set_executor("mctop", mctop);
    plugin.set_executor("mcrank", mcrank);
    for c in ["addlevels", "addxp", "mmoedit"] {
        plugin.set_executor(c, experience);
    }
    plugin.set_executor("skillreset", skillreset);
    plugin.set_executor("mcability", mcability);
    plugin.set_executor("mcgod", mcgod);
    plugin.set_executor("mcchatspy", mcchatspy);
    plugin.set_executor("mcnotify", mcnotify);
    plugin.set_executor("mclevelupsound", mclevelupsound);
    plugin.set_executor("mmodebug", mmodebug);
    plugin.set_executor("mcrefresh", mcrefresh);
    plugin.set_executor("mccooldown", mccooldown);
    plugin.set_executor("mcremove", mcremove);
    plugin.set_executor("mmoshowdb", mmoshowdb);
    plugin.set_executor("mcconvert", mcconvert);
    plugin.set_executor("mcpurge", mcpurge);
    plugin.set_executor("mcscoreboard", mcscoreboard);
    plugin.set_executor("mmoxpbar", mmoxpbar);
    plugin.set_executor("xprate", xprate);
    plugin.set_executor("mcmmo", mcmmo_cmd);
    plugin.set_executor("mmoinfo", mmoinfo);
    plugin.set_executor("mcmmoreloadlocale", reloadlocale);
    plugin.set_executor("party", crate::party::command);
    plugin.set_executor("partychat", |s: &CommandSender, _l: &str, a: &[&str]| crate::party::chat_command(s, a));
    plugin.set_executor("adminchat", |s: &CommandSender, _l: &str, a: &[&str]| crate::party::admin_command(s, a));
    plugin.set_executor("ptp", |s: &CommandSender, _l: &str, a: &[&str]| crate::party::ptp(s, a));
    for sk in crate::skills::ALL {
        plugin.set_executor(sk.lower(), skill_command);
    }
    // Tab completion: skills, players.
    let skills = |_s: &CommandSender, _l: &str, a: &[&str]| -> Vec<String> {
        let last = a.last().map(|x| x.to_ascii_lowercase()).unwrap_or_default();
        let mut out: Vec<String> = Skill::non_child().map(|s| s.lower().to_owned()).collect();
        out.push("all".into());
        out.into_iter().filter(|s| s.starts_with(&last)).collect()
    };
    let players_then_skills = |_s: &CommandSender, _l: &str, a: &[&str]| -> Vec<String> {
        let last = a.last().map(|x| x.to_ascii_lowercase()).unwrap_or_default();
        let list: Vec<String> = if a.len() <= 1 {
            horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).chain(Skill::non_child().map(|s| s.lower().to_owned())).collect()
        } else if a.len() == 2 {
            Skill::non_child().map(|s| s.lower().to_owned()).chain(std::iter::once("all".to_owned())).collect()
        } else {
            Vec::new()
        };
        list.into_iter().filter(|s| s.to_ascii_lowercase().starts_with(&last)).collect()
    };
    for c in ["mctop"] {
        plugin.set_tab_completer(c, skills);
    }
    for c in ["addlevels", "addxp", "mmoedit", "skillreset"] {
        plugin.set_tab_completer(c, players_then_skills);
    }
    plugin.set_tab_completer("party", |_s: &CommandSender, _l: &str, a: &[&str]| -> Vec<String> {
        let subs = ["join", "accept", "create", "help", "info", "quit", "xpshare", "itemshare", "invite", "kick", "disband", "owner", "lock", "unlock", "password", "rename", "chat", "teleport"];
        let last = a.last().map(|x| x.to_ascii_lowercase()).unwrap_or_default();
        if a.len() <= 1 {
            subs.iter().filter(|s| s.starts_with(&last)).map(|s| (*s).to_owned()).collect()
        } else {
            horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).filter(|n| n.to_ascii_lowercase().starts_with(&last)).collect()
        }
    });
}
