//! HorizonsX's server health, as Paper and Folia report it: `/tps` (Folia's `CommandServerHealth`: the server's
//! TPS and tick times over 5 s, 15 s, 1 min, 5 min and 15 min, each region's last tick, the chunk system's
//! loads and generations a second), `/mspt` (Paper's tick times from the last 5 s, 10 s and 1 min) and
//! `/profiler [seconds]` (Folia's region profiler: the server's `horizon-report`, a time tree of every phase
//! on the server's and the region threads, written to `reports/`).
use super::Game;
use horizon_client::text::Text;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Ticks kept: 15 minutes at 20 a second.
const KEPT: usize = 15 * 60 * 20;

struct Ticks {
    /// When each tick ended and what it took (ms), oldest first.
    ticks: VecDeque<(Instant, f32)>,
    /// The chunk system's counters once a second (when, read from saves, generated), 16 s of them.
    chunks: VecDeque<(Instant, u64, u64)>,
}

static TICKS: Mutex<Ticks> = Mutex::new(Ticks { ticks: VecDeque::new(), chunks: VecDeque::new() });

/// One region's last tick, for `/tps` (Folia lists each region's).
#[derive(Clone, Debug, Default)]
pub(super) struct RegionStat {
    pub level: String,
    /// The block at its first section's corner.
    pub at: (i32, i32),
    pub chunks: usize,
    pub players: usize,
    pub ms: f32,
    /// Still ticking past the server's tick (lag isolation).
    pub late: bool,
}

static REGIONS: Mutex<Vec<RegionStat>> = Mutex::new(Vec::new());

/// A profile asked for by `/profiler`: its seconds (0: none), for the server's loop to start.
static PROFILE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The server's loop, after each tick: what it took.
pub(super) fn record_tick(took: Duration) {
    let now = Instant::now();
    let mut t = lock(&TICKS);
    if t.ticks.len() == KEPT {
        t.ticks.pop_front();
    }
    t.ticks.push_back((now, took.as_secs_f32() * 1000.0));
    if t.chunks.back().is_none_or(|c| now.duration_since(c.0) >= Duration::from_secs(1)) {
        let (loaded, generated) = horizon_client::streaming::Streaming::chunk_counts();
        if t.chunks.len() == 16 {
            t.chunks.pop_front();
        }
        t.chunks.push_back((now, loaded, generated));
    }
}

/// A level's regions this tick (`tick_world_regions`), replacing that level's last ones.
pub(super) fn record_regions(level: &str, stats: Vec<RegionStat>) {
    let mut r = lock(&REGIONS);
    r.retain(|s| s.level != level);
    r.extend(stats);
}

/// `/profiler`'s request, taken by the server's loop.
pub(super) fn take_profile_request() -> Option<u32> {
    match PROFILE.swap(0, std::sync::atomic::Ordering::Relaxed) {
        0 => None,
        s => Some(s),
    }
}

/// TPS (at most 20 a second, as Paper counts it) and the tick times (average, max) of the ticks that ended
/// in the last `window`.
fn window(t: &Ticks, now: Instant, window: Duration) -> Option<(f64, f64, f64, f64)> {
    let since = now.checked_sub(window)?;
    let mut n = 0usize;
    let (mut sum, mut min, mut max) = (0.0f64, f64::MAX, 0.0f64);
    let mut first = None;
    for &(at, ms) in t.ticks.iter().rev() {
        if at < since {
            break;
        }
        first = Some(at);
        n += 1;
        let ms = f64::from(ms);
        sum += ms;
        min = min.min(ms);
        max = max.max(ms);
    }
    let first = first?;
    // Over what the ticks really cover (a server up for 3 s has 3 s of a 1 min window).
    let span = now.duration_since(first).as_secs_f64().max(0.05) + 0.05;
    let tps = (n as f64 / span.min(window.as_secs_f64())).min(20.0);
    Some((tps, sum / n as f64, min, max))
}

fn color_tps(tps: f64) -> u32 {
    if tps >= 19.5 {
        horizon_client::text::GREEN
    } else if tps >= 15.0 {
        horizon_client::text::YELLOW
    } else {
        horizon_client::text::RED
    }
}

