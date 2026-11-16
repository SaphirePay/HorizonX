//! Jobs' SQLite database (`plugins/Jobs/jobs.sqlite.db`), its tables and columns as Jobs Reborn makes them so an
//! existing file carries over: users, jobNames, jobs, archive, points, limits, blocks, exploreData, worlds.
//! Read at pre-login (that event is off the server thread) and by the background thread; written in batches.

use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::Mutex;

static DB: Mutex<Option<Connection>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&Connection) -> rusqlite::Result<R>) -> Option<R> {
    let held = DB.lock().unwrap_or_else(|e| e.into_inner());
    let c = held.as_ref()?;
    match f(c) {
        Ok(r) => Some(r),
        Err(e) => {
            crate::log(&format!("database: {e}"));
            None
        }
    }
}

pub fn open(folder: &Path) -> bool {
    let path = folder.join("jobs.sqlite.db");
    let Ok(c) = Connection::open(&path) else { return false };
    let _ = c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
        CREATE TABLE IF NOT EXISTS `jobNames` (`id` INTEGER PRIMARY KEY AUTOINCREMENT, `name` varchar(36));
        CREATE TABLE IF NOT EXISTS `worlds` (`id` INTEGER PRIMARY KEY AUTOINCREMENT, `name` varchar(36));
        CREATE TABLE IF NOT EXISTS `users` (`id` INTEGER PRIMARY KEY AUTOINCREMENT, `player_uuid` varchar(36), `username` text, `seen` bigint, `donequests` int, `quests` text, `messageOptions` int);
        CREATE TABLE IF NOT EXISTS `jobs` (`id` INTEGER PRIMARY KEY AUTOINCREMENT, `userid` int, `job` text, `experience` double, `level` int, `jobid` int);
        CREATE TABLE IF NOT EXISTS `archive` (`id` INTEGER PRIMARY KEY AUTOINCREMENT, `userid` int, `job` text, `experience` int, `level` int, `left` bigint, `jobid` int);
        CREATE TABLE IF NOT EXISTS `blocks` (`id` INTEGER PRIMARY KEY AUTOINCREMENT, `world` varchar(36), `x` int, `y` int, `z` int, `recorded` bigint, `resets` bigint, `worldid` int);
        CREATE TABLE IF NOT EXISTS `limits` (`id` INTEGER PRIMARY KEY AUTOINCREMENT, `userid` int, `type` varchar(36), `collected` double, `started` bigint, `typeid` int);
        CREATE TABLE IF NOT EXISTS `log` (`id` INTEGER PRIMARY KEY AUTOINCREMENT, `userid` int, `time` bigint, `action` varchar(20), `itemname` text, `count` int, `money` double, `exp` double, `points` double);
        CREATE TABLE IF NOT EXISTS `exploreData` (`id` INTEGER PRIMARY KEY AUTOINCREMENT, `worldname` varchar(64), `chunkX` int, `chunkZ` int, `playerNames` text, `worldid` int);
        CREATE TABLE IF NOT EXISTS `points` (`id` INTEGER PRIMARY KEY AUTOINCREMENT, `userid` int, `totalpoints` double, `currentpoints` double);
        CREATE INDEX IF NOT EXISTS hx_jobs_user ON `jobs` (`userid`);
        CREATE INDEX IF NOT EXISTS hx_users_uuid ON `users` (`player_uuid`);",
    );
    *DB.lock().unwrap_or_else(|e| e.into_inner()) = Some(c);
    true
}

