//! Tebex 2.4.2 for Horizon, written again in Rust (the Java it follows is in java/).
//!
//! Same commands (`/tebex ...`, `/buy`), same `plugins/Tebex/config.yml`, same store API calls. What changed is
//! where the work runs:
//!
//! - Every request to Tebex goes out on one of two HTTP threads (`queue`: the command queue, listings, events;
//!   `ask`: what a player waits for, a checkout link or a command's answer), never on the server thread.
//! - The server thread does only game work: who is online, inventory space, running the bought commands, the
//!   shop menus. Their results come back through `on_main`, drained once a tick (an atomic check when idle).
//! - A join is a map lookup (and, with a store connected, one address query): no futures, no reflection, no
//!   Java bridge. The converted plugin held every join about 10 s.
//! - Done commands are deleted in one request per tick, not one per command.

mod api;
mod gui;
mod util;

use api::{Client, Fail, QueuedCommand, QueuedPlayer, ServerInfo};
use horizon_bukkit::event::{PlayerJoinEvent, PlayerQuitEvent};
use horizon_bukkit::{CommandSender, Player, Plugin, bukkit_plugin};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

// ---- State ----

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static STATE: Mutex<Option<State>> = Mutex::new(None);
static VERBOSE: AtomicBool = AtomicBool::new(false);
static ONLINE_MODE: AtomicBool = AtomicBool::new(true);
/// Only the newest scheduled queue check runs (Java's chain of `executeAsyncLater` can double up).
static CHECK_GEN: AtomicU64 = AtomicU64::new(0);

pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub secret: String,
    pub buy_enabled: bool,
    pub buy_name: String,
    pub verbose: bool,
    pub proxy: bool,
    pub auto_report: bool,
    /// The whole file, for the `gui:` settings.
    pub yaml: serde_yaml::Value,
}

/// Who a queued purchase is for, as `getPlayerId` decides: the UUID on an online-mode server, else the name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PlayerKey {
    Uuid(String),
    Name(String),
}

impl PlayerKey {
    fn of(name: &str, uuid: Option<&str>) -> Self {
        let geyser = state(|s| s.info.as_ref().is_some_and(|i| i.store.game_type.to_ascii_lowercase().contains("geyser")));
        match uuid.map(util::dashed) {
            Some(uuid) if is_online_mode() && !geyser && uuid != util::EMPTY_UUID => Self::Uuid(uuid),
            _ => Self::Name(name.to_ascii_lowercase()),
        }
    }
    fn find(&self) -> Option<Player> {
        match self {
            Self::Uuid(uuid) => horizon_bukkit::fam_ess::get_player_by_id(uuid),
            Self::Name(name) => horizon_bukkit::get_player(name).filter(|p| p.get_name().eq_ignore_ascii_case(name)),
        }
    }
}

pub struct State {
    pub cfg: Settings,
    pub setup: bool,
    pub info: Option<ServerInfo>,
    pub categories: Option<Arc<Vec<api::Category>>>,
    queued: HashMap<PlayerKey, i64>,
    joins: Vec<Value>,
    events: Vec<Value>,
    checking: bool,
    deletes: Vec<i64>,
    pub menus: HashMap<String, gui::Menu>,
}

/// The state, briefly. Never held across a game call or a request.
pub fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut held: MutexGuard<'_, Option<State>> = STATE.lock().unwrap_or_else(|e| e.into_inner());
    match held.as_mut() {
        Some(s) => f(s),
        None => f(&mut State::empty()),
    }
}

impl State {
    fn empty() -> Self {
        Self {
            cfg: load_settings(None),
            setup: false,
            info: None,
            categories: None,
            queued: HashMap::new(),
            joins: Vec::new(),
            events: Vec::new(),
            checking: false,
            deletes: Vec::new(),
            menus: HashMap::new(),
        }
    }
}

fn is_online_mode() -> bool {
    ONLINE_MODE.load(Ordering::Relaxed) || state(|s| s.cfg.proxy)
}

fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

fn secret() -> String {
    state(|s| s.cfg.secret.clone())
}

// ---- Logs and plugin events (Tebex's triage reports) ----

#[derive(Clone, Copy, Debug)]
pub enum Level {
    Info,
    Warning,
    Error,
}

fn log(level: Level, msg: &str) {
    let Some(p) = plugin() else { return };
    match level {
        Level::Info => p.info(msg),
        Level::Warning => p.warning(msg),
        Level::Error => p.severe(msg),
    }
}

/// A log line from an HTTP thread: written on the server thread.
pub fn post_log(level: Level, msg: String) {
    on_main(move || log(level, &msg));
}

fn info(msg: &str) {
    log(Level::Info, msg);
}

fn debug(msg: &str) {
    if verbose() {
        info(&format!("[DEBUG] {msg}"));
    }
}

/// A debug line from an HTTP thread.
fn post_debug(msg: String) {
    if verbose() {
        post_log(Level::Info, format!("[DEBUG] {msg}"));
    }
}

/// `warning(message, solution)`: two lines, and a report to Tebex when `auto-report-enabled`.
fn warning(msg: &str, solution: &str) {
    log(Level::Warning, msg);
    log(Level::Warning, &format!("- {solution}"));
    plugin_event("WARNING", msg);
}

