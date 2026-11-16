//! Cooldowns in `data/database.db`, the Java plugin's own SQLite file and tables (`Players`: uuid, count,
//! last_rtp_date; one table per world: uuid, date), so cooldowns carry over. Every read and write runs on one
//! database thread; answers come back to the server thread through the scheduler.

use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::Mutex;

#[derive(Clone, Default, Debug)]
pub struct PlayerData {
    pub count: i64,
    /// Milliseconds of the last rtp (0 = none).
    pub global: i64,
    pub worlds: HashMap<String, i64>,
}

pub enum Job {
    Tables(Vec<String>),
    Load(String, Vec<String>),
    SetPlayer(String, i64, i64),
    SetCooldown(String, String, i64),
    RemoveCooldown(String, String),
    Stop,
}

static TX: Mutex<Option<(Sender<Job>, std::thread::JoinHandle<()>)>> = Mutex::new(None);

/// What the server thread knows of each online player.
pub static CACHE: Mutex<Option<HashMap<String, PlayerData>>> = Mutex::new(None);

pub fn start(file: PathBuf) {
    let (tx, rx) = channel::<Job>();
    let handle = std::thread::Builder::new()
        .name("BetterRTP database".into())
        .spawn(move || {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let Ok(conn) = Connection::open(&file) else {
                crate::log(&format!("Could not open {}", file.display()));
                return;
            };
            let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; CREATE TABLE IF NOT EXISTS `Players` (`uuid` varchar(32) PRIMARY KEY, `count` long, `last_rtp_date` long);");
            while let Ok(job) = rx.recv() {
                // A burst of writes is one transaction.
                let mut batch = vec![job];
                batch.extend(rx.try_iter());
                let _ = conn.execute_batch("BEGIN");
                let mut stop = false;
                for job in batch {
                    match job {
                        Job::Tables(worlds) => {
                            for w in worlds {
                                let _ = conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS `{}` (`uuid` varchar(32) PRIMARY KEY, `date` long);", w.replace('`', "")));
                            }
                        }
                        Job::Load(uuid, worlds) => {
                            let mut data = PlayerData::default();
                            if let Ok(mut st) = conn.prepare_cached("SELECT `count`, `last_rtp_date` FROM `Players` WHERE `uuid` = ?1")
                                && let Ok((c, t)) = st.query_row(params![uuid], |r| Ok((r.get::<_, i64>(0).unwrap_or(0), r.get::<_, i64>(1).unwrap_or(0))))
                            {
                                data.count = c;
                                data.global = t;
                            }
                            for w in worlds {
                                let sql = format!("SELECT `date` FROM `{}` WHERE `uuid` = ?1", w.replace('`', ""));
                                if let Ok(mut st) = conn.prepare(&sql)
                                    && let Ok(t) = st.query_row(params![uuid], |r| r.get::<_, i64>(0))
                                {
                                    data.worlds.insert(w, t);
                                }
                            }
                            crate::on_main(move || {
                                CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(uuid, data);
                            });
                        }
                        Job::SetPlayer(uuid, count, last) => {
                            let _ = conn.execute("INSERT OR REPLACE INTO `Players` (`uuid`, `count`, `last_rtp_date`) VALUES (?1, ?2, ?3)", params![uuid, count, last]);
                        }
                        Job::SetCooldown(world, uuid, date) => {
                            let w = world.replace('`', "");
                            let _ = conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS `{w}` (`uuid` varchar(32) PRIMARY KEY, `date` long);"));
                            let _ = conn.execute(&format!("INSERT OR REPLACE INTO `{w}` (`uuid`, `date`) VALUES (?1, ?2)"), params![uuid, date]);
                        }
                        Job::RemoveCooldown(world, uuid) => {
                            let _ = conn.execute(&format!("DELETE FROM `{}` WHERE `uuid` = ?1", world.replace('`', "")), params![uuid]);
                        }
                        Job::Stop => stop = true,
                    }
                }
                let _ = conn.execute_batch("COMMIT");
                if stop {
                    return;
                }
            }
        })
        .ok();
    if let Some(h) = handle {
        *TX.lock().unwrap_or_else(|e| e.into_inner()) = Some((tx, h));
    }
}

pub fn send(job: Job) {
    if let Some((tx, _)) = TX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(job);
    }
}

pub fn stop() {
    if let Some((tx, h)) = TX.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = tx.send(Job::Stop);
        let _ = h.join();
    }
}

pub fn get(uuid: &str) -> Option<PlayerData> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(uuid).cloned())
}

pub fn update(uuid: &str, f: impl FnOnce(&mut PlayerData)) -> PlayerData {
    let mut held = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let d = held.get_or_insert_with(HashMap::new).entry(uuid.to_owned()).or_default();
    f(d);
    d.clone()
}

pub fn forget(uuid: &str) {
    if let Some(m) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        m.remove(uuid);
    }
}