pub fn close() {
    *DB.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// A job's id in jobNames (made when new).
fn job_id(c: &Connection, name: &str) -> rusqlite::Result<i64> {
    if let Some(id) = c.query_row("SELECT `id` FROM `jobNames` WHERE `name` = ?1", params![name], |r| r.get(0)).optional()? {
        return Ok(id);
    }
    c.execute("INSERT INTO `jobNames` (`name`) VALUES (?1)", params![name])?;
    Ok(c.last_insert_rowid())
}

#[derive(Clone, Default)]
pub struct Loaded {
    pub user_id: i64,
    pub jobs: Vec<(String, i32, f64)>,
    pub archived: Vec<(String, i32, f64, i64)>,
    pub points: (f64, f64),
    pub limits: Vec<(String, f64, i64)>,
    pub message_options: i64,
}

/// A player's rows (made when new).
pub fn load_player(uuid: &str, name: &str) -> Option<Loaded> {
    with(|c| {
        let now = crate::now_ms();
        let found: Option<(i64, i64)> = c.query_row("SELECT `id`, IFNULL(`messageOptions`, 0) FROM `users` WHERE `player_uuid` = ?1", params![uuid], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        let (user_id, opts) = match found {
            Some(x) => {
                c.execute("UPDATE `users` SET `username` = ?1, `seen` = ?2 WHERE `id` = ?3", params![name, now, x.0])?;
                x
            }
            None => {
                c.execute("INSERT INTO `users` (`player_uuid`, `username`, `seen`, `donequests`, `quests`, `messageOptions`) VALUES (?1, ?2, ?3, 0, '', 0)", params![uuid, name, now])?;
                (c.last_insert_rowid(), 0)
            }
        };
        let mut l = Loaded { user_id, message_options: opts, ..Loaded::default() };
        let mut st = c.prepare("SELECT IFNULL(n.`name`, j.`job`), j.`level`, j.`experience` FROM `jobs` j LEFT JOIN `jobNames` n ON n.`id` = j.`jobid` WHERE j.`userid` = ?1")?;
        for row in st.query_map(params![user_id], |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get::<_, i32>(1)?, r.get::<_, f64>(2)?)))? {
            l.jobs.push(row?);
        }
        let mut st = c.prepare("SELECT IFNULL(n.`name`, a.`job`), a.`level`, a.`experience`, IFNULL(a.`left`, 0) FROM `archive` a LEFT JOIN `jobNames` n ON n.`id` = a.`jobid` WHERE a.`userid` = ?1")?;
        for row in st.query_map(params![user_id], |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get::<_, i32>(1)?, r.get::<_, f64>(2)?, r.get::<_, i64>(3)?)))? {
            l.archived.push(row?);
        }
        l.points = c.query_row("SELECT `currentpoints`, `totalpoints` FROM `points` WHERE `userid` = ?1", params![user_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?.unwrap_or((0.0, 0.0));
        let mut st = c.prepare("SELECT `type`, `collected`, `started` FROM `limits` WHERE `userid` = ?1")?;
        for row in st.query_map(params![user_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?)))? {
            l.limits.push(row?);
        }
        Ok(l)
    })
}

/// What a save writes for one player.
pub struct Row {
    pub user_id: i64,
    pub name: String,
    pub jobs: Vec<(String, i32, f64)>,
    pub archived: Vec<(String, i32, f64, i64)>,
    pub points: (f64, f64),
    pub limits: Vec<(String, f64, i64)>,
    pub message_options: i64,
}

pub fn save(rows: &[Row]) {
    if rows.is_empty() {
        return;
    }
    with(|c| {
        c.execute_batch("BEGIN")?;
        let now = crate::now_ms();
        for r in rows {
            c.execute("UPDATE `users` SET `username` = ?1, `seen` = ?2, `messageOptions` = ?3 WHERE `id` = ?4", params![r.name, now, r.message_options, r.user_id])?;
            c.execute("DELETE FROM `jobs` WHERE `userid` = ?1", params![r.user_id])?;
            for (job, level, exp) in &r.jobs {
                let id = job_id(c, job)?;
                c.execute("INSERT INTO `jobs` (`userid`, `job`, `experience`, `level`, `jobid`) VALUES (?1, ?2, ?3, ?4, ?5)", params![r.user_id, job, exp, level, id])?;
            }
            c.execute("DELETE FROM `archive` WHERE `userid` = ?1", params![r.user_id])?;
            for (job, level, exp, left) in &r.archived {
                let id = job_id(c, job)?;
                c.execute("INSERT INTO `archive` (`userid`, `job`, `experience`, `level`, `left`, `jobid`) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![r.user_id, job, *exp as i64, level, left, id])?;
            }
            let updated = c.execute("UPDATE `points` SET `currentpoints` = ?1, `totalpoints` = ?2 WHERE `userid` = ?3", params![r.points.0, r.points.1, r.user_id])?;
            if updated == 0 {
                c.execute("INSERT INTO `points` (`userid`, `totalpoints`, `currentpoints`) VALUES (?1, ?2, ?3)", params![r.user_id, r.points.1, r.points.0])?;
            }
            c.execute("DELETE FROM `limits` WHERE `userid` = ?1", params![r.user_id])?;
            for (kind, collected, started) in &r.limits {
                c.execute("INSERT INTO `limits` (`userid`, `type`, `collected`, `started`, `typeid`) VALUES (?1, ?2, ?3, ?4, 0)", params![r.user_id, kind, collected, started])?;
            }
        }
        c.execute_batch("COMMIT")
    });
}

/// Top of one job (None: all jobs by total level): (name, uuid, level, exp).
pub fn top(job: Option<&str>, limit: usize, offset: usize, hidden: &[String]) -> Vec<(String, String, i32, f64)> {
    with(|c| {
        let mut out = Vec::new();
        let hide = |n: &str| hidden.iter().any(|h| h.eq_ignore_ascii_case(n));
        match job {
            Some(j) => {
                let mut st = c.prepare("SELECT u.`username`, u.`player_uuid`, j.`level`, j.`experience` FROM `jobs` j JOIN `users` u ON u.`id` = j.`userid` LEFT JOIN `jobNames` n ON n.`id` = j.`jobid` WHERE LOWER(IFNULL(n.`name`, j.`job`)) = LOWER(?1) ORDER BY j.`level` DESC, j.`experience` DESC LIMIT ?2 OFFSET ?3")?;
                for row in st.query_map(params![j, (limit + hidden.len()) as i64, offset as i64], |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get::<_, i32>(2)?, r.get::<_, f64>(3)?)))? {
                    let r = row?;
                    if !hide(&r.0) {
                        out.push(r);
                    }
                }
            }
            None => {
                let mut st = c.prepare("SELECT u.`username`, u.`player_uuid`, SUM(j.`level`) AS lv, SUM(j.`experience`) FROM `jobs` j JOIN `users` u ON u.`id` = j.`userid` GROUP BY j.`userid` ORDER BY lv DESC LIMIT ?1 OFFSET ?2")?;
                for row in st.query_map(params![(limit + hidden.len()) as i64, offset as i64], |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get::<_, i32>(2)?, r.get::<_, f64>(3)?)))? {
                    let r = row?;
                    if !hide(&r.0) {
                        out.push(r);
                    }
                }
            }
        }
        out.truncate(limit);
        Ok(out)
    })
    .unwrap_or_default()
}

