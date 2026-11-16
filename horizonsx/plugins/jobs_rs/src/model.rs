//! Jobs players in memory (JobsPlayer + JobProgression): the payment pipeline (Jobs.action: income, points and
//! exp from each job's equations, boosts, minimums, limits), levels up/down with their messages and commands, the
//! payment buffer (money paid every `economy-batch-delay` s, as Jobs' BufferedEconomy), block protection and the
//! explored-chunk counts.

use crate::settings::{self, Action, Settings};
use horizon_bukkit::{BossBar, Player};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex, MutexGuard};

#[derive(Clone)]
pub struct Prog {
    pub job: String,
    pub level: i32,
    pub exp: f64,
}

pub struct JPlayer {
    pub player: Player,
    pub uuid: String,
    pub name: String,
    pub user_id: i64,
    pub jobs: Vec<Prog>,
    pub archived: Vec<(String, i32, f64, i64)>,
    pub points: f64,
    pub total_points: f64,
    /// money, exp, points: collected in the window, when the window started (ms).
    pub limits: [(f64, i64); 3],
    pub message_options: i64,
    pub max_jobs: i32,
    pub boost: [f64; 3],
    pub join_perm: HashSet<String>,
    pub vip: HashSet<String>,
    pub creative: bool,
    pub actionbar: bool,
    pub bossbar_on: bool,
    pub pay_money: f64,
    pub pay_points: f64,
    pub shown: (f64, f64, f64),
    pub bossbar: Option<(BossBar, i64)>,
    pub bar_job: Option<String>,
    pub dirty: bool,
    pub last_chunk: Option<(String, i32, i32)>,
    pub flying: bool,
}

impl JPlayer {
    pub fn in_job(&self, job: &str) -> bool {
        self.jobs.iter().any(|p| p.job.eq_ignore_ascii_case(job))
    }
    pub fn total_level(&self) -> i32 {
        self.jobs.iter().map(|p| p.level).sum()
    }
    pub fn max_level(&self, s: &Settings, job: &str) -> i32 {
        let Some(j) = s.job(job) else { return 0 };
        if j.vip_max_level > 0 && self.vip.contains(&j.name.to_ascii_lowercase()) { j.vip_max_level } else { j.max_level }
    }
    fn row(&self) -> crate::db::Row {
        let kinds = ["MONEY", "EXP", "POINTS"];
        crate::db::Row {
            user_id: self.user_id,
            name: self.name.clone(),
            jobs: self.jobs.iter().map(|p| (p.job.clone(), p.level, p.exp)).collect(),
            archived: self.archived.clone(),
            points: (self.points, self.total_points),
            limits: self.limits.iter().enumerate().filter(|(_, l)| l.1 > 0).map(|(i, l)| (kinds[i].to_owned(), l.0, l.1)).collect(),
            message_options: self.message_options,
        }
    }
}

pub static PLAYERS: LazyLock<Mutex<HashMap<String, JPlayer>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// Rows read at pre-login, waiting for the join.
pub static PENDING: LazyLock<Mutex<HashMap<String, crate::db::Loaded>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn players() -> MutexGuard<'static, HashMap<String, JPlayer>> {
    PLAYERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Global boosts (`/jobs boost`): job (lowercase, "all") -> [money, exp, points] (multiplier - 1, until ms).
pub static BOOSTS: LazyLock<Mutex<HashMap<String, [(f64, i64); 3]>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn global_boost(job: &str, kind: usize) -> f64 {
    let now = crate::now_ms();
    let b = BOOSTS.lock().unwrap_or_else(|e| e.into_inner());
    let get = |k: &str| b.get(k).map(|v| v[kind]).filter(|(_, until)| *until == 0 || *until > now).map(|(m, _)| m).unwrap_or(0.0);
    get(&job.to_ascii_lowercase()) + get("all")
}

