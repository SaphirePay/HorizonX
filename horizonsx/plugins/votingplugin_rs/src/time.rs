//! Days, weeks and months for totals and resets, in the server's local time (TZ_OFFSET_SECONDS, else UTC).

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn offset_ms() -> i64 {
    std::env::var("TZ_OFFSET_SECONDS").ok().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0) * 1000
}

pub fn day_index(ms: i64) -> i64 {
    (ms + offset_ms()).div_euclid(86_400_000)
}

/// Weeks starting on Monday (1970-01-01 was a Thursday).
pub fn week_index(ms: i64) -> i64 {
    (day_index(ms) + 3).div_euclid(7)
}

/// year * 12 + month (0-11).
pub fn month_index(ms: i64) -> i64 {
    let (y, m, _) = civil(day_index(ms));
    y * 12 + (m - 1)
}

/// Days since the epoch -> (year, month 1-12, day 1-31).
pub fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn ms_until_next_day(ms: i64) -> i64 {
    let local = ms + offset_ms();
    86_400_000 - local.rem_euclid(86_400_000)
}

pub fn ms_until_next_week(ms: i64) -> i64 {
    let days_into = (day_index(ms) + 3).rem_euclid(7);
    (6 - days_into) * 86_400_000 + ms_until_next_day(ms)
}

/// `d MMM yyyy HH:mm` for a time.
pub fn date(ms: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let local = ms + offset_ms();
    let (y, m, d) = civil(local.div_euclid(86_400_000));
    let secs = local.rem_euclid(86_400_000) / 1000;
    format!("{d} {} {y} {:02}:{:02}", MONTHS[(m - 1) as usize], secs / 3600, secs / 60 % 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn calendar() {
        assert_eq!(super::civil(0), (1970, 1, 1));
        assert_eq!(super::civil(19_723), (2024, 1, 1));
        assert_eq!(super::month_index(0), 1970 * 12);
    }
}
