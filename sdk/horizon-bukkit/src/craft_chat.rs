//! CraftBukkit's legacy text <-> component conversion (org.bukkit.craftbukkit.util.CraftChatMessage, Paper 26.3),
//! ported from its bytecode: what ItemMeta.setDisplayName/setLore(String) store and getDisplayName/getLore give back.
//! Paper's exact shapes matter: plugins compare item names by the legacy string they read back, and an item saved on
//! Paper (`{"text":"","extra":["Blade"]}`) must still be `isSimilar` to one made here from the same string.
//!
//! Not ported: StringMessage's URL group (a web address in the text becomes its own component with an open_url click
//! event on Paper when not `plain`); here it stays in the text run.

use serde_json::{Map, Value, json};

/// `CraftChatMessage.fromStringOrNull`: null (no component) for null or "", else the first line's component.
pub fn from_string_or_null(text: &str) -> Option<Value> {
    if text.is_empty() {
        return None;
    }
    Some(from_string(text))
}

/// `CraftChatMessage.fromString(text)[0]`: a root `""` with each run of text appended in its style.
pub fn from_string(text: &str) -> Value {
    #[derive(Clone, Default, PartialEq)]
    struct Style {
        color: Option<String>,
        bold: Option<bool>,
        italic: Option<bool>,
        underlined: Option<bool>,
        strikethrough: Option<bool>,
        obfuscated: Option<bool>,
    }
    let reset = Style { bold: Some(false), italic: Some(false), underlined: Some(false), strikethrough: Some(false), obfuscated: Some(false), ..Default::default() };
    let empty = Style { italic: Some(false), ..Default::default() };
    let mut modifier = Style::default();
    let mut runs: Vec<Value> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut current = 0usize;
    let mut needs_add = false;
    let mut seen_color = false;
    let mut hex: Option<String> = None;
    let mut ended = false;

    let run = |s: &str, style: &Style| -> Value {
        if *style == Style::default() {
            return json!(s);
        }
        let mut o = Map::new();
        o.insert("text".into(), json!(s));
        if let Some(c) = &style.color {
            o.insert("color".into(), json!(c));
        }
        for (k, v) in [("bold", style.bold), ("italic", style.italic), ("underlined", style.underlined), ("strikethrough", style.strikethrough), ("obfuscated", style.obfuscated)] {
            if let Some(v) = v {
                o.insert(k.into(), json!(v));
            }
        }
        Value::Object(o)
    };

    let mut i = 0usize;
    while i < chars.len() && !ended {
        let c = chars[i];
        // A legacy code: the section sign and one of [0-9a-fk-orx] (case-insensitive).
        let code = (c == '\u{a7}').then(|| chars.get(i + 1).map(|n| n.to_ascii_lowercase())).flatten().filter(|n| n.is_ascii_hexdigit() || ('k'..='o').contains(n) || *n == 'r' || *n == 'x');
        if code.is_none() && c != '\n' {
            i += 1;
            continue;
        }
        if i > current {
            needs_add = false;
            let s: String = chars[current..i].iter().collect();
            runs.push(run(&s, &modifier));
        }
        match code {
            Some(f) => {
                if f == 'x' {
                    hex = Some("#".to_owned());
                } else if let Some(h) = hex.as_mut() {
                    h.push(f);
                    if h.len() == 7 {
                        let parsed = h.chars().skip(1).all(|d| d.is_ascii_hexdigit()).then(|| h.to_ascii_uppercase());
                        modifier = Style { color: parsed, ..reset.clone() };
                        hex = None;
                    }
                } else if ('k'..='o').contains(&f) {
                    match f {
                        'l' => modifier.bold = Some(true),
                        'o' => modifier.italic = Some(true),
                        'm' => modifier.strikethrough = Some(true),
                        'n' => modifier.underlined = Some(true),
                        _ => modifier.obfuscated = Some(true),
                    }
                } else {
                    // A colour or reset: from RESET the first time, EMPTY after; what the old style had on goes off.
                    let previous = modifier.clone();
                    modifier = if seen_color { empty.clone() } else { reset.clone() };
                    modifier.color = legacy_color_name(f).map(str::to_owned);
                    seen_color = true;
                    if previous.bold == Some(true) {
                        modifier.bold = Some(false);
                    }
                    if previous.italic == Some(true) {
                        modifier.italic = Some(false);
                    }
                    if previous.obfuscated == Some(true) {
                        modifier.obfuscated = Some(false);
                    }
                    if previous.strikethrough == Some(true) {
                        modifier.strikethrough = Some(false);
                    }
                    if previous.underlined == Some(true) {
                        modifier.underlined = Some(false);
                    }
                }
                needs_add = true;
                i += 2;
                current = i;
            }
            None => {
                // A newline: the first line's component ends here (fromString's [0]).
                if needs_add {
                    runs.push(run("", &modifier));
                }
                needs_add = false;
                ended = true;
                current = chars.len();
            }
        }
    }
    if !ended && (current < chars.len() || needs_add) {
        let s: String = chars[current.min(chars.len())..].iter().collect();
        runs.push(run(&s, &modifier));
    }
    if runs.is_empty() {
        return json!("");
    }
    json!({ "text": "", "extra": runs })
}

