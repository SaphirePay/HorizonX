//! schedulers.yml: CRON times (`<minute> <hour> <day> <month> <day of week>`) that start dark auctions, read as
//! cron4j (the Java plugin's scheduler) reads them: every field must match, `*`, lists, ranges, steps (`*/5`,
//! `0/2`, `10-30/5`), month and day names, `L` for a month's last day, several patterns joined by `|`. Times are
//! in `timezone` (`server` = the machine's).

use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
use horizon_kit::yaml;
use serde_yaml::Value as Yaml;

#[derive(Clone, Debug)]
struct Field {
    allowed: Vec<bool>,
    last_day: bool,
}

impl Field {
    fn matches(&self, v: u32) -> bool {
        self.allowed.get(v as usize).copied().unwrap_or(false)
    }
}

#[derive(Clone, Debug)]
pub struct Pattern {
    minute: Field,
    hour: Field,
    day: Field,
    month: Field,
    weekday: Field,
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
const DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

fn value(text: &str, names: &[&str], base: u32) -> Result<u32, String> {
    let t = text.trim().to_ascii_lowercase();
    if let Some(i) = names.iter().position(|n| *n == t) {
        return Ok(i as u32 + base);
    }
    t.parse::<u32>().map_err(|_| format!("invalid value \"{text}\""))
}

fn field(text: &str, min: u32, max: u32, names: &[&str], base: u32, days: bool) -> Result<Field, String> {
    let mut allowed = vec![false; max as usize + 2];
    let mut last_day = false;
    for part in text.split(',') {
        let part = part.trim();
        if days && part.eq_ignore_ascii_case("l") {
            last_day = true;
            continue;
        }
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.trim().parse::<u32>().map_err(|_| format!("invalid step in \"{part}\""))?.max(1)),
            None => (part, 1),
        };
        let (from, to) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (value(a, names, base)?, value(b, names, base)?)
        } else {
            let v = value(range, names, base)?;
            (v, if part.contains('/') { max } else { v })
        };
        if from < min || to > max + u32::from(names == DAYS) || from > to {
            return Err(format!("\"{part}\" is out of range {min}-{max}"));
        }
        let mut v = from;
        while v <= to {
            // cron4j takes 7 for Sunday too.
            let slot = if names == DAYS && v == 7 { 0 } else { v };
            allowed[slot as usize] = true;
            v += step;
        }
    }
    Ok(Field { allowed, last_day })
}

impl Pattern {
    pub fn parse(text: &str) -> Result<Pattern, String> {
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(format!("\"{text}\" needs 5 fields, it has {}", parts.len()));
        }
        Ok(Pattern {
            minute: field(parts[0], 0, 59, &[], 0, false)?,
            hour: field(parts[1], 0, 23, &[], 0, false)?,
            day: field(parts[2], 1, 31, &[], 0, true)?,
            month: field(parts[3], 1, 12, &MONTHS, 1, false)?,
            weekday: field(parts[4], 0, 6, &DAYS, 0, false)?,
        })
    }

    fn day_matches<T: TimeZone>(&self, at: &DateTime<T>) -> bool {
        let last = {
            let next = at.clone() + Duration::days(1);
            next.month() != at.month()
        };
        (self.day.matches(at.day()) || (self.day.last_day && last)) && self.month.matches(at.month()) && self.weekday.matches(at.weekday().num_days_from_sunday())
    }

    fn matches<T: TimeZone>(&self, at: &DateTime<T>) -> bool {
        self.minute.matches(at.minute()) && self.hour.matches(at.hour()) && self.day_matches(at)
    }

    /// How many minutes from a local time it is safe to skip: a whole day (less an hour, for daylight saving) from
    /// a midnight of a day that can't match, the rest of an hour that can't, else one.
    fn skip<T: TimeZone>(&self, at: &DateTime<T>) -> i64 {
        if at.hour() == 0 && at.minute() == 0 && !self.day_matches(at) {
            return 23 * 60;
        }
        if at.minute() == 0 && (!self.hour.matches(at.hour()) || !self.day_matches(at)) {
            return 60;
        }
        1
    }
}

/// One `times` entry: patterns joined by `|`.
#[derive(Clone, Debug)]
pub struct Cron(Vec<Pattern>);