/// The schedule.yml boost running now for a job (multiplier - 1).
fn schedule_boost(s: &Settings, job: &str, kind: usize) -> f64 {
    crate::schedule::active(s).iter().filter(|b| b.jobs.iter().any(|j| j.eq_ignore_ascii_case("all") || j.eq_ignore_ascii_case(job))).map(|b| [b.money, b.exp, b.points][kind] - 1.0).sum()
}

const BOOST_AMOUNTS: [&str; 18] = ["0.05", "0.1", "0.15", "0.2", "0.25", "0.3", "0.4", "0.5", "0.6", "0.75", "1", "1.0", "1.5", "2", "2.0", "2.5", "3", "5"];

/// The permission-derived values for one player (join and every few minutes; each is a game query).
pub fn read_perms(p: &Player, s: &Settings) -> (i32, [f64; 3], HashSet<String>, HashSet<String>) {
    let mut max = s.max_jobs;
    for n in (1..=30).rev() {
        if p.has_permission(&format!("jobs.max.{n}")) {
            max = max.max(n);
            break;
        }
    }
    let mut boost = [0.0; 3];
    for (i, kind) in ["money", "exp", "points"].iter().enumerate() {
        for amount in BOOST_AMOUNTS.iter().rev() {
            if p.has_permission(&format!("jobs.boost.all.{kind}.{amount}")) || p.has_permission(&format!("jobs.boost.all.all.{amount}")) {
                boost[i] = amount.parse().unwrap_or(0.0);
                break;
            }
        }
    }
    let mut join = HashSet::new();
    let mut vip = HashSet::new();
    for j in &s.jobs {
        let n = j.name.to_ascii_lowercase();
        // Jobs registers jobs.join.<job> with default true: allowed unless a permission plugin says false.
        let node = format!("jobs.join.{n}");
        if p.has_permission(&node) || !p.is_permission_set(&node) {
            join.insert(n.clone());
        }
        if j.vip_max_level > 0 && p.has_permission(&format!("jobs.{n}.vipmaxlevel")) {
            vip.insert(n);
        }
    }
    (max, boost, join, vip)
}

/// A joining player: the rows read at pre-login (or read now on a thread, then added).
pub fn join(player: &Player) {
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let name = player.get_name().to_owned();
    let loaded = PENDING.lock().unwrap_or_else(|e| e.into_inner()).remove(&uuid);
    match loaded {
        Some(l) => add(player, l),
        None => {
            let p = player.clone();
            std::thread::spawn(move || {
                if let Some(l) = crate::db::load_player(&uuid, &name) {
                    crate::later(move || add(&p, l));
                }
            });
        }
    }
}

fn add(player: &Player, l: crate::db::Loaded) {
    let s = settings::get();
    let (max_jobs, boost, join_perm, vip) = read_perms(player, &s);
    let now = crate::now_ms();
    let mut limits = [(0.0, 0); 3];
    for (kind, collected, started) in &l.limits {
        let i = match kind.as_str() {
            "MONEY" => 0,
            "EXP" => 1,
            _ => 2,
        };
        if now - started < s.limits[i].seconds * 1000 {
            limits[i] = (*collected, *started);
        }
    }
    let jobs: Vec<Prog> = l.jobs.iter().filter_map(|(n, lv, xp)| s.job(n).map(|j| Prog { job: j.name.clone(), level: (*lv).max(1), exp: *xp })).collect();
    let jp = JPlayer {
        player: player.clone(),
        uuid: player.get_unique_id().to_ascii_lowercase(),
        name: player.get_name().to_owned(),
        user_id: l.user_id,
        jobs,
        archived: l.archived,
        points: l.points.0,
        total_points: l.points.1,
        limits,
        message_options: l.message_options,
        max_jobs,
        boost,
        join_perm,
        vip,
        creative: player.get_game_mode() == "CREATIVE",
        actionbar: s.action_bars && !s.default_actionbar.eq_ignore_ascii_case("Off"),
        bossbar_on: s.bossbar && !s.default_bossbar.eq_ignore_ascii_case("Off"),
        pay_money: 0.0,
        pay_points: 0.0,
        shown: (0.0, 0.0, 0.0),
        bossbar: None,
        bar_job: None,
        dirty: false,
        last_chunk: None,
        flying: false,
    };
    players().insert(jp.uuid.clone(), jp);
}

