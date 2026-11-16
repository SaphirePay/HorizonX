//! Every AdvancedBan command (Command.java): permission, argument check (the Java regexes), then the processor.

use crate::store::{self, PType, Punishment};
use crate::{create, delete, duration_of, files, format_date, has_layout, has_perms, is_ip, layout, now, prefix, reason_of, send, to_millis, uuid_of};
use horizon_bukkit::{CommandSender, Plugin};

const NAMES: &[&str] = &[
    "advancedban", "ban", "tempban", "ipban", "tempipban", "kick", "warn", "note", "tempwarn", "mute", "tempmute", "banlist", "history", "warns", "notes", "check", "systemprefs", "unwarn", "unnote", "unpunish", "unban", "unmute", "change-reason",
];

pub fn register(plugin: &Plugin) {
    for name in NAMES {
        let cmd: &'static str = name;
        let registered = if cmd == "advancedban" { "AdvancedBan" } else if cmd == "systemprefs" { "systemPrefs" } else { cmd };
        plugin.set_executor(registered, move |s, _, a| {
            run(s, cmd, a);
            true
        });
        plugin.set_tab_completer(registered, move |s, _, a| complete(s, cmd, a));
    }
}

fn kind_of(cmd: &str) -> Option<PType> {
    Some(match cmd {
        "ban" => PType::Ban,
        "tempban" => PType::TempBan,
        "ipban" => PType::IpBan,
        "tempipban" => PType::TempIpBan,
        "mute" => PType::Mute,
        "tempmute" => PType::TempMute,
        "warn" => PType::Warning,
        "tempwarn" => PType::TempWarning,
        "note" => PType::Note,
        "kick" => PType::Kick,
        _ => return None,
    })
}

fn is_time(s: &str) -> bool {
    if let Some(l) = s.strip_prefix('#') {
        return !l.is_empty();
    }
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    !digits.is_empty() && !digits.starts_with('0') && matches!(&s[digits.len()..], "w" | "d" | "h" | "m" | "s" | "mo")
}

fn page_ok(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.chars().all(|c| c.is_ascii_digit())
}

/// The Java command's argument pattern.
fn valid(cmd: &str, a: &[&str]) -> bool {
    match cmd {
        "ban" | "ipban" | "mute" | "warn" | "note" | "kick" => !a.is_empty(),
        "tempban" | "tempipban" | "tempmute" | "tempwarn" => {
            let a = if a.first().is_some_and(|f| f.eq_ignore_ascii_case("-s")) { &a[1..] } else { a };
            a.len() >= 2 && is_time(a[1])
        }
        "unban" | "unmute" | "check" => a.len() == 1,
        "unwarn" | "unnote" => (a.len() == 1 && a[0].chars().all(|c| c.is_ascii_digit())) || (a.len() == 2 && a[0].eq_ignore_ascii_case("clear")),
        "unpunish" => a.len() == 1 && a[0].chars().all(|c| c.is_ascii_digit()),
        "change-reason" => (a.len() >= 2 && a[0].chars().all(|c| c.is_ascii_digit())) || (a.len() >= 3 && (a[0].eq_ignore_ascii_case("ban") || a[0].eq_ignore_ascii_case("mute"))),
        "banlist" => a.is_empty() || (a.len() == 1 && page_ok(a[0])),
        "history" => a.len() == 1 || (a.len() == 2 && page_ok(a[1])),
        "warns" | "notes" => a.len() <= 2,
        _ => true,
    }
}

fn permission(cmd: &str) -> Option<String> {
    if let Some(k) = kind_of(cmd) {
        return Some(k.perms().to_owned());
    }
    Some(
        match cmd {
            "unban" => "ab.ban.undo",
            "unmute" => "ab.mute.undo",
            "unwarn" => "ab.warn.undo",
            "unnote" => "ab.note.undo",
            "unpunish" => "ab.all.undo",
            "change-reason" => "ab.changeReason",
            "banlist" => "ab.banlist",
            "history" => "ab.history",
            "check" => "ab.check",
            "systemprefs" => "ab.systemprefs",
            _ => return None,
        }
        .to_owned(),
    )
}

