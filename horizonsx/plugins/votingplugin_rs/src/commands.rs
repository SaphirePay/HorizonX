//! `/vote` (sites, next, last, total, top, today, party, points, best, streak, gui, shop, toggle) and
//! `/adminvote` (`/av`: vote, points, voteparty, reload, sites, settotal, clearoffline, user), with their aliases
//! (`/votetop`, `/avreload`...). Player commands are open (VotingPlugin's GiveDefaultPermission); admin ones need
//! `VotingPlugin.Admin`.

use crate::settings::{self, colour};
use crate::users::{self, User};
use horizon_bukkit::{CommandSender, Inventory, Player};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

fn tell(sender: &CommandSender, text: &str) {
    for line in text.split('\n') {
        sender.send_message(line);
    }
}

fn hm(ms: i64) -> (String, String) {
    let m = ms.max(0) / 60_000;
    ((m / 60).to_string(), (m % 60).to_string())
}

/// The online user by name (or the sender), copied.
fn user_of(sender: &CommandSender, arg: Option<&str>) -> Option<User> {
    let us = users::users();
    match arg {
        Some(n) => us.values().find(|u| u.name.eq_ignore_ascii_case(n)).cloned(),
        None => sender.as_player().and_then(|p| us.get(&p.get_unique_id().to_ascii_lowercase()).cloned()),
    }
}

/// An online user, or the row read off the server thread and shown when it comes back.
fn with_user(sender: &CommandSender, arg: Option<&str>, show: fn(&CommandSender, &User)) {
    if let Some(u) = user_of(sender, arg) {
        show(sender, &u);
        return;
    }
    let Some(name) = arg.map(str::to_owned) else {
        tell(sender, "§cOnly players can do that.");
        return;
    };
    let sender = sender.clone();
    std::thread::spawn(move || {
        let found = users::load_by_name(&name);
        horizon_kit::sync::on_main(move || match found {
            Some(u) => show(&sender, &u),
            None => tell(&sender, &settings::get().msg("Format.UserNotExist", &[("player", &name)])),
        });
    });
}

fn sites(sender: &CommandSender) {
    let s = settings::get();
    for l in s.lines("Format.Commands.Vote.Text", &[]) {
        tell(sender, &l);
    }
    let user = user_of(sender, None);
    let only_open = s.cfg.bool("Format.Commands.Vote.OnlyShowSitesToVote", false);
    for (i, site) in s.enabled_sites().filter(|x| !x.hidden).enumerate() {
        if only_open && user.as_ref().is_some_and(|u| u.wait(site) > 0) {
            continue;
        }
        tell(sender, &s.msg("Format.Commands.Vote.Sites", &[("num", &(i + 1).to_string()), ("SiteName", &site.name), ("url", &site.url)]));
    }
}

fn next(sender: &CommandSender, u: &User) {
    let s = settings::get();
    tell(sender, &s.msg("Format.Commands.Vote.Next.Title", &[("player", &u.name)]));
    for site in s.enabled_sites().filter(|x| !x.hidden) {
        let wait = u.wait(site);
        let info = if wait <= 0 {
            s.msg("Format.Commands.Vote.Next.Info.CanVote", &[])
        } else {
            let (h, m) = hm(wait);
            s.msg(if site.daily { "Format.Commands.Vote.Next.Info.VoteDelayDaily" } else { "Format.Commands.Vote.Next.Info.TimeUntilVote" }, &[("hours", &h), ("minutes", &m)])
        };
        tell(sender, &s.msg("Format.Commands.Vote.Next.Layout", &[("SiteName", &site.name), ("info", &info)]));
    }
}

fn last(sender: &CommandSender, u: &User) {
    let s = settings::get();
    tell(sender, &s.msg("Format.Commands.Vote.Last.Title", &[("player", &u.name)]));
    let now = crate::time::now_ms();
    for site in s.enabled_sites().filter(|x| !x.hidden) {
        let since = match u.last_votes.get(&site.key) {
            Some(t) if *t > 0 => s.msg("Format.Commands.Vote.Last.LastVoted", &[("times", &horizon_kit::time::format((now - t).max(0) as u64))]),
            _ => s.msg("Format.Commands.Vote.Last.NeverVoted", &[]),
        };
        tell(sender, &s.msg("Format.Commands.Vote.Last.Line", &[("SiteName", &site.name), ("timeSince", &since)]));
    }
}

