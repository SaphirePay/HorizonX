//! `ConsoleInjector`. Java hangs an appender on Log4j. The SDK has no such tap, so this reads the game's
//! own `logs/latest.log` as it grows: every line the console shows goes there too. See GAPS.md.
use essentials::prelude::*;
use crate::client::model::Mentions;
use crate::host::{self, Host, Level};
use crate::message_util::sanitize_discord_markdown;
use crate::service::JdaDiscordService;
use regex::Regex;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, OnceLock, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const QUEUE_PROCESS_PERIOD: Duration = Duration::from_secs(2);
const QUEUE_CAPACITY: usize = 500;
const POLL: Duration = Duration::from_millis(250);
const MAX_CONTENT_LENGTH: usize = 2000;

/// `[HH:mm:ss] [thread/LEVEL]: message`: Log4j's pattern, as the game writes it.
static LOG_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[\d\d:\d\d:\d\d\] \[[^\]]*/([A-Z]+)\]: (.*)$").expect("regex"));
/// `FormatUtil.stripAnsi`.
static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").expect("regex"));

static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();

/// Where the log is, which tests choose.
pub fn set_log_file(path: PathBuf) {
    let _ = LOG_FILE.set(path);
}

fn int_level(name: &str) -> i32 {
    match name {
        "FATAL" => 100,
        "ERROR" => 200,
        "WARN" => 300,
        "DEBUG" => 500,
        "TRACE" => 600,
        _ => 400,
    }
}

/// A line of the log: its level and its text. One that isn't in the pattern continues the last,
/// a stack trace say, and counts as `INFO`.
pub fn parse_line(line: &str) -> (String, String) {
    match LOG_LINE.captures(line) {
        Some(found) => (found[1].to_owned(), found[2].to_owned()),
        None => ("INFO".to_owned(), line.to_owned()),
    }
}

/// `TimeFormat.TIME_LONG.format(Instant.now())`.
fn timestamp() -> String {
    format!("<t:{}:T>", SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_secs()))
}

/// `Splitter.fixedLength`.
fn split_fixed(text: &str, length: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(length.max(1)).map(|chunk| chunk.iter().collect()).collect()
}

/// `append`: what goes on the queue for a line, if anything.
pub fn format_entry(service: &JdaDiscordService, level: &str, message: &str) -> Vec<String> {
    let settings = service.settings();
    if int_level(level) > settings.get_console_log_level() {
        return Vec::new();
    }
    // Ansi strip is for normal colors, the other for color codes that weren't formatted.
    let entry = strip_format(&ANSI.replace_all(message, "")).trim().to_owned();
    if entry.is_empty() || settings.get_console_filters().iter().any(|pattern| pattern.is_match(&entry)) {
        return Vec::new();
    }
    let args = [Arg::from(timestamp()), Arg::from(level), Arg::from(sanitize_discord_markdown(&entry))];
    split_fixed(&settings.console_format().format(&args), MAX_CONTENT_LENGTH - 50)
}

/// Joins what's queued into messages of up to 2000 characters, each line after a line break.
pub fn drain(queue: &mut VecDeque<String>, mut send: impl FnMut(String)) {
    let mut buffer = String::new();
    while let Some(line) = queue.front() {
        if buffer.chars().count() + line.chars().count() > MAX_CONTENT_LENGTH - 2 {
            send(std::mem::take(&mut buffer));
            continue;
        }
        buffer.push('\n');
        buffer.push_str(line);
        queue.pop_front();
    }
    if !buffer.is_empty() {
        send(buffer);
    }
}

/// What's new in a growing file.
struct Tail {
    path: PathBuf,
    position: u64,
    partial: Vec<u8>,
}

impl Tail {
    /// From the end: what's there is old.
    fn new(path: &Path) -> Self {
        Self { path: path.to_owned(), position: std::fs::metadata(path).map_or(0, |data| data.len()), partial: Vec::new() }
    }
    fn read_lines(&mut self) -> Vec<String> {
        let Ok(mut file) = File::open(&self.path) else { return Vec::new() };
        let length = file.metadata().map_or(0, |data| data.len());
        if length < self.position {
            // A new run's log.
            self.position = 0;
            self.partial.clear();
        }
        if file.seek(SeekFrom::Start(self.position)).is_err() {
            return Vec::new();
        }
        let mut fresh = Vec::new();
        let Ok(read) = file.read_to_end(&mut fresh) else { return Vec::new() };
        self.position += read as u64;
        self.partial.append(&mut fresh);
        let Some(end) = self.partial.iter().rposition(|byte| *byte == b'\n') else { return Vec::new() };
        let rest = self.partial.split_off(end + 1);
        let done = std::mem::replace(&mut self.partial, rest);
        String::from_utf8_lossy(&done).lines().map(|line| line.trim_end_matches('\r').to_owned()).collect()
    }
}

struct Shared {
    service: Weak<JdaDiscordService>,
    host: Arc<dyn Host>,
    stop: AtomicBool,
    removed: AtomicBool,
    wake: (Mutex<()>, Condvar),
}

