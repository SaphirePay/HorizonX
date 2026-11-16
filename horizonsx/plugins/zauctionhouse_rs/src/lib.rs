//! zAuctionHouse 4 rewritten in Rust for Horizon: `/ah` and its sub-commands, the auction inventories from the
//! same zMenu-format files, selling with prices like `1.5k`, taxes, listing limits by permission, expiry, sales
//! history, money kept for offline sellers, the same messages.yml. Files are read on background threads; the
//! data is in memory and written to `storage.json` by its own thread (the Java plugin's SQLite/MySQL data is not
//! read). The Java it follows is in java/.

pub mod conf;
pub mod gui;
pub mod ops;
pub mod store;
pub mod text;
pub mod yaml;

use conf::Conf;
use gui::{InvDef, Nav, PState};
use horizon_bukkit::fam_ev_player::{AsyncPlayerChatEvent, InventoryCloseEvent, InventoryDragEvent};
use horizon_bukkit::prelude::*;
use horizon_bukkit::{CommandSender, Player, Plugin, bukkit_plugin};
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static CONF: Mutex<Option<Arc<Conf>>> = Mutex::new(None);
static INVS: Mutex<Option<Arc<HashMap<String, Arc<InvDef>>>>> = Mutex::new(None);
static STATES: Mutex<Option<HashMap<String, PState>>> = Mutex::new(None);
/// Players whose next chat line is a search.
pub static AWAITING: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
static COOLDOWN: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

type MainJob = Box<dyn FnOnce() + Send>;
static MAIN: Mutex<Vec<MainJob>> = Mutex::new(Vec::new());
static MAIN_PENDING: AtomicBool = AtomicBool::new(false);
static REFRESH: AtomicBool = AtomicBool::new(false);

pub fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn conf() -> Arc<Conf> {
    CONF.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

pub fn inventory(name: &str) -> Option<Arc<InvDef>> {
    INVS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(&gui::file_of(name)).cloned()
}

pub fn states<R>(f: impl FnOnce(&mut HashMap<String, PState>) -> R) -> R {
    f(STATES.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new))
}

/// Runs `f` on the server thread at the start of the next tick.
pub fn on_main(f: impl FnOnce() + Send + 'static) {
    MAIN.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(f));
    MAIN_PENDING.store(true, Ordering::Release);
}

fn drain_main() {
    if MAIN_PENDING.swap(false, Ordering::AcqRel) {
        let jobs = std::mem::take(&mut *MAIN.lock().unwrap_or_else(|e| e.into_inner()));
        for job in jobs {
            job();
        }
    }
}

/// `update-inventory-on-action`: everyone looking at the market sees a change next tick (once per tick).
pub fn refresh_viewers() {
    if REFRESH.swap(true, Ordering::AcqRel) {
        return;
    }
    on_main(|| {
        REFRESH.store(false, Ordering::Release);
        let looking: Vec<String> = states(|s| s.iter().filter(|(_, ps)| ps.view.as_ref().is_some_and(|v| matches!(v.nav.inv.as_str(), "auction" | "selling-items" | "purchase-confirm" | "remove-confirm"))).map(|(u, _)| u.clone()).collect());
        if looking.is_empty() {
            return;
        }
        for p in horizon_bukkit::get_online_players() {
            if looking.contains(&p.get_unique_id())
                && let Some(nav) = states(|s| s.get(&p.get_unique_id()).and_then(|ps| ps.view.as_ref().map(|v| v.nav.clone())))
            {
                // A confirm window whose item is gone goes back.
                if nav.target.is_some_and(|id| !store::with(|d| d.listed.iter().any(|l| l.id == id))) {
                    continue;
                }
                ops::open(&p, nav, false);
            }
        }
    });
}

/// config + inventories read on a background thread, swapped in on the server thread.
fn reload(then: impl FnOnce() + Send + 'static) {
    let Some(p) = plugin() else { return };
    let folder = p.get_data_folder();
    std::thread::spawn(move || {
        let conf = conf::load(&folder);
        let invs = gui::load_all(&folder);
        on_main(move || {
            *CONF.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(conf));
            *INVS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(invs));
            horizon_kit::hooks::refresh();
            then();
        });
    });
}