fn error(msg: &str) {
    log(Level::Error, msg);
    plugin_event("ERROR", msg);
}

fn plugin_event(level: &str, msg: &str) {
    let plugin_version = plugin().map(|p| p.get_version().to_owned()).unwrap_or_default();
    let mc = mc_version();
    state(|s| {
        if !s.cfg.auto_report || s.events.len() >= 500 {
            return;
        }
        let mut event = json!({
            "game_id": "Minecraft", "framework_id": "Horizon", "runtime_version": "Rust",
            "framework_version": mc, "plugin_version": plugin_version,
            "event_message": msg, "event_level": level, "trace": "",
        });
        if s.setup
            && let Some(i) = &s.info
        {
            event["server_id"] = json!(i.server_id.to_string());
            event["store_id"] = json!(i.store.id.to_string());
            event["store_name"] = json!(i.store.name);
        }
        s.events.push(event);
    });
}

// ---- Threads ----

type MainJob = Box<dyn FnOnce() + Send>;
static MAIN: Mutex<Vec<MainJob>> = Mutex::new(Vec::new());
static MAIN_PENDING: AtomicBool = AtomicBool::new(false);

/// Runs `f` on the server thread, at the start of the next tick.
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
    if HOLDING.load(Ordering::Relaxed) && IN_FLIGHT.load(Ordering::Acquire) == 0 && !MAIN_PENDING.load(Ordering::Acquire) {
        hold_ticks(false);
    }
}

/// Requests sent and not yet answered.
static IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static HOLDING: AtomicBool = AtomicBool::new(false);

/// An empty server pauses its ticks, and answers wait for a tick: while a request is out, pausing is held off
/// (Paper's `Server.allowPausing`), so `/tebex secret` from the console of an empty server still answers.
fn hold_ticks(hold: bool) {
    if HOLDING.swap(hold, Ordering::AcqRel) != hold
        && let Some(p) = plugin()
    {
        horizon_bukkit::fam_core::allow_pausing(&p, !hold);
    }
}

type Job = Box<dyn FnOnce(&Client) + Send>;

enum Msg {
    Run(Job),
    At(Instant, Job),
    Stop,
}

/// An HTTP thread with its own queue: jobs run in order, timed ones when due.
struct Lane {
    tx: Mutex<Option<Sender<Msg>>>,
    done: Mutex<Option<Receiver<()>>>,
}

static QUEUE: Lane = Lane::new();
static ASK: Lane = Lane::new();

impl Lane {
    const fn new() -> Self {
        Self { tx: Mutex::new(None), done: Mutex::new(None) }
    }
    fn start(&self, name: &str, client: Client) {
        let (tx, rx) = mpsc::channel::<Msg>();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let _ = std::thread::Builder::new().name(format!("Tebex {name}")).spawn(move || {
            let mut timed: Vec<(Instant, Job)> = Vec::new();
            loop {
                let now = Instant::now();
                while let Some(i) = timed.iter().position(|(at, _)| *at <= now) {
                    let (_, job) = timed.swap_remove(i);
                    run_job(&client, job);
                }
                let wait = timed.iter().map(|(at, _)| at.saturating_duration_since(now)).min().unwrap_or(Duration::from_secs(3600));
                match rx.recv_timeout(wait) {
                    Ok(Msg::Run(job)) => {
                        run_job(&client, job);
                        IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
                    }
                    Ok(Msg::At(at, job)) => timed.push((at, job)),
                    Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {}
                }
            }
            let _ = done_tx.send(());
        });
        *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        *self.done.lock().unwrap_or_else(|e| e.into_inner()) = Some(done_rx);
    }
    /// A request now (called on the server thread only: it may hold pausing off).
    fn run(&self, job: impl FnOnce(&Client) + Send + 'static) {
        if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            IN_FLIGHT.fetch_add(1, Ordering::AcqRel);
            if tx.send(Msg::Run(Box::new(job))).is_err() {
                IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
                return;
            }
            hold_ticks(true);
        }
    }
    fn after(&self, delay: Duration, job: impl FnOnce(&Client) + Send + 'static) {
        if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(Msg::At(Instant::now() + delay, Box::new(job)));
        }
    }
    /// Stops the thread and waits for it (a request in flight ends within its timeout): the plugin's code must be
    /// gone before its library is unloaded.
    fn stop(&self) {
        if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = tx.send(Msg::Stop);
        }
        if let Some(done) = self.done.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = done.recv_timeout(Duration::from_secs(16));
        }
    }
}

fn run_job(client: &Client, job: Job) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(client))).is_err() {
        post_log(Level::Error, "a Tebex request failed unexpectedly".into());
    }
}

// ---- Config ----

const DEFAULT_CONFIG: &str = include_str!("../config.yml");

fn config_path() -> Option<std::path::PathBuf> {
    plugin().map(|p| p.get_data_folder().join("config.yml"))
}