fn total(sender: &CommandSender, u: &User) {
    let s = settings::get();
    for l in s.lines("Format.Commands.Vote.Total", &[("player", &u.name), ("DailyTotal", &u.day.to_string()), ("WeeklyTotal", &u.week.to_string()), ("MonthlyTotal", &u.month.to_string()), ("AllTimeTotal", &u.all.to_string())]) {
        tell(sender, &l);
    }
}

fn total_all(sender: &CommandSender) {
    let s = settings::get();
    let g = users::SERVER.lock().unwrap_or_else(|e| e.into_inner()).global;
    for l in s.lines("Format.Commands.Vote.TotalAll", &[("DailyTotal", &g[0].to_string()), ("WeeklyTotal", &g[1].to_string()), ("MonthlyTotal", &g[2].to_string()), ("AllTimeTotal", &g[3].to_string())]) {
        tell(sender, &l);
    }
}

fn points(sender: &CommandSender, u: &User) {
    tell(sender, &settings::get().msg("Format.Commands.Vote.Points", &[("Player", &u.name), ("Points", &u.points.to_string())]));
}

fn best(sender: &CommandSender, u: &User) {
    let s = settings::get();
    tell(sender, &s.msg("Format.Commands.Vote.Best.Title", &[("player", &u.name)]));
    for l in s.lines("Format.Commands.Vote.Best.Lines", &[("HighestDailyTotal", &u.high_day.to_string()), ("HighestWeeklyTotal", &u.high_week.to_string()), ("HighestMonthlyTotal", &u.high_month.to_string())]) {
        tell(sender, &l);
    }
}

fn streak(sender: &CommandSender, u: &User) {
    let s = settings::get();
    tell(sender, &s.msg("Format.Commands.Vote.Streak.Title", &[("player", &u.name)]));
    let v = [
        ("DailyStreak", u.day_streak.to_string()),
        ("WeeklyStreak", u.week_streak.to_string()),
        ("MonthlyStreak", u.month_streak.to_string()),
        ("BestDailyStreak", u.best_day_streak.to_string()),
        ("BestWeeklyStreak", u.best_week_streak.to_string()),
        ("BestMonthlyStreak", u.best_month_streak.to_string()),
    ];
    let vars: Vec<(&str, &str)> = v.iter().map(|(a, b)| (*a, b.as_str())).collect();
    for l in s.lines("Format.Commands.Vote.Streak.Lines", &vars) {
        tell(sender, &l);
    }
}

fn top(sender: &CommandSender, kind: Option<&str>, page: usize) {
    let s = settings::get();
    let kind = kind.unwrap_or(&s.top_default).to_owned();
    let tops = users::tops();
    let list = tops.of(&kind);
    let pages = list.len().div_ceil(10).max(1);
    let page = page.clamp(1, pages);
    let label = match kind.to_ascii_lowercase().as_str() {
        "all" | "alltime" => s.msg("Format.TopVoter.AllTime", &[]),
        "week" | "weekly" => s.msg("Format.TopVoter.Weekly", &[]),
        "day" | "daily" => s.msg("Format.TopVoter.Daily", &[]),
        _ => s.msg("Format.TopVoter.Monthly", &[]),
    };
    tell(sender, &s.msg("Format.Commands.Vote.Top.Title", &[("Top", &label), ("page", &page.to_string()), ("maxpages", &pages.to_string())]));
    for (i, (name, _, votes)) in list.iter().enumerate().skip((page - 1) * 10).take(10) {
        tell(sender, &s.msg("Format.Commands.Vote.Top.Line", &[("num", &(i + 1).to_string()), ("player", name), ("votes", &votes.to_string())]));
    }
}

