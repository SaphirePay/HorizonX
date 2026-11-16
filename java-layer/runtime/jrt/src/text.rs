//! `java.text`: `DecimalFormat` and `NumberFormat`, what plugins use to show money, TPS and distances.
//! The pattern language as Java reads it for the US locale: `#` `0` `,` `.` `%`, literal prefix and suffix,
//! and the rounding modes. Scientific notation (`E`) isn't read.
use super::*;

pub struct JDecimal(Mutex<Format>);

#[derive(Clone)]
struct Format {
    prefix: String,
    suffix: String,
    min_int: usize,
    min_frac: usize,
    max_frac: usize,
    group: usize,
    grouping: bool,
    percent: bool,
    rounding: String,
}

fn parse(pattern: &str) -> Format {
    let positive = pattern.split(';').next().unwrap_or("");
    let is_pattern = |c: char| matches!(c, '#' | '0' | ',' | '.');
    let start = positive.find(is_pattern).unwrap_or(positive.len());
    let end = positive.rfind(is_pattern).map_or(start, |i| i + 1);
    let literal = |text: &str| text.replace('\'', "");
    let (prefix, body, suffix) = (literal(&positive[..start]), &positive[start..end], literal(&positive[end..]));
    let (int_part, frac_part) = body.split_once('.').unwrap_or((body, ""));
    let group = int_part.rfind(',').map_or(0, |at| int_part[at + 1..].chars().filter(|c| matches!(c, '#' | '0')).count());
    Format {
        percent: prefix.contains('%') || suffix.contains('%'),
        prefix,
        suffix,
        min_int: int_part.chars().filter(|c| *c == '0').count(),
        min_frac: frac_part.chars().filter(|c| *c == '0').count(),
        max_frac: frac_part.chars().filter(|c| matches!(c, '0' | '#')).count(),
        group,
        grouping: group > 0,
        rounding: "HALF_EVEN".into(),
    }
}

/// Java (8 and later) rounds the double's exact binary value: `2.675` is `2.67499...`, so "#.##" gives 2.67.
fn round(value: f64, digits: usize, mode: &str) -> f64 {
    if mode == "HALF_EVEN" {
        // Rust's formatting rounds the exact value half-to-even, as Java does.
        return format!("{value:.digits$}").parse().unwrap_or(value);
    }
    let scale = 10f64.powi(digits as i32);
    let negative = value < 0.0;
    let magnitude = value.abs() * scale;
    let floor = magnitude.floor();
    let fraction = magnitude - floor;
    let up = match mode {
        "UP" => fraction > 0.0,
        "DOWN" => false,
        "CEILING" => fraction > 0.0 && !negative,
        "FLOOR" => fraction > 0.0 && negative,
        "HALF_UP" => fraction >= 0.5,
        "HALF_DOWN" => fraction > 0.5,
        _ => fraction > 0.5 || (fraction == 0.5 && floor % 2.0 != 0.0),
    };
    let rounded = if up { floor + 1.0 } else { floor } / scale;
    if negative { -rounded } else { rounded }
}

fn format(f: &Format, value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return format!("{}{}\u{221e}{}", if value < 0.0 { "-" } else { "" }, f.prefix, f.suffix);
    }
    let value = if f.percent { value * 100.0 } else { value };
    let rounded = round(value, f.max_frac, &f.rounding);
    // Java keeps the sign of a negative number that rounds to zero: -0.004 is "-0.00".
    let negative = value.is_sign_negative();
    let digits = format!("{:.*}", f.max_frac, rounded.abs());
    let (int_digits, frac_digits) = digits.split_once('.').unwrap_or((&digits, ""));
    let mut frac = frac_digits.to_owned();
    while frac.len() > f.min_frac && frac.ends_with('0') {
        frac.pop();
    }
    let mut int = int_digits.trim_start_matches('0').to_owned();
    while int.len() < f.min_int {
        int.insert(0, '0');
    }
    if f.grouping && f.group > 0 && int.len() > f.group {
        let chars: Vec<char> = int.chars().collect();
        let mut grouped = String::new();
        for (i, c) in chars.iter().enumerate() {
            if i > 0 && (chars.len() - i) % f.group == 0 {
                grouped.push(',');
            }
            grouped.push(*c);
        }
        int = grouped;
    }
    if int.is_empty() && frac.is_empty() {
        int.push('0');
    }
    let body = if frac.is_empty() { int } else { format!("{int}.{frac}") };
    format!("{}{}{}{}", if negative { "-" } else { "" }, f.prefix, body, f.suffix)
}

fn made(pattern: &str) -> V {
    native(JDecimal(Mutex::new(parse(pattern))))
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    match class {
        "DecimalFormat" => Some(Ok(made(&a.first().map_or("#,##0.###".to_owned(), V::text)))),
        "DecimalFormatSymbols" => Some(Ok(native(crate::javautil::JStub))),
        _ => None,
    }
}

pub fn static_call(class: &str, name: &str, _a: &[V]) -> Option<R> {
    // `DecimalFormatSymbols.getInstance(Locale.US)`: the same stand-in as `new DecimalFormatSymbols(...)`;
    // formats here use '.' and ',' as the US and ROOT locales do.
    if class == "DecimalFormatSymbols" && matches!(name, "getInstance") {
        return Some(Ok(native(crate::javautil::JStub)));
    }
    if !matches!(class, "NumberFormat" | "DecimalFormat") {
        return None;
    }
    Some(Ok(match name {
        "getInstance" | "getNumberInstance" => made("#,##0.###"),
        "getIntegerInstance" => made("#,##0"),
        "getPercentInstance" => made("#,##0%"),
        "getCurrencyInstance" => made("$#,##0.00"),
        "getCompactNumberInstance" => made("#,##0"),
        _ => return None,
    }))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    let d = this.native::<JDecimal>()?;
    let mut f = lock(&d.0);
    Some(Ok(match name {
        "format" => s(&format(&f, a[0].double())),
        "parse" => {
            let text: String = a[0].text().chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-')).collect();
            match text.parse::<f64>() {
                Ok(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => V::Long(n as i64),
                Ok(n) => V::Double(n),
                Err(_) => return Some(Err(exc("ParseException", format!("Unparseable number: \"{}\"", a[0].text())))),
            }
        }
        "setRoundingMode" => {
            f.rounding = a[0].text();
            V::Null
        }
        "setMaximumFractionDigits" => {
            f.max_frac = a[0].int().max(0) as usize;
            f.min_frac = f.min_frac.min(f.max_frac);
            V::Null
        }
        "setMinimumFractionDigits" => {
            f.min_frac = a[0].int().max(0) as usize;
            f.max_frac = f.max_frac.max(f.min_frac);
            V::Null
        }
        "setMinimumIntegerDigits" => {
            f.min_int = a[0].int().max(0) as usize;
            V::Null
        }
        "setGroupingUsed" => {
            f.grouping = a[0].truth();
            if f.grouping && f.group == 0 {
                f.group = 3;
            }
            V::Null
        }
        "setGroupingSize" => {
            f.group = a[0].int().max(0) as usize;
            V::Null
        }
        "applyPattern" => {
            *f = parse(&a[0].text());
            V::Null
        }
        "getMaximumFractionDigits" => V::Int(f.max_frac as i32),
        "getMinimumFractionDigits" => V::Int(f.min_frac as i32),
        "setParseIntegerOnly" | "setDecimalFormatSymbols" | "setPositivePrefix" | "setNegativePrefix" | "setDecimalSeparatorAlwaysShown" => V::Null,
        _ => return None,
    }))
}
