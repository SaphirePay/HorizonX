//! `DescParseTickFormat`: game ticks to a time of day, and back.
use crate::prelude::*;
use regex::Regex;
use std::sync::LazyLock;

const TICKS_AT_MIDNIGHT: i64 = 18_000;
const TICKS_PER_DAY: i64 = 24_000;
const TICKS_PER_HOUR: i64 = 1_000;

static TICKS: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[0-9]+ti?c?k?s?$").expect("regex"));
static TWENTY_FOUR: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[0-9]{2}[^0-9]?[0-9]{2}$").expect("regex"));
static TWELVE: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[0-9]{1,2}([^0-9]?[0-9]{2})?(pm|am)$").expect("regex"));

/// `parse`: `None` is its `NumberFormatException`.
pub fn parse(desc: &str) -> Option<i64> {
    let desc: String = desc.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == ':').collect();
    parse_ticks(&desc).or_else(|| parse24(&desc)).or_else(|| parse12(&desc)).or_else(|| parse_alias(&desc))
}

fn digits(desc: &str) -> String {
    desc.chars().filter(char::is_ascii_digit).collect()
}

fn parse_ticks(desc: &str) -> Option<i64> {
    TICKS.is_match(desc).then(|| digits(desc).parse::<i64>().ok()).flatten().map(|ticks| ticks % TICKS_PER_DAY)
}

fn parse24(desc: &str) -> Option<i64> {
    if !TWENTY_FOUR.is_match(desc) {
        return None;
    }
    let digits = digits(desc);
    Some(hours_minutes_to_ticks(digits[..2].parse().ok()?, digits[2..4].parse().ok()?))
}

fn parse12(desc: &str) -> Option<i64> {
    if !TWELVE.is_match(desc) {
        return None;
    }
    let time = digits(desc);
    let (mut hours, minutes): (i64, i64) = match time.len() {
        4 => (time[..2].parse().ok()?, time[2..4].parse().ok()?),
        3 => (time[..1].parse().ok()?, time[1..3].parse().ok()?),
        1 | 2 => (time.parse().ok()?, 0),
        _ => return None,
    };
    if desc.ends_with("pm") && hours != 12 {
        hours += 12;
    }
    if desc.ends_with("am") && hours == 12 {
        hours -= 12;
    }
    Some(hours_minutes_to_ticks(hours, minutes))
}

fn hours_minutes_to_ticks(hours: i64, minutes: i64) -> i64 {
    let whole = TICKS_AT_MIDNIGHT + hours * TICKS_PER_HOUR;
    ((whole as f64 + minutes as f64 / 60.0 * TICKS_PER_HOUR as f64) as i64) % TICKS_PER_DAY
}

fn parse_alias(desc: &str) -> Option<i64> {
    Some(match desc {
        "sunrise" | "dawn" => 23_000,
        "daystart" | "day" => 0,
        "morning" => 1_000,
        "midday" | "noon" => 6_000,
        "afternoon" => 9_000,
        "sunset" | "dusk" | "sundown" | "nightfall" => 12_000,
        "nightstart" | "night" => 14_000,
        "midnight" => 18_000,
        _ => return None,
    })
}

/// `meansReset`.
pub fn means_reset(desc: &str) -> bool {
    matches!(desc, "reset" | "normal" | "default")
}

/// `ticksToDate`, as the hour and minute it comes to, with Java's added second.
fn clock(ticks: i64) -> (i64, i64) {
    let ticks = ticks - TICKS_AT_MIDNIGHT + TICKS_PER_DAY;
    let mut left = ticks.rem_euclid(TICKS_PER_DAY);
    let hours = left / TICKS_PER_HOUR;
    left -= hours * TICKS_PER_HOUR;
    let per_minute = 1000.0 / 60.0;
    let minutes = (left as f64 / per_minute).floor() as i64;
    let seconds = ((left as f64 - minutes as f64 * per_minute) / (per_minute / 60.0)).floor() as i64 + 1;
    let total = (hours * 3600 + minutes * 60 + seconds) % 86_400;
    (total / 3600, total / 60 % 60)
}

/// `format24`.
pub fn format24(ticks: i64) -> String {
    let (hour, minute) = clock(ticks);
    format!("{hour:02}:{minute:02}")
}

/// `format12`.
pub fn format12(ticks: i64) -> String {
    let (hour, minute) = clock(ticks);
    let shown = if hour % 12 == 0 { 12 } else { hour % 12 };
    format!("{shown}:{minute:02} {}", if hour < 12 { "AM" } else { "PM" })
}

/// `formatTicks`.
pub fn format_ticks(ticks: i64) -> String {
    format!("{} ticks", ticks % TICKS_PER_DAY)
}

/// `format`.
pub fn format(ticks: i64) -> String {
    tl_literal("timeFormat", crate::args![format24(ticks), format12(ticks), format_ticks(ticks)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_day_starts_at_six() {
        assert_eq!(format24(0), "06:00");
        assert_eq!(format24(6000), "12:00");
        assert_eq!(format24(18000), "00:00");
        assert_eq!(format12(0), "6:00 AM");
        assert_eq!(format12(18000), "12:00 AM");
        assert_eq!(format12(13000), "7:00 PM");
    }

    #[test]
    fn every_way_of_saying_a_time() {
        assert_eq!(parse("1000t"), Some(1000));
        assert_eq!(parse("25000ticks"), Some(1000));
        assert_eq!(parse("12:30"), Some(6500));
        assert_eq!(parse("1230"), Some(6500));
        assert_eq!(parse("6am"), Some(0));
        assert_eq!(parse("12am"), Some(18000));
        assert_eq!(parse("1:30pm"), Some(7500));
        assert_eq!(parse("noon"), Some(6000));
        assert_eq!(parse("nope"), None);
    }
}
