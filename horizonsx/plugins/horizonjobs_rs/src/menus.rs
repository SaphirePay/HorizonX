//! The menus: `/jobs` (the five job cards), a job's payouts, and the top workers of each job.
//!
//! A job card's tooltip is the server's job card: the job's colour, "PLAYER JOB", what the job is, the level,
//! exp to the next level with the percentage, how many others work it, and "[CLICK HERE TO JOIN THIS JOB]".

use crate::conf::{self, Job, Work};
use crate::{data, ui, work};
use horizon_bukkit::{ItemStack, Player};
use horizon_kit::gui;
use horizon_kit::hooks::economy;

fn has(plugin: &str) -> bool {
    horizon_bukkit::fam_core::get_plugin_manager().is_plugin_enabled(plugin)
}

fn run(p: &Player, cmd: &str) {
    p.close_inventory();
    let (p, cmd) = (p.clone(), cmd.to_owned());
    horizon_kit::app::later(1, move || {
        p.perform_command(&cmd);
    });
}

/// The card's lines about a job's progress (level, exp, others).
fn progress_lines(job: &Job, pr: &data::Prog, others: usize) -> Vec<String> {
    let c = &job.color;
    let level = pr.level();
    let (need, pct) = if level >= job.max_level { (job.needed(level), 100.0) } else { let n = job.needed(level); (n, (pr.exp / n * 100.0).clamp(0.0, 100.0)) };
    let mut out = vec![
        format!(" <{c}>| Level: <{}>{} Lvl <white>[{level}]", ui::BODY, job.name),
        format!(" <{c}>| Experience: <#55ff55>{} <{c}>/ <#ff5555>{} <{}>({:.1}%)", ui::exp(pr.exp), ui::exp(need), ui::GREY, pct),
        format!(" <{c}>| {} <{}>Other {}", ui::num(others as i64), ui::BODY, job.plural),
    ];
    if level >= job.max_level {
        out[1] = format!(" <{c}>| Experience: <#55ff55>MAX LEVEL");
    }
    out
}

/// A job's card: the item and tooltip of the jobs menu.
pub fn card(p: &Player, prof: &data::Profile, job: &Job) -> ItemStack {
    let c = &job.color;
    let working = prof.works(&job.id);
    let mut lore = vec![format!("<{}>{}", ui::DIM, ui::small("Player Job")), String::new()];
    lore.extend(job.description.iter().map(|l| format!("<{}>{l}", ui::BODY)));
    lore.push(String::new());
    lore.extend(progress_lines(job, &prof.prog(&job.id), data::workers(&job.id, &p.get_unique_id())));
    lore.push(String::new());
    let what = if working { "leave this job" } else { "join this job" };
    lore.push(format!("<{c}>[{} <{}>{}<{c}>]", ui::small("click here to"), job.light, ui::small(what)));
    lore.push(format!("<{}>{}", ui::DIM, ui::small("right-click to see what it pays")));
    let mut item = ui::icon(&job.icon, &format!("<{c}><bold>{} Job", job.name), &lore);
    if working {
        ui::glint(&mut item);
    }
    item
}

/// Left: join or leave. Right: payouts.
fn card_click(c: &gui::Ctx, job_id: &str, back: fn(&Player)) {
    ui::click(&c.player);
    if c.click.is_right() {
        payouts(&c.player, job_id, 0);
        return;
    }
    let works = data::get(&c.player.get_unique_id()).works(job_id);
    let done = if works { leave(&c.player, job_id) } else { join(&c.player, job_id, true) };
    if done {
        back(&c.player);
    }
}