/// A leaving player: paid what is owed, saved, forgotten.
pub fn quit(uuid: &str) {
    let jp = players().remove(uuid);
    let Some(jp) = jp else { return };
    if jp.pay_money != 0.0 {
        pay_money(&jp.name, jp.pay_money);
    }
    if let Some((bar, _)) = &jp.bossbar {
        bar.remove_all();
    }
    let row = jp.row();
    std::thread::spawn(move || crate::db::save(&[row]));
}

fn pay_money(name: &str, amount: f64) {
    if amount > 0.0 {
        horizon_kit::hooks::economy::deposit(name, amount);
    } else if amount < 0.0 {
        horizon_kit::hooks::economy::withdraw(name, -amount);
    }
}

/// The dirty players' rows for the saver.
pub fn dirty_rows() -> Vec<crate::db::Row> {
    let mut ps = players();
    ps.values_mut().filter_map(|p| std::mem::take(&mut p.dirty).then(|| p.row())).collect()
}

pub fn all_rows() -> Vec<crate::db::Row> {
    players().values().map(JPlayer::row).collect()
}

/// What to do once the lock is gone.
#[derive(Default)]
pub struct Out(pub Vec<Box<dyn FnOnce() + Send>>);

impl Out {
    pub fn push(&mut self, f: impl FnOnce() + Send + 'static) {
        self.0.push(Box::new(f));
    }
    pub fn run(self) {
        for f in self.0 {
            f();
        }
    }
}

fn fill_cmd(cmd: &str, name: &str, job: &str, old: i32, new: i32) -> String {
    cmd.replace("[player]", name).replace("[playerName]", name).replace("[name]", name).replace("[jobname]", job).replace("[job]", job).replace("[oldlevel]", &old.to_string()).replace("[newlevel]", &new.to_string())
}

pub fn run_command(line: &str, player: &Player) {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("player:") {
        player.perform_command(rest.trim_start_matches('/'));
    } else {
        let rest = line.strip_prefix("console:").unwrap_or(line);
        horizon_bukkit::fam_core::dispatch_command_as(&horizon_bukkit::CommandSender::Console, rest.trim_start_matches('/'));
    }
}

