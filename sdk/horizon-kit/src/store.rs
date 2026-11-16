//! Data that outlives the server, kept in memory and written off the server thread.
//!
//! - [`Docs`]: one JSON document per key (a player's uuid, a shop id) in a folder: `load` (call it at
//!   `AsyncPlayerPreLoginEvent` or from any thread), `get`/`edit` from memory, `edit` marks it dirty, a writer
//!   thread saves dirty documents in batches (written beside, then renamed) a few seconds later; `unload` on
//!   quit; `save_all` on disable waits for the writes. Typed with serde (`#[derive(Serialize, Deserialize,
//!   Default, Clone)] struct PlayerData { .. }`).
//! - [`File`]: one JSON file holding one value (a plugin's whole small database), same writer, same rules.
//!
//! Nothing here asks the game: safe from any thread.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

enum Job {
    Write(PathBuf, Vec<u8>),
    Flush(mpsc::Sender<()>),
}

/// One writer thread per process for all stores: writes are small and in order.
fn writer() -> Sender<Job> {
    static TX: Mutex<Option<Sender<Job>>> = Mutex::new(None);
    let mut held = TX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(tx) = held.as_ref() {
        return tx.clone();
    }
    let (tx, rx) = mpsc::channel::<Job>();
    let _ = std::thread::Builder::new().name("kit-store-writer".into()).spawn(move || {
        for job in rx {
            match job {
                Job::Write(path, bytes) => write_atomic(&path, &bytes),
                Job::Flush(done) => {
                    let _ = done.send(());
                }
            }
        }
    });
    *held = Some(tx.clone());
    tx
}

/// Written beside, then moved over: a crash mid-write never leaves half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Waits (up to 10 s) until every write handed over so far is on disk.
pub fn flush() {
    let (tx, rx) = mpsc::channel();
    if writer().send(Job::Flush(tx)).is_ok() {
        let _ = rx.recv_timeout(Duration::from_secs(10));
    }
}

fn file_name(key: &str) -> String {
    key.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect()
}

struct Inner<T> {
    docs: HashMap<String, T>,
    dirty: HashSet<String>,
}

/// A folder of JSON documents by key, in memory once loaded.
pub struct Docs<T> {
    dir: PathBuf,
    inner: Arc<Mutex<Inner<T>>>,
}

impl<T: Serialize + DeserializeOwned + Default + Clone + Send + 'static> Docs<T> {
    /// `folder/<key>.json`; a saver thread writes dirty documents every `every` (e.g. 5 s).
    pub fn open(folder: &Path, every: Duration) -> Docs<T> {
        let inner = Arc::new(Mutex::new(Inner { docs: HashMap::new(), dirty: HashSet::new() }));
        let docs = Docs { dir: folder.to_owned(), inner: inner.clone() };
        let dir = folder.to_owned();
        let weak = Arc::downgrade(&inner);
        let _ = std::thread::Builder::new().name("kit-docs-saver".into()).spawn(move || {
            loop {
                std::thread::sleep(every);
                let Some(inner) = weak.upgrade() else { return };
                save_dirty(&dir, &inner);
            }
        });
        docs
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{}.json", file_name(key)))
    }

    /// Reads the key's file into memory (once; a missing file is `T::default()`). Blocking disk read: call it
    /// off the server thread (pre-login) or accept a short read.
    pub fn load(&self, key: &str) {
        if self.inner.lock().unwrap_or_else(|e| e.into_inner()).docs.contains_key(key) {
            return;
        }
        let doc: T = std::fs::read(self.path(key)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).docs.entry(key.to_owned()).or_insert(doc);
    }

    /// A copy of the document (loaded if it wasn't).
    pub fn get(&self, key: &str) -> T {
        self.load(key);
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).docs.get(key).cloned().unwrap_or_default()
    }

    /// Changes the document in memory; it is saved with the next batch.
    pub fn edit<R>(&self, key: &str, f: impl FnOnce(&mut T) -> R) -> R {
        self.load(key);
        let mut held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        held.dirty.insert(key.to_owned());
        f(held.docs.entry(key.to_owned()).or_default())
    }

    /// Reads without loading or marking (None when not in memory).
    pub fn peek<R>(&self, key: &str, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).docs.get(key).map(f)
    }

    /// Every document in memory (online players, usually).
    pub fn loaded(&self) -> Vec<(String, T)> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).docs.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Saves it (if changed) and drops it from memory (on quit).
    pub fn unload(&self, key: &str) {
        let mut held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if held.dirty.remove(key)
            && let Some(doc) = held.docs.get(key)
            && let Ok(bytes) = serde_json::to_vec_pretty(doc)
        {
            let _ = writer().send(Job::Write(self.path(key), bytes));
        }
        held.docs.remove(key);
    }

    /// Deletes the key's document from memory and disk.
    pub fn remove(&self, key: &str) {
        let mut held = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        held.docs.remove(key);
        held.dirty.remove(key);
        let _ = std::fs::remove_file(self.path(key));
    }

    /// Every key with a file on disk (offline players too).
    pub fn keys_on_disk(&self) -> Vec<String> {
        std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.strip_suffix(".json").map(str::to_owned))
            .collect()
    }

    /// Writes everything changed and waits for it (on_disable).
    pub fn save_all(&self) {
        save_dirty(&self.dir, &self.inner);
        flush();
    }
}