// ---- /ah ----

fn syntax_help(s: &CommandSender) {
    let conf = conf();
    let rows = [
        ("/ah", "command-description-auction"),
        ("/ah sell <price> [<amount>] [<economy>]", "command-description-auction-sell"),
        ("/ah claim", "command-description-auction-claim"),
        ("/ah search <query>", "command-description-auction-search"),
        ("/ah page <page>", "command-description-auction-page"),
        ("/ah option [<option>] [<value>]", "command-description-auction-option"),
        ("/ah admin", "command-description-auction-admin"),
    ];
    for (syntax, key) in rows {
        let description = conf.lines(key).first().cloned().unwrap_or_default();
        ops::msg_to(s, "command-syntax-help", &[("%syntax%", syntax.into()), ("%description%", description)]);
    }
}

fn need_player<'a>(s: &'a CommandSender) -> Option<&'a Player> {
    let p = s.as_player();
    if p.is_none() {
        ops::msg_to(s, "command-no-console", &[]);
    }
    p
}

fn denied(s: &CommandSender, perm: &str) -> bool {
    if !perm.is_empty() && !s.has_permission(perm) {
        ops::msg_to(s, "command-no-permission", &[]);
        return true;
    }
    false
}

/// `commands.cooldown`: a short wait between commands.
fn cooling(p: &Player) -> bool {
    if p.has_permission("zauctionhouse.cooldown.command.bypass") {
        return false;
    }
    let mut held = COOLDOWN.lock().unwrap_or_else(|e| e.into_inner());
    let map = held.get_or_insert_with(HashMap::new);
    let uuid = p.get_unique_id();
    let now = Instant::now();
    if map.get(&uuid).is_some_and(|t| now.duration_since(*t).as_millis() < 100) {
        drop(held);
        ops::msg(p, "command-cooldown", &[]);
        return true;
    }
    map.insert(uuid, now);
    false
}

/// A player by name: online first, then known offline players (uuid, name).
fn target(name: &str) -> Option<(String, String)> {
    if let Some(p) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name)) {
        return Some((p.get_unique_id(), p.get_name().to_owned()));
    }
    let off = horizon_bukkit::get_offline_player(name);
    off.get_unique_id().map(|u| (u.to_owned(), off.get_name().to_owned()))
}

fn ah(s: &CommandSender, args: &[&str]) -> bool {
    let conf = conf();
    let Some(first) = args.first().map(|a| a.to_ascii_lowercase()) else {
        if denied(s, "zauctionhouse.use") {
            return true;
        }
        match s.as_player() {
            Some(p) => ops::open_main(p, 1),
            None => syntax_help(s),
        }
        return true;
    };
    let rest: Vec<String> = args[1..].iter().map(|a| (*a).to_owned()).collect();
    if conf.sell_aliases.contains(&first) {
        let Some(p) = need_player(s) else { return true };
        if denied(s, "zauctionhouse.sell") || cooling(p) {
            return true;
        }
        ops::sell(p, &rest);
    } else if conf.claim_aliases.contains(&first) {
        let Some(p) = need_player(s) else { return true };
        if !denied(s, "zauctionhouse.use") {
            ops::claim_money(p, false);
        }
    } else if conf.search_aliases.contains(&first) {
        let Some(p) = need_player(s) else { return true };
        if denied(s, "zauctionhouse.use") {
            return true;
        }
        if rest.is_empty() {
            ops::msg(p, "command-syntax-error", &[("%syntax%", "/ah search <query>".into())]);
        } else {
            ops::search(p, &rest.join(" "));
        }
    } else if conf.page_aliases.contains(&first) {
        let Some(p) = need_player(s) else { return true };
        if denied(s, "zauctionhouse.use") {
            return true;
        }
        match rest.first().and_then(|x| x.parse::<usize>().ok()) {
            Some(n) => ops::open_main(p, n.max(1)),
            None => ops::msg(p, "command-syntax-error", &[("%syntax%", "/ah page <page>".into())]),
        }
    } else if let Some(cmd) = conf.inventory_commands.iter().find(|c| c.aliases.contains(&first)) {
        let Some(p) = need_player(s) else { return true };
        if denied(s, &cmd.permission) {
            return true;
        }
        states(|st| {
            if let Some(ps) = st.get_mut(&p.get_unique_id()) {
                ps.stack.clear();
                ps.view = None;
            }
        });
        ops::open(p, Nav { inv: gui::file_of(&cmd.inventory), page: 1, ..Nav::default() }, false);
    } else if matches!(first.as_str(), "option" | "options" | "opt") {
        let Some(p) = need_player(s) else { return true };
        if !denied(s, "zauctionhouse.option") {
            option(p, &rest);
        }
    } else if first == "admin" {
        if !denied(s, "zauctionhouse.admin") {
            admin(s, &rest);
        }
    } else {
        syntax_help(s);
    }
    true
}

