//! The boards: every player's total per board, and the totals at each timed type's last reset (a timed value
//! is total minus that). One thread owns them; the server thread sends it batches of readings and reads sorted
//! snapshots (`Arc`, swapped after each batch). Saved to `cache.json` (written beside, then renamed) at most once
//! a minute and on stop.

use chrono::{Datelike, Duration, Local, TimeZone, Timelike, Weekday};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

pub const TYPES: [&str; 6] = ["alltime", "hourly", "daily", "weekly", "monthly", "yearly"];

pub struct Update {
    pub uuid: String,
    pub name: String,
    pub display: String,
    pub prefix: String,
    pub suffix: String,
    pub values: Vec<(String, f64)>,
    pub extras: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct Row {
    pub name: String,
    pub display: String,
    pub prefix: String,
    pub suffix: String,
    pub total: f64,
    /// Total at the last reset, per timed type (hourly..yearly).
    pub base: [f64; 5],
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub uuid: String,
    pub name: String,
    pub display: String,
    pub prefix: String,
    pub suffix: String,
    pub value: f64,
}

/// What placeholders read: per (board, type) the entries best first, and positions.
#[derive(Default)]
pub struct Snapshot {
    boards: Vec<String>,
    sorted: HashMap<(String, String), Vec<Entry>>,
    positions: HashMap<(String, String), HashMap<String, usize>>,
    extras: HashMap<String, HashMap<String, String>>,
}

impl Snapshot {
    /// The board's name as stored, from a lower-case placeholder.
    pub fn board_name(&self, lower: &str) -> Option<String> {
        self.boards.iter().find(|b| b.eq_ignore_ascii_case(lower)).cloned()
    }
    pub fn top(&self, board: &str, kind: &str, index: usize) -> Option<&Entry> {
        self.sorted.get(&(board.to_owned(), kind.to_owned()))?.get(index)
    }
    pub fn position(&self, board: &str, kind: &str, uuid: &str) -> Option<usize> {
        self.positions.get(&(board.to_owned(), kind.to_owned()))?.get(uuid).copied()
    }
    pub fn value(&self, board: &str, kind: &str, uuid: &str) -> Option<f64> {
        let i = self.position(board, kind, uuid)?;
        self.top(board, kind, i - 1).map(|e| e.value)
    }
    pub fn total(&self, board: &str, kind: &str) -> f64 {
        self.sorted.get(&(board.to_owned(), kind.to_owned())).map_or(0.0, |v| v.iter().map(|e| e.value).sum())
    }
    pub fn size(&self, board: &str) -> usize {
        self.sorted.get(&(board.to_owned(), "alltime".to_owned())).map_or(0, Vec::len)
    }
    pub fn extra(&self, uuid: &str, extra: &str) -> Option<String> {
        self.extras.get(uuid)?.iter().find(|(k, _)| k.eq_ignore_ascii_case(extra)).map(|(_, v)| v.clone())
    }
}

#[derive(Default)]
struct Data {
    boards: BTreeMap<String, HashMap<String, Row>>,
    /// Board -> type -> next reset (unix seconds).
    resets: HashMap<String, HashMap<String, i64>>,
    extras: HashMap<String, HashMap<String, String>>,
}

enum Msg {
    Batch(Vec<Update>),
    Edit(Box<dyn FnOnce(&mut Data) + Send>),
    Stop,
}

static SNAP: RwLock<Option<Arc<Snapshot>>> = RwLock::new(None);
static TX: Mutex<Option<Sender<Msg>>> = Mutex::new(None);
static DONE: Mutex<Option<Receiver<()>>> = Mutex::new(None);
static BOARDS: RwLock<Vec<String>> = RwLock::new(Vec::new());

pub fn snapshot() -> Arc<Snapshot> {
    SNAP.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

pub fn boards() -> Vec<String> {
    BOARDS.read().unwrap_or_else(|e| e.into_inner()).clone()
}

fn type_index(kind: &str) -> Option<usize> {
    TYPES.iter().position(|t| *t == kind).and_then(|i| i.checked_sub(1))
}

/// When a timed type next rolls over (`TimedType.getNextReset`).
pub fn next_reset(kind: &str, weekly_on: Weekday) -> Option<i64> {
    let now = Local::now();
    let midnight = |d: chrono::DateTime<Local>| d.with_hour(0).and_then(|d| d.with_minute(0)).and_then(|d| d.with_second(0)).and_then(|d| d.with_nanosecond(0));
    let next = match kind {
        "hourly" => (now + Duration::hours(1)).with_minute(0).and_then(|d| d.with_second(0)).and_then(|d| d.with_nanosecond(0))?,
        "daily" => midnight(now + Duration::days(1))?,
        "weekly" => {
            let ahead = (weekly_on.num_days_from_monday() as i64 - now.weekday().num_days_from_monday() as i64).rem_euclid(7);
            let ahead = if ahead == 0 { 7 } else { ahead };
            midnight(now + Duration::days(ahead))?
        }
        "monthly" => {
            let (y, m) = if now.month() == 12 { (now.year() + 1, 1) } else { (now.year(), now.month() + 1) };
            Local.with_ymd_and_hms(y, m, 1, 0, 0, 0).single()?
        }
        "yearly" => Local.with_ymd_and_hms(now.year() + 1, 1, 1, 0, 0, 0).single()?,
        _ => return None,
    };
    Some(next.timestamp())
}

fn rebuild(d: &Data, reverse: &[String]) -> Snapshot {
    let mut snap = Snapshot { boards: d.boards.keys().cloned().collect(), extras: d.extras.clone(), ..Default::default() };
    for (board, rows) in &d.boards {
        let rev = reverse.iter().any(|r| r.eq_ignore_ascii_case(board));
        for (ti, t) in TYPES.iter().enumerate() {
            let mut list: Vec<Entry> = rows
                .iter()
                .map(|(uuid, r)| Entry {
                    uuid: uuid.clone(),
                    name: r.name.clone(),
                    display: r.display.clone(),
                    prefix: r.prefix.clone(),
                    suffix: r.suffix.clone(),
                    value: if ti == 0 { r.total } else { r.total - r.base[ti - 1] },
                })
                .collect();
            list.sort_by(|a, b| {
                let o = b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal);
                let o = if rev { o.reverse() } else { o };
                o.then_with(|| a.name.cmp(&b.name))
            });
            let pos: HashMap<String, usize> = list.iter().enumerate().map(|(i, e)| (e.uuid.clone(), i + 1)).collect();
            snap.positions.insert((board.clone(), (*t).to_owned()), pos);
            snap.sorted.insert((board.clone(), (*t).to_owned()), list);
        }
    }
    snap
}

fn publish(d: &Data) {
    let reverse = crate::SETTINGS.read().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.reverse.clone()).unwrap_or_default();
    *BOARDS.write().unwrap_or_else(|e| e.into_inner()) = d.boards.keys().cloned().collect();
    *SNAP.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(rebuild(d, &reverse)));
}

