//! VotingPlugin (BenCodez, CC BY 3.0) written again natively in Rust for Horizon, with a Votifier receiver built
//! in (NuVotifier protocol v1 RSA and v2 tokens, on its own thread and port). Same plugin name, commands, aliases,
//! Config.yml / VoteSites.yml / SpecialRewards.yml / Shop.yml / Rewards formats and SQLite user table (Users.db).
//!
//! Votes are counted off the server thread when the voter is offline (kept for their next join); rewards run
//! on the server thread. `%votingplugin_...%` placeholders are offered.

pub mod commands;
pub mod rewards;
pub mod settings;
pub mod time;
pub mod users;
pub mod votes;
pub mod votifier;

use horizon_bukkit::event::{PlayerJoinEvent, PlayerQuitEvent};
use horizon_bukkit::fam_ev_player::{AsyncPlayerPreLoginEvent, InventoryClickEvent, InventoryCloseEvent};
use horizon_bukkit::{Plugin, bukkit_plugin, enums};
use std::sync::Mutex;

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);

pub fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn folder() -> std::path::PathBuf {
    plugin().map(|p| p.get_data_folder()).unwrap_or_default()
}

pub fn log(text: &str) {
    if let Some(p) = plugin() {
        p.info(text);
    }
}

pub fn reload() {
    settings::load(&folder());
}

/// Sites `player` can vote on now (enabled, not hidden).
fn sites_open(u: &users::User) -> usize {
    let s = settings::get();
    s.enabled_sites().filter(|x| !x.hidden && u.wait(x) <= 0).count()
}

/// The reminders of one type (LOGIN, INTERVAL, FIRST_JOIN) a player qualifies for, the highest priority first.
fn remind(player: &horizon_bukkit::Player, kind: &str) {
    let s = settings::get();
    if !s.cfg.bool("VoteReminderOptions.Enabled", true) || player.has_permission("VotingPlugin.NoRemind") {
        return;
    }
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let Some(u) = users::users().get(&uuid).cloned() else { return };
    let open = sites_open(&u);
    let total = s.enabled_sites().filter(|x| !x.hidden).count();
    let now = time::now_ms();
    let Some(map) = s.reminders.as_mapping() else { return };
    let mut list: Vec<(&serde_yaml::Value, i64)> = map.iter().filter(|(_, r)| r.get("Type").and_then(|x| x.as_str()).is_some_and(|t| t.eq_ignore_ascii_case(kind))).map(|(_, r)| (r, r.get("Priority").and_then(|x| x.as_i64()).unwrap_or(0))).collect();
    list.sort_by(|a, b| b.1.cmp(&a.1));
    for (r, _) in list {
        let cond = r.get("Conditions");
        if cond.and_then(|c| c.get("CanVoteAny")).and_then(|x| x.as_bool()).unwrap_or(false) && open == 0 {
            continue;
        }
        if cond.and_then(|c| c.get("CanVoteAll")).and_then(|x| x.as_bool()).unwrap_or(false) && open < total {
            continue;
        }
        let cooldown = settings::duration_ms(r.get("Cooldown"), 1000);
        if cooldown > 0 && now - u.reminded < cooldown {
            continue;
        }
        if let Some(rw) = r.get("Rewards") {
            rewards::give(player, rw, &[("sitesavailable", &open.to_string())]);
        }
        if let Some(x) = users::users().get_mut(&uuid) {
            x.reminded = now;
            x.dirty = true;
        }
        if s.cfg.bool("VoteReminderOptions.StopAfterMatch", true) {
            break;
        }
    }
}