pub fn open(p: &Player) {
    if !p.has_permission("horizonjobs.use") {
        ui::deny(p, "You don't have permission for that (horizonjobs.use).");
        return;
    }
    let s = conf::get();
    let uuid = p.get_unique_id();
    let prof = data::get(&uuid);
    let max = work::max_jobs(p);
    let mut m = ui::frame(5, "Jobs");

    // Who they are and what they work.
    let working: Vec<String> = prof.jobs.iter().filter_map(|id| s.job(id)).map(|j| format!("<{}>{}", j.color, j.name)).collect();
    let total_level: u32 = s.jobs.iter().map(|j| prof.prog(&j.id).level()).sum();
    let earned: f64 = prof.progress.values().map(|p| p.earned).sum();
    let mut head = ui::head(p.get_name(), &format!("<gradient:{}:{}><bold>Your Jobs</bold></gradient>", ui::ACCENT, ui::ACCENT2), &[
        format!("<{}>{}", ui::DIM, ui::small("Player Profile")),
        String::new(),
        format!(" <{}>| Working as: {}", ui::GREY, if working.is_empty() { format!("<{}>nothing yet", ui::DIM) } else { working.join("<#52525b>, ") }),
        format!(" <{}>| Jobs: <white>{} <{}>/ <white>{max}", ui::GREY, prof.jobs.len(), ui::DIM),
        format!(" <{}>| Total level: <white>{total_level}", ui::GREY),
        format!(" <{}>| Earned from jobs: <#55ff55>{}", ui::GREY, ui::money(earned)),
        String::new(),
        format!("<{}>{}", ui::DIM, ui::small("click for your stats")),
    ]);
    ui::glint(&mut head);
    m.button(4, head, |c| {
        ui::click(&c.player);
        c.close();
        stats(&c.player, None);
    });

    for (col, job) in gui::centered(s.jobs.len()).into_iter().zip(s.jobs.iter()) {
        let id = job.id.clone();
        m.button(18 + col, card(p, &prof, job), move |c| card_click(c, &id, open));
        // The pane under each card: working it or not.
        let pane = if prof.works(&job.id) {
            let pr = prof.prog(&job.id);
            let pct = (pr.exp / job.needed(pr.level()) * 100.0).clamp(0.0, 100.0);
            ui::icon("LIME_STAINED_GLASS_PANE", &format!("<#55ff55>● <{}>Working as a {}", ui::BODY, job.name), &[format!("{} <{}>{pct:.1}%", ui::meter(pct / 100.0, 10, &job.color, "#3f3f46"), ui::GREY)])
        } else {
            ui::icon("GRAY_STAINED_GLASS_PANE", &format!("<{}>● Not working as a {}", ui::DIM, job.name), &[])
        };
        let id2 = job.id.clone();
        m.button(27 + col, pane, move |c| {
            ui::click(&c.player);
            payouts(&c.player, &id2, 0);
        });
    }

    m.button(38, ui::icon("GOLD_INGOT", "<#facc15><bold>Top Workers", &[format!("<{}>{}", ui::DIM, ui::small("leaderboards")), String::new(), format!("<{}>The highest level player", ui::BODY), format!("<{}>of every job.", ui::BODY), String::new(), format!("<{}>{}", ui::DIM, ui::small("click to view"))]), |c| {
        ui::click(&c.player);
        top(&c.player, 0);
    });
    let keep = if s.keep_progress { "Leaving a job keeps your level." } else { "Leaving a job resets its level." };
    m.item(42, ui::icon("BOOK", "<#22d3ee><bold>How Jobs Work", &[
        format!("<{}>{}", ui::DIM, ui::small("information")),
        String::new(),
        format!("<{}>Join a job, then do its work", ui::BODY),
        format!("<{}>to earn money and experience.", ui::BODY),
        format!("<{}>Every level pays <#55ff55>{:.0}%</#55ff55> more,", ui::BODY, s.income_per_level * 100.0),
        format!("<{}>and a level up pays a bonus.", ui::BODY),
        String::new(),
        format!("<{}>You can work <white>{max}</white> job{} at once.", ui::BODY, if max == 1 { "" } else { "s" }),
        format!("<{}>{keep}", ui::BODY),
        String::new(),
        format!("<{}>{}", ui::DIM, ui::small("right-click a job to see its pay")),
    ]));
    if has("HorizonSMP") {
        m.button(36, ui::icon("ARROW", "<#a1a1aa>← <white>Back", &[format!("<{}>To the server menu", ui::DIM)]), |c| {
            ui::click(&c.player);
            run(&c.player, "menu");
        });
    }
    ui::close_button(&mut m);
    m.open(p);
}

// ---- Joining and leaving ----