fn load_settings(text: Option<&str>) -> Settings {
    let yaml: serde_yaml::Value = text.and_then(|t| serde_yaml::from_str(t).ok()).unwrap_or_else(|| serde_yaml::from_str(DEFAULT_CONFIG).unwrap_or_default());
    let flag = |v: &serde_yaml::Value, default: bool| v.as_bool().unwrap_or(default);
    let mut secret = yaml["server"]["secret-key"].as_str().unwrap_or_default().to_owned();
    if let Ok(env) = std::env::var("TEBEX_SECRET_KEY")
        && !env.is_empty()
    {
        secret = env;
    }
    Settings {
        secret,
        buy_enabled: flag(&yaml["buy-command"]["enabled"], true),
        buy_name: yaml["buy-command"]["name"].as_str().unwrap_or("buy").to_owned(),
        verbose: flag(&yaml["verbose"], false),
        proxy: flag(&yaml["server"]["proxy"], false),
        auto_report: flag(&yaml["auto-report-enabled"], true),
        yaml,
    }
}

fn read_config() -> Settings {
    let Some(path) = config_path() else { return load_settings(None) };
    if !path.exists() {
        let _ = std::fs::write(&path, DEFAULT_CONFIG);
    }
    let text = std::fs::read_to_string(&path).ok();
    if let Some(t) = &text
        && serde_yaml::from_str::<serde_yaml::Value>(t).is_err()
    {
        warning(
            "Failed to load configuration: invalid YAML",
            "Check that your configuration is valid and in the proper format and reload the plugin. You may delete `Tebex/config.yml` and a new configuration will be generated.",
        );
    }
    load_settings(text.as_deref())
}

/// `saveConfig`: the secret and `verbose` written back, comments kept.
fn save_config(secret: &str, verbose_on: bool) {
    let Some(path) = config_path() else { return };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| DEFAULT_CONFIG.to_owned());
    let text = util::yaml_set(&text, "server.secret-key", &util::yaml_string(secret));
    let text = util::yaml_set(&text, "verbose", if verbose_on { "true" } else { "false" });
    if let Err(e) = std::fs::write(&path, text) {
        error(&format!("Failed to save configuration: {e}"));
    }
}

fn reload_config() {
    let cfg = read_config();
    VERBOSE.store(cfg.verbose, Ordering::Relaxed);
    state(|s| s.cfg = cfg);
}

/// BuycraftX's `config.properties`, moved into Tebex's config once (`migrateConfig`).
fn migrate_buycraft() {
    let old = std::path::Path::new("plugins/BuycraftX/config.properties");
    let Ok(text) = std::fs::read_to_string(old) else { return };
    info("Detected legacy BuycraftX configuration. Attempting to migrate...");
    let props: HashMap<&str, &str> = text.lines().filter(|l| !l.trim_start().starts_with('#')).filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim(), v.trim())).collect();
    let Some(key) = props.get("server-key").filter(|k| **k != "INVALID" && !k.is_empty()) else { return };
    let Some(path) = config_path() else { return };
    let mut cfg = std::fs::read_to_string(&path).unwrap_or_else(|_| DEFAULT_CONFIG.to_owned());
    if let Some(name) = props.get("buy-command-name") {
        cfg = util::yaml_set(&cfg, "buy-command.name", &util::yaml_string(name));
    }
    let disabled = props.get("disable-buy-command").is_some_and(|v| v.eq_ignore_ascii_case("true"));
    cfg = util::yaml_set(&cfg, "buy-command.enabled", if disabled { "false" } else { "true" });
    cfg = util::yaml_set(&cfg, "verbose", props.get("verbose").copied().unwrap_or("false"));
    cfg = util::yaml_set(&cfg, "server.proxy", props.get("is-bungeecord").copied().unwrap_or("false"));
    cfg = util::yaml_set(&cfg, "server.secret-key", &util::yaml_string(key));
    if std::fs::write(&path, cfg).is_ok() {
        info("Successfully migrated your config from BuycraftX.");
    }
}

// ---- The store ----

/// `initStore`: connect with the secret key, then load the listing and check the queue.
fn init_store() {
    state(|s| {
        s.queued.clear();
        s.setup = false;
    });
    let key = secret();
    if key.is_empty() {
        info("Welcome to Tebex! It seems like this is a new setup.");
        info("To get started, please use the 'tebex secret <key>' command in the console.");
        return;
    }
    QUEUE.run(move |c| match c.information(&key) {
        Ok(info) => on_main(move || connected(info)),
        Err(Fail::NotFound) => on_main(|| {
            warning("Failed to connect your server: Server not found", "Please double-check your server key or run the setup command again.");
            state(|s| s.setup = false);
        }),
        Err(e) => on_main(move || warning(&format!("Failed to retrieve server information. {e}"), "Please double check your server key or run the setup command again.")),
    });
}

fn connected(server: ServerInfo) {
    info(&format!("Connected to {} - {} server.", server.server_name, server.store.game_type));
    state(|s| {
        s.info = Some(server);
        s.setup = true;
    });
    plugin_event("INFO", "Server init");
    send_telemetry();
    refresh_listing();
    QUEUE.run(|c| check_queue(c, true));
}

fn send_telemetry() {
    let key = secret();
    let version = mc_version();
    let plugin_version = plugin().map(|p| p.get_version().to_owned()).unwrap_or_default();
    let online = is_online_mode();
    QUEUE.run(move |c| {
        let body = json!({ "server": { "platform": "BUKKIT", "platform_version": version, "online_mode": online }, "plugin": { "version": plugin_version } });
        let _ = c.send("POST", "https://plugin.buycraft.net/analytics/startup", Some(&key), Some(&body));
    });
}

