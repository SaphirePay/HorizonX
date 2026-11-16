//! `database.db`, in Plan's own tables (plan_servers, plan_users, plan_user_info, plan_sessions, plan_world_times,
//! plan_worlds, plan_kills, plan_tps, plan_ping, plan_nicknames): an existing Plan SQLite database keeps working and
//! its history shows on the pages. Writes go to one database thread (a burst is one transaction); reads (pages,
//! commands) open their own connection on their own thread (WAL: they never wait on the writes).

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, RwLock};

pub static FILE: RwLock<Option<PathBuf>> = RwLock::new(None);
pub static SERVER_UUID: RwLock<String> = RwLock::new(String::new());

pub struct Session {
    pub uuid: String,
    pub name: String,
    pub start: i64,
    pub end: i64,
    pub deaths: i64,
    pub mob_kills: i64,
    pub afk: i64,
    /// world -> [survival, creative, adventure, spectator] ms
    pub worlds: HashMap<String, [i64; 4]>,
    /// (victim uuid, weapon, date)
    pub kills: Vec<(String, String, i64)>,
    /// (min, max, avg) over the session, when measured.
    pub ping: Option<(i32, i32, f64)>,
}

pub struct Tps {
    pub date: i64,
    pub tps: f64,
    pub players: i64,
    pub entities: i64,
    pub chunks: i64,
    pub mspt: f64,
}

pub enum Job {
    /// uuid, name, registered (ms), op
    Register(String, String, i64, bool),
    Session(Box<Session>),
    Tps(Tps),
    Kicked(String),
    Stop,
}