const OPTIONS: [(&str, &str, &str); 2] = [("broadcast_sell", "option-broadcast-sell-enabled", "option-broadcast-sell-disabled"), ("broadcast_purchase", "option-broadcast-purchase-enabled", "option-broadcast-purchase-disabled")];

fn option(p: &Player, args: &[String]) {
    let uuid = p.get_unique_id();
    let current = |key: &str| store::with(|d| d.options.get(&uuid).and_then(|o| o.get(key).copied())).unwrap_or(true);
    let Some(name) = args.first() else {
        ops::msg(p, "option-list-header", &[]);
        for (key, _, _) in OPTIONS {
            ops::msg(p, "option-list-entry", &[("%option%", key.into()), ("%value%", current(key).to_string())]);
        }
        return;
    };
    let Some((key, on, off)) = OPTIONS.iter().find(|(k, _, _)| k.eq_ignore_ascii_case(name)) else {
        return ops::msg(p, "command-syntax-error", &[("%syntax%", "/ah option [broadcast_sell|broadcast_purchase] [true|false]".into())]);
    };
    let value = match args.get(1).map(|v| v.to_ascii_lowercase()) {
        Some(v) if v == "true" => true,
        Some(v) if v == "false" => false,
        _ => !current(key),
    };
    store::with(|d| d.options.entry(uuid.clone()).or_default().insert((*key).to_owned(), value));
    store::changed();
    ops::msg(p, if value { on } else { off }, &[]);
}

