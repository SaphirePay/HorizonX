//! HorizonSMP: everything a survival multiplayer server needs on top of the game, in one native plugin:
//!
//! - [`claims`]: land claims (golden shovel, `/claim`), trust levels, protection from griefing, explosions, fire.
//! - [`shop`]: the server shop, `/sell`, `/worth`.
//! - [`crates`]: crates with keys and a spinning reward strip.
//! - [`teams`]: teams with a home, chat, roles and no friendly fire.
//! - [`rewards`]: daily streak, playtime milestones, the first-join welcome, playtime.
//! - [`combat`]: combat tag, combat logging, new-player PvP protection, kill counts.
//! - [`bounty`]: bounties on players.
//! - [`stats`]: profiles and leaderboards.
//! - [`menu`]: `/menu`, the hub that opens all of it (and other plugins' screens).
//!
//! Also here: the server-list MOTD, tips every few minutes and `/rules`. Every menu and message shares
//! [`ui`]'s look. Data lives in plugins/HorizonSMP (players/, claims.json, teams.json, ...), saved off the server
//! thread.

pub mod bounty;
pub mod claims;
pub mod combat;
pub mod conf;
pub mod crates;
pub mod data;
pub mod menu;
pub mod rewards;
pub mod shop;
pub mod spawn;
pub mod stats;
pub mod teams;
pub mod ui;

use horizon_bukkit::fam_ev_world::PaperServerListPingEvent;
use horizon_bukkit::prelude::*;
use horizon_kit::{app, gui, hooks};
use std::collections::HashMap;
use std::sync::Mutex;

/// Whether the player has a node (LuckPerms decides); tells them when not.
pub fn allowed(p: &Player, node: &str) -> bool {
    if p.has_permission(node) {
        return true;
    }
    ui::deny(p, &format!("You don't have permission for that ({node})."));
    false
}

/// Gives an item; what doesn't fit drops at the player's feet.
pub fn give_item(p: &Player, stack: &ItemStack) {
    if !horizon_kit::inv::give(p, stack) {
        ui::bar(p, "<#facc15>Your inventory is full: the rest dropped at your feet.");
    }
}

// ---- Teleport with a warm-up ----

struct Warmup {
    to: Location,
    from: Location,
    at: u64,
    what: String,
}

static WARMUPS: Mutex<Option<HashMap<String, Warmup>>> = Mutex::new(None);

fn warmups<R>(f: impl FnOnce(&mut HashMap<String, Warmup>) -> R) -> R {
    let mut held = WARMUPS.lock().unwrap_or_else(|e| e.into_inner());
    f(held.get_or_insert_with(HashMap::new))
}

/// Teleports after `secs` if the player stands still (0 = at once).
pub fn warmup(p: &Player, secs: u64, what: &str, to: Location) {
    if secs == 0 {
        p.teleport(&to);
        ui::sound(p, "ENTITY_ENDERMAN_TELEPORT:0.6:1.2");
        return;
    }
    let at = horizon_kit::time::now_ms() + secs * 1000;
    warmups(|m| m.insert(p.get_unique_id(), Warmup { to, from: p.get_location(), at, what: what.to_owned() }));
    ui::say(p, "Teleport", &format!("Teleporting to {what} in <white>{secs}</white> seconds. Don't move!"));
    ui::sound(p, "BLOCK_NOTE_BLOCK_PLING:0.5:0.8");
}

fn warmup_tick() {
    let now = horizon_kit::time::now_ms();
    let ids: Vec<String> = warmups(|m| m.keys().cloned().collect());
    for uuid in ids {
        let Some(p) = horizon_bukkit::fam_ess::get_player_by_id(&uuid) else {
            warmups(|m| m.remove(&uuid));
            continue;
        };
        let here = p.get_location();
        let (moved, due, what) = warmups(|m| {
            let w = m.get(&uuid)?;
            let moved = w.from.get_world().get_name() != here.get_world().get_name() || (w.from.x - here.x).powi(2) + (w.from.z - here.z).powi(2) + (w.from.y - here.y).powi(2) > 0.6;
            Some((moved, w.at <= now, w.what.clone()))
        })
        .unwrap_or((true, false, String::new()));
        if moved || combat::tagged(&uuid) {
            warmups(|m| m.remove(&uuid));
            ui::bar(&p, "<#f87171>✖ Teleport cancelled: you moved.");
            ui::bad(&p);
        } else if due {
            if let Some(w) = warmups(|m| m.remove(&uuid)) {
                p.teleport(&w.to);
                ui::bar(&p, &format!("<#4ade80>✔ Teleported to {what}."));
                ui::sound(&p, "ENTITY_ENDERMAN_TELEPORT:0.6:1.2");
            }
        } else {
            let left = warmups(|m| m.get(&uuid).map(|w| w.at.saturating_sub(now))).unwrap_or(0);
            ui::bar(&p, &format!("<#22d3ee>Teleporting in <white>{:.1}s</white> <#71717a>• don't move", left as f64 / 1000.0));
        }
    }
}