fn to_json(d: &Data) -> Value {
    let boards: serde_json::Map<String, Value> = d
        .boards
        .iter()
        .map(|(b, rows)| {
            let rows: serde_json::Map<String, Value> = rows
                .iter()
                .map(|(u, r)| (u.clone(), json!({ "name": r.name, "display": r.display, "prefix": r.prefix, "suffix": r.suffix, "total": r.total, "base": r.base })))
                .collect();
            (b.clone(), Value::Object(rows))
        })
        .collect();
    json!({ "boards": boards, "resets": d.resets, "extras": d.extras })
}

fn from_json(v: &Value) -> Data {
    let mut d = Data::default();
    for (b, rows) in v["boards"].as_object().into_iter().flatten() {
        let mut map = HashMap::new();
        for (u, r) in rows.as_object().into_iter().flatten() {
            let mut base = [0.0; 5];
            for (i, x) in r["base"].as_array().into_iter().flatten().enumerate().take(5) {
                base[i] = x.as_f64().unwrap_or(0.0);
            }
            let s = |k: &str| r[k].as_str().unwrap_or_default().to_owned();
            map.insert(u.clone(), Row { name: s("name"), display: s("display"), prefix: s("prefix"), suffix: s("suffix"), total: r["total"].as_f64().unwrap_or(0.0), base });
        }
        d.boards.insert(b.clone(), map);
    }
    if let Ok(r) = serde_json::from_value(v["resets"].clone()) {
        d.resets = r;
    }
    if let Ok(e) = serde_json::from_value(v["extras"].clone()) {
        d.extras = e;
    }
    d
}