fn usage_path(cmd: &str) -> String {
    match kind_of(cmd) {
        Some(k) => format!("{}.Usage", k.name()),
        None => match cmd {
            "unban" => "UnBan.Usage".into(),
            "unmute" => "UnMute.Usage".into(),
            "unwarn" => "UnWarn.Usage".into(),
            "unnote" => "UnNote.Usage".into(),
            "unpunish" => "UnPunish.Usage".into(),
            "change-reason" => "ChangeReason.Usage".into(),
            "banlist" => "Banlist.Usage".into(),
            "history" => "History.Usage".into(),
            "warns" => "Warns.Usage".into(),
            "notes" => "Notes.Usage".into(),
            "check" => "Check.Usage".into(),
            _ => String::new(),
        },
    }
}

fn operator(sender: &CommandSender) -> String {
    match sender {
        CommandSender::Player(p) => p.get_name().to_owned(),
        _ => "CONSOLE".into(),
    }
}

pub fn run(sender: &CommandSender, cmd: &str, args: &[&str]) {
    // A command another plugin also registers (Essentials' /ban, /tempban, /mute) reaches this executor twice
    // on Horizon today: the second, identical within 150 ms from the same sender, is the same command.
    {
        static LAST: std::sync::Mutex<Option<(String, std::time::Instant)>> = std::sync::Mutex::new(None);
        let key = format!("{}\u{0}{cmd}\u{0}{}", sender.get_name(), args.join(" "));
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_ref().is_some_and(|(k, at)| *k == key && at.elapsed() < std::time::Duration::from_millis(150)) {
            return;
        }
        *last = Some((key, std::time::Instant::now()));
    }
    if let Some(perm) = permission(cmd)
        && !has_perms(sender, &perm)
    {
        return send(sender, "General.NoPerms", true, &[]);
    }
    if !valid(cmd, args) {
        return send(sender, &usage_path(cmd), true, &[]);
    }
    let mut a: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    match cmd {
        c if kind_of(c).is_some() => punish(sender, kind_of(c).expect("kind"), &mut a),
        "unban" => revoke(sender, PType::Ban, &a[0]),
        "unmute" => revoke(sender, PType::Mute, &a[0]),
        "unwarn" => revoke_list(sender, PType::Warning, "UnWarn", &a),
        "unnote" => revoke_list(sender, PType::Note, "UnNote", &a),
        "unpunish" => revoke_id(sender, "UnPunish", a[0].parse().unwrap_or(-1), None),
        "change-reason" => change_reason(sender, &a),
        "banlist" => {
            let mut all = store::mem().active.clone();
            all.sort_by(|x, y| y.start.cmp(&x.start));
            all.truncate(150);
            list(sender, "Banlist", "invalid", all, false, a.first().map(String::as_str));
        }
        "history" => {
            let name = a[0].clone();
            let target = if is_ip(&name) { name.clone() } else {
                match uuid_of(&name) {
                    Some(u) => u,
                    None => return send(sender, "General.FailedFetch", true, &[("NAME", &name)]),
                }
            };
            let page = a.get(1).cloned();
            let reply_to = sender.clone();
            store::send(store::Job::History(
                vec![target],
                Box::new(move |rows| {
                    crate::on_main(move || list(&reply_to, "History", &name, rows, true, page.as_deref()));
                }),
            ));
        }
        "warns" | "notes" => {
            let (kind, other, own, perm_other, perm_own) = if cmd == "warns" { (PType::Warning, "Warns", "WarnsOwn", "ab.warns.other", "ab.warns.own") } else { (PType::Note, "Notes", "NotesOwn", "ab.notes.other", "ab.notes.own") };
            if a.first().is_some_and(|f| !page_ok(f)) {
                if !has_perms(sender, perm_other) {
                    return send(sender, "General.NoPerms", true, &[]);
                }
                let name = a[0].clone();
                let Some(uuid) = uuid_of(&name) else { return send(sender, "General.FailedFetch", true, &[("NAME", &name)]) };
                let rows = store::of(&uuid, kind, now());
                list(sender, other, &name, rows, false, a.get(1).map(String::as_str));
            } else {
                if !has_perms(sender, perm_own) {
                    return send(sender, "General.NoPerms", true, &[]);
                }
                let name = sender.get_name().to_owned();
                let Some(uuid) = uuid_of(&name) else { return send(sender, "General.FailedFetch", true, &[("NAME", &name)]) };
                let rows = store::of(&uuid, kind, now());
                list(sender, own, &name, rows, false, a.first().map(String::as_str));
            }
        }
        "check" => check(sender, &a[0]),
        "systemprefs" => {
            let secs = now() / 1000;
            sender.send_message("§c§lAdvancedBan v2 §cSystemPrefs");
            sender.send_message(&format!("§cServer-Time §8» §7{}:{}", secs / 3600 % 24, secs / 60 % 60));
            sender.send_message(&format!("§cYour UUID (Intern) §8» §7{}", match sender {
                CommandSender::Player(p) => p.get_unique_id().replace('-', ""),
                _ => "none".into(),
            }));
            if let Some(t) = a.first() {
                let id = uuid_of(t).unwrap_or_else(|| "none".into());
                sender.send_message(&format!("§c{t}'s UUID (Intern) §8» §7{id}"));
                sender.send_message(&format!("§c{t}'s UUID (Fetched) §8» §7{id}"));
            }
        }
        "advancedban" => advancedban(sender, &a),
        _ => {}
    }
}

