//! Times as placeholders.yml's `Config.TimeFormat` writes them.

use crate::settings;

/// Milliseconds as `1 Day(s), 2 Hour(s) and 5 Secs` (`Infinite` for -1, `ZeroAll` for 0).
pub fn duration(ms: i64) -> String {
    let s = settings();
    let p = &s.placeholders;
    if ms < 0 {
        return p.str("Config.TimeFormat.Infinite", "Inf");
    }
    if ms == 0 {
        return p.str("Config.TimeFormat.ZeroAll", "None");
    }
    let mut left = ms;
    let days = left / 86_400_000;
    left -= days * 86_400_000;
    let hours = left / 3_600_000;
    left -= hours * 3_600_000;
    let minutes = left / 60_000;
    left -= minutes * 60_000;
    let seconds = left / 1000;
    let mut parts = Vec::new();
    for (n, key, def) in [(days, "Days", "{0} Day(s)"), (hours, "Hours", "{0} Hour(s)"), (minutes, "Minutes", "{0} Min(s)"), (seconds, "Seconds", "{0} Secs")] {
        if n > 0 {
            parts.push(p.str(&format!("Config.TimeFormat.{key}"), def).replace("{0}", &n.to_string()));
        }
    }
    if parts.is_empty() {
        return p.str("Config.TimeFormat.ZeroAll", "None");
    }
    let middle = p.str("Config.TimeFormat.Separator.Middle", ", ");
    let last = p.str("Config.TimeFormat.Separator.Last", " and ");
    let mut out = String::new();
    for (i, part) in parts.iter().enumerate() {
        out.push_str(part);
        let after = parts.len() - 1 - i;
        if after >= 2 {
            out.push_str(&middle);
        } else if after == 1 {
            out.push_str(&last);
        }
    }
    out
}
