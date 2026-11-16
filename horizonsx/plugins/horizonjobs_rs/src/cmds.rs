//! `/jobs`: the menu, and join/leave/info/top/stats; `/jobs admin ...` for staff.

use crate::{conf, data, menus, ui};
use horizon_bukkit::CommandSender;

fn job_names() -> Vec<String> {
    conf::get().jobs.iter().map(|j| j.id.clone()).collect()
}

fn help(sender: &CommandSender) {
    let lines = [
        "<white>/jobs</white> <#71717a>• the jobs menu",
        "<white>/jobs join <job></white> <#71717a>• start working a job",
        "<white>/jobs leave <job></white> <#71717a>• stop working a job",
        "<white>/jobs info <job></white> <#71717a>• what a job pays",
        "<white>/jobs top [job]</white> <#71717a>• the top workers",
        "<white>/jobs stats [player]</white> <#71717a>• levels in every job",
    ];
    ui::tell(sender, &lines.join("\n"));
    if sender.has_permission("horizonjobs.admin") {
        ui::tell(sender, "<white>/jobs admin</white> <#71717a>• setlevel, addexp, reset, reload");
    }
}

/// The same command line from the same sender within 250 ms (an engine's double dispatch) runs once.
fn repeated(key: &str) -> bool {
    static LAST: std::sync::Mutex<Option<(String, u64)>> = std::sync::Mutex::new(None);
    let now = horizon_kit::time::now_ms();
    let mut held = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let again = held.as_ref().is_some_and(|(k, at)| k == key && now.saturating_sub(*at) < 250);
    *held = Some((key.to_owned(), now));
    again
}

pub fn run(sender: &CommandSender, args: &[&str]) -> bool {
    if repeated(&format!("{}\u{0}{}", sender.get_name(), args.join(" "))) {
        return true;
    }
    let sub = args.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    if sub == "admin" {
        admin(sender, &args[1..]);
        return true;
    }
    if sub == "help" || sub == "?" {
        help(sender);
        return true;
    }
    let Some(p) = sender.as_player().cloned() else {
        if sub.is_empty() {
            help(sender);
        } else {
            ui::tell(sender, "<#f87171>Players only (staff: /jobs admin).");
        }
        return true;
    };
    let job_arg = args.get(1).copied();
    let need_job = |what: &str| -> Option<String> {
        match job_arg.and_then(|a| conf::get().job(a).map(|j| j.id.clone())) {
            Some(id) => Some(id),
            None => {
                ui::say(&p, &format!("<#f87171>Which job?</#f87171> /jobs {what} <{}>", job_names().join("|")));
                None
            }
        }
    };
    match sub.as_str() {
        "" | "browse" | "menu" | "gui" | "list" => menus::open(&p),
        "join" => {
            if let Some(id) = need_job("join") {
                menus::join(&p, &id, false);
            }
        }
        "leave" | "quit" => {
            if let Some(id) = need_job("leave") {
                menus::leave(&p, &id);
            }
        }
        "info" | "pay" | "payouts" => {
            if let Some(id) = need_job("info") {
                menus::payouts(&p, &id, 0);
            }
        }
        "top" | "leaderboard" => {
            let s = conf::get();
            let which = job_arg.and_then(|a| s.job(a)).and_then(|j| s.jobs.iter().position(|x| x.id == j.id)).unwrap_or(0);
            menus::top(&p, which);
        }
        "stats" | "stat" | "level" => {
            if !p.has_permission("horizonjobs.stats") {
                ui::deny(&p, "You don't have permission for that (horizonjobs.stats).");
            } else if let Some(n) = job_arg {
                if !p.has_permission("horizonjobs.stats.others") {
                    ui::deny(&p, "You don't have permission for that (horizonjobs.stats.others).");
                } else {
                    match data::uuid_of(n) {
                        Some(t) => menus::stats(&p, Some(t)),
                        None => ui::say(&p, "<#f87171>No player by that name has worked here."),
                    }
                }
            } else {
                menus::stats(&p, None);
            }
        }
        other => match conf::get().job(other) {
            // `/jobs miner`: that job's payouts.
            Some(j) => menus::payouts(&p, &j.id.clone(), 0),
            None => help(sender),
        },
    }
    true
}

