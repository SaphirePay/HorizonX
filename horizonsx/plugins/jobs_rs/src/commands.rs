//! `/jobs` and its sub-commands (permissions `jobs.command.<sub>` and `jobs.command.admin.<sub>` as Jobs).

use crate::model::{self, JoinError};
use crate::settings::{self, Action};
use horizon_bukkit::{CommandSender, Player};

fn tell(sender: &CommandSender, text: &str) {
    for line in text.split('\n') {
        sender.send_message(line);
    }
}

fn perm(sender: &CommandSender, sub: &str) -> bool {
    let ok = sender.has_permission(&format!("jobs.command.{sub}")) || sender.has_permission("jobs.command.*") || sender.has_permission("jobs.admin");
    if !ok {
        tell(sender, &settings::get().msg("general.error.permission", &[]));
    }
    ok
}

fn online(name: &str) -> Option<Player> {
    horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name))
}

fn uuid(p: &Player) -> String {
    p.get_unique_id().to_ascii_lowercase()
}

fn jobname(s: &settings::Settings, name: &str) -> String {
    s.job(name).map(|j| format!("{}{}", settings::chat_color(&j.color), j.name)).unwrap_or_else(|| name.to_owned())
}

pub fn do_join(player: &Player, name: &str) {
    let s = settings::get();
    let jn = jobname(&s, name);
    match model::join_job(&uuid(player), name, false) {
        Ok(_) => player.send_message(&s.msg("command.join.success", &[("jobname", &jn)])),
        Err(JoinError::Unknown) => player.send_message(&s.msg("general.error.job", &[])),
        Err(JoinError::Already) => player.send_message(&s.msg("command.join.error.alreadyin", &[("jobname", &jn)])),
        Err(JoinError::MaxJobs) => player.send_message(&s.msg("command.join.error.maxjobs", &[])),
        Err(JoinError::Full) => player.send_message(&s.msg("command.join.error.fullslots", &[("jobname", &jn)])),
        Err(JoinError::NoPerm) => player.send_message(&s.msg("general.error.permission", &[])),
        Err(JoinError::Rejoin(secs)) => player.send_message(&s.msg("command.join.error.rejoin", &[("time", &format!("{secs}s"))])),
    }
}

pub fn do_leave(player: &Player, name: &str) {
    let s = settings::get();
    match model::leave_job(&uuid(player), name) {
        Some(j) => player.send_message(&s.msg("command.leave.success", &[("jobname", &jobname(&s, &j))])),
        None => player.send_message(&s.msg("general.error.job", &[])),
    }
}

/// `/jobs info <job> [action]`: what the job pays for, at the reader's level.
pub fn show_info(sender: &CommandSender, name: &str, action: Option<&str>) {
    let s = settings::get();
    let Some(job) = s.job(name) else {
        tell(sender, &s.msg("general.error.job", &[]));
        return;
    };
    let (level, numjobs, maxjobs) = match sender.as_player() {
        Some(p) => {
            let ps = model::players();
            let jp = ps.get(&uuid(p));
            (jp.and_then(|j| j.jobs.iter().find(|x| x.job == job.name).map(|x| x.level)).unwrap_or(1), jp.map(|j| j.jobs.len()).unwrap_or(1), jp.map(|j| j.max_jobs).unwrap_or(s.max_jobs))
        }
        None => (1, 1, s.max_jobs),
    };
    tell(sender, &s.msg("command.info.help.title", &[]));
    tell(sender, &format!("{}{}", settings::chat_color(&job.color), job.display));
    if job.max_level > 0 {
        tell(sender, &s.msg("command.info.help.newMax", &[("max", &job.max_level.to_string())]));
    }
    let wanted = action.and_then(Action::parse);
    let mut names = Vec::new();
    for (act, label) in settings::ACTIONS {
        let Some(table) = job.actions.get(&act).filter(|t| !t.is_empty()) else { continue };
        names.push(label);
        if wanted.is_some_and(|w| w != act) || (wanted.is_none() && action.is_some()) {
            continue;
        }
        tell(sender, &format!("§6{label}:"));
        let mut keys: Vec<&String> = table.keys().collect();
        keys.sort();
        for k in keys.into_iter().take(if wanted.is_some() { 200 } else { 12 }) {
            let info = &table[k];
            let (m, p, e) = job.pay(info, level, numjobs, maxjobs);
            let mut line = format!("  {}", s.msg("command.info.help.material", &[("material", k)]));
            if m != 0.0 {
                line.push_str(&s.msg("command.info.help.money", &[("money", &s.money(m))]));
            }
            if p != 0.0 {
                line.push_str(&s.msg("command.info.help.points", &[("points", &s.points_fmt(p))]));
            }
            if e != 0.0 {
                line.push_str(&s.msg("command.info.help.exp", &[("exp", &s.exp_fmt(e))]));
            }
            tell(sender, &line);
        }
    }
    if action.is_none() {
        tell(sender, &s.msg("command.info.help.actions", &[("actions", &names.join(", "))]));
    }
}

