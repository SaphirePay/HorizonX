//! Small shared parts: the messages (Multiverse's `.properties` locale files), command flags, the server-thread hop
//! and the file writer thread.

use horizon_bukkit::CommandSender;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, RwLock};

// ---- Messages ----

static MESSAGES: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);
const EN: &str = include_str!("../data/multiverse-core_en.properties");

/// Java `.properties`: `key=value`, `\uXXXX`, `\n`, `\ ` and line continuations.
pub fn parse_properties(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut pending = String::new();
    for raw in text.lines() {
        let line = raw.trim_start();
        if pending.is_empty() && (line.is_empty() || line.starts_with('#') || line.starts_with('!')) {
            continue;
        }
        let continued = line.ends_with('\\') && !line.ends_with("\\\\");
        pending.push_str(if continued { &line[..line.len() - 1] } else { line });
        if continued {
            continue;
        }
        let entry = std::mem::take(&mut pending);
        let split = entry.char_indices().find(|&(i, c)| (c == '=' || c == ':') && !entry[..i].ends_with('\\')).map(|(i, _)| i);
        let Some(at) = split else { continue };
        out.insert(unescape(entry[..at].trim()), unescape(entry[at + 1..].trim_start()));
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// The English messages, with any `multiverse-core_<locale>.properties` the owner put in the data folder over them.
pub fn load_messages(folder: &std::path::Path, locale: &str) {
    let mut map = parse_properties(EN);
    for name in [format!("multiverse-core_{locale}.properties"), "multiverse-core_en.properties".to_owned()] {
        if let Ok(text) = std::fs::read_to_string(folder.join(&name)) {
            map.extend(parse_properties(&text));
            break;
        }
    }
    *MESSAGES.write().unwrap_or_else(|e| e.into_inner()) = Some(map);
}

/// `mv-core.<key>` with `{name}` replacements, colours applied.
pub fn m(key: &str, pairs: &[(&str, &str)]) -> String {
    let full = if key.starts_with("mv-core.") { key.to_owned() } else { format!("mv-core.{key}") };
    let raw = MESSAGES.read().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&full).cloned()).unwrap_or(full);
    let mut text = raw;
    for (k, v) in pairs {
        text = text.replace(&format!("{{{k}}}"), v);
    }
    horizon_kit::text::colorize(&text)
}

/// Sends a message (each `\n` its own line).
pub fn tell(to: &CommandSender, text: &str) {
    for line in text.split('\n') {
        to.send_message(line);
    }
}

pub fn say(to: &CommandSender, key: &str, pairs: &[(&str, &str)]) {
    tell(to, &m(key, pairs));
}

// ---- Command flags (`--seed 5`, `-n`) ----

pub struct Flags {
    pub set: HashMap<String, Option<String>>,
}

impl Flags {
    /// `flags`: (name, alias, takes a value). Values that look like another flag aren't taken.
    pub fn parse(args: &[String], known: &[(&str, &str, bool)]) -> Result<Flags, String> {
        let mut set = HashMap::new();
        let mut i = 0;
        while i < args.len() {
            let a = args[i].as_str();
            let Some(&(name, _, takes)) = known.iter().find(|(n, al, _)| a.eq_ignore_ascii_case(n) || (!al.is_empty() && a.eq_ignore_ascii_case(al))) else {
                return Err(format!("&cUnknown flag '{a}'."));
            };
            if takes && i + 1 < args.len() && !args[i + 1].starts_with('-') {
                set.insert(name.to_owned(), Some(args[i + 1].clone()));
                i += 2;
            } else {
                set.insert(name.to_owned(), None);
                i += 1;
            }
        }
        Ok(Flags { set })
    }
    pub fn has(&self, name: &str) -> bool {
        self.set.contains_key(name)
    }
    pub fn value(&self, name: &str) -> Option<&str> {
        self.set.get(name).and_then(|v| v.as_deref())
    }
}

/// Splits arguments at the first one starting with `-` (positional, flags).
pub fn split_flags(args: &[String]) -> (Vec<String>, Vec<String>) {
    let at = args.iter().position(|a| a.starts_with("--") || (a.starts_with('-') && a.len() > 1 && !a[1..].chars().next().is_some_and(|c| c.is_ascii_digit()))).unwrap_or(args.len());
    (args[..at].to_vec(), args[at..].to_vec())
}

// ---- Threads ----

/// Runs `f` on the server thread at the next tick (the SDK scheduler only queues: safe from any thread).
pub fn on_main(f: impl FnOnce() + Send + 'static) {
    let mut f = Some(f);
    horizon_bukkit::scheduler::run_task(move || {
        if let Some(f) = f.take() {
            f();
        }
    });
}

enum Write {
    File(PathBuf, String),
    Stop,
}

static WRITER: Mutex<Option<(Sender<Write>, std::thread::JoinHandle<()>)>> = Mutex::new(None);

/// The file writer: writes run in order on one thread; a later write of the same file replaces a queued one.
pub fn start_writer() {
    let (tx, rx) = channel::<Write>();
    let handle = std::thread::Builder::new()
        .name("Multiverse-Core writer".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut batch = vec![first];
                batch.extend(rx.try_iter());
                let mut latest: Vec<(PathBuf, String)> = Vec::new();
                let mut stop = false;
                for w in batch {
                    match w {
                        Write::File(p, t) => {
                            latest.retain(|(q, _)| *q != p);
                            latest.push((p, t));
                        }
                        Write::Stop => stop = true,
                    }
                }
                for (p, t) in latest {
                    let tmp = p.with_extension("yml.tmp");
                    if std::fs::write(&tmp, &t).is_ok() {
                        let _ = std::fs::rename(&tmp, &p);
                    } else {
                        let _ = std::fs::write(&p, &t);
                    }
                }
                if stop {
                    return;
                }
            }
        })
        .ok();
    if let Some(h) = handle {
        *WRITER.lock().unwrap_or_else(|e| e.into_inner()) = Some((tx, h));
    }
}

pub fn write_file(path: PathBuf, text: String) {
    match WRITER.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some((tx, _)) => {
            let _ = tx.send(Write::File(path, text));
        }
        None => {
            let _ = std::fs::write(path, text);
        }
    }
}

/// Writes what's queued and ends the thread (on disable).
pub fn stop_writer() {
    if let Some((tx, h)) = WRITER.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = tx.send(Write::Stop);
        let _ = h.join();
    }
}

pub fn log(text: &str) {
    horizon_bukkit::horizon_mod::log(&format!("[Multiverse-Core] {text}"));
}

/// Java's `DecimalFormat` with 0-2 fraction digits.
pub fn df(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_owned() }
}