/// Joins `job_id`; at the job limit with one job, offers to switch. True when they joined now.
pub fn join(p: &Player, job_id: &str, from_menu: bool) -> bool {
    let s = conf::get();
    let Some(job) = s.job(job_id).cloned() else {
        ui::say(p, "<#f87171>There's no job by that name.");
        return false;
    };
    if !p.has_permission("horizonjobs.join") || !p.has_permission(&format!("horizonjobs.join.{}", job.id)) {
        ui::deny(p, &format!("You can't join {} (horizonjobs.join.{}).", job.name, job.id));
        return false;
    }
    let uuid = p.get_unique_id();
    let prof = data::get(&uuid);
    if prof.works(&job.id) {
        ui::say(p, &format!("You already work as a <{}>{}</{}>.", job.color, job.name, job.color));
        return false;
    }
    let left_ago = horizon_kit::time::now_ms().saturating_sub(prof.left_at);
    if s.join_cooldown_ms > 0 && left_ago < s.join_cooldown_ms && !p.has_permission("horizonjobs.admin") {
        let secs = (s.join_cooldown_ms - left_ago).div_ceil(1000);
        ui::deny(p, &format!("You left a job recently: join another in {}.", horizon_kit::time::format_short(secs * 1000)));
        return false;
    }
    let max = work::max_jobs(p);
    if prof.jobs.len() >= max {
        if max == 1 && from_menu {
            if let Some(cur) = prof.jobs.first().and_then(|id| s.job(id)).cloned() {
                let (cur_id, new_id) = (cur.id.clone(), job.id.clone());
                gui::confirm(p, &format!("Switch from {} to {}?", cur.name, job.name), move |p| {
                    if leave_quiet(p, &cur_id) {
                        // Switching is one move: the cooldown starts after it, not before.
                        do_join(p, &new_id);
                        data::edit(&p.get_unique_id(), |d| d.left_at = horizon_kit::time::now_ms());
                    }
                    open(p);
                });
                return false;
            }
        }
        ui::deny(p, &format!("You already have {max} job{}: leave one first.", if max == 1 { "" } else { "s" }));
        return false;
    }
    do_join(p, &job.id);
    true
}

fn do_join(p: &Player, job_id: &str) {
    let s = conf::get();
    let Some(job) = s.job(job_id) else { return };
    let uuid = p.get_unique_id();
    let name = p.get_name().to_owned();
    let keep = s.keep_progress;
    let d = data::edit(&uuid, |d| {
        d.name = name;
        d.jobs.push(job.id.clone());
        if !keep {
            d.progress.remove(&job.id);
        }
        d.progress.entry(job.id.clone()).or_insert_with(|| data::Prog { level: 1, ..Default::default() });
        d.clone()
    });
    data::summarize(&uuid, &d);
    let c = &job.color;
    ui::title(p, &format!("<{c}><bold>{}", ui::small(&format!("{} job", job.name))), &format!("<{}>You now work as a <{c}>{}", ui::BODY, job.name));
    ui::good(p);
    ui::say(p, &format!("You joined the <{c}>{}</{c}> job. <{}>Right-click it in /jobs to see what pays.", job.name, ui::DIM));
}

/// Leaves without the menu or messages (switching). True when they were in it.
fn leave_quiet(p: &Player, job_id: &str) -> bool {
    let s = conf::get();
    let keep = s.keep_progress;
    let uuid = p.get_unique_id();
    let (was, d) = data::edit(&uuid, |d| {
        let was = d.works(job_id);
        d.jobs.retain(|j| j != job_id);
        if was {
            d.left_at = horizon_kit::time::now_ms();
            if !keep {
                d.progress.remove(job_id);
            }
        }
        (was, d.clone())
    });
    data::summarize(&uuid, &d);
    was
}

pub fn leave(p: &Player, job_id: &str) -> bool {
    let s = conf::get();
    let Some(job) = s.job(job_id).cloned() else {
        ui::say(p, "<#f87171>There's no job by that name.");
        return false;
    };
    if !p.has_permission("horizonjobs.join") {
        ui::deny(p, "You can't leave jobs (horizonjobs.join).");
        return false;
    }
    if !data::get(&p.get_unique_id()).works(&job.id) {
        ui::say(p, &format!("You don't work as a <{}>{}</{}>.", job.color, job.name, job.color));
        return false;
    }
    let finish = move |p: &Player, job: &Job| {
        leave_quiet(p, &job.id);
        ui::bad(p);
        let kept = if conf::get().keep_progress { format!(" <{}>Your level is kept if you come back.", ui::DIM) } else { String::new() };
        ui::say(p, &format!("You left the <{}>{}</{}> job.{kept}", job.color, job.name, job.color));
    };
    if s.keep_progress {
        finish(p, &job);
        true
    } else {
        let j = job.clone();
        gui::confirm(p, &format!("Leave {}? Its level resets.", job.name), move |p| {
            finish(p, &j);
            open(p);
        });
        false
    }
}

// ---- Payouts ----

