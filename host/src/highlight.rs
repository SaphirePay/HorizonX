//! Config files in colour, the way hosting panels show them: keys, text, numbers, true/false, comments and
//! punctuation each their own colour, with line numbers. YAML, JSON, .properties, TOML/.cfg/.conf and logs.
use crate::ui::{self, px, white};
use gpui::{div, prelude::*, Div, FontStyle, FontWeight, HighlightStyle, Hsla, SharedString, StyledText};
use horizon_theme::glass::ink;
use std::ops::Range;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Yaml,
    Json,
    Props,
    Toml,
    Log,
    Plain,
}

fn kind_of(name: &str) -> Kind {
    let n = name.to_ascii_lowercase();
    if n.ends_with(".yml") || n.ends_with(".yaml") {
        Kind::Yaml
    } else if n.ends_with(".json") {
        Kind::Json
    } else if n.ends_with(".properties") {
        Kind::Props
    } else if n.ends_with(".toml") || n.ends_with(".cfg") || n.ends_with(".conf") || n.ends_with(".ini") {
        Kind::Toml
    } else if n.ends_with(".log") {
        Kind::Log
    } else {
        Kind::Plain
    }
}

#[derive(Clone, Copy)]
enum Tok {
    Key,
    Str,
    Num,
    Bool,
    Comment,
    Punct,
    Header,
    Warn,
    Error,
    Dim,
}

fn style(t: Tok) -> HighlightStyle {
    let c = |hex: u32| -> Hsla { ui::color(hex) };
    let mut s = HighlightStyle::default();
    match t {
        Tok::Key => s.color = Some(c(0xb9a6ff)),
        Tok::Str => s.color = Some(c(0x8fe3a8)),
        Tok::Num => s.color = Some(c(0xf5c77e)),
        Tok::Bool => s.color = Some(c(0x7cc4ff)),
        Tok::Comment => {
            s.color = Some(white(0.38));
            s.font_style = Some(FontStyle::Italic);
        }
        Tok::Punct => s.color = Some(white(0.42)),
        Tok::Header => {
            s.color = Some(c(0xd9ceff));
            s.font_weight = Some(FontWeight::BOLD);
        }
        Tok::Warn => s.color = Some(c(0xf2c46b)),
        Tok::Error => s.color = Some(c(0xff8a8a)),
        Tok::Dim => s.color = Some(white(0.45)),
    }
    s
}

/// Where a `#` (or `;`) comment starts outside quotes, if any.
fn comment_at(line: &str, marks: &[char]) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut prev = ' ';
    for (i, ch) in line.char_indices() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if marks.contains(&ch) && (i == 0 || prev.is_whitespace()) => return Some(i),
            None => {}
        }
        prev = ch;
    }
    None
}

/// A scalar value's token: quoted text, a number, true/false/null, or plain text.
fn value(out: &mut Vec<(Range<usize>, Tok)>, line: &str, range: Range<usize>) {
    let raw = &line[range.clone()];
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    let start = range.start + (raw.len() - raw.trim_start().len());
    let end = start + trimmed.len();
    let lower = trimmed.to_ascii_lowercase();
    let tok = if trimmed.starts_with('"') || trimmed.starts_with('\'') {
        Tok::Str
    } else if trimmed.parse::<f64>().is_ok() {
        Tok::Num
    } else if matches!(lower.as_str(), "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~") {
        Tok::Bool
    } else if trimmed == "[]" || trimmed == "{}" || trimmed == "|" || trimmed == ">" {
        Tok::Punct
    } else {
        Tok::Str
    };
    out.push((start..end, tok));
}