/// Workers per job (for /jobs browse's counts).
pub fn workers() -> Vec<(String, i64)> {
    with(|c| {
        let mut st = c.prepare("SELECT IFNULL(n.`name`, j.`job`), COUNT(*) FROM `jobs` j LEFT JOIN `jobNames` n ON n.`id` = j.`jobid` GROUP BY IFNULL(n.`name`, j.`job`)")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get::<_, i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
    .unwrap_or_default()
}

/// An offline player's id and jobs by name (admin commands on someone not online).
pub fn find_user(name: &str) -> Option<(i64, String)> {
    with(|c| c.query_row("SELECT `id`, `player_uuid` FROM `users` WHERE LOWER(`username`) = LOWER(?1) ORDER BY `seen` DESC LIMIT 1", params![name], |r| Ok((r.get(0)?, r.get::<_, String>(1)?))).optional()).flatten()
}

// ---- Block protection and explored chunks ----

pub fn load_blocks() -> Vec<(String, i32, i32, i32, i64, i64)> {
    with(|c| {
        let mut st = c.prepare("SELECT `world`, `x`, `y`, `z`, IFNULL(`recorded`, 0), IFNULL(`resets`, -1) FROM `blocks`")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
    .unwrap_or_default()
}

pub fn save_blocks(added: &[(String, i32, i32, i32, i64, i64)], removed: &[(String, i32, i32, i32)]) {
    if added.is_empty() && removed.is_empty() {
        return;
    }
    with(|c| {
        c.execute_batch("BEGIN")?;
        for (w, x, y, z) in removed {
            c.execute("DELETE FROM `blocks` WHERE `world` = ?1 AND `x` = ?2 AND `y` = ?3 AND `z` = ?4", params![w, x, y, z])?;
        }
        for (w, x, y, z, rec, resets) in added {
            c.execute("DELETE FROM `blocks` WHERE `world` = ?1 AND `x` = ?2 AND `y` = ?3 AND `z` = ?4", params![w, x, y, z])?;
            c.execute("INSERT INTO `blocks` (`world`, `x`, `y`, `z`, `recorded`, `resets`, `worldid`) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)", params![w, x, y, z, rec, resets])?;
        }
        c.execute_batch("COMMIT")
    });
}

pub fn load_explore() -> Vec<(String, i32, i32, String)> {
    with(|c| {
        let mut st = c.prepare("SELECT `worldname`, `chunkX`, `chunkZ`, IFNULL(`playerNames`, '') FROM `exploreData`")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
    .unwrap_or_default()
}

pub fn save_explore(rows: &[(String, i32, i32, String)]) {
    if rows.is_empty() {
        return;
    }
    with(|c| {
        c.execute_batch("BEGIN")?;
        for (w, x, z, names) in rows {
            let n = c.execute("UPDATE `exploreData` SET `playerNames` = ?1 WHERE `worldname` = ?2 AND `chunkX` = ?3 AND `chunkZ` = ?4", params![names, w, x, z])?;
            if n == 0 {
                c.execute("INSERT INTO `exploreData` (`worldname`, `chunkX`, `chunkZ`, `playerNames`, `worldid`) VALUES (?1, ?2, ?3, ?4, 0)", params![w, x, z, names])?;
            }
        }
        c.execute_batch("COMMIT")
    });
}
