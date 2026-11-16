//! `FormatUtil`: `&` codes, section signs, and who may use which.
use crate::prelude::*;
use horizon_bukkit::chat_color;
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

const COLOR_CODES: &str = "0123456789abcdef";
const FORMAT_CODES: &str = "lmnor";
const MAGIC_CODE: char = 'k';
/// `ChatColor.values()`' names, as the permission nodes have them.
const CODE_NAMES: [(char, &str); 22] = [
    ('0', "black"), ('1', "dark_blue"), ('2', "dark_green"), ('3', "dark_aqua"), ('4', "dark_red"), ('5', "dark_purple"), ('6', "gold"), ('7', "gray"), ('8', "dark_gray"), ('9', "blue"), ('a', "green"), ('b', "aqua"),
    ('c', "red"), ('d', "light_purple"), ('e', "yellow"), ('f', "white"), ('k', "obfuscated"), ('l', "bold"), ('m', "strikethrough"), ('n', "underline"), ('o', "italic"), ('r', "reset"),
];

static STRIP_ALL: LazyLock<Regex> = LazyLock::new(|| Regex::new("\u{a7}+([0-9a-fk-orA-FK-OR])").expect("a valid pattern"));
static STRIP_RGB: LazyLock<Regex> = LazyLock::new(|| Regex::new("\u{a7}x((?:\u{a7}[0-9a-fA-F]){6})").expect("a valid pattern"));
static REPLACE_ALL: LazyLock<Regex> = LazyLock::new(|| Regex::new("(&)?&([0-9a-fk-orA-FK-OR])").expect("a valid pattern"));
static REPLACE_ALL_RGB: LazyLock<Regex> = LazyLock::new(|| Regex::new("(&)?&#([0-9a-fA-F]{6})").expect("a valid pattern"));
static LOG_COLOR: LazyLock<Regex> = LazyLock::new(|| Regex::new("\x1b\\[([0-9]{1,2}(;[0-9]{1,2})?)?[m|K]").expect("a valid pattern"));
static IP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([01]?\d\d?|2[0-4]\d|25[0-5])\.([01]?\d\d?|2[0-4]\d|25[0-5])\.([01]?\d\d?|2[0-4]\d|25[0-5])\.([01]?\d\d?|2[0-4]\d|25[0-5])$").expect("a valid pattern"));
/// `AbstractChatEvent.URL_PATTERN`.
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"((?:(?:https?)://)?[\w\-_.]{2,})\.([a-zA-Z]{2,}(?:/\S+)?)").expect("a valid pattern"));

/// `stripFormat`.
pub fn strip_format(input: &str) -> String {
    chat_color::strip_color(input)
}

/// `stripEssentialsFormat`.
pub fn strip_essentials_format(input: &str) -> String {
    REPLACE_ALL_RGB.replace_all(&REPLACE_ALL.replace_all(input, ""), "").into_owned()
}

/// `stripMiniFormat`.
pub fn strip_mini_format(input: &str) -> String {
    crate::adventure::strip_tags(input)
}

/// `stripLogColorFormat`.
pub fn strip_log_color_format(input: &str) -> String {
    LOG_COLOR.replace_all(input, "").into_owned()
}

/// `replaceFormat`.
pub fn replace_format(input: &str) -> String {
    let all: BTreeSet<char> = CODE_NAMES.iter().map(|(code, _)| *code).collect();
    replace_color(input, &all, true)
}

fn replace_color(input: &str, supported: &BTreeSet<char>, rgb: bool) -> String {
    let legacy = REPLACE_ALL.replace_all(input, |found: &regex::Captures<'_>| {
        let code = &found[2];
        let lower = code.chars().next().map_or(' ', |c| c.to_ascii_lowercase());
        if found.get(1).is_none() && supported.contains(&lower) { format!("\u{a7}{code}") } else { format!("&{code}") }
    });
    if !rgb {
        return legacy.into_owned();
    }
    REPLACE_ALL_RGB
        .replace_all(&legacy, |found: &regex::Captures<'_>| {
            let hex = &found[2];
            if found.get(1).is_none()
                && let Ok(color) = parse_hex_color(hex)
            {
                return color;
            }
            format!("&#{hex}")
        })
        .into_owned()
}

/// `parseHexColor`: a bad one is Java's `NumberFormatException`.
pub fn parse_hex_color(color: &str) -> Result<String, ()> {
    let color = color.strip_prefix('#').unwrap_or(color);
    fail_if!(color.len() != 6 || !color.bytes().all(|b| b.is_ascii_hexdigit()), ());
    let mut out = "\u{a7}x".to_owned();
    for c in color.chars() {
        out.push('\u{a7}');
        out.push(c);
    }
    Ok(out)
}