fn stats(sender: &CommandSender, target: &str) {
    let s = settings::get();
    let ps = model::players();
    let Some(jp) = ps.values().find(|j| j.name.eq_ignore_ascii_case(target)) else {
        drop(ps);
        tell(sender, &s.msg("general.error.noinfoByPlayer", &[("playername", target)]));
        return;
    };
    if jp.jobs.is_empty() {
        tell(sender, &s.msg("command.stats.error.nojob", &[]));
        return;
    }
    tell(sender, &s.msg("general.info.toplineseparator", &[("playerdisplayname", &jp.name), ("playername", &jp.name)]));
    for p in &jp.jobs {
        let Some(job) = s.job(&p.job) else { continue };
        let max = jp.max_level(&s, &p.job);
        if max > 0 && p.level >= max {
            tell(sender, &s.msg("command.stats.output.maxLevel", &[("joblevel", &p.level.to_string()), ("jobname", &jobname(&s, &p.job))]));
        } else {
            tell(sender, &s.msg("command.stats.output.Level", &[("joblevel", &p.level.to_string()), ("jobname", &jobname(&s, &p.job)), ("jobxp", &s.exp_fmt(p.exp)), ("jobmaxxp", &format!("{:.0}", job.max_exp(p.level, jp.jobs.len())))]));
        }
    }
    tell(sender, &s.msg("general.info.separator", &[]));
}

fn top(sender: &CommandSender, job: Option<String>, page: usize) {
    let s = settings::get();
    let amount = s.top_amount;
    let hidden = s.hidden_top.clone();
    // Unsaved progress first, then the query off the server thread; the answer comes back to it.
    let rows = model::dirty_rows();
    let sender = sender.clone();
    std::thread::spawn(move || {
        crate::db::save(&rows);
        let list = crate::db::top(job.as_deref(), amount, (page.max(1) - 1) * amount, &hidden);
        crate::later(move || {
            let s = settings::get();
            match &job {
                Some(j) => tell(&sender, &s.msg("command.top.output.topline", &[("amount", &amount.to_string()), ("jobname", &jobname(&s, j))])),
                None => tell(&sender, &s.msg("command.gtop.output.topline", &[("amount", &amount.to_string())])),
            }
            let key = if job.is_some() { "command.top.output.list" } else { "command.gtop.output.list" };
            for (i, (name, _, level, exp)) in list.iter().enumerate() {
                tell(&sender, &s.msg(key, &[("number", &((page.max(1) - 1) * amount + i + 1).to_string()), ("playerdisplayname", name), ("playername", name), ("level", &level.to_string()), ("exp", &format!("{exp:.0}"))]));
            }
        });
    });
}

