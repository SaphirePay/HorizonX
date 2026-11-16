//! A vote, from the Votifier receiver (any thread) to its rewards: the site found (or made, AutoCreateVoteSites),
//! the totals counted, then for an online voter on the server thread the site's rewards, EverySiteReward, the
//! broadcast, FirstVote/FirstVoteToday/AllSites, VoteMilestones (and the older Cumulative/MileStones), vote
//! streaks and the vote party; for an offline one the data thread keeps it for the next join.

use crate::rewards;
use crate::settings::{self, Settings};
use crate::users::{self, SERVER, User};
use horizon_bukkit::Player;
use serde_yaml::Value as Yaml;
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

pub enum Job {
    Offline(String, String, i64),
    Save(Vec<User>),
}

static DATA: Mutex<Option<Sender<Job>>> = Mutex::new(None);
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static THREAD: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

pub fn send(job: Job) {
    if let Some(tx) = DATA.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(job);
    }
}

/// The data thread: offline votes, saves, the minute checks (resets) and top lists.
pub fn start() {
    let (tx, rx) = channel::<Job>();
    *DATA.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    STOP.store(false, std::sync::atomic::Ordering::SeqCst);
    let h = std::thread::Builder::new()
        .name("VotingPlugin-data".into())
        .spawn(move || {
            users::refresh_tops();
            let mut last_minute = std::time::Instant::now();
            let mut last_top = std::time::Instant::now();
            loop {
                match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                    Ok(Job::Offline(name, site, time)) => {
                        let s = settings::get();
                        if !users::offline_vote(&name, &site, time, &s) {
                            crate::log(&format!("Vote from {name} on {site}: no such player has joined; not counted."));
                        }
                    }
                    Ok(Job::Save(list)) => users::save(&list),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => return,
                }
                if STOP.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                if last_minute.elapsed().as_secs() >= 60 {
                    users::check_periods();
                    users::save(&users::dirty());
                    users::save_server();
                    last_minute = std::time::Instant::now();
                }
                if last_top.elapsed().as_secs() >= 120 {
                    users::refresh_tops();
                    last_top = std::time::Instant::now();
                }
            }
        })
        .ok();
    *THREAD.lock().unwrap_or_else(|e| e.into_inner()) = h;
}

pub fn stop() {
    STOP.store(true, std::sync::atomic::Ordering::SeqCst);
    *DATA.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if let Some(h) = THREAD.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = h.join();
    }
}

/// A vote from the receiver (or `/av vote`): any thread.
pub fn received(service: &str, user: &str, address: &str, _timestamp: &str) {
    if user.trim().is_empty() {
        return;
    }
    let mut s = settings::get();
    if s.site_by_service(service).is_none() {
        if !s.auto_create {
            crate::log(&format!("Vote from {user} via unknown service '{service}' (AutoCreateVoteSites is off)."));
            return;
        }
        create_site(service);
        s = settings::get();
    }
    let Some(site) = s.site_by_service(service).cloned() else { return };
    if !site.enabled {
        return;
    }
    crate::log(&format!("Received a vote from {user} on {} ({address}).", site.key));
    let time = crate::time::now_ms();
    let uuid = users::uuid_by_name(user);
    match uuid {
        Some(u) if !site.force_offline => {
            let name = user.to_owned();
            horizon_kit::sync::on_main(move || online_vote(&u, &name, &site.key, time, true));
        }
        _ => send(Job::Offline(user.to_owned(), site.key.clone(), time)),
    }
    count_party(None);
}

/// Adds a site to VoteSites.yml for a service never seen (as VotingPlugin's AutoCreateVoteSites).
fn create_site(service: &str) {
    let folder = crate::folder();
    let mut c = horizon_kit::Config::load(&folder, "VoteSites.yml", include_str!("../data/VoteSites.yml"));
    let key: String = service.chars().map(|ch| if ch.is_alphanumeric() { ch } else { '_' }).collect();
    let p = format!("VoteSites.{key}");
    c.set(&format!("{p}.Enabled"), Yaml::Bool(true));
    c.set(&format!("{p}.Name"), Yaml::String(key.clone()));
    c.set(&format!("{p}.ServiceSite"), Yaml::String(service.to_owned()));
    c.set(&format!("{p}.VoteURL"), Yaml::String("VoteURL".into()));
    c.set(&format!("{p}.VoteDelay"), Yaml::String("24h".into()));
    c.set(&format!("{p}.Priority"), Yaml::Number(0.into()));
    c.set(&format!("{p}.Rewards.Messages.Player"), Yaml::String("&aThanks for voting on %ServiceSite%!".into()));
    c.save();
    crate::log(&format!("Made vote site '{key}' for service '{service}' (AutoCreateVoteSites)."));
    settings::load(&folder);
}