/// PunishmentProcessor.
fn punish(sender: &CommandSender, kind: PType, a: &mut Vec<String>) {
    let silent = match a.iter().take(4).position(|x| x.eq_ignore_ascii_case("-s")) {
        Some(i) => {
            a.remove(i);
            true
        }
        None => false,
    };
    if a.is_empty() {
        return send(sender, &format!("{}.Usage", kind.name()), true, &[]);
    }
    let name = a.remove(0);
    if kind == PType::Kick && !horizon_bukkit::get_online_players().iter().any(|p| p.get_name().eq_ignore_ascii_case(&name)) {
        return send(sender, "Kick.NotOnline", true, &[("NAME", &name)]);
    }
    let target = if kind.ip() {
        if is_ip(&name) {
            name.clone()
        } else {
            match store::mem().ips.get(&name.to_ascii_lowercase()).cloned() {
                Some(ip) => ip,
                None => return send(sender, "Ipban.IpNotCashed", true, &[("NAME", &name)]),
            }
        }
    } else {
        match uuid_of(&name) {
            Some(u) => u,
            None => return send(sender, "General.FailedFetch", true, &[("NAME", &name)]),
        }
    };
    if exempt(&name, sender, kind) {
        return send(sender, &format!("{}.Exempt", kind.basic().name()), true, &[("NAME", &name)]);
    }
    let mut end = -1;
    let mut template = String::new();
    if kind.is_temp() {
        if a.is_empty() {
            return send(sender, &format!("{}.Usage", kind.name()), true, &[]);
        }
        let time = a.remove(0);
        if let Some(l) = time.strip_prefix('#') {
            if !has_layout(true, &format!("Time.{l}")) {
                return send(sender, "General.LayoutNotFound", true, &[("NAME", l)]);
            }
            let level = store::level(&target, l) as usize;
            let steps = files(|f| f.layouts.list(&format!("Time.{l}")));
            let step = steps.get(level.min(steps.len().saturating_sub(1))).cloned().unwrap_or_else(|| "perma".into());
            end = if step.eq_ignore_ascii_case("perma") { -1 } else { now() + to_millis(&step) };
            template = l.to_owned();
        } else {
            let add = to_millis(&time);
            if !has_perms(sender, &format!("ab.{}.dur.max", kind.name())) {
                let max = files(|f| (1..=10).rev().find(|i| has_perms(sender, &format!("ab.{}.dur.{i}", kind.name())) && f.config.at(&format!("TempPerms.{i}")).is_some()).map(|i| f.config.int(&format!("TempPerms.{i}"), 0) * 1000));
                if let Some(max) = max
                    && add > max
                {
                    return send(sender, &format!("{}.MaxDuration", kind.name()), true, &[("MAX", &(max / 1000).to_string())]);
                }
            }
            end = now() + add;
        }
    }
    let reason = a.join(" ");
    if (reason.starts_with('@') || reason.starts_with('~')) && reason.len() > 1 {
        let first = reason.split(' ').next().unwrap_or("");
        if !has_layout(true, &format!("Message.{}", &first[1..])) {
            return send(sender, "General.LayoutNotFound", true, &[("NAME", &first[1..])]);
        }
    }
    let reason = (!reason.is_empty()).then_some(reason);
    let basic = kind.basic();
    if (basic == PType::Mute || basic == PType::Ban) && store::find(&[&target], basic, now()).is_some() {
        return send(sender, &format!("{}.AlreadyDone", basic.name()), true, &[("NAME", &name)]);
    }
    create(&name, &target, reason, &operator(sender), kind, end, &template, silent);
    send(sender, &format!("{}.Done", basic.name()), true, &[("NAME", &name)]);
}

