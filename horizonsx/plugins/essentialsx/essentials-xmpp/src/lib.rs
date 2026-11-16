//! `EssentialsXMPP`. Java's `Smack` is `client`; the socket has a thread of its own, and everything
//! that touches the game comes back to the main thread on a one-tick timer. GAPS.md has what differs.
pub mod client;
pub mod xml;

use essentials::prelude::*;
use chrono::Local;
use client::{Incoming, Session, Settings};
use essentials::command_source::console_display_name;
use essentials::config::Config;
use essentials::utils::format::strip_log_color_format;
use horizon_bukkit::enums::event_priority::MONITOR;
use horizon_bukkit::event::{PlayerJoinEvent, PlayerQuitEvent};
use horizon_bukkit::fam_ev_player::AsyncPlayerChatEvent;
use horizon_bukkit::world::match_player;
use horizon_bukkit::{Plugin, dispatch_command, scheduler};
use serde_yaml::{Mapping, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const TEMPLATE: &str = include_str!("../config.yml");
const ADDRESS: &str = "address";
const SPY: &str = "spy";
const LAG_LINE: &str = "Can't keep up! Did the system time change, or is the server overloaded?";
const KEEP_ALIVE: Duration = Duration::from_secs(30);
const QUEUE: usize = 1024;

fn folder() -> PathBuf {
    essentials::data_folder().parent().map_or_else(|| PathBuf::from("plugins/EssentialsXMPP"), |plugins| plugins.join("EssentialsXMPP"))
}

/// `java.util.logging.Level`'s numbers for the levels `log-level` can name.
fn level_number(name: &str) -> i32 {
    match name.to_uppercase().as_str() {
        "OFF" => i32::MAX,
        "SEVERE" => 1000,
        "WARNING" => 900,
        "CONFIG" => 700,
        "FINE" => 500,
        "FINER" => 400,
        "FINEST" => 300,
        "ALL" => i32::MIN,
        other => other.parse().unwrap_or(800),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    Info,
    Warning,
    Severe,
}

impl Level {
    fn number(self) -> i32 {
        match self {
            Self::Info => 800,
            Self::Warning => 900,
            Self::Severe => 1000,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Severe => "SEVERE",
        }
    }
}

/// `logger.addHandler(this)`'s settings, which `reloadConfig` sets.
struct LogForward {
    users: Vec<String>,
    level: i32,
    ignore_lag: bool,
}

static FORWARD: Mutex<Option<LogForward>> = Mutex::new(None);
static RECORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// What the socket's thread hands to the main thread.
static INBOUND: Mutex<Vec<Inbound>> = Mutex::new(Vec::new());

enum Inbound {
    Log(Level, String),
    Heard(String, String),
}

/// `publish`, past the formatter: the line a log user gets.
fn record(level: Level, text: &str) -> Option<String> {
    let guard = FORWARD.lock().unwrap_or_else(|e| e.into_inner());
    let forward = guard.as_ref()?;
    if (forward.ignore_lag && text == LAG_LINE) || level.number() < forward.level {
        return None;
    }
    Some(format!("{} EssentialsXMPP\n{}: {}\n", Local::now().format("%b %d, %Y %-I:%M:%S %p"), level.name(), text))
}

/// `logger.log`, which also feeds `publish`.
fn log(level: Level, text: &str) {
    match level {
        Level::Info => essentials::info(text),
        Level::Warning => essentials::warning(text),
        Level::Severe => essentials::severe(text),
    }
    if let Some(line) = record(level, text) {
        RECORDS.lock().unwrap_or_else(|e| e.into_inner()).push(strip_format(&strip_log_color_format(&line)));
    }
}

/// `bare` of `StringUtils.parseBareAddress`.
fn bare_address(address: &str) -> &str {
    address.split('/').next().unwrap_or(address)
}

/// `String.format(format, name, message)` for what a chat format can hold.
fn format_chat(format: &str, name: &str, message: &str) -> String {
    let mut out = String::with_capacity(format.len() + name.len() + message.len());
    let mut chars = format.chars().peekable();
    let mut next = 0;
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&digit) = chars.peek() {
            if !digit.is_ascii_digit() && digit != '$' {
                break;
            }
            spec.push(digit);
            chars.next();
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('n') => out.push('\n'),
            Some('s' | 'S') => {
                let index = spec.strip_suffix('$').and_then(|number| number.parse::<usize>().ok()).map(|number| number - 1).unwrap_or_else(|| {
                    next += 1;
                    next - 1
                });
                out.push_str(match index {
                    0 => name,
                    1 => message,
                    _ => "",
                });
            }
            Some(other) => {
                out.push('%');
                out.push_str(&spec);
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// `UserManager`, on `users.yml`.
struct Users {
    config: Config,
    spies: Vec<String>,
}

static USERS: LazyLock<Mutex<Users>> = LazyLock::new(|| {
    let mut users = Users { config: Config::new(folder().join("users.yml")), spies: Vec::new() };
    users.reload();
    Mutex::new(users)
});

fn users() -> std::sync::MutexGuard<'static, Users> {
    USERS.lock().unwrap_or_else(|e| e.into_inner())
}

impl Users {
    fn reload(&mut self) {
        self.config.load();
        self.spies = self.config.get_keys().iter().filter(|key| self.is_spy(key)).filter_map(|key| self.get_address(key)).collect();
    }
    fn is_spy(&self, name: &str) -> bool {
        self.config.get_boolean(&format!("{}.{SPY}", name.to_lowercase()), false)
    }
    fn get_address(&self, name: &str) -> Option<String> {
        self.config.get_string(&format!("{}.{ADDRESS}", name.to_lowercase()), None)
    }
    fn get_user_by_address(&self, search: &str) -> Option<String> {
        self.config.get_keys().into_iter().find(|name| self.config.get_string(&format!("{name}.{ADDRESS}"), None).is_some_and(|address| address.eq_ignore_ascii_case(search)))
    }
    fn set_user(&mut self, name: &str, address: Option<String>, spy: bool) {
        let mut data = Mapping::new();
        if let Some(address) = address {
            data.insert(Value::from(ADDRESS), Value::from(address));
        }
        data.insert(Value::from(SPY), Value::from(spy));
        self.config.set(name, Value::Mapping(data));
        self.config.save();
        self.reload();
    }
    fn set_address(&mut self, name: &str, address: &str) {
        let spy = self.is_spy(name);
        self.set_user(&name.to_lowercase(), Some(address.to_owned()), spy);
    }
    fn toggle_spy(&mut self, name: &str) -> bool {
        let spy = !self.is_spy(name);
        let address = self.get_address(name);
        self.set_user(&name.to_lowercase(), address, spy);
        spy
    }
}

enum Out {
    Message(String, String),
    Presence(String, bool),
    Quit,
}

/// The connection's state, which a caller can ask without touching the socket.
struct Link {
    out: SyncSender<Out>,
    /// A first connection failed: as in Java, nothing tries again until a reload.
    failed: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

static LINK: Mutex<Option<Link>> = Mutex::new(None);

fn hand(item: Inbound) {
    INBOUND.lock().unwrap_or_else(|e| e.into_inner()).push(item);
}

fn say(level: Level, text: String) {
    hand(Inbound::Log(level, text));
}

/// One connected stretch: until the server goes, or `Out::Quit`. `Ok` means it was told to stop.
fn serve(session: &mut Session, out: &Receiver<Out>) -> Result<(), String> {
    session.presence("No one online.", false)?;
    let mut alive = Instant::now();
    let mut flushed = Instant::now();
    loop {
        for heard in session.poll()? {
            let Incoming::Message { from, body } = heard;
            hand(Inbound::Heard(from, body));
        }
        loop {
            match out.try_recv() {
                Ok(Out::Message(to, body)) => session.message(&to, &body)?,
                Ok(Out::Presence(status, dnd)) => session.presence(&status, dnd)?,
                Ok(Out::Quit) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    session.close();
                    return Ok(());
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
            alive = Instant::now();
        }
        // `loggerThread`, every two seconds.
        if flushed.elapsed() >= Duration::from_secs(2) {
            flushed = Instant::now();
            let lines = std::mem::take(&mut *RECORDS.lock().unwrap_or_else(|e| e.into_inner()));
            if !lines.is_empty() {
                let log_users = FORWARD.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|forward| forward.users.clone()).unwrap_or_default();
                for user in log_users {
                    for line in &lines {
                        session.message(&user, line)?;
                    }
                }
            }
        }
        if alive.elapsed() >= KEEP_ALIVE {
            alive = Instant::now();
            session.keep_alive()?;
        }
    }
}

/// `ReconnectionManager`'s wait before each try: soon at first, then seldom.
fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(match attempt {
        0..=2 => 10,
        3..=8 => 60,
        _ => 300,
    })
}

/// Waits, still hearing a stop. Anything else sent meanwhile stays queued: the receiver isn't read
/// past the first item, so it goes back on the worker's own list.
fn pause(out: &Receiver<Out>, wait: Duration, held: &mut Vec<Out>) -> bool {
    let end = Instant::now() + wait;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        match out.recv_timeout(left) {
            Ok(Out::Quit) | Err(RecvTimeoutError::Disconnected) => return true,
            Ok(item) => {
                if held.len() < QUEUE {
                    held.push(item);
                }
            }
            Err(RecvTimeoutError::Timeout) => return false,
        }
    }
}

fn run_link(settings: Settings, out: Receiver<Out>, failed: Arc<AtomicBool>) {
    let mut connected_once = false;
    let mut attempt = 0;
    let mut held = Vec::new();
    loop {
        say(Level::Info, format!("Connecting to xmpp server {}:{} as user {}.", settings.server, settings.port, settings.user));
        match client::login(&settings) {
            Ok(mut session) => {
                connected_once = true;
                attempt = 0;
                if !session.secure {
                    say(Level::Warning, "Logged in without TLS: the password and all messages cross the network unencrypted.".to_owned());
                }
                let backlog: Result<(), String> = held.drain(..).try_for_each(|item| match item {
                    Out::Message(to, body) => session.message(&to, &body),
                    Out::Presence(status, dnd) => session.presence(&status, dnd),
                    Out::Quit => Ok(()),
                });
                match backlog.and_then(|()| serve(&mut session, &out)) {
                    Ok(()) => return,
                    Err(reason) => say(Level::Warning, format!("Lost the connection to {}: {reason}", settings.server)),
                }
            }
            Err(failure) => {
                say(Level::Warning, format!("Failed to connect to server: {}: {}", settings.server, failure.reason));
                say(Level::Warning, format!("Connected: {}", failure.connected));
                say(Level::Warning, format!("Secure: {}", failure.secure));
                say(Level::Warning, format!("Using TLS: {}", failure.secure));
                say(Level::Warning, format!("Authenticated: {}", failure.authenticated));
                if !connected_once {
                    failed.store(true, Ordering::Relaxed);
                    return;
                }
            }
        }
        if pause(&out, backoff(attempt), &mut held) {
            return;
        }
        attempt += 1;
    }
}

/// `disconnect`, which also ends the thread: waits only as long as a socket's short timeout takes.
fn disconnect() {
    let link = LINK.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(link) = link {
        let _ = link.out.try_send(Out::Quit);
        drop(link.out);
        let end = Instant::now() + Duration::from_secs(2);
        while !link.thread.is_finished() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// `connect` and the rest of `reloadConfig`. A slow server can't hold up the main thread here: the
/// login is on the socket's thread, where Java's was on this one.
fn reload_config() {
    *FORWARD.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let mut config = Config::with_template(folder().join("config.yml"), TEMPLATE);
    config.load();
    disconnect();
    let Some(server) = config.get_string("xmpp.server", None).filter(|server| server != "example.com") else {
        log(Level::Warning, &mini_to_legacy(&tl_literal("xmppNotConfigured", &[])));
        return;
    };
    let settings = Settings {
        service: config.get_string("xmpp.servicename", Some(&server)).unwrap_or_else(|| server.clone()),
        port: u16::try_from(config.get_int("xmpp.port", 5222)).unwrap_or(5222),
        user: config.get_string("xmpp.user", None).unwrap_or_default(),
        password: config.get_string("xmpp.password", None).unwrap_or_default(),
        sasl: config.get_boolean("xmpp.sasl-enabled", false),
        require_tls: config.get_boolean("xmpp.require-server-tls", false),
        roots: None,
        server,
    };
    if config.get_boolean("log-enabled", false) {
        *FORWARD.lock().unwrap_or_else(|e| e.into_inner()) = Some(LogForward {
            users: config.get_string_list("log-users"),
            level: level_number(&config.string_or("log-level", "info")),
            ignore_lag: config.get_boolean("ignore-lag-messages", true),
        });
    }
    let (out, queue) = sync_channel(QUEUE);
    let failed = Arc::new(AtomicBool::new(false));
    let flag = failed.clone();
    match std::thread::Builder::new().name("EssentialsXMPP".to_owned()).spawn(move || run_link(settings, queue, flag)) {
        Ok(thread) => *LINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Link { out, failed, thread }),
        Err(e) => log(Level::Severe, &format!("Couldn't start the XMPP thread: {e}")),
    }
}

/// `XMPPManager.sendMessage`: `false` when there's no address, no connection to come, or no room.
pub fn send_message(address: Option<&str>, message: &str) -> bool {
    let Some(address) = address.filter(|address| !address.is_empty()) else { return false };
    let link = LINK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(link) = link.as_ref().filter(|link| !link.failed.load(Ordering::Relaxed)) else { return false };
    !matches!(link.out.try_send(Out::Message(address.to_owned(), strip_format(message))), Err(TrySendError::Full(_) | TrySendError::Disconnected(_)))
}

/// `updatePresence`.
fn update_presence() {
    let link = LINK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(link) = link.as_ref() else {
        drop(link);
        essentials::warning(&mini_to_legacy(&tl_literal("xmppNotConfigured", &[])));
        return;
    };
    let online = ess().get_online_users().len();
    let presence = match online {
        0 => Out::Presence("No one online.".to_owned(), true),
        1 => Out::Presence("1 player online.".to_owned(), false),
        many => Out::Presence(format!("{many} players online."), false),
    };
    let _ = link.out.try_send(presence);
}

/// `getUserByAddress`.
fn user_by_address(address: &str) -> Option<User> {
    let name = users().get_user_by_address(address)?;
    ess().get_offline_user(&name)
}

/// `broadcastMessage`: the game, then the spies, but not who said it.
fn broadcast(sender: &User, message: &str, address: &str) {
    ess().broadcast_message_from(sender, message);
    let spies = users().spies.clone();
    for spy in spies {
        if !spy.eq_ignore_ascii_case(address) {
            send_message(Some(&spy), message);
        }
    }
}

/// `sendPrivateMessage`.
fn send_private_message(address: &str, message: &str) {
    let Some((target, _)) = message.split_once(' ') else { return };
    let matches = match_player(&target[1..]);
    if matches.is_empty() {
        send_message(Some(address), &format!("User {target} not found"));
        return;
    }
    let sender = user_by_address(address).map_or_else(|| "null".to_owned(), |user| user.get_name());
    for player in matches {
        player.send_message(&format!("[{sender}>{}]  {message}", player.display_name().to_legacy()));
    }
}

/// `sendCommand`.
fn send_command(address: &str, message: &str) {
    let mut config = Config::with_template(folder().join("config.yml"), TEMPLATE);
    config.load();
    if config.get_string_list("op-users").iter().any(|user| user == address) {
        dispatch_command(&message[1..]);
    }
}

/// `processMessage`.
fn process_message(from: &str, message: &str) {
    let address = bare_address(from);
    match message.chars().next() {
        Some('@') => send_private_message(address, message),
        Some('/') => send_command(address, message),
        Some(_) => {
            if let Some(sender) = user_by_address(address) {
                broadcast(&sender, &format!("={}: {message}", sender.get_display_name()), address);
            }
        }
        None => {}
    }
}

/// The main thread's side of the socket's thread, every tick.
fn pump() {
    let items = std::mem::take(&mut *INBOUND.lock().unwrap_or_else(|e| e.into_inner()));
    for item in items {
        match item {
            Inbound::Log(level, text) => log(level, &text),
            Inbound::Heard(from, body) => process_message(&from, &body),
        }
    }
}

/// `sendMessageToSpyUsers`.
fn send_to_spies(message: &str) {
    let spies = users().spies.clone();
    for address in spies {
        send_message(Some(&address), message);
    }
}

fn run_xmpp(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let Some(address) = users().get_address(&args[0]) else {
        sender.send_message("§cThere are no players matching that name.");
        return Ok(());
    };
    let message = get_final_arg(args, 1);
    let name = sender.get_user().map_or_else(console_display_name, |user| user.get_display_name());
    sender.send_message(&format!("[{name}>{address}] {message}"));
    if !send_message(Some(&address), &format!("[{name}] {message}")) {
        sender.send_message("§cError sending message.");
    }
    Ok(())
}

fn run_xmppspy(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    loop_online_players(sender, false, true, &args[0], |user| {
        let enabled = users().toggle_spy(&user.get_name());
        sender.send_message(&format!("XMPP Spy {} for {}", if enabled { "enabled" } else { "disabled" }, user.get_display_name()));
        Ok(())
    })
}

fn run_setxmpp(user: &User, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    users().set_address(&user.get_name(), &args[0]);
    user.send_message(&format!("XMPP address set to {}", args[0]));
    Ok(())
}

const COMMANDS: [Command; 3] = [
    Command { run_source: Some(run_xmpp), ..Command::new("xmpp") },
    Command { run_source: Some(run_xmppspy), ..Command::new("xmppspy") },
    Command { run_user: Some(run_setxmpp), ..Command::new("setxmpp") },
];

/// `onEnable`.
pub fn on_enable(plugin: Plugin) {
    let _ = std::fs::create_dir_all(folder());
    LazyLock::force(&USERS);
    reload_config();
    ess().add_reload_listener(|| {
        users().reload();
        reload_config();
    });
    scheduler::run_task_timer(1, 1, pump);
    plugin.register_event_with(MONITOR, false, |event: &mut PlayerJoinEvent| {
        ess().schedule_sync_delayed_task_later(update_presence, 1);
        if let Some(user) = ess().get_user(event.get_player()) {
            send_to_spies(&format!("Player {} joined the game", user.get_display_name()));
        }
    });
    plugin.register_event_with(MONITOR, false, |event: &mut AsyncPlayerChatEvent| {
        if let Some(user) = ess().get_user(event.get_player()) {
            send_to_spies(&format_chat(event.get_format(), &user.get_display_name(), event.get_message()));
        }
    });
    plugin.register_event_with(MONITOR, false, |event: &mut PlayerQuitEvent| {
        ess().schedule_sync_delayed_task_later(update_presence, 1);
        if let Some(user) = ess().get_user(event.get_player()) {
            send_to_spies(&format!("Player {} left the game", user.get_display_name()));
        }
    });
    register_commands(plugin, &COMMANDS);
}

/// `onDisable`.
pub fn on_disable() {
    disconnect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chat_format_reads_as_java_formats_it() {
        assert_eq!(format_chat("<%1$s> %2$s", "Steve", "hi"), "<Steve> hi");
        assert_eq!(format_chat("%s: %s", "Steve", "hi"), "Steve: hi");
        assert_eq!(format_chat("%2$s by %1$s 100%%", "Steve", "hi"), "hi by Steve 100%");
        assert_eq!(format_chat("%d stays", "Steve", "hi"), "%d stays");
    }

    #[test]
    fn a_bare_address_has_no_resource() {
        assert_eq!(bare_address("a@b.c/phone"), "a@b.c");
        assert_eq!(bare_address("a@b.c"), "a@b.c");
    }

    #[test]
    fn log_levels_read_as_java_parses_them() {
        assert_eq!(level_number("warning"), 900);
        assert_eq!(level_number("info"), 800);
        assert_eq!(level_number("nonsense"), 800);
        assert_eq!(level_number("950"), 950);
    }

    #[test]
    fn reconnecting_slows_down() {
        assert!(backoff(0) < backoff(5) && backoff(5) < backoff(50));
    }
}