impl Shared {
    /// `remove`, from inside: the thread ends and the relay's webhook closes.
    fn fail(&self) {
        host::log(&self.host, Level::Severe, &tl_literal("discordErrorWebhook", &[]));
        self.removed.store(true, Ordering::SeqCst);
        self.stop.store(true, Ordering::SeqCst);
    }
    fn run(self: Arc<Self>, path: PathBuf) {
        let mut tail = Tail::new(&path);
        let mut queue: VecDeque<String> = VecDeque::new();
        let mut last_drain = Instant::now();
        while !self.stop.load(Ordering::SeqCst) {
            {
                let guard = self.wake.0.lock().unwrap_or_else(|e| e.into_inner());
                if !self.stop.load(Ordering::SeqCst) {
                    let _ = self.wake.1.wait_timeout(guard, POLL);
                }
            }
            let Some(service) = self.service.upgrade() else { return };
            for line in tail.read_lines() {
                let (level, message) = parse_line(&line);
                for part in format_entry(&service, &level, &message) {
                    if queue.len() >= QUEUE_CAPACITY {
                        if self.host.is_debug() {
                            host::log(&self.host, Level::Info, "Console relay queue full, dropping message.");
                        }
                    } else {
                        queue.push_back(part);
                    }
                }
            }
            if last_drain.elapsed() < QUEUE_PROCESS_PERIOD {
                continue;
            }
            last_drain = Instant::now();
            drain(&mut queue, |content| self.send_message(&service, content));
        }
    }
    /// `sendMessage`.
    fn send_message(self: &Arc<Self>, service: &Arc<JdaDiscordService>, content: String) {
        let Some(webhook) = service.get_console_webhook().filter(|webhook| !webhook.is_shutdown()) else { return };
        let mut message = service.console_webhook_message(content);
        message.mentions = Mentions::NoGroups;
        let this = self.clone();
        webhook.send(message).then(move |sent| {
            if sent.is_err() {
                this.fail();
            }
        });
    }
}

pub struct ConsoleInjector {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl ConsoleInjector {
    pub fn start(service: &Arc<JdaDiscordService>) -> Self {
        let path = LOG_FILE.get().cloned().unwrap_or_else(|| PathBuf::from("logs/latest.log"));
        let shared = Arc::new(Shared { service: Arc::downgrade(service), host: service.host().clone(), stop: AtomicBool::new(false), removed: AtomicBool::new(false), wake: (Mutex::new(()), Condvar::new()) });
        let worker = shared.clone();
        let thread = std::thread::Builder::new().name("EssX-Discord-Console".to_owned()).spawn(move || worker.run(path)).expect("thread");
        Self { shared, thread: Some(thread) }
    }
    /// `isRemoved`.
    pub fn is_removed(&self) -> bool {
        self.shared.removed.load(Ordering::SeqCst)
    }
    /// `remove`: the thread is gone when this returns.
    pub fn remove(mut self) {
        self.shared.removed.store(true, Ordering::SeqCst);
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.wake.1.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_s_lines_come_apart() {
        assert_eq!(parse_line("[12:00:01] [Server thread/WARN]: [Essentials] look out"), ("WARN".to_owned(), "[Essentials] look out".to_owned()));
        assert_eq!(parse_line("[12:00:01] [User Authenticator #1/INFO]: hi: there"), ("INFO".to_owned(), "hi: there".to_owned()));
        assert_eq!(parse_line("    at some.stack.Trace"), ("INFO".to_owned(), "    at some.stack.Trace".to_owned()));
    }

    #[test]
    fn long_lines_split() {
        assert_eq!(split_fixed("abcde", 2), ["ab", "cd", "e"]);
    }

    #[test]
    fn drain_joins_up_to_the_limit() {
        let mut queue: VecDeque<String> = ["a".repeat(1000), "b".repeat(1000), "c".to_owned()].into_iter().collect();
        let mut sent = Vec::new();
        drain(&mut queue, |content| sent.push(content));
        assert_eq!(sent, [format!("\n{}", "a".repeat(1000)), format!("\n{}\nc", "b".repeat(1000))]);
        assert!(queue.is_empty());
    }

    #[test]
    fn the_tail_sees_only_what_is_new_and_whole() {
        let folder = std::env::temp_dir().join(format!("essx-tail-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("latest.log");
        std::fs::write(&path, "old line\n").unwrap();
        let mut tail = Tail::new(&path);
        assert!(tail.read_lines().is_empty());
        let append = |text: &str| {
            use std::io::Write;
            std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(text.as_bytes()).unwrap();
        };
        append("one\ntw");
        assert_eq!(tail.read_lines(), ["one"]);
        append("o\r\n");
        assert_eq!(tail.read_lines(), ["two"]);
        std::fs::write(&path, "new\n").unwrap();
        assert_eq!(tail.read_lines(), ["new"]);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
