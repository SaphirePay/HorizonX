//! Text as AxAPI's StringUtils and bcommons write it: `&` colours, `&#RRGGBB`, MiniMessage, `%key%` replacements;
//! money as `number-formatting` says, times as `timer-format` says.

use horizon_bukkit::adventure::Component;
use horizon_bukkit::{CommandSender, Player};
use horizon_kit::{mini, text};

/// `%key%` replacements, in order.
pub fn fill(raw: &str, reps: &[(&str, &str)]) -> String {
    let mut out = raw.to_owned();
    for (k, v) in reps {
        out = out.replace(k, v);
    }
    out
}

/// A formatted line as the client shows it (StringUtils.format).
pub fn component(raw: &str) -> Component {
    Component(mini::parse_mixed(&text::colorize(raw)))
}

/// A formatted line as legacy `§` text (StringUtils.formatToString): titles, item names, holograms.
pub fn legacy(raw: &str) -> String {
    mini::legacy(&text::colorize(raw))
}

pub fn send(player: &Player, raw: &str) {
    player.send_component(&component(raw));
}

pub fn send_to(sender: &CommandSender, raw: &str) {
    sender.send_component(&component(raw));
}

/// A line for the console, colours taken out.
pub fn console(raw: &str) {
    horizon_bukkit::horizon_mod::log(&mini::plain(&text::colorize(raw)));
}

/// `number-formatting`: mode 0 a DecimalFormat pattern (`#,###.##`), 1 short (`1.5K`), 2 raw.
pub fn number(amount: f64, mode: i64, pattern: &str) -> String {
    match mode {
        1 => short(amount),
        2 => raw(amount),
        _ => decimal(amount, pattern),
    }
}

/// Java's `Double.toString` for the plain numbers it prints.
pub fn raw(amount: f64) -> String {
    if amount.fract() == 0.0 && amount.abs() < 1e7 { format!("{amount:.1}") } else { format!("{amount}") }
}

/// `#,###.##`: grouped by three when the pattern has a comma, at most as many decimals as it has `#`/`0` after
/// the point (DecimalFormat's HALF_EVEN), trailing zeros dropped for `#`, kept for `0`.
pub fn decimal(amount: f64, pattern: &str) -> String {
    let (int_part, frac_part) = pattern.split_once('.').unwrap_or((pattern, ""));
    let max = frac_part.chars().filter(|c| *c == '#' || *c == '0').count();
    let min = frac_part.chars().filter(|c| *c == '0').count();
    let group = int_part.contains(',');
    let negative = amount < 0.0;
    let scale = 10f64.powi(max as i32);
    let scaled = (amount.abs() * scale).round_ties_even();
    let whole = (scaled / scale).trunc();
    let mut frac = format!("{:0width$}", (scaled - whole * scale).round() as u64, width = max);
    while frac.len() > min && frac.ends_with('0') {
        frac.pop();
    }
    let digits = format!("{whole:.0}");
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if group && i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if negative && (whole > 0.0 || !frac.is_empty()) { "-" } else { "" };
    if frac.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}.{frac}") }
}

/// `1.5K`, `2M`, `3.25B`.
pub fn short(amount: f64) -> String {
    let units = [(1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")];
    for (size, unit) in units {
        if amount.abs() >= size {
            return format!("{}{unit}", decimal(amount / size, "#.##"));
        }
    }
    decimal(amount, "#.##")
}

/// `timer-format`: 1 `HH:MM:SS` (`MM:SS` under an hour), 2 the largest unit (`20m`), 3 `01h 25m 35s`; the unit
/// letters from lang.yml `time.*`.
pub fn time(ms: i64, format: i64, units: &[String; 4]) -> String {
    let total = (ms.max(0) + 999) / 1000;
    let (d, h, m, s) = (total / 86_400, total / 3600 % 24, total / 60 % 60, total % 60);
    let [us, um, uh, ud] = units;
    match format {
        2 => {
            if d > 0 {
                format!("{d}{ud}")
            } else if h > 0 {
                format!("{h}{uh}")
            } else if m > 0 {
                format!("{m}{um}")
            } else {
                format!("{s}{us}")
            }
        }
        3 => {
            let mut parts = Vec::new();
            if d > 0 {
                parts.push(format!("{d:02}{ud}"));
            }
            if d > 0 || h > 0 {
                parts.push(format!("{h:02}{uh}"));
            }
            if d > 0 || h > 0 || m > 0 {
                parts.push(format!("{m:02}{um}"));
            }
            parts.push(format!("{s:02}{us}"));
            parts.join(" ")
        }
        _ => {
            let hours = total / 3600;
            if hours > 0 { format!("{hours:02}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_as_decimal_format() {
        assert_eq!(decimal(1234567.891, "#,###.##"), "1,234,567.89");
        assert_eq!(decimal(100.0, "#,###.##"), "100");
        assert_eq!(decimal(1000.5, "#,###.##"), "1,000.5");
        assert_eq!(decimal(-2500.0, "#,###.##"), "-2,500");
        assert_eq!(short(1500.0), "1.5K");
        assert_eq!(short(2_000_000.0), "2M");
        assert_eq!(short(950.0), "950");
    }

    #[test]
    fn times() {
        let u = ["s".to_owned(), "m".to_owned(), "h".to_owned(), "d".to_owned()];
        assert_eq!(time(15_000, 1, &u), "00:15");
        assert_eq!(time(5_135_000, 1, &u), "01:25:35");
        assert_eq!(time(1_200_000, 2, &u), "20m");
        assert_eq!(time(5_135_000, 3, &u), "01h 25m 35s");
        assert_eq!(time(9_000, 3, &u), "09s");
    }
}
