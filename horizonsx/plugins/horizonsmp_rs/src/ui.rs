//! The look every part of HorizonSMP shares: one palette (Nyxora's purple and cyan, as HorizonBoard's), one way
//! to say things (chat line with a section tag, action bar, title, sound), one way to build a menu (dark frame,
//! close and back buttons in the same slots everywhere) and one way to print money and time.

use horizon_bukkit::adventure::Component;
use horizon_bukkit::{CommandSender, ItemStack, Material, Player};
use horizon_kit::{gui, item, mini, sound};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;

pub const ACCENT: &str = "#a855f7";
pub const ACCENT2: &str = "#22d3ee";

/// `<grad>Text</grad>` in the server's gradient.
pub fn grad(text: &str) -> String {
    format!("<gradient:{ACCENT}:{ACCENT2}>{text}</gradient>")
}

/// The chat tag of a section: `✦ Claims »`.
fn tag(section: &str) -> String {
    format!("<gradient:{ACCENT}:{ACCENT2}><bold>{section}</bold></gradient> <#3f3f46>» <#d4d4d8>")
}

pub fn component(text: &str) -> Component {
    Component(mini::parse_mixed(text))
}

/// One chat line (or several, split on `\n`) with the section's tag.
pub fn say(p: &Player, section: &str, text: &str) {
    let t = tag(section);
    for line in text.split('\n') {
        p.send_component(&component(&format!("{t}{line}")));
    }
}

/// Plain lines with no tag (panels).
pub fn raw(p: &Player, text: &str) {
    for line in text.split('\n') {
        p.send_component(&component(line));
    }
}

/// To any sender (console too).
pub fn tell(to: &CommandSender, section: &str, text: &str) {
    let t = tag(section);
    for line in text.split('\n') {
        to.send_component(&component(&format!("{t}{line}")));
    }
}

pub fn tell_raw(to: &CommandSender, text: &str) {
    for line in text.split('\n') {
        to.send_component(&component(line));
    }
}

pub fn bar(p: &Player, text: &str) {
    p.send_action_bar(&component(text));
}

pub fn title(p: &Player, title: &str, subtitle: &str) {
    p.show_title(&component(title), &component(subtitle), 8, 50, 14);
}

pub fn sound(p: &Player, spec: &str) {
    sound::play(p, spec);
}
pub fn click(p: &Player) {
    sound(p, "UI_BUTTON_CLICK:0.45:1.6");
}
pub fn good(p: &Player) {
    sound(p, "ENTITY_EXPERIENCE_ORB_PICKUP:0.7:1.2");
}
pub fn bad(p: &Player) {
    sound(p, "BLOCK_NOTE_BLOCK_BASS:0.7:0.6");
}
pub fn fanfare(p: &Player) {
    sound(p, "UI_TOAST_CHALLENGE_COMPLETE:0.6:1.0");
}

/// A "you can't" on the action bar with a low note, at most once a second a player (a held mouse button raises
/// the same event many times a second).
pub fn deny(p: &Player, text: &str) {
    static LAST: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
    let now = horizon_kit::time::now_ms();
    let uuid = p.get_unique_id();
    let fresh = {
        let mut held = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let map = held.get_or_insert_with(HashMap::new);
        let last = map.get(&uuid).copied().unwrap_or(0);
        if now.saturating_sub(last) >= 1000 {
            map.insert(uuid, now);
            true
        } else {
            false
        }
    };
    if fresh {
        bar(p, &format!("<#f87171>✖ <#fca5a5>{text}"));
        bad(p);
    }
}

// ---- Items ----

fn material(name: &str) -> Material {
    Material::get_material(&name.to_ascii_uppercase()).unwrap_or_else(|| Material::of("STONE"))
}

/// A menu icon: MiniMessage name and lore, italics off, attribute lines hidden.
pub fn icon(mat: &str, name: &str, lore: &[String]) -> ItemStack {
    let mut s = ItemStack::new(material(mat), 1);
    item::name(&mut s, name);
    if !lore.is_empty() {
        item::lore(&mut s, lore);
    }
    hide_extras(&mut s);
    s
}

pub fn hide_extras(s: &mut ItemStack) {
    s.set_component(
        "minecraft:tooltip_display",
        json!({ "hidden_components": ["minecraft:attribute_modifiers", "minecraft:enchantments", "minecraft:stored_enchantments", "minecraft:potion_contents", "minecraft:jukebox_playable", "minecraft:trim", "minecraft:dyed_color", "minecraft:banner_patterns", "minecraft:unbreakable"] }),
    );
}

pub fn glint(s: &mut ItemStack) {
    s.set_component("minecraft:enchantment_glint_override", json!(true));
}

pub fn amount(mut s: ItemStack, n: i32) -> ItemStack {
    s.set_amount(n.clamp(1, 99));
    s
}