fn today(sender: &CommandSender) {
    let s = settings::get();
    for l in s.lines("Format.Commands.Vote.Today.Title", &[("page", "1"), ("maxpage", "1")]) {
        tell(sender, &l);
    }
    let day = crate::time::day_index(crate::time::now_ms());
    for u in users::users().values() {
        for (site, t) in &u.last_votes {
            if crate::time::day_index(*t) == day {
                tell(sender, &s.msg("Format.Commands.Vote.Today.Line", &[("player", &u.name), ("VoteSite", site), ("Time", &crate::time::date(*t))]));
            }
        }
    }
}

fn party(sender: &CommandSender) {
    let s = settings::get();
    let required = s.party.get("VotesRequired").and_then(|x| x.as_i64()).unwrap_or(20);
    let votes = users::SERVER.lock().unwrap_or_else(|e| e.into_inner()).party;
    for l in s.lines("Format.Commands.Vote.Party", &[("Votes", &votes.to_string()), ("NeededVotes", &(required - votes).max(0).to_string()), ("VotesRequired", &required.to_string())]) {
        tell(sender, &l);
    }
}

// ---- GUIs: the vote sites, the vote shop ----

#[derive(Clone)]
enum Click {
    Site(String),
    Shop(String),
}

static MENUS: LazyLock<Mutex<HashMap<String, (Inventory, HashMap<usize, Click>)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn open_gui(player: &Player) {
    let s = settings::get();
    let u = user_of(&CommandSender::Player(player.clone()), None).unwrap_or_default();
    let list: Vec<&settings::Site> = s.enabled_sites().filter(|x| !x.hidden).collect();
    let rows = (list.len() / 9 + 1).clamp(1, 6);
    let mut inv = Inventory::create(rows * 9, &colour("&3&lVote Sites"));
    let mut slots = HashMap::new();
    for (i, site) in list.iter().enumerate().take(rows * 9) {
        let wait = u.wait(site);
        let (h, m) = hm(wait);
        let state = if wait <= 0 { s.msg("Format.Commands.Vote.Next.Info.CanVote", &[]) } else { s.msg("Format.Commands.Vote.Next.Info.TimeUntilVote", &[("hours", &h), ("minutes", &m)]) };
        let mut it = crate::rewards::item(&site.item, &[]).unwrap_or_else(|| horizon_bukkit::ItemStack::new(horizon_bukkit::Material::of("DIAMOND"), 1));
        it.set_display_name(&colour(&format!("&6&l{}", site.name)));
        it.set_lore(vec![colour(&format!("&7{state}")), colour("&eClick for the vote link")]);
        inv.set_item(i, Some(it));
        slots.insert(i, Click::Site(site.key.clone()));
    }
    player.open_inventory(&inv);
    MENUS.lock().unwrap_or_else(|e| e.into_inner()).insert(player.get_unique_id().to_ascii_lowercase(), (inv, slots));
}

fn open_shop(player: &Player) {
    let s = settings::get();
    if !s.shop.get("VoteShop").and_then(|v| v.get("Enabled")).and_then(|x| x.as_bool()).unwrap_or(true) {
        player.send_message(&colour(s.shop.get("VoteShop").and_then(|v| v.get("Disabled")).and_then(|x| x.as_str()).unwrap_or("&cVote shop disabled")));
        return;
    }
    let Some(items) = s.shop.get("Shop").and_then(|x| x.as_mapping()) else { return };
    let entries: Vec<(String, &serde_yaml::Value)> = items.iter().filter_map(|(k, v)| Some((k.as_str()?.to_owned(), v))).filter(|(_, v)| v.get("Cost").is_some()).collect();
    let rows = (entries.len() / 9 + 1).clamp(1, 6);
    let title = s.shop.get("VoteShop").and_then(|v| v.get("Name")).and_then(|x| x.as_str()).unwrap_or("VoteShop").to_owned();
    let mut inv = Inventory::create(rows * 9, &colour(&title));
    let mut slots = HashMap::new();
    let mut slot = 0;
    for (key, v) in entries {
        if let Some(perm) = v.get("Permission").and_then(|x| x.as_str()).filter(|p| !p.is_empty())
            && !player.has_permission(perm)
            && v.get("HideOnNoPermission").and_then(|x| x.as_bool()).unwrap_or(true)
        {
            continue;
        }
        if slot >= rows * 9 {
            break;
        }
        if let Some(it) = crate::rewards::item(v.get("DisplayItem").unwrap_or(&serde_yaml::Value::Null), &[]) {
            inv.set_item(slot, Some(it));
            slots.insert(slot, Click::Shop(key));
            slot += 1;
        }
    }
    player.open_inventory(&inv);
    MENUS.lock().unwrap_or_else(|e| e.into_inner()).insert(player.get_unique_id().to_ascii_lowercase(), (inv, slots));
}

