//! Menus drawn by a resource pack, with live text anywhere on them.
//!
//! A chest menu's title is text, and text can be anything a font can draw: a picture the size of the whole window
//! (or bigger: side panels, banners), and lines of the default font placed at any pixel. [`Title`] builds that
//! title: the art first, then text at window coordinates (0, 0 = the window's top-left corner, the slot in row r,
//! column c has its frame at `(7 + 18c, 17 + 18r)`), left, centred or right aligned, any colour, bold or not.
//!
//! ```ignore
//! let arts = ui::Arts::load(&app::folder().join("ui.json"));          // written by tools/uipack.py
//! let title = ui::Title::new()
//!     .art(arts.get("bank").unwrap())
//!     .text(30, 22, "Balance", "#a7f3d0")
//!     .bold_text(30, 32, &format!("{balance}"), "white")
//!     .right(168, 6, &format!("#{rank}"), "#c4b5fd")
//!     .centred(88, 128, "Inventory", "#e2daf8")
//!     .json();
//! let mut menu = gui::Menu::raw(6, &title);                          // raw: the title is JSON already
//! menu.button(slot, gui::hotspot("<green>Deposit", &[]), |c| ...);   // an invisible button over drawn art
//! ```
//!
//! What the pack needs comes from `tools/uipack.py` (the art as `<ns>:art` glyphs, the shared `kitui` fonts:
//! `kitui:space` for pixel moves, `kitui:y<N>` for text at window row N, `kitui:blank` for invisible items) and
//! the player must have the pack loaded: [`crate::pack::Tracker`] sends it and reopens the menu once it's in.
//!
//! Limits: text rows 5 to 240 (the font can lift text at most one pixel above the title line); widths are the
//! default font's (other fonts or `<font:...>` in the text measure wrong); a menu title is sent whole, so to change
//! live text, open the menu again (the items stay where they are).

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

/// The font that moves the pen by pixels.
pub const SPACE_FONT: &str = "kitui:space";
/// The highest and lowest window rows live text can start at.
pub const TOP: i32 = 5;
pub const BOTTOM: i32 = 240;

/// One character's advance in the default font (the space is 4; characters the font doesn't have count 8).
pub fn advance(ch: char) -> i32 {
    let code = ch as u32;
    match crate::ui_widths::ADVANCES.binary_search_by_key(&code, |&(c, _)| c) {
        Ok(i) => i32::from(crate::ui_widths::ADVANCES[i].1),
        Err(_) => 8,
    }
}

/// How wide `text` draws in the default font, bold adding a pixel to every character but spaces.
pub fn width(text: &str, bold: bool) -> i32 {
    text.chars().map(|ch| advance(ch) + i32::from(bold && ch != ' ')).sum()
}

/// The characters of `kitui:space` that move the pen by `px` (negative: left).
pub fn space(px: i32) -> String {
    let (base, n) = if px < 0 { (0xF801, -px) } else { (0xF821, px) };
    let mut out = String::new();
    let mut left = n.min(2047);
    for k in (0..11).rev() {
        if left >= 1 << k {
            out.push(char::from_u32(base + k as u32).unwrap_or(' '));
            left -= 1 << k;
        }
    }
    out
}

/// A picture of the pack drawn behind a menu: a glyph `ch` of `font`, `left` pixels of it left of the window's
/// edge, `top` above it, `advance` the pen's move after it (its width + 1).
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Art {
    pub font: String,
    pub ch: char,
    #[serde(default)]
    pub left: i32,
    #[serde(default)]
    pub top: i32,
    pub advance: i32,
    #[serde(default = "six")]
    pub rows: usize,
}

fn six() -> usize {
    6
}

/// The pictures of a pack, by name (`ui.json`, written by `tools/uipack.py`).
#[derive(Clone, Debug, Default)]
pub struct Arts(pub HashMap<String, Art>);

