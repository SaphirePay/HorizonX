//! `java.time.LocalTime` and `LocalDateTime`: wall-clock times, kept as milliseconds (of the day, or
//! since 1970 on the wall clock). The wall clock is UTC's: a converted plugin has no time zone database.
use crate::javaio::JDuration;
use crate::{R, V, exc, native, s};

const DAY: i64 = 86_400_000;
const DAYS: [&str; 7] = ["MONDAY", "TUESDAY", "WEDNESDAY", "THURSDAY", "FRIDAY", "SATURDAY", "SUNDAY"];

pub struct JLocalTime(pub i64);
pub struct JLocalDateTime(pub i64);

/// The wall clock's `now()`: the machine's local time, as Java's `LocalDateTime.now()` reads it.
fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64) + local_offset_ms()
}

/// The local time zone's offset from UTC now, in milliseconds (Windows asks the system; elsewhere UTC).
#[cfg(windows)]
fn local_offset_ms() -> i64 {
    #[repr(C)]
    struct SystemTime([u16; 8]);
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: SystemTime,
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: SystemTime,
        daylight_bias: i32,
    }
    unsafe extern "system" {
        fn GetTimeZoneInformation(info: *mut TimeZoneInformation) -> u32;
    }
    // SAFETY: a plain C struct the call fills in; all zeroes is a valid value of it.
    let mut info: TimeZoneInformation = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut info) };
    let bias = info.bias + match id {
        1 => info.standard_bias,
        2 => info.daylight_bias,
        _ => 0,
    };
    -i64::from(bias) * 60_000
}

/// Linux and macOS: the C library's own answer for the local zone (TZ, /etc/localtime), as the JVM reads it.
#[cfg(unix)]
fn local_offset_ms() -> i64 {
    // `struct tm` as glibc and macOS lay it out; only `tm_gmtoff` (seconds east of UTC) is read.
    #[repr(C)]
    struct Tm {
        fields: [i32; 9],
        gmtoff: std::ffi::c_long,
        zone: *const std::ffi::c_char,
    }
    unsafe extern "C" {
        fn time(t: *mut i64) -> i64;
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }
    // SAFETY: `time` with null only returns the time; `localtime_r` fills the struct it is given.
    unsafe {
        let now = time(std::ptr::null_mut());
        let mut tm: Tm = std::mem::zeroed();
        if localtime_r(&now, &mut tm).is_null() {
            return 0;
        }
        i64::from(tm.gmtoff) * 1000
    }
}

#[cfg(not(any(windows, unix)))]
fn local_offset_ms() -> i64 {
    0
}

fn time(ms: i64) -> V {
    native(JLocalTime(ms.rem_euclid(DAY)))
}