pub fn on_click(e: &mut horizon_bukkit::fam_ev_player::InventoryClickEvent) {
    let Some(gui) = e.get_inventory().gui() else { return };
    let Some(player) = e.get_who_clicked() else { return };
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let found = {
        let menus = MENUS.lock().unwrap_or_else(|e| e.into_inner());
        let Some((inv, slots)) = menus.get(&uuid) else { return };
        if !inv.same_as(gui) {
            return;
        }
        usize::try_from(e.get_raw_slot()).ok().and_then(|s| slots.get(&s).cloned())
    };
    e.set_cancelled(true);
    let Some(click) = found else { return };
    horizon_kit::sync::on_main(move || match click {
        Click::Site(key) => {
            player.close_inventory();
            let s = settings::get();
            if let Some(site) = s.site(&key) {
                player.send_message(&s.msg("Format.Commands.Vote.Sites", &[("num", "1"), ("SiteName", &site.name), ("url", &site.url)]));
            }
        }
        Click::Shop(key) => buy(&player, &key),
    });
}

pub fn closed(uuid: &str) {
    MENUS.lock().unwrap_or_else(|e| e.into_inner()).remove(uuid);
}

fn buy(player: &Player, key: &str) {
    let s = settings::get();
    let Some(v) = s.shop.get("Shop").and_then(|x| x.get(key)).cloned() else { return };
    let cost = v.get("Cost").and_then(|x| x.as_i64()).unwrap_or(0);
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let ok = {
        let mut us = users::users();
        match us.get_mut(&uuid) {
            Some(u) if u.points >= cost => {
                u.points -= cost;
                u.dirty = true;
                true
            }
            _ => false,
        }
    };
    let ident = v.get("Identifier_Name").and_then(|x| x.as_str()).unwrap_or(key).to_owned();
    if !ok {
        player.send_message(&s.msg("Format.ShopFailed", &[("Points", &cost.to_string())]));
        return;
    }
    if v.get("CloseGUI").and_then(|x| x.as_bool()).unwrap_or(true) {
        player.close_inventory();
    }
    crate::rewards::give(player, v.get("Rewards").unwrap_or(&serde_yaml::Value::Null), &[]);
    player.send_message(&s.msg("Format.ShopPurchase", &[("Identifier", &ident), ("Points", &cost.to_string())]));
}

// ---- /vote ----