static MC_VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn mc_version() -> String {
    MC_VERSION.get_or_init(|| horizon_bukkit::get_bukkit_version().split('-').next().unwrap_or("26.3").to_owned()).clone()
}

fn refresh_listing() {
    if !state(|s| s.setup) {
        return;
    }
    let key = secret();
    QUEUE.run(move |c| {
        if let Ok(list) = c.listing(&key) {
            let list = Arc::new(list);
            state(|s| s.categories = Some(list));
        }
    });
}

/// The next queue check, `seconds` from now; an earlier one still waiting is dropped.
fn schedule_check(seconds: u64) {
    let gen_now = CHECK_GEN.fetch_add(1, Ordering::AcqRel) + 1;
    QUEUE.after(Duration::from_secs(seconds), move |c| {
        if CHECK_GEN.load(Ordering::Acquire) == gen_now {
            check_queue(c, true);
        }
    });
}

/// `checkCommandQueue` (on the queue thread): who has purchases waiting.
fn check_queue(c: &Client, use_remote: bool) {
    let (setup, busy, key) = state(|s| {
        let busy = s.checking;
        if s.setup && !busy {
            s.checking = true;
            s.queued.clear();
        }
        (s.setup, busy, s.cfg.secret.clone())
    });
    if !setup {
        post_debug("Tebex is not set up. Skipping check.".into());
        schedule_check(60);
        return;
    }
    if busy {
        post_debug("Command queue check already in progress. Skipping to avoid duplicate execution.".into());
        if use_remote {
            schedule_check(60);
        }
        return;
    }
    post_debug("Checking for due players...".into());
    match c.due_players(&key) {
        Err(e) => {
            let msg = e.to_string();
            let (text, solution, wait) = match e.code() {
                Some(429) => ("Failed to get due players: Rate Limit".to_owned(), "We will try again after 5 minutes.", 300),
                Some(403) => (
                    "Failed to get due players: Forbidden".to_owned(),
                    "Please check your secret key and run `/tebex.forcecheck` to try again. We will wait 30 minutes before trying again.",
                    1800,
                ),
                _ => (format!("Failed to get due players: {msg}"), "We will try again at the next due player check.", 60),
            };
            on_main(move || warning(&text, solution));
            schedule_check(wait);
            state(|s| s.checking = false);
        }
        Ok(due) => {
            if use_remote {
                schedule_check(due.next_check.clamp(10, 3600) as u64);
            }
            let offline = due.execute_offline;
            on_main(move || {
                if !due.players.is_empty() {
                    debug(&format!("Found {} {} with pending commands.", due.players.len(), if due.players.len() == 1 { "player" } else { "players" }));
                }
                // One list of who is on, for every player in the queue.
                let online = Online::now();
                for p in due.players {
                    handle_online(p, &online);
                }
                // After every online fetch above (the same thread, in order).
                QUEUE.run(move |c| finish_check(c, offline));
            });
        }
    }
}

fn finish_check(c: &Client, offline: bool) {
    if offline && state(|s| s.setup) {
        match c.offline_commands(&secret()) {
            Ok(cmds) if !cmds.is_empty() => on_main(move || run_offline(cmds)),
            Ok(_) => {}
            Err(e) => on_main(move || {
                warning(&format!("Failed to retrieve offline commands - some commands may not have been processed. {e}"), "We will try again at the next due player check.")
            }),
        }
    }
    state(|s| s.checking = false);
}

/// Who is on, from one engine query.
struct Online {
    by_uuid: HashMap<String, Player>,
    by_name: HashMap<String, Player>,
}

impl Online {
    fn now() -> Self {
        let players = horizon_bukkit::get_online_players();
        let by_uuid = players.iter().map(|p| (p.get_unique_id(), p.clone())).collect();
        let by_name = players.into_iter().map(|p| (p.get_name().to_ascii_lowercase(), p)).collect();
        Self { by_uuid, by_name }
    }
    fn get(&self, key: &PlayerKey) -> Option<&Player> {
        match key {
            PlayerKey::Uuid(u) => self.by_uuid.get(u),
            PlayerKey::Name(n) => self.by_name.get(n),
        }
    }
}

/// `handleOnlineCommands` (server thread): fetch the commands of someone online, or remember them for their join.
fn handle_online(p: QueuedPlayer, online: &Online) {
    if !state(|s| s.setup) {
        return;
    }
    let key = PlayerKey::of(&p.name, p.uuid.as_deref());
    if online.get(&key).is_none() {
        debug(&format!("Player {} has online commands but is not connected. Skipping.", p.name));
        state(|s| s.queued.insert(key, p.id));
        return;
    }
    fetch_online(p, key);
}

fn fetch_online(p: QueuedPlayer, key: PlayerKey) {
    let secret = secret();
    post_debug(format!("Processing online commands for player '{}'...", p.name));
    QUEUE.run(move |c| match c.online_commands(&secret, &p) {
        Ok(cmds) if cmds.is_empty() => post_debug(format!("No commands found for {}.", p.name)),
        Ok(cmds) => on_main(move || run_online(&p.name, key, cmds)),
        Err(e) => on_main(move || warning(&format!("Failed to get online commands: {e}"), "We will try again at the next due player check.")),
    });
}

