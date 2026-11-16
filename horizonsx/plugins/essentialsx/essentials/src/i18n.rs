//! `com.earth2me.essentials.I18n`: `messages.properties` through Java's `ResourceBundle` and
//! `MessageFormat`. Translations in `plugins/Essentials/messages` win, as `FileResClassLoader` makes them.
use crate::adventure::{escape_tags, legacy_to_mini};
use crate::big_decimal::{BigDecimal, Rounding};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, RwLock};

/// A translation's argument. `MessageFormat` writes numbers itself, grouped, and leaves the others.
#[derive(Clone, Debug)]
pub enum Arg {
    /// Anything else: its text is never parsed for tags.
    Text(String),
    /// `AdventureUtil.parsed`: MiniMessage in the argument counts.
    Parsed(String),
    Int(i64),
    Double(f64),
    Decimal(BigDecimal),
    /// Java's `null`.
    Null,
}

/// `AdventureUtil.parsed`.
pub fn parsed(text: impl Into<String>) -> Arg {
    Arg::Parsed(text.into())
}

macro_rules! arg_from {
    ($($kind:ty => |$value:ident| $make:expr;)*) => {
        $(impl From<$kind> for Arg {
            fn from($value: $kind) -> Self {
                $make
            }
        })*
    };
}

arg_from! {
    &str => |value| Self::Text(value.to_owned());
    String => |value| Self::Text(value);
    &String => |value| Self::Text(value.clone());
    i32 => |value| Self::Int(i64::from(value));
    i64 => |value| Self::Int(value);
    u32 => |value| Self::Int(i64::from(value));
    usize => |value| Self::Int(value as i64);
    f32 => |value| Self::Double(f64::from(value));
    f64 => |value| Self::Double(value);
    BigDecimal => |value| Self::Decimal(value);
    &BigDecimal => |value| Self::Decimal(value.clone());
}

impl From<Option<String>> for Arg {
    fn from(value: Option<String>) -> Self {
        value.map_or(Self::Null, Self::Text)
    }
}

/// The argument list: `args!["name", 3]`.
#[macro_export]
macro_rules! args {
    ($($value:expr),* $(,)?) => {
        &[$($crate::i18n::Arg::from($value)),*]
    };
}

include!(concat!(env!("OUT_DIR"), "/bundled_messages.rs"));

type Bundle = HashMap<String, String>;

/// `java.util.Locale`, as `language_COUNTRY_variant`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Locale {
    language: String,
    country: String,
    variant: String,
}

impl Locale {
    pub fn of(language: &str, country: &str, variant: &str) -> Self {
        Self { language: language.to_ascii_lowercase(), country: country.to_ascii_uppercase(), variant: variant.to_owned() }
    }
    pub fn english() -> Self {
        Self::of("en", "", "")
    }
    /// `getLanguage`.
    pub fn language(&self) -> &str {
        &self.language
    }
    /// `Locale.toString`.
    pub fn name(&self) -> String {
        let mut name = self.language.clone();
        if !self.country.is_empty() || !self.variant.is_empty() {
            name.push('_');
            name.push_str(&self.country);
        }
        if !self.variant.is_empty() {
            name.push('_');
            name.push_str(&self.variant);
        }
        name
    }
    /// `ResourceBundle`'s candidates, most specific first: `messages` itself is the last.
    fn candidates(&self) -> Vec<String> {
        let mut names = Vec::new();
        if !self.language.is_empty() {
            if !self.country.is_empty() && !self.variant.is_empty() {
                names.push(format!("messages_{}_{}_{}", self.language, self.country, self.variant));
            }
            if !self.country.is_empty() {
                names.push(format!("messages_{}_{}", self.language, self.country));
            }
            names.push(format!("messages_{}", self.language));
        }
        names.push("messages".to_owned());
        names
    }
    /// `NumberFormat.getInstance(locale)`'s grouping and decimal marks, for the languages shipped.
    fn number_marks(&self) -> (&'static str, &'static str) {
        match self.language.as_str() {
            "de" | "es" | "it" | "nl" | "pt" | "tr" | "da" | "el" | "id" | "hr" | "sr" | "ro" | "vi" | "bs" | "is" | "sl" => (".", ","),
            "fr" => ("\u{202f}", ","),
            "ru" | "pl" | "cs" | "sk" | "uk" | "bg" | "hu" | "lt" | "lv" | "et" | "fi" | "no" | "nb" | "sv" => ("\u{a0}", ","),
            _ => (",", "."),
        }
    }
}

