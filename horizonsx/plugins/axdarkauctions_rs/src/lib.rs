//! AxDarkAuctions 1.12.1 (Artillex Studios, Apache-2.0) written natively in Rust for Horizon. The Java plugin
//! drives AxAPI (NMS packets, its own GUI, config and command libraries); here the same plugin is the SDK and
//! horizon-kit: the same files (config.yml, lang.yml, currencies.yml, schedulers.yml, guis/, auctions/), the same
//! commands, messages, GUIs, NPCs, holograms, schedules and editor.
//!
//! Everything runs on the server thread: the one-second auction clock is a server task, GUI clicks and NPC clicks
//! come back to it, and work that could fire events back into the plugin (commands, gives) runs after the
//! auction state is let go (`later`).

pub mod auction;
pub mod commands;
pub mod currency;
pub mod editor;
pub mod entities;
pub mod menus;
pub mod names;
pub mod schedule;
pub mod texts;

use horizon_bukkit::event::{PlayerCommandPreprocessEvent, PlayerQuitEvent};
use horizon_bukkit::fam_ev_player::{AsyncPlayerChatEvent, PlayerInteractEvent, PlayerUseUnknownEntityEvent};
use horizon_bukkit::{Player, Plugin, bukkit_plugin};
use horizon_kit::{Config, app, gui, packets, yaml};
use serde_yaml::Value as Yaml;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

pub const DEFAULT_CONFIG: &str = include_str!("../data/config.yml");
pub const DEFAULT_LANG: &str = include_str!("../data/lang.yml");
pub const DEFAULT_CURRENCIES: &str = include_str!("../data/currencies.yml");
pub const DEFAULT_SCHEDULERS: &str = include_str!("../data/schedulers.yml");
pub const DEFAULT_MAIN_GUI: &str = include_str!("../data/guis/main.yml");
pub const DEFAULT_PREVIEW_GUI: &str = include_str!("../data/guis/preview.yml");
pub const DEFAULT_AUCTION: &str = include_str!("../data/auctions/example.yml");

/// One bidding stage of config.yml `bidding`: from this amount up, this item and this step.
#[derive(Clone, Debug)]
pub struct Stage {
    pub from: f64,
    pub item: Yaml,
    pub increase: f64,
}

/// Every file but the auctions'.
pub struct Settings {
    pub config: Config,
    pub lang: Config,
    pub currencies_file: Config,
    pub currencies: Vec<currency::Currency>,
    pub schedulers_file: Config,
    pub schedules: schedule::Schedules,
    pub main_gui: Config,
    pub preview_gui: Config,
    pub stages: Vec<Stage>,
}

static SETTINGS: Mutex<Option<Settings>> = Mutex::new(None);

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The settings, read (`None` before enable).
pub fn settings<R>(f: impl FnOnce(&Settings) -> R) -> Option<R> {
    lock(&SETTINGS).as_ref().map(f)
}

pub fn settings_mut<R>(f: impl FnOnce(&mut Settings) -> R) -> Option<R> {
    lock(&SETTINGS).as_mut().map(f)
}

pub fn config_bool(path: &str, default: bool) -> bool {
    settings(|s| s.config.bool(path, default)).unwrap_or(default)
}

pub fn config_int(path: &str, default: i64) -> i64 {
    settings(|s| s.config.int(path, default)).unwrap_or(default)
}

/// A lang.yml line, raw.
pub fn lang(key: &str) -> String {
    settings(|s| s.lang.str(key, key)).unwrap_or_else(|| key.to_owned())
}

pub fn lang_list(key: &str) -> Vec<String> {
    settings(|s| s.lang.list(key)).unwrap_or_default()
}

pub fn prefix() -> String {
    settings(|s| s.config.str("prefix", "")).unwrap_or_default()
}

/// MessageUtils.sendLang: the prefix, the lang.yml line, the replacements.
pub fn send_lang(to: &horizon_bukkit::CommandSender, key: &str, reps: &[(&str, &str)]) {
    texts::send_to(to, &format!("{}{}", prefix(), texts::fill(&lang(key), reps)));
}

pub fn tell_lang(player: &Player, key: &str, reps: &[(&str, &str)]) {
    texts::send(player, &format!("{}{}", prefix(), texts::fill(&lang(key), reps)));
}