/// The item a payout line shows: the block or item itself, a spawn egg for mobs.
fn thing_icon(work: Work, thing: &str) -> String {
    let fixed = match thing {
        "carrots" => "carrot",
        "potatoes" => "potato",
        "beetroots" => "beetroot",
        "cocoa" => "cocoa_beans",
        "sweet_berry_bush" => "sweet_berries",
        "torchflower_crop" => "torchflower",
        "pitcher_crop" => "pitcher_plant",
        "bamboo" if work == Work::Break => "bamboo",
        other => other,
    };
    let candidate = match work {
        Work::Kill | Work::Breed => format!("{fixed}_spawn_egg"),
        _ => fixed.to_owned(),
    };
    if ui::material_ok(&candidate) {
        candidate
    } else if ui::material_ok(fixed) {
        fixed.to_owned()
    } else if matches!(work, Work::Kill) {
        "bone".into()
    } else {
        "paper".into()
    }
}

pub fn payouts(p: &Player, job_id: &str, page: usize) {
    let s = conf::get();
    let Some(job) = s.job(job_id).cloned() else { return };
    let uuid = p.get_unique_id();
    let prof = data::get(&uuid);
    let level = prof.prog(&job.id).level();
    let mult = work::level_mult(level) * work::boost(p.get_name());
    let c = job.color.clone();
    let mut m = ui::frame(6, &format!("{} Job • Payouts", job.name));
    // The card joins or leaves here too, then shows this page again.
    let id = job.id.clone();
    m.button(4, card(p, &prof, &job), move |ctx| {
        ui::click(&ctx.player);
        if ctx.click.is_right() {
            return;
        }
        let works = data::get(&ctx.player.get_unique_id()).works(&id);
        let done = if works { leave(&ctx.player, &id) } else { join(&ctx.player, &id, true) };
        if done {
            payouts(&ctx.player, &id, page);
        }
    });

    let content: Vec<usize> = (9..45).filter(|s| (1..8).contains(&(s % 9))).collect();
    let per = content.len();
    let pages = job.pays.len().div_ceil(per).max(1);
    let page = page.min(pages - 1);
    for (slot, pay) in content.iter().zip(job.pays.iter().skip(page * per).take(per)) {
        let first = &pay.things[0];
        let mut lore = vec![format!("<{}>{}", ui::DIM, ui::small(&format!("{} to earn", pay.work.verb())))];
        if pay.things.len() > 1 {
            let also: Vec<String> = pay.things[1..].iter().take(4).map(|t| ui::pretty(t)).collect();
            let more = pay.things.len().saturating_sub(5);
            lore.push(format!("<{}>Also: {}{}", ui::GREY, also.join(", "), if more > 0 { format!(" +{more} more") } else { String::new() }));
        }
        lore.push(String::new());
        lore.push(format!(" <{c}>| Money: <#55ff55>{}", ui::money(pay.money)));
        lore.push(format!(" <{c}>| Experience: <#55ffff>+{}", ui::exp(pay.exp)));
        if (mult - 1.0).abs() > 1e-6 && pay.money > 0.0 {
            lore.push(format!(" <{c}>| At your level: <#55ff55>{}", ui::money(pay.money * mult)));
        }
        if pay.work == Work::Break && work::ripe_age(first).is_some() {
            lore.push(String::new());
            lore.push(format!("<{}>Only when fully grown.", ui::DIM));
        }
        if pay.work == Work::Kill {
            lore.push(String::new());
            lore.push(format!("<{}>Spawner mobs pay {:.0}%.", ui::DIM, s.spawner_multiplier * 100.0));
        }
        m.item(*slot, ui::icon(&thing_icon(pay.work, first), &format!("<{c}>{}", ui::pretty(first)), &lore));
    }
    if page > 0 {
        let id = job.id.clone();
        m.button(48, ui::icon("ARROW", "<white>← Previous page", &[format!("<{}>Page {page} of {pages}", ui::DIM)]), move |c| {
            ui::click(&c.player);
            payouts(&c.player, &id, page - 1);
        });
    }
    if page + 1 < pages {
        let id = job.id.clone();
        m.button(50, ui::icon("ARROW", "<white>Next page →", &[format!("<{}>Page {} of {pages}", ui::DIM, page + 2)]), move |c| {
            ui::click(&c.player);
            payouts(&c.player, &id, page + 1);
        });
    }
    ui::back_button(&mut m, "the jobs", open);
    ui::close_button(&mut m);
    m.open(p);
}

// ---- Top workers ----

