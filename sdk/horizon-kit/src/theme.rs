//! One look for every plugin's menus: the Nyxora palette, item names and lore laid out the same way everywhere,
//! and the standard buttons (back, close, pages, confirm, toggles), so a new menu looks finished from its first
//! line.
//!
//! ```ignore
//! use horizon_kit::theme::{self, Lore};
//! menu.button(13, theme::item("DIAMOND", "Diamond Pack", Lore::new()
//!     .text("Sixteen diamonds, ready to use.")
//!     .blank()
//!     .field("Price", "$2,500")
//!     .field("Stock", "12 left")
//!     .bar(0.4, "Restocks in 2h")
//!     .blank()
//!     .click("Left-click", "to buy")
//!     .click("Right-click", "to preview")), |c| { /* ... */ });
//! menu.button(49, theme::close(), |c| c.close());
//! ```

use crate::item;
use horizon_bukkit::{ItemStack, Material};
use serde_json::json;

/// Names, headlines.
pub const TEXT: &str = "#f4f4f5";
/// Descriptions, field names.
pub const MUTED: &str = "#a1a1aa";
/// Quiet details: separators, units.
pub const DIM: &str = "#71717a";
/// The network's colour: titles, highlights.
pub const PRIMARY: &str = "#a855f7";
/// Soft purple: secondary text on purple.
pub const SOFT: &str = "#c4b5fd";
/// Actions ("Left-click to ...") and values that move.
pub const ACCENT: &str = "#67e8f9";
pub const GOOD: &str = "#4ade80";
pub const WARN: &str = "#fbbf24";
pub const BAD: &str = "#f87171";

/// Item text that isn't italic, in a colour: `<!i><#hex>text`.
fn styled(color: &str, text: &str) -> String {
    format!("<!i><{color}>{text}")
}

/// An item's name: bold, light.
pub fn name(text: &str) -> String {
    styled(TEXT, &format!("<b>{text}</b>"))
}

/// An item's name in a colour (rarity, state).
pub fn name_in(color: &str, text: &str) -> String {
    styled(color, &format!("<b>{text}</b>"))
}

/// Lore built line by line, in the theme's layout.
#[derive(Clone, Debug, Default)]
pub struct Lore(Vec<String>);