/// PunishmentProcessor.processExempt: ExemptPlayers, `ab.<type>.exempt` and its levels.
fn exempt(name: &str, sender: &CommandSender, kind: PType) -> bool {
    let perm = format!("ab.{}.exempt", kind.name());
    let level = |has: &dyn Fn(&str) -> bool| (1..=10).rev().find(|i| has(&format!("{perm}.{i}"))).unwrap_or(0);
    match horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name)) {
        Some(target) => {
            let t = |p: &str| crate::player_has(&target, p);
            if t(&perm) {
                return true;
            }
            let tl = level(&t);
            tl != 0 && level(&|p: &str| has_perms(sender, p)) <= tl
        }
        None => files(|f| f.config.list("ExemptPlayers")).iter().any(|e| e.eq_ignore_ascii_case(name)),
    }
}

/// RevokeProcessor (unban / unmute).
fn revoke(sender: &CommandSender, kind: PType, name: &str) {
    let target = if is_ip(name) { name.to_owned() } else {
        match uuid_of(name) {
            Some(u) => u,
            None => return send(sender, "General.FailedFetch", true, &[("NAME", name)]),
        }
    };
    match store::find(&[&target], kind, now()) {
        None => send(sender, &format!("Un{}.NotPunished", kind.name()), true, &[("NAME", name)]),
        Some(p) => {
            delete(&p, Some(&operator(sender)));
            send(sender, &format!("Un{}.Done", kind.name()), true, &[("NAME", name)]);
        }
    }
}

fn revoke_id(sender: &CommandSender, path: &str, id: i64, kind: Option<PType>) {
    match store::by_id(id).filter(|p| kind.is_none_or(|k| p.kind.basic() == k)) {
        None => send(sender, &format!("{path}.NotFound"), true, &[("ID", &id.to_string())]),
        Some(p) => {
            delete(&p, Some(&operator(sender)));
            send(sender, &format!("{path}.Done"), true, &[("ID", &id.to_string())]);
        }
    }
}

fn revoke_list(sender: &CommandSender, kind: PType, path: &str, a: &[String]) {
    if a[0].eq_ignore_ascii_case("clear") {
        let name = &a[1];
        let Some(uuid) = uuid_of(name) else { return send(sender, "General.FailedFetch", true, &[("NAME", name)]) };
        let list = store::of(&uuid, kind, now());
        if list.is_empty() {
            return send(sender, &format!("{path}.Clear.Empty"), true, &[("NAME", name)]);
        }
        let op = operator(sender);
        for p in &list {
            delete(p, Some(&op));
        }
        send(sender, &format!("{path}.Clear.Done"), true, &[("COUNT", &list.len().to_string())]);
    } else {
        revoke_id(sender, path, a[0].parse().unwrap_or(-1), Some(kind));
    }
}

