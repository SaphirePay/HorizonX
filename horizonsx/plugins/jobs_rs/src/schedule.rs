//! schedule.yml boosts: a time-of-day window (and days) during which jobs pay more, announced at start and end.

use crate::settings::{Schedule, Settings};
use std::sync::Mutex;

fn now_parts() -> (u32, usize) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64 + local_offset();
    let day = (secs / 86400 + 4).rem_euclid(7) as usize; // 1970-01-01 was a Thursday: 0 = Sunday
    ((secs.rem_euclid(86400)) as u32, day)
}

/// The server's offset from UTC in seconds (TZ_OFFSET_SECONDS, else UTC).
fn local_offset() -> i64 {
    std::env::var("TZ_OFFSET_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
}

const DAYS: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];

fn running(b: &Schedule, t: u32, day: usize) -> bool {
    if !b.enabled {
        return false;
    }
    let day_ok = b.days.is_empty() || b.days.iter().any(|d| d.eq_ignore_ascii_case("all") || d.to_ascii_lowercase().starts_with(&DAYS[day][..3]));
    if !day_ok {
        return false;
    }
    if b.from <= b.until { t >= b.from && t < b.until } else { t >= b.from || t < b.until }
}

pub fn active(s: &Settings) -> Vec<&Schedule> {
    let (t, day) = now_parts();
    s.schedule.iter().filter(|b| running(b, t, day)).collect()
}

static WAS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Announces boosts that started or stopped (a timer, every few seconds).
pub fn tick() {
    let s = crate::settings::get();
    let now: Vec<String> = active(&s).iter().map(|b| b.name.clone()).collect();
    let mut was = WAS.lock().unwrap_or_else(|e| e.into_inner());
    for b in &s.schedule {
        let on = now.contains(&b.name);
        let before = was.contains(&b.name);
        let lines = if on && !before { &b.start_msg } else if !on && before { &b.stop_msg } else { continue };
        for l in lines {
            horizon_bukkit::broadcast_message(&horizon_kit::text::colorize(l));
        }
    }
    *was = now;
}