fn strip_color(input: &str, strip: &BTreeSet<char>) -> String {
    STRIP_ALL.replace_all(input, |found: &regex::Captures<'_>| if strip.contains(&found[1].to_lowercase().chars().next().unwrap_or(' ')) { String::new() } else { found[0].to_owned() }).into_owned()
}

fn unformat(message: &str, supported: &BTreeSet<char>, rgb: bool) -> String {
    let message = STRIP_RGB.replace_all(message, |found: &regex::Captures<'_>| if rgb { format!("&#{}", found[1].replace('\u{a7}', "")) } else { String::new() });
    STRIP_ALL
        .replace_all(&message, |found: &regex::Captures<'_>| {
            let code = found[1].to_lowercase();
            if supported.contains(&code.chars().next().unwrap_or(' ')) { format!("&{code}") } else { String::new() }
        })
        .into_owned()
}

/// `unformatString(String)`.
pub fn unformat_string(message: &str) -> String {
    let all: BTreeSet<char> = CODE_NAMES.iter().map(|(code, _)| *code).collect();
    unformat(message, &all, true)
}

/// `unformatString(IUser, String, String)`.
pub fn unformat_string_for(user: &User, perm_base: &str, message: &str) -> String {
    unformat(message, &get_supported(user, perm_base), user.is_authorized(&format!("{perm_base}.rgb")))
}

fn get_supported(user: &User, perm_base: &str) -> BTreeSet<char> {
    let mut supported = BTreeSet::new();
    if user.is_authorized(&format!("{perm_base}.color")) {
        supported.extend(COLOR_CODES.chars());
    }
    if user.is_authorized(&format!("{perm_base}.format")) {
        supported.extend(FORMAT_CODES.chars());
    }
    if user.is_authorized(&format!("{perm_base}.magic")) {
        supported.insert(MAGIC_CODE);
    }
    for (code, name) in CODE_NAMES {
        let node = format!("{perm_base}.{name}");
        if !user.is_permission_set(&node) {
            continue;
        }
        if user.is_authorized(&node) {
            supported.insert(code);
        } else {
            supported.remove(&code);
        }
    }
    supported
}

/// `formatString`.
pub fn format_string(user: &User, perm_base: &str, message: &str) -> String {
    let supported = get_supported(user, perm_base);
    let strip: BTreeSet<char> = CODE_NAMES.iter().map(|(code, _)| *code).filter(|code| !supported.contains(code)).collect();
    let rgb = user.is_authorized(&format!("{perm_base}.rgb"));
    let mut message = message.to_owned();
    if !strip.is_empty() {
        message = strip_color(&message, &strip);
    }
    if !supported.is_empty() || rgb {
        message = replace_color(&message, &supported, rgb);
    }
    message
}

/// `formatMessage`.
pub fn format_message(user: &User, perm_base: &str, input: &str) -> String {
    let message = format_string(user, perm_base, input);
    if user.is_authorized(&format!("{perm_base}.url")) { message } else { block_url(&message) }
}

fn block_url(input: &str) -> String {
    let mut text = URL.replace_all(input, "$1 $2").into_owned();
    while URL.is_match(&text) {
        text = URL.replace_all(&text, "$1 $2").into_owned();
    }
    text
}

/// `lastCode`.
pub fn last_code(input: &str) -> String {
    let Some(at) = input.rfind('\u{a7}') else { return String::new() };
    input[at..].chars().take(2).collect::<String>().chars().count().eq(&2).then(|| input[at..].chars().take(2).collect()).unwrap_or_default()
}

/// `validIP`.
pub fn valid_ip(address: &str) -> bool {
    IP.is_match(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_keeps_escaped_ampersands() {
        assert_eq!(replace_format("&cHi &&c"), "\u{a7}cHi &c");
        assert_eq!(replace_format("&zHi"), "&zHi");
        assert_eq!(replace_format("&#ff0000x"), "\u{a7}x\u{a7}f\u{a7}f\u{a7}0\u{a7}0\u{a7}0\u{a7}0x");
    }

    #[test]
    fn unformat_is_the_way_back() {
        assert_eq!(unformat_string("\u{a7}cHi"), "&cHi");
        assert_eq!(unformat_string(&replace_format("&#ff0000x")), "&#ff0000x");
    }

    #[test]
    fn urls_are_broken_apart() {
        assert_eq!(block_url("see example.com now"), "see example com now");
    }

    #[test]
    fn last_code_needs_a_letter() {
        assert_eq!(last_code("a\u{a7}cb"), "\u{a7}c");
        assert_eq!(last_code("a\u{a7}"), "");
    }

    #[test]
    fn ips() {
        assert!(valid_ip("127.0.0.1") && !valid_ip("300.0.0.1"));
    }
}