fn write(path: &Path, d: &Data) {
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string(&to_json(d)).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Timed types whose reset time passed: their base becomes the current total.
fn roll(d: &mut Data, weekly_on: Weekday) -> bool {
    let now = Local::now().timestamp();
    let mut changed = false;
    let boards: Vec<String> = d.boards.keys().cloned().collect();
    for b in boards {
        for t in &TYPES[1..] {
            let due = d.resets.get(&b).and_then(|m| m.get(*t)).copied();
            match due {
                Some(at) if at > now => {}
                Some(_) => {
                    let ti = type_index(t).unwrap_or(0);
                    if let Some(rows) = d.boards.get_mut(&b) {
                        for r in rows.values_mut() {
                            r.base[ti] = r.total;
                        }
                    }
                    d.resets.entry(b.clone()).or_default().insert((*t).to_owned(), next_reset(t, weekly_on).unwrap_or(now + 3600));
                    changed = true;
                }
                None => {
                    d.resets.entry(b.clone()).or_default().insert((*t).to_owned(), next_reset(t, weekly_on).unwrap_or(now + 3600));
                }
            }
        }
    }
    changed
}

fn apply(d: &mut Data, batch: Vec<Update>) {
    let (zero_check, dont_add_zero) = crate::SETTINGS.read().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| (s.require_zero_validation, s.dont_add_zero.clone())).unwrap_or_default();
    for u in batch {
        for (board, value) in &u.values {
            let Some(rows) = d.boards.get_mut(board) else { continue };
            let skip_zero = dont_add_zero.iter().any(|b| b.eq_ignore_ascii_case(board)) || (zero_check && *value == 0.0);
            match rows.get_mut(&u.uuid) {
                Some(r) => {
                    r.total = *value;
                    (r.name, r.display, r.prefix, r.suffix) = (u.name.clone(), u.display.clone(), u.prefix.clone(), u.suffix.clone());
                }
                None if skip_zero && *value == 0.0 => {}
                None => {
                    // A new player's timed values start at zero: the base is what they have now.
                    rows.insert(u.uuid.clone(), Row { name: u.name.clone(), display: u.display.clone(), prefix: u.prefix.clone(), suffix: u.suffix.clone(), total: *value, base: [*value; 5] });
                }
            }
        }
        if !u.extras.is_empty() {
            d.extras.entry(u.uuid.clone()).or_default().extend(u.extras);
        }
    }
}

pub fn start(folder: &Path) {
    let path: PathBuf = folder.join("cache.json");
    let mut data = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| from_json(&v)).unwrap_or_default();
    let weekly_on = crate::SETTINGS.read().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(Weekday::Sun, |s| s.weekly_on);
    roll(&mut data, weekly_on);
    publish(&data);
    let (tx, rx) = mpsc::channel::<Msg>();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    *TX.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    *DONE.lock().unwrap_or_else(|e| e.into_inner()) = Some(done_rx);
    let _ = std::thread::Builder::new().name("ajLeaderboards-boards".into()).spawn(move || {
        let mut dirty = false;
        let mut saved = Instant::now();
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(30)) {
                Ok(Msg::Batch(b)) => {
                    apply(&mut data, b);
                    roll(&mut data, weekly_on);
                    publish(&data);
                    dirty = true;
                }
                Ok(Msg::Edit(f)) => {
                    f(&mut data);
                    roll(&mut data, weekly_on);
                    publish(&data);
                    dirty = true;
                }
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    if roll(&mut data, weekly_on) {
                        publish(&data);
                        dirty = true;
                    }
                }
            }
            if dirty && saved.elapsed().as_secs() >= 60 {
                write(&path, &data);
                dirty = false;
                saved = Instant::now();
            }
        }
        write(&path, &data);
        let _ = done_tx.send(());
    });
}

fn edit(f: impl FnOnce(&mut Data) + Send + 'static) {
    if let Some(tx) = TX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(Msg::Edit(Box::new(f)));
    }
}

/// Answers from the board thread, waited for (commands only).
fn ask<R: Send + 'static>(f: impl FnOnce(&mut Data) -> R + Send + 'static) -> Option<R> {
    let (tx, rx) = mpsc::channel();
    edit(move |d| {
        let _ = tx.send(f(d));
    });
    rx.recv_timeout(std::time::Duration::from_secs(5)).ok()
}