// ---- Rules, tips, MOTD ----

pub fn rules(p: &Player) {
    let s = conf::get();
    let mut lines = vec![String::new(), format!(" {}", ui::grad(&format!("<bold>{} RULES</bold>", s.server.to_ascii_uppercase())))];
    lines.extend(s.rules.iter().map(|r| format!(" {r}")));
    if !s.discord.is_empty() {
        lines.push(format!(" <#71717a>Questions? <#22d3ee>{}", s.discord));
    }
    lines.push(String::new());
    ui::raw(p, &lines.join("\n"));
}

static TIP: Mutex<usize> = Mutex::new(0);

fn announce() {
    let s = conf::get();
    if !s.announce || s.announce_lines.is_empty() || horizon_bukkit::get_online_players().is_empty() {
        return;
    }
    let i = {
        let mut t = TIP.lock().unwrap_or_else(|e| e.into_inner());
        *t = (*t + 1) % s.announce_lines.len();
        *t
    };
    for p in horizon_bukkit::get_online_players() {
        ui::raw(&p, &format!("\n {}\n", s.announce_lines[i]));
    }
}

fn on_ping(e: &mut PaperServerListPingEvent) {
    let s = conf::get();
    if s.motd {
        e.set_motd_component(&ui::component(&format!("{}\n{}", s.motd1, s.motd2)));
    }
}

// ---- Joining and leaving ----

fn on_prelogin(e: &mut AsyncPlayerPreLoginEvent) {
    data::players().load(&e.get_unique_id());
}

fn on_join(e: &mut PlayerJoinEvent) {
    let p = e.get_player().clone();
    rewards::join(&p);
    teams::joined(&p);
}

fn on_quit(e: &mut PlayerQuitEvent) {
    let p = e.get_player().clone();
    let uuid = p.get_unique_id();
    combat::on_quit(&p);
    rewards::quit(&uuid);
    claims::quit(&uuid);
    warmups(|m| m.remove(&uuid));
    let d = data::get(&uuid);
    data::summarize(&uuid, &d);
    app::later(200, move || {
        if horizon_bukkit::fam_ess::get_player_by_id(&uuid).is_none() {
            data::players().unload(&uuid);
        }
    });
}

/// Every minute: claim blocks for time online, leaderboards up to date.
fn minute() {
    for p in horizon_bukkit::get_online_players() {
        if conf::get().claims {
            claims::accrue(&p);
        }
        let uuid = p.get_unique_id();
        let d = data::get(&uuid);
        data::summarize(&uuid, &d);
    }
}

/// The same command line from the same sender within 250 ms (the engine's double dispatch).
fn repeated(key: &str) -> bool {
    static LAST: Mutex<Option<(String, u64)>> = Mutex::new(None);
    let now = horizon_kit::time::now_ms();
    let mut held = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let again = held.as_ref().is_some_and(|(k, at)| k == key && now.saturating_sub(*at) < 250);
    *held = Some((key.to_owned(), now));
    again
}

// ---- Admin ----

fn admin_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("reload") => {
            conf::reload();
            shop::reload();
            crates::reload();
            ui::tell(sender, "HorizonSMP", "<#4ade80>Reloaded</#4ade80> config.yml, shop.yml and crates.yml.");
        }
        Some("setupspawn" | "buildspawn") => spawn::setup(sender, &args[1..]),
        Some("stats") => {
            ui::tell(sender, "HorizonSMP", &format!("Claims <white>{}</white> • players known <white>{}</white> • online <white>{}</white>", claims::claim_count(), data::summaries().read(|m| m.len()), horizon_bukkit::get_online_players().len()));
        }
        _ => ui::tell(sender, "HorizonSMP", "<white>/horizonsmp reload</white> <#71717a>• <white>/horizonsmp stats</white> <#71717a>• <white>/horizonsmp setupspawn [radius]</white> <#71717a>• /claim bypass • /crate give • /claim giveblocks"),
    }
    true
}