fn yaml(line: &str) -> Vec<(Range<usize>, Tok)> {
    let mut out = vec![];
    let comment = comment_at(line, &['#']);
    let code_end = comment.unwrap_or(line.len());
    let code = &line[..code_end];
    let mut i = code.len() - code.trim_start().len();
    // List dashes.
    while code[i..].starts_with("- ") || &code[i..] == "-" {
        out.push((i..i + 1, Tok::Punct));
        i += 1;
        i += code[i..].len() - code[i..].trim_start().len();
    }
    let rest = &code[i..];
    // `key:` then a value (or nothing: a section).
    let key_end = if rest.starts_with('"') || rest.starts_with('\'') {
        let q = rest.chars().next().unwrap_or('"');
        rest[1..].find(q).map(|e| e + 2).filter(|e| rest[*e..].starts_with(':'))
    } else {
        rest.find(": ").or_else(|| rest.strip_suffix(':').map(|r| r.len())).filter(|e| !rest[..*e].contains(['{', '[', '"']))
    };
    match key_end {
        Some(e) if e > 0 => {
            let section = rest[e + 1..].trim().is_empty();
            out.push((i..i + e, if section && i == 0 { Tok::Header } else { Tok::Key }));
            out.push((i + e..i + e + 1, Tok::Punct));
            value(&mut out, line, i + e + 1..code_end);
        }
        _ => value(&mut out, line, i..code_end),
    }
    if let Some(c) = comment {
        out.push((c..line.len(), Tok::Comment));
    }
    out
}

fn props(line: &str, toml: bool) -> Vec<(Range<usize>, Tok)> {
    let mut out = vec![];
    let trimmed = line.trim_start();
    let lead = line.len() - trimmed.len();
    if trimmed.starts_with('#') || trimmed.starts_with(';') || trimmed.starts_with('!') {
        return vec![(lead..line.len(), Tok::Comment)];
    }
    if toml && trimmed.starts_with('[') {
        return vec![(lead..line.len(), Tok::Header)];
    }
    let comment = if toml { comment_at(line, &['#', ';']) } else { None };
    let code_end = comment.unwrap_or(line.len());
    let sep = line[..code_end].find(['=', ':']);
    match sep {
        Some(s) => {
            out.push((lead..s.max(lead), Tok::Key));
            out.push((s..s + 1, Tok::Punct));
            value(&mut out, line, s + 1..code_end);
        }
        None => value(&mut out, line, lead..code_end),
    }
    if let Some(c) = comment {
        out.push((c..line.len(), Tok::Comment));
    }
    out
}

fn json(line: &str) -> Vec<(Range<usize>, Tok)> {
    let mut out = vec![];
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c == '"' {
            let start = i;
            i += 1;
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            let after = line[i..].trim_start();
            out.push((start..i, if after.starts_with(':') { Tok::Key } else { Tok::Str }));
            continue;
        }
        if c == '-' || c.is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || matches!(b[i], b'.' | b'-' | b'+' | b'e' | b'E')) {
                i += 1;
            }
            out.push((start..i, Tok::Num));
            continue;
        }
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < b.len() && b[i].is_ascii_alphabetic() {
                i += 1;
            }
            if matches!(&line[start..i], "true" | "false" | "null") {
                out.push((start..i, Tok::Bool));
            }
            continue;
        }
        if "{}[]:,".contains(c) {
            out.push((i..i + 1, Tok::Punct));
        }
        i += 1;
    }
    out
}

fn log(line: &str) -> Vec<(Range<usize>, Tok)> {
    let upper = line.to_ascii_uppercase();
    let whole = if upper.contains("ERROR") || upper.contains("SEVERE") || upper.contains("EXCEPTION") {
        Some(Tok::Error)
    } else if upper.contains("WARN") {
        Some(Tok::Warn)
    } else {
        None
    };
    let mut out = vec![];
    // The `[12:00:01]` time and `[thread/LEVEL]:` in front, quiet.
    let mut end = 0;
    for _ in 0..2 {
        if line[end..].starts_with('[') {
            if let Some(close) = line[end..].find(']') {
                end += close + 1;
                end += line[end..].len() - line[end..].trim_start_matches([' ', ':']).len();
            }
        }
    }
    if end > 0 {
        out.push((0..end, Tok::Dim));
    }
    if let Some(t) = whole {
        out.push((end..line.len(), t));
    }
    out
}