fn legacy_color_name(code: char) -> Option<&'static str> {
    Some(match code {
        '0' => "black",
        '1' => "dark_blue",
        '2' => "dark_green",
        '3' => "dark_aqua",
        '4' => "dark_red",
        '5' => "dark_purple",
        '6' => "gold",
        '7' => "gray",
        '8' => "dark_gray",
        '9' => "blue",
        'a' => "green",
        'b' => "aqua",
        'c' => "red",
        'd' => "light_purple",
        'e' => "yellow",
        'f' => "white",
        _ => return None,
    })
}

fn legacy_code_of(name: &str) -> Option<char> {
    ('0'..='9').chain('a'..='f').find(|c| legacy_color_name(*c) == Some(name))
}

/// `CraftChatMessage.fromComponent`: each component of the tree in order (itself, then its `extra`, depth first),
/// its colour (or a reset after formatting), its formats, its text.
pub fn from_component(component: &Value) -> String {
    let mut out = String::new();
    let mut had_format = false;
    let mut flat = Vec::new();
    flatten(component, &mut flat);
    for c in flat {
        let (text, style) = match c {
            Value::String(s) => (s.clone(), None),
            Value::Object(o) => (o.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(), Some(o)),
            _ => (String::new(), None),
        };
        let on = |k: &str| style.and_then(|o| o.get(k)).is_some_and(|v| v.as_bool() == Some(true) || v.as_i64() == Some(1));
        let color = style.and_then(|o| o.get("color")).and_then(Value::as_str);
        if !text.is_empty() || color.is_some() {
            if let Some(color) = color {
                match legacy_code_of(color) {
                    Some(code) => {
                        out.push('\u{a7}');
                        out.push(code);
                    }
                    None => {
                        out.push_str("\u{a7}x");
                        for d in color.trim_start_matches('#').to_ascii_uppercase().chars() {
                            out.push('\u{a7}');
                            out.push(d);
                        }
                    }
                }
                had_format = true;
            } else if had_format {
                out.push_str("\u{a7}r");
                had_format = false;
            }
        }
        for (k, code) in [("bold", 'l'), ("italic", 'o'), ("underlined", 'n'), ("strikethrough", 'm'), ("obfuscated", 'k')] {
            if on(k) {
                out.push('\u{a7}');
                out.push(code);
                had_format = true;
            }
        }
        out.push_str(&text);
    }
    out
}

/// A text component read from SNBT (an item saved by Paper: `{bold:0b,...}`) has its style flags as numbers; the
/// game's codec reads them as booleans. Made booleans here, so the same item compares equal however it came in.
pub fn normalize_component(id: &str, value: Value) -> Value {
    if !matches!(id, "minecraft:custom_name" | "minecraft:item_name" | "minecraft:lore") {
        return value;
    }
    fn fix(v: Value) -> Value {
        match v {
            Value::Object(o) => Value::Object(
                o.into_iter()
                    .map(|(k, v)| {
                        let v = if matches!(k.as_str(), "bold" | "italic" | "underlined" | "strikethrough" | "obfuscated") {
                            match v.as_i64() {
                                Some(n) => Value::Bool(n != 0),
                                None => v,
                            }
                        } else {
                            fix(v)
                        };
                        (k, v)
                    })
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.into_iter().map(fix).collect()),
            other => other,
        }
    }
    fix(value)
}

