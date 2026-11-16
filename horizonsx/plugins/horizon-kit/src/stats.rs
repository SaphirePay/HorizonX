//! A profiler a plugin's `/x profiler` can show: named timers (runs, average, max) and a packet count, kept only
//! while it runs. `kit::stats::time("tick", started.elapsed())` anywhere; `report()` for the lines.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Default, Clone, Copy)]
struct Timer {
    count: u64,
    total: Duration,
    max: Duration,
}

#[derive(Default)]
struct Stats {
    running: bool,
    timers: BTreeMap<&'static str, Timer>,
    packets: u64,
}

static STATS: Mutex<Option<Stats>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Stats) -> R) -> R {
    f(STATS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Stats::default))
}

pub fn running() -> bool {
    with(|s| s.running)
}

/// Started, or stopped: `true` when it changed.
pub fn set_running(on: bool) -> bool {
    with(|s| std::mem::replace(&mut s.running, on) != on)
}

pub fn reset() {
    with(|s| {
        s.timers.clear();
        s.packets = 0;
    });
}

pub fn time(name: &'static str, d: Duration) {
    with(|s| {
        if s.running {
            let t = s.timers.entry(name).or_default();
            t.count += 1;
            t.total += d;
            t.max = t.max.max(d);
        }
    });
}

/// `f` timed under `name` (only counted while the profiler runs).
pub fn timed<R>(name: &'static str, f: impl FnOnce() -> R) -> R {
    let started = std::time::Instant::now();
    let out = f();
    time(name, started.elapsed());
    out
}

pub fn count(packets: usize) {
    with(|s| {
        if s.running {
            s.packets += packets as u64;
        }
    });
}

/// One line per timer (`&`-coloured), then the packets.
pub fn report() -> Vec<String> {
    with(|s| {
        let mut lines: Vec<String> = s
            .timers
            .iter()
            .map(|(name, t)| {
                let avg = if t.count == 0 { 0.0 } else { t.total.as_secs_f64() * 1000.0 / t.count as f64 };
                format!(" &8• &b{name}&7: {} runs, avg &b{avg:.3} ms&7, max &b{:.3} ms", t.count, t.max.as_secs_f64() * 1000.0)
            })
            .collect();
        lines.push(format!(" &8• &bpackets&7: {}", s.packets));
        lines
    })
}