pub fn run(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    let s = settings::get();
    let sub = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let rest = if args.is_empty() { &[][..] } else { &args[1..] };
    let me = sender.as_player().cloned();
    let need_player = || {
        if me.is_none() {
            tell(sender, &s.msg("general.error.ingame", &[]));
        }
        me.clone()
    };
    match sub.as_str() {
        "" | "help" | "?" => {
            tell(sender, &s.msg("command.help.output.title", &[]));
            for c in ["browse", "join", "leave", "leaveall", "info", "stats", "top", "gtop", "points", "limit", "toggle", "shop", "archive"] {
                if sender.has_permission(&format!("jobs.command.{c}")) {
                    let args = s.lang.get(&format!("command.{c}.help.args")).cloned().unwrap_or_default();
                    let info = s.lang.get(&format!("command.{c}.help.info")).cloned().unwrap_or_default();
                    tell(sender, &horizon_kit::text::colorize(&format!("&7/{label} {c} &f{args} &f- &2{info}")));
                }
            }
        }
        "browse" => {
            if !perm(sender, "browse") {
                return true;
            }
            if let Some(p) = need_player()
                && s.gui_on_browse
            {
                crate::gui::browse(&p);
                return true;
            }
            tell(sender, &s.msg("command.browse.output.header", &[]));
            for j in &s.jobs {
                tell(sender, &s.msg("command.browse.output.console.list", &[("jobname", &jobname(&s, &j.name))]));
            }
            tell(sender, &s.msg("command.browse.output.footer", &[]));
        }
        "join" => {
            if !perm(sender, "join") {
                return true;
            }
            let Some(p) = need_player() else { return true };
            match rest.first() {
                Some(j) => do_join(&p, j),
                None => crate::gui::browse(&p),
            }
        }
        "leave" => {
            if !perm(sender, "leave") {
                return true;
            }
            let Some(p) = need_player() else { return true };
            match rest.first() {
                Some(j) => do_leave(&p, j),
                None => tell(sender, &format!("§c/{label} leave {}", s.lang.get("command.leave.help.args").cloned().unwrap_or_default())),
            }
        }
        "leaveall" => {
            if !perm(sender, "leaveall") {
                return true;
            }
            let Some(p) = need_player() else { return true };
            let jobs: Vec<String> = model::players().get(&uuid(&p)).map(|j| j.jobs.iter().map(|x| x.job.clone()).collect()).unwrap_or_default();
            if jobs.is_empty() {
                tell(sender, &s.msg("command.leaveall.error.nojobs", &[]));
                return true;
            }
            for j in jobs {
                model::leave_job(&uuid(&p), &j);
            }
            tell(sender, &s.msg("command.leaveall.success", &[]));
        }
        "info" => {
            if !perm(sender, "info") {
                return true;
            }
            match rest.first() {
                Some(j) => show_info(sender, j, rest.get(1).copied()),
                None => tell(sender, &format!("§c/{label} info {}", s.lang.get("command.info.help.args").cloned().unwrap_or_default())),
            }
        }
        "stats" => {
            if !perm(sender, "stats") {
                return true;
            }
            let target = match rest.first() {
                Some(n) => (*n).to_owned(),
                None => match need_player() {
                    Some(p) => p.get_name().to_owned(),
                    None => return true,
                },
            };
            stats(sender, &target);
        }
        "top" => {
            if !perm(sender, "top") {
                return true;
            }
            let Some(j) = rest.first().and_then(|j| s.job(j)) else {
                tell(sender, &s.msg("command.top.error.nojob", &[]));
                return true;
            };
            top(sender, Some(j.name.clone()), rest.get(1).and_then(|p| p.parse().ok()).unwrap_or(1));
        }
        "gtop" => {
            if !perm(sender, "gtop") {
                return true;
            }
            top(sender, None, rest.first().and_then(|p| p.parse().ok()).unwrap_or(1));
        }
        "points" => {
            if !perm(sender, "points") {
                return true;
            }
            let target = rest.first().map(|n| (*n).to_owned()).or_else(|| me.as_ref().map(|p| p.get_name().to_owned())).unwrap_or_default();
            let ps = model::players();
            match ps.values().find(|j| j.name.eq_ignore_ascii_case(&target)) {
                Some(jp) => {
                    tell(sender, &s.msg("command.points.currentpoints", &[("currentpoints", &s.points_fmt(jp.points))]));
                    tell(sender, &s.msg("command.points.totalpoints", &[("totalpoints", &s.points_fmt(jp.total_points))]));
                }
                None => tell(sender, &s.msg("general.error.noinfoByPlayer", &[("playername", &target)])),
            }
        }
        "limit" => {
            if !perm(sender, "limit") {
                return true;
            }
            if !s.limits.iter().any(|l| l.enabled) {
                tell(sender, &s.msg("command.limit.output.notenabled", &[]));
                return true;
            }
            let target = rest.first().map(|n| (*n).to_owned()).or_else(|| me.as_ref().map(|p| p.get_name().to_owned())).unwrap_or_default();
            let ps = model::players();
            let Some(jp) = ps.values().find(|j| j.name.eq_ignore_ascii_case(&target)) else { return true };
            let now = crate::now_ms();
            for (i, (time_key, limit_key)) in [("command.limit.output.moneytime", "command.limit.output.moneyLimit"), ("command.limit.output.exptime", "command.limit.output.expLimit"), ("command.limit.output.pointstime", "command.limit.output.pointsLimit")].iter().enumerate() {
                let l = &s.limits[i];
                if !l.enabled {
                    continue;
                }
                let left = (jp.limits[i].1 + l.seconds * 1000 - now).max(0) / 1000;
                tell(sender, &s.msg(time_key, &[("time", &format!("{}m {}s", left / 60, left % 60))]));
                let total = l.equation.eval(&[("totallevel", jp.total_level() as f64)]);
                tell(sender, &s.msg(limit_key, &[("current", &format!("{:.2}", jp.limits[i].0)), ("total", &format!("{total:.2}"))]));
            }
        }
        "toggle" => {
            if !perm(sender, "toggle") {
                return true;
            }
            let Some(p) = need_player() else { return true };
            let what = rest.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
            let mut ps = model::players();
            let Some(jp) = ps.get_mut(&uuid(&p)) else { return true };
            let (kind, on) = match what.as_str() {
                "bossbar" if s.bossbar => {
                    jp.bossbar_on = !jp.bossbar_on;
                    ("BossBar", jp.bossbar_on)
                }
                "actionbar" if s.action_bars => {
                    jp.actionbar = !jp.actionbar;
                    ("ActionBar", jp.actionbar)
                }
                _ => {
                    drop(ps);
                    tell(sender, &s.msg("command.toggle.output.turnedoff", &[]));
                    return true;
                }
            };
            jp.message_options = (jp.actionbar as i64) | ((jp.bossbar_on as i64) << 1);
            jp.dirty = true;
            drop(ps);
            tell(sender, &s.msg(if on { "command.toggle.output.Rapid" } else { "command.toggle.output.Off" }, &[("type", kind)]));
        }
        "shop" => {
            if !perm(sender, "shop") || !s.shop_enabled {
                return true;
            }
            if let Some(p) = need_player() {
                crate::gui::shop(&p);
            }
        }
        "archive" => {
            if !perm(sender, "archive") {
                return true;
            }
            let target = rest.first().map(|n| (*n).to_owned()).or_else(|| me.as_ref().map(|p| p.get_name().to_owned())).unwrap_or_default();
            let ps = model::players();
            let Some(jp) = ps.values().find(|j| j.name.eq_ignore_ascii_case(&target)) else { return true };
            if jp.archived.is_empty() {
                tell(sender, &s.msg("command.archive.error.nojob", &[]));
            }
            for (j, lv, xp, _) in &jp.archived {
                tell(sender, &format!("§e{} §7lvl §f{lv} §7exp §f{xp:.0}", jobname(&s, j)));
            }
        }
        // ---- Admin ----
        "reload" => {
            if !perm(sender, "reload") {
                return true;
            }
            crate::reload();
            tell(sender, &settings::get().msg("command.reload.output", &[]));
        }
        "employ" | "fire" | "promote" | "demote" | "grantxp" | "removexp" | "level" => {
            if !perm(sender, &sub) {
                return true;
            }
            let (Some(target), Some(job)) = (rest.first(), rest.get(1)) else {
                tell(sender, &format!("§c/{label} {sub} {}", s.lang.get(&format!("command.{sub}.help.args")).cloned().unwrap_or_default()));
                return true;
            };
            let Some(tp) = online(target) else {
                tell(sender, &s.msg("general.error.noinfoByPlayer", &[("playername", target)]));
                return true;
            };
            let Some(j) = s.job(job) else {
                tell(sender, &s.msg("general.error.job", &[]));
                return true;
            };
            let jn = jobname(&s, &j.name);
            let amount: f64 = rest.get(2).and_then(|a| a.parse().ok()).unwrap_or(1.0);
            let u = uuid(&tp);
            match sub.as_str() {
                "employ" => match model::join_job(&u, &j.name, true) {
                    Ok(_) => tp.send_message(&s.msg("command.employ.output.target", &[("jobname", &jn)])),
                    Err(_) => tell(sender, &s.msg("command.employ.error.alreadyin", &[("jobname", &jn)])),
                },
                "fire" => match model::leave_job(&u, &j.name) {
                    Some(_) => tp.send_message(&s.msg("command.fire.output.target", &[("jobname", &jn)])),
                    None => tell(sender, &s.msg("command.fire.error.nojob", &[("jobname", &jn)])),
                },
                "promote" => {
                    model::admin_change(&u, &j.name, amount as i32, 0.0, None);
                    tp.send_message(&s.msg("command.promote.output.target", &[("levelsgained", &(amount as i32).to_string()), ("jobname", &jn)]));
                }
                "demote" => {
                    model::admin_change(&u, &j.name, -(amount as i32), 0.0, None);
                    tp.send_message(&s.msg("command.demote.output.target", &[("levelslost", &(amount as i32).to_string()), ("jobname", &jn)]));
                }
                "grantxp" => {
                    model::admin_change(&u, &j.name, 0, amount, None);
                    tp.send_message(&s.msg("command.grantxp.output.target", &[("xpgained", &format!("{amount}")), ("jobname", &jn)]));
                }
                "removexp" => {
                    model::admin_change(&u, &j.name, 0, -amount, None);
                    tp.send_message(&s.msg("command.removexp.output.target", &[("xplost", &format!("{amount}")), ("jobname", &jn)]));
                }
                _ => {
                    model::admin_change(&u, &j.name, 0, 0.0, Some(amount as i32));
                }
            }
            tell(sender, &s.msg("general.admin.success", &[]));
        }
        "fireall" => {
            if !perm(sender, "fireall") {
                return true;
            }
            let Some(target) = rest.first() else { return true };
            let targets: Vec<Player> = if target.eq_ignore_ascii_case("all") { horizon_bukkit::get_online_players() } else { online(target).into_iter().collect() };
            for tp in targets {
                let jobs: Vec<String> = model::players().get(&uuid(&tp)).map(|j| j.jobs.iter().map(|x| x.job.clone()).collect()).unwrap_or_default();
                for j in jobs {
                    model::leave_job(&uuid(&tp), &j);
                }
                tp.send_message(&s.msg("command.fireall.output.target", &[]));
            }
            tell(sender, &s.msg("general.admin.success", &[]));
        }
        "transfer" => {
            if !perm(sender, "transfer") {
                return true;
            }
            let (Some(target), Some(old), Some(new)) = (rest.first(), rest.get(1), rest.get(2)) else { return true };
            let Some(tp) = online(target) else { return true };
            let u = uuid(&tp);
            let (Some(oj), Some(nj)) = (s.job(old), s.job(new)) else {
                tell(sender, &s.msg("general.error.job", &[]));
                return true;
            };
            {
                let mut ps = model::players();
                if let Some(jp) = ps.get_mut(&u)
                    && let Some(p) = jp.jobs.iter_mut().find(|p| p.job == oj.name)
                {
                    p.job = nj.name.clone();
                    jp.dirty = true;
                }
            }
            tp.send_message(&s.msg("command.transfer.output.target", &[("oldjobname", &jobname(&s, &oj.name)), ("newjobname", &jobname(&s, &nj.name))]));
        }
        "editpoints" => {
            if !perm(sender, "editpoints") {
                return true;
            }
            let (Some(op), Some(target), Some(amount)) = (rest.first(), rest.get(1), rest.get(2).and_then(|a| a.parse::<f64>().ok())) else { return true };
            let mut ps = model::players();
            let Some(jp) = ps.values_mut().find(|j| j.name.eq_ignore_ascii_case(target)) else { return true };
            let key = match op.to_ascii_lowercase().as_str() {
                "set" => {
                    jp.points = amount;
                    "command.editpoints.output.set"
                }
                "add" => {
                    jp.points += amount;
                    jp.total_points += amount;
                    "command.editpoints.output.add"
                }
                _ => {
                    jp.points -= amount;
                    "command.editpoints.output.take"
                }
            };
            jp.dirty = true;
            let (name, total) = (jp.name.clone(), jp.points);
            drop(ps);
            tell(sender, &s.msg(key, &[("playerdisplayname", &name), ("amount", &s.points_fmt(amount)), ("total", &s.points_fmt(total))]));
        }
        "boost" => {
            if !perm(sender, "boost") {
                return true;
            }
            // /jobs boost <job|all|reset> [exp/money/points/all] <rate> [time e.g. 1h]
            let Some(first) = rest.first() else { return true };
            if first.eq_ignore_ascii_case("reset") {
                model::BOOSTS.lock().unwrap_or_else(|e| e.into_inner()).clear();
                tell(sender, &s.msg("command.boost.output.allreset", &[]));
                return true;
            }
            let (job, kind, rate, time) = match rest {
                [j, k, r, t, ..] => (*j, *k, r.parse::<f64>().ok(), Some(*t)),
                [j, k, r] if r.parse::<f64>().is_ok() => (*j, *k, r.parse::<f64>().ok(), None),
                [j, r, t] => (*j, "all", r.parse::<f64>().ok(), Some(*t)),
                [j, r] => (*j, "all", r.parse::<f64>().ok(), None),
                _ => return true,
            };
            let Some(rate) = rate else { return true };
            let key = if job.eq_ignore_ascii_case("all") { "all".to_owned() } else if let Some(j) = s.job(job) { j.name.to_ascii_lowercase() } else {
                tell(sender, &s.msg("general.error.job", &[]));
                return true;
            };
            let secs = time.map(parse_time).unwrap_or(0);
            let until = if secs > 0 { crate::now_ms() + secs * 1000 } else { 0 };
            let mut b = model::BOOSTS.lock().unwrap_or_else(|e| e.into_inner());
            let e = b.entry(key.clone()).or_insert([(0.0, 0); 3]);
            for (i, k) in ["money", "exp", "points"].iter().enumerate() {
                if kind.eq_ignore_ascii_case("all") || kind.eq_ignore_ascii_case(k) {
                    e[i] = (rate - 1.0, until);
                }
            }
            drop(b);
            tell(sender, &s.msg("command.boost.output.boostadded", &[("boost", &format!("{rate}")), ("jobname", &key)]));
        }
        _ => tell(sender, &s.msg("command.help.output.cmdUsage", &[("command", label)])),
    }
    true
}

