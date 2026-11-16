//! Durations as plugins write them in configs and commands, and as they print them.
//!
//! - [`parse`]: `10m`, `2h30m`, `1d 2h`, `3 minutes`, `30 sec`, `2hrs`, `1w`, `500ms`, bare `90` (seconds) ->
//!   milliseconds. `perm`/`permanent`/`forever` -> `None` with [`Parsed::Forever`].
//! - [`ticks`]: the same in game ticks (20 a second).
//! - [`format`]: `1d 2h 3m 4s` (largest units first, zero parts left out), [`format_short`]: `1d2h`, and
//!   [`format_clock`]: `01:02:03`.

/// What [`parse_full`] read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parsed {
    Millis(u64),
    Forever,
}

fn unit_ms(unit: &str) -> Option<u64> {
    Some(match unit.trim().to_ascii_lowercase().as_str() {
        "ms" | "milli" | "millis" | "millisecond" | "milliseconds" => 1,
        "" | "s" | "sec" | "secs" | "second" | "seconds" => 1000,
        "m" | "min" | "mins" | "minute" | "minutes" => 60_000,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000,
        "d" | "day" | "days" => 86_400_000,
        "w" | "wk" | "wks" | "week" | "weeks" => 604_800_000,
        "mo" | "mon" | "month" | "months" => 2_592_000_000,
        "y" | "yr" | "yrs" | "year" | "years" => 31_536_000_000,
        _ => return None,
    })
}

/// A duration, or forever (`perm`, `permanent`, `forever`, `-1`).
pub fn parse_full(text: &str) -> Option<Parsed> {
    let t = text.trim().to_ascii_lowercase();
    if matches!(t.as_str(), "perm" | "permanent" | "forever" | "-1" | "inf" | "infinite") {
        return Some(Parsed::Forever);
    }
    let mut total: u64 = 0;
    let mut any = false;
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
            i += 1;
        }
        if start == i {
            return None;
        }
        let n: f64 = chars[start..i].iter().collect::<String>().parse().ok()?;
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        let ustart = i;
        while i < chars.len() && chars[i].is_ascii_alphabetic() {
            i += 1;
        }
        let unit: String = chars[ustart..i].iter().collect();
        let ms = unit_ms(&unit)?;
        total = total.saturating_add((n * ms as f64).round() as u64);
        any = true;
    }
    any.then_some(Parsed::Millis(total))
}

/// Milliseconds (`None` for text that isn't a duration, and for forever).
pub fn parse(text: &str) -> Option<u64> {
    match parse_full(text)? {
        Parsed::Millis(ms) => Some(ms),
        Parsed::Forever => None,
    }
}

/// Game ticks (20 a second).
pub fn ticks(text: &str) -> Option<u64> {
    parse(text).map(|ms| ms / 50)
}

const PARTS: [(u64, &str); 5] = [(604_800_000, "w"), (86_400_000, "d"), (3_600_000, "h"), (60_000, "m"), (1000, "s")];

/// `1d 2h 3m 4s` (zero parts left out; `0s` for nothing).
pub fn format(ms: u64) -> String {
    let mut left = ms;
    let mut out: Vec<String> = Vec::new();
    for (size, unit) in PARTS {
        if left >= size {
            out.push(format!("{}{unit}", left / size));
            left %= size;
        }
    }
    if out.is_empty() { "0s".to_owned() } else { out.join(" ") }
}

/// `1d2h` (at most the two largest parts).
pub fn format_short(ms: u64) -> String {
    let mut left = ms;
    let mut out = String::new();
    let mut n = 0;
    for (size, unit) in PARTS {
        if left >= size && n < 2 {
            out.push_str(&format!("{}{unit}", left / size));
            left %= size;
            n += 1;
        }
    }
    if out.is_empty() { "0s".to_owned() } else { out }
}

/// `HH:MM:SS` (hours grow past 99).
pub fn format_clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durations() {
        assert_eq!(parse("10m"), Some(600_000));
        assert_eq!(parse("2h30m"), Some(9_000_000));
        assert_eq!(parse("1d 2h"), Some(93_600_000));
        assert_eq!(parse("3 minutes"), Some(180_000));
        assert_eq!(parse("30 sec"), Some(30_000));
        assert_eq!(parse("2hrs"), Some(7_200_000));
        assert_eq!(parse("90"), Some(90_000));
        assert_eq!(parse("1.5h"), Some(5_400_000));
        assert_eq!(parse("abc"), None);
        assert_eq!(parse_full("perm"), Some(Parsed::Forever));
        assert_eq!(ticks("5s"), Some(100));
        assert_eq!(format(93_784_000), "1d 2h 3m 4s");
        assert_eq!(format_short(93_784_000), "1d2h");
        assert_eq!(format_clock(3_723_000), "01:02:03");
    }
}
