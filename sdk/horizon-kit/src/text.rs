//! Colours as DecentHolograms writes them: `&` codes, hex (`&#RRGGBB`, `<#RRGGBB>`, `{#RRGGBB}`, `#RRGGBB`), gradients
//! (`<#RRGGBB>text</#RRGGBB>`), rainbows (`<RAINBOW50>text</RAINBOW>`), turned into the game's own text: a legacy
//! string (`§` codes, `§x§R§R§G§G§B§B` for hex) and from it a text component (the JSON the client reads).

use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;

const LEGACY: [(char, u32); 16] = [
    ('0', 0x000000), ('1', 0x0000AA), ('2', 0x00AA00), ('3', 0x00AAAA), ('4', 0xAA0000), ('5', 0xAA00AA), ('6', 0xFFAA00), ('7', 0xAAAAAA),
    ('8', 0x555555), ('9', 0x5555FF), ('a', 0x55FF55), ('b', 0x55FFFF), ('c', 0xFF5555), ('d', 0xFF55FF), ('e', 0xFFFF55), ('f', 0xFFFFFF),
];

fn hex_code(rgb: u32) -> String {
    let h = format!("{rgb:06x}");
    let mut out = String::from("§x");
    for c in h.chars() {
        out.push('§');
        out.push(c);
    }
    out
}

fn parse_hex(s: &str) -> Option<u32> {
    (s.len() == 6 && s.chars().all(|c| c.is_ascii_hexdigit())).then(|| u32::from_str_radix(s, 16).ok()).flatten()
}

const SPECIAL: [&str; 5] = ["&l", "&n", "&o", "&k", "&m"];

/// The formatting codes of a text that aren't colours (`&l&o`), and the text without them.
fn take_special(text: &str) -> (String, String) {
    let mut special = String::new();
    let mut rest = text.to_owned();
    for code in SPECIAL {
        if rest.contains(code) {
            special.push_str(code);
            rest = rest.replace(code, "");
        }
    }
    (special, rest)
}

/// `<#RRGGBB>text</#RRGGBB>`: each letter its step between the two colours.
fn gradients(text: &str) -> String {
    let mut out = text.to_owned();
    loop {
        let Some((start, open_len, from)) = find_tag(&out, false) else { break };
        let after = start + open_len;
        // The closing tag, with no other opening tag before it.
        let Some(close_rel) = out[after..].find("</#").or_else(|| out[after..].find("{/#")) else { break };
        let close = after + close_rel;
        if out.len() < close + 10 {
            break;
        }
        let Some(to) = parse_hex(&out[close + 3..close + 9]) else { break };
        let body = &out[after..close];
        if find_tag(body, false).is_some() {
            break;
        }
        let (special, body) = take_special(body);
        let chars: Vec<char> = body.chars().collect();
        let n = chars.len();
        let mut colored = String::new();
        for (i, c) in chars.iter().enumerate() {
            let rgb = if n <= 1 {
                0xFFFFFF
            } else {
                let lerp = |a: u32, b: u32| -> u32 {
                    let step = (a as i32 - b as i32).unsigned_abs() as i32 / (n as i32 - 1);
                    let dir = if a < b { 1 } else { -1 };
                    (a as i32 + step * i as i32 * dir).clamp(0, 255) as u32
                };
                (lerp(from >> 16, to >> 16) << 16) | (lerp((from >> 8) & 255, (to >> 8) & 255) << 8) | lerp(from & 255, to & 255)
            };
            colored.push_str(&hex_code(rgb));
            colored.push_str(&special);
            colored.push(*c);
        }
        out = format!("{}{}{}", &out[..start], colored, &out[close + 10..]);
    }
    out
}

/// An opening `<#RRGGBB>` or `{#RRGGBB}` tag: where, how long, its colour.
fn find_tag(s: &str, _closing: bool) -> Option<(usize, usize, u32)> {
    let b = s.as_bytes();
    let mut i = 0;
    while i + 9 <= b.len() {
        if (b[i] == b'<' || b[i] == b'{') && b[i + 1] == b'#' && (b[i + 8] == b'>' || b[i + 8] == b'}')
            && let Some(rgb) = parse_hex(&s[i + 2..i + 8])
        {
            return Some((i, 9, rgb));
        }
        i += 1;
    }
    None
}