impl Lore {
    pub fn new() -> Self {
        Lore(Vec::new())
    }
    /// A plain description line (muted).
    pub fn text(mut self, text: &str) -> Self {
        self.0.push(styled(MUTED, text));
        self
    }
    /// A line of any MiniMessage, as given.
    pub fn raw(mut self, mini: &str) -> Self {
        self.0.push(format!("<!i>{mini}"));
        self
    }
    /// An empty line, to group the rest.
    pub fn blank(mut self) -> Self {
        self.0.push(String::new());
        self
    }
    /// `Price: $2,500` (name muted, value light).
    pub fn field(self, key: &str, value: &str) -> Self {
        self.field_in(key, value, TEXT)
    }
    /// A field with its value in a colour.
    pub fn field_in(mut self, key: &str, value: &str, color: &str) -> Self {
        self.0.push(format!("<!i><{DIM}>▪ <{MUTED}>{key}: <{color}>{value}"));
        self
    }
    /// A progress bar (`fraction` 0..1) with a label after it.
    pub fn bar(mut self, fraction: f64, label: &str) -> Self {
        self.0.push(format!("<!i>{} <{MUTED}>{label}", bar(fraction, 20)));
        self
    }
    /// `▸ Left-click to buy`: what a click does.
    pub fn click(mut self, button: &str, what: &str) -> Self {
        self.0.push(format!("<!i><{ACCENT}>▸ {button} <{MUTED}>{what}"));
        self
    }
    /// A line that warns (can't afford, locked...).
    pub fn warn(mut self, text: &str) -> Self {
        self.0.push(styled(BAD, &format!("✖ {text}")));
        self
    }
    /// A line that's good news (owned, unlocked...).
    pub fn good(mut self, text: &str) -> Self {
        self.0.push(styled(GOOD, &format!("✔ {text}")));
        self
    }
    /// A thin separator.
    pub fn rule(mut self) -> Self {
        self.0.push(styled("#3f3f46", "──────────────────"));
        self
    }
    pub fn lines(&self) -> &[String] {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `▮▮▮▮▮▯▯▯▯▯` as MiniMessage: the done part green, the rest grey.
pub fn bar(fraction: f64, cells: usize) -> String {
    let f = fraction.clamp(0.0, 1.0);
    let done = (f * cells as f64).round() as usize;
    format!("<{GOOD}>{}<#3f3f46>{}", "|".repeat(done), "|".repeat(cells.saturating_sub(done)))
}

/// Hides the tooltip's noise (attack damage, enchantments, dye, trims...), so a button shows only its text.
pub fn clean(stack: &mut ItemStack) {
    let hidden = ["minecraft:attribute_modifiers", "minecraft:enchantments", "minecraft:unbreakable", "minecraft:dyed_color", "minecraft:trim", "minecraft:stored_enchantments", "minecraft:potion_contents", "minecraft:jukebox_playable", "minecraft:banner_patterns", "minecraft:bundle_contents", "minecraft:container"];
    stack.set_component("minecraft:tooltip_display", json!({ "hide_tooltip": false, "hidden_components": hidden }));
}

fn material(name: &str) -> Material {
    Material::get_material(&name.to_ascii_uppercase()).unwrap_or_else(|| Material::of(name))
}

/// A themed item: a bold name, the lore, no tooltip noise.
pub fn item(material_name: &str, title: &str, lore: Lore) -> ItemStack {
    let mut s = ItemStack::new(material(material_name), 1);
    item::name(&mut s, &name(title));
    if !lore.is_empty() {
        item::lore(&mut s, &lore.0);
    }
    clean(&mut s);
    s
}

/// The same with the name in a colour.
pub fn item_in(material_name: &str, color: &str, title: &str, lore: Lore) -> ItemStack {
    let mut s = item(material_name, "", lore);
    item::name(&mut s, &name_in(color, title));
    s
}

/// A slot filler: the black pane with no tooltip at all.
pub fn filler() -> ItemStack {
    let mut s = ItemStack::new(Material::of("BLACK_STAINED_GLASS_PANE"), 1);
    s.set_component("minecraft:tooltip_display", json!({ "hide_tooltip": true }));
    s
}

/// "Back" (to the menu before).
pub fn back() -> ItemStack {
    item("ARROW", "Back", Lore::new().click("Click", "to go back"))
}

/// "Close".
pub fn close() -> ItemStack {
    item_in("BARRIER", BAD, "Close", Lore::new())
}

/// "Next page" (`page` counted from 1).
pub fn next(page: usize, pages: usize) -> ItemStack {
    item("SPECTRAL_ARROW", "Next Page", Lore::new().text(&format!("Page {page} of {pages}")).blank().click("Click", "for the next page"))
}

/// "Previous page".
pub fn previous(page: usize, pages: usize) -> ItemStack {
    item("SPECTRAL_ARROW", "Previous Page", Lore::new().text(&format!("Page {page} of {pages}")).blank().click("Click", "for the page before"))
}

/// A setting that's on or off: lime or grey dye, its state said in words.
pub fn toggle(on: bool, title: &str, description: &str) -> ItemStack {
    let lore = Lore::new().text(description).blank().field_in("Status", if on { "Enabled" } else { "Disabled" }, if on { GOOD } else { BAD }).blank().click("Click", if on { "to turn off" } else { "to turn on" });
    item(if on { "LIME_DYE" } else { "GRAY_DYE" }, title, lore)
}

/// "Confirm" (green).
pub fn confirm(title: &str, detail: &str) -> ItemStack {
    item_in("LIME_CONCRETE", GOOD, title, if detail.is_empty() { Lore::new() } else { Lore::new().text(detail) })
}

/// "Cancel" (red).
pub fn cancel(title: &str) -> ItemStack {
    item_in("RED_CONCRETE", BAD, title, Lore::new())
}

/// An information card (a book): a title and lines of text, no action.
pub fn info(title: &str, lines: &[&str]) -> ItemStack {
    let lore = lines.iter().fold(Lore::new(), |l, s| l.text(s));
    item("BOOK", title, lore)
}

/// "1,234,567"
pub fn number(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// "$1,234.50" (cents shown only when there are some).
pub fn money(amount: f64) -> String {
    let cents = (amount.abs() * 100.0).round() as i64;
    let whole = number(cents / 100);
    let sign = if amount < 0.0 { "-" } else { "" };
    if cents % 100 == 0 { format!("{sign}${whole}") } else { format!("{sign}${whole}.{:02}", cents % 100) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lore_layout() {
        let l = Lore::new().text("Hi").blank().field("Price", "$5").click("Left-click", "to buy");
        assert_eq!(l.lines(), &["<!i><#a1a1aa>Hi", "", "<!i><#71717a>▪ <#a1a1aa>Price: <#f4f4f5>$5", "<!i><#67e8f9>▸ Left-click <#a1a1aa>to buy"]);
    }

    #[test]
    fn numbers() {
        assert_eq!(number(1234567), "1,234,567");
        assert_eq!(number(-1000), "-1,000");
        assert_eq!(money(2500.0), "$2,500");
        assert_eq!(money(12.5), "$12.50");
        assert_eq!(bar(0.5, 4), "<#4ade80>||<#3f3f46>||");
    }
}
