//! The look: the server's purple and cyan for chat, each job's own colour in its menus, small caps for the
//! tooltip labels (as the job cards: "PLAYER JOB", "[CLICK HERE TO JOIN THIS JOB]"), and one way to print money.

use horizon_bukkit::adventure::Component;
use horizon_bukkit::{CommandSender, ItemStack, Material, Player};
use horizon_kit::{gui, item, mini, sound};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;

pub const ACCENT: &str = "#a855f7";
pub const ACCENT2: &str = "#22d3ee";
/// Tooltip text colours: body, label grey, dim grey.
pub const BODY: &str = "#d4d4d8";
pub const GREY: &str = "#a1a1aa";
pub const DIM: &str = "#71717a";

fn tag() -> String {
    format!("<gradient:{ACCENT}:{ACCENT2}><bold>Jobs</bold></gradient> <#3f3f46>» <{BODY}>")
}

pub fn component(text: &str) -> Component {
    Component(mini::parse_mixed(text))
}

pub fn say(p: &Player, text: &str) {
    let t = tag();
    for line in text.split('\n') {
        p.send_component(&component(&format!("{t}{line}")));
    }
}

pub fn raw(p: &Player, text: &str) {
    for line in text.split('\n') {
        p.send_component(&component(line));
    }
}

pub fn tell(to: &CommandSender, text: &str) {
    let t = tag();
    for line in text.split('\n') {
        to.send_component(&component(&format!("{t}{line}")));
    }
}

pub fn bar(p: &Player, text: &str) {
    p.send_action_bar(&component(text));
}

pub fn title(p: &Player, title: &str, subtitle: &str) {
    p.show_title(&component(title), &component(subtitle), 6, 50, 14);
}

pub fn click(p: &Player) {
    sound::play(p, "UI_BUTTON_CLICK:0.45:1.6");
}
pub fn good(p: &Player) {
    sound::play(p, "ENTITY_EXPERIENCE_ORB_PICKUP:0.7:1.2");
}
pub fn bad(p: &Player) {
    sound::play(p, "BLOCK_NOTE_BLOCK_BASS:0.7:0.6");
}
pub fn levelup(p: &Player) {
    sound::play(p, "ENTITY_PLAYER_LEVELUP:0.8:1.0");
}

/// A "you can't" on the action bar with a low note, at most once a second a player.
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

// ---- Text ----

/// `PLAYER JOB` -> `ᴘʟᴀʏᴇʀ ᴊᴏʙ` (the small capitals the job cards use).
pub fn small(text: &str) -> String {
    text.chars()
        .map(|c| match c.to_ascii_lowercase() {
            'a' => 'ᴀ',
            'b' => 'ʙ',
            'c' => 'ᴄ',
            'd' => 'ᴅ',
            'e' => 'ᴇ',
            'f' => 'ꜰ',
            'g' => 'ɢ',
            'h' => 'ʜ',
            'i' => 'ɪ',
            'j' => 'ᴊ',
            'k' => 'ᴋ',
            'l' => 'ʟ',
            'm' => 'ᴍ',
            'n' => 'ɴ',
            'o' => 'ᴏ',
            'p' => 'ᴘ',
            'q' => 'ǫ',
            'r' => 'ʀ',
            's' => 's',
            't' => 'ᴛ',
            'u' => 'ᴜ',
            'v' => 'ᴠ',
            'w' => 'ᴡ',
            'x' => 'x',
            'y' => 'ʏ',
            'z' => 'ᴢ',
            _ => c,
        })
        .collect()
}

/// `1100.0`, `12.5`: one decimal, as the cards show exp.
pub fn exp(v: f64) -> String {
    format!("{:.1}", (v * 10.0).floor() / 10.0)
}

/// `$1,234` / `$12.50` / `$1.2M`.
pub fn money(v: f64) -> String {
    let neg = v < 0.0;
    let a = v.abs();
    let s = if a >= 10_000_000.0 {
        format!("{:.1}M", a / 1_000_000.0)
    } else {
        let whole = a.trunc() as u64;
        let cents = ((a.fract() * 100.0).round() as u64).min(99);
        let g = num(whole as i64);
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

/// `Diamond Ore` from `deepslate_diamond_ore`'s kind: `DIAMOND_ORE` -> `Diamond Ore`.
pub fn pretty(id: &str) -> String {
    id.trim_start_matches("minecraft:")
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

/// A bar of `n` cells, `filled` of 0..1 lit.
pub fn meter(filled: f64, n: usize, on: &str, off: &str) -> String {
    let lit = ((filled.clamp(0.0, 1.0) * n as f64).round() as usize).min(n);
    format!("<{on}>{}<{off}>{}", "▮".repeat(lit), "▮".repeat(n - lit))
}

// ---- Items ----

pub fn material_ok(name: &str) -> bool {
    Material::get_material(&name.to_ascii_uppercase()).is_some_and(|m| m.is_item())
}

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
        json!({ "hidden_components": ["minecraft:attribute_modifiers", "minecraft:enchantments", "minecraft:unbreakable", "minecraft:dyed_color", "minecraft:potion_contents"] }),
    );
}

pub fn glint(s: &mut ItemStack) {
    s.set_component("minecraft:enchantment_glint_override", json!(true));
}

pub fn head(owner: &str, name: &str, lore: &[String]) -> ItemStack {
    let mut s = item::head(owner);
    item::name(&mut s, name);
    if !lore.is_empty() {
        item::lore(&mut s, lore);
    }
    s
}

// ---- Menus ----

/// The dark frame every jobs menu has.
pub fn frame(rows: usize, title: &str) -> gui::Menu {
    let mut m = gui::Menu::new(rows, &format!("&8{title}"));
    m.fill_border(gui::filler("BLACK_STAINED_GLASS_PANE"));
    m
}

pub fn close_button(m: &mut gui::Menu) {
    let slot = m.size() - 5;
    m.button(slot, icon("BARRIER", "<#f87171><bold>Close", &[]), |c| {
        click(&c.player);
        c.close();
    });
}

pub fn back_button(m: &mut gui::Menu, label: &str, to: fn(&Player)) {
    let slot = m.size() - 9;
    m.button(slot, icon("ARROW", "<#a1a1aa>← <white>Back", &[format!("<{DIM}>To {label}")]), move |c| {
        click(&c.player);
        to(&c.player);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text() {
        assert_eq!(small("PLAYER JOB"), "ᴘʟᴀʏᴇʀ ᴊᴏʙ");
        assert_eq!(exp(0.0), "0.0");
        assert_eq!(exp(1100.0), "1100.0");
        assert_eq!(exp(12.56), "12.5");
        assert_eq!(money(1234.5), "$1,234.50");
        assert_eq!(pretty("deepslate_coal_ore"), "Deepslate Coal Ore");
    }
}
