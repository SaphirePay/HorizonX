//! Punishments: the active ones (the `Punishments` table) kept in memory, the history (`PunishmentHistory`) in the
//! database only. Storage is SQLite (`data/storage.db`, the Java plugin's own two tables and columns) on one
//! database thread: the server thread never waits on it except for the one load on enable.

use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PType {
    Ban,
    TempBan,
    IpBan,
    TempIpBan,
    Mute,
    TempMute,
    Warning,
    TempWarning,
    Kick,
    Note,
}

pub const ALL: [PType; 10] = [PType::Ban, PType::TempBan, PType::IpBan, PType::TempIpBan, PType::Mute, PType::TempMute, PType::Warning, PType::TempWarning, PType::Kick, PType::Note];

impl PType {
    /// The messages section (`Tempban`).
    pub fn name(self) -> &'static str {
        match self {
            PType::Ban => "Ban",
            PType::TempBan => "Tempban",
            PType::IpBan => "Ipban",
            PType::TempIpBan => "Tempipban",
            PType::Mute => "Mute",
            PType::TempMute => "Tempmute",
            PType::Warning => "Warn",
            PType::TempWarning => "Tempwarn",
            PType::Kick => "Kick",
            PType::Note => "Note",
        }
    }
    /// The database name (the Java enum's).
    pub fn db(self) -> &'static str {
        match self {
            PType::Ban => "BAN",
            PType::TempBan => "TEMP_BAN",
            PType::IpBan => "IP_BAN",
            PType::TempIpBan => "TEMP_IP_BAN",
            PType::Mute => "MUTE",
            PType::TempMute => "TEMP_MUTE",
            PType::Warning => "WARNING",
            PType::TempWarning => "TEMP_WARNING",
            PType::Kick => "KICK",
            PType::Note => "NOTE",
        }
    }
    pub fn from_db(s: &str) -> Option<PType> {
        ALL.iter().copied().find(|t| t.db().eq_ignore_ascii_case(s))
    }
    pub fn perms(self) -> &'static str {
        match self {
            PType::Ban => "ab.ban.perma",
            PType::TempBan => "ab.ban.temp",
            PType::IpBan => "ab.ipban.perma",
            PType::TempIpBan => "ab.ipban.temp",
            PType::Mute => "ab.mute.perma",
            PType::TempMute => "ab.mute.temp",
            PType::Warning => "ab.warn.perma",
            PType::TempWarning => "ab.warn.temp",
            PType::Kick => "ab.kick.use",
            PType::Note => "ab.note.use",
        }
    }
    pub fn is_temp(self) -> bool {
        matches!(self, PType::TempBan | PType::TempIpBan | PType::TempMute | PType::TempWarning)
    }
    pub fn basic(self) -> PType {
        match self {
            PType::TempBan | PType::IpBan | PType::TempIpBan => PType::Ban,
            PType::TempMute => PType::Mute,
            PType::TempWarning => PType::Warning,
            t => t,
        }
    }
    pub fn permanent(self) -> PType {
        match self {
            PType::IpBan | PType::TempIpBan => PType::IpBan,
            t => t.basic(),
        }
    }
    pub fn ip(self) -> bool {
        matches!(self, PType::IpBan | PType::TempIpBan)
    }
}

#[derive(Clone, Debug)]
pub struct Punishment {
    pub id: i64,
    pub name: String,
    /// The player's uuid without dashes, or an IP for IP bans.
    pub uuid: String,
    pub reason: Option<String>,
    pub operator: String,
    pub kind: PType,
    pub start: i64,
    pub end: i64,
    pub calculation: String,
}

impl Punishment {
    pub fn expired(&self, now: i64) -> bool {
        self.kind.is_temp() && self.end <= now
    }
}

pub struct Memory {
    pub active: Vec<Punishment>,
    pub next_id: i64,
    /// lower-case name -> last IP (this run, as the Java plugin keeps them).
    pub ips: HashMap<String, String>,
    /// (uuid, calculation lower-case) -> how many times in the history.
    pub levels: HashMap<(String, String), i64>,
    /// The database has been read.
    pub loaded: bool,
}

pub static MEM: std::sync::LazyLock<Mutex<Memory>> = std::sync::LazyLock::new(|| Mutex::new(Memory { active: Vec::new(), next_id: 1, ips: HashMap::new(), levels: HashMap::new(), loaded: false }));

pub fn mem() -> std::sync::MutexGuard<'static, Memory> {
    MEM.lock().unwrap_or_else(|e| e.into_inner())
}

type HistoryReply = Box<dyn FnOnce(Vec<Punishment>) + Send>;