/// TimeUtils.formatTime.
pub fn format_time(ms: i64) -> String {
    settings(|s| {
        let units = [s.lang.str("time.second", "s"), s.lang.str("time.minute", "m"), s.lang.str("time.hour", "h"), s.lang.str("time.day", "d")];
        texts::time(ms, s.config.int("timer-format", 1), &units)
    })
    .unwrap_or_default()
}

/// Utils.getFormattedCurrency: the currency's `name` with `%price%` in `number-formatting`.
pub fn format_money(currency: &str, amount: f64) -> String {
    settings(|s| {
        let number = texts::number(amount, s.config.int("number-formatting.mode", 0), &s.config.str("number-formatting.formatted", "#,###.##"));
        let display = s.currencies.iter().find(|c| c.name.eq_ignore_ascii_case(currency)).map_or_else(|| "%price%".to_owned(), |c| c.display.clone());
        display.replace("%price%", &number)
    })
    .unwrap_or_default()
}

pub fn currency(name: &str) -> Option<currency::Currency> {
    settings(|s| s.currencies.iter().find(|c| c.name.eq_ignore_ascii_case(name)).cloned()).flatten()
}

pub fn currency_names() -> Vec<String> {
    settings(|s| s.currencies.iter().map(|c| c.name.clone()).collect()).unwrap_or_default()
}

/// BiddingUtils.getBidding: the stage with the largest start not above the amount.
pub fn stage_for(amount: f64) -> Option<Stage> {
    settings(|s| s.stages.iter().filter(|st| st.from <= amount).max_by(|a, b| a.from.total_cmp(&b.from)).or_else(|| s.stages.first()).cloned()).flatten()
}

// ---- Work after the state is let go ----

thread_local! {
    static LATER: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
}

/// Runs `f` once the current entry point (tick, command, click, event) has let the auctions go: commands,
/// gives and teleports, which can call back into the plugin.
pub fn later(f: impl FnOnce() + 'static) {
    LATER.with(|l| l.borrow_mut().push(Box::new(f)));
}

/// The end of every entry point: the deferred work, then the tick's packets.
pub fn finish() {
    loop {
        let work = LATER.with(|l| std::mem::take(&mut *l.borrow_mut()));
        if work.is_empty() {
            break;
        }
        for f in work {
            f();
        }
    }
    packets::flush();
}

// ---- Chat input (the editor's text fields) ----

type Input = Box<dyn FnOnce(&Player, String) + Send>;
static INPUTS: Mutex<Option<HashMap<String, Input>>> = Mutex::new(None);

/// The next chat line of this player goes to `f` (on the server thread) instead of the chat.
pub fn ask(player: &Player, f: impl FnOnce(&Player, String) + Send + 'static) {
    lock(&INPUTS).get_or_insert_with(HashMap::new).insert(player.get_unique_id(), Box::new(f));
}

pub fn forget_input(player: &Player) {
    if let Some(m) = lock(&INPUTS).as_mut() {
        m.remove(&player.get_unique_id());
    }
}

// ---- Files ----

/// Reads every file again (`/axda reload`, enable): config.yml, lang.yml, currencies.yml, schedulers.yml, the
/// two GUIs, the bidding stages; then the auctions and the item names.
pub fn reload_files() {
    let folder = app::folder();
    let config = Config::load(&folder, "config.yml", DEFAULT_CONFIG);
    let lang = Config::load(&folder, "lang.yml", DEFAULT_LANG);
    let currencies_file = Config::load(&folder, "currencies.yml", DEFAULT_CURRENCIES);
    let schedulers_file = Config::load(&folder.clone(), "schedulers.yml", DEFAULT_SCHEDULERS);
    let main_gui = Config::load(&folder.join("guis"), "main.yml", DEFAULT_MAIN_GUI);
    let preview_gui = Config::load(&folder.join("guis"), "preview.yml", DEFAULT_PREVIEW_GUI);
    let currencies = currency::load(&currencies_file.doc);
    let schedules = schedule::load(&schedulers_file.doc);
    let mut stages = Vec::new();
    if let Some(Yaml::Mapping(m)) = config.at("bidding") {
        for (k, v) in m {
            let Some(from) = yaml::key_str(k).and_then(|k| k.trim().parse::<f64>().ok()) else { continue };
            stages.push(Stage { from, item: v.clone(), increase: yaml::num(v, "increase", 100.0) });
        }
    }
    stages.sort_by(|a, b| a.from.total_cmp(&b.from));
    names::reload(folder.clone(), &config.str("language", "en_US"));
    *lock(&SETTINGS) = Some(Settings { config, lang, currencies_file, currencies, schedulers_file, schedules, main_gui, preview_gui, stages });
    // The first start writes the example auction, as the jar's resources are copied out.
    let auctions = folder.join("auctions");
    if !auctions.exists() {
        let _ = std::fs::create_dir_all(&auctions);
        let _ = std::fs::write(auctions.join("example.yml"), DEFAULT_AUCTION);
    }
    auction::reload_all();
}