fn placeholders(uuid: &str, _name: &str, params: &str) -> Option<String> {
    let p = params.to_ascii_lowercase();
    let u = users::users().get(&uuid.to_ascii_lowercase()).cloned();
    let s = settings::get();
    // Top_<Type>_<n> (a name) and Top_<Type>Votes_<n> (the count).
    if let Some(rest) = p.strip_prefix("top_") {
        let (kind, n) = rest.rsplit_once('_')?;
        let tops = users::tops();
        if n == "position" {
            let list = tops.of(kind);
            return Some(list.iter().position(|r| r.1 == uuid.to_ascii_lowercase()).map(|i| (i + 1).to_string()).unwrap_or_default());
        }
        let n: usize = n.parse().ok()?;
        let (kind, votes) = match kind.strip_suffix("votes") {
            Some(k) => (k, true),
            None => (kind, false),
        };
        let row = tops.of(kind).get(n.checked_sub(1)?);
        return Some(match (row, votes) {
            (Some(r), true) => r.2.to_string(),
            (Some(r), false) => r.0.clone(),
            (None, _) => String::new(),
        });
    }
    let sv = users::SERVER.lock().unwrap_or_else(|e| e.into_inner());
    let required = s.party.get("VotesRequired").and_then(|x| x.as_i64()).unwrap_or(20);
    let server = match p.as_str() {
        "votepartyvotescurrent" => Some(sv.party.to_string()),
        "votepartyvotesneeded" => Some((required - sv.party).max(0).to_string()),
        "votepartyvotesrequired" => Some(required.to_string()),
        "globaldailytotal" => Some(sv.global[0].to_string()),
        "globalweeklytotal" => Some(sv.global[1].to_string()),
        "globalmonthtotal" => Some(sv.global[2].to_string()),
        "globalalltimetotal" => Some(sv.global[3].to_string()),
        "timeuntildayreset" => Some(horizon_kit::time::format(time::ms_until_next_day(time::now_ms()) as u64)),
        "timeuntilweekreset" => Some(horizon_kit::time::format(time::ms_until_next_week(time::now_ms()) as u64)),
        _ => None,
    };
    drop(sv);
    if server.is_some() {
        return server;
    }
    let u = u?;
    Some(match p.as_str() {
        "total" | "total_monthly" | "total_month" | "monthtotal" => u.month.to_string(),
        "alltimetotal" | "total_alltime" | "total_all" => u.all.to_string(),
        "total_weekly" | "total_week" | "weeklytotal" => u.week.to_string(),
        "total_daily" | "total_day" | "dailytotal" => u.day.to_string(),
        "lastmonthtotal" => u.last_month.to_string(),
        "points" => u.points.to_string(),
        "points_format" => format_thousands(u.points),
        "bestdailytotal" => u.high_day.to_string(),
        "bestweeklytotal" => u.high_week.to_string(),
        "bestmonthlytotal" => u.high_month.to_string(),
        "dailyvotestreak" => u.day_streak.to_string(),
        "weeklyvotestreak" => u.week_streak.to_string(),
        "monthvotestreak" => u.month_streak.to_string(),
        "bestdailyvotestreak" => u.best_day_streak.to_string(),
        "bestweeklyvotestreak" => u.best_week_streak.to_string(),
        "bestmonthvotestreak" => u.best_month_streak.to_string(),
        "disablebroadcast" => u.no_broadcast.to_string(),
        "canvote" => (sites_open(&u) > 0).to_string(),
        "sitesavailable" | "canvotesites" => sites_open(&u).to_string(),
        "sitesavailabletotal" => s.enabled_sites().filter(|x| !x.hidden).count().to_string(),
        "votepartycontributedvotes" => u.party_votes.to_string(),
        "next_anysite" => {
            let wait = s.enabled_sites().filter(|x| !x.hidden).map(|x| u.wait(x)).min().unwrap_or(0);
            if wait <= 0 { s.msg("Format.Commands.Vote.Next.Info.CanVote", &[]) } else { horizon_kit::time::format(wait as u64) }
        }
        other => {
            if let Some(site) = other.strip_prefix("next_") {
                let site = s.site(site)?;
                let wait = u.wait(site);
                if wait <= 0 { s.msg("Format.Commands.Vote.Next.Info.CanVote", &[]) } else { horizon_kit::time::format(wait as u64) }
            } else if let Some(site) = other.strip_prefix("canvote_") {
                (u.wait(s.site(site)?) <= 0).to_string()
            } else if let Some(site) = other.strip_prefix("last_") {
                let site = s.site(site)?;
                u.last_votes.get(&site.key).map(|t| time::date(*t)).unwrap_or_default()
            } else if let Some(kind) = other.strip_prefix("total_") {
                u.total(&kind.to_ascii_uppercase()).to_string()
            } else {
                return None;
            }
        }
    })
}