/// `.properties` text, as `Properties.load` reads it.
pub fn parse_properties(text: &str) -> Bundle {
    let mut bundle = Bundle::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start_matches([' ', '\t', '\u{c}']);
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }
        // A line ending in an odd number of backslashes goes on.
        let mut logical = trimmed.to_owned();
        while logical.chars().rev().take_while(|c| *c == '\\').count() % 2 == 1 {
            logical.pop();
            match lines.next() {
                Some(next) => logical.push_str(next.trim_start_matches([' ', '\t', '\u{c}'])),
                None => break,
            }
        }
        let chars: Vec<char> = logical.chars().collect();
        let mut at = 0;
        while at < chars.len() && !matches!(chars[at], '=' | ':' | ' ' | '\t' | '\u{c}') {
            at += if chars[at] == '\\' { 2 } else { 1 };
        }
        let key_end = at.min(chars.len());
        while at < chars.len() && matches!(chars[at], ' ' | '\t' | '\u{c}') {
            at += 1;
        }
        if at < chars.len() && matches!(chars[at], '=' | ':') {
            at += 1;
        }
        while at < chars.len() && matches!(chars[at], ' ' | '\t' | '\u{c}') {
            at += 1;
        }
        bundle.insert(unescape(&chars[..key_end]), unescape(&chars[at..]));
    }
    bundle
}

fn unescape(chars: &[char]) -> String {
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' || i + 1 >= chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        match chars[i] {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'f' => out.push('\u{c}'),
            'u' if i + 4 < chars.len() => {
                let hex: String = chars[i + 1..=i + 4].iter().collect();
                out.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('?'));
                i += 4;
            }
            other => out.push(other),
        }
        i += 1;
    }
    out
}

#[derive(Default)]
struct Loaded {
    current: Option<Locale>,
    /// Each locale's chain of bundles, most specific first.
    chains: HashMap<Locale, Arc<Vec<Bundle>>>,
}

static STATE: LazyLock<RwLock<Loaded>> = LazyLock::new(Default::default);

fn messages_folder() -> PathBuf {
    crate::data_folder().join("messages")
}

fn builtin(name: &str) -> Option<Bundle> {
    BUNDLED.iter().find(|(file, _)| *file == name).map(|(_, text)| parse_properties(text))
}

fn on_disk(name: &str) -> Option<Bundle> {
    std::fs::read_to_string(messages_folder().join(format!("{name}.properties"))).ok().map(|text| parse_properties(&text))
}

/// `blockingLoadBundle`: disk first, then the jar, each with its own parents.
fn load_chain(locale: &Locale) -> Arc<Vec<Bundle>> {
    let candidates = locale.candidates();
    let disk: Vec<Bundle> = candidates.iter().filter_map(|name| on_disk(name)).collect();
    if !disk.is_empty() {
        return Arc::new(disk);
    }
    Arc::new(candidates.iter().filter_map(|name| builtin(name)).collect())
}

fn chain(locale: &Locale) -> Arc<Vec<Bundle>> {
    if let Some(found) = STATE.read().unwrap_or_else(|e| e.into_inner()).chains.get(locale) {
        return found.clone();
    }
    let loaded = load_chain(locale);
    STATE.write().unwrap_or_else(|e| e.into_inner()).chains.insert(locale.clone(), loaded.clone());
    loaded
}

/// `getCurrentLocale`.
pub fn current_locale() -> Locale {
    STATE.read().unwrap_or_else(|e| e.into_inner()).current.clone().unwrap_or_else(Locale::english)
}

/// `updateLocale`.
pub fn update_locale(locale: &str) {
    {
        let mut state = STATE.write().unwrap_or_else(|e| e.into_inner());
        if !locale.is_empty() {
            state.current = Some(get_locale(locale));
        }
        state.chains.clear();
    }
    crate::info(&format!("Using locale {}", current_locale().name()));
}

/// `I18n.getLocale`.
pub fn get_locale(text: &str) -> Locale {
    if text.is_empty() {
        return current_locale();
    }
    let parts: Vec<&str> = text.split(['_', '.']).collect();
    match parts.as_slice() {
        [language] => Locale::of(language, "", ""),
        [language, country] => Locale::of(language, country, ""),
        [language, country, variant] => Locale::of(language, country, variant),
        _ => current_locale(),
    }
}

/// True if the key exists anywhere: Java finds out by catching `MissingResourceException`.
pub fn has_key(key: &str) -> bool {
    translate(&current_locale(), key).is_some()
}

fn translate(locale: &Locale, key: &str) -> Option<String> {
    let lookup = |locale: &Locale| chain(locale).iter().find_map(|bundle| bundle.get(key).cloned());
    lookup(locale).or_else(|| lookup(&current_locale())).or_else(|| lookup(&Locale::english()))
}

/// `tlLiteral`.
pub fn tl_literal(key: &str, args: &[Arg]) -> String {
    tl_locale(&current_locale(), key, args)
}