/// Exp added to a job: levels gained (with their messages, sounds and commands) or lost.
fn add_exp(s: &Settings, jp: &mut JPlayer, idx: usize, exp: f64, out: &mut Out) {
    let numjobs = jp.jobs.len();
    let job_name = jp.jobs[idx].job.clone();
    let Some(job) = s.job(&job_name).cloned() else { return };
    let max_level = jp.max_level(s, &job_name);
    let prog = &mut jp.jobs[idx];
    let old = prog.level;
    prog.exp += exp;
    if prog.level == 1 && prog.exp < 0.0 {
        prog.exp = 0.0;
    }
    while prog.exp < 0.0 && prog.level > 1 {
        prog.level -= 1;
        prog.exp += job.max_exp(prog.level, numjobs);
    }
    while prog.exp >= job.max_exp(prog.level, numjobs) && (max_level <= 0 || prog.level < max_level) {
        prog.exp -= job.max_exp(prog.level, numjobs);
        prog.level += 1;
    }
    if max_level > 0 && prog.level >= max_level {
        prog.exp = prog.exp.min(job.max_exp(prog.level, numjobs) - 1.0).max(0.0);
    }
    let new = prog.level;
    jp.dirty = true;
    if new == old {
        return;
    }
    let player = jp.player.clone();
    let name = jp.name.clone();
    let display = player.get_display_name().to_owned();
    let jobname = format!("{}{}", settings::chat_color(&job.color), job.name);
    if new < old {
        out.push({
            let t = s.msg("message.leveldown.message", &[("lostLevel", &(old - new).to_string()), ("jobname", &jobname), ("joblevel", &new.to_string())]);
            let p = player.clone();
            move || p.send_message(&t)
        });
        return;
    }
    let broadcast = s.broadcast_levelup && (s.broadcast_levels.is_empty() || s.broadcast_levels.contains(&new));
    let key = if broadcast { "message.levelup.broadcast" } else { "message.levelup.nobroadcast" };
    let text = s.msg(key, &[("playerdisplayname", &display), ("playername", &name), ("joblevel", &new.to_string()), ("jobname", &jobname)]);
    let sound = s.sound_levelup;
    {
        let p = player.clone();
        out.push(move || {
            if broadcast {
                horizon_bukkit::broadcast_message(&text);
            } else {
                p.send_message(&text);
            }
            if let Some((v, pitch)) = sound {
                p.play_sound(&p.get_location(), "ENTITY_PLAYER_LEVELUP", v, pitch);
            }
        });
    }
    for c in &job.commands {
        let hit = (old + 1..=new).any(|lv| (c.from == 0 || lv >= c.from) && (c.until == 0 || lv <= c.until));
        if hit {
            for cmd in &c.commands {
                let line = fill_cmd(cmd, &name, &job.name, old, new);
                let p = player.clone();
                out.push(move || run_command(&line, &p));
            }
        }
    }
    if max_level > 0 && new >= max_level {
        for cmd in &job.max_level_commands {
            let line = fill_cmd(cmd, &name, &job.name, old, new);
            let p = player.clone();
            out.push(move || run_command(&line, &p));
        }
    }
}

fn under_limit(s: &Settings, jp: &mut JPlayer, kind: usize, amount: f64) -> bool {
    let l = &s.limits[kind];
    if !l.enabled || amount <= 0.0 {
        return true;
    }
    let now = crate::now_ms();
    if jp.limits[kind].1 == 0 || now - jp.limits[kind].1 > l.seconds * 1000 {
        jp.limits[kind] = (0.0, now);
    }
    let cap = l.equation.eval(&[("totallevel", jp.total_level() as f64)]);
    if jp.limits[kind].0 + amount > cap {
        return false;
    }
    jp.limits[kind].0 += amount;
    true
}