fn admin(s: &CommandSender, args: &[String]) {
    let sub = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let rows = [
        ("/ah admin reload", "command-description-auction-reload"),
        ("/ah admin open <player> <listed|expired|purchased>", "command-description-auction-admin-open"),
        ("/ah admin forceopen <player> <inventory> [<page>]", "command-description-auction-admin-forceopen"),
        ("/ah admin history [<player>]", "command-description-auction-admin-history"),
        ("/ah admin add <player> <listed|expired|purchased> [<price>]", "command-description-auction-admin-add"),
    ];
    match sub.as_str() {
        "reload" | "rl" => {
            if denied(s, "zauctionhouse.reload") {
                return;
            }
            let s = s.clone();
            reload(move || ops::msg_to(&s, "reload-success", &[]));
        }
        "open" => {
            if denied(s, "zauctionhouse.admin.items") {
                return;
            }
            let Some(p) = need_player(s) else { return };
            let (Some(name), Some(kind)) = (args.get(1), args.get(2)) else { return ops::msg(p, "command-syntax-error", &[("%syntax%", rows[1].0.into())]) };
            let Some(t) = target(name) else { return ops::msg(p, "admin-target-not-found", &[("%target%", name.clone())]) };
            let inv = match kind.to_ascii_lowercase().as_str() {
                "expired" => "expired-items",
                "purchased" => "purchased-items",
                _ => "selling-items",
            };
            ops::msg(p, "admin-open-inventory", &[("%type%", kind.clone()), ("%target%", t.1.clone())]);
            ops::open(p, Nav { inv: inv.into(), page: 1, target: None, admin: Some(t) }, false);
        }
        "history" => {
            if denied(s, "zauctionhouse.admin.items") {
                return;
            }
            let Some(p) = need_player(s) else { return };
            let t = match args.get(1) {
                Some(name) => match target(name) {
                    Some(t) => t,
                    None => return ops::msg(p, "admin-target-not-found", &[("%target%", name.clone())]),
                },
                None => (p.get_unique_id(), p.get_name().to_owned()),
            };
            ops::msg(p, "admin-open-history", &[("%target%", t.1.clone())]);
            ops::open(p, Nav { inv: "history".into(), page: 1, target: None, admin: Some(t) }, false);
        }
        "forceopen" => {
            let (Some(name), Some(inv)) = (args.get(1), args.get(2)) else { return ops::msg_to(s, "command-syntax-error", &[("%syntax%", rows[2].0.into())]) };
            let Some(p) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name)) else {
                return ops::msg_to(s, "admin-target-not-found", &[("%target%", name.clone())]);
            };
            let page = args.get(3).and_then(|x| x.parse::<usize>().ok()).unwrap_or(1).max(1);
            ops::msg_to(s, "admin-forceopen-inventory", &[("%inventory%", inv.clone()), ("%target%", p.get_name().to_owned()), ("%page%", page.to_string())]);
            ops::open(&p, Nav { inv: gui::file_of(inv), page, ..Nav::default() }, false);
        }
        "add" => {
            if denied(s, "zauctionhouse.admin.items") {
                return;
            }
            let Some(p) = need_player(s) else { return };
            let (Some(name), Some(kind)) = (args.get(1), args.get(2)) else { return ops::msg(p, "command-syntax-error", &[("%syntax%", rows[4].0.into())]) };
            let Some(t) = target(name) else { return ops::msg(p, "admin-target-not-found", &[("%target%", name.clone())]) };
            let price = args.get(3).and_then(|x| conf().parse_price(x)).unwrap_or(1000.0);
            ops::admin_add(p, t, &kind.to_ascii_lowercase(), price);
        }
        "generate" | "migrate" | "cache" | "logs" | "option" => {
            s.send_message("§cThis admin tool is not part of the Rust zAuctionHouse.");
        }
        _ => {
            let conf = conf();
            for (syntax, key) in rows {
                let description = conf.lines(key).first().cloned().unwrap_or_default();
                ops::msg_to(s, "command-syntax-help", &[("%syntax%", syntax.into()), ("%description%", description)]);
            }
        }
    }
}