/// `tlLocale`. A key no bundle has gives the key back, where Java throws.
pub fn tl_locale(locale: &Locale, key: &str, args: &[Arg]) -> String {
    let Some(text) = translate(locale, key) else { return key.to_owned() };
    if args.is_empty() {
        return text.replace("''", "'");
    }
    let processed: Vec<Arg> = args
        .iter()
        .map(|arg| match arg {
            Arg::Text(text) => Arg::Text(legacy_to_mini(&escape_tags(text))),
            Arg::Parsed(text) => Arg::Text(text.clone()),
            other => other.clone(),
        })
        .collect();
    message_format(locale, &text, &processed).replace('\u{a0}', " ")
}

/// `NumberFormat.getInstance`: grouped, at most three decimals, half even.
fn format_number(locale: &Locale, value: &BigDecimal) -> String {
    let (group, decimal) = locale.number_marks();
    let rounded = value.set_scale(3, Rounding::HalfEven).to_plain_string();
    let (negative, digits) = rounded.strip_prefix('-').map_or((false, rounded.as_str()), |rest| (true, rest));
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let mut grouped = String::new();
    for (index, c) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index) % 3 == 0 {
            grouped.push_str(group);
        }
        grouped.push(c);
    }
    let fraction = fraction.trim_end_matches('0');
    let sign = if negative && (grouped != "0" || !fraction.is_empty()) { "-" } else { "" };
    if fraction.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}{decimal}{fraction}") }
}

fn format_arg(locale: &Locale, arg: Option<&Arg>, index: usize) -> String {
    match arg {
        None => format!("{{{index}}}"),
        Some(Arg::Text(text) | Arg::Parsed(text)) => text.clone(),
        Some(Arg::Int(value)) => format_number(locale, &BigDecimal::from_i64(*value)),
        Some(Arg::Double(value)) if value.is_nan() => "NaN".to_owned(),
        Some(Arg::Double(value)) if value.is_infinite() => if *value > 0.0 { "\u{221e}" } else { "-\u{221e}" }.to_owned(),
        Some(Arg::Double(value)) => format_number(locale, &BigDecimal::from_f64(*value)),
        Some(Arg::Decimal(value)) => format_number(locale, value),
        Some(Arg::Null) => "null".to_owned(),
    }
}

/// `MessageFormat`: `{0}`, its quoting, and a format that names a number's own pattern is read as `{0}`.
pub fn message_format(locale: &Locale, pattern: &str, args: &[Arg]) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut quoted = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                out.push('\'');
                i += 2;
            } else {
                quoted = !quoted;
                i += 1;
            }
            continue;
        }
        if quoted || c != '{' {
            out.push(c);
            i += 1;
            continue;
        }
        let mut depth = 1;
        let mut end = i + 1;
        while end < chars.len() && depth > 0 {
            match chars[end] {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            end += 1;
        }
        let inside: String = chars[i + 1..end.saturating_sub(1).max(i + 1)].iter().collect();
        match inside.split(',').next().unwrap_or_default().trim().parse::<usize>() {
            Ok(index) => out.push_str(&format_arg(locale, args.get(index), index)),
            Err(_) => {
                // Java refuses the pattern; `format()` there falls back to `[...]`.
                out.push('[');
                out.push_str(&inside);
                out.push(']');
            }
        }
        i = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(pattern: &str, args: &[Arg]) -> String {
        message_format(&Locale::english(), pattern, args)
    }

    #[test]
    fn quotes_follow_message_format() {
        assert_eq!(fmt("it''s {0}", args!["x"]), "it's x");
        assert_eq!(fmt("'{0}' {0}", args!["x"]), "{0} x");
    }

    #[test]
    fn numbers_are_grouped_like_number_format() {
        assert_eq!(fmt("{0}", args![1234567]), "1,234,567");
        assert_eq!(fmt("{0}", args![1.5f64]), "1.5");
        assert_eq!(fmt("{0}", args![0.12345f64]), "0.123");
        assert_eq!(fmt("{0}", args![1000.0f64]), "1,000");
        assert_eq!(fmt("{0}", args![-5]), "-5");
    }

    #[test]
    fn a_missing_argument_stays_as_written() {
        assert_eq!(fmt("{0} {1}", args!["x"]), "x {1}");
    }

    #[test]
    fn properties_read_like_java() {
        let bundle = parse_properties("# c\nbalance=<green>Balance\\:<secondary> {0}\nkey\\ a = b\\u0041\nlong=a\\\n   b\nempty=\n");
        assert_eq!(bundle["balance"], "<green>Balance:<secondary> {0}");
        assert_eq!(bundle["key a"], "bA");
        assert_eq!(bundle["long"], "ab");
        assert_eq!(bundle["empty"], "");
    }

    #[test]
    fn the_real_file_parses_and_has_its_keys() {
        let bundle = parse_properties(include_str!("../resources/messages/messages.properties"));
        assert_eq!(bundle["heal"], "<primary>You have been healed.");
        assert!(bundle.len() > 1000);
    }

    #[test]
    fn locale_names() {
        assert_eq!(Locale::of("pt", "br", "").candidates(), ["messages_pt_BR", "messages_pt", "messages"]);
    }
}