pub fn vote(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    // `/votetop 2` and friends: the alias is the sub-command.
    let alias = label.to_ascii_lowercase();
    let sub_from_alias = alias.strip_prefix("vote").filter(|r| !r.is_empty()).map(str::to_owned).or_else(|| alias.strip_prefix('v').filter(|r| !r.is_empty() && *r != "ote").map(str::to_owned));
    let (sub, rest): (String, Vec<&str>) = match sub_from_alias {
        Some(s) => (s, args.to_vec()),
        None => (args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default(), args.iter().skip(1).copied().collect()),
    };
    let s = settings::get();
    match sub.as_str() {
        "" | "url" | "list" | "all" => sites(sender),
        "help" => {
            tell(sender, &s.msg("Format.Commands.Vote.Help.Title", &[]));
            for (c, h) in [("/vote", "Vote sites"), ("/vote next [player]", "Time until you can vote"), ("/vote last [player]", "Your last votes"), ("/vote total [player|all]", "Vote totals"), ("/vote top [type] [page]", "Top voters"), ("/vote today", "Who voted today"), ("/vote party", "Vote party progress"), ("/vote points [player]", "Voting points"), ("/vote streak", "Vote streaks"), ("/vote best", "Best totals"), ("/vote gui", "Vote GUI"), ("/vote shop", "Vote shop"), ("/vote toggle broadcast", "Vote broadcasts on/off")] {
                tell(sender, &s.msg("Format.Commands.Vote.Help.Line", &[("Command", c), ("HelpMessage", h)]));
            }
        }
        "next" => with_user(sender, rest.first().copied(), next),
        "last" => with_user(sender, rest.first().copied(), last),
        "total" => {
            if rest.first().is_some_and(|a| a.eq_ignore_ascii_case("all")) {
                total_all(sender);
            } else {
                with_user(sender, rest.first().copied(), total);
            }
        }
        "points" => with_user(sender, rest.first().copied(), points),
        "best" => with_user(sender, rest.first().copied(), best),
        "streak" => with_user(sender, rest.first().copied(), streak),
        "top" => {
            let (kind, page) = match rest.as_slice() {
                [] => (None, 1),
                [a] => match a.parse::<usize>() {
                    Ok(n) => (None, n),
                    Err(_) => (Some(*a), 1),
                },
                [a, b, ..] => (Some(*a), b.parse().unwrap_or(1)),
            };
            top(sender, kind, page);
        }
        "today" => today(sender),
        "party" => party(sender),
        "gui" => match sender.as_player() {
            Some(p) => open_gui(p),
            None => sites(sender),
        },
        "shop" => {
            if let Some(p) = sender.as_player() {
                open_shop(p);
            }
        }
        "toggle" | "togglebroadcast" => {
            let Some(p) = sender.as_player() else { return true };
            let state = {
                let mut us = users::users();
                us.get_mut(&p.get_unique_id().to_ascii_lowercase()).map(|u| {
                    u.no_broadcast = !u.no_broadcast;
                    u.dirty = true;
                    u.no_broadcast
                })
            };
            if let Some(off) = state {
                tell(sender, &s.msg(if off { "Format.Commands.Vote.ToggleBroadcasts.Disabled" } else { "Format.Commands.Vote.ToggleBroadcasts.Enabled" }, &[]));
            }
        }
        _ => tell(sender, &s.msg("Format.InvalidCommand.Vote", &[])),
    }
    true
}

// ---- /adminvote ----

