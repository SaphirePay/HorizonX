//! Voters: VotingPlugin's SQLite user table (`Users.db`, table `Users`, AdvancedCore's columns: AllTimeTotal,
//! MonthTotal, WeeklyTotal, DailyTotal, Points, LastVotes `site//ms%line%...`, streaks, OfflineVotes...), online
//! players in memory (read at pre-login), offline votes written by the data thread and given at the next join,
//! the day/week/month resets, top lists cached for /vote top and placeholders, and the vote party counter.

use crate::settings::{self, Settings};
use horizon_bukkit::Player;
use rusqlite::types::Value as Sql;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, RwLock};

#[derive(Clone, Default)]
pub struct User {
    pub uuid: String,
    pub name: String,
    pub all: i64,
    pub month: i64,
    pub week: i64,
    pub day: i64,
    pub points: i64,
    pub last_votes: HashMap<String, i64>,
    pub day_streak: i64,
    pub week_streak: i64,
    pub month_streak: i64,
    pub best_day_streak: i64,
    pub best_week_streak: i64,
    pub best_month_streak: i64,
    pub high_day: i64,
    pub high_week: i64,
    pub high_month: i64,
    pub last_month: i64,
    pub no_broadcast: bool,
    pub offline: Vec<(String, i64)>,
    pub party_votes: i64,
    pub milestones: HashMap<String, i64>,
    /// Day/week/month numbers of the last vote that counted for a streak.
    pub streak_marks: (i64, i64, i64),
    pub reminded: i64,
    pub dirty: bool,
}

const LINE: &str = "%line%";

impl User {
    fn encode_votes(&self) -> String {
        self.last_votes.iter().map(|(k, v)| format!("{k}//{v}")).collect::<Vec<_>>().join(LINE)
    }
    pub fn total(&self, kind: &str) -> i64 {
        match kind {
            "ALLTIME" | "ALL" | "ALL_TIME" => self.all,
            "MONTHLY" | "MONTH" => self.month,
            "WEEKLY" | "WEEK" => self.week,
            "DAILY" | "DAY" => self.day,
            "POINTS" => self.points,
            "ALLSITES_TODAY" => {
                let today = crate::time::day_index(crate::time::now_ms());
                self.last_votes.values().filter(|t| crate::time::day_index(**t) == today).count() as i64
            }
            _ => self.all,
        }
    }
    /// Ms until `site` can be voted on again (0: now).
    pub fn wait(&self, site: &settings::Site) -> i64 {
        let Some(last) = self.last_votes.get(&site.key).copied() else { return 0 };
        let now = crate::time::now_ms();
        if site.daily {
            if crate::time::day_index(last) < crate::time::day_index(now) { 0 } else { crate::time::ms_until_next_day(now) }
        } else {
            (last + site.delay_ms - now).max(0)
        }
    }
}

// ---- Database ----

static DB: Mutex<Option<Connection>> = Mutex::new(None);

const COLUMNS: [(&str, &str); 24] = [
    ("PlayerName", "VARCHAR(16)"),
    ("AllTimeTotal", "INT DEFAULT '0'"),
    ("MonthTotal", "INT DEFAULT '0'"),
    ("WeeklyTotal", "INT DEFAULT '0'"),
    ("DailyTotal", "INT DEFAULT '0'"),
    ("Points", "INT DEFAULT '0'"),
    ("LastVotes", "MEDIUMTEXT"),
    ("DayVoteStreak", "INT DEFAULT '0'"),
    ("WeekVoteStreak", "INT DEFAULT '0'"),
    ("MonthVoteStreak", "INT DEFAULT '0'"),
    ("BestDayVoteStreak", "INT DEFAULT '0'"),
    ("BestWeekVoteStreak", "INT DEFAULT '0'"),
    ("BestMonthVoteStreak", "INT DEFAULT '0'"),
    ("HighestDailyTotal", "INT DEFAULT '0'"),
    ("HighestWeeklyTotal", "INT DEFAULT '0'"),
    ("HighestMonthlyTotal", "INT DEFAULT '0'"),
    ("LastMonthTotal", "INT DEFAULT '0'"),
    ("DisableBroadcast", "VARCHAR(5)"),
    ("OfflineVotes", "MEDIUMTEXT"),
    ("VotePartyVotes", "INT DEFAULT '0'"),
    ("LastOnline", "VARCHAR(20)"),
    ("HxMilestones", "MEDIUMTEXT"),
    ("HxStreakMarks", "VARCHAR(64)"),
    ("VoteRemindersLast", "VARCHAR(20)"),
];

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