/// A player's head with a name and lore.
pub fn head(owner: &str, name: &str, lore: &[String]) -> ItemStack {
    let mut s = item::head(owner);
    item::name(&mut s, name);
    if !lore.is_empty() {
        item::lore(&mut s, lore);
    }
    s
}

/// Lore lines from `&str`s.
pub fn lines(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| (*s).to_owned()).collect()
}

// ---- Menus ----

pub const CLOSE_SLOT_6: usize = 49;
pub const BACK_SLOT_6: usize = 45;

/// A menu with the dark frame every HorizonSMP menu has: black glass border, dark grey inside.
pub fn frame(rows: usize, title: &str) -> gui::Menu {
    let mut m = gui::Menu::new(rows, &format!("&8{title}"));
    m.fill_border(gui::filler("BLACK_STAINED_GLASS_PANE"));
    m
}

/// The close button (barrier) in the bottom middle.
pub fn close_button(m: &mut gui::Menu) {
    let slot = m.size() - 5;
    m.button(slot, icon("BARRIER", "<#f87171><bold>Close", &[]), |c| {
        click(&c.player);
        c.close();
    });
}

/// A back button (arrow) bottom left that opens `to`.
pub fn back_button(m: &mut gui::Menu, label: &str, to: fn(&Player)) {
    let slot = m.size() - 9;
    m.button(slot, icon("ARROW", "<#a1a1aa>← <white>Back", &[format!("<#71717a>To {label}")]), move |c| {
        click(&c.player);
        to(&c.player);
    });
}

/// Inner slots of a `rows` menu with a one-slot border (rows 1..rows-1, columns 1..7).
pub fn inner(rows: usize) -> Vec<usize> {
    (0..rows * 9).filter(|s| { let (r, c) = (s / 9, s % 9); r >= 1 && r + 1 < rows && (1..8).contains(&c) }).collect()
}

// ---- Numbers ----

/// `$1,234` / `$12.50` / `$1.2M`.
pub fn money(v: f64) -> String {
    let neg = v < 0.0;
    let a = v.abs();
    let s = if a >= 10_000_000.0 {
        format!("{:.1}M", a / 1_000_000.0)
    } else {
        let whole = a.trunc() as u64;
        let digits = whole.to_string();
        let mut g = String::new();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                g.push(',');
            }
            g.push(ch);
        }
        let cents = ((a.fract() * 100.0).round() as u64).min(99);
        if cents == 0 { g } else { format!("{g}.{cents:02}") }
    };
    format!("{}${s}", if neg { "-" } else { "" })
}

/// `1,234`.
pub fn num(v: i64) -> String {
    let digits = v.unsigned_abs().to_string();
    let mut g = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            g.push(',');
        }
        g.push(ch);
    }
    if v < 0 { format!("-{g}") } else { g }
}

/// `2d 4h`, `3h 12m`, `5m 3s`, `12s`.
pub fn duration(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// A bar of `n` cells, `filled` of 0..1 lit: `▮▮▮▮▯▯`.
pub fn meter(filled: f64, n: usize, on: &str, off: &str) -> String {
    let lit = ((filled.clamp(0.0, 1.0) * n as f64).round() as usize).min(n);
    format!("<{on}>{}<{off}>{}", "▮".repeat(lit), "▮".repeat(n - lit))
}

/// `Diamond Ore` from `DIAMOND_ORE`.
pub fn pretty(material: &str) -> String {
    material
        .trim_start_matches("minecraft:")
        .split(['_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let lower = w.to_ascii_lowercase();
            let mut c = lower.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escapes text a player typed so MiniMessage leaves it as it is.
pub fn esc(text: &str) -> String {
    text.replace('<', "\\<")
}

/// A small fast random number source (xorshift), seeded from the clock.
pub fn rand_u64() -> u64 {
    static STATE: Mutex<u64> = Mutex::new(0);
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if *s == 0 {
        *s = horizon_kit::time::now_ms() ^ 0x9E37_79B9_7F4A_7C15 | 1;
    }
    let mut x = *s;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *s = x;
    x
}

pub fn rand_f64() -> f64 {
    (rand_u64() >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbers() {
        assert_eq!(money(1234.0), "$1,234");
        assert_eq!(money(12.5), "$12.50");
        assert_eq!(money(25_000_000.0), "$25.0M");
        assert_eq!(num(1234567), "1,234,567");
        assert_eq!(duration(3725), "1h 2m");
        assert_eq!(duration(59), "59s");
        assert_eq!(pretty("DIAMOND_ORE"), "Diamond Ore");
        assert_eq!(inner(6).len(), 28);
        assert_eq!(inner(3), (10..17).collect::<Vec<_>>());
    }
}