impl Arts {
    pub fn load(path: &Path) -> Arts {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        Arts(serde_json::from_str(&text).unwrap_or_default())
    }
    pub fn parse(text: &str) -> Arts {
        Arts(serde_json::from_str(text).unwrap_or_default())
    }
    pub fn get(&self, name: &str) -> Option<&Art> {
        self.0.get(name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Centre,
    Right,
}

/// A menu title: art and text placed by pixel. [`Title::json`] is the title for `gui::Menu::raw`.
#[derive(Clone, Debug)]
pub struct Title {
    parts: Vec<Value>,
    /// The pen, in window x (the title starts at 8).
    pen: i32,
}

impl Default for Title {
    fn default() -> Self {
        Title::new()
    }
}

impl Title {
    pub fn new() -> Title {
        Title { parts: Vec::new(), pen: 8 }
    }

    fn move_to(&mut self, x: i32) {
        let d = x - self.pen;
        if d != 0 {
            self.parts.push(json!({ "text": space(d), "font": SPACE_FONT }));
            self.pen = x;
        }
    }

    /// The menu's picture, its left edge `art.left` pixels left of the window.
    pub fn art(mut self, art: &Art) -> Title {
        self.move_to(-art.left);
        self.parts.push(json!({ "text": art.ch.to_string(), "font": art.font, "color": "white" }));
        self.pen = -art.left + art.advance;
        self
    }

    /// A glyph of any font (an icon in the pack), its left edge at window x; `advance` its width + 1.
    pub fn glyph(mut self, x: i32, font: &str, ch: char, advance: i32) -> Title {
        self.move_to(x);
        self.parts.push(json!({ "text": ch.to_string(), "font": font, "color": "white" }));
        self.pen = x + advance;
        self
    }

    /// A glyph drawn in `colour` (the font's picture multiplied by it: a white picture takes any colour, its
    /// transparency kept), its left edge at window x; `advance` its width + 1.
    pub fn tinted(mut self, x: i32, font: &str, ch: char, advance: i32, colour: &str) -> Title {
        self.move_to(x);
        self.parts.push(json!({ "text": ch.to_string(), "font": font, "color": colour }));
        self.pen = x + advance;
        self
    }

    /// Text with its top-left corner at (x, y) of the window.
    pub fn text(self, x: i32, y: i32, text: &str, colour: &str) -> Title {
        self.place(x, y, text, colour, false, Align::Left, false)
    }

    pub fn bold_text(self, x: i32, y: i32, text: &str, colour: &str) -> Title {
        self.place(x, y, text, colour, true, Align::Left, false)
    }

    /// Text centred on x.
    pub fn centred(self, x: i32, y: i32, text: &str, colour: &str) -> Title {
        self.place(x, y, text, colour, false, Align::Centre, false)
    }

    /// Text ending at x.
    pub fn right(self, x: i32, y: i32, text: &str, colour: &str) -> Title {
        self.place(x, y, text, colour, false, Align::Right, false)
    }

    /// The general form: alignment, bold, and `shadow` (the game's text shadow; titles have none by default).
    #[allow(clippy::too_many_arguments)]
    pub fn place(mut self, x: i32, y: i32, text: &str, colour: &str, bold: bool, align: Align, shadow: bool) -> Title {
        if text.is_empty() {
            return self;
        }
        let y = y.clamp(TOP, BOTTOM);
        let w = width(text, bold);
        let x = match align {
            Align::Left => x,
            Align::Centre => x - w / 2,
            Align::Right => x - w,
        };
        self.move_to(x);
        let mut part = json!({ "text": text, "font": format!("kitui:y{y}"), "color": colour, "bold": bold });
        if shadow {
            part["shadow_color"] = json!(0xFF00_0000u32 as i32);
        }
        self.parts.push(part);
        self.pen = x + w;
        self
    }

    /// Text from MiniMessage-ish markup is not measured here: pass plain text and a colour per call.
    /// The finished title (a text component as JSON) for `gui::Menu::raw`.
    pub fn json(&self) -> String {
        json!({ "text": "", "extra": self.parts }).to_string()
    }
}

/// Plain text in the server's small capitals (labels): `sc("Daily rewards")`.
pub fn sc(text: &str) -> String {
    crate::smallcaps::small(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_and_spaces() {
        assert_eq!(advance(' '), 4);
        assert_eq!(advance('i'), 2);
        assert_eq!(width("Hi", false), advance('H') + advance('i'));
        assert_eq!(width("Hi", true), width("Hi", false) + 2);
        assert_eq!(space(5), "\u{F823}\u{F821}");
        assert_eq!(space(-3), "\u{F802}\u{F801}");
    }

    #[test]
    fn pen_follows_the_text() {
        let t = Title::new().text(20, 30, "Hi", "white").right(168, 30, "12", "gold");
        let v: Value = serde_json::from_str(&t.json()).unwrap();
        let parts = v["extra"].as_array().unwrap();
        assert_eq!(parts[0]["text"], json!(space(12)));
        assert_eq!(parts[1]["font"], json!("kitui:y30"));
        let after = 20 + width("Hi", false);
        assert_eq!(parts[2]["text"], json!(space(168 - width("12", false) - after)));
    }
}
