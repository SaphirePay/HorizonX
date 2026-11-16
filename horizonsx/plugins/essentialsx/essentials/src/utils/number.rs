//! `NumberUtil`, and the `java.text.DecimalFormat` it stands on.
use crate::prelude::*;
use crate::big_decimal::Rounding;
use std::sync::RwLock;

/// `java.text.DecimalFormat`, for patterns of `#`, `0`, `,` and `.` with text before and after.
#[derive(Clone, Debug)]
pub struct DecimalFormat {
    prefix: String,
    suffix: String,
    group: usize,
    min_int: usize,
    min_frac: i32,
    max_frac: i32,
    group_mark: String,
    decimal_mark: String,
    rounding: Rounding,
}

impl DecimalFormat {
    pub fn new(pattern: &str, rounding: Rounding) -> Self {
        let pattern = pattern.split(';').next().unwrap_or(pattern);
        let is_part = |c: char| matches!(c, '#' | '0' | ',' | '.');
        let start = pattern.find(is_part).unwrap_or(pattern.len());
        let end = pattern.rfind(is_part).map_or(start, |at| at + 1);
        let number = &pattern[start..end];
        let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
        let group = whole.rfind(',').map_or(0, |at| whole.len() - at - 1);
        Self {
            prefix: pattern[..start].replace('\'', ""),
            suffix: pattern[end..].replace('\'', ""),
            group,
            min_int: whole.matches('0').count(),
            min_frac: fraction.matches('0').count() as i32,
            max_frac: fraction.len() as i32,
            group_mark: ",".to_owned(),
            decimal_mark: ".".to_owned(),
            rounding,
        }
    }
    /// `_getCurrencyFormat`: `FLOOR`, with the marks of `currency-symbol-format-locale`.
    pub fn with_locale(pattern: &str, language_tag: Option<&str>) -> Self {
        let mut format = Self::new(pattern, Rounding::Floor);
        let language = language_tag.unwrap_or("en").split(['-', '_']).next().unwrap_or("en").to_lowercase();
        if let Some(group) = group_mark_of(&language) {
            group.clone_into(&mut format.group_mark);
            ",".clone_into(&mut format.decimal_mark);
        }
        format
    }
    /// `format(BigDecimal)`.
    pub fn format(&self, value: &BigDecimal) -> String {
        let rounded = value.set_scale(self.max_frac, self.rounding).to_plain_string();
        let (negative, digits) = rounded.strip_prefix('-').map_or((false, rounded.as_str()), |rest| (true, rest));
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        let whole = whole.trim_start_matches('0');
        let whole = format!("{}{whole}", "0".repeat(self.min_int.saturating_sub(whole.len())));
        let mut grouped = String::new();
        for (index, c) in whole.chars().enumerate() {
            if self.group > 0 && index > 0 && (whole.len() - index) % self.group == 0 {
                grouped.push_str(&self.group_mark);
            }
            grouped.push(c);
        }
        let mut fraction = fraction.to_owned();
        while fraction.len() as i32 > self.min_frac && fraction.ends_with('0') {
            fraction.pop();
        }
        let zero = grouped.chars().all(|c| c == '0') && fraction.chars().all(|c| c == '0');
        let sign = if negative && !zero { "-" } else { "" };
        let point = if fraction.is_empty() { String::new() } else { format!("{}{fraction}", self.decimal_mark) };
        format!("{sign}{}{grouped}{point}{}", self.prefix, self.suffix)
    }
}

static PRETTY_FORMAT: RwLock<Option<DecimalFormat>> = RwLock::new(None);

/// `internalSetPrettyFormat`.
pub fn internal_set_pretty_format(format: DecimalFormat) {
    *PRETTY_FORMAT.write().unwrap_or_else(|e| e.into_inner()) = Some(format);
}

fn pretty() -> DecimalFormat {
    PRETTY_FORMAT.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| DecimalFormat::new("#,##0.00", Rounding::Floor))
}

/// `shortCurrency`.
pub fn short_currency(value: &BigDecimal) -> String {
    let symbol = settings().get_currency_symbol();
    if settings().is_currency_symbol_suffixed() { format!("{}{symbol}", format_as_currency(value)) } else { format!("{symbol}{}", format_as_currency(value)) }
}

/// `formatDouble`: `#,###.##`, half up.
pub fn format_double(value: f64) -> String {
    DecimalFormat::new("#,###.##", Rounding::HalfUp).format(&BigDecimal::from_f64(value))
}

/// `formatAsCurrency`: `#0.00`, floor, and no `.00`.
pub fn format_as_currency(value: &BigDecimal) -> String {
    let text = DecimalFormat::new("#0.00", Rounding::Floor).format(value);
    text.strip_suffix(".00").map_or_else(|| text.clone(), str::to_owned)
}

/// `formatAsPrettyCurrency`.
pub fn format_as_pretty_currency(value: &BigDecimal) -> String {
    let text = pretty().format(value);
    text.strip_suffix(".00").map_or_else(|| text.clone(), str::to_owned)
}

fn display(value: &BigDecimal, exact: bool) -> String {
    let mut currency = if exact { value.to_plain_string() } else { format_as_pretty_currency(value) };
    let mut sign = "";
    if value.signum() < 0 {
        currency = currency.chars().skip(1).collect();
        sign = "-";
    }
    let symbol = settings().get_currency_symbol();
    if settings().is_currency_symbol_suffixed() {
        format!("{sign}{}", tl_literal("currency", crate::args![currency, symbol]))
    } else {
        format!("{sign}{}", tl_literal("currency", crate::args![symbol, currency]))
    }
}

/// `displayCurrency`: this can be MiniMessage, so it goes through `parsed`.
pub fn display_currency(value: &BigDecimal) -> String {
    display(value, false)
}