/// `processOnlineCommands` (server thread).
fn run_online(name: &str, key: PlayerKey, cmds: Vec<QueuedCommand>) {
    if !state(|s| s.setup) {
        return;
    }
    let Some(player) = key.find() else {
        // Gone since the check: their commands wait for their next join.
        if let Some(first) = cmds.first() {
            let id = first.player.id;
            state(|s| s.queued.insert(key, id));
        }
        return;
    };
    debug(&format!("Found {} online {}.", cmds.len(), if cmds.len() == 1 { "command" } else { "commands" }));
    // Commands run a tick later at the soonest, so the space is the same for each of them: one inventory read.
    let free = free_slots(&player);
    let mut room = true;
    for cmd in cmds {
        let parsed = cmd.parsed();
        if free < cmd.slots {
            info(&format!("Skipping command '{parsed}' for player '{name}' due to no inventory space. Free slots: {free}. Slots required: {}", cmd.slots));
            room = false;
            continue;
        }
        let (name, key2) = (name.to_owned(), key.clone());
        let run = move || {
            if key2.find().is_none() {
                info(&format!("Skipping command '{parsed}' for player '{name}' because they are no longer online"));
                return;
            }
            info(&format!("Dispatching command '{parsed}' for player '{name}'"));
            if !horizon_bukkit::dispatch_command(&parsed) {
                let mut solution = format!("Manually try `{parsed}` for player {name}. Check that the command syntax is correct.");
                if cmd.payment != 0 {
                    solution.push_str(&format!(" Re-run this command at https://creator.tebex.io/payments/{}", cmd.payment));
                }
                warning("Command failed to execute: No further information", &solution);
            }
            command_done(cmd.id);
        };
        later(cmd.delay, run);
    }
    if room {
        state(|s| s.queued.remove(&key));
    }
}

/// Offline commands: the bracket placeholders (`PlaceholderManager`), then the run.
fn run_offline(cmds: Vec<QueuedCommand>) {
    for mut cmd in cmds {
        let p = cmd.player.clone();
        let uuid_text = if p.has_uuid() { p.uuid.clone().unwrap_or_default() } else { p.name.clone() };
        cmd.command = util::replace_bracketed(&cmd.command, &["uuid", "id"], &uuid_text);
        let name = match p.uuid.as_deref() {
            Some(uuid) => {
                let known = horizon_bukkit::get_offline_player(uuid);
                if known.has_played_before() && !known.get_name().is_empty() { known.get_name().to_owned() } else { p.name.clone() }
            }
            None => p.name.clone(),
        };
        cmd.command = util::replace_bracketed(&cmd.command, &["name", "player", "username"], &name);
        let parsed = cmd.parsed();
        let run = move || {
            info(&format!("Dispatching offline command '{parsed}' for player '{}'.", p.name));
            if !horizon_bukkit::dispatch_command(&parsed) {
                let mut solution = "Check that the command syntax is correct.".to_owned();
                if cmd.payment != 0 {
                    solution.push_str(&format!(" Re-run this command at https://creator.tebex.io/payments/{}", cmd.payment));
                }
                warning(&format!("Command `{parsed}` failed to execute: "), &solution);
            }
            command_done(cmd.id);
        };
        later(cmd.delay, run);
    }
}

/// On the server thread, now or `seconds` from now.
fn later(seconds: i64, f: impl FnOnce() + Send + 'static) {
    if seconds <= 0 {
        f();
        return;
    }
    let Some(p) = plugin() else { return };
    let mut f = Some(f);
    p.run_task_later(seconds as u64 * 20, move || {
        if let Some(f) = f.take() {
            f();
        }
    });
}

/// A delivered command: deleted from the queue with the others of this tick, in one request.
fn command_done(id: i64) {
    let first = state(|s| {
        s.deletes.push(id);
        s.deletes.len() == 1
    });
    if first {
        QUEUE.run(|c| {
            let ids = state(|s| std::mem::take(&mut s.deletes));
            if !ids.is_empty()
                && let Err(e) = c.delete_commands(&secret(), &ids)
            {
                post_log(Level::Error, format!("Failed to delete commands: {e}"));
            }
        });
    }
}

/// `getFreeSlots`: empty slots of the 36 storage slots.
fn free_slots(player: &Player) -> i64 {
    let contents = player.get_inventory().get_contents();
    let used = contents.iter().take(36).filter(|s| s.as_ref().is_some_and(|s| !s.is_empty())).count();
    36 - used.min(36) as i64
}

fn send_join_events() {
    let (setup, batch) = state(|s| {
        let n = if s.setup { s.joins.len().min(750) } else { 0 };
        (s.setup, s.joins.drain(..n).collect::<Vec<_>>())
    });
    if !setup || batch.is_empty() {
        return;
    }
    let key = secret();
    QUEUE.run(move |c| {
        if c.post_batches("/events", Some(&key), &batch, 100) {
            post_debug("Successfully sent join events.".into());
        } else {
            // Kept for the next try, as Java keeps them.
            state(|s| {
                s.joins.splice(0..0, batch);
                s.joins.truncate(5000);
            });
        }
    });
}

fn send_plugin_events() {
    let (setup, events) = state(|s| (s.setup, std::mem::take(&mut s.events)));
    if !setup || events.is_empty() {
        return;
    }
    QUEUE.run(move |c| {
        c.post_batches("https://plugin-logs.tebex.io/events", None, &events, 100);
    });
}