/// Saves schedulers.yml and reads the schedules again (the scheduler editor).
pub fn save_schedulers(change: impl FnOnce(&mut Config)) {
    settings_mut(|s| {
        change(&mut s.schedulers_file);
        s.schedulers_file.save();
        s.schedules = schedule::load(&s.schedulers_file.doc);
    });
}

// ---- Events ----

/// Who in an auction holds `axdarkauctions.admin`, asked on the server thread (at join and once a second): the
/// command and interact events run where the engine can't answer a player's permissions, and a permission
/// asked there reads as false for an operator.
static ADMINS: Mutex<Option<std::collections::HashSet<String>>> = Mutex::new(None);

pub fn refresh_admin(p: &Player) {
    let admin = p.has_permission("axdarkauctions.admin");
    let mut held = lock(&ADMINS);
    let set = held.get_or_insert_with(Default::default);
    if admin {
        set.insert(p.get_unique_id());
    } else {
        set.remove(&p.get_unique_id());
    }
}

fn is_admin(p: &Player) -> bool {
    lock(&ADMINS).as_ref().is_some_and(|s| s.contains(&p.get_unique_id()))
}

fn register_events(plugin: &Plugin) {
    plugin.register_event(|e: &mut PlayerInteractEvent| {
        let p = e.get_player().clone();
        if !is_admin(&p) && auction::in_any(&p) {
            e.set_cancelled(true);
            tell_lang(&p, "errors.disallowed", &[]);
        }
        finish();
    });
    plugin.register_event(|e: &mut PlayerCommandPreprocessEvent| {
        let p = e.get_player().clone();
        if is_admin(&p) {
            return;
        }
        let message = e.get_message().to_owned();
        let aliases = settings(|s| s.config.list("command-aliases")).unwrap_or_default();
        if aliases.iter().any(|a| message.starts_with(&format!("/{a}"))) {
            return;
        }
        if auction::in_any(&p) {
            e.set_cancelled(true);
            tell_lang(&p, "errors.disallowed", &[]);
        }
        finish();
    });
    plugin.register_event(|e: &mut PlayerQuitEvent| {
        let p = e.get_player().clone();
        auction::leave_everywhere(&p);
        forget_input(&p);
        menus::forget(&p);
        finish();
    });
    plugin.register_event_with("LOWEST", true, |e: &mut AsyncPlayerChatEvent| {
        let p = e.get_player().clone();
        let taken = lock(&INPUTS).as_mut().and_then(|m| m.remove(&p.get_unique_id()));
        if let Some(f) = taken {
            let line = e.get_message().to_owned();
            texts::send(&p, &format!("&f{line}"));
            e.set_cancelled(true);
            let mut job = Some((f, p, line));
            app::later(0, move || {
                if let Some((f, p, line)) = job.take() {
                    f(&p, line);
                    finish();
                }
            });
        }
    });
    plugin.register_event(|e: &mut PlayerUseUnknownEntityEvent| {
        if !e.get_hand().eq_ignore_ascii_case("HAND") {
            return;
        }
        let id = e.get_entity_id();
        if let Some(p) = e.get_player() {
            entities::clicked(&p, id);
        }
        finish();
    });
}

fn on_enable(plugin: Plugin) {
    app::set(plugin);
    gui::install(plugin);
    let _ = horizon_kit::wire::ids();
    reload_files();
    commands::register(&plugin);
    register_events(&plugin);
    // AuctionTicker: every auction's clock, once a second; the schedules each minute.
    plugin.run_task_timer(20, 20, || {
        auction::tick_all();
        auction::check_schedules();
        finish();
    });
    horizon_kit::hooks::provide_placeholders(&plugin, "axdarkauctions", |key, _uuid, _name| auction::placeholder(key));
    texts::console("&#D200FF[AxDarkAuctions] Loaded plugin!");
}

fn on_disable(_plugin: Plugin) {
    auction::stop_all();
    gui::close_all();
    finish();
    app::clear();
}

bukkit_plugin!(on_enable, on_disable);
