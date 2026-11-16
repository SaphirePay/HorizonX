//! mcMMO's messages: the jar's `locale_en_US.properties` (Java properties: `\uXXXX`, `\n`, line continuations),
//! with `plugins/mcMMO/locales/locale_override.properties` over it as in mcMMO. `{0}` arguments as MessageFormat
//! (`''` is a quote), `&` colours and `[[COLOR]]` tokens to `§`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, RwLock};

const DEFAULT: &str = include_str!("../data/locale_en_US.properties");

static MESSAGES: LazyLock<RwLock<HashMap<String, String>>> = LazyLock::new(|| RwLock::new(parse(DEFAULT)));

pub fn parse(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
            continue;
        }
        let mut logical = trimmed.to_owned();
        // A trailing odd backslash continues the line.
        while logical.ends_with('\\') && !logical.ends_with("\\\\") {
            logical.pop();
            match lines.next() {
                Some(next) => logical.push_str(next.trim_start()),
                None => break,
            }
        }
        let Some(split) = logical.find(['=', ':']) else { continue };
        let key = unescape(logical[..split].trim_end());
        let value = unescape(logical[split + 1..].trim_start_matches(' '));
        out.insert(key, value);
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Read again: the built-in locale, then the override file.
pub fn load(folder: &Path) {
    let mut map = parse(DEFAULT);
    let dir = folder.join("locales");
    let _ = std::fs::create_dir_all(&dir);
    let over = dir.join("locale_override.properties");
    if !over.exists() {
        let _ = std::fs::write(&over, "# Put message keys here (copied from mcMMO's locale_en_US.properties) to change them.\n");
    }
    if let Ok(text) = std::fs::read_to_string(&over) {
        map.extend(parse(&text));
    }
    *MESSAGES.write().unwrap_or_else(|e| e.into_inner()) = map;
}

fn colour_tokens(s: &str) -> String {
    if !s.contains("[[") {
        return s.to_owned();
    }
    const TOKENS: [(&str, &str); 22] = [
        ("[[BLACK]]", "§0"),
        ("[[DARK_BLUE]]", "§1"),
        ("[[DARK_GREEN]]", "§2"),
        ("[[DARK_AQUA]]", "§3"),
        ("[[DARK_RED]]", "§4"),
        ("[[DARK_PURPLE]]", "§5"),
        ("[[GOLD]]", "§6"),
        ("[[GRAY]]", "§7"),
        ("[[DARK_GRAY]]", "§8"),
        ("[[BLUE]]", "§9"),
        ("[[GREEN]]", "§a"),
        ("[[AQUA]]", "§b"),
        ("[[RED]]", "§c"),
        ("[[LIGHT_PURPLE]]", "§d"),
        ("[[YELLOW]]", "§e"),
        ("[[WHITE]]", "§f"),
        ("[[BOLD]]", "§l"),
        ("[[UNDERLINE]]", "§n"),
        ("[[ITALIC]]", "§o"),
        ("[[STRIKE]]", "§m"),
        ("[[MAGIC]]", "§k"),
        ("[[RESET]]", "§r"),
    ];
    let mut out = s.to_owned();
    for (t, c) in TOKENS {
        out = out.replace(t, c);
    }
    out
}

/// MessageFormat: `{0}`... and `''` -> `'`.
fn format(raw: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(raw.len() + 16);
    let bytes: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == '\'' && bytes.get(i + 1) == Some(&'\'') {
            out.push('\'');
            i += 2;
            continue;
        }
        if c == '{' {
            let digits: String = bytes[i + 1..].iter().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() && bytes.get(i + 1 + digits.len()) == Some(&'}') {
                let n: usize = digits.parse().unwrap_or(0);
                out.push_str(args.get(n).copied().unwrap_or(""));
                i += digits.len() + 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// A message, filled and coloured (`§`); the key itself when the locale lacks it.
pub fn get(key: &str, args: &[&str]) -> String {
    let held = MESSAGES.read().unwrap_or_else(|e| e.into_inner());
    let raw = held.get(key).map(String::as_str).unwrap_or(key);
    horizon_kit::text::colorize(&colour_tokens(&format(raw, args)))
}

/// Whether the locale has the key.
pub fn has(key: &str) -> bool {
    MESSAGES.read().unwrap_or_else(|e| e.into_inner()).contains_key(key)
}

#[cfg(test)]
mod tests {
    #[test]
    fn props() {
        let m = super::parse("a.b=Hello {0}''s\\nx\nc=\\u00e9t\\\n  continued\n");
        assert_eq!(m["a.b"], "Hello {0}''s\nx");
        assert_eq!(m["c"], "étcontinued");
        assert_eq!(super::format("Hi {0}''s {1}", &["A", "B"]), "Hi A's B");
    }
}