static TX: Mutex<Option<(Sender<Job>, std::thread::JoinHandle<()>)>> = Mutex::new(None);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS plan_servers (id integer PRIMARY KEY, uuid varchar(36) NOT NULL UNIQUE, name varchar(100), web_address varchar(100), is_installed boolean NOT NULL DEFAULT 1, is_proxy boolean NOT NULL DEFAULT 0, max_players integer NOT NULL DEFAULT -1, plan_version varchar(18) NOT NULL DEFAULT 'Old');
CREATE TABLE IF NOT EXISTS plan_users (id integer PRIMARY KEY, uuid varchar(36) NOT NULL UNIQUE, registered bigint NOT NULL, name varchar(36) NOT NULL, times_kicked integer NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS plan_user_info (id integer PRIMARY KEY, user_id integer NOT NULL, server_id integer NOT NULL, join_address varchar(191), registered bigint NOT NULL, opped boolean NOT NULL DEFAULT 0, banned boolean NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS plan_worlds (id integer PRIMARY KEY, world_name varchar(100) NOT NULL, server_uuid varchar(36) NOT NULL);
CREATE TABLE IF NOT EXISTS plan_sessions (id integer PRIMARY KEY, user_id integer NOT NULL, server_id integer NOT NULL, session_start bigint NOT NULL, session_end bigint NOT NULL, mob_kills integer NOT NULL, deaths integer NOT NULL, afk_time bigint NOT NULL, join_address_id integer NOT NULL DEFAULT 1);
CREATE TABLE IF NOT EXISTS plan_world_times (id integer PRIMARY KEY, user_id integer NOT NULL, world_id integer NOT NULL, server_id integer NOT NULL, session_id integer NOT NULL, survival_time bigint NOT NULL DEFAULT 0, creative_time bigint NOT NULL DEFAULT 0, adventure_time bigint NOT NULL DEFAULT 0, spectator_time bigint NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS plan_kills (id integer PRIMARY KEY, killer_uuid varchar(36) NOT NULL, victim_uuid varchar(36) NOT NULL, server_uuid varchar(36) NOT NULL, weapon varchar(30) NOT NULL, date bigint NOT NULL, session_id integer NOT NULL);
CREATE TABLE IF NOT EXISTS plan_tps (id integer PRIMARY KEY, server_id integer NOT NULL, date bigint NOT NULL, tps double, players_online integer, cpu_usage double, ram_usage bigint, entities integer, chunks_loaded integer, free_disk_space bigint, mspt_average double, mspt_95th_percentile double, mspt_jitter_average double, mspt_jitter_max double);
CREATE TABLE IF NOT EXISTS plan_ping (id integer PRIMARY KEY, user_id integer NOT NULL, server_id integer NOT NULL, date bigint NOT NULL, max_ping integer NOT NULL, min_ping integer NOT NULL, avg_ping double NOT NULL);
CREATE TABLE IF NOT EXISTS plan_nicknames (id integer PRIMARY KEY, uuid varchar(36) NOT NULL, nickname varchar(75) NOT NULL, server_uuid varchar(36) NOT NULL, last_used bigint NOT NULL, UNIQUE (uuid, server_uuid, nickname));
CREATE INDEX IF NOT EXISTS hz_sessions_user ON plan_sessions (user_id);
CREATE INDEX IF NOT EXISTS hz_sessions_start ON plan_sessions (session_start);
CREATE INDEX IF NOT EXISTS hz_tps_date ON plan_tps (server_id, date);
";

fn open(file: &PathBuf) -> rusqlite::Result<Connection> {
    let c = Connection::open(file)?;
    c.busy_timeout(std::time::Duration::from_secs(10))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    Ok(c)
}

fn server_id(c: &Connection, server: &str) -> i64 {
    c.query_row("SELECT id FROM plan_servers WHERE uuid=?1", params![server], |r| r.get(0)).unwrap_or(0)
}

fn world_id(c: &Connection, world: &str, server: &str) -> i64 {
    if let Ok(Some(id)) = c.query_row("SELECT id FROM plan_worlds WHERE world_name=?1 AND server_uuid=?2 LIMIT 1", params![world, server], |r| r.get(0)).optional() {
        return id;
    }
    let _ = c.execute("INSERT INTO plan_worlds (world_name, server_uuid) VALUES (?1, ?2)", params![world, server]);
    c.last_insert_rowid()
}

fn user_id(c: &Connection, uuid: &str) -> Option<i64> {
    c.query_row("SELECT id FROM plan_users WHERE uuid=?1", params![uuid], |r| r.get(0)).optional().ok().flatten()
}

fn write(c: &Connection, job: Job, server: &str, sid: i64) {
    match job {
        Job::Register(uuid, name, registered, op) => {
            let _ = c.execute("INSERT OR IGNORE INTO plan_users (uuid, registered, name, times_kicked) VALUES (?1, ?2, ?3, 0)", params![uuid, registered, name]);
            let _ = c.execute("UPDATE plan_users SET name=?2 WHERE uuid=?1", params![uuid, name]);
            if let Some(uid) = user_id(c, &uuid) {
                let has: bool = c.query_row("SELECT COUNT(*) FROM plan_user_info WHERE user_id=?1 AND server_id=?2", params![uid, sid], |r| r.get::<_, i64>(0)).unwrap_or(0) > 0;
                if !has {
                    let _ = c.execute("INSERT INTO plan_user_info (user_id, server_id, join_address, registered, opped, banned) VALUES (?1, ?2, NULL, ?3, ?4, 0)", params![uid, sid, registered, op]);
                }
            }
            let _ = c.execute("INSERT INTO plan_nicknames (uuid, server_uuid, nickname, last_used) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(uuid, server_uuid, nickname) DO UPDATE SET last_used = MAX(excluded.last_used, plan_nicknames.last_used)", params![uuid, server, name, registered]);
        }
        Job::Session(s) => {
            let Some(uid) = user_id(c, &s.uuid) else { return };
            if c.execute("INSERT INTO plan_sessions (user_id, server_id, session_start, session_end, mob_kills, deaths, afk_time, join_address_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)", params![uid, sid, s.start, s.end, s.mob_kills, s.deaths, s.afk]).is_err() {
                return;
            }
            let session = c.last_insert_rowid();
            for (world, t) in &s.worlds {
                let wid = world_id(c, world, server);
                let _ = c.execute("INSERT INTO plan_world_times (user_id, world_id, server_id, session_id, survival_time, creative_time, adventure_time, spectator_time) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)", params![uid, wid, sid, session, t[0], t[1], t[2], t[3]]);
            }
            for (victim, weapon, date) in &s.kills {
                let _ = c.execute("INSERT INTO plan_kills (killer_uuid, victim_uuid, server_uuid, weapon, date, session_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![s.uuid, victim, server, weapon, date, session]);
            }
            if let Some((min, max, avg)) = s.ping {
                let _ = c.execute("INSERT INTO plan_ping (user_id, server_id, date, max_ping, min_ping, avg_ping) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![uid, sid, s.end, max, min, avg]);
            }
        }
        Job::Tps(t) => {
            let full = c.execute(
                "INSERT INTO plan_tps (server_id, date, tps, players_online, cpu_usage, ram_usage, entities, chunks_loaded, free_disk_space, mspt_average, mspt_95th_percentile, mspt_jitter_average, mspt_jitter_max) VALUES (?1, ?2, ?3, ?4, -1, -1, ?5, ?6, -1, ?7, -1, -1, -1)",
                params![sid, t.date, t.tps, t.players, t.entities, t.chunks, t.mspt],
            );
            if full.is_err() {
                let _ = c.execute("INSERT INTO plan_tps (server_id, date, tps, players_online, cpu_usage, ram_usage, entities, chunks_loaded, free_disk_space) VALUES (?1, ?2, ?3, ?4, -1, -1, ?5, ?6, -1)", params![sid, t.date, t.tps, t.players, t.entities, t.chunks]);
            }
        }
        Job::Kicked(uuid) => {
            let _ = c.execute("UPDATE plan_users SET times_kicked=times_kicked+1 WHERE uuid=?1", params![uuid]);
        }
        Job::Stop => {}
    }
}

pub fn start(file: PathBuf, server_uuid: String, server_name: String, address: String) {
    *FILE.write().unwrap_or_else(|e| e.into_inner()) = Some(file.clone());
    *SERVER_UUID.write().unwrap_or_else(|e| e.into_inner()) = server_uuid.clone();
    let (tx, rx) = channel::<Job>();
    let handle = std::thread::Builder::new()
        .name("Plan database".into())
        .spawn(move || {
            let c = match open(&file) {
                Ok(c) => c,
                Err(e) => return crate::log(&format!("Could not open {}: {e}", file.display())),
            };
            if let Err(e) = c.execute_batch(SCHEMA) {
                crate::log(&format!("Database tables: {e}"));
            }
            let _ = c.execute("INSERT OR IGNORE INTO plan_servers (uuid, name, web_address, is_installed, is_proxy, plan_version) VALUES (?1, ?2, ?3, 1, 0, '5.8 build 3638')", params![server_uuid, server_name, address]);
            let _ = c.execute("UPDATE plan_servers SET name=?2, web_address=?3, is_installed=1 WHERE uuid=?1", params![server_uuid, server_name, address]);
            let sid = server_id(&c, &server_uuid);
            while let Ok(first) = rx.recv() {
                let mut batch = vec![first];
                batch.extend(rx.try_iter());
                let stop = batch.iter().any(|j| matches!(j, Job::Stop));
                let _ = c.execute_batch("BEGIN");
                for job in batch {
                    write(&c, job, &server_uuid, sid);
                }
                let _ = c.execute_batch("COMMIT");
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

// ---- Reads (any thread but the server's) ----

thread_local! {
    static READER: RefCell<Option<Connection>> = const { RefCell::new(None) };
}

/// `f` with this thread's read connection.
pub fn read<R>(f: impl FnOnce(&Connection) -> R) -> Option<R> {
    READER.with(|r| {
        let mut r = r.borrow_mut();
        if r.is_none() {
            let file = FILE.read().unwrap_or_else(|e| e.into_inner()).clone()?;
            *r = open(&file).ok();
        }
        r.as_ref().map(f)
    })
}

fn server() -> String {
    SERVER_UUID.read().unwrap_or_else(|e| e.into_inner()).clone()
}

const DAY: i64 = 86_400_000;

fn count(c: &Connection, sql: &str, p: impl rusqlite::Params) -> i64 {
    c.query_row(sql, p, |r| r.get::<_, Option<i64>>(0)).ok().flatten().unwrap_or(0)
}

/// The server page's numbers.
pub fn overview(now: i64) -> Value {
    read(|c| {
        let sid = server_id(c, &server());
        let players = count(c, "SELECT COUNT(*) FROM plan_user_info WHERE server_id=?1", params![sid]);
        let new = |since: i64| count(c, "SELECT COUNT(*) FROM plan_user_info WHERE server_id=?1 AND registered>=?2", params![sid, since]);
        let unique = |since: i64| count(c, "SELECT COUNT(DISTINCT user_id) FROM plan_sessions WHERE server_id=?1 AND session_end>=?2", params![sid, since]);
        let sessions = count(c, "SELECT COUNT(*) FROM plan_sessions WHERE server_id=?1", params![sid]);
        let playtime = count(c, "SELECT SUM(session_end-session_start) FROM plan_sessions WHERE server_id=?1", params![sid]);
        let afk = count(c, "SELECT SUM(afk_time) FROM plan_sessions WHERE server_id=?1", params![sid]);
        let kills = count(c, "SELECT COUNT(*) FROM plan_kills WHERE server_uuid=?1", params![server()]);
        let mob_kills = count(c, "SELECT SUM(mob_kills) FROM plan_sessions WHERE server_id=?1", params![sid]);
        let deaths = count(c, "SELECT SUM(deaths) FROM plan_sessions WHERE server_id=?1", params![sid]);
        let peak_all: (i64, i64) = c.query_row("SELECT players_online, date FROM plan_tps WHERE server_id=?1 ORDER BY players_online DESC, date DESC LIMIT 1", params![sid], |r| Ok((r.get(0)?, r.get(1)?))).unwrap_or((0, 0));
        let peak_2d: (i64, i64) = c.query_row("SELECT players_online, date FROM plan_tps WHERE server_id=?1 AND date>=?2 ORDER BY players_online DESC, date DESC LIMIT 1", params![sid, now - 2 * DAY], |r| Ok((r.get(0)?, r.get(1)?))).unwrap_or((0, 0));
        let tps_week: f64 = c.query_row("SELECT AVG(tps) FROM plan_tps WHERE server_id=?1 AND date>=?2", params![sid, now - 7 * DAY], |r| r.get::<_, Option<f64>>(0)).ok().flatten().unwrap_or(0.0);
        let low_tps = count(c, "SELECT COUNT(*) FROM plan_tps WHERE server_id=?1 AND date>=?2 AND tps<10", params![sid, now - 7 * DAY]);
        json!({
            "players": players, "new_day": new(now - DAY), "new_week": new(now - 7 * DAY), "new_month": new(now - 30 * DAY),
            "unique_day": unique(now - DAY), "unique_week": unique(now - 7 * DAY), "unique_month": unique(now - 30 * DAY),
            "sessions": sessions, "playtime": playtime, "afk": afk, "avg_session": if sessions > 0 { playtime / sessions } else { 0 },
            "player_kills": kills, "mob_kills": mob_kills, "deaths": deaths,
            "peak_all": peak_all.0, "peak_all_date": peak_all.1, "peak_2d": peak_2d.0, "peak_2d_date": peak_2d.1,
            "tps_week": tps_week, "low_tps_minutes_week": low_tps,
        })
    })
    .unwrap_or(Value::Null)
}

/// Players online and TPS over the last `days` days, one point a sample.
pub fn online(now: i64, days: i64) -> Value {
    read(|c| {
        let sid = server_id(c, &server());
        let mut st = match c.prepare_cached("SELECT date, players_online, tps, entities, chunks_loaded, mspt_average FROM plan_tps WHERE server_id=?1 AND date>=?2 ORDER BY date") {
            Ok(st) => st,
            Err(_) => return Value::Array(Vec::new()),
        };
        let rows = st.query_map(params![sid, now - days * DAY], |r| Ok(json!([r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, Option<f64>>(2)?, r.get::<_, Option<i64>>(3)?, r.get::<_, Option<i64>>(4)?, r.get::<_, Option<f64>>(5).unwrap_or(None)])));
        Value::Array(rows.map(|r| r.flatten().collect()).unwrap_or_default())
    })
    .unwrap_or(Value::Null)
}

/// For players who joined in each of the last 8 weeks: how many came back a day or more after their first day,
/// and a week or more after.
pub fn retention(now: i64) -> Value {
    read(|c| {
        let sid = server_id(c, &server());
        let mut out = Vec::new();
        for w in (0..8).rev() {
            let (from, to) = (now - (w + 1) * 7 * DAY, now - w * 7 * DAY);
            let joined = count(c, "SELECT COUNT(*) FROM plan_user_info WHERE server_id=?1 AND registered>=?2 AND registered<?3", params![sid, from, to]);
            let back_day = count(c, "SELECT COUNT(*) FROM plan_user_info u WHERE u.server_id=?1 AND u.registered>=?2 AND u.registered<?3 AND EXISTS (SELECT 1 FROM plan_sessions s WHERE s.user_id=u.user_id AND s.server_id=u.server_id AND s.session_start>=u.registered+86400000)", params![sid, from, to]);
            let back_week = count(c, "SELECT COUNT(*) FROM plan_user_info u WHERE u.server_id=?1 AND u.registered>=?2 AND u.registered<?3 AND EXISTS (SELECT 1 FROM plan_sessions s WHERE s.user_id=u.user_id AND s.server_id=u.server_id AND s.session_start>=u.registered+604800000)", params![sid, from, to]);
            out.push(json!({ "from": from, "joined": joined, "returned_day": back_day, "returned_week": back_week }));
        }
        Value::Array(out)
    })
    .unwrap_or(Value::Null)
}

/// Sessions started per hour of the day and the average players online per hour (last 30 days, UTC + offset).
pub fn peak_hours(now: i64, offset_ms: i64) -> Value {
    read(|c| {
        let sid = server_id(c, &server());
        let mut starts = [0i64; 24];
        if let Ok(mut st) = c.prepare("SELECT session_start FROM plan_sessions WHERE server_id=?1 AND session_start>=?2") {
            for t in st.query_map(params![sid, now - 30 * DAY], |r| r.get::<_, i64>(0)).map(|r| r.flatten().collect::<Vec<_>>()).unwrap_or_default() {
                starts[((t + offset_ms).rem_euclid(DAY) / 3_600_000) as usize] += 1;
            }
        }
        let mut sum = [0f64; 24];
        let mut n = [0f64; 24];
        if let Ok(mut st) = c.prepare("SELECT date, players_online FROM plan_tps WHERE server_id=?1 AND date>=?2") {
            for (t, p) in st.query_map(params![sid, now - 30 * DAY], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0)))).map(|r| r.flatten().collect::<Vec<_>>()).unwrap_or_default() {
                let h = ((t + offset_ms).rem_euclid(DAY) / 3_600_000) as usize;
                sum[h] += p as f64;
                n[h] += 1.0;
            }
        }
        let avg: Vec<f64> = (0..24).map(|h| if n[h] > 0.0 { sum[h] / n[h] } else { 0.0 }).collect();
        json!({ "session_starts": starts, "average_online": avg })
    })
    .unwrap_or(Value::Null)
}

/// Players with their totals (search by part of the name; top by playtime first).
pub fn players(search: &str, limit: i64) -> Value {
    read(|c| {
        let sid = server_id(c, &server());
        let pattern = format!("%{}%", search.replace('%', "").replace('_', "\\_"));
        let sql = "SELECT u.uuid, u.name, i.registered, COALESCE(SUM(s.session_end-s.session_start),0), COALESCE(SUM(s.afk_time),0), COUNT(s.id), COALESCE(MAX(s.session_end),0)
                   FROM plan_user_info i JOIN plan_users u ON u.id=i.user_id LEFT JOIN plan_sessions s ON s.user_id=u.id AND s.server_id=i.server_id
                   WHERE i.server_id=?1 AND u.name LIKE ?2 ESCAPE '\\' GROUP BY u.id ORDER BY 4 DESC LIMIT ?3";
        let mut st = match c.prepare_cached(sql) {
            Ok(st) => st,
            Err(_) => return Value::Array(Vec::new()),
        };
        let rows = st.query_map(params![sid, pattern, limit], |r| {
            Ok(json!({ "uuid": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?, "registered": r.get::<_, i64>(2)?, "playtime": r.get::<_, i64>(3)?, "afk": r.get::<_, i64>(4)?, "sessions": r.get::<_, i64>(5)?, "last_seen": r.get::<_, i64>(6)? }))
        });
        Value::Array(rows.map(|r| r.flatten().collect()).unwrap_or_default())
    })
    .unwrap_or(Value::Null)
}

/// One player's page: totals, sessions, world times, kills.
pub fn player(name_or_uuid: &str) -> Value {
    read(|c| {
        let sid = server_id(c, &server());
        let row: Option<(i64, String, String, i64, i64)> = c
            .query_row("SELECT id, uuid, name, registered, times_kicked FROM plan_users WHERE uuid=?1 OR name=?1 COLLATE NOCASE ORDER BY registered DESC LIMIT 1", params![name_or_uuid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .optional()
            .ok()
            .flatten();
        let Some((uid, uuid, name, registered, kicked)) = row else { return Value::Null };
        let total = |sql: &str| count(c, sql, params![uid, sid]);
        let playtime = total("SELECT SUM(session_end-session_start) FROM plan_sessions WHERE user_id=?1 AND server_id=?2");
        let afk = total("SELECT SUM(afk_time) FROM plan_sessions WHERE user_id=?1 AND server_id=?2");
        let sessions_n = total("SELECT COUNT(*) FROM plan_sessions WHERE user_id=?1 AND server_id=?2");
        let deaths = total("SELECT SUM(deaths) FROM plan_sessions WHERE user_id=?1 AND server_id=?2");
        let mob_kills = total("SELECT SUM(mob_kills) FROM plan_sessions WHERE user_id=?1 AND server_id=?2");
        let player_kills = count(c, "SELECT COUNT(*) FROM plan_kills WHERE killer_uuid=?1", params![uuid]);
        let player_deaths = count(c, "SELECT COUNT(*) FROM plan_kills WHERE victim_uuid=?1", params![uuid]);
        let last_seen = total("SELECT MAX(session_end) FROM plan_sessions WHERE user_id=?1 AND server_id=?2");
        let mut sessions = Vec::new();
        if let Ok(mut st) = c.prepare("SELECT session_start, session_end, afk_time, deaths, mob_kills FROM plan_sessions WHERE user_id=?1 AND server_id=?2 ORDER BY session_start DESC LIMIT 50") {
            if let Ok(rows) = st.query_map(params![uid, sid], |r| Ok(json!({ "start": r.get::<_, i64>(0)?, "end": r.get::<_, i64>(1)?, "afk": r.get::<_, i64>(2)?, "deaths": r.get::<_, i64>(3)?, "mob_kills": r.get::<_, i64>(4)? }))) {
                sessions = rows.flatten().collect();
            }
        }
        let mut worlds = Vec::new();
        if let Ok(mut st) = c.prepare("SELECT w.world_name, SUM(t.survival_time), SUM(t.creative_time), SUM(t.adventure_time), SUM(t.spectator_time) FROM plan_world_times t JOIN plan_worlds w ON w.id=t.world_id WHERE t.user_id=?1 AND t.server_id=?2 GROUP BY w.world_name ORDER BY 2+3+4+5 DESC") {
            if let Ok(rows) = st.query_map(params![uid, sid], |r| Ok(json!({ "world": r.get::<_, String>(0)?, "survival": r.get::<_, i64>(1)?, "creative": r.get::<_, i64>(2)?, "adventure": r.get::<_, i64>(3)?, "spectator": r.get::<_, i64>(4)? }))) {
                worlds = rows.flatten().collect();
            }
        }
        let mut kills = Vec::new();
        if let Ok(mut st) = c.prepare("SELECT k.date, COALESCE(v.name, k.victim_uuid), k.weapon FROM plan_kills k LEFT JOIN plan_users v ON v.uuid=k.victim_uuid WHERE k.killer_uuid=?1 ORDER BY k.date DESC LIMIT 30") {
            if let Ok(rows) = st.query_map(params![uuid], |r| Ok(json!({ "date": r.get::<_, i64>(0)?, "victim": r.get::<_, String>(1)?, "weapon": r.get::<_, String>(2)? }))) {
                kills = rows.flatten().collect();
            }
        }
        let ping: Value = c.query_row("SELECT AVG(avg_ping), MIN(min_ping), MAX(max_ping) FROM plan_ping WHERE user_id=?1 AND server_id=?2", params![uid, sid], |r| Ok(json!({ "avg": r.get::<_, Option<f64>>(0)?, "min": r.get::<_, Option<i64>>(1)?, "max": r.get::<_, Option<i64>>(2)? }))).unwrap_or(Value::Null);
        let nicknames: Vec<String> = c.prepare("SELECT nickname FROM plan_nicknames WHERE uuid=?1 ORDER BY last_used DESC LIMIT 10").and_then(|mut st| st.query_map(params![uuid], |r| r.get::<_, String>(0)).map(|r| r.flatten().collect())).unwrap_or_default();
        json!({
            "uuid": uuid, "name": name, "registered": registered, "times_kicked": kicked, "playtime": playtime, "afk": afk, "sessions_count": sessions_n,
            "deaths": deaths, "mob_kills": mob_kills, "player_kills": player_kills, "player_deaths": player_deaths, "last_seen": last_seen,
            "sessions": sessions, "worlds": worlds, "kills": kills, "ping": ping, "nicknames": nicknames,
        })
    })
    .unwrap_or(Value::Null)
}

/// A player's totals for the placeholders (playtime, afk, sessions, kills, deaths, mob kills, registered).
pub fn totals(uuid: &str) -> Option<[i64; 7]> {
    read(|c| {
        let sid = server_id(c, &server());
        let uid = user_id(c, uuid)?;
        let t = |sql: &str| count(c, sql, params![uid, sid]);
        Some([
            t("SELECT SUM(session_end-session_start) FROM plan_sessions WHERE user_id=?1 AND server_id=?2"),
            t("SELECT SUM(afk_time) FROM plan_sessions WHERE user_id=?1 AND server_id=?2"),
            t("SELECT COUNT(*) FROM plan_sessions WHERE user_id=?1 AND server_id=?2"),
            count(c, "SELECT COUNT(*) FROM plan_kills WHERE killer_uuid=?1", params![uuid]),
            t("SELECT SUM(deaths) FROM plan_sessions WHERE user_id=?1 AND server_id=?2"),
            t("SELECT SUM(mob_kills) FROM plan_sessions WHERE user_id=?1 AND server_id=?2"),
            t("SELECT registered FROM plan_user_info WHERE user_id=?1 AND server_id=?2"),
        ])
    })
    .flatten()
}

pub fn registered_count() -> i64 {
    read(|c| {
        let sid = server_id(c, &server());
        count(c, "SELECT COUNT(*) FROM plan_user_info WHERE server_id=?1", params![sid])
    })
    .unwrap_or(0)
}