/// What one vote gives (server thread): counted now when `count`, else it was counted when it came in offline.
pub fn online_vote(uuid: &str, name: &str, site_key: &str, time: i64, count: bool) {
    let s = settings::get();
    let Some(player) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_unique_id().eq_ignore_ascii_case(uuid)) else {
        send(Job::Offline(name.to_owned(), site_key.to_owned(), time));
        return;
    };
    let Some(site) = s.site(site_key).cloned() else { return };
    let before;
    let after;
    {
        let mut us = users::users();
        let Some(u) = us.get_mut(uuid) else { return };
        before = u.clone();
        if count {
            users::count_vote(u, &site.key, time, &s);
        }
        after = u.clone();
    }
    let vars: Vec<(String, String)> = vec![
        ("site".into(), site.name.clone()),
        ("SiteName".into(), site.name.clone()),
        ("ServiceSite".into(), site.service.clone()),
        ("VoteSite".into(), site.name.clone()),
        ("url".into(), site.url.clone()),
        ("AllTimeTotal".into(), after.all.to_string()),
        ("total".into(), after.all.to_string()),
        ("points".into(), after.points.to_string()),
    ];
    let v: Vec<(&str, &str)> = vars.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    rewards::give(&player, &site.rewards, &v);
    rewards::give(&player, &s.every_site, &v);
    // Broadcast to everyone who hasn't turned them off.
    if s.broadcast_type != "NONE" && !s.broadcast_msg.is_empty() {
        let text = settings::colour(&settings::fill(&s.broadcast_msg, &[("player", name), ("site", &site.name), ("SiteName", &site.name)]));
        let quiet: Vec<String> = users::users().values().filter(|u| u.no_broadcast).map(|u| u.uuid.clone()).collect();
        for p in horizon_bukkit::get_online_players() {
            if !quiet.contains(&p.get_unique_id().to_ascii_lowercase()) {
                p.send_message(&text);
            }
        }
    }
    special(&s, &player, &before, &after, &v);
}

fn total_of(u: &User, kind: &str) -> i64 {
    u.total(&kind.to_ascii_uppercase().replace("TOTAL", "").replace("_VOTES", ""))
}

/// First votes, milestones, cumulative rewards and streaks crossed by this vote.
fn special(s: &Settings, player: &Player, before: &User, after: &User, v: &[(&str, &str)]) {
    if after.all == 1 && before.all == 0 {
        rewards::give(player, &s.first_vote, v);
    }
    if after.day == 1 && before.day == 0 {
        rewards::give(player, &s.first_vote_today, v);
    }
    let enabled = s.enabled_sites().count() as i64;
    if enabled > 0 && after.total("ALLSITES_TODAY") >= enabled && before.total("ALLSITES_TODAY") < enabled {
        rewards::give(player, &s.all_sites, v);
    }
    let now = crate::time::now_ms();
    let mut hits: Vec<(String, Yaml, i64)> = Vec::new();
    for m in &s.milestones {
        let (b, a) = (total_of(before, &m.total), total_of(after, &m.total));
        if a <= b {
            continue;
        }
        let hit = m.at.iter().any(|x| *x > b && *x <= a) || (m.every > 0 && (b / m.every) != (a / m.every));
        if !hit {
            continue;
        }
        let last = after.milestones.get(&m.name).copied().unwrap_or(0);
        let allowed = match &m.limit {
            Some((kind, _)) if kind == "WINDOW_DAY" => crate::time::day_index(last) != crate::time::day_index(now),
            Some((kind, _)) if kind == "WINDOW_WEEK" => crate::time::week_index(last) != crate::time::week_index(now),
            Some((kind, _)) if kind == "WINDOW_MONTH" => crate::time::month_index(last) != crate::time::month_index(now),
            Some((kind, ms)) if kind == "COOLDOWN" => now - last >= *ms,
            _ => true,
        };
        if allowed {
            hits.push((m.name.clone(), m.rewards.clone(), a));
        }
    }
    if !hits.is_empty() {
        let mut us = users::users();
        if let Some(u) = us.get_mut(&after.uuid) {
            for (n, _, _) in &hits {
                u.milestones.insert(n.clone(), now);
            }
            u.dirty = true;
        }
    }
    for (_, r, amount) in hits {
        let a = amount.to_string();
        let mut vv: Vec<(&str, &str)> = v.to_vec();
        vv.push(("amount", &a));
        rewards::give(player, &r, &vv);
    }
    for (n, kind, r) in &s.cumulative {
        let (b, a) = (total_of(before, kind), total_of(after, kind));
        if a > b && b / n != a / n {
            let c = n.to_string();
            let mut vv: Vec<(&str, &str)> = v.to_vec();
            vv.push(("cumulative", &c));
            rewards::give(player, r, &vv);
        }
    }
    for (n, r) in &s.old_milestones {
        if before.all < *n && after.all >= *n {
            let c = n.to_string();
            let mut vv: Vec<(&str, &str)> = v.to_vec();
            vv.push(("milestone", &c));
            rewards::give(player, r, &vv);
        }
    }
    for st in &s.streaks {
        let (b, a) = match st.kind.as_str() {
            "WEEKLY" | "WEEK" => (before.week_streak, after.week_streak),
            "MONTHLY" | "MONTH" => (before.month_streak, after.month_streak),
            _ => (before.day_streak, after.day_streak),
        };
        if a == b || st.amount <= 0 {
            continue;
        }
        let hit = if st.recurring { a % st.amount == 0 } else { a == st.amount };
        if hit {
            let c = a.to_string();
            let mut vv: Vec<(&str, &str)> = v.to_vec();
            vv.push(("streak", &c));
            rewards::give(player, &st.rewards, &vv);
        }
    }
}