fn admin(sender: &CommandSender, args: &[&str]) {
    if !sender.has_permission("horizonjobs.admin") {
        ui::tell(sender, "<#f87171>You don't have permission for that (horizonjobs.admin).");
        return;
    }
    let sub = args.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    if sub == "reload" {
        match conf::reload() {
            Ok(n) => ui::tell(sender, &format!("<#4ade80>Reloaded</#4ade80> config.yml: {n} jobs.")),
            Err(e) => ui::tell(sender, &format!("<#f87171>Not reloaded:</#f87171> {e}")),
        }
        return;
    }
    let usage = "<white>/jobs admin setlevel <player> <job> <level></white> <#71717a>•</#71717a> <white>addexp <player> <job> <exp></white> <#71717a>•</#71717a> <white>join|leave <player> <job></white> <#71717a>•</#71717a> <white>reset <player> [job]</white> <#71717a>•</#71717a> <white>reload</white>";
    let Some(target) = args.get(1) else {
        ui::tell(sender, usage);
        return;
    };
    let Some((uuid, name)) = data::uuid_of(target) else {
        ui::tell(sender, "<#f87171>No player by that name has played here.");
        return;
    };
    let s = conf::get();
    let job = args.get(2).and_then(|a| s.job(a)).cloned();
    let online = horizon_bukkit::fam_ess::get_player_by_id(&uuid);
    let done = |d: &data::Profile| {
        data::summarize(&uuid, d);
        if online.is_none() {
            data::players().unload(&uuid);
        }
    };
    match (sub.as_str(), job) {
        ("setlevel", Some(j)) => {
            let Some(level) = args.get(3).and_then(|a| a.parse::<u32>().ok()) else { return ui::tell(sender, usage) };
            let level = level.clamp(1, j.max_level);
            let d = data::edit(&uuid, |d| {
                let pr = d.progress.entry(j.id.clone()).or_default();
                pr.level = level;
                pr.exp = 0.0;
                d.clone()
            });
            done(&d);
            ui::tell(sender, &format!("{name} is now <{}>{} Lvl [{level}]</{}>.", j.color, j.name, j.color));
        }
        ("addexp", Some(j)) => {
            let Some(amount) = args.get(3).and_then(|a| a.parse::<f64>().ok()) else { return ui::tell(sender, usage) };
            let d = data::edit(&uuid, |d| {
                let pr = d.progress.entry(j.id.clone()).or_insert_with(|| data::Prog { level: 1, ..Default::default() });
                pr.level = pr.level.max(1);
                pr.exp = (pr.exp + amount).max(0.0);
                pr.total_exp += amount.max(0.0);
                while pr.level < j.max_level && pr.exp >= j.needed(pr.level) {
                    pr.exp -= j.needed(pr.level);
                    pr.level += 1;
                }
                d.clone()
            });
            done(&d);
            let pr = d.prog(&j.id);
            ui::tell(sender, &format!("Gave {name} {} exp: <{}>{} Lvl [{}]</{}>, {} / {}.", ui::exp(amount), j.color, j.name, pr.level(), j.color, ui::exp(pr.exp), ui::exp(j.needed(pr.level()))));
        }
        ("join", Some(j)) => {
            let d = data::edit(&uuid, |d| {
                if !d.works(&j.id) {
                    d.jobs.push(j.id.clone());
                }
                d.progress.entry(j.id.clone()).or_insert_with(|| data::Prog { level: 1, ..Default::default() });
                d.clone()
            });
            done(&d);
            ui::tell(sender, &format!("{name} now works as a {}.", j.name));
        }
        ("leave", Some(j)) => {
            let d = data::edit(&uuid, |d| {
                d.jobs.retain(|x| x != &j.id);
                d.clone()
            });
            done(&d);
            ui::tell(sender, &format!("{name} no longer works as a {}.", j.name));
        }
        ("reset", j) => {
            let d = data::edit(&uuid, |d| {
                match &j {
                    Some(j) => {
                        d.progress.remove(&j.id);
                        d.jobs.retain(|x| x != &j.id);
                    }
                    None => {
                        d.progress.clear();
                        d.jobs.clear();
                    }
                }
                d.left_at = 0;
                d.clone()
            });
            done(&d);
            ui::tell(sender, &format!("Reset {name}'s {}.", j.map(|j| format!("{} job", j.name)).unwrap_or_else(|| "jobs".into())));
        }
        _ => ui::tell(sender, usage),
    }
}

pub fn complete(sender: &CommandSender, args: &[&str]) -> Vec<String> {
    let pick = |opts: Vec<String>, typed: &str| -> Vec<String> {
        let t = typed.to_ascii_lowercase();
        opts.into_iter().filter(|o| o.to_ascii_lowercase().starts_with(&t)).collect()
    };
    let players = || horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).collect::<Vec<_>>();
    let mut subs: Vec<String> = ["join", "leave", "info", "top", "stats", "help"].iter().map(|s| (*s).to_owned()).collect();
    if sender.has_permission("horizonjobs.admin") {
        subs.push("admin".into());
    }
    match args.len() {
        0 | 1 => pick(subs, args.first().copied().unwrap_or("")),
        2 => match args[0].to_ascii_lowercase().as_str() {
            "join" | "leave" | "info" | "top" => pick(job_names(), args[1]),
            "stats" => pick(players(), args[1]),
            "admin" if sender.has_permission("horizonjobs.admin") => pick(["setlevel", "addexp", "join", "leave", "reset", "reload"].iter().map(|s| (*s).to_owned()).collect(), args[1]),
            _ => Vec::new(),
        },
        3 if args[0].eq_ignore_ascii_case("admin") => pick(players(), args[2]),
        4 if args[0].eq_ignore_ascii_case("admin") => pick(job_names(), args[3]),
        _ => Vec::new(),
    }
}