fn change_reason(sender: &CommandSender, a: &[String]) {
    let (p, rest): (Option<Punishment>, &[String]) = if a[0].chars().all(|c| c.is_ascii_digit()) {
        (store::by_id(a[0].parse().unwrap_or(-1)), &a[1..])
    } else {
        let kind = if a[0].eq_ignore_ascii_case("mute") { PType::Mute } else { PType::Ban };
        let target = if is_ip(&a[1]) { a[1].clone() } else {
            match uuid_of(&a[1]) {
                Some(u) => u,
                None => return send(sender, "General.FailedFetch", true, &[("NAME", &a[1])]),
            }
        };
        (store::find(&[&target], kind, now()), &a[2..])
    };
    let reason = rest.join(" ");
    if (reason.starts_with('@') || reason.starts_with('~')) && reason.len() > 1 {
        let first = reason.split(' ').next().unwrap_or("");
        if !has_layout(true, &format!("Message.{}", &first[1..])) {
            return send(sender, "General.LayoutNotFound", true, &[("NAME", &first[1..])]);
        }
    }
    match p {
        Some(p) => {
            store::set_reason(p.id, &reason);
            send(sender, "ChangeReason.Done", true, &[("ID", &p.id.to_string())]);
        }
        None => send(sender, "ChangeReason.NotFound", true, &[]),
    }
}

/// ListProcessor: five entries a page.
fn list(sender: &CommandSender, config: &str, name: &str, mut rows: Vec<Punishment>, history: bool, page: Option<&str>) {
    if !history {
        let t = now();
        let gone: Vec<i64> = rows.iter().filter(|p| p.expired(t)).map(|p| p.id).collect();
        for id in gone {
            store::remove(id);
        }
        rows.retain(|p| !p.expired(t));
    }
    if rows.is_empty() {
        return send(sender, &format!("{config}.NoEntries"), true, &[("NAME", name)]);
    }
    let page: usize = page.and_then(|p| p.parse().ok()).unwrap_or(1);
    if rows.len() as f64 / 5.0 + 1.0 <= page as f64 {
        return send(sender, &format!("{config}.OutOfIndex"), true, &[("PAGE", &page.to_string())]);
    }
    let pre = prefix();
    for line in layout(false, &format!("{config}.Header"), &[("PREFIX", &pre), ("NAME", name)]) {
        sender.send_message(&line);
    }
    for p in rows.iter().skip((page - 1) * 5).take(5) {
        let who = if p.kind.ip() { format!("{} / {}", p.name, p.uuid) } else { p.name.clone() };
        for line in layout(false, &format!("{config}.Entry"), &[("PREFIX", &pre), ("NAME", &who), ("DURATION", &duration_of(p, history)), ("OPERATOR", &p.operator), ("REASON", &reason_of(p)), ("TYPE", p.kind.name()), ("ID", &p.id.to_string()), ("DATE", &format_date(p.start))]) {
            sender.send_message(&line);
        }
    }
    let total = rows.len() / 5 + usize::from(rows.len() % 5 != 0);
    send(sender, &format!("{config}.Footer"), false, &[("CURRENT_PAGE", &page.to_string()), ("TOTAL_PAGES", &total.to_string()), ("COUNT", &rows.len().to_string())]);
    if rows.len() as f64 / 5.0 + 1.0 > (page + 1) as f64 {
        send(sender, &format!("{config}.PageFooter"), false, &[("NEXT_PAGE", &(page + 1).to_string()), ("NAME", name)]);
    }
}