fn date_time(ms: i64) -> V {
    native(JLocalDateTime(ms))
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = ((doy - (153 * mp + 2) / 5 + 1) as u32, if mp < 10 { mp + 3 } else { mp - 9 } as u32);
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The milliseconds a time value stands for: `Duration.between` and formatters read them.
pub fn millis(v: &V) -> Option<i64> {
    v.native::<JLocalDateTime>().map(|t| t.0).or_else(|| v.native::<JLocalTime>().map(|t| t.0))
}

fn clock(ms: i64) -> String {
    let ms = ms.rem_euclid(DAY);
    let (h, m, sec, milli) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    match (sec, milli) {
        (0, 0) => format!("{h:02}:{m:02}"),
        (_, 0) => format!("{h:02}:{m:02}:{sec:02}"),
        _ => format!("{h:02}:{m:02}:{sec:02}.{milli:03}"),
    }
}

pub fn field(class: &str, name: &str) -> Option<V> {
    Some(match (class, name) {
        ("LocalTime", "MIDNIGHT" | "MIN") => time(0),
        ("LocalTime", "NOON") => time(DAY / 2),
        ("LocalTime", "MAX") => time(DAY - 1),
        _ => return None,
    })
}

fn parse_clock(text: &str) -> Option<i64> {
    let mut parts = text.trim().split(':');
    let h: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next().map_or(Some(0.0), |p| p.parse().ok())?;
    ((0..24).contains(&h) && (0..60).contains(&m)).then(|| h * 3_600_000 + m * 60_000 + (sec * 1000.0) as i64)
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    let int = |i: usize| a.get(i).map_or(0, V::long);
    Some(Ok(match (class, name) {
        ("LocalTime", "of") => {
            if !(0..24).contains(&int(0)) || !(0..60).contains(&int(1)) || !(0..60).contains(&int(2)) {
                return Some(Err(exc("DateTimeException", format!("Invalid time {}:{}", int(0), int(1)))));
            }
            time(int(0) * 3_600_000 + int(1) * 60_000 + int(2) * 1000 + int(3) / 1_000_000)
        }
        ("LocalTime", "ofSecondOfDay") => time(int(0) * 1000),
        ("LocalTime", "now") => time(now_ms()),
        ("LocalTime", "parse") => match parse_clock(&a[0].text()) {
            Some(ms) => time(ms),
            None => return Some(Err(exc("DateTimeParseException", format!("Text '{}' could not be parsed", a[0].text())))),
        },
        ("LocalDateTime", "now") => date_time(now_ms()),
        ("LocalDateTime", "of") if a.len() >= 5 && matches!(a[1], V::Int(_) | V::Long(_)) => {
            date_time(days_from_civil(int(0), int(1), int(2)) * DAY + int(3) * 3_600_000 + int(4) * 60_000 + int(5) * 1000)
        }
        ("LocalDateTime", "of") if a.len() == 2 => {
            let day = a[0].native::<crate::javautil::JLocalDate>().map_or(0, |d| days_from_civil(d.0.0, i64::from(d.0.1), i64::from(d.0.2)));
            date_time(day * DAY + millis(&a[1]).unwrap_or(0))
        }
        ("LocalDateTime", "ofEpochSecond") => date_time(int(0) * 1000),
        ("LocalDateTime", "ofInstant") => date_time(a[0].native::<crate::javautil::JInstant>().map_or(0, |i| i.0)),
        _ => return None,
    }))
}

fn compare(at: i64, a: &[V]) -> Option<i64> {
    a.first().and_then(millis).map(|other| at - other)
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    let n = |i: usize| a.get(i).map_or(0, V::long);
    if let V::Str(text) = this {
        // `DayOfWeek.MONDAY.getValue()`: the game of names Java's enums play here.
        let day = DAYS.iter().position(|d| **d == **text)?;
        return Some(Ok(match name {
            "getValue" => V::Int(day as i32 + 1),
            "plus" => s(DAYS[(day as i64 + n(0)).rem_euclid(7) as usize]),
            "minus" => s(DAYS[(day as i64 - n(0)).rem_euclid(7) as usize]),
            _ => return None,
        }));
    }
    if let Some(t) = this.native::<JLocalTime>() {
        let at = t.0;
        let set = |unit: i64, span: i64| time(at - (at / unit % span) * unit + n(0) * unit);
        return Some(Ok(match name {
            "getHour" => V::Int((at / 3_600_000) as i32),
            "getMinute" => V::Int((at / 60_000 % 60) as i32),
            "getSecond" => V::Int((at / 1000 % 60) as i32),
            "getNano" => V::Int((at % 1000 * 1_000_000) as i32),
            "toSecondOfDay" => V::Int((at / 1000) as i32),
            "toNanoOfDay" => V::Long(at * 1_000_000),
            "plusHours" => time(at + n(0) * 3_600_000),
            "plusMinutes" => time(at + n(0) * 60_000),
            "plusSeconds" => time(at + n(0) * 1000),
            "minusHours" => time(at - n(0) * 3_600_000),
            "minusMinutes" => time(at - n(0) * 60_000),
            "minusSeconds" => time(at - n(0) * 1000),
            "plus" => time(at + a.first().and_then(|v| v.native::<JDuration>()).map_or(0, |d| d.0)),
            "minus" => time(at - a.first().and_then(|v| v.native::<JDuration>()).map_or(0, |d| d.0)),
            "withHour" => set(3_600_000, 24),
            "withMinute" => set(60_000, 60),
            "withSecond" => set(1000, 60),
            "withNano" => time(at - at % 1000 + n(0) / 1_000_000),
            "truncatedTo" => time(at - at % 1000),
            "isAfter" => V::Bool(compare(at, a)? > 0),
            "isBefore" => V::Bool(compare(at, a)? < 0),
            "compareTo" => V::Int(compare(at, a)?.signum() as i32),
            "equals" => V::Bool(compare(at, a) == Some(0)),
            "hashCode" => V::Int(at as i32),
            "toString" => s(&clock(at)),
            "format" => return Some(crate::call(&a[0], "format", &[instant(at)])),
            "atDate" => {
                let day = a[0].native::<crate::javautil::JLocalDate>().map_or(0, |d| days_from_civil(d.0.0, i64::from(d.0.1), i64::from(d.0.2)));
                date_time(day * DAY + at)
            }
            _ => return None,
        }));
    }
    if let Some(t) = this.native::<JLocalDateTime>() {
        let at = t.0;
        let (day, of_day) = (at.div_euclid(DAY), at.rem_euclid(DAY));
        let (y, mo, d) = civil(day);
        let set = |unit: i64, span: i64| date_time(at - (of_day / unit % span) * unit + n(0) * unit);
        let months = |count: i64| {
            let total = y * 12 + i64::from(mo) - 1 + count;
            let (ny, nm) = (total.div_euclid(12), total.rem_euclid(12) + 1);
            let last = days_from_civil(if nm == 12 { ny + 1 } else { ny }, if nm == 12 { 1 } else { nm + 1 }, 1) - days_from_civil(ny, nm, 1);
            date_time(days_from_civil(ny, nm, i64::from(d).min(last)) * DAY + of_day)
        };
        return Some(Ok(match name {
            "getYear" => V::Int(y as i32),
            "getMonthValue" => V::Int(mo as i32),
            "getDayOfMonth" => V::Int(d as i32),
            "getDayOfYear" => V::Int((day - days_from_civil(y, 1, 1) + 1) as i32),
            "getDayOfWeek" => s(DAYS[(day + 3).rem_euclid(7) as usize]),
            "getHour" => V::Int((of_day / 3_600_000) as i32),
            "getMinute" => V::Int((of_day / 60_000 % 60) as i32),
            "getSecond" => V::Int((of_day / 1000 % 60) as i32),
            "getNano" => V::Int((of_day % 1000 * 1_000_000) as i32),
            "with" => match a.first().and_then(|v| v.native::<JLocalTime>()) {
                Some(clock) => date_time(day * DAY + clock.0),
                None => this.clone(),
            },
            "withHour" => set(3_600_000, 24),
            "withMinute" => set(60_000, 60),
            "withSecond" => set(1000, 60),
            "withNano" => date_time(at - at.rem_euclid(1000) + n(0) / 1_000_000),
            "withDayOfMonth" => date_time(days_from_civil(y, i64::from(mo), n(0)) * DAY + of_day),
            "plusDays" => date_time(at + n(0) * DAY),
            "plusWeeks" => date_time(at + n(0) * 7 * DAY),
            "plusHours" => date_time(at + n(0) * 3_600_000),
            "plusMinutes" => date_time(at + n(0) * 60_000),
            "plusSeconds" => date_time(at + n(0) * 1000),
            "plusMonths" => months(n(0)),
            "plusYears" => months(n(0) * 12),
            "minusDays" => date_time(at - n(0) * DAY),
            "minusWeeks" => date_time(at - n(0) * 7 * DAY),
            "minusHours" => date_time(at - n(0) * 3_600_000),
            "minusMinutes" => date_time(at - n(0) * 60_000),
            "minusSeconds" => date_time(at - n(0) * 1000),
            "minusMonths" => months(-n(0)),
            "minusYears" => months(-n(0) * 12),
            "plus" => date_time(at + a.first().and_then(|v| v.native::<JDuration>()).map_or(0, |d| d.0)),
            "minus" => date_time(at - a.first().and_then(|v| v.native::<JDuration>()).map_or(0, |d| d.0)),
            "truncatedTo" => date_time(at - at.rem_euclid(1000)),
            "toLocalTime" => time(of_day),
            "toLocalDate" => native(crate::javautil::JLocalDate((y, mo, d))),
            "isAfter" => V::Bool(compare(at, a)? > 0),
            "isBefore" => V::Bool(compare(at, a)? < 0),
            "isEqual" | "equals" => V::Bool(compare(at, a) == Some(0)),
            "compareTo" => V::Int(compare(at, a)?.signum() as i32),
            "hashCode" => V::Int(at as i32 ^ (at >> 32) as i32),
            "atZone" | "atOffset" => this.clone(),
            "toInstant" => instant(at),
            "toEpochSecond" => V::Long(at.div_euclid(1000)),
            "toString" => s(&format!("{y:04}-{mo:02}-{d:02}T{}", clock(of_day))),
            "format" => return Some(crate::call(&a[0], "format", &[instant(at)])),
            _ => return None,
        }));
    }
    None
}

fn instant(ms: i64) -> V {
    native(crate::javautil::JInstant(ms))
}