/// One paid action of `count` units (`Jobs.action`): every job the player works pays for `key`.
pub fn action(uuid: &str, action: Action, key: &str, world: &str, count: f64) {
    let s = settings::get();
    if !s.uses(action) || !s.world_enabled(world) {
        return;
    }
    let mut out = Out::default();
    {
        let mut ps = players();
        let Some(jp) = ps.get_mut(uuid) else { return };
        if jp.creative && !s.pay_creative {
            return;
        }
        let numjobs = jp.jobs.len();
        let maxjobs = jp.max_jobs;
        if numjobs == 0 {
            let Some(none) = s.none.as_ref() else { return };
            let Some(info) = none.info(action, key, 1) else { return };
            let (mut income, mut points, _) = none.pay(info, 1, 0, maxjobs);
            income *= count;
            points *= count;
            income = boosted(&s, income, jp.boost[0] + global_boost("none", 0) + schedule_boost(&s, "none", 0), s.min_money);
            points = boosted(&s, points, jp.boost[2] + global_boost("none", 2) + schedule_boost(&s, "none", 2), s.min_points);
            if !under_limit(&s, jp, 0, income) {
                income = 0.0;
            }
            if !under_limit(&s, jp, 2, points) {
                points = 0.0;
            }
            jp.pay_money += income;
            jp.pay_points += points;
            jp.shown.0 += income;
            jp.shown.2 += points;
            if points != 0.0 {
                jp.points += points;
                jp.total_points += points.max(0.0);
                jp.dirty = true;
            }
            return;
        }
        for idx in 0..numjobs {
            let prog = jp.jobs[idx].clone();
            let Some(job) = s.job(&prog.job) else { continue };
            if !job.works_in(world) {
                continue;
            }
            if s.disable_payment_max && job.max_level > 0 && prog.level >= jp.max_level(&s, &prog.job) {
                continue;
            }
            let Some(info) = job.info(action, key, prog.level) else { continue };
            let (mut income, mut points, mut exp) = job.pay(info, prog.level, numjobs, maxjobs);
            income *= count;
            points *= count;
            exp *= count;
            income = boosted(&s, income, jp.boost[0] + global_boost(&job.name, 0) + schedule_boost(&s, &job.name, 0), s.min_money);
            exp = boosted(&s, exp, jp.boost[1] + global_boost(&job.name, 1) + schedule_boost(&s, &job.name, 1), s.min_exp);
            points = boosted(&s, points, jp.boost[2] + global_boost(&job.name, 2) + schedule_boost(&s, &job.name, 2), s.min_points);
            if !under_limit(&s, jp, 0, income) {
                income = 0.0;
                if s.limits[0].stop_with.contains(&'E') {
                    exp = 0.0;
                }
                if s.limits[0].stop_with.contains(&'P') {
                    points = 0.0;
                }
            }
            if !under_limit(&s, jp, 1, exp) {
                exp = 0.0;
            }
            if !under_limit(&s, jp, 2, points) {
                points = 0.0;
            }
            if income == 0.0 && points == 0.0 && exp == 0.0 {
                continue;
            }
            jp.pay_money += income;
            if points != 0.0 {
                jp.points += points;
                jp.total_points += points.max(0.0);
            }
            jp.shown.0 += income;
            jp.shown.1 += exp;
            jp.shown.2 += points;
            jp.bar_job = Some(job.name.clone());
            if s.add_xp_player && exp != 0.0 {
                let p = jp.player.clone();
                let e = exp.round() as i32;
                out.push(move || p.give_exp(e));
            }
            add_exp(&s, jp, idx, exp, &mut out);
        }
    }
    out.run();
}

fn boosted(s: &Settings, amount: f64, boost: f64, min: Option<f64>) -> f64 {
    if amount == 0.0 {
        return 0.0;
    }
    let mut f = amount;
    if amount > 0.0 || s.negative_boost {
        f = amount + amount * boost;
    }
    if (amount > 0.0 && f < 0.0) || (amount < 0.0 && f > 0.0) {
        f = 0.0;
    }
    if let Some(m) = min
        && amount > 0.0
    {
        f = f.max(amount * m);
    }
    f
}

/// Money owed paid out (every `economy-batch-delay` s, on the server thread).
pub fn flush_payments() {
    let owed: Vec<(String, f64)> = {
        let mut ps = players();
        ps.values_mut().filter(|p| p.pay_money != 0.0).map(|p| (p.name.clone(), std::mem::take(&mut p.pay_money))).collect()
    };
    for (name, amount) in owed {
        pay_money(&name, amount);
    }
}