pub enum Job {
    Insert(Punishment, bool),
    Delete(i64),
    Reason(i64, String),
    History(Vec<String>, HistoryReply),
    Stop,
}

static TX: Mutex<Option<(Sender<Job>, std::thread::JoinHandle<()>)>> = Mutex::new(None);

fn row(r: &rusqlite::Row) -> rusqlite::Result<Option<Punishment>> {
    let kind: String = r.get("punishmentType")?;
    let Some(kind) = PType::from_db(&kind) else { return Ok(None) };
    Ok(Some(Punishment {
        id: r.get("id")?,
        name: r.get::<_, Option<String>>("name")?.unwrap_or_default(),
        uuid: r.get::<_, Option<String>>("uuid")?.unwrap_or_default(),
        reason: r.get::<_, Option<String>>("reason")?,
        operator: r.get::<_, Option<String>>("operator")?.unwrap_or_default(),
        kind,
        start: r.get::<_, Option<i64>>("start")?.unwrap_or(0),
        end: r.get::<_, Option<i64>>("end")?.unwrap_or(-1),
        calculation: r.get::<_, Option<String>>("calculation")?.unwrap_or_default(),
    }))
}

const CREATE: &str = "CREATE TABLE IF NOT EXISTS `Punishments` (`id` INTEGER PRIMARY KEY, `name` VARCHAR(16), `uuid` VARCHAR(35), `reason` VARCHAR(100), `operator` VARCHAR(16), `punishmentType` VARCHAR(16), `start` BIGINT, `end` BIGINT, `calculation` VARCHAR(50));
CREATE TABLE IF NOT EXISTS `PunishmentHistory` (`id` INTEGER PRIMARY KEY AUTOINCREMENT, `name` VARCHAR(16), `uuid` VARCHAR(35), `reason` VARCHAR(100), `operator` VARCHAR(16), `punishmentType` VARCHAR(16), `start` BIGINT, `end` BIGINT, `calculation` VARCHAR(50));
CREATE INDEX IF NOT EXISTS `history_uuid` ON `PunishmentHistory` (`uuid`);";

