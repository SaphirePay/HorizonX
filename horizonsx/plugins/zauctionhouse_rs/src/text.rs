//! zAuctionHouse's text: MiniMessage with `&` codes, `#rrggbb` shorthand and the `<primary>`-style colour tags of
//! config.yml; prices, times and dates as its formats write them.

use horizon_bukkit::{Component, ItemStack};
use serde_json::{Value, json};

/// `#rrggbb` (not after `&`, `<`, `:` or `§`) -> `&#rrggbb`, and the configured colour tags -> `<#rrggbb>`.
pub fn prepare(text: &str, colors: &[(String, String)]) -> String {
    let mut s = text.to_owned();
    for (tag, hex) in colors {
        if s.contains(tag.as_str()) {
            s = s.replace(tag.as_str(), &format!("<{hex}>"));
            let close = tag.replacen('<', "</", 1);
            s = s.replace(&close, "");
        }
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    for (i, c) in chars.iter().enumerate() {
        if *c == '#' && chars.len() >= i + 7 && chars[i + 1..i + 7].iter().all(char::is_ascii_hexdigit) {
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            if !matches!(prev, Some('&' | '<' | ':' | '§')) {
                out.push('&');
            }
        }
        out.push(*c);
    }
    out
}

/// Text -> component JSON.
pub fn component(text: &str, colors: &[(String, String)]) -> Value {
    horizon_kit::mini::parse_mixed(&prepare(text, colors))
}

/// Item text: not italic unless it says so (as zMenu builds names and lore).
pub fn item_component(text: &str, colors: &[(String, String)]) -> Value {
    let mut v = component(text, colors);
    if let Value::Object(m) = &mut v
        && !m.contains_key("italic")
    {
        m.insert("italic".into(), json!(false));
    }
    v
}

pub fn legacy(text: &str, colors: &[(String, String)]) -> String {
    horizon_kit::mini::to_legacy(&component(text, colors))
}

pub fn set_name(stack: &mut ItemStack, text: &str, colors: &[(String, String)]) {
    stack.set_display_name_component(&Component(item_component(text, colors)));
}

pub fn set_lore(stack: &mut ItemStack, lines: &[String], colors: &[(String, String)]) {
    if lines.is_empty() {
        stack.remove_component("minecraft:lore");
        return;
    }
    stack.set_component("minecraft:lore", Value::Array(lines.iter().map(|l| item_component(l, colors)).collect()));
}

pub fn add_lore(stack: &mut ItemStack, lines: &[String], colors: &[(String, String)]) {
    let mut lore: Vec<Value> = stack.get_component("minecraft:lore").and_then(Value::as_array).cloned().unwrap_or_default();
    lore.extend(lines.iter().map(|l| item_component(l, colors)));
    stack.set_component("minecraft:lore", Value::Array(lore));
}

/// `#,###.#`-style decimal format: grouping when there's a `,`, as many decimals as `#`/`0` after the `.`.
pub fn decimal(value: f64, pattern: &str) -> String {
    let decimals = pattern.split_once('.').map_or(0, |(_, d)| d.chars().filter(|c| *c == '#' || *c == '0').count());
    let min_decimals = pattern.split_once('.').map_or(0, |(_, d)| d.chars().filter(|c| *c == '0').count());
    let grouping = pattern.contains(',');
    let neg = value < 0.0;
    let mut text = format!("{:.*}", decimals, value.abs());
    if let Some(dot) = text.find('.') {
        let mut frac: String = text[dot + 1..].to_owned();
        while frac.len() > min_decimals && frac.ends_with('0') {
            frac.pop();
        }
        let int = text[..dot].to_owned();
        text = if frac.is_empty() { int } else { format!("{int}.{frac}") };
    }
    let (int, frac) = match text.split_once('.') {
        Some((i, f)) => (i.to_owned(), Some(f.to_owned())),
        None => (text.clone(), None),
    };
    let int = if grouping {
        let digits: Vec<char> = int.chars().collect();
        let mut g = String::new();
        for (i, d) in digits.iter().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                g.push(',');
            }
            g.push(*d);
        }
        g
    } else {
        int
    };
    let mut out = if neg { format!("-{int}") } else { int };
    if let Some(f) = frac {
        out.push('.');
        out.push_str(&f);
    }
    out
}

/// `String.format("%.1fK", value)`-style formats of the price reductions.
pub fn printf_float(pattern: &str, value: f64) -> String {
    if let Some(start) = pattern.find("%.") {
        let rest = &pattern[start + 2..];
        if let Some(fpos) = rest.find('f') {
            let p: usize = rest[..fpos].parse().unwrap_or(2);
            return format!("{}{:.*}{}", &pattern[..start], p, value, &rest[fpos + 1..]);
        }
    }
    decimal(value, pattern)
}

/// `TimerBuilder`: seconds -> `%02dᴅ %02dʜ %02dᴍ` etc.
pub fn duration(seconds: i64, day: &str, hour: &str, minute: &str, second: &str) -> String {
    let s = seconds.max(0);
    let (d, h, m, sec) = (s / 86400, s % 86400 / 3600, s % 3600 / 60, s % 60);
    let p = |n: i64| format!("{n:02}");
    if d > 0 {
        horizon_kit::fmt::java(day, &[&p(d), &p(h), &p(m)])
    } else if h > 0 {
        horizon_kit::fmt::java(hour, &[&p(h), &p(m), &p(sec)])
    } else if m > 0 {
        horizon_kit::fmt::java(minute, &[&p(m), &p(sec)])
    } else {
        horizon_kit::fmt::java(second, &[&p(sec)])
    }
}

/// A `SimpleDateFormat`-style date (UTC): `dd/MM/yyyy HH:mm:ss`.
pub fn date(ms: i64, pattern: &str) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // days -> civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    pattern
        .replace("yyyy", &format!("{y:04}"))
        .replace("MM", &format!("{m:02}"))
        .replace("dd", &format!("{d:02}"))
        .replace("HH", &format!("{:02}", rem / 3600))
        .replace("mm", &format!("{:02}", rem % 3600 / 60))
        .replace("ss", &format!("{:02}", rem % 60))
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Named placeholders `%key%` filled.
pub fn fill(text: &str, pairs: &[(&str, String)]) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let mut out = text.to_owned();
    for (k, v) in pairs {
        if out.contains(k) {
            out = out.replace(k, v);
        }
    }
    out
}
