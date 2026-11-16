//! What HorizonJobs keeps: one JSON file a player (their jobs and each job's level and exp, loaded before they
//! join, saved in batches off the server thread), a small table of everyone (counts on the job cards, /jobs top,
//! offline players), and the blocks players placed (so place-and-break pays nothing).

use horizon_kit::store::{Docs, File};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Prog {
    pub level: u32,
    /// Exp toward the next level.
    pub exp: f64,
    /// Money earned in this job, in total.
    pub earned: f64,
    /// Exp earned in this job, in total.
    pub total_exp: f64,
}

impl Prog {
    pub fn level(&self) -> u32 {
        self.level.max(1)
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// The jobs they work now (ids), in the order joined.
    pub jobs: Vec<String>,
    /// Every job they ever worked (kept when they leave, unless the config says otherwise).
    pub progress: BTreeMap<String, Prog>,
    /// Unix ms they last left a job (the join cooldown).
    pub left_at: u64,
}

impl Profile {
    pub fn works(&self, job: &str) -> bool {
        self.jobs.iter().any(|j| j == job)
    }
    pub fn prog(&self, job: &str) -> Prog {
        self.progress.get(job).cloned().unwrap_or(Prog { level: 1, ..Default::default() })
    }
}

pub fn players() -> &'static Docs<Profile> {
    static DOCS: OnceLock<Docs<Profile>> = OnceLock::new();
    DOCS.get_or_init(|| Docs::open(&horizon_kit::app::folder().join("players"), Duration::from_secs(5)))
}

pub fn get(uuid: &str) -> Profile {
    players().get(uuid)
}

pub fn edit<R>(uuid: &str, f: impl FnOnce(&mut Profile) -> R) -> R {
    players().edit(uuid, f)
}

/// One row of the table of everyone.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Summary {
    pub name: String,
    pub jobs: Vec<String>,
    /// job -> (level, exp toward next).
    pub levels: BTreeMap<String, (u32, f64)>,
}

pub fn summaries() -> &'static File<HashMap<String, Summary>> {
    static FILE: OnceLock<File<HashMap<String, Summary>>> = OnceLock::new();
    FILE.get_or_init(|| File::open(&horizon_kit::app::folder().join("workers.json"), Duration::from_secs(30)))
}

pub fn summarize(uuid: &str, p: &Profile) {
    let row = Summary { name: p.name.clone(), jobs: p.jobs.clone(), levels: p.progress.iter().map(|(k, v)| (k.clone(), (v.level(), v.exp))).collect() };
    summaries().edit(|m| {
        m.insert(uuid.to_owned(), row);
    });
}

/// How many players (online or not) work `job`, leaving out `except`.
pub fn workers(job: &str, except: &str) -> usize {
    summaries().read(|m| m.iter().filter(|(u, s)| u.as_str() != except && s.jobs.iter().any(|j| j == job)).count())
}

/// A known player's uuid and name by name (online first, then the table).
pub fn uuid_of(name: &str) -> Option<(String, String)> {
    if let Some(p) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name)) {
        return Some((p.get_unique_id(), p.get_name().to_owned()));
    }
    summaries().read(|m| m.iter().find(|(_, s)| s.name.eq_ignore_ascii_case(name)).map(|(u, s)| (u.clone(), s.name.clone())))
}

// ---- Placed blocks ----

/// Blocks players placed that a job would pay for breaking: `world x y z`. The oldest are forgotten past the cap.
struct Placed {
    set: HashSet<String>,
    order: VecDeque<String>,
    dirty: bool,
}

const PLACED_CAP: usize = 300_000;

fn placed() -> &'static Mutex<Placed> {
    static P: OnceLock<Mutex<Placed>> = OnceLock::new();
    P.get_or_init(|| {
        let list: Vec<String> = std::fs::read(horizon_kit::app::folder().join("placed.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let set = list.iter().cloned().collect();
        Mutex::new(Placed { set, order: list.into(), dirty: false })
    })
}

pub fn block_key(world: &str, x: i32, y: i32, z: i32) -> String {
    format!("{world} {x} {y} {z}")
}

pub fn mark_placed(key: String) {
    let mut p = placed().lock().unwrap_or_else(|e| e.into_inner());
    if p.set.insert(key.clone()) {
        p.order.push_back(key);
        while p.order.len() > PLACED_CAP {
            if let Some(old) = p.order.pop_front() {
                p.set.remove(&old);
            }
        }
        p.dirty = true;
    }
}

/// True (and forgotten) when a player placed it.
pub fn take_placed(key: &str) -> bool {
    let mut p = placed().lock().unwrap_or_else(|e| e.into_inner());
    if p.set.remove(key) {
        p.dirty = true;
        // The queue entry goes when it reaches the front (or is left harmlessly if placed again).
        true
    } else {
        false
    }
}

/// Writes placed.json when it changed (off the server thread).
pub fn save_placed(wait: bool) {
    let list: Option<Vec<String>> = {
        let mut p = placed().lock().unwrap_or_else(|e| e.into_inner());
        if !p.dirty {
            None
        } else {
            p.dirty = false;
            // Drop queue entries already broken (and older copies of re-placed ones) so the file only holds live ones.
            let mut seen = HashSet::new();
            let mut live: Vec<String> = p.order.iter().rev().filter(|k| p.set.contains(*k) && seen.insert((*k).clone())).cloned().collect();
            live.reverse();
            p.order = live.into();
            Some(p.order.iter().cloned().collect())
        }
    };
    let Some(list) = list else { return };
    let path = horizon_kit::app::folder().join("placed.json");
    let write = move || {
        if let Ok(bytes) = serde_json::to_vec(&list) {
            horizon_kit::store::write_atomic(&path, &bytes);
        }
    };
    if wait {
        write();
    } else {
        let _ = std::thread::Builder::new().name("HorizonJobs-placed".into()).spawn(write);
    }
}