fn tokens(kind: Kind, line: &str) -> Vec<(Range<usize>, Tok)> {
    let mut t = match kind {
        Kind::Yaml => yaml(line),
        Kind::Json => json(line),
        Kind::Props => props(line, false),
        Kind::Toml => props(line, true),
        Kind::Log => log(line),
        Kind::Plain => vec![],
    };
    // In order, not overlapping, on character boundaries.
    t.retain(|(r, _)| r.start < r.end && r.end <= line.len() && line.is_char_boundary(r.start) && line.is_char_boundary(r.end));
    t.sort_by_key(|(r, _)| r.start);
    let mut last = 0;
    t.retain(|(r, _)| {
        let keep = r.start >= last;
        if keep {
            last = r.end;
        }
        keep
    });
    t
}

/// A file's text in colour with line numbers, at most `max` lines.
pub fn view(name: &str, body: &str, max: usize) -> Div {
    let kind = kind_of(name);
    let lines: Vec<&str> = body.lines().take(max).collect();
    let width = (lines.len().max(1).to_string().len() as f32) * 8. + 14.;
    let mut col = div().flex().flex_col();
    for (n, l) in lines.iter().enumerate() {
        let text: SharedString = if l.is_empty() { " ".into() } else { l.to_string().into() };
        let runs: Vec<(Range<usize>, HighlightStyle)> = tokens(kind, l).into_iter().map(|(r, t)| (r, style(t))).collect();
        col = col.child(
            div()
                .flex()
                .items_start()
                .child(div().flex_none().w(px(width)).pr(px(12.)).flex().justify_end().child(ui::mono(12., ink::OFF).child((n + 1).to_string())))
                .child(div().flex_1().min_w_0().font_family(ui::MONO).text_size(px(12.)).line_height(px(18.)).text_color(white(0.8)).child(StyledText::new(text).with_highlights(runs))),
        );
    }
    if body.lines().count() > max {
        col = col.child(ui::text(12., FontWeight::NORMAL, ink::QUIET).mt(px(8.)).child(format!("Showing the first {max} lines. Open it in your editor for the rest.")));
    }
    col
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(kind: Kind, line: &str) -> Vec<(String, &'static str)> {
        tokens(kind, line)
            .into_iter()
            .map(|(r, t)| {
                let name = match t {
                    Tok::Key => "key",
                    Tok::Str => "str",
                    Tok::Num => "num",
                    Tok::Bool => "bool",
                    Tok::Comment => "comment",
                    Tok::Punct => "punct",
                    Tok::Header => "header",
                    Tok::Warn => "warn",
                    Tok::Error => "error",
                    Tok::Dim => "dim",
                };
                (line[r].to_string(), name)
            })
            .collect()
    }

    #[test]
    fn yaml_keys_values_and_comments() {
        assert_eq!(kinds(Kind::Yaml, "  max-homes: 3 # per player"), vec![("max-homes".into(), "key"), (":".into(), "punct"), ("3".into(), "num"), ("# per player".into(), "comment")]);
        assert_eq!(kinds(Kind::Yaml, "settings:"), vec![("settings".into(), "header"), (":".into(), "punct")]);
        assert_eq!(kinds(Kind::Yaml, "  - \"world\""), vec![("-".into(), "punct"), ("\"world\"".into(), "str")]);
        assert_eq!(kinds(Kind::Yaml, "enabled: true"), vec![("enabled".into(), "key"), (":".into(), "punct"), ("true".into(), "bool")]);
        assert_eq!(kinds(Kind::Yaml, "url: 'http://a#b'"), vec![("url".into(), "key"), (":".into(), "punct"), ("'http://a#b'".into(), "str")]);
    }

    #[test]
    fn json_and_properties() {
        assert_eq!(kinds(Kind::Json, "  \"port\": 25565,"), vec![("\"port\"".into(), "key"), (":".into(), "punct"), ("25565".into(), "num"), (",".into(), "punct")]);
        assert_eq!(kinds(Kind::Props, "pvp=true"), vec![("pvp".into(), "key"), ("=".into(), "punct"), ("true".into(), "bool")]);
        assert_eq!(kinds(Kind::Props, "#Minecraft server properties"), vec![("#Minecraft server properties".into(), "comment")]);
        assert_eq!(kinds(Kind::Toml, "[server]"), vec![("[server]".into(), "header")]);
    }
}