/// The action bar / boss bar with what was earned since the last show (every 10 ticks).
pub fn flush_bars() {
    let s = settings::get();
    let now = crate::now_ms();
    let mut work: Vec<(Player, Option<String>, Option<(String, f64, Option<BossBar>)>)> = Vec::new();
    let mut hide: Vec<BossBar> = Vec::new();
    {
        let mut ps = players();
        for jp in ps.values_mut() {
            if let Some((bar, until)) = &jp.bossbar
                && *until > 0
                && *until <= now
            {
                hide.push(bar.clone());
                jp.bossbar = Some((bar.clone(), 0));
            }
            let (m, e, p) = std::mem::take(&mut jp.shown);
            jp.pay_points = 0.0;
            if m == 0.0 && e == 0.0 && p == 0.0 {
                continue;
            }
            let mut text = None;
            if s.action_bars && jp.actionbar {
                let mut t = String::new();
                if m != 0.0 {
                    t.push_str(&s.msg("command.toggle.output.paid.ACmoney", &[("amount", &s.money(m))]));
                }
                if p != 0.0 {
                    t.push_str(&s.msg("command.toggle.output.paid.ACpoints", &[("points", &s.points_fmt(p))]));
                }
                if e != 0.0 {
                    t.push_str(&s.msg("command.toggle.output.paid.ACexp", &[("exp", &s.exp_fmt(e))]));
                }
                text = Some(t);
            }
            let mut bar = None;
            if s.bossbar && jp.bossbar_on
                && let Some(jn) = jp.bar_job.clone()
                && let Some(prog) = jp.jobs.iter().find(|x| x.job == jn)
                && let Some(job) = s.job(&jn)
            {
                let max = job.max_exp(prog.level, jp.jobs.len());
                let title = s.msg("command.stats.bossBarOutput", &[("joblevel", &prog.level.to_string()), ("jobname", &format!("{}{}", settings::chat_color(&job.color), job.name)), ("jobxp", &s.exp_fmt(prog.exp)), ("jobmaxxp", &format!("{max:.0}")), ("gain", &s.msg("command.stats.bossBarGain", &[("gain", &s.exp_fmt(e))]))]);
                bar = Some((title, (prog.exp / max).clamp(0.0, 1.0), jp.bossbar.as_ref().map(|b| b.0.clone())));
                if let Some(b) = jp.bossbar.as_mut() {
                    b.1 = now + s.bossbar_timer as i64 * 1000;
                }
            }
            work.push((jp.player.clone(), text, bar));
        }
    }
    for b in hide {
        b.set_visible(false);
    }
    let mut made = Vec::new();
    for (p, text, bar) in work {
        if let Some(t) = text {
            p.send_action_bar(&horizon_bukkit::adventure::legacy(&t));
        }
        if let Some((title, progress, existing)) = bar {
            match existing {
                Some(b) => {
                    b.set_title(&title);
                    b.set_progress(progress);
                    b.set_visible(true);
                }
                None => {
                    let b = BossBar::create(&title, "GREEN");
                    b.set_progress(progress);
                    b.add_player(&p);
                    made.push((p.get_unique_id().to_ascii_lowercase(), b));
                }
            }
        }
    }
    if !made.is_empty() {
        let mut ps = players();
        for (u, b) in made {
            if let Some(jp) = ps.get_mut(&u) {
                jp.bossbar = Some((b, now + s.bossbar_timer as i64 * 1000));
            }
        }
    }
}

// ---- Joining and leaving jobs ----

pub enum JoinError {
    Unknown,
    Already,
    MaxJobs,
    Full,
    NoPerm,
    Rejoin(i64),
}

pub fn workers_of(job: &str) -> usize {
    players().values().filter(|p| p.in_job(job)).count()
}