fn format_thousands(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    let started = std::time::Instant::now();
    let f = plugin.get_data_folder();
    let _ = std::fs::write(f.join("LICENSE.md"), include_str!("../LICENSE"));
    reload();
    if !users::open(&f) {
        log("Could not open Users.db; VotingPlugin is off.");
        return;
    }
    users::load_server();
    votes::start();
    votifier::start(f.clone());
    plugin.run_task_timer(1, 1, horizon_kit::sync::drain);

    for name in ["vote", "votehelp", "votetotal", "votelast", "votenext", "votetop", "votetoday", "votegui", "voteurl", "voteparty", "voteshop", "votebest", "votestreak", "votetogglebroadcast", "votepoints", "votelist", "voteall"] {
        plugin.set_executor(name, commands::vote);
        plugin.set_tab_completer(name, commands::complete);
    }
    for name in ["adminvote", "av", "adminvotehelp", "adminvotereload", "adminvoteversion", "adminvotesites", "adminvotevote", "adminvoteservicesites", "adminvotevoteparty", "adminvotereloadall", "adminvoteclearofflinevotes", "adminvotetest", "adminvoteuser", "adminvotereport", "adminvoteperms", "adminvotetestreward", "adminvoterewards", "adminvoteuuid", "adminvotevotesite", "adminvoteconfig", "adminvotegui", "adminvotegivereward", "adminvotecleartotal", "adminvotebackgroundupdate", "adminvotepermsplayer", "adminvotegiveall", "adminvotegiveallonline", "adminvoteplayername", "adminvotestatus", "adminvoteplaceholders"] {
        plugin.set_executor(name, commands::admin);
        plugin.set_tab_completer(name, commands::complete);
    }
    horizon_kit::hooks::provide_placeholders(&plugin, "votingplugin", placeholders);

    plugin.register_event(|e: &mut AsyncPlayerPreLoginEvent| {
        users::prelogin(&e.get_unique_id().to_ascii_lowercase(), e.get_name());
    });
    plugin.register_event(|e: &mut PlayerJoinEvent| {
        let p = e.get_player().clone();
        let (offline, first) = users::join(&p);
        if !offline.is_empty() {
            let pl = p.clone();
            if let Some(plugin) = crate::plugin() {
                let mut once = Some(offline);
                plugin.run_task_later(40, move || {
                    if let Some(list) = once.take() {
                        votes::deliver_offline(&pl, list);
                    }
                });
            }
        }
        if let Some(plugin) = crate::plugin() {
            let kind = if first { "FIRST_JOIN" } else { "LOGIN" };
            let pl = p.clone();
            plugin.run_task_later(60, move || remind(&pl, kind));
        }
    });
    plugin.register_event(|e: &mut PlayerQuitEvent| {
        let u = e.get_player().get_unique_id().to_ascii_lowercase();
        users::quit(&u);
        commands::closed(&u);
    });
    plugin.register_event_with(enums::event_priority::HIGH, false, |e: &mut InventoryClickEvent| commands::on_click(e));
    plugin.register_event(|e: &mut InventoryCloseEvent| {
        if let Some(p) = e.get_player() {
            commands::closed(&p.get_unique_id().to_ascii_lowercase());
        }
    });
    // Interval reminders: every minute, players whose INTERVAL reminder is due (its Cooldown guards repeats).
    plugin.run_task_timer(1200, 1200, || {
        for p in horizon_bukkit::get_online_players() {
            remind(&p, "INTERVAL");
        }
    });

    for p in horizon_bukkit::get_online_players() {
        let u = p.get_unique_id().to_ascii_lowercase();
        let name = p.get_name().to_owned();
        std::thread::spawn(move || {
            users::prelogin(&u, &name);
            horizon_kit::sync::on_main(move || {
                let _ = users::join(&p);
            });
        });
    }
    log(&format!("VotingPlugin 7.1.1 enabled in {} ms (Horizon native, built-in Votifier).", started.elapsed().as_millis()));
}

fn on_disable(_plugin: Plugin) {
    votifier::stop();
    votes::stop();
    let all: Vec<users::User> = users::users().values().cloned().collect();
    users::save(&all);
    users::save_server();
    users::close();
}

bukkit_plugin!(on_enable, on_disable);