fn flatten<'v>(c: &'v Value, out: &mut Vec<&'v Value>) {
    match c {
        Value::Array(items) => {
            // A JSON list is a component of its first element with the rest as siblings.
            for item in items {
                flatten(item, out);
            }
        }
        Value::Object(o) => {
            out.push(c);
            if let Some(Value::Array(extra)) = o.get("extra") {
                for e in extra {
                    flatten(e, out);
                }
            }
        }
        _ => out.push(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Paper 26.3's own answers (jprobe --paper, 11-13), as JSON.
    #[test]
    fn paper_shapes() {
        assert_eq!(from_string("Blade"), json!({"text": "", "extra": ["Blade"]}));
        assert_eq!(from_string("\u{a7}aHi"), json!({"text": "", "extra": [{"text": "Hi", "color": "green", "bold": false, "italic": false, "obfuscated": false, "strikethrough": false, "underlined": false}]}));
        assert_eq!(from_string("\u{a7}a\u{a7}lBold \u{a7}rplain"), json!({"text": "", "extra": [{"text": "Bold ", "color": "green", "bold": true, "italic": false, "obfuscated": false, "strikethrough": false, "underlined": false}, {"text": "plain", "bold": false, "italic": false}]}));
        assert_eq!(from_string("a\u{a7}cb"), json!({"text": "", "extra": ["a", {"text": "b", "color": "red", "bold": false, "italic": false, "obfuscated": false, "strikethrough": false, "underlined": false}]}));
        assert_eq!(from_string("\u{a7}x\u{a7}f\u{a7}f\u{a7}0\u{a7}0\u{a7}0\u{a7}0hex"), json!({"text": "", "extra": [{"text": "hex", "color": "#FF0000", "bold": false, "italic": false, "obfuscated": false, "strikethrough": false, "underlined": false}]}));
        assert_eq!(from_string("\u{a7}lB\u{a7}oI"), json!({"text": "", "extra": [{"text": "B", "bold": true}, {"text": "I", "bold": true, "italic": true}]}));
        assert_eq!(from_string("\u{a7}k"), json!({"text": "", "extra": [{"text": "", "obfuscated": true}]}));
        assert_eq!(from_string("\u{a7}cred\u{a7}lbold\u{a7}cred2"), json!({"text": "", "extra": [
            {"text": "red", "color": "red", "bold": false, "italic": false, "obfuscated": false, "strikethrough": false, "underlined": false},
            {"text": "bold", "color": "red", "bold": true, "italic": false, "obfuscated": false, "strikethrough": false, "underlined": false},
            {"text": "red2", "color": "red", "bold": false, "italic": false}]}));
        assert_eq!(from_string_or_null(""), None);
        assert_eq!(from_string(""), json!(""));
    }

    #[test]
    fn paper_back() {
        for (input, back) in [("Blade", "Blade"), ("\u{a7}aHi", "\u{a7}aHi"), ("\u{a7}a\u{a7}lBold \u{a7}rplain", "\u{a7}a\u{a7}lBold \u{a7}rplain"), ("a\u{a7}cb", "a\u{a7}cb"),
            ("\u{a7}x\u{a7}f\u{a7}f\u{a7}0\u{a7}0\u{a7}0\u{a7}0hex", "\u{a7}x\u{a7}F\u{a7}F\u{a7}0\u{a7}0\u{a7}0\u{a7}0hex"), ("\u{a7}lB\u{a7}oI", "\u{a7}lB\u{a7}r\u{a7}l\u{a7}oI"), ("\u{a7}k", "\u{a7}k"),
            ("\u{a7}cred\u{a7}lbold\u{a7}cred2", "\u{a7}cred\u{a7}c\u{a7}lbold\u{a7}cred2")] {
            assert_eq!(from_component(&from_string(input)), back, "{input}");
        }
    }
}