pub fn admin(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    let s = settings::get();
    if !sender.has_permission("VotingPlugin.Admin") && !sender.has_permission("votingplugin.admin") {
        tell(sender, &s.msg("Format.NoPerms", &[]));
        return true;
    }
    let alias = label.to_ascii_lowercase();
    let from_alias = alias.strip_prefix("adminvote").or_else(|| alias.strip_prefix("av")).filter(|r| !r.is_empty()).map(str::to_owned);
    let (sub, rest): (String, Vec<&str>) = match from_alias {
        Some(x) => (x, args.to_vec()),
        None => (args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default(), args.iter().skip(1).copied().collect()),
    };
    match sub.as_str() {
        "" | "help" => {
            for l in ["&6/av vote <player> <site> &7- trigger a vote", "&6/av points <player> <set|add|remove> <n> &7- voting points", "&6/av voteparty <force|setvotecount n> &7- vote party", "&6/av settotal <player> <alltime|monthly|weekly|daily> <n>", "&6/av clearoffline <player>", "&6/av sites &7- vote sites and services", "&6/av reload", "&6/av version"] {
                tell(sender, &colour(l));
            }
        }
        "reload" | "reloadall" => {
            crate::reload();
            tell(sender, &colour("&aVotingPlugin reloaded (the Votifier port takes a restart of the plugin)."));
        }
        "version" => tell(sender, &colour("&6VotingPlugin 7.1.1 &7(Horizon native rewrite, CC BY 3.0, original by BenCodez)")),
        "sites" | "servicesites" => {
            for site in &s.sites {
                tell(sender, &colour(&format!("&6{} &7service &e{} &7enabled &f{} &7delay &f{}", site.key, site.service, site.enabled, horizon_kit::time::format(site.delay_ms as u64))));
            }
        }
        "vote" | "test" => {
            let (Some(player), Some(site)) = (rest.first(), rest.get(1)) else {
                tell(sender, &colour("&c/av vote <player> <site>"));
                return true;
            };
            let service = s.site(site).map(|x| x.service.clone()).unwrap_or_else(|| (*site).to_owned());
            crate::votes::received(&service, player, "127.0.0.1", &crate::time::now_ms().to_string());
            tell(sender, &colour(&format!("&aVote triggered for {player} on {site}.")));
        }
        "points" | "user" => {
            // /av points <player> <set|add|remove> <n>   or   /av user <player> addpoints <n>
            let (Some(name), Some(op), Some(n)) = (rest.first(), rest.get(1), rest.get(2).and_then(|x| x.parse::<i64>().ok())) else {
                tell(sender, &colour("&c/av points <player> <set|add|remove> <n>"));
                return true;
            };
            let op = op.to_ascii_lowercase();
            let edit = move |u: &mut User| match op.trim_end_matches("points") {
                "set" => u.points = n,
                "add" | "give" => u.points += n,
                _ => u.points -= n,
            };
            let online = {
                let mut us = users::users();
                match us.values_mut().find(|u| u.name.eq_ignore_ascii_case(name)) {
                    Some(u) => {
                        edit(u);
                        u.dirty = true;
                        true
                    }
                    None => false,
                }
            };
            if !online {
                let name = (*name).to_owned();
                std::thread::spawn(move || {
                    if let Some(mut u) = users::load_by_name(&name) {
                        edit(&mut u);
                        users::save(&[u]);
                    }
                });
            }
            tell(sender, &s.msg("Format.Commands.AdminVote.Points.PlayerGiven", &[("amount", &n.to_string())]));
        }
        "settotal" => {
            let (Some(name), Some(kind), Some(n)) = (rest.first(), rest.get(1), rest.get(2).and_then(|x| x.parse::<i64>().ok())) else { return true };
            let mut us = users::users();
            if let Some(u) = us.values_mut().find(|u| u.name.eq_ignore_ascii_case(name)) {
                match kind.to_ascii_lowercase().as_str() {
                    "alltime" | "all" => u.all = n,
                    "weekly" | "week" => u.week = n,
                    "daily" | "day" => u.day = n,
                    _ => u.month = n,
                }
                u.dirty = true;
            }
            tell(sender, &colour("&aTotal set."));
        }
        "clearoffline" | "clearofflinevotes" => {
            let Some(name) = rest.first().map(|x| (*x).to_owned()) else { return true };
            std::thread::spawn(move || {
                if let Some(mut u) = users::load_by_name(&name) {
                    u.offline.clear();
                    users::save(&[u]);
                }
            });
            tell(sender, &colour("&aOffline votes cleared."));
        }
        "voteparty" => match rest.first().map(|x| x.to_ascii_lowercase()).as_deref() {
            Some("force") => crate::votes::count_party(Some(())),
            Some("setvotecount") => {
                if let Some(n) = rest.get(1).and_then(|x| x.parse::<i64>().ok()) {
                    users::SERVER.lock().unwrap_or_else(|e| e.into_inner()).party = n;
                }
                party(sender);
            }
            _ => party(sender),
        },
        _ => tell(sender, &s.msg("Format.InvalidCommand.AdminVote", &[])),
    }
    true
}

pub fn complete(_s: &CommandSender, label: &str, args: &[&str]) -> Vec<String> {
    let last = args.last().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let admin = label.to_ascii_lowercase().starts_with("a");
    let list: Vec<String> = if args.len() <= 1 {
        let subs: &[&str] = if admin { &["vote", "points", "voteparty", "settotal", "clearoffline", "sites", "reload", "version", "help"] } else { &["next", "last", "total", "top", "today", "party", "points", "streak", "best", "gui", "shop", "toggle", "help"] };
        subs.iter().map(|x| (*x).to_owned()).collect()
    } else if admin && args[0].eq_ignore_ascii_case("vote") && args.len() == 3 {
        settings::get().sites.iter().map(|x| x.key.clone()).collect()
    } else if !admin && args[0].eq_ignore_ascii_case("top") {
        ["monthly", "weekly", "daily", "alltime"].iter().map(|x| (*x).to_owned()).collect()
    } else {
        horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).collect()
    };
    list.into_iter().filter(|x| x.to_ascii_lowercase().starts_with(&last)).collect()
}
