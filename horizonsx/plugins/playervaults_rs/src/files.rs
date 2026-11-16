//! The vault files as PlayerVaultsX keeps them: `newvaults/<uuid>.yml`, one `vault<N>` key per vault holding
//! base64 of CardboardBox's `writeInventory` (an int count, then each slot's length and Paper's
//! `ItemStack.serializeAsBytes`, `[0]` for an empty slot). Files are read and written on one I/O thread, in
//! order; item bytes are made and read back by the game on the server thread.

use base64::Engine;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

/// One vault: each slot's item bytes (`None` = empty).
pub type Slots = Vec<Option<Vec<u8>>>;

/// A player's file: vault number -> its slots (as stored).
pub type VaultFile = BTreeMap<u32, Slots>;

/// `CardboardBoxSerialization.writeInventory`.
pub fn encode(slots: &Slots) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + slots.len() * 8);
    out.extend_from_slice(&(slots.len() as i32).to_be_bytes());
    for s in slots {
        match s {
            Some(b) if !b.is_empty() => {
                out.extend_from_slice(&(b.len() as i32).to_be_bytes());
                out.extend_from_slice(b);
            }
            _ => {
                out.extend_from_slice(&1i32.to_be_bytes());
                out.push(0);
            }
        }
    }
    out
}

/// `fromStorage`: the slots of a stored vault, or `None` when it can't be read.
pub fn decode(text: &str) -> Option<Slots> {
    let clean: String = text.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')).collect();
    if clean.is_empty() {
        return Some(vec![None; 54]);
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(clean.as_bytes()).ok()?;
    let word = |at: usize| Some(i32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let count = usize::try_from(word(0)?).ok()?;
    let mut at = 4;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let len = usize::try_from(word(at)?).ok()?;
        at += 4;
        let item = bytes.get(at..at + len)?;
        at += len;
        out.push((len > 1 || item.first().is_some_and(|b| *b != 0)).then(|| item.to_vec()));
    }
    Some(out)
}

fn path_of(dir: &Path, owner: &str) -> PathBuf {
    dir.join(format!("{owner}.yml"))
}

/// A file read: `vault<N>` keys decoded (unreadable vaults left out and reported).
pub fn read(dir: &Path, owner: &str) -> (VaultFile, Vec<String>) {
    let mut out = VaultFile::new();
    let mut errors = Vec::new();
    let Ok(text) = std::fs::read_to_string(path_of(dir, owner)) else { return (out, errors) };
    let Ok(doc) = horizon_kit::yaml::parse(&text) else {
        errors.push(format!("Failed to load items for {owner}: the file is not valid YAML"));
        return (out, errors);
    };
    if let serde_yaml::Value::Mapping(m) = doc {
        for (k, v) in m {
            let Some(n) = k.as_str().and_then(|k| k.strip_prefix("vault")).and_then(|n| n.parse::<u32>().ok()) else { continue };
            match v.as_str().and_then(decode) {
                Some(slots) => {
                    out.insert(n, slots);
                }
                None => errors.push(format!("Failed to load items for {owner} vault {n}")),
            }
        }
    }
    (out, errors)
}

fn render(file: &VaultFile) -> String {
    let mut s = String::new();
    for (n, slots) in file {
        let b64 = base64::engine::general_purpose::STANDARD.encode(encode(slots));
        s.push_str(&format!("vault{n}: '{b64}'\n"));
    }
    s
}

enum Job {
    Save { owner: String, file: VaultFile },
    Delete { owner: String },
    Read { owner: String, done: Box<dyn FnOnce(VaultFile, Vec<String>) + Send> },
    Write { path: PathBuf, text: String },
}

static TX: Mutex<Option<Sender<Job>>> = Mutex::new(None);
static HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

/// The I/O thread for `dir` (newvaults), backups into `dir/backups` when `backups`.
pub fn start(dir: PathBuf, backups: bool) {
    let (tx, rx) = channel::<Job>();
    let handle = std::thread::Builder::new()
        .name("playervaults-io".into())
        .spawn(move || {
            let _ = std::fs::create_dir_all(&dir);
            for job in rx {
                match job {
                    Job::Save { owner, file } => {
                        let path = path_of(&dir, &owner);
                        if backups && path.exists() {
                            let b = dir.join("backups");
                            let _ = std::fs::create_dir_all(&b);
                            let _ = std::fs::rename(&path, b.join(format!("{owner}.yml")));
                        }
                        let tmp = path.with_extension("yml.tmp");
                        if std::fs::write(&tmp, render(&file)).is_ok() {
                            let _ = std::fs::rename(&tmp, &path);
                        }
                    }
                    Job::Delete { owner } => {
                        let _ = std::fs::remove_file(path_of(&dir, &owner));
                    }
                    Job::Read { owner, done } => {
                        let (file, errors) = read(&dir, &owner);
                        done(file, errors);
                    }
                    Job::Write { path, text } => {
                        let tmp = path.with_extension("tmp");
                        if std::fs::write(&tmp, text).is_ok() {
                            let _ = std::fs::rename(&tmp, &path);
                        }
                    }
                }
            }
        })
        .ok();
    *TX.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    *HANDLE.lock().unwrap_or_else(|e| e.into_inner()) = handle;
}

fn send(job: Job) {
    if let Some(tx) = TX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(job);
    }
}

pub fn save(owner: &str, file: VaultFile) {
    send(Job::Save { owner: owner.to_owned(), file });
}

pub fn delete(owner: &str) {
    send(Job::Delete { owner: owner.to_owned() });
}

pub fn load(owner: &str, done: impl FnOnce(VaultFile, Vec<String>) + Send + 'static) {
    send(Job::Read { owner: owner.to_owned(), done: Box::new(done) });
}

pub fn write(path: PathBuf, text: String) {
    send(Job::Write { path, text });
}

/// Everything queued written, the thread ended.
pub fn stop() {
    *TX.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if let Some(h) = HANDLE.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = h.join();
    }
}

/// `Cleanup`: vault files not changed in `days` deleted (backups kept).
pub fn purge(dir: &Path, days: i64) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let limit = std::time::Duration::from_secs((days.max(0) as u64) * 86_400);
    let mut n = 0;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "yml")
            && e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > limit)
            && std::fs::remove_file(&p).is_ok()
        {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    #[test]
    fn round_trip() {
        let slots: super::Slots = vec![Some(vec![1, 2, 3]), None, Some(vec![9; 40])];
        let b64 = base64::engine::general_purpose::STANDARD.encode(super::encode(&slots));
        // Java's MIME encoder breaks lines; they are ignored on the way back.
        let mime: String = b64.chars().collect::<Vec<_>>().chunks(76).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("\r\n");
        assert_eq!(super::decode(&mime), Some(slots));
    }
}