fn complete(s: &CommandSender, args: &[&str]) -> Vec<String> {
    let conf = conf();
    let starts = |list: Vec<String>, typed: &str| -> Vec<String> {
        let t = typed.to_ascii_lowercase();
        list.into_iter().filter(|x| x.to_ascii_lowercase().starts_with(&t)).collect()
    };
    let names = || horizon_bukkit::get_online_players().into_iter().map(|p| p.get_name().to_owned()).collect::<Vec<_>>();
    match args {
        [] => Vec::new(),
        [first] => {
            let mut subs = vec![conf.sell_aliases.first().cloned().unwrap_or_else(|| "sell".into()), conf.claim_aliases.first().cloned().unwrap_or_else(|| "claim".into()), conf.search_aliases.first().cloned().unwrap_or_else(|| "search".into()), conf.page_aliases.first().cloned().unwrap_or_else(|| "page".into()), "option".into()];
            for c in &conf.inventory_commands {
                if c.permission.is_empty() || s.has_permission(&c.permission) {
                    subs.extend(c.aliases.first().cloned());
                }
            }
            if s.has_permission("zauctionhouse.admin") {
                subs.push("admin".into());
            }
            starts(subs, first)
        }
        [sub, rest @ ..] => {
            let sub = sub.to_ascii_lowercase();
            if conf.sell_aliases.contains(&sub) {
                return match rest.len() {
                    1 => starts(["1000", "10000", "25000"].map(String::from).to_vec(), rest[0]),
                    2 => {
                        let max = s.as_player().and_then(|p| p.get_inventory().get_item_in_main_hand()).map_or(64, |i| i.get_max_stack_size());
                        starts(vec!["1".into(), max.to_string()], rest[1])
                    }
                    3 => starts(conf.economies.iter().map(|e| e.name.clone()).collect(), rest[2]),
                    _ => Vec::new(),
                };
            }
            match (sub.as_str(), rest) {
                ("option" | "options" | "opt", [o]) => starts(OPTIONS.iter().map(|(k, _, _)| (*k).to_owned()).collect(), o),
                ("option" | "options" | "opt", [_, v]) => starts(vec!["true".into(), "false".into()], v),
                ("admin", [a]) => starts(["reload", "open", "forceopen", "history", "add"].map(String::from).to_vec(), a),
                ("admin", [_, p]) => starts(names(), p),
                ("admin", [a, _, k]) if matches!(a.to_ascii_lowercase().as_str(), "open" | "add") => starts(["listed", "expired", "purchased"].map(String::from).to_vec(), k),
                ("admin", [a, _, k]) if a.eq_ignore_ascii_case("forceopen") => {
                    starts(INVS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|m| m.keys().cloned().collect()).unwrap_or_default(), k)
                }
                _ => Vec::new(),
            }
        }
    }
}

/// `%zauctionhouse_..%`.
fn placeholder(uuid: &str, params: &str) -> Option<String> {
    let conf = conf();
    let p = params.to_ascii_lowercase();
    let ps = |f: &dyn Fn(&PState) -> String| states(|s| s.get(uuid).map(f));
    Some(match p.as_str() {
        "listed_items" => store::with(|d| d.listed.len()).to_string(),
        "sorting_name" => {
            let sort = ps(&|x| x.sort.clone()).unwrap_or_else(|| conf.default_sort.clone());
            conf.sort_names.get(&sort).cloned().unwrap_or(sort)
        }
        "category_id" => ps(&|x| x.category.clone()).unwrap_or_else(|| "all".into()),
        "category_name" => {
            let id = ps(&|x| x.category.clone()).unwrap_or_else(|| "all".into());
            if id == "all" { conf.all_category_name.clone() } else { conf.categories.iter().find(|c| c.id == id).map_or(id, |c| c.display.clone()) }
        }
        "search_query" => ps(&|x| x.search.clone().unwrap_or_default()).unwrap_or_default(),
        "search_active" => ps(&|x| x.search.is_some().to_string()).unwrap_or_else(|| "false".into()),
        "pending_money" => conf.format_price(&conf.default_economy, ops::pending_total(uuid)),
        "pending_money_raw" => format!("{:.2}", ops::pending_total(uuid)),
        "has_pending_money" => (ops::pending_total(uuid) > 0.0).to_string(),
        "expired" | "expired_items" => store::with(|d| d.expired.iter().filter(|l| l.seller == uuid).count()).to_string(),
        "purchased" | "purchased_items" => store::with(|d| d.purchased.iter().filter(|l| l.buyer.as_deref() == Some(uuid)).count()).to_string(),
        "selling" | "selling_items" => store::with(|d| d.listed.iter().filter(|l| l.seller == uuid).count()).to_string(),
        _ => {
            if let Some(cat) = p.strip_prefix("category_count_") {
                return Some(ops::category_counts(&conf).get(cat).copied().unwrap_or(0).to_string());
            }
            if let Some(eco) = p.strip_prefix("pending_money_") {
                let v = store::with(|d| d.pending.get(uuid).and_then(|m| m.get(eco).copied())).unwrap_or(0.0);
                return Some(conf.format_price(eco, v));
            }
            if p.starts_with("max_items_") {
                let player = horizon_bukkit::get_online_players().into_iter().find(|x| x.get_unique_id() == uuid)?;
                return Some(ops::limit(&player, &conf).to_string());
            }
            return None;
        }
    })
}