pub fn top(p: &Player, which: usize) {
    if !p.has_permission("horizonjobs.top") {
        ui::deny(p, "You don't have permission for that (horizonjobs.top).");
        return;
    }
    let s = conf::get();
    let which = which.min(s.jobs.len() - 1);
    let job = s.jobs[which].clone();
    let mut rows: Vec<(String, bool, u32, f64)> = data::summaries().read(|m| {
        m.values().filter_map(|r| r.levels.get(&job.id).map(|(l, e)| (r.name.clone(), r.jobs.contains(&job.id), (*l).max(1), *e))).filter(|(_, _, l, e)| *l > 1 || *e > 0.0).collect()
    });
    rows.sort_by(|a, b| b.2.cmp(&a.2).then(b.3.total_cmp(&a.3)));
    let mut m = ui::frame(6, &format!("Top Workers • {}", job.name));
    for (i, (col, j)) in gui::centered(s.jobs.len()).into_iter().zip(s.jobs.iter()).enumerate() {
        let on = i == which;
        let mut icon = ui::icon(&j.icon, &format!("<{}><bold>{}", if on { j.color.as_str() } else { ui::GREY }, j.plural), &[String::new(), format!("<{}>{}", ui::DIM, ui::small(if on { "showing" } else { "click to show" }))]);
        if on {
            ui::glint(&mut icon);
        }
        m.button(col, icon, move |c| {
            ui::click(&c.player);
            top(&c.player, i);
        });
    }
    let c = &job.color;
    let slots: [usize; 17] = [13, 21, 23, 28, 29, 30, 31, 32, 33, 34, 37, 38, 39, 40, 41, 42, 43];
    for (rank, (slot, (name, working, level, exp))) in slots.iter().zip(rows.iter()).enumerate() {
        let medal = match rank {
            0 => "#facc15",
            1 => "#e4e4e7",
            2 => "#fb923c",
            _ => ui::GREY,
        };
        let mut item = ui::head(name, &format!("<{medal}><bold>#{} <white>{name}", rank + 1), &[
            format!("<{}>{}", ui::DIM, ui::small(&format!("{} {}", job.name, if *working { "working" } else { "retired" }))),
            String::new(),
            format!(" <{c}>| Level: <{}>{} Lvl <white>[{level}]", ui::BODY, job.name),
            format!(" <{c}>| Experience: <#55ff55>{} <{c}>/ <#ff5555>{}", ui::exp(*exp), ui::exp(job.needed(*level))),
        ]);
        item.set_amount((rank + 1) as i32);
        m.item(*slot, item);
    }
    if rows.is_empty() {
        m.item(31, ui::icon("STRUCTURE_VOID", &format!("<{}>Nobody yet", ui::GREY), &[format!("<{}>Be the first {} to level up!", ui::BODY, job.name)]));
    }
    ui::back_button(&mut m, "the jobs", open);
    ui::close_button(&mut m);
    m.open(p);
}

// ---- Stats (chat) ----

pub fn stats(p: &Player, target: Option<(String, String)>) {
    let s = conf::get();
    let (uuid, name) = target.unwrap_or_else(|| (p.get_unique_id(), p.get_name().to_owned()));
    let online = horizon_bukkit::fam_ess::get_player_by_id(&uuid).is_some();
    let prof = if online {
        data::get(&uuid)
    } else {
        data::summaries().read(|m| m.get(&uuid).cloned()).map(|r| data::Profile {
            name: r.name,
            jobs: r.jobs,
            progress: r.levels.into_iter().map(|(k, (l, e))| (k, data::Prog { level: l, exp: e, ..Default::default() })).collect(),
            left_at: 0,
        }).unwrap_or_default()
    };
    let mut lines = vec![String::new(), format!(" <gradient:{}:{}><bold>{}</bold></gradient> <{}>{}", ui::ACCENT, ui::ACCENT2, name, ui::DIM, ui::small("job stats"))];
    for job in &s.jobs {
        let pr = prof.prog(&job.id);
        let need = job.needed(pr.level());
        let pct = if pr.level() >= job.max_level { 100.0 } else { (pr.exp / need * 100.0).clamp(0.0, 100.0) };
        let mark = if prof.works(&job.id) { format!("<#55ff55>●") } else { format!("<#3f3f46>●") };
        lines.push(format!(" {mark} <{}>{:<11}<white>Lvl [{}] {} <{}>{pct:.1}%", job.color, job.name, pr.level(), ui::meter(pct / 100.0, 12, &job.color, "#3f3f46"), ui::GREY));
    }
    if online && p.get_unique_id() == uuid {
        lines.push(format!(" <{}>Balance: <#55ff55>{}", ui::GREY, ui::money(economy::balance(&name))));
    }
    lines.push(String::new());
    ui::raw(p, &lines.join("\n"));
}