/// One more vote toward the party; at the goal its rewards for everyone (on the server thread).
pub fn count_party(force: Option<()>) {
    let s = settings::get();
    if !s.party.get("Enabled").and_then(|x| x.as_bool()).unwrap_or(false) {
        return;
    }
    let required = s.party.get("VotesRequired").and_then(|x| x.as_i64()).unwrap_or(20).max(1);
    let reached = {
        let mut sv = SERVER.lock().unwrap_or_else(|e| e.into_inner());
        if force.is_none() {
            sv.party += 1;
        }
        let today = crate::time::day_index(crate::time::now_ms());
        let once_a_day = s.party.get("OnlyOncePerDay").and_then(|x| x.as_bool()).unwrap_or(false);
        let left = required - sv.party;
        let at: Vec<i64> = s.party.get("VoteReminder").and_then(|r| r.get("AtVotes")).and_then(|x| x.as_sequence()).map(|l| l.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default();
        if at.contains(&left) {
            let text = s.party.get("VoteReminder").and_then(|r| r.get("Broadcast")).and_then(|x| x.as_str()).unwrap_or("").to_owned();
            let l = left.to_string();
            horizon_kit::sync::on_main(move || {
                horizon_bukkit::broadcast_message(&settings::colour(&settings::fill(&text, &[("votesrequired", &l)])));
            });
        }
        if (sv.party >= required || force.is_some()) && !(once_a_day && sv.party_day == today) {
            sv.party = 0;
            sv.party_day = today;
            true
        } else {
            false
        }
    };
    if reached {
        horizon_kit::sync::on_main(party_rewards);
    }
}

fn party_rewards() {
    let s = settings::get();
    let p = &s.party;
    if let Some(b) = p.get("Broadcast").and_then(|x| x.as_str()).filter(|b| !b.is_empty()) {
        horizon_bukkit::broadcast_message(&settings::colour(b));
    }
    for c in p.get("GlobalCommands").and_then(|x| x.as_sequence()).into_iter().flatten().filter_map(|x| x.as_str()) {
        horizon_bukkit::fam_core::dispatch_command_as(&horizon_bukkit::CommandSender::Console, c.trim_start_matches('/'));
    }
    let min_votes = p.get("UserVotesRequired").and_then(|x| x.as_i64()).unwrap_or(0);
    let rewards = p.get("Rewards").cloned().unwrap_or(Yaml::Null);
    let contributed: std::collections::HashMap<String, i64> = users::users().values().map(|u| (u.uuid.clone(), u.party_votes)).collect();
    for pl in horizon_bukkit::get_online_players() {
        let u = pl.get_unique_id().to_ascii_lowercase();
        if contributed.get(&u).copied().unwrap_or(0) < min_votes {
            continue;
        }
        rewards::give(&pl, &rewards, &[]);
    }
    for u in users::users().values_mut() {
        u.party_votes = 0;
        u.dirty = true;
    }
}

/// A joining player's votes from while they were away, given now.
pub fn deliver_offline(player: &Player, offline: Vec<(String, i64)>) {
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let name = player.get_name().to_owned();
    for (site, time) in offline {
        online_vote(&uuid, &name, &site, time, false);
    }
}