/// `<#RRGGBB>`, `{#RRGGBB}`, `&#RRGGBB`, `§#RRGGBB`, `#RRGGBB`: a hex colour from there on.
fn solid(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let at = |k: usize| chars.get(k).copied().unwrap_or('\0');
        let hex_from = |k: usize| -> Option<u32> { (k + 6 <= chars.len()).then(|| parse_hex(&chars[k..k + 6].iter().collect::<String>())).flatten() };
        // A MiniMessage tag (`<gradient:#a855f7:#22d3ee>`, `<color:#fff>`, `</gradient>`) goes through as it is:
        // its colours are arguments for the MiniMessage reader, and turned into legacy codes here they became a
        // white-to-black gradient.
        if at(i) == '<'
            && (at(i + 1).is_ascii_alphabetic() || (at(i + 1) == '/' && at(i + 2).is_ascii_alphabetic()))
            && let Some(close) = chars[i..].iter().position(|c| *c == '>')
        {
            out.extend(&chars[i..=i + close]);
            i += close + 1;
            continue;
        }
        if (at(i) == '<' || at(i) == '{') && at(i + 1) == '#' && (at(i + 8) == '>' || at(i + 8) == '}')
            && let Some(rgb) = hex_from(i + 2)
        {
            out.push_str(&hex_code(rgb));
            i += 9;
            continue;
        }
        if (at(i) == '&' || at(i) == '§') && at(i + 1) == '#'
            && let Some(rgb) = hex_from(i + 2)
        {
            out.push_str(&hex_code(rgb));
            i += 8;
            continue;
        }
        if at(i) == '#'
            && let Some(rgb) = hex_from(i + 1)
        {
            out.push_str(&hex_code(rgb));
            i += 7;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn hsb(h: f64, s: f64, v: f64) -> u32 {
    let h = (h - h.floor()) * 6.0;
    let (f, i) = (h - h.floor(), h.floor() as i32);
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    (((r * 255.0 + 0.5) as u32) << 16) | (((g * 255.0 + 0.5) as u32) << 8) | ((b * 255.0 + 0.5) as u32)
}

/// `<RAINBOW50>text</RAINBOW>`: each letter a step around the colour wheel (the number its saturation).
fn rainbows(text: &str) -> String {
    let mut out = text.to_owned();
    while let Some(start) = out.find("<RAINBOW") {
        let Some(gt) = out[start..].find('>').map(|k| start + k) else { break };
        let Ok(sat) = out[start + 8..gt].parse::<u32>() else { break };
        let Some(end) = out[gt..].find("</RAINBOW>").map(|k| gt + k) else { break };
        let (special, body) = take_special(&out[gt + 1..end]);
        let chars: Vec<char> = body.chars().collect();
        let step = 1.0 / chars.len().max(1) as f64;
        let s = f64::from(sat.min(100)) / 100.0;
        let mut colored = String::new();
        for (i, c) in chars.iter().enumerate() {
            colored.push_str(&hex_code(hsb(step * i as f64, s, s)));
            colored.push_str(&special);
            colored.push(*c);
        }
        out = format!("{}{}{}", &out[..start], colored, &out[end + "</RAINBOW>".len()..]);
    }
    out
}

/// `&` codes (`&c`, `&l`, `&x&R&R...`) to `§` ones, as Bukkit's translateAlternateColorCodes.
fn ampersands(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, c) in chars.iter().enumerate() {
        if *c == '&' && chars.get(i + 1).is_some_and(|n| "0123456789AaBbCcDdEeFfKkLlMmNnOoRrXx".contains(*n)) {
            out.push('§');
        } else {
            out.push(*c);
        }
    }
    out
}

/// A text with all of DecentHolograms' colour forms, as a legacy `§` string (cached: the same lines come again).
pub fn colorize(text: &str) -> String {
    static CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
    if let Some(hit) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(text)) {
        return hit.clone();
    }
    let out = ampersands(&rainbows(&solid(&gradients(text))));
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    if map.len() > 4096 {
        map.clear();
    }
    map.insert(text.to_owned(), out.clone());
    out
}