impl Cron {
    pub fn parse(text: &str) -> Result<Cron, String> {
        text.split('|').map(Pattern::parse).collect::<Result<Vec<_>, _>>().map(Cron)
    }
    pub fn matches<T: TimeZone>(&self, at: &DateTime<T>) -> bool {
        self.0.iter().any(|p| p.matches(at))
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Zone {
    Server,
    Named(chrono_tz::Tz),
}

impl Zone {
    pub fn of(name: &str) -> Zone {
        if name.eq_ignore_ascii_case("server") {
            return Zone::Server;
        }
        name.parse::<chrono_tz::Tz>().map(Zone::Named).unwrap_or(Zone::Server)
    }
}

/// Whether the cron runs in the minute starting at `at` (UTC).
pub fn runs_at(cron: &Cron, zone: Zone, at: DateTime<Utc>) -> bool {
    match zone {
        Zone::Server => cron.matches(&at.with_timezone(&chrono::Local)),
        Zone::Named(tz) => cron.matches(&at.with_timezone(&tz)),
    }
}

/// The next minute (as milliseconds since the epoch) the cron runs in after `now`, within a year: cron4j's
/// Predictor.nextMatchingTime.
pub fn next_after(cron: &Cron, zone: Zone, now: DateTime<Utc>) -> Option<i64> {
    let start = now.with_second(0)?.with_nanosecond(0)? + Duration::minutes(1);
    let end = start + Duration::days(366);
    cron.0
        .iter()
        .filter_map(|p| {
            let mut at = start;
            while at < end {
                let (hit, skip) = match zone {
                    Zone::Server => {
                        let local = at.with_timezone(&chrono::Local);
                        (p.matches(&local), p.skip(&local))
                    }
                    Zone::Named(tz) => {
                        let local = at.with_timezone(&tz);
                        (p.matches(&local), p.skip(&local))
                    }
                };
                if hit {
                    return Some(at.timestamp_millis());
                }
                at += Duration::minutes(skip);
            }
            None
        })
        .min()
}

/// One scheduler of schedulers.yml.
#[derive(Clone, Debug)]
pub struct Scheduler {
    pub name: String,
    pub times: Vec<(String, Cron)>,
    pub random: bool,
    pub auctions: Vec<String>,
}

pub struct Schedules {
    pub zone: Zone,
    pub all: Vec<Scheduler>,
}

/// Every scheduler (a section with `times`); a pattern that doesn't parse is reported and left out.
pub fn load(doc: &Yaml) -> Schedules {
    let zone = Zone::of(&yaml::string(doc, "timezone").unwrap_or_else(|| "server".into()));
    let mut all = Vec::new();
    for name in yaml::keys(doc, "") {
        if !yaml::is_section(doc, &name) {
            continue;
        }
        let mut times = Vec::new();
        for t in yaml::list(doc, &format!("{name}.times")) {
            match Cron::parse(&t) {
                Ok(c) => times.push((t, c)),
                Err(e) => crate::texts::console(&format!("&#7700FFInvalid CRON format: &#D200FF{t} &#7700FF({e})")),
            }
        }
        all.push(Scheduler { name: name.clone(), times, random: yaml::boolean(doc, &format!("{name}.random"), false), auctions: yaml::list(doc, &format!("{name}.auctions")) });
    }
    Schedules { zone, all }
}

impl Schedules {
    /// The next run of a scheduler, `None` when it never runs.
    pub fn next_of(&self, scheduler: &str, now: DateTime<Utc>) -> Option<i64> {
        let s = self.all.iter().find(|s| s.name == scheduler)?;
        s.times.iter().filter_map(|(_, c)| next_after(c, self.zone, now)).min()
    }
    /// The next run that starts this auction for sure (random schedulers left out, as the Java plugin does).
    pub fn next_for_auction(&self, auction: &str, now: DateTime<Utc>) -> Option<i64> {
        self.all.iter().filter(|s| !s.random && s.auctions.iter().any(|a| a == auction)).filter_map(|s| self.next_of(&s.name, now)).min()
    }
    /// The auctions to start in this minute.
    pub fn due(&self, at: DateTime<Utc>, pick: &mut dyn FnMut(usize) -> usize) -> Vec<String> {
        let mut out = Vec::new();
        for s in &self.all {
            if s.auctions.is_empty() || !s.times.iter().any(|(_, c)| runs_at(c, self.zone, at)) {
                continue;
            }
            if s.random {
                out.push(s.auctions[pick(s.auctions.len())].clone());
            } else {
                out.extend(s.auctions.iter().cloned());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_fields() {
        let c = Cron::parse("0 18 * * *").unwrap();
        let at = Utc.with_ymd_and_hms(2026, 11, 5, 18, 0, 0).unwrap();
        assert!(c.matches(&at));
        assert!(!c.matches(&(at + Duration::minutes(1))));
        let every = Cron::parse("*/15 9-17 * * mon-fri").unwrap();
        let thu = Utc.with_ymd_and_hms(2026, 11, 5, 9, 45, 0).unwrap();
        assert!(every.matches(&thu));
        assert!(!every.matches(&(thu + Duration::days(2))));
        let last = Cron::parse("30 12 L * *").unwrap();
        assert!(last.matches(&Utc.with_ymd_and_hms(2026, 2, 28, 12, 30, 0).unwrap()));
        assert!(Cron::parse("61 * * * *").is_err());
        assert!(Cron::parse("0 0 * *").is_err());
        let next = next_after(&c, Zone::Named(chrono_tz::UTC), Utc.with_ymd_and_hms(2026, 11, 5, 17, 59, 30).unwrap()).unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 11, 5, 18, 0, 0).unwrap().timestamp_millis());
    }
}