/// `/jobs join`: the job added (its archived level back with the configured loss).
pub fn join_job(uuid: &str, job_name: &str, force: bool) -> Result<String, JoinError> {
    let s = settings::get();
    let Some(job) = s.job(job_name).cloned() else { return Err(JoinError::Unknown) };
    let workers = workers_of(&job.name);
    let mut out = Out::default();
    let r = {
        let mut ps = players();
        let Some(jp) = ps.get_mut(uuid) else { return Err(JoinError::Unknown) };
        if jp.in_job(&job.name) {
            return Err(JoinError::Already);
        }
        if !force {
            if !jp.join_perm.contains(&job.name.to_ascii_lowercase()) {
                return Err(JoinError::NoPerm);
            }
            if !job.ignore_max && jp.max_jobs > 0 && jp.jobs.len() as i32 >= jp.max_jobs {
                return Err(JoinError::MaxJobs);
            }
            if job.slots > 0 && workers as i32 >= job.slots {
                return Err(JoinError::Full);
            }
            if let Some((_, _, _, left)) = jp.archived.iter().find(|a| a.0.eq_ignore_ascii_case(&job.name))
                && job.rejoin_cooldown > 0
            {
                let wait = left + job.rejoin_cooldown * 1000 - crate::now_ms();
                if wait > 0 {
                    return Err(JoinError::Rejoin(wait / 1000));
                }
            }
        }
        let (level, exp) = match jp.archived.iter().position(|a| a.0.eq_ignore_ascii_case(&job.name)) {
            Some(i) => {
                let (_, lv, xp, _) = jp.archived.remove(i);
                let max = jp.max_level(&s, &job.name);
                let kept = if s.fix_at_max && max > 0 && lv >= max { lv } else { ((lv as f64) * (1.0 - s.level_loss / 100.0)).round() as i32 };
                (kept.max(1), if kept == lv { xp } else { 0.0 })
            }
            None => (1, 0.0),
        };
        jp.jobs.push(Prog { job: job.name.clone(), level, exp });
        jp.dirty = true;
        let name = jp.name.clone();
        let p = jp.player.clone();
        for cmd in &job.cmd_on_join {
            let line = fill_cmd(cmd, &name, &job.name, level, level);
            let pl = p.clone();
            out.push(move || run_command(&line, &pl));
        }
        Ok(job.name.clone())
    };
    out.run();
    r
}

/// `/jobs leave`: the job archived (for a later rejoin).
pub fn leave_job(uuid: &str, job_name: &str) -> Option<String> {
    let s = settings::get();
    let job = s.job(job_name)?.clone();
    let mut out = Out::default();
    let r = {
        let mut ps = players();
        let jp = ps.get_mut(uuid)?;
        let i = jp.jobs.iter().position(|p| p.job.eq_ignore_ascii_case(&job.name))?;
        let prog = jp.jobs.remove(i);
        jp.archived.retain(|a| !a.0.eq_ignore_ascii_case(&job.name));
        jp.archived.push((prog.job.clone(), prog.level, prog.exp, crate::now_ms()));
        jp.dirty = true;
        let name = jp.name.clone();
        let p = jp.player.clone();
        for cmd in &job.cmd_on_leave {
            let line = fill_cmd(cmd, &name, &job.name, prog.level, prog.level);
            let pl = p.clone();
            out.push(move || run_command(&line, &pl));
        }
        Some(job.name.clone())
    };
    out.run();
    r
}

/// Admin: levels or exp changed (`promote`, `demote`, `grantxp`, `removexp`, `level`).
pub fn admin_change(uuid: &str, job_name: &str, levels: i32, exp: f64, set_level: Option<i32>) -> bool {
    let s = settings::get();
    let mut out = Out::default();
    let ok = {
        let mut ps = players();
        let Some(jp) = ps.get_mut(uuid) else { return false };
        let Some(i) = jp.jobs.iter().position(|p| p.job.eq_ignore_ascii_case(job_name)) else { return false };
        if let Some(l) = set_level {
            jp.jobs[i].level = l.max(1);
            jp.jobs[i].exp = 0.0;
        }
        if levels != 0 {
            let max = jp.max_level(&s, job_name);
            let l = (jp.jobs[i].level + levels).max(1);
            jp.jobs[i].level = if max > 0 { l.min(max) } else { l };
        }
        if exp != 0.0 {
            add_exp(&s, jp, i, exp, &mut out);
        }
        jp.dirty = true;
        true
    };
    out.run();
    ok
}

// ---- Block protection (placed blocks pay nothing when broken) ----

#[derive(Default)]
pub struct Protection {
    pub placed: HashMap<(String, i32, i32, i32), (i64, i64)>,
    pub broken_at: HashMap<(String, i32, i32, i32), i64>,
    pub added: Vec<(String, i32, i32, i32, i64, i64)>,
    pub removed: Vec<(String, i32, i32, i32)>,
}