/// Legacy codes out (`&c`, `§l`, hex): the plain letters.
pub fn strip(text: &str) -> String {
    let chars: Vec<char> = colorize(text).chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '§' && i + 1 < chars.len() {
            i += 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// A legacy `§` string as a text component: one part per run of the same style, as the client draws it.
pub fn component(legacy: &str) -> Value {
    let mut parts: Vec<Value> = Vec::new();
    let mut color: Option<String> = None;
    let (mut bold, mut italic, mut under, mut strike, mut obf) = (false, false, false, false, false);
    let mut buf = String::new();
    let chars: Vec<char> = legacy.chars().collect();
    let flush = |buf: &mut String, parts: &mut Vec<Value>, color: &Option<String>, b: bool, i: bool, u: bool, s: bool, o: bool| {
        if buf.is_empty() {
            return;
        }
        let mut part = json!({ "text": std::mem::take(buf) });
        if let Some(c) = color {
            part["color"] = json!(c);
        }
        for (key, on) in [("bold", b), ("italic", i), ("underlined", u), ("strikethrough", s), ("obfuscated", o)] {
            if on {
                part[key] = json!(true);
            }
        }
        parts.push(part);
    };
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '§' && i + 1 < chars.len() {
            let code = chars[i + 1].to_ascii_lowercase();
            // §x§R§R§G§G§B§B
            if code == 'x' && i + 13 < chars.len() + 0 && (0..6).all(|k| chars.get(i + 2 + k * 2) == Some(&'§') && chars.get(i + 3 + k * 2).is_some_and(|c| c.is_ascii_hexdigit())) {
                flush(&mut buf, &mut parts, &color, bold, italic, under, strike, obf);
                let hex: String = (0..6).map(|k| chars[i + 3 + k * 2]).collect();
                color = Some(format!("#{}", hex.to_ascii_uppercase()));
                (bold, italic, under, strike, obf) = (false, false, false, false, false);
                i += 14;
                continue;
            }
            if let Some((_, rgb)) = LEGACY.iter().find(|(c, _)| *c == code) {
                flush(&mut buf, &mut parts, &color, bold, italic, under, strike, obf);
                color = Some(format!("#{rgb:06X}"));
                (bold, italic, under, strike, obf) = (false, false, false, false, false);
                i += 2;
                continue;
            }
            let style = match code {
                'l' => Some(&mut bold),
                'o' => Some(&mut italic),
                'n' => Some(&mut under),
                'm' => Some(&mut strike),
                'k' => Some(&mut obf),
                _ => None,
            };
            if let Some(flag) = style {
                if !*flag {
                    flush(&mut buf, &mut parts, &color, bold, italic, under, strike, obf);
                    match code {
                        'l' => bold = true,
                        'o' => italic = true,
                        'n' => under = true,
                        'm' => strike = true,
                        _ => obf = true,
                    }
                }
                i += 2;
                continue;
            }
            if code == 'r' {
                flush(&mut buf, &mut parts, &color, bold, italic, under, strike, obf);
                color = None;
                (bold, italic, under, strike, obf) = (false, false, false, false, false);
                i += 2;
                continue;
            }
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush(&mut buf, &mut parts, &color, bold, italic, under, strike, obf);
    match parts.len() {
        0 => json!({ "text": "" }),
        1 => parts.pop().unwrap_or_else(|| json!({ "text": "" })),
        _ => json!({ "text": "", "extra": parts }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colours() {
        assert_eq!(colorize("&cHi"), "§cHi");
        assert_eq!(colorize("&#ff0000x"), "§x§f§f§0§0§0§0x");
        assert_eq!(strip("<#ff0000>ab</#0000ff>"), "ab");
        let c = component("§cRed §lbold");
        assert_eq!(c["extra"][0]["text"], "Red ");
        assert_eq!(c["extra"][1]["bold"], true);
        assert_eq!(component("§x§1§2§3§4§5§6z")["color"], "#123456");
    }
}