/// `1h`, `30m`, `90s`, `2d` (a bare number is seconds).
fn parse_time(t: &str) -> i64 {
    let (num, unit) = t.split_at(t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len()));
    let n: i64 = num.parse().unwrap_or(0);
    match unit {
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        _ => n,
    }
}

pub fn complete(_sender: &CommandSender, _label: &str, args: &[&str]) -> Vec<String> {
    let s = settings::get();
    let last = args.last().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let list: Vec<String> = match args.len() {
        0 | 1 => ["browse", "join", "leave", "leaveall", "info", "stats", "top", "gtop", "points", "limit", "toggle", "shop", "archive", "reload", "employ", "fire", "fireall", "promote", "demote", "grantxp", "removexp", "transfer", "editpoints", "boost", "level"].iter().map(|x| (*x).to_owned()).collect(),
        2 => match args[0].to_ascii_lowercase().as_str() {
            "join" | "leave" | "info" | "top" => s.jobs.iter().map(|j| j.name.clone()).collect(),
            "toggle" => vec!["actionbar".into(), "bossbar".into()],
            "boost" => s.jobs.iter().map(|j| j.name.clone()).chain(["all".to_owned(), "reset".to_owned()]).collect(),
            _ => horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).collect(),
        },
        3 => match args[0].to_ascii_lowercase().as_str() {
            "employ" | "fire" | "promote" | "demote" | "grantxp" | "removexp" | "transfer" | "level" => s.jobs.iter().map(|j| j.name.clone()).collect(),
            "info" => settings::ACTIONS.iter().map(|(_, n)| (*n).to_owned()).collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    list.into_iter().filter(|x| x.to_ascii_lowercase().starts_with(&last)).collect()
}
