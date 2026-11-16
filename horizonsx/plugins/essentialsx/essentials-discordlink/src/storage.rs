//! `AccountStorage`: `accounts.json`, the same file, and a thread that saves it when it's changed.
use essentials::prelude::*;
use essentials_discord::host::{self, Host, Level};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const SAVE_DELAY: Duration = Duration::from_secs(10);

struct Shared {
    file: PathBuf,
    /// Guava's `BiMap`: UUID to Discord id, each side unique.
    map: Mutex<BTreeMap<String, String>>,
    dirty: AtomicBool,
    stop: AtomicBool,
    wake: (Mutex<()>, Condvar),
    host: Arc<dyn Host>,
}

impl Shared {
    fn map(&self) -> MutexGuard<'_, BTreeMap<String, String>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn write(&self) -> std::io::Result<()> {
        let text = serde_json::to_string(&*self.map())?;
        std::fs::write(&self.file, text)
    }
    fn save(&self) {
        if !self.dirty.swap(false, Ordering::SeqCst) {
            return;
        }
        if self.host.is_debug() {
            host::log(&self.host, Level::Info, "Saving linked discord accounts to disk...");
        }
        if let Err(error) = self.write() {
            host::log(&self.host, Level::Severe, &format!("Failed to save link accounts! {error}"));
            // Mark the map as dirty and pray it fixes itself :D
            self.dirty.store(true, Ordering::SeqCst);
        }
    }
    fn run(&self) {
        while !self.stop.load(Ordering::SeqCst) {
            let until = Instant::now() + SAVE_DELAY;
            let mut guard = self.wake.0.lock().unwrap_or_else(|e| e.into_inner());
            while !self.stop.load(Ordering::SeqCst) {
                let Some(left) = until.checked_duration_since(Instant::now()) else {
                    break;
                };
                guard = self.wake.1.wait_timeout(guard, left).unwrap_or_else(|e| e.into_inner()).0;
            }
            drop(guard);
            if !self.stop.load(Ordering::SeqCst) {
                self.save();
            }
        }
    }
}

pub struct AccountStorage {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

/// What's in the file. Gson gives an empty file as `null`, and a value twice is an error as in `BiMap`.
fn parse(text: &str) -> Result<BTreeMap<String, String>, String> {
    if text.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let map: Option<BTreeMap<String, String>> = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let map = map.unwrap_or_default();
    let mut seen: Vec<&String> = map.values().collect();
    seen.sort();
    fail_if!(seen.windows(2).any(|pair| pair[0] == pair[1]), "a Discord id is linked twice".to_owned());
    Ok(map)
}

impl AccountStorage {
    pub fn new(folder: PathBuf, host: Arc<dyn Host>) -> Result<Self, String> {
        std::fs::create_dir_all(&folder).map_err(|_| "Unable to create account file!".to_owned())?;
        let file = folder.join("accounts.json");
        if !file.exists() {
            std::fs::write(&file, "").map_err(|_| "Unable to create account file!".to_owned())?;
        }
        let text = std::fs::read_to_string(&file).map_err(|error| error.to_string())?;
        let shared = Arc::new(Shared { file, map: Mutex::new(parse(&text)?), dirty: AtomicBool::new(false), stop: AtomicBool::new(false), wake: (Mutex::new(()), Condvar::new()), host });
        let worker = shared.clone();
        let thread = std::thread::Builder::new().name("EssX-DiscordLink-Accounts".to_owned()).spawn(move || worker.run()).map_err(|error| error.to_string())?;
        Ok(Self { shared, thread: Mutex::new(Some(thread)) })
    }
    /// `getRawStorageMap`.
    pub fn get_raw_storage_map(&self) -> BTreeMap<String, String> {
        self.shared.map().clone()
    }
    /// `add`: `forcePut`, so neither side stays linked to anyone else.
    pub fn add(&self, uuid: &str, discord_id: &str) {
        let mut map = self.shared.map();
        map.retain(|_, id| id != discord_id);
        map.insert(uuid.to_owned(), discord_id.to_owned());
        drop(map);
        self.queue_save();
    }
    /// `remove(UUID)`.
    pub fn remove_uuid(&self, uuid: &str) -> bool {
        let removed = self.shared.map().remove(uuid).is_some();
        if removed {
            self.queue_save();
        }
        removed
    }
    /// `remove(String)`.
    pub fn remove_discord_id(&self, discord_id: &str) -> bool {
        let mut map = self.shared.map();
        let before = map.len();
        map.retain(|_, id| id != discord_id);
        let removed = map.len() != before;
        drop(map);
        if removed {
            self.queue_save();
        }
        removed
    }
    /// `getUUID`.
    pub fn get_uuid(&self, discord_id: &str) -> Option<String> {
        self.shared.map().iter().find(|(_, id)| *id == discord_id).map(|(uuid, _)| uuid.clone())
    }
    /// `getDiscordId`.
    pub fn get_discord_id(&self, uuid: &str) -> Option<String> {
        self.shared.map().get(uuid).cloned()
    }
    pub fn queue_save(&self) {
        self.shared.dirty.store(true, Ordering::SeqCst);
    }
    /// `shutdown`: the thread is gone, and what's unsaved is on disk.
    pub fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.wake.1.notify_all();
        let thread = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
        if self.shared.dirty.load(Ordering::SeqCst)
            && let Err(error) = self.shared.write()
        {
            host::log(&self.shared.host, Level::Severe, &format!("Failed to shutdown link accounts save! {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use essentials_discord::host::{Feedback, Relay, Task};

    struct Quiet;

    impl Host for Quiet {
        fn run_main(&self, task: Task) {
            task();
        }
        fn is_main(&self) -> bool {
            true
        }
        fn write_log(&self, _level: Level, _text: &str) {}
        fn is_debug(&self) -> bool {
            false
        }
        fn deliver(&self, _relay: Relay) {}
        fn dispatch_command(&self, _line: &str, _feedback: Feedback) {}
    }

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("essx-link-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn parse_takes_what_gson_wrote() {
        assert!(parse("").unwrap().is_empty());
        assert!(parse("null").unwrap().is_empty());
        assert_eq!(parse(r#"{"a":"1"}"#).unwrap()["a"], "1");
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"a":"1","b":"1"}"#).is_err());
    }

    #[test]
    fn a_link_is_kept_both_ways_and_force_put() {
        let storage = AccountStorage::new(folder("both"), Arc::new(Quiet)).unwrap();
        storage.add("u1", "d1");
        storage.add("u2", "d1");
        assert_eq!(storage.get_uuid("d1").as_deref(), Some("u2"));
        assert_eq!(storage.get_discord_id("u1"), None);
        assert!(storage.remove_discord_id("d1"));
        assert!(!storage.remove_uuid("u2"));
        storage.shutdown();
    }

    #[test]
    fn shutdown_saves_and_the_next_start_reads_it() {
        let place = folder("save");
        let storage = AccountStorage::new(place.clone(), Arc::new(Quiet)).unwrap();
        storage.add("11111111-1111-1111-1111-111111111111", "123");
        storage.shutdown();
        assert_eq!(std::fs::read_to_string(place.join("accounts.json")).unwrap(), r#"{"11111111-1111-1111-1111-111111111111":"123"}"#);
        let again = AccountStorage::new(place.clone(), Arc::new(Quiet)).unwrap();
        assert_eq!(again.get_uuid("123").as_deref(), Some("11111111-1111-1111-1111-111111111111"));
        again.shutdown();
        std::fs::remove_dir_all(&place).unwrap();
    }
}