pub static PROTECT: LazyLock<Mutex<Protection>> = LazyLock::new(|| Mutex::new(Protection::default()));

pub fn protect() -> MutexGuard<'static, Protection> {
    PROTECT.lock().unwrap_or_else(|e| e.into_inner())
}

/// A placed block: protected (for its blocksTimer seconds, or for good). Returns whether placing pays.
pub fn on_place(world: &str, x: i32, y: i32, z: i32, mat: &str) -> bool {
    let s = settings::get();
    let now = crate::now_ms();
    let key = (world.to_owned(), x, y, z);
    let timer = s.block_timers.get(mat).copied();
    let resets = match timer {
        Some(t) if t > 0 => now + t * 1000,
        _ => -1,
    };
    let mut p = protect();
    let recent = p.broken_at.get(&key).is_some_and(|t| now - t < s.break_timer * 1000);
    p.placed.insert(key.clone(), (now, resets));
    p.added.push((key.0, x, y, z, now, resets));
    if p.broken_at.len() > 50_000 {
        p.broken_at.retain(|_, t| now - *t < 60_000);
    }
    !recent
}

/// A broken block: whether breaking it pays (not a protected placed one).
pub fn on_break(world: &str, x: i32, y: i32, z: i32) -> bool {
    let now = crate::now_ms();
    let key = (world.to_owned(), x, y, z);
    let mut p = protect();
    p.broken_at.insert(key.clone(), now);
    match p.placed.remove(&key) {
        Some((_, resets)) => {
            p.removed.push((key.0, x, y, z));
            resets > 0 && resets <= now
        }
        None => true,
    }
}

pub fn load_protection() {
    let rows = crate::db::load_blocks();
    let mut p = protect();
    let now = crate::now_ms();
    for (w, x, y, z, rec, resets) in rows {
        if resets > 0 && resets < now {
            continue;
        }
        p.placed.entry((w, x, y, z)).or_insert((rec, resets));
    }
}

pub fn save_protection() {
    let (added, removed) = {
        let mut p = protect();
        (std::mem::take(&mut p.added), std::mem::take(&mut p.removed))
    };
    crate::db::save_blocks(&added, &removed);
}

// ---- Exploring ----

#[derive(Default)]
pub struct Explore {
    pub chunks: HashMap<(String, i32, i32), Vec<String>>,
    pub dirty: HashSet<(String, i32, i32)>,
}

pub static EXPLORE: LazyLock<Mutex<Explore>> = LazyLock::new(|| Mutex::new(Explore::default()));

pub fn load_explore() {
    let rows = crate::db::load_explore();
    let mut e = EXPLORE.lock().unwrap_or_else(|e| e.into_inner());
    for (w, x, z, names) in rows {
        e.chunks.insert((w, x, z), names.split(';').filter(|n| !n.is_empty()).map(str::to_owned).collect());
    }
}

/// A player in a chunk: their visit number there if this is their first time (Explore pays by it).
pub fn explore(world: &str, cx: i32, cz: i32, name: &str) -> Option<usize> {
    let mut e = EXPLORE.lock().unwrap_or_else(|e| e.into_inner());
    let key = (world.to_owned(), cx, cz);
    let list = e.chunks.entry(key.clone()).or_default();
    if list.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        return None;
    }
    list.push(name.to_owned());
    let n = list.len();
    e.dirty.insert(key);
    Some(n)
}

pub fn save_explore() {
    let rows: Vec<(String, i32, i32, String)> = {
        let mut e = EXPLORE.lock().unwrap_or_else(|e| e.into_inner());
        let dirty: Vec<_> = e.dirty.drain().collect();
        dirty.into_iter().filter_map(|k| e.chunks.get(&k).map(|v| (k.0.clone(), k.1, k.2, v.join(";")))).collect()
    };
    crate::db::save_explore(&rows);
}
