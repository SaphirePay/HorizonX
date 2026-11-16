//! `DateUtil`, on `chrono` in the server's time zone, as `GregorianCalendar` is.
use crate::prelude::*;
use chrono::{DateTime, Datelike, Duration, Local, Months, TimeZone};
use regex::Regex;
use std::sync::LazyLock;

static TIME_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:([0-9]+)\s*y[a-z]*[,\s]*)?(?:([0-9]+)\s*mo[a-z]*[,\s]*)?(?:([0-9]+)\s*w[a-z]*[,\s]*)?(?:([0-9]+)\s*d[a-z]*[,\s]*)?(?:([0-9]+)\s*h[a-z]*[,\s]*)?(?:([0-9]+)\s*m[a-z]*[,\s]*)?(?:([0-9]+)\s*(?:s[a-z]*)?)?").expect("a valid pattern")
});
const MAX_YEARS: i32 = 100_000;

fn local(millis: i64) -> DateTime<Local> {
    Local.timestamp_millis_opt(millis).single().unwrap_or_else(Local::now)
}

fn add_months(date: DateTime<Local>, months: i64) -> DateTime<Local> {
    let count = Months::new(months.unsigned_abs().min(u64::from(u32::MAX)) as u32);
    let moved = if months >= 0 { date.checked_add_months(count) } else { date.checked_sub_months(count) };
    moved.unwrap_or(date)
}

/// `removeTimePattern`.
pub fn remove_time_pattern(input: &str) -> String {
    TIME_PATTERN.replace(input, "").trim().to_owned()
}

/// `parseDateDiff`: the moment the text is from now.
pub fn parse_date_diff(time: &str, future: bool) -> EssResult<i64> {
    parse_date_diff_from(time, future, false)
}

/// `parseDateDiff` with `emptyEpoch`, counted from 1970 rather than now.
pub fn parse_date_diff_from(time: &str, future: bool, empty_epoch: bool) -> EssResult<i64> {
    let mut found = None;
    for captures in TIME_PATTERN.captures_iter(time) {
        if captures.get(0).is_none_or(|whole| whole.as_str().is_empty()) {
            continue;
        }
        found = Some(captures);
        break;
    }
    let captures = found.ok_or_else(|| EssError::tl("illegalDate", &[]))?;
    // `Integer.parseInt`: a number too long for an int is Java's `NumberFormatException`.
    let number = |group: usize| -> EssResult<i64> {
        match captures.get(group) {
            Some(text) if !text.as_str().is_empty() => text.as_str().parse::<i32>().map(i64::from).map_err(|_| EssError::Message(format!("For input string: \"{}\"", text.as_str()))),
            _ => Ok(0),
        }
    };
    let sign = if future { 1 } else { -1 };
    let (years, months, weeks, days, hours, minutes, seconds) = (number(1)?.min(i64::from(MAX_YEARS)), number(2)?, number(3)?, number(4)?, number(5)?, number(6)?, number(7)?);
    let mut date = if empty_epoch { local(0) } else { Local::now() };
    date = add_months(date, years * 12 * sign);
    date = add_months(date, months * sign);
    let span = Duration::try_weeks(weeks).unwrap_or_default() + Duration::try_days(days).unwrap_or_default() + Duration::try_hours(hours).unwrap_or_default() + Duration::try_minutes(minutes).unwrap_or_default() + Duration::try_seconds(seconds).unwrap_or_default();
    date = if future { date.checked_add_signed(span) } else { date.checked_sub_signed(span) }.unwrap_or(date);
    let max = add_months(Local::now(), 120);
    Ok(if date > max { max.timestamp_millis() } else { date.timestamp_millis() })
}

#[derive(Clone, Copy)]
enum Unit {
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
}

fn add_unit(date: DateTime<Local>, unit: Unit, amount: i64) -> DateTime<Local> {
    match unit {
        Unit::Year => add_months(date, amount * 12),
        Unit::Month => add_months(date, amount),
        Unit::Day => date.checked_add_signed(Duration::days(amount)).unwrap_or(date),
        Unit::Hour => date.checked_add_signed(Duration::hours(amount)).unwrap_or(date),
        Unit::Minute => date.checked_add_signed(Duration::minutes(amount)).unwrap_or(date),
        Unit::Second => date.checked_add_signed(Duration::seconds(amount)).unwrap_or(date),
    }
}

/// `dateDiff`: whole units, walked one at a time so months and leap years count as they fall.
fn date_diff(unit: Unit, from: &mut DateTime<Local>, to: DateTime<Local>, future: bool) -> i64 {
    let mut to = to;
    if (from.year() - to.year()).abs() > MAX_YEARS {
        to = to.with_year(from.year() + if future { MAX_YEARS } else { -MAX_YEARS }).unwrap_or(to);
    }
    let mut diff = 0;
    let mut saved = *from;
    while (future && *from <= to) || (!future && *from >= to) {
        saved = *from;
        *from = add_unit(*from, unit, if future { 1 } else { -1 });
        diff += 1;
    }
    *from = saved;
    diff - 1
}

/// `formatDateDiff(long)`.
pub fn format_date_diff(date: i64) -> String {
    format_date_diff_between(Local::now(), local(date))
}

/// `formatDateDiff(Calendar, Calendar)`.
pub fn format_date_diff_between(from: DateTime<Local>, to: DateTime<Local>) -> String {
    if to == from {
        return tl_literal("now", &[]);
    }
    let future = to > from;
    // The 50 ms Java adds, against what code takes to run.
    let to = to + Duration::milliseconds(if future { 50 } else { -50 });
    let names = [("year", "years"), ("month", "months"), ("day", "days"), ("hour", "hours"), ("minute", "minutes"), ("second", "seconds")];
    let units = [Unit::Year, Unit::Month, Unit::Day, Unit::Hour, Unit::Minute, Unit::Second];
    let mut from = from;
    let mut text = String::new();
    let mut accuracy = 0;
    for (unit, (one, many)) in units.into_iter().zip(names) {
        if accuracy > 2 {
            break;
        }
        let diff = date_diff(unit, &mut from, to, future);
        if diff > 0 {
            accuracy += 1;
            text.push_str(&format!(" {diff} {}", tl_literal(if diff > 1 { many } else { one }, &[])));
        }
    }
    if text.is_empty() {
        return tl_literal("now", &[]);
    }
    text.trim().to_owned()
}

/// `formatDate`: `DateFormat.LONG` for both date and time, as an English server has it.
pub fn format_date(date: i64) -> String {
    local(date).format("%B %-d, %Y at %-I:%M:%S %p %Z").to_string()
}