// ---- Events ----

/// A join: with no store connected, nothing at all; with one, a join record and a map lookup.
fn on_join(player: &Player) {
    let (setup, none_queued) = state(|s| (s.setup, s.queued.is_empty()));
    if !setup {
        return;
    }
    let ip = player.get_socket_address().map(|a| a.ip().to_string()).unwrap_or_default();
    let uuid = player.get_unique_id();
    let event = json!({ "username_id": uuid, "event_type": "server.join", "event_date": util::utc_now(), "username": player.get_name(), "ip": util::anonymize_ip(&ip) });
    let key = (!none_queued).then(|| PlayerKey::of(player.get_name(), Some(&uuid)));
    let queued = state(|s| {
        s.joins.push(event);
        if s.joins.len() > 5000 {
            s.joins.remove(0);
        }
        key.as_ref().and_then(|k| s.queued.get(k).copied())
    });
    if let (Some(id), Some(key)) = (queued, key) {
        fetch_online(QueuedPlayer::new(id, player.get_name(), Some(&uuid), None), key);
    }
}

// ---- Commands ----

struct Cmd {
    name: &'static str,
    usage: &'static str,
    about: &'static str,
}

impl Cmd {
    /// The `<...>` arguments it needs (`PlayerCommand.numArgsRequired`).
    fn needs(&self) -> usize {
        self.usage.matches('<').count()
    }
}

const COMMANDS: &[Cmd] = &[
    Cmd { name: "ban", usage: "<playerName> <reason> <ip>", about: "Bans a user from the webstore." },
    Cmd { name: "checkout", usage: "<packageId>", about: "Creates a payment link for a package." },
    Cmd { name: "debug", usage: "<true/false>", about: "Enables or disables debug logging." },
    Cmd { name: "forcecheck", usage: "", about: "Checks immediately for any purchases that need to be delivered." },
    Cmd { name: "goals", usage: "", about: "Shows progress to community goals." },
    Cmd { name: "help", usage: "", about: "Shows available commands." },
    Cmd { name: "info", usage: "", about: "Shows information about the connected store." },
    Cmd { name: "lookup", usage: "<username>", about: "Gets user transaction info from your webstore." },
    Cmd { name: "reload", usage: "", about: "Reloads the plugin configuration and store connection." },
    Cmd { name: "secret", usage: "<key>", about: "Connects to your store." },
    Cmd { name: "sendlink", usage: "<packageId> <username>", about: "Sends a purchase link to a player." },
];

/// Who asked: answers go back to them, later if a request is needed.
#[derive(Clone)]
struct Asker {
    sender: CommandSender,
    console: bool,
    name: String,
}

impl Asker {
    fn of(sender: &CommandSender) -> Self {
        Self { console: sender.as_player().is_none(), name: sender.get_name().to_owned(), sender: sender.clone() }
    }
    fn tell(&self, line: &str) {
        self.sender.send_message(line);
    }
    fn fancy(&self, msg: &str, args: &[&str]) -> String {
        util::fancy(self.console, msg, args)
    }
    fn error(&self, msg: &str) -> String {
        util::error(self.console, msg)
    }
    fn success(&self, msg: &str, args: &[&str]) -> String {
        util::success(self.console, msg, args)
    }
    /// A reply from an HTTP thread, sent on the server thread.
    fn later(&self, lines: Vec<String>) {
        let me = self.clone();
        on_main(move || {
            for line in lines {
                me.tell(&line);
            }
        });
    }
}

fn tebex_command(sender: &CommandSender, args: &[&str]) -> bool {
    let a = Asker::of(sender);
    if args.is_empty() && sender.has_permission("tebex.tebex") {
        let version = plugin().map(|p| p.get_version().to_owned()).unwrap_or_default();
        a.tell(&a.fancy("Welcome to Tebex!", &[]));
        a.tell(&a.fancy("This server is running version {0}", &[&format!("v{version}")]));
        return true;
    }
    let name = args.first().copied().unwrap_or("tebex");
    let Some(cmd) = COMMANDS.iter().find(|c| c.name == name) else {
        a.tell(&a.error("Command not found."));
        return true;
    };
    if !a.console && !sender.has_permission(&format!("tebex.{}", cmd.name)) {
        a.tell(&a.error("Unrecognized command or no permission."));
        return true;
    }
    let rest = &args[1..];
    if rest.len() < cmd.needs() {
        a.tell(&a.error(&format!("Usage: /tebex {} {}", cmd.name, cmd.usage)));
        return true;
    }
    match cmd.name {
        "ban" => cmd_ban(&a, rest),
        "checkout" => cmd_checkout(&a, rest),
        "debug" => cmd_debug(&a, rest),
        "forcecheck" => {
            QUEUE.run(|c| check_queue(c, false));
            a.tell(&a.success("Check completed. See console for details.", &[]));
        }
        "goals" => cmd_goals(&a),
        "help" => cmd_help(&a),
        "info" => cmd_info(&a),
        "lookup" => cmd_lookup(&a, rest),
        "reload" => {
            reload_config();
            refresh_listing();
            send_plugin_events();
            a.tell(&a.success("Reload completed.", &[]));
        }
        "secret" => cmd_secret(&a, rest),
        "sendlink" => cmd_sendlink(&a, rest),
        _ => {}
    }
    true
}

