//! `AdventureUtil` and `PaperAdventureFacet`: messages are MiniMessage, with the two tags EssentialsX
//! adds, `<primary>` and `<secondary>`, whose colors come from `message-colors` in config.yml.
use horizon_bukkit::{Component, chat_color, mini_message};
use std::sync::RwLock;

static COLORS: RwLock<(String, String)> = RwLock::new((String::new(), String::new()));

/// `initAdventureFacet`: the colors are as `Settings` reads them, a `#rrggbb` or a color's name.
pub fn set_colors(primary: &str, secondary: &str) {
    *COLORS.write().unwrap_or_else(|e| e.into_inner()) = (primary.to_owned(), secondary.to_owned());
}

fn colors() -> (String, String) {
    let (primary, secondary) = COLORS.read().unwrap_or_else(|e| e.into_inner()).clone();
    (if primary.is_empty() { "gold".to_owned() } else { primary }, if secondary.is_empty() { "red".to_owned() } else { secondary })
}

/// What `TagResolver.resolver("primary", ...)` does: `horizon-bukkit`'s parser knows the colors alone.
fn resolve_custom_tags(message: &str) -> String {
    let (primary, secondary) = colors();
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(at) = rest.find('<') {
        let escaped = rest[..at].chars().rev().take_while(|c| *c == '\\').count() % 2 == 1;
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let replaced = if escaped {
            None
        } else if rest.starts_with("<primary>") {
            Some((9, format!("<{primary}>")))
        } else if rest.starts_with("</primary>") {
            Some((10, format!("</{primary}>")))
        } else if rest.starts_with("<secondary>") {
            Some((11, format!("<{secondary}>")))
        } else if rest.starts_with("</secondary>") {
            Some((12, format!("</{secondary}>")))
        } else {
            None
        };
        match replaced {
            Some((length, tag)) => {
                out.push_str(&tag);
                rest = &rest[length..];
            }
            None => {
                out.push('<');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `deserializeMiniMessage`.
pub fn deserialize_mini_message(message: &str) -> Component {
    mini_message(&resolve_custom_tags(message))
}

/// `miniToLegacy`.
pub fn mini_to_legacy(message: &str) -> String {
    deserialize_mini_message(message).to_legacy()
}

/// `escapeTags`: what the player typed is never a tag.
pub fn escape_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => out.push_str("\\<"),
            '\\' if matches!(chars.peek(), Some('<' | '\\') | None) => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out
}

/// `stripTags`: the text of a MiniMessage string.
pub fn strip_tags(input: &str) -> String {
    deserialize_mini_message(input).plain()
}

const COLOR_TAGS: [&str; 16] = ["black", "dark_blue", "dark_green", "dark_aqua", "dark_red", "dark_purple", "gold", "gray", "dark_gray", "blue", "green", "aqua", "red", "light_purple", "yellow", "white"];

/// `AdventureUtil.fromCharName`: the color of a section sign code.
pub fn from_char_name(code: char) -> Option<&'static str> {
    "0123456789abcdef".find(code).map(|index| COLOR_TAGS[index])
}

fn decoration_tag(code: char) -> Option<&'static str> {
    Some(match code {
        'k' => "obfuscated",
        'l' => "bold",
        'm' => "strikethrough",
        'n' => "underlined",
        'o' => "italic",
        _ => return None,
    })
}

/// `legacyToMini`: section sign codes as tags, every one closed again, so that the message around
/// it keeps its own color. A color ends the decorations before it, as the legacy format does.
pub fn legacy_to_mini(message: &str) -> String {
    if !message.contains('\u{a7}') {
        return message.to_owned();
    }
    let chars: Vec<char> = message.chars().collect();
    let mut out = String::with_capacity(message.len());
    let mut open: Vec<String> = Vec::new();
    let close_all = |out: &mut String, open: &mut Vec<String>| {
        for tag in open.drain(..).rev() {
            out.push_str(&format!("</{tag}>"));
        }
    };
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\u{a7}' || i + 1 >= chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let code = chars[i + 1].to_ascii_lowercase();
        // `§x§r§r§g§g§b§b`, the "unusual" hex form.
        if code == 'x' && i + 13 < chars.len() && (0..6).all(|n| chars[i + 2 + n * 2] == '\u{a7}' && chars[i + 3 + n * 2].is_ascii_hexdigit()) {
            let hex: String = (0..6).map(|n| chars[i + 3 + n * 2]).collect();
            close_all(&mut out, &mut open);
            let tag = format!("#{}", hex.to_ascii_lowercase());
            out.push_str(&format!("<{tag}>"));
            open.push(tag);
            i += 14;
            continue;
        }
        if let Some(color) = from_char_name(code) {
            close_all(&mut out, &mut open);
            out.push_str(&format!("<{color}>"));
            open.push(color.to_owned());
        } else if let Some(decoration) = decoration_tag(code) {
            if !open.iter().any(|tag| tag == decoration) {
                out.push_str(&format!("<{decoration}>"));
                open.push(decoration.to_owned());
            }
        } else if code == 'r' {
            close_all(&mut out, &mut open);
        } else {
            out.push('\u{a7}');
            out.push(chars[i + 1]);
        }
        i += 2;
    }
    close_all(&mut out, &mut open);
    out
}

static URL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?:(https?)://)?([-\w_.]+\.\w{2,})(/\S*)?").expect("regex"));

/// `legacyToMiniWithUrls`: as `legacy_to_mini`, with links that open when clicked.
pub fn legacy_to_mini_with_urls(message: &str) -> String {
    let linked = URL.replace_all(message, |found: &regex::Captures<'_>| {
        let shown = &found[0];
        let target = if found.get(1).is_some() { shown.to_owned() } else { format!("http://{shown}") };
        format!("<click:open_url:'{target}'>{shown}</click>")
    });
    legacy_to_mini(&linked)
}

/// `chat_color::strip_color`, kept here so that `utils` has one import.
pub fn strip_color(text: &str) -> String {
    chat_color::strip_color(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_tags_become_colors_unless_escaped() {
        assert_eq!(resolve_custom_tags("<primary>a</primary>"), "<gold>a</gold>");
        assert_eq!(resolve_custom_tags("\\<primary>a"), "\\<primary>a");
    }

    #[test]
    fn legacy_is_closed_so_the_message_keeps_its_color() {
        assert_eq!(legacy_to_mini("\u{a7}cHi"), "<red>Hi</red>");
        assert_eq!(legacy_to_mini("\u{a7}c\u{a7}lHi\u{a7}rx"), "<red><bold>Hi</bold></red>x");
        assert_eq!(legacy_to_mini("plain"), "plain");
    }

    #[test]
    fn tags_are_escaped() {
        assert_eq!(escape_tags("<red>a"), "\\<red>a");
    }
}