/// `displayCurrencyExactly`.
pub fn display_currency_exactly(value: &BigDecimal) -> String {
    display(value, true)
}

/// `sanitizeCurrencyString`.
pub fn sanitize_currency_string(input: &str) -> String {
    input.replace(&settings().get_currency_symbol(), "")
}

/// `isInt`.
pub fn is_int(text: &str) -> bool {
    text.parse::<i32>().is_ok()
}

/// `isLong`.
pub fn is_long(text: &str) -> bool {
    text.parse::<i64>().is_ok()
}

/// `isPositiveInt`.
pub fn is_positive_int(text: &str) -> bool {
    text.parse::<i32>().is_ok_and(|value| value > 0)
}

/// `isNumeric`.
pub fn is_numeric(text: &str) -> bool {
    text.chars().all(char::is_numeric)
}

/// `isHexadecimal`.
pub fn is_hexadecimal(text: &str) -> bool {
    i32::from_str_radix(text, 16).is_ok()
}

/// The grouping mark of a language that writes `1.000,5`, and `None` for the US marks.
fn group_mark_of(language: &str) -> Option<&'static str> {
    match language {
        "de" | "es" | "it" | "nl" | "pt" | "tr" | "da" | "id" => Some("."),
        "fr" | "ru" | "pl" | "cs" | "sk" | "uk" | "sv" | "fi" | "no" | "nb" => Some("\u{a0}"),
        _ => None,
    }
}

/// `NumberFormat.parse` then `new BigDecimal(number.toString())`: it stops at the first mark it
/// can't use, and an answer that isn't a whole `long` went through a `double`.
fn number_format_parse(text: &str, language: &str) -> Option<BigDecimal> {
    let (decimal, group) = match group_mark_of(language) {
        Some(".") => (',', Some('.')),
        Some(_) => (',', None),
        None => ('.', Some(',')),
    };
    let mut plain = String::new();
    let mut in_fraction = false;
    for mark in text.chars() {
        if mark.is_ascii_digit() {
            plain.push(mark);
        } else if mark == decimal && !in_fraction {
            in_fraction = true;
            plain.push('.');
        } else if Some(mark) != group || in_fraction {
            break;
        }
    }
    if !plain.contains(|mark: char| mark.is_ascii_digit()) {
        return None;
    }
    let value: f64 = plain.parse().ok()?;
    if value.fract() == 0.0 && value < 9.2e18 {
        return Some(BigDecimal::from_i64(value as i64));
    }
    Some(BigDecimal::from_f64(value))
}

/// `parseStringToBDecimal(input)`: US marks.
pub fn parse_string_to_big_decimal(text: &str) -> EssResult<BigDecimal> {
    parse_string_to_big_decimal_in(text, "en")
}

/// `parseStringToBDecimal(input, locale)`: `10k`, `1.5m`, in that language's marks.
pub fn parse_string_to_big_decimal_in(text: &str, language: &str) -> EssResult<BigDecimal> {
    fail_if!(text.is_empty(), EssError::Message("IllegalArgumentException".to_owned()));
    let sanitized: String = text.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | ',')).collect();
    let multiplier = match text.replace(&sanitized, "").to_uppercase().as_str() {
        "" => None,
        "K" => Some(1_000),
        "M" => Some(1_000_000),
        "B" => Some(1_000_000_000),
        "T" => Some(1_000_000_000_000),
        _ => return Err(EssError::invalid_modifier()),
    };
    let amount = number_format_parse(&sanitized, language).ok_or_else(|| EssError::Message(format!("Unparseable number: \"{sanitized}\"")))?;
    Ok(match multiplier {
        Some(multiplier) => amount.multiply(&BigDecimal::from_i64(multiplier)),
        None => amount,
    })
}

/// `constrainToRange`.
pub fn constrain_to_range(value: i32, min: i32, max: i32) -> i32 {
    value.max(min).min(max)
}

/// `Integer.decode`, for `#rrggbb`.
pub fn decode_hex(text: &str) -> Option<i32> {
    i32::from_str_radix(text.strip_prefix('#').unwrap_or(text), 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(text: &str) -> BigDecimal {
        BigDecimal::parse(text).unwrap()
    }

    #[test]
    fn pretty_currency_floors_and_groups() {
        let format = DecimalFormat::new("#,##0.00", Rounding::Floor);
        assert_eq!(format.format(&d("1234567.899")), "1,234,567.89");
        assert_eq!(format.format(&d("0")), "0.00");
        assert_eq!(format.format(&d("-1.5")), "-1.50");
        assert_eq!(format.format(&d("-1.001")), "-1.01");
    }

    #[test]
    fn optional_digits_drop() {
        assert_eq!(DecimalFormat::new("#,###.##", Rounding::HalfUp).format(&d("1234.5")), "1,234.5");
        assert_eq!(DecimalFormat::new("#0.00", Rounding::Floor).format(&d("5")), "5.00");
        assert_eq!(format_double(19.999), "20");
    }

    #[test]
    fn a_format_may_carry_text() {
        assert_eq!(DecimalFormat::new("$#,##0.00", Rounding::Floor).format(&d("1000")), "$1,000.00");
    }

    #[test]
    fn suffixes_multiply() {
        assert_eq!(parse_string_to_big_decimal("10k").unwrap().to_plain_string(), "10000");
        assert_eq!(parse_string_to_big_decimal("1,000").unwrap().to_plain_string(), "1000");
        assert!(matches!(parse_string_to_big_decimal("10x"), Err(EssError::Tl { .. })));
    }

    #[test]
    fn java_parse_rules() {
        assert!(is_int("+5") && is_int("-5") && !is_int("5.0") && !is_int(""));
        assert!(is_numeric("") && is_numeric("123") && !is_numeric("1a"));
    }
}