pub fn send(batch: Vec<Update>) {
    if let Some(tx) = TX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(Msg::Batch(batch));
    }
}

pub fn add_board(name: &str) -> bool {
    let name = name.to_owned();
    ask(move |d| {
        if d.boards.keys().any(|b| b.eq_ignore_ascii_case(&name)) {
            return false;
        }
        d.boards.insert(name, HashMap::new());
        true
    })
    .unwrap_or(false)
}

pub fn remove_board(name: &str) -> bool {
    let name = name.to_owned();
    ask(move |d| {
        let key = d.boards.keys().find(|b| b.eq_ignore_ascii_case(&name)).cloned();
        key.is_some_and(|k| {
            d.resets.remove(&k);
            d.boards.remove(&k).is_some()
        })
    })
    .unwrap_or(false)
}

pub fn remove_player(name: &str, board: Option<&str>) -> usize {
    let (name, board) = (name.to_ascii_lowercase(), board.map(str::to_ascii_lowercase));
    ask(move |d| {
        let mut n = 0;
        for (b, rows) in d.boards.iter_mut() {
            if board.as_ref().is_some_and(|x| !b.eq_ignore_ascii_case(x)) {
                continue;
            }
            let before = rows.len();
            rows.retain(|uuid, r| r.name.to_ascii_lowercase() != name && uuid.to_ascii_lowercase() != name);
            n += usize::from(rows.len() != before);
        }
        n
    })
    .unwrap_or(0)
}

pub fn reset(board: &str, kind: &str) -> bool {
    let (board, kind) = (board.to_owned(), kind.to_owned());
    ask(move |d| {
        let Some(key) = d.boards.keys().find(|b| b.eq_ignore_ascii_case(&board)).cloned() else { return false };
        let rows = d.boards.get_mut(&key).expect("board");
        match type_index(&kind) {
            Some(ti) => rows.values_mut().for_each(|r| r.base[ti] = r.total),
            None => rows.clear(),
        }
        true
    })
    .unwrap_or(false)
}

pub fn export(path: &Path) {
    let path = path.to_owned();
    edit(move |d| write(&path, d));
}

pub fn import(path: &Path) -> bool {
    let Some(v) = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { return false };
    let incoming = from_json(&v);
    edit(move |d| {
        for (b, rows) in incoming.boards {
            d.boards.entry(b).or_default().extend(rows);
        }
        d.extras.extend(incoming.extras);
    });
    true
}

pub fn stop() {
    if let Some(tx) = TX.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = tx.send(Msg::Stop);
    }
    if let Some(done) = DONE.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = done.recv_timeout(std::time::Duration::from_secs(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timed_values_and_sorting() {
        let mut d = Data::default();
        d.boards.insert("kills".into(), HashMap::new());
        let up = |uuid: &str, name: &str, v: f64| Update { uuid: uuid.into(), name: name.into(), display: String::new(), prefix: String::new(), suffix: String::new(), values: vec![("kills".into(), v)], extras: vec![] };
        apply(&mut d, vec![up("a", "Alice", 10.0), up("b", "Bob", 5.0)]);
        apply(&mut d, vec![up("b", "Bob", 25.0)]);
        let s = rebuild(&d, &[]);
        assert_eq!(s.top("kills", "alltime", 0).map(|e| e.name.as_str()), Some("Bob"));
        assert_eq!(s.position("kills", "alltime", "a"), Some(2));
        // Bob made 20 since he was first seen; Alice none.
        assert_eq!(s.top("kills", "daily", 0).map(|e| (e.name.as_str(), e.value)), Some(("Bob", 20.0)));
        assert_eq!(s.total("kills", "alltime"), 35.0);
        let rev = rebuild(&d, &["kills".into()]);
        assert_eq!(rev.top("kills", "alltime", 0).map(|e| e.name.as_str()), Some("Alice"));
        assert!(next_reset("weekly", Weekday::Sun).is_some_and(|t| t > Local::now().timestamp()));
        let back = from_json(&to_json(&d));
        assert_eq!(back.boards["kills"]["b"].total, 25.0);
    }
}
