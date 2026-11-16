//! What HorizonX remembers between runs: each server's players over time and every player's record
//! (`host-stats.json` in the server's folder, written once a minute while the app is open and the server
//! runs), this PC's processor and memory use, and the app's own settings (`host-settings.json` in the
//! data folder).
use crate::servers::Status;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const FILE: &str = "host-stats.json";
/// Minute samples kept: the last day.
const DAY: u64 = 24 * 3600;
/// Hourly averages kept: the last 30 days.
const MONTH: u64 = 30 * DAY;
/// Days of playtime kept per player.
const PLAY_DAYS: usize = 14;

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// One hour of a server: players summed over its samples, how many samples, the most at once.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Hour {
    pub hour: u64,
    pub sum: u32,
    pub n: u16,
    pub peak: u16,
}

/// A player's record on one server.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerRecord {
    pub name: String,
    pub first_seen: u64,
    pub last_seen: u64,
    pub sessions: u32,
    /// When the latest session began (Unix seconds).
    #[serde(default)]
    pub session_start: u64,
    /// Seconds online, all together.
    pub playtime: u64,
    /// Seconds online by local day (`2026-11-15`), the last 14 days played.
    #[serde(default)]
    pub days: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    /// (Unix minute, players online), the last day, while the server ran.
    #[serde(default)]
    pub minutes: Vec<(u64, u16)>,
    #[serde(default)]
    pub hours: Vec<Hour>,
    /// By UUID.
    #[serde(default)]
    pub players: BTreeMap<String, PlayerRecord>,
    /// Who was online at the last sample (UUIDs): a UUID not among them starts a session.
    #[serde(default)]
    pub online: Vec<String>,
    #[serde(default)]
    pub last_sample: u64,
}