fn check(sender: &CommandSender, name: &str) {
    let Some(uuid) = uuid_of(name) else { return send(sender, "General.FailedFetch", true, &[("NAME", name)]) };
    let ip = store::mem().ips.get(&name.to_ascii_lowercase()).cloned();
    let cached = crate::message("Check.Cached", &[]);
    let not_cached = crate::message("Check.NotCached", &[]);
    let online = horizon_bukkit::get_online_players().iter().any(|p| p.get_name().eq_ignore_ascii_case(name));
    let t = now();
    let mute = store::find(&[&uuid], PType::Mute, t);
    let ban = store::find(&[&uuid], PType::Ban, t);
    send(sender, "Check.Header", true, &[("NAME", name), ("CACHED", if online { &cached } else { &not_cached })]);
    send(sender, "Check.UUID", false, &[("UUID", &uuid), ("CACHED", if online { &cached } else { &not_cached })]);
    if has_perms(sender, "ab.check.ip") {
        send(sender, "Check.IP", false, &[("IP", ip.as_deref().unwrap_or("none cashed")), ("CACHED", if ip.is_some() { &cached } else { &not_cached })]);
    }
    let dur = |p: &Option<Punishment>| p.as_ref().map_or("§anone".to_owned(), |p| if p.kind.is_temp() { format!("§e{}", duration_of(p, false)) } else { "§cperma".into() });
    send(sender, "Check.Mute", false, &[("DURATION", &dur(&mute))]);
    if let Some(m) = &mute {
        send(sender, "Check.MuteReason", false, &[("REASON", &reason_of(m))]);
    }
    send(sender, "Check.Ban", false, &[("DURATION", &dur(&ban))]);
    if let Some(b) = &ban {
        send(sender, "Check.BanReason", false, &[("REASON", &reason_of(b))]);
    }
    send(sender, "Check.Warn", false, &[("COUNT", &crate::current_warns(&uuid).to_string())]);
    send(sender, "Check.Note", false, &[("COUNT", &store::of(&uuid, PType::Note, t).len().to_string())]);
    // The country comes from ip-api.com, asked on a thread of its own (it arrives after the other lines).
    if let Some(ip) = ip {
        let to = sender.clone();
        std::thread::spawn(move || {
            let country = ureq::get(&format!("http://ip-api.com/json/{ip}")).timeout(std::time::Duration::from_secs(5)).call().ok().and_then(|r| r.into_string().ok()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v["country"].as_str().map(str::to_owned));
            crate::on_main(move || send(&to, "Check.Geo", false, &[("LOCATION", country.as_deref().unwrap_or("failed!"))]));
        });
    } else {
        send(sender, "Check.Geo", false, &[("LOCATION", "failed!")]);
    }
}