impl Game<'_> {
    /// `/tps`: Folia's server health report.
    pub(super) fn cmd_tps(&mut self) -> std::result::Result<i32, horizon_client::command::CommandError> {
        let now = Instant::now();
        let (lines, tps_now) = {
            let t = lock(&TICKS);
            let mut lines = Vec::new();
            let windows = [("5s", 5u64), ("15s", 15), ("1m", 60), ("5m", 300), ("15m", 900)];
            let stats: Vec<_> = windows.iter().map(|&(_, s)| window(&t, now, Duration::from_secs(s))).collect();
            let tps: Vec<String> = stats.iter().map(|s| s.map_or("-".into(), |s| format!("{:.2}", s.0))).collect();
            let mspt: Vec<String> = stats.iter().map(|s| s.map_or("-".into(), |s| format!("{:.1}", s.1))).collect();
            let names: Vec<&str> = windows.iter().map(|w| w.0).collect();
            lines.push(format!("Server health: TPS ({}): {}", names.join(", "), tps.join(", ")));
            lines.push(format!("MSPT ({}): {}", names.join(", "), mspt.join(", ")));
            // The chunk system's throughput over the last 15 s (Folia's /tps counters).
            if let (Some(a), Some(b)) = (t.chunks.front(), t.chunks.back()) {
                let s = b.0.duration_since(a.0).as_secs_f64();
                if s > 0.5 {
                    lines.push(format!(
                        "Chunks: {:.1} loaded/s, {:.1} generated/s",
                        (b.1 - a.1) as f64 / s,
                        (b.2 - a.2) as f64 / s
                    ));
                }
            }
            (lines, stats[0].map_or(20.0, |s| s.0))
        };
        let color = color_tps(tps_now);
        for (i, line) in lines.into_iter().enumerate() {
            let text = Text::literal(line);
            self.system_message(if i == 0 { text.with_color(color) } else { text });
        }
        let regions = lock(&REGIONS).clone();
        let (count, slowest, average, late) = super::regions::region_report();
        self.system_message(Text::literal(format!(
            "Regions: {count} (last tick slowest {slowest:.1} ms, average {average:.1} ms; {late} late ticks since start)"
        )));
        let mut busiest = regions;
        busiest.sort_by(|a, b| b.ms.total_cmp(&a.ms));
        for r in busiest.iter().take(5) {
            let ms_tps = if r.ms <= 50.0 { 20.0 } else { 1000.0 / f64::from(r.ms) };
            self.system_message(
                Text::literal(format!(
                    "  {} near {}, {}: {:.1} ms ({:.1} TPS), {} chunks, {} players{}",
                    r.level,
                    r.at.0,
                    r.at.1,
                    r.ms,
                    ms_tps,
                    r.chunks,
                    r.players,
                    if r.late { ", running late" } else { "" }
                ))
                .with_color(color_tps(ms_tps)),
            );
        }
        Ok(tps_now.round() as i32)
    }

    /// `/mspt`: Paper's tick times (average/min/max) from the last 5 s, 10 s and 1 min.
    pub(super) fn cmd_mspt(&mut self) -> std::result::Result<i32, horizon_client::command::CommandError> {
        let now = Instant::now();
        let parts: Vec<String> = {
            let t = lock(&TICKS);
            [5u64, 10, 60]
                .iter()
                .map(|&s| {
                    window(&t, now, Duration::from_secs(s))
                        .map_or("-".into(), |w| format!("{:.1}/{:.1}/{:.1}", w.1, w.2, w.3))
                })
                .collect()
        };
        self.system_message(Text::literal("Server tick times (avg/min/max) from last 5s, 10s, 1m:".to_owned()));
        self.system_message(Text::literal(format!("◴ {}", parts.join(", "))));
        Ok(1)
    }

    /// `/profiler [seconds]`: a profile of every phase's time on the server's and the region threads, for
    /// `seconds` (10 unless told), written to `reports/` by the server's loop (`horizon-report`).
    pub(super) fn cmd_profiler(
        &mut self,
        ctx: &horizon_client::command::Context,
    ) -> std::result::Result<i32, horizon_client::command::CommandError> {
        let seconds = match ctx.get("seconds") {
            Some(horizon_client::command::Arg::Int(s)) => (*s).clamp(1, 600) as u32,
            _ => 10,
        };
        if self.options.dedicated.is_none() {
            self.system_message(Text::literal("The profiler runs on a dedicated server".to_owned()));
            return Ok(0);
        }
        PROFILE.store(seconds, std::sync::atomic::Ordering::Relaxed);
        self.system_message(Text::literal(format!(
            "Profiling every region and the server's thread for {seconds} s; the report goes to reports/"
        )));
        Ok(seconds as i32)
    }
}