/// The database opened, the active punishments read into memory (waited for: once, on enable), then the thread
/// takes writes and history reads.
pub fn start(file: PathBuf, now: i64) {
    let (tx, rx) = channel::<Job>();
    let (ready_tx, ready_rx) = channel::<()>();
    let handle = std::thread::Builder::new()
        .name("AdvancedBan database".into())
        .spawn(move || {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let conn = match Connection::open(&file) {
                Ok(c) => c,
                Err(e) => {
                    crate::log(&format!("Could not open the database {}: {e}", file.display()));
                    let _ = ready_tx.send(());
                    return;
                }
            };
            let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;");
            let _ = conn.execute_batch(CREATE);
            // Expired ones go first, as DELETE_OLD_PUNISHMENTS does on start.
            let _ = conn.execute("DELETE FROM `Punishments` WHERE `end` <= ?1 AND `end` != -1", params![now]);
            let mut active = Vec::new();
            if let Ok(mut st) = conn.prepare("SELECT * FROM `Punishments`") {
                if let Ok(rows) = st.query_map([], row) {
                    active.extend(rows.flatten().flatten());
                }
            }
            let mut levels = HashMap::new();
            if let Ok(mut st) = conn.prepare("SELECT `uuid`, LOWER(`calculation`), COUNT(*) FROM `PunishmentHistory` WHERE `calculation` IS NOT NULL AND `calculation` != '' GROUP BY `uuid`, LOWER(`calculation`)") {
                if let Ok(rows) = st.query_map([], |r| Ok(((r.get::<_, String>(0)?, r.get::<_, String>(1)?), r.get::<_, i64>(2)?))) {
                    levels.extend(rows.flatten());
                }
            }
            let max_id: i64 = conn.query_row("SELECT COALESCE(MAX(`id`), 0) FROM `Punishments`", [], |r| r.get(0)).unwrap_or(0);
            {
                let mut m = mem();
                m.next_id = max_id + 1;
                m.active = active;
                m.levels = levels;
                m.loaded = true;
            }
            let _ = ready_tx.send(());
            while let Ok(job) = rx.recv() {
                let mut batch = vec![job];
                batch.extend(rx.try_iter());
                let _ = conn.execute_batch("BEGIN");
                let mut stop = false;
                let mut replies: Vec<(HistoryReply, Vec<Punishment>)> = Vec::new();
                for job in batch {
                    match job {
                        Job::Insert(p, active) => {
                            let _ = conn.execute(
                                "INSERT INTO `PunishmentHistory` (`name`, `uuid`, `reason`, `operator`, `punishmentType`, `start`, `end`, `calculation`) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                                params![p.name, p.uuid, p.reason, p.operator, p.kind.db(), p.start, p.end, p.calculation],
                            );
                            if active {
                                let _ = conn.execute(
                                    "INSERT OR REPLACE INTO `Punishments` (`id`, `name`, `uuid`, `reason`, `operator`, `punishmentType`, `start`, `end`, `calculation`) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                                    params![p.id, p.name, p.uuid, p.reason, p.operator, p.kind.db(), p.start, p.end, p.calculation],
                                );
                            }
                        }
                        Job::Delete(id) => {
                            let _ = conn.execute("DELETE FROM `Punishments` WHERE `id` = ?1", params![id]);
                        }
                        Job::Reason(id, reason) => {
                            let _ = conn.execute("UPDATE `Punishments` SET `reason` = ?1 WHERE `id` = ?2", params![reason, id]);
                        }
                        Job::History(targets, reply) => {
                            let mut out = Vec::new();
                            let sql = if targets.len() > 1 { "SELECT * FROM `PunishmentHistory` WHERE `uuid` = ?1 OR `uuid` = ?2 ORDER BY `start` DESC" } else { "SELECT * FROM `PunishmentHistory` WHERE `uuid` = ?1 ORDER BY `start` DESC" };
                            if let Ok(mut st) = conn.prepare(sql) {
                                let a = targets.first().cloned().unwrap_or_default();
                                let b = targets.get(1).cloned().unwrap_or_default();
                                let rows = if targets.len() > 1 { st.query_map(params![a, b], row).map(|r| r.flatten().flatten().collect::<Vec<_>>()) } else { st.query_map(params![a], row).map(|r| r.flatten().flatten().collect::<Vec<_>>()) };
                                if let Ok(rows) = rows {
                                    out = rows;
                                }
                            }
                            replies.push((reply, out));
                        }
                        Job::Stop => stop = true,
                    }
                }
                let _ = conn.execute_batch("COMMIT");
                for (reply, rows) in replies {
                    reply(rows);
                }
                if stop {
                    return;
                }
            }
        })
        .ok();
    if let Some(h) = handle {
        *TX.lock().unwrap_or_else(|e| e.into_inner()) = Some((tx, h));
        let _ = ready_rx.recv_timeout(std::time::Duration::from_secs(30));
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

// ---- Lookups (memory) ----

/// Expired temp punishments dropped (and deleted in the database).
pub fn prune(now: i64) {
    let mut m = mem();
    let mut gone = Vec::new();
    m.active.retain(|p| {
        let keep = !p.expired(now);
        if !keep {
            gone.push(p.id);
        }
        keep
    });
    drop(m);
    for id in gone {
        send(Job::Delete(id));
    }
}

/// The active punishment of `basic` kind on any of `targets` (uuid / IP).
pub fn find(targets: &[&str], basic: PType, now: i64) -> Option<Punishment> {
    prune(now);
    mem().active.iter().find(|p| p.kind.basic() == basic && targets.iter().any(|t| p.uuid.eq_ignore_ascii_case(t))).cloned()
}

pub fn of(uuid: &str, basic: PType, now: i64) -> Vec<Punishment> {
    prune(now);
    mem().active.iter().filter(|p| p.kind.basic() == basic && p.uuid.eq_ignore_ascii_case(uuid)).cloned().collect()
}

pub fn by_id(id: i64) -> Option<Punishment> {
    mem().active.iter().find(|p| p.id == id).cloned()
}

pub fn remove(id: i64) -> Option<Punishment> {
    let mut m = mem();
    let at = m.active.iter().position(|p| p.id == id)?;
    let p = m.active.remove(at);
    drop(m);
    send(Job::Delete(id));
    Some(p)
}

/// Stored: in memory (and the database on its thread). The id is given here.
pub fn add(mut p: Punishment) -> Punishment {
    let mut m = mem();
    let active = p.kind != PType::Kick;
    if active {
        p.id = m.next_id;
        m.next_id += 1;
        m.active.push(p.clone());
    }
    if !p.calculation.is_empty() {
        *m.levels.entry((p.uuid.clone(), p.calculation.to_ascii_lowercase())).or_insert(0) += 1;
    }
    drop(m);
    send(Job::Insert(p.clone(), active));
    p
}

pub fn set_reason(id: i64, reason: &str) -> bool {
    let mut m = mem();
    let Some(p) = m.active.iter_mut().find(|p| p.id == id) else { return false };
    p.reason = Some(reason.to_owned());
    drop(m);
    send(Job::Reason(id, reason.to_owned()));
    true
}

pub fn level(uuid: &str, layout: &str) -> i64 {
    mem().levels.get(&(uuid.to_owned(), layout.to_ascii_lowercase())).copied().unwrap_or(0)
}