fn save_dirty<T: Serialize>(dir: &Path, inner: &Mutex<Inner<T>>) {
    let batch: Vec<(String, Vec<u8>)> = {
        let mut held = inner.lock().unwrap_or_else(|e| e.into_inner());
        let keys: Vec<String> = held.dirty.drain().collect();
        keys.into_iter().filter_map(|k| Some((k.clone(), serde_json::to_vec_pretty(held.docs.get(&k)?).ok()?))).collect()
    };
    let tx = writer();
    for (k, bytes) in batch {
        let _ = tx.send(Job::Write(dir.join(format!("{}.json", file_name(&k))), bytes));
    }
}

/// One JSON file holding one value, in memory, saved off-thread after changes.
pub struct File<T> {
    path: PathBuf,
    value: Arc<Mutex<(T, bool)>>,
}

impl<T: Serialize + DeserializeOwned + Default + Clone + Send + 'static> File<T> {
    /// Reads it now (missing or broken = default) and saves changes every `every`.
    pub fn open(path: &Path, every: Duration) -> File<T> {
        let v: T = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let value = Arc::new(Mutex::new((v, false)));
        let weak = Arc::downgrade(&value);
        let p = path.to_owned();
        let _ = std::thread::Builder::new().name("kit-file-saver".into()).spawn(move || {
            loop {
                std::thread::sleep(every);
                let Some(v) = weak.upgrade() else { return };
                save_file(&p, &v);
            }
        });
        File { path: path.to_owned(), value }
    }
    pub fn get(&self) -> T {
        self.value.lock().unwrap_or_else(|e| e.into_inner()).0.clone()
    }
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.value.lock().unwrap_or_else(|e| e.into_inner()).0)
    }
    pub fn edit<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut held = self.value.lock().unwrap_or_else(|e| e.into_inner());
        held.1 = true;
        f(&mut held.0)
    }
    pub fn save_now(&self) {
        save_file(&self.path, &self.value);
        flush();
    }
}

fn save_file<T: Serialize>(path: &Path, v: &Mutex<(T, bool)>) {
    let bytes = {
        let mut held = v.lock().unwrap_or_else(|e| e.into_inner());
        if !held.1 {
            return;
        }
        held.1 = false;
        serde_json::to_vec_pretty(&held.0).ok()
    };
    if let Some(b) = bytes {
        let _ = writer().send(Job::Write(path.to_owned(), b));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug, PartialEq)]
    struct D {
        coins: i64,
    }
    #[test]
    fn docs_round_trip() {
        let dir = std::env::temp_dir().join(format!("kit_store_{}", std::process::id()));
        let docs: Docs<D> = Docs::open(&dir, Duration::from_secs(60));
        docs.edit("a-b", |d| d.coins = 5);
        docs.save_all();
        let again: Docs<D> = Docs::open(&dir, Duration::from_secs(60));
        assert_eq!(again.get("a-b").coins, 5);
        assert_eq!(again.keys_on_disk(), vec!["a-b".to_owned()]);
        let f: File<Vec<i32>> = File::open(&dir.join("all.json"), Duration::from_secs(60));
        f.edit(|v| v.push(3));
        f.save_now();
        let g: File<Vec<i32>> = File::open(&dir.join("all.json"), Duration::from_secs(60));
        assert_eq!(g.get(), vec![3]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