fn needs_setup(a: &Asker) -> Option<String> {
    (!state(|s| s.setup)).then(|| a.error("Tebex is not set up yet. Use /tebex secret <key> first."))
}

fn cmd_ban(a: &Asker, rest: &[&str]) {
    if let Some(e) = needs_setup(a) {
        return a.tell(&e);
    }
    let (name, reason, ip) = (rest[0].to_owned(), rest[1].to_owned(), rest[2].to_owned());
    let (a, key) = (a.clone(), secret());
    ASK.run(move |c| {
        let line = match c.ban(&key, &name, &ip, &reason) {
            Ok(true) => a.success("User {0} has been banned successfully.", &[&name]),
            Ok(false) => {
                post_log(Level::Warning, format!("Failed to process ban for user '{name}'"));
                a.error("Failed to ban user.")
            }
            Err(e) => a.error(&format!("Failed to ban user. {e}")),
        };
        a.later(vec![line]);
    });
}

fn cmd_checkout(a: &Asker, rest: &[&str]) {
    if a.console {
        return a.tell(&a.error("This command cannot be run from the console."));
    }
    let Ok(package) = rest[0].parse::<i64>() else {
        return a.tell(&a.error("The Package ID must be a number."));
    };
    if let Some(e) = needs_setup(a) {
        return a.tell(&e);
    }
    let (a, key) = (a.clone(), secret());
    ASK.run(move |c| match c.checkout(&key, package, &a.name) {
        Ok(url) => a.later(vec![checkout_line(&url), a.success("Checkout link sent to you.", &[])]),
        Err(e) => a.later(vec![a.error(&e.to_string())]),
    });
}

/// `sendCheckoutLink`.
pub fn checkout_line(url: &str) -> String {
    format!("Checkout started! Complete payment here: {url}")
}

fn cmd_debug(a: &Asker, rest: &[&str]) {
    let value = rest[0].to_ascii_lowercase();
    if value != "true" && value != "false" {
        return a.tell(&a.error("Specify 'true' or 'false' for debug mode."));
    }
    let on = value == "true";
    VERBOSE.store(on, Ordering::Relaxed);
    let key = state(|s| {
        s.cfg.verbose = on;
        s.cfg.secret.clone()
    });
    save_config(&key, on);
    a.tell(&a.fancy("Debug mode set to {0}", &[&value]));
}

fn cmd_goals(a: &Asker) {
    if let Some(e) = needs_setup(a) {
        return a.tell(&e);
    }
    let (a, key) = (a.clone(), secret());
    ASK.run(move |c| {
        let lines = match c.goals(&key) {
            Ok(goals) if goals.is_empty() => vec![a.fancy("No community goals available.", &[])],
            Ok(goals) => std::iter::once(a.fancy("Community Goals: ", &[]))
                .chain(
                    goals
                        .iter()
                        .filter(|g| g.status != "DISABLED")
                        .map(|g| a.fancy(&format!("- {} ({}/{}) [{}]", g.name, util::two_places(g.current), util::two_places(g.target), g.status), &[])),
                )
                .collect(),
            Err(e) => vec![a.error(&e.to_string())],
        };
        a.later(lines);
    });
}

fn allowed(a: &Asker) -> Vec<&'static Cmd> {
    COMMANDS.iter().filter(|c| a.console || a.sender.has_permission(&format!("tebex.{}", c.name))).collect()
}

fn cmd_help(a: &Asker) {
    a.tell(&a.fancy("Available commands:", &[]));
    for c in allowed(a) {
        let usage = if c.usage.is_empty() { String::new() } else { format!(" {}", c.usage) };
        a.tell(&a.fancy(&format!("/tebex {}{usage} | {}", c.name, c.about), &[]));
    }
}

fn cmd_info(a: &Asker) {
    let Some(info) = state(|s| s.info.clone().filter(|_| s.setup)) else {
        return a.tell(&a.error("Tebex is not set up yet. Use /tebex secret <key> first."));
    };
    a.tell(&a.fancy("Information for this server:", &[]));
    a.tell(&a.fancy("{0} for webstore {1}", &[&info.server_name, &info.store.name]));
    a.tell(&a.fancy("Server prices are in {0}", &[&info.store.currency_iso]));
    a.tell(&a.fancy("Webstore URL: {0}", &[&info.store.domain]));
}

fn cmd_lookup(a: &Asker, rest: &[&str]) {
    if let Some(e) = needs_setup(a) {
        return a.tell(&e);
    }
    let (a, key, user) = (a.clone(), secret(), rest[0].to_owned());
    ASK.run(move |c| {
        let lines = match c.lookup(&key, &user) {
            Ok(Some(l)) => vec![
                a.fancy("Username: {0}", &[&l.username]),
                a.fancy("Id: {0}", &[&l.id]),
                a.fancy("Chargeback Rate: {0}%", &[&l.chargeback_rate.to_string()]),
                a.fancy("Bans Total: {0}", &[&l.ban_count.to_string()]),
                a.fancy("Payments: {0}", &[&l.payments.to_string()]),
            ],
            Ok(None) => vec![a.error("user not found")],
            Err(e) => vec![a.error(&e.to_string())],
        };
        a.later(lines);
    });
}