fn advancedban(sender: &CommandSender, a: &[String]) {
    match a.first().map(String::as_str) {
        Some("reload") => {
            if has_perms(sender, "ab.reload") {
                crate::reload();
                sender.send_message("§a§lAdvancedBan §8§l» §7Reloaded!");
            } else {
                send(sender, "General.NoPerms", true, &[]);
            }
        }
        Some("help") => {
            if !has_perms(sender, "ab.help") {
                return send(sender, "General.NoPerms", true, &[]);
            }
            for (c, d) in [
                ("/ban [Name] [Reason/@Layout]", "Ban a user permanently"),
                ("/banip [Name/IP] [Reason/@Layout]", "Ban a user by IP"),
                ("/tempban [Name] [Xmo/Xd/Xh/Xm/Xs/#TimeLayout] [Reason/@Layout]", "Ban a user temporary"),
                ("/mute [Name] [Reason/@Layout]", "Mute a user permanently"),
                ("/tempmute [Name] [Xmo/Xd/Xh/Xm/Xs/#TimeLayout] [Reason/@Layout]", "Mute a user temporary"),
                ("/warn [Name] [Reason/@Layout]", "Warn a user permanently"),
                ("/note [Name] [Note]", "Adds a note to a user"),
                ("/tempwarn [Name] [Xmo/Xd/Xh/Xm/Xs/#TimeLayout] [Reason/@Layout]", "Warn a user temporary"),
                ("/kick [Name] [Reason/@Layout]", "Kick a user"),
                ("/unban [Name/IP]", "Unban a user"),
                ("/unmute [Name]", "Unmute a user"),
                ("/unwarn [ID] or /unwarn clear [Name]", "Deletes a warn"),
                ("/unnote [ID] or /unnote clear [Name]", "Deletes a note"),
                ("/change-reason [ID or ban/mute USER] [New reason]", "Changes the reason of a punishment"),
                ("/unpunish [ID]", "Deletes a punishment by ID"),
                ("/banlist <Page>", "See all punishments"),
                ("/history [Name/IP] <Page>", "See a users history"),
                ("/warns [Name] <Page>", "See your or a users warnings"),
                ("/notes [Name] <Page>", "See your or a users notes"),
                ("/check [Name]", "Get all information about a user"),
                ("/AdvancedBan <reload/help>", "Reloads the plugin or shows help page"),
            ] {
                sender.send_message(&format!("§c{c}"));
                sender.send_message(&format!("§8» §7{d}"));
            }
        }
        _ => {
            sender.send_message("§8§l§m-=====§r §c§lAdvancedBan v2 §8§l§m=====-§r ");
            sender.send_message("  §cDev §8• §7Leoko");
            sender.send_message("  §cStatus §8• §a§oStable");
            sender.send_message("  §cVersion §8• §72.3.0 (Rust)");
            sender.send_message("  §cLicense §8• §7GPL v3");
            sender.send_message("  §cStorage §8• §7SQLite");
            sender.send_message("  §cServer §8• §7Horizon");
            sender.send_message("  §cUUID-Mode §8• §7Server");
            sender.send_message("§8§l§m-=========================-§r ");
        }
    }
}

// ---- Tab completion ----

fn players() -> Vec<String> {
    horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).collect()
}

fn layouts(section: &str, mark: char) -> Vec<String> {
    files(|f| f.layouts.keys(section)).into_iter().map(|k| format!("{mark}{k}")).collect()
}

pub fn complete(sender: &CommandSender, cmd: &str, args: &[&str]) -> Vec<String> {
    if let Some(perm) = permission(cmd)
        && !has_perms(sender, &perm)
    {
        return Vec::new();
    }
    let last = args.last().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let silent = args.first().is_some_and(|a| a.eq_ignore_ascii_case("-s"));
    let n = args.len() - usize::from(silent);
    let out: Vec<String> = match cmd {
        c if kind_of(c).is_some() => {
            let kind = kind_of(c).expect("kind");
            match (n, kind.is_temp()) {
                (0 | 1, _) => {
                    let mut v = players();
                    if !silent {
                        v.push("-s".into());
                    }
                    v
                }
                (2, true) => {
                    let mut v = layouts("Time", '#');
                    v.extend(["1h", "1d", "1w", "1mo", "30m"].iter().map(|s| s.to_string()));
                    v
                }
                _ => {
                    let mut v = layouts("Message", '@');
                    v.push("Reason...".into());
                    v
                }
            }
        }
        "unban" | "unmute" | "check" | "history" if args.len() <= 1 => players(),
        "warns" | "notes" if args.len() <= 1 => players(),
        "unwarn" | "unnote" if args.len() <= 1 => vec!["clear".into()],
        "unwarn" | "unnote" if args.len() == 2 && args[0].eq_ignore_ascii_case("clear") => players(),
        "change-reason" if args.len() <= 1 => vec!["ban".into(), "mute".into()],
        "change-reason" if args.len() == 2 && !args[0].chars().all(|c| c.is_ascii_digit()) => players(),
        "advancedban" if args.len() <= 1 => vec!["help".into(), "reload".into()],
        _ => Vec::new(),
    };
    out.into_iter().filter(|o| o.to_ascii_lowercase().starts_with(&last)).collect()
}