pub fn open(folder: &std::path::Path) -> bool {
    let Ok(c) = Connection::open(folder.join("Users.db")) else { return false };
    let _ = c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; CREATE TABLE IF NOT EXISTS Users (uuid VARCHAR(37) PRIMARY KEY);");
    let existing: Vec<String> = c.prepare("PRAGMA table_info(Users)").and_then(|mut st| st.query_map([], |r| r.get::<_, String>(1))?.collect()).unwrap_or_default();
    for (col, ty) in COLUMNS {
        if !existing.iter().any(|e| e.eq_ignore_ascii_case(col)) {
            let _ = c.execute(&format!("ALTER TABLE Users ADD COLUMN `{col}` {ty}"), []);
        }
    }
    *DB.lock().unwrap_or_else(|e| e.into_inner()) = Some(c);
    true
}

pub fn close() {
    *DB.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn int(v: &Sql) -> i64 {
    match v {
        Sql::Integer(i) => *i,
        Sql::Real(f) => *f as i64,
        Sql::Text(t) => t.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn text(v: &Sql) -> String {
    match v {
        Sql::Text(t) => t.clone(),
        Sql::Integer(i) => i.to_string(),
        _ => String::new(),
    }
}

const SELECT: &str = "SELECT uuid, PlayerName, AllTimeTotal, MonthTotal, WeeklyTotal, DailyTotal, Points, LastVotes, DayVoteStreak, WeekVoteStreak, MonthVoteStreak, BestDayVoteStreak, BestWeekVoteStreak, BestMonthVoteStreak, HighestDailyTotal, HighestWeeklyTotal, HighestMonthlyTotal, LastMonthTotal, DisableBroadcast, OfflineVotes, VotePartyVotes, HxMilestones, HxStreakMarks, VoteRemindersLast FROM Users";

fn read_row(r: &rusqlite::Row) -> rusqlite::Result<User> {
    let g = |i: usize| r.get::<_, Sql>(i).unwrap_or(Sql::Null);
    let mut u = User {
        uuid: text(&g(0)).to_ascii_lowercase(),
        name: text(&g(1)),
        all: int(&g(2)),
        month: int(&g(3)),
        week: int(&g(4)),
        day: int(&g(5)),
        points: int(&g(6)),
        day_streak: int(&g(8)),
        week_streak: int(&g(9)),
        month_streak: int(&g(10)),
        best_day_streak: int(&g(11)),
        best_week_streak: int(&g(12)),
        best_month_streak: int(&g(13)),
        high_day: int(&g(14)),
        high_week: int(&g(15)),
        high_month: int(&g(16)),
        last_month: int(&g(17)),
        no_broadcast: text(&g(18)).eq_ignore_ascii_case("true"),
        party_votes: int(&g(20)),
        reminded: int(&g(23)),
        ..User::default()
    };
    for e in text(&g(7)).split(LINE) {
        if let Some((k, v)) = e.split_once("//") {
            u.last_votes.insert(k.to_owned(), v.trim().parse().unwrap_or(0));
        }
    }
    for e in text(&g(19)).split(LINE).filter(|e| !e.is_empty()) {
        let (site, t) = e.split_once("//").unwrap_or((e, "0"));
        u.offline.push((site.to_owned(), t.parse().unwrap_or(0)));
    }
    for e in text(&g(21)).split(LINE) {
        if let Some((k, v)) = e.split_once("//") {
            u.milestones.insert(k.to_owned(), v.parse().unwrap_or(0));
        }
    }
    let marks: Vec<i64> = text(&g(22)).split(',').map(|x| x.parse().unwrap_or(0)).collect();
    u.streak_marks = (marks.first().copied().unwrap_or(0), marks.get(1).copied().unwrap_or(0), marks.get(2).copied().unwrap_or(0));
    Ok(u)
}

fn write(c: &Connection, u: &User) -> rusqlite::Result<()> {
    c.execute("INSERT OR IGNORE INTO Users (uuid) VALUES (?1)", params![u.uuid])?;
    c.execute(
        "UPDATE Users SET PlayerName=?2, AllTimeTotal=?3, MonthTotal=?4, WeeklyTotal=?5, DailyTotal=?6, Points=?7, LastVotes=?8, DayVoteStreak=?9, WeekVoteStreak=?10, MonthVoteStreak=?11, BestDayVoteStreak=?12, BestWeekVoteStreak=?13, BestMonthVoteStreak=?14, HighestDailyTotal=?15, HighestWeeklyTotal=?16, HighestMonthlyTotal=?17, LastMonthTotal=?18, DisableBroadcast=?19, OfflineVotes=?20, VotePartyVotes=?21, HxMilestones=?22, HxStreakMarks=?23, VoteRemindersLast=?24, LastOnline=?25 WHERE uuid=?1",
        params![
            u.uuid,
            u.name,
            u.all,
            u.month,
            u.week,
            u.day,
            u.points,
            u.encode_votes(),
            u.day_streak,
            u.week_streak,
            u.month_streak,
            u.best_day_streak,
            u.best_week_streak,
            u.best_month_streak,
            u.high_day,
            u.high_week,
            u.high_month,
            u.last_month,
            u.no_broadcast.to_string(),
            u.offline.iter().map(|(s, t)| format!("{s}//{t}")).collect::<Vec<_>>().join(LINE),
            u.party_votes,
            u.milestones.iter().map(|(k, v)| format!("{k}//{v}")).collect::<Vec<_>>().join(LINE),
            format!("{},{},{}", u.streak_marks.0, u.streak_marks.1, u.streak_marks.2),
            u.reminded.to_string(),
            crate::time::now_ms().to_string(),
        ],
    )?;
    Ok(())
}

pub fn save(users: &[User]) {
    if users.is_empty() {
        return;
    }
    with(|c| {
        c.execute_batch("BEGIN")?;
        for u in users {
            write(c, u)?;
        }
        c.execute_batch("COMMIT")
    });
}

pub fn load(uuid: &str) -> Option<User> {
    with(|c| {
        let mut st = c.prepare(&format!("{SELECT} WHERE LOWER(uuid) = LOWER(?1)"))?;
        let mut rows = st.query_map(params![uuid], read_row)?;
        Ok(rows.next().transpose()?)
    })
    .flatten()
}

pub fn load_by_name(name: &str) -> Option<User> {
    with(|c| {
        let mut st = c.prepare(&format!("{SELECT} WHERE LOWER(PlayerName) = LOWER(?1) LIMIT 1"))?;
        let mut rows = st.query_map(params![name], read_row)?;
        Ok(rows.next().transpose()?)
    })
    .flatten()
}

// ---- Online users ----

pub static USERS: LazyLock<Mutex<HashMap<String, User>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
pub static PENDING: LazyLock<Mutex<HashMap<String, User>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn users() -> MutexGuard<'static, HashMap<String, User>> {
    USERS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn uuid_by_name(name: &str) -> Option<String> {
    users().values().find(|u| u.name.eq_ignore_ascii_case(name)).map(|u| u.uuid.clone())
}

/// Pre-login (off the server thread): the row read now.
pub fn prelogin(uuid: &str, name: &str) {
    let mut u = load(uuid).unwrap_or_else(|| User { uuid: uuid.to_owned(), ..User::default() });
    u.uuid = uuid.to_owned();
    u.name = name.to_owned();
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).insert(uuid.to_owned(), u);
}

/// Join: the user online; offline votes then given (returned for the caller to reward).
pub fn join(player: &Player) -> (Vec<(String, i64)>, bool) {
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner()).remove(&uuid);
    let first = pending.as_ref().is_some_and(|u| u.all == 0 && u.last_votes.is_empty() && u.points == 0);
    let mut u = pending.unwrap_or_else(|| User { uuid: uuid.clone(), ..User::default() });
    u.name = player.get_name().to_owned();
    let offline = std::mem::take(&mut u.offline);
    u.dirty = true;
    users().insert(uuid, u);
    (offline, first)
}

pub fn quit(uuid: &str) {
    if let Some(u) = users().remove(uuid) {
        std::thread::spawn(move || save(&[u]));
    }
}

pub fn dirty() -> Vec<User> {
    users().values_mut().filter_map(|u| std::mem::take(&mut u.dirty).then(|| u.clone())).collect()
}

pub fn add_points(uuid: &str, n: i64) {
    if let Some(u) = users().get_mut(uuid) {
        u.points += n;
        u.dirty = true;
    }
}

// ---- Server data: periods and the vote party ----

#[derive(Default)]
pub struct Server {
    pub day: i64,
    pub week: i64,
    pub month: i64,
    pub party: i64,
    pub party_day: i64,
    pub global: [i64; 4],
}

pub static SERVER: Mutex<Server> = Mutex::new(Server { day: 0, week: 0, month: 0, party: 0, party_day: 0, global: [0; 4] });

fn server_file() -> std::path::PathBuf {
    crate::folder().join("ServerData.yml")
}

pub fn load_server() {
    let c = horizon_kit::Config::load(&crate::folder(), "ServerData.yml", "");
    let now = crate::time::now_ms();
    let mut s = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    s.day = c.int("PrevDay", crate::time::day_index(now));
    s.week = c.int("PrevWeek", crate::time::week_index(now));
    s.month = c.int("PrevMonth", crate::time::month_index(now));
    s.party = c.int("VoteParty.Total", 0);
    s.party_day = c.int("VoteParty.Day", 0);
    s.global = [c.int("Global.Daily", 0), c.int("Global.Weekly", 0), c.int("Global.Monthly", 0), c.int("Global.AllTime", 0)];
}

pub fn save_server() {
    let text = {
        let s = SERVER.lock().unwrap_or_else(|e| e.into_inner());
        format!(
            "# Kept by the Horizon VotingPlugin: the last day/week/month seen (for resets), the vote party and server totals.\nPrevDay: {}\nPrevWeek: {}\nPrevMonth: {}\nVoteParty:\n  Total: {}\n  Day: {}\nGlobal:\n  Daily: {}\n  Weekly: {}\n  Monthly: {}\n  AllTime: {}\n",
            s.day, s.week, s.month, s.party, s.party_day, s.global[0], s.global[1], s.global[2], s.global[3]
        )
    };
    let _ = std::fs::write(server_file(), text);
}

/// Day, week and month rollovers (the data thread, once a minute): totals moved to highest/last, streaks of
/// those who didn't vote broken, in the table and in memory alike.
pub fn check_periods() {
    let now = crate::time::now_ms();
    let (d, w, m) = (crate::time::day_index(now), crate::time::week_index(now), crate::time::month_index(now));
    let (new_day, new_week, new_month) = {
        let s = SERVER.lock().unwrap_or_else(|e| e.into_inner());
        (d != s.day, w != s.week, m != s.month)
    };
    if !new_day && !new_week && !new_month {
        return;
    }
    // Everyone online written first, then the same change made in the table and in memory.
    let online: Vec<User> = users().values().cloned().collect();
    save(&online);
    with(|c| {
        if new_day {
            c.execute_batch("UPDATE Users SET HighestDailyTotal = MAX(IFNULL(HighestDailyTotal,0), IFNULL(DailyTotal,0)), DayVoteStreak = CASE WHEN IFNULL(DailyTotal,0) > 0 THEN DayVoteStreak ELSE 0 END, DailyTotal = 0")?;
        }
        if new_week {
            c.execute_batch("UPDATE Users SET HighestWeeklyTotal = MAX(IFNULL(HighestWeeklyTotal,0), IFNULL(WeeklyTotal,0)), WeekVoteStreak = CASE WHEN IFNULL(WeeklyTotal,0) > 0 THEN WeekVoteStreak ELSE 0 END, WeeklyTotal = 0")?;
        }
        if new_month {
            c.execute_batch("UPDATE Users SET HighestMonthlyTotal = MAX(IFNULL(HighestMonthlyTotal,0), IFNULL(MonthTotal,0)), MonthVoteStreak = CASE WHEN IFNULL(MonthTotal,0) > 0 THEN MonthVoteStreak ELSE 0 END, LastMonthTotal = IFNULL(MonthTotal,0), MonthTotal = 0")?;
        }
        Ok(())
    });
    {
        let mut us = users();
        for u in us.values_mut() {
            if new_day {
                u.high_day = u.high_day.max(u.day);
                if u.day == 0 {
                    u.day_streak = 0;
                }
                u.day = 0;
            }
            if new_week {
                u.high_week = u.high_week.max(u.week);
                if u.week == 0 {
                    u.week_streak = 0;
                }
                u.week = 0;
            }
            if new_month {
                u.high_month = u.high_month.max(u.month);
                if u.month == 0 {
                    u.month_streak = 0;
                }
                u.last_month = u.month;
                u.month = 0;
            }
        }
    }
    let s = settings::get();
    {
        let mut sv = SERVER.lock().unwrap_or_else(|e| e.into_inner());
        if new_day {
            sv.global[0] = 0;
            if s.party.get("ResetEachDay").and_then(|x| x.as_bool()).unwrap_or(false) {
                sv.party = 0;
            }
        }
        if new_week {
            sv.global[1] = 0;
            if s.party.get("ResetWeekly").and_then(|x| x.as_bool()).unwrap_or(false) {
                sv.party = 0;
            }
        }
        if new_month {
            sv.global[2] = 0;
            if s.party.get("ResetMonthly").and_then(|x| x.as_bool()).unwrap_or(false) {
                sv.party = 0;
            }
        }
        sv.day = d;
        sv.week = w;
        sv.month = m;
    }
    save_server();
    refresh_tops();
}

// ---- Top lists ----

#[derive(Default)]
pub struct Tops {
    pub all: Vec<(String, String, i64)>,
    pub month: Vec<(String, String, i64)>,
    pub week: Vec<(String, String, i64)>,
    pub day: Vec<(String, String, i64)>,
    pub last_month: Vec<(String, String, i64)>,
}

static TOPS: LazyLock<RwLock<Arc<Tops>>> = LazyLock::new(|| RwLock::new(Arc::new(Tops::default())));

pub fn tops() -> Arc<Tops> {
    TOPS.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The top lists read again (the data thread; online players written first).
pub fn refresh_tops() {
    save(&dirty());
    let q = |col: &str| -> Vec<(String, String, i64)> {
        with(|c| {
            let mut st = c.prepare(&format!("SELECT IFNULL(PlayerName,''), uuid, IFNULL({col},0) FROM Users WHERE IFNULL({col},0) > 0 ORDER BY CAST({col} AS INTEGER) DESC LIMIT 1000"))?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?.to_ascii_lowercase(), int(&r.get::<_, Sql>(2)?))))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap_or_default()
    };
    let t = Tops { all: q("AllTimeTotal"), month: q("MonthTotal"), week: q("WeeklyTotal"), day: q("DailyTotal"), last_month: q("LastMonthTotal") };
    *TOPS.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(t);
}

impl Tops {
    pub fn of(&self, kind: &str) -> &Vec<(String, String, i64)> {
        match kind.to_ascii_lowercase().as_str() {
            "all" | "alltime" => &self.all,
            "week" | "weekly" => &self.week,
            "day" | "daily" => &self.day,
            "lastmonth" => &self.last_month,
            _ => &self.month,
        }
    }
}

/// Offline vote (the data thread): the row's totals counted and the vote kept for the next join.
pub fn offline_vote(name: &str, site: &str, time: i64, s: &Settings) -> bool {
    let Some(mut u) = load_by_name(name) else { return false };
    count_vote(&mut u, site, time, s);
    if s.offline_limit < 0 || (u.offline.len() as i64) < s.offline_limit {
        u.offline.push((site.to_owned(), time));
    }
    save(&[u]);
    true
}

/// The totals, last vote time, points and streaks of one vote.
pub fn count_vote(u: &mut User, site: &str, time: i64, s: &Settings) {
    u.all += 1;
    u.month += 1;
    u.week += 1;
    u.day += 1;
    u.points += s.points_on_vote;
    u.party_votes += 1;
    u.last_votes.insert(site.to_owned(), time);
    let (d, w, m) = (crate::time::day_index(time), crate::time::week_index(time), crate::time::month_index(time));
    if u.streak_marks.0 != d {
        u.day_streak = if u.streak_marks.0 == d - 1 { u.day_streak + 1 } else { 1 };
        u.streak_marks.0 = d;
    }
    if u.streak_marks.1 != w {
        u.week_streak = if u.streak_marks.1 == w - 1 { u.week_streak + 1 } else { 1 };
        u.streak_marks.1 = w;
    }
    if u.streak_marks.2 != m {
        u.month_streak = if u.streak_marks.2 == m - 1 { u.month_streak + 1 } else { 1 };
        u.streak_marks.2 = m;
    }
    u.best_day_streak = u.best_day_streak.max(u.day_streak);
    u.best_week_streak = u.best_week_streak.max(u.week_streak);
    u.best_month_streak = u.best_month_streak.max(u.month_streak);
    u.high_day = u.high_day.max(u.day);
    u.high_week = u.high_week.max(u.week);
    u.high_month = u.high_month.max(u.month);
    u.dirty = true;
    let mut sv = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    for g in sv.global.iter_mut() {
        *g += 1;
    }
}