fn cmd_secret(a: &Asker, rest: &[&str]) {
    if !a.console {
        a.tell(&a.fancy("Checking your secret key...", &[]));
    }
    let new_key = rest[0].to_owned();
    let (old_key, verbose_on) = state(|s| (s.cfg.secret.clone(), s.cfg.verbose));
    state(|s| s.cfg.secret = new_key.clone());
    save_config(&new_key, verbose_on);
    let a = a.clone();
    ASK.run(move |c| match c.information(&new_key) {
        Ok(info) => {
            let line = a.fancy("Successfully connected to your store: {0} as {1}", &[&info.store.name, &info.server_name]);
            on_main(move || {
                state(|s| s.info = Some(info));
                init_store();
                a.tell(&line);
            });
        }
        Err(_) => on_main(move || {
            state(|s| s.cfg.secret = old_key.clone());
            save_config(&old_key, verbose_on);
            a.tell(&a.error("Your secret key was invalid. Please try again."));
        }),
    });
}

fn cmd_sendlink(a: &Asker, rest: &[&str]) {
    let Ok(package) = rest[0].parse::<i64>() else {
        return a.tell(&a.error("Package ID must be a number."));
    };
    let Some(target) = horizon_bukkit::get_player(rest[1]) else {
        return a.tell(&a.error(&format!("{} must be online to receive a package link.", rest[1])));
    };
    if let Some(e) = needs_setup(a) {
        return a.tell(&e);
    }
    let (a, key) = (a.clone(), secret());
    ASK.run(move |c| match c.checkout(&key, package, target.get_name()) {
        Ok(url) => {
            let line = a.success(&format!("Checkout link sent to {}", target.get_name()), &[]);
            on_main(move || {
                target.send_message(&checkout_line(&url));
                a.tell(&line);
            });
        }
        Err(e) => a.later(vec![a.error(&format!("Failed to send checkout link: {e}"))]),
    });
}

fn tebex_complete(sender: &CommandSender, args: &[&str]) -> Vec<String> {
    if args.len() != 1 {
        return Vec::new();
    }
    let a = Asker::of(sender);
    allowed(&a).into_iter().map(|c| c.name.to_owned()).filter(|n| n.starts_with(args[0])).collect()
}

/// `/buy` (its name from `buy-command.name`).
fn buy_command(sender: &CommandSender) -> bool {
    let console = sender.as_player().is_none();
    let say = |text: &str| sender.send_message(&if console { horizon_bukkit::chat_color::strip_color(text) } else { text.to_owned() });
    if !state(|s| s.setup) {
        say("§cTebex is not setup yet!");
        return true;
    }
    match sender.as_player() {
        Some(player) => {
            gui::open_home(player);
            true
        }
        None => {
            say("§cThe buy command cannot be used from the console.");
            false
        }
    }
}

fn register_buy(p: Plugin) {
    let (enabled, name) = state(|s| (s.cfg.buy_enabled, s.cfg.buy_name.clone()));
    if !enabled || name.trim().is_empty() {
        return;
    }
    let command = horizon_bukkit::pure2::Command::new(&name);
    if !horizon_bukkit::fam_core::get_command_map().register(&p, "tebex", &command) {
        error(&format!("Failed to register buy command: /{name} is taken"));
        return;
    }
    p.set_executor(&name, |sender, _, _| buy_command(sender));
}

// ---- Enable / disable ----

fn on_enable(p: Plugin) {
    let started = Instant::now();
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    ONLINE_MODE.store(horizon_bukkit::server::get_online_mode(), Ordering::Relaxed);
    migrate_buycraft();
    let cfg = read_config();
    VERBOSE.store(cfg.verbose, Ordering::Relaxed);
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(State { cfg, ..State::empty() });

    let agent = format!("Minecraft-BUKKIT-{}/Tebex-{}", mc_version(), p.get_version());
    let client = Client::new(agent);
    QUEUE.start("queue", client.clone());
    ASK.start("ask", client);

    p.set_executor("tebex", |s, _, a| tebex_command(s, a));
    p.set_tab_completer("tebex", |s, _, a| tebex_complete(s, a));
    register_buy(p);

    p.register_event_with("NORMAL", false, |e: &mut PlayerJoinEvent| on_join(e.get_player()));
    p.register_event_with("MONITOR", false, |e: &mut PlayerQuitEvent| {
        let uuid = e.get_player().get_unique_id();
        state(|s| s.menus.remove(&uuid));
    });
    p.register_event_with("NORMAL", false, gui::on_click);

    // Answers from the HTTP threads, once a tick (an atomic load when there are none).
    p.run_task_timer(1, 1, drain_main);
    p.run_task_timer(6000, 6000, refresh_listing);
    p.run_task_timer(12000, 12000, send_plugin_events);
    p.run_task_timer(1200, 1200, send_join_events);

    init_store();
    info(&format!("Enabled in {} ms (Rust).", started.elapsed().as_millis()));
}

fn on_disable(_p: Plugin) {
    QUEUE.stop();
    ASK.stop();
    hold_ticks(false);
    IN_FLIGHT.store(0, Ordering::Release);
    MAIN.lock().unwrap_or_else(|e| e.into_inner()).clear();
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