fn on_close(event: &mut InventoryCloseEvent) {
    let Some(player) = event.get_player() else { return };
    let gui = event.get_inventory().gui().cloned();
    states(|s| {
        if let Some(ps) = s.get_mut(&player.get_unique_id())
            && ps.view.as_ref().is_some_and(|v| gui.as_ref().is_none_or(|g| g.same_as(&v.inv)))
        {
            ps.view = None;
            ps.stack.clear();
        }
    });
}

fn on_drag(event: &mut InventoryDragEvent) {
    let Some(player) = event.get_who_clicked() else { return };
    let gui = event.get_inventory().gui().cloned();
    if states(|s| s.get(&player.get_unique_id()).and_then(|ps| ps.view.as_ref()).is_some_and(|v| gui.as_ref().is_some_and(|g| g.same_as(&v.inv)))) {
        event.set_cancelled(true);
    }
}

fn on_enable(p: Plugin) {
    let started = Instant::now();
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    p.set_executor("zauctionhouse", |s, _, a| ah(s, a));
    p.set_tab_completer("zauctionhouse", |s, _, a| complete(s, a));
    p.register_event_with("NORMAL", false, gui::on_click);
    p.register_event_with("NORMAL", false, on_drag);
    p.register_event_with("MONITOR", false, on_close);
    p.register_event_with("LOWEST", false, |e: &mut AsyncPlayerChatEvent| {
        let uuid = e.get_player().get_unique_id();
        if AWAITING.lock().unwrap_or_else(|e| e.into_inner()).remove(&uuid) {
            e.set_cancelled(true);
            let player = e.get_player().clone();
            let message = e.get_message().to_owned();
            on_main(move || ops::search(&player, &message));
        }
    });
    p.register_event(|e: &mut PlayerJoinEvent| {
        let player = e.get_player().clone();
        let delay = conf().auto_claim_delay.min(conf().sales_notification_delay);
        if let Some(pl) = plugin() {
            let mut once = Some(player);
            pl.run_task_later(delay, move || {
                if let Some(player) = once.take()
                    && player.is_online()
                {
                    ops::on_join(&player);
                }
            });
        }
    });
    p.register_event(|e: &mut PlayerQuitEvent| {
        let uuid = e.get_player().get_unique_id();
        states(|s| s.remove(&uuid));
        AWAITING.lock().unwrap_or_else(|e| e.into_inner()).remove(&uuid);
    });
    horizon_kit::hooks::provide_placeholders(&p, "zauctionhouse", |uuid, _, params| placeholder(uuid, params));
    p.run_task_timer(1, 1, drain_main);
    let folder = p.get_data_folder();
    std::thread::spawn(move || {
        let data = store::load(folder.join("storage.json"));
        on_main(move || {
            store::install(data);
            store::start_saver();
            reload(move || {
                // Categories of what was stored (a block check per material, cached by the game).
                let conf = conf();
                store::with(|d| {
                    for l in d.listed.iter_mut() {
                        l.categories = ops::categorize(&conf, &l.item);
                    }
                });
                let sweep = conf.sweep_seconds * 20;
                if let Some(pl) = plugin() {
                    pl.run_task_timer(sweep, sweep, ops::sweep);
                    let n = store::with(|d| d.listed.len());
                    pl.info(&format!("{n} listed items loaded. Enabled in {} ms (Rust).", started.elapsed().as_millis()));
                }
            });
        });
    });
}

fn on_disable(_p: Plugin) {
    for player in horizon_bukkit::get_online_players() {
        if states(|s| s.get(&player.get_unique_id()).is_some_and(|ps| ps.view.is_some())) {
            player.close_inventory();
        }
    }
    store::stop();
    MAIN.lock().unwrap_or_else(|e| e.into_inner()).clear();
    *STATES.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
