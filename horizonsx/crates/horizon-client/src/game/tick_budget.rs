//! HorizonsX: how much of a server tick is left for primed TNT, so a TNT field never takes the tick past 50 ms.
//!
//! The server thread marks each tick's start. TNT (on the server thread or a region's) gets what is left of a
//! 40 ms tick (`tick-target-ms`) after what the tick spent before it and what came after TNT the tick before (the network, block
//! entities), at most `max-tnt-ms-per-tick`. What doesn't fit waits a tick, fuse and place as they were.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The tick the budget aims at, `tick-target-ms` in `spigot.yml` (HorizonsX's; default 40: 10 ms short of 50 for
/// the server's work after the regions land, which comes after TNT is done), read again every few seconds.
fn target() -> Duration {
    static READ_AT: AtomicU64 = AtomicU64::new(0);
    static MS: AtomicU64 = AtomicU64::new(40);
    let now = base().elapsed().as_secs() + 1;
    if now >= READ_AT.load(Ordering::Relaxed) {
        READ_AT.store(now + 5, Ordering::Relaxed);
        let text = std::fs::read_to_string("spigot.yml").unwrap_or_default();
        let ms = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("tick-target-ms:"))
            .and_then(|value| value.trim().parse::<f64>().ok())
            .map_or(40, |ms| ms.clamp(10.0, 50.0) as u64);
        MS.store(ms, Ordering::Relaxed);
    }
    Duration::from_millis(MS.load(Ordering::Relaxed))
}
/// The least TNT gets: a field always burns down, if slowly.
const LEAST: Duration = Duration::from_millis(2);

fn base() -> Instant {
    static BASE: OnceLock<Instant> = OnceLock::new();
    *BASE.get_or_init(Instant::now)
}

fn now_ns() -> u64 {
    base().elapsed().as_nanos() as u64 + 1
}

/// When this tick started (0: no server tick yet: a world with no server loop).
static STARTED: AtomicU64 = AtomicU64::new(0);
/// How far into the tick the last TNT finished (0: none ticked).
static TNT_END: AtomicU64 = AtomicU64::new(0);
/// What the last tick spent after TNT.
static TAIL: AtomicU64 = AtomicU64::new(0);

/// The server tick starts.
pub(super) fn tick_begins() {
    STARTED.store(now_ns(), Ordering::Relaxed);
    TNT_END.store(0, Ordering::Relaxed);
}

/// The server tick's work is done (before the wait for the next tick): what it spent after its TNT is noted. (Taken
/// at the next tick's start instead, it counted the wait too: an idle server's TNT got only the least, 2 ms, and a
/// field of thousands went off at 50 a second for minutes.)
pub(super) fn tick_ends() {
    let started = STARTED.load(Ordering::Relaxed);
    let tnt_end = TNT_END.load(Ordering::Relaxed);
    if started != 0 && tnt_end != 0 {
        let total = now_ns().saturating_sub(started);
        TAIL.store(total.saturating_sub(tnt_end), Ordering::Relaxed);
    }
}

/// What this tick has left for TNT.
pub(super) fn left_for_tnt() -> Duration {
    let started = STARTED.load(Ordering::Relaxed);
    if started == 0 {
        return Duration::MAX;
    }
    let spent = Duration::from_nanos(now_ns().saturating_sub(started));
    // A tick that started long ago is no server tick (a world ticked another way): no say.
    if spent > Duration::from_secs(2) {
        return Duration::MAX;
    }
    let tail = Duration::from_nanos(TAIL.load(Ordering::Relaxed)).min(Duration::from_millis(40));
    target().saturating_sub(spent).saturating_sub(tail).max(LEAST)
}

/// TNT has ticked for this tick (here: the server thread or one region).
pub(super) fn tnt_done() {
    let started = STARTED.load(Ordering::Relaxed);
    if started != 0 {
        TNT_END.fetch_max(now_ns().saturating_sub(started), Ordering::Relaxed);
    }
}