fn on_enable(plugin: Plugin) {
    app::set(plugin);
    gui::install(plugin);
    conf::reload();
    shop::reload();
    crates::reload();
    claims::load();
    teams::load();
    let _ = data::summaries();

    // A command another plugin also registers (Essentials' /sell, /worth, /playtime, /rules) reaches this one
    // twice for one line the player typed: the second within a moment is dropped.
    let cmd = |name: &str, run: fn(&CommandSender, &[&str]) -> bool| {
        let label = name.to_owned();
        plugin.set_executor(name, move |s, _, a| {
            if repeated(&format!("{}\u{0}{label}\u{0}{}", s.get_name(), a.join(" "))) {
                return true;
            }
            run(s, a)
        })
    };
    cmd("menu", menu::menu_cmd);
    cmd("claim", claims::claim_cmd);
    cmd("trust", claims::trust_entry);
    cmd("untrust", claims::untrust_entry);
    cmd("unclaim", claims::unclaim_entry);
    cmd("shop", shop::shop_cmd);
    cmd("sell", shop::sell_cmd);
    cmd("worth", shop::worth_cmd);
    cmd("crate", crates::crate_cmd);
    cmd("team", teams::team_cmd);
    cmd("tc", teams::tc_cmd);
    cmd("daily", rewards::daily_cmd);
    cmd("rewards", rewards::rewards_cmd);
    cmd("playtime", rewards::playtime_cmd);
    cmd("pvp", combat::pvp_cmd);
    cmd("bounty", bounty::bounty_cmd);
    cmd("stats", stats::stats_cmd);
    cmd("top", stats::top_cmd);
    cmd("rules", |s, _| {
        if let Some(p) = s.as_player() {
            rules(&p.clone());
        }
        true
    });
    cmd("horizonsmp", admin_cmd);
    plugin.set_tab_completer("claim", |_, _, a| claims::complete(a));
    plugin.set_tab_completer("trust", |_, _, a| claims::trust_complete(a));
    plugin.set_tab_completer("untrust", |_, _, a| claims::trust_complete(a).into_iter().take(if a.len() <= 1 { usize::MAX } else { 0 }).collect());
    plugin.set_tab_completer("team", |s, _, a| teams::complete(s, a));
    plugin.set_tab_completer("crate", |s, _, a| crates::complete(s, a));
    plugin.set_tab_completer("bounty", |_, _, a| bounty::complete(a));
    plugin.set_tab_completer("shop", |_, _, a| shop::complete(a));
    plugin.set_tab_completer("sell", |_, _, a| if a.len() <= 1 { ["hand", "handall", "all"].iter().filter(|s| s.starts_with(&a.first().map(|x| x.to_ascii_lowercase()).unwrap_or_default())).map(|s| (*s).to_owned()).collect() } else { Vec::new() });
    plugin.set_tab_completer("top", |_, _, a| if a.len() <= 1 { vec!["playtime".into(), "kills".into(), "mobs".into(), "mined".into(), "streak".into()] } else { Vec::new() });
    plugin.set_tab_completer("pvp", |_, _, a| if a.len() <= 1 { vec!["enable".into()] } else { Vec::new() });
    plugin.set_tab_completer("horizonsmp", |_, _, a| if a.len() <= 1 { vec!["reload".into(), "stats".into(), "setupspawn".into()] } else { Vec::new() });

    // Crates first (they take the click before claims look at it).
    crates::register(plugin);
    claims::register(plugin);
    combat::register(plugin);
    plugin.register_event_with("MONITOR", true, stats::on_break);
    plugin.register_event_with("LOWEST", false, on_prelogin);
    plugin.register_event_with("MONITOR", false, on_join);
    plugin.register_event_with("MONITOR", false, on_quit);
    plugin.register_event_with("HIGH", false, on_ping);

    hooks::provide_placeholders(&plugin, "horizonsmp", |uuid, _name, params| {
        if uuid.is_empty() {
            return None;
        }
        let d = || data::get(uuid);
        Some(match params {
            "team" => teams::team_name(uuid).unwrap_or_default(),
            "playtime" => ui::duration(d().playtime),
            "kills" => d().kills.to_string(),
            "deaths" => d().deaths.to_string(),
            "claim_blocks" => (claims::total_blocks(uuid) - claims::used_blocks(uuid)).to_string(),
            "claims" => claims::claims_of(uuid).len().to_string(),
            "bounty" => ui::money(bounty::on(uuid)),
            "streak" => d().daily_streak.to_string(),
            "combat" => if combat::tagged(uuid) { "yes".into() } else { "no".into() },
            _ => return None,
        })
    });

    // A hot reload: everyone online gets their playtime counted from now.
    for p in horizon_bukkit::get_online_players() {
        data::players().load(&p.get_unique_id());
        rewards::join_quietly(&p);
    }

    plugin.run_task_timer(1, 1, crates::tick);
    plugin.run_task_timer(5, 10, || {
        claims::watch_borders();
        combat::tick();
        warmup_tick();
    });
    plugin.run_task_timer(40, 40, crates::sparkle);
    plugin.run_task_timer(300, 300, rewards::tick);
    plugin.run_task_timer(1200, 1200, minute);
    let every = conf::get().announce_minutes.max(1) * 1200;
    plugin.run_task_timer(every, every, announce);
    horizon_bukkit::horizon_mod::log(&format!("[HorizonSMP] Ready: {} claims, {} shop categories, {} crates", claims::claim_count(), shop::category_count(), crates::all().len()));
}

fn on_disable(_plugin: Plugin) {
    for p in horizon_bukkit::get_online_players() {
        let uuid = p.get_unique_id();
        rewards::quit(&uuid);
        claims::unshow(&p);
        let d = data::get(&uuid);
        data::summarize(&uuid, &d);
    }
    gui::close_all();
    data::players().save_all();
    claims::save();
    teams::save();
    bounty::save();
    crates::save();
    data::summaries().save_now();
    horizon_kit::store::flush();
    app::clear();
}

bukkit_plugin!(on_enable, on_disable);