pub fn load(dir: &Path) -> Stats {
    fs::read_to_string(dir.join(FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(dir: &Path, s: &Stats) {
    let Ok(text) = serde_json::to_string(s) else { return };
    let tmp = dir.join(format!("{FILE}.tmp"));
    if fs::write(&tmp, text).is_ok() {
        let _ = fs::rename(&tmp, dir.join(FILE));
    }
}

fn local_day(t: u64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_opt(t as i64, 0).single().map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

fn local_hour(t: u64) -> u32 {
    use chrono::{TimeZone, Timelike};
    chrono::Local.timestamp_opt(t as i64, 0).single().map(|d| d.hour()).unwrap_or(0)
}

impl Stats {
    /// Takes in one look at the server. `t` is now (Unix seconds).
    pub fn record(&mut self, status: &Status, t: u64) {
        // Time since the last look counts as playtime; a gap (the app was closed) counts as one minute.
        let dt = match t.saturating_sub(self.last_sample) {
            d if self.last_sample > 0 && d <= 300 => d,
            _ => 60,
        };
        self.last_sample = t;
        if !status.running() {
            self.online.clear();
            return;
        }
        let n = status.players.len() as u16;
        let minute = t / 60;
        match self.minutes.last_mut() {
            Some(last) if last.0 == minute => last.1 = last.1.max(n),
            _ => self.minutes.push((minute, n)),
        }
        let oldest = (t.saturating_sub(DAY)) / 60;
        self.minutes.retain(|m| m.0 >= oldest);
        let hour = t / 3600;
        match self.hours.last_mut() {
            Some(h) if h.hour == hour => {
                h.sum += n as u32;
                h.n += 1;
                h.peak = h.peak.max(n);
            }
            _ => self.hours.push(Hour { hour, sum: n as u32, n: 1, peak: n }),
        }
        let oldest = t.saturating_sub(MONTH) / 3600;
        self.hours.retain(|h| h.hour >= oldest);
        let day = local_day(t);
        let mut online = Vec::with_capacity(status.players.len());
        for p in &status.players {
            let key = if p.uuid.is_empty() { p.name.to_lowercase() } else { p.uuid.clone() };
            let rec = self.players.entry(key.clone()).or_insert_with(|| PlayerRecord { first_seen: t, ..Default::default() });
            rec.name = p.name.clone();
            if self.online.contains(&key) {
                rec.playtime += dt;
                *rec.days.entry(day.clone()).or_default() += dt as u32;
                while rec.days.len() > PLAY_DAYS {
                    let first = rec.days.keys().next().cloned().unwrap_or_default();
                    rec.days.remove(&first);
                }
            } else {
                rec.sessions += 1;
                rec.session_start = t;
            }
            rec.last_seen = t;
            online.push(key);
        }
        self.online = online;
    }

    /// Players online over `range`, oldest first: (Unix seconds, players). Minutes for an hour, ten-minute
    /// steps for a day, hours for a week, four-hour steps for a month.
    pub fn series(&self, range: Range, t: u64) -> Vec<(u64, f32)> {
        let from = t.saturating_sub(range.seconds());
        match range {
            Range::Hour => self.minutes.iter().filter(|m| m.0 * 60 >= from).map(|m| (m.0 * 60, m.1 as f32)).collect(),
            Range::Day => bucket(self.minutes.iter().filter(|m| m.0 * 60 >= from).map(|m| (m.0 * 60, m.1 as f32)), 600),
            Range::Week => self.hours.iter().filter(|h| h.hour * 3600 >= from).map(|h| (h.hour * 3600, h.sum as f32 / h.n.max(1) as f32)).collect(),
            Range::Month => bucket(self.hours.iter().filter(|h| h.hour * 3600 >= from).map(|h| (h.hour * 3600, h.sum as f32 / h.n.max(1) as f32)), 4 * 3600),
        }
    }

    /// The numbers over a chart: the most at once (and when), the average, how many different players, the
    /// busiest hour of the day and the average session.
    pub fn summary(&self, range: Range, t: u64) -> Summary {
        let from = t.saturating_sub(range.seconds());
        let samples: Vec<(u64, f32)> = match range {
            Range::Hour | Range::Day => self.minutes.iter().filter(|m| m.0 * 60 >= from).map(|m| (m.0 * 60, m.1 as f32)).collect(),
            _ => self.hours.iter().filter(|h| h.hour * 3600 >= from).map(|h| (h.hour * 3600, h.peak as f32)).collect(),
        };
        let peak = samples.iter().cloned().fold(None, |best: Option<(u64, f32)>, s| match best {
            Some(b) if b.1 >= s.1 => Some(b),
            _ => Some(s),
        });
        let average = if samples.is_empty() { None } else { Some(samples.iter().map(|s| s.1).sum::<f32>() / samples.len() as f32) };
        let seen: Vec<&PlayerRecord> = self.players.values().filter(|p| p.last_seen >= from).collect();
        let (time, sessions) = seen.iter().fold((0u64, 0u64), |(a, b), p| (a + p.playtime, b + p.sessions as u64));
        let mut by_hour = [(0f32, 0u32); 24];
        for (when, v) in &samples {
            let h = local_hour(*when) as usize;
            by_hour[h].0 += v;
            by_hour[h].1 += 1;
        }
        let busiest = by_hour
            .iter()
            .enumerate()
            .filter(|(_, (_, n))| *n > 0)
            .map(|(h, (s, n))| (h as u32, s / *n as f32))
            .filter(|(_, avg)| *avg > 0.)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(h, _)| h);
        Summary { peak, average, different: seen.len(), busiest_hour: busiest, average_session: (sessions > 0).then(|| time / sessions) }
    }
}

/// Averages of `samples` in steps of `step` seconds.
fn bucket(samples: impl Iterator<Item = (u64, f32)>, step: u64) -> Vec<(u64, f32)> {
    let mut out: Vec<(u64, f32, u32)> = vec![];
    for (t, v) in samples {
        let b = t / step * step;
        match out.last_mut() {
            Some(last) if last.0 == b => {
                last.1 += v;
                last.2 += 1;
            }
            _ => out.push((b, v, 1)),
        }
    }
    out.into_iter().map(|(t, s, n)| (t, s / n as f32)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Range {
    Hour,
    #[default]
    Day,
    Week,
    Month,
}

impl Range {
    pub fn seconds(self) -> u64 {
        match self {
            Range::Hour => 3600,
            Range::Day => DAY,
            Range::Week => 7 * DAY,
            Range::Month => MONTH,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    /// (when, players).
    pub peak: Option<(u64, f32)>,
    pub average: Option<f32>,
    pub different: usize,
    /// Local hour of the day (0..23).
    pub busiest_hour: Option<u32>,
    /// Seconds.
    pub average_session: Option<u64>,
}

/// Records one look at a server in its stats file and returns the stats.
pub fn record(dir: &Path, status: &Status) -> Stats {
    let mut s = load(dir);
    s.record(status, now());
    save(dir, &s);
    s
}

/// A tidy top for a chart's scale over `max`, and its step: 25 in steps of 5, 100 in steps of 25, 2000 in
/// steps of 500... (1, 2, 2.5 or 5 times a power of ten, four or five steps).
pub fn nice_scale(max: f32) -> (f32, f32) {
    let max = max.max(4.);
    let raw = max / 5.;
    let pow = 10f32.powf(raw.log10().floor());
    let step = [1., 2., 2.5, 5., 10.].iter().map(|m| m * pow).find(|s| *s >= raw).unwrap_or(10. * pow);
    let top = (max / step).ceil() * step;
    (top, step)
}

// ----- This PC ----------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pc {
    /// Processor use since the last call, 0..100.
    pub cpu: f32,
    pub memory_used: u64,
    pub memory_total: u64,
}

#[cfg(windows)]
pub fn pc() -> Option<Pc> {
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;
    static LAST: Mutex<Option<(u64, u64)>> = Mutex::new(None);
    let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut idle, mut kernel, mut user) = (zero, zero, zero);
    // SAFETY: three valid out-pointers; the call only writes them.
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return None;
    }
    let (idle, busy_total) = (ft(idle), ft(kernel) + ft(user));
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let cpu = match *last {
        Some((li, lt)) if busy_total > lt => {
            let total = (busy_total - lt) as f32;
            (100. * (1. - (idle.saturating_sub(li)) as f32 / total)).clamp(0., 100.)
        }
        _ => 0.,
    };
    *last = Some((idle, busy_total));
    // SAFETY: a zeroed struct with its length set, as the call requires.
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 {
        return None;
    }
    Some(Pc { cpu, memory_used: m.ullTotalPhys - m.ullAvailPhys, memory_total: m.ullTotalPhys })
}

#[cfg(not(windows))]
pub fn pc() -> Option<Pc> {
    use std::sync::Mutex;
    static LAST: Mutex<Option<(u64, u64)>> = Mutex::new(None);
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let nums: Vec<u64> = stat.lines().next()?.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect();
    let idle = nums.get(3)? + nums.get(4).copied().unwrap_or(0);
    let total: u64 = nums.iter().sum();
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let cpu = match *last {
        Some((li, lt)) if total > lt => (100. * (1. - (idle - li) as f32 / (total - lt) as f32)).clamp(0., 100.),
        _ => 0.,
    };
    *last = Some((idle, total));
    let info = fs::read_to_string("/proc/meminfo").ok()?;
    let kb = |key: &str| info.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()).unwrap_or(0) * 1024;
    let total_mem = kb("MemTotal:");
    Some(Pc { cpu, memory_used: total_mem.saturating_sub(kb("MemAvailable:")), memory_total: total_mem })
}

/// A process's own memory (private bytes: what it holds, even while Windows has trimmed its working set, as it
/// does for a server paused with nobody on). Linux: its resident memory.
#[cfg(windows)]
pub fn process_memory(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: the handle is checked, used once and closed; the counters struct is zeroed with its size set.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut c: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ok = GetProcessMemoryInfo(handle, &mut c as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS, c.cb);
        CloseHandle(handle);
        (ok != 0 && c.PrivateUsage > 0).then_some(c.PrivateUsage as u64)
    }
}

#[cfg(not(windows))]
pub fn process_memory(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let kb: u64 = text.lines().find(|l| l.starts_with("VmRSS:"))?.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// Free bytes on the drive that holds `path`.
#[cfg(windows)]
pub fn disk_free(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: a NUL-terminated path and one out-pointer; the others may be null.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
pub fn disk_free(path: &Path) -> Option<u64> {
    let out = std::process::Command::new("df").arg("-k").arg(path).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().nth(1)?.split_whitespace().nth(3)?.parse::<u64>().ok().map(|k| k * 1024)
}

// ----- HorizonX's own settings ------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostSettings {
    /// Start servers that were running when HorizonX was last closed.
    pub restore_servers: bool,
    /// Servers running at the last look (their folders).
    pub were_running: Vec<PathBuf>,
    /// Memory for each new server, in GB.
    pub new_server_memory_gb: u32,
    /// A chat message five minutes before a scheduled restart.
    pub warn_before_restart: bool,
}

impl Default for HostSettings {
    fn default() -> Self {
        Self { restore_servers: true, were_running: vec![], new_server_memory_gb: 4, warn_before_restart: true }
    }
}

fn settings_path() -> PathBuf {
    crate::backend::data_dir().join("host-settings.json")
}

pub fn settings() -> HostSettings {
    fs::read_to_string(settings_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_settings(s: &HostSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "HorizonX";
/// The name before the app was HorizonX (Horizon Host 0.2).
const RUN_NAME_BEFORE: &str = "Horizon Host";

/// Whether Windows opens HorizonX when the user signs in.
#[cfg(windows)]
pub fn opens_with_windows() -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_KEY) else { return false };
    [RUN_NAME, RUN_NAME_BEFORE].iter().any(|n| k.get_value::<String, _>(n).is_ok())
}

#[cfg(windows)]
pub fn set_opens_with_windows(on: bool) -> Result<(), String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY).map_err(|e| e.to_string())?;
    // The entry from before the rename goes either way, so Windows never opens the app twice.
    let _ = key.delete_value(RUN_NAME_BEFORE);
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value(RUN_NAME, &format!("\"{}\"", exe.display())).map_err(|e| e.to_string())
    } else {
        match key.delete_value(RUN_NAME) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(windows))]
pub fn opens_with_windows() -> bool {
    false
}

#[cfg(not(windows))]
pub fn set_opens_with_windows(_on: bool) -> Result<(), String> {
    Err("Only on Windows for now".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::servers::Player;

    fn status(names: &[&str]) -> Status {
        let mut s = Status::stopped();
        s.phase = "running".into();
        s.players = names.iter().map(|n| Player { name: n.to_string(), uuid: format!("u-{n}"), ping: None }).collect();
        s
    }

    #[test]
    fn sessions_and_playtime_count_from_the_second_look() {
        let mut s = Stats::default();
        s.record(&status(&["Alex"]), 1_000_000);
        s.record(&status(&["Alex", "Kai"]), 1_000_060);
        s.record(&status(&["Kai"]), 1_000_120);
        s.record(&status(&["Alex", "Kai"]), 1_000_180);
        let alex = &s.players["u-Alex"];
        assert_eq!((alex.sessions, alex.playtime), (2, 60));
        let kai = &s.players["u-Kai"];
        assert_eq!((kai.sessions, kai.playtime), (1, 120));
        assert_eq!(s.minutes.len(), 4);
    }

    #[test]
    fn a_stopped_server_ends_every_session() {
        let mut s = Stats::default();
        s.record(&status(&["Alex"]), 2_000_000);
        s.record(&Status::stopped(), 2_000_060);
        s.record(&status(&["Alex"]), 2_000_120);
        assert_eq!(s.players["u-Alex"].sessions, 2);
    }

    #[test]
    fn summary_peak_average_and_players() {
        let mut s = Stats::default();
        let t = 3_000_000;
        for (i, n) in [1usize, 3, 2].iter().enumerate() {
            let names: Vec<String> = (0..*n).map(|k| format!("p{k}")).collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            s.record(&status(&refs), t + i as u64 * 60);
        }
        let sum = s.summary(Range::Day, t + 180);
        assert_eq!(sum.peak.map(|p| p.1), Some(3.));
        assert_eq!(sum.average, Some(2.));
        assert_eq!(sum.different, 3);
    }

    #[test]
    fn scale_tops_are_tidy() {
        assert_eq!(nice_scale(22.), (25., 5.));
        assert_eq!(nice_scale(1650.), (2000., 500.));
        assert_eq!(nice_scale(80.), (80., 20.));
        assert_eq!(nice_scale(430.), (500., 100.));
        assert_eq!(nice_scale(0.), (4., 1.));
    }
}
