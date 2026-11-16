//! Items as plugin configs describe them, and items saved as text.
//!
//! - [`Spec::from_yaml`]: one item section in any of the usual spellings (DeluxeMenus, zMenu, crates, shops):
//!   `material`/`type`/`item`/`id`, `amount`, `name`/`display_name`/`display-name`, `lore`, `model_data`/
//!   `custom-model-data`, `enchantments` (list `NAME:LVL` or map), `flags`/`item_flags`/`hide_*`, `glow`,
//!   `unbreakable`, heads (`head-<name>`, `basehead-<b64>`, `texture-<hash>`, `url:`/`texture:`/`skull-owner:`),
//!   `rgb`/`color`, `item_model`, `rarity`, `tooltip_style`, `hide_tooltip`, `nbt_string`/`nbt_int`.
//!   [`Spec::build`] makes the stack with your placeholder filler (`%player%`, `{price}`...).
//! - [`to_base64`]/[`from_base64`] (and the `items_` versions): an item saved whole (Paper's `serializeAsBytes`)
//!   as text for JSON/YAML files. These are engine queries: call them on the server thread.
//! - [`name`]/[`lore`]: names and lore as text components (hex, gradients, not italic), [`head`] for heads.

use crate::yaml;
use horizon_bukkit::{ItemStack, Material};
use serde_json::{Map, Value, json};
use serde_yaml::Value as Yaml;

// ---- base64 ----

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// One item as base64 text (`serializeAsBytes`); empty text for no item.
pub fn to_base64(stack: Option<&ItemStack>) -> String {
    stack.filter(|s| !s.get_type().is_air()).map_or_else(String::new, |s| b64_encode(&s.serialize_as_bytes()))
}

pub fn from_base64(text: &str) -> Option<ItemStack> {
    if text.is_empty() {
        return None;
    }
    ItemStack::deserialize_bytes(&b64_decode(text)?)
}

/// Many items as one base64 text (`serializeItemsAsBytes`).
pub fn items_to_base64(stacks: &[Option<ItemStack>]) -> String {
    b64_encode(&ItemStack::serialize_items_as_bytes(stacks))
}

pub fn items_from_base64(text: &str) -> Vec<Option<ItemStack>> {
    b64_decode(text).map(|b| ItemStack::deserialize_items_from_bytes(&b)).unwrap_or_default()
}

// ---- Names, lore, heads ----

/// A coloured text (`&`, hex, `<#a>..</#b>`, MiniMessage) as a component that isn't italic (item text is by
/// default).
pub fn text_component(text: &str) -> Value {
    let mut v = crate::mini::parse_mixed(&crate::text::colorize(text));
    if let Some(o) = v.as_object_mut()
        && !o.contains_key("italic")
    {
        o.insert("italic".into(), json!(false));
    }
    v
}

pub fn name(stack: &mut ItemStack, text: &str) {
    stack.set_component("minecraft:custom_name", text_component(text));
}

/// Lore lines (`\n` inside a line splits it, as plugins expect).
pub fn lore(stack: &mut ItemStack, lines: &[String]) {
    let comps: Vec<Value> = lines.iter().flat_map(|l| l.split('\n').map(str::to_owned).collect::<Vec<_>>()).map(|l| text_component(&l)).collect();
    stack.set_component("minecraft:lore", Value::Array(comps));
}

/// A player head: by name (`Notch`), by base64 texture (`eyJ0...`), or by texture hash/URL.
pub fn head(owner_or_texture: &str) -> ItemStack {
    let mut stack = ItemStack::new(Material::of("PLAYER_HEAD"), 1);
    let t = owner_or_texture.trim();
    let textures = |value: String| json!({ "properties": [{ "name": "textures", "value": value }] });
    let profile = if t.len() > 40 && t.starts_with("ey") {
        textures(t.to_owned())
    } else if t.starts_with("http") || (t.len() >= 40 && t.chars().all(|c| c.is_ascii_hexdigit())) {
        let url = if t.starts_with("http") { t.to_owned() } else { format!("http://textures.minecraft.net/texture/{t}") };
        textures(b64_encode(format!("{{\"textures\":{{\"SKIN\":{{\"url\":\"{url}\"}}}}}}").as_bytes()))
    } else {
        json!({ "name": t })
    };
    stack.set_component("minecraft:profile", profile);
    stack
}

/// `Enchantment.getByName` names (old Bukkit ones too) or keys -> the game's id.
pub fn enchantment_key(name: &str) -> String {
    let up = name.trim().to_ascii_uppercase();
    let modern = match up.as_str() {
        "PROTECTION_ENVIRONMENTAL" => "protection",
        "PROTECTION_FIRE" => "fire_protection",
        "PROTECTION_FALL" => "feather_falling",
        "PROTECTION_EXPLOSIONS" => "blast_protection",
        "PROTECTION_PROJECTILE" => "projectile_protection",
        "OXYGEN" => "respiration",
        "WATER_WORKER" => "aqua_affinity",
        "DAMAGE_ALL" => "sharpness",
        "DAMAGE_UNDEAD" => "smite",
        "DAMAGE_ARTHROPODS" => "bane_of_arthropods",
        "LOOT_BONUS_MOBS" => "looting",
        "DIG_SPEED" => "efficiency",
        "DURABILITY" => "unbreaking",
        "LOOT_BONUS_BLOCKS" => "fortune",
        "ARROW_DAMAGE" => "power",
        "ARROW_KNOCKBACK" => "punch",
        "ARROW_FIRE" => "flame",
        "ARROW_INFINITE" => "infinity",
        "LUCK" => "luck_of_the_sea",
        "SWEEPING_EDGE" | "SWEEPING" => "sweeping_edge",
        _ => "",
    };
    let id = if modern.is_empty() { up.to_ascii_lowercase() } else { modern.to_owned() };
    if id.contains(':') { id } else { format!("minecraft:{id}") }
}

/// `PotionEffectType.getByName` names (old ones too) -> the game's id.
pub fn effect_key(name: &str) -> String {
    let modern = match name.trim().to_ascii_uppercase().as_str() {
        "SLOW" => "slowness",
        "FAST_DIGGING" => "haste",
        "SLOW_DIGGING" => "mining_fatigue",
        "INCREASE_DAMAGE" => "strength",
        "HEAL" => "instant_health",
        "HARM" => "instant_damage",
        "JUMP" => "jump_boost",
        "CONFUSION" => "nausea",
        "DAMAGE_RESISTANCE" => "resistance",
        _ => "",
    };
    let id = if modern.is_empty() { name.trim().to_ascii_lowercase() } else { modern.to_owned() };
    if id.contains(':') { id } else { format!("minecraft:{id}") }
}

/// Item flags -> the components the tooltip hides.
pub fn hidden_by(flag: &str) -> &'static [&'static str] {
    match flag.trim().to_ascii_uppercase().as_str() {
        "HIDE_ATTRIBUTES" => &["minecraft:attribute_modifiers"],
        "HIDE_ENCHANTS" => &["minecraft:enchantments"],
        "HIDE_UNBREAKABLE" => &["minecraft:unbreakable"],
        "HIDE_DESTROYS" => &["minecraft:can_break"],
        "HIDE_PLACED_ON" => &["minecraft:can_place_on"],
        "HIDE_DYE" => &["minecraft:dyed_color"],
        "HIDE_ARMOR_TRIM" => &["minecraft:trim"],
        "HIDE_STORED_ENCHANTS" => &["minecraft:stored_enchantments"],
        "HIDE_ADDITIONAL_TOOLTIP" | "HIDE_POTION_EFFECTS" => &[
            "minecraft:potion_contents", "minecraft:banner_patterns", "minecraft:written_book_content", "minecraft:fireworks", "minecraft:firework_explosion",
            "minecraft:jukebox_playable", "minecraft:container", "minecraft:bundle_contents", "minecraft:map_id", "minecraft:instrument", "minecraft:stored_enchantments",
        ],
        _ => &[],
    }
}

fn rgb(text: &str) -> Option<i64> {
    let t = text.trim().trim_start_matches('#');
    if t.len() == 6
        && let Ok(n) = i64::from_str_radix(t, 16)
    {
        return Some(n);
    }
    let parts: Vec<i64> = t.split(',').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    (parts.len() >= 3).then(|| ((parts[0] & 255) << 16) | ((parts[1] & 255) << 8) | (parts[2] & 255))
}

fn key_of(text: &str) -> Option<String> {
    let t = text.trim().to_ascii_lowercase();
    if t.is_empty() || t.contains(' ') {
        return None;
    }
    Some(if t.contains(':') { t } else { format!("minecraft:{t}") })
}

/// An item as a config describes it. Every text field keeps its placeholders until [`Spec::build`].
#[derive(Clone, Debug, Default)]
pub struct Spec {
    pub material: String,
    pub amount: Option<String>,
    pub name: Option<String>,
    pub lore: Vec<String>,
    pub model_data: Option<String>,
    pub enchantments: Vec<(String, u32)>,
    pub flags: Vec<String>,
    pub glow: Option<String>,
    pub unbreakable: bool,
    /// Head: owner name, base64 texture, hash or URL.
    pub head: Option<String>,
    pub rgb: Option<String>,
    pub item_model: Option<String>,
    pub rarity: Option<String>,
    pub tooltip_style: Option<String>,
    pub hide_tooltip: bool,
    pub damage: Option<String>,
    /// `custom_data` entries: (key, value text, is number).
    pub nbt: Vec<(String, String, bool)>,
}

impl Spec {
    /// Reads one item section; the first key present wins for each field.
    pub fn from_yaml(v: &Yaml) -> Spec {
        let first = |keys: &[&str]| keys.iter().find_map(|k| yaml::string(v, k));
        let mut s = Spec {
            material: first(&["material", "type", "item", "id", "icon"]).unwrap_or_else(|| "STONE".into()),
            amount: first(&["amount", "count", "dynamic_amount"]),
            name: first(&["display_name", "display-name", "displayName", "name"]),
            lore: ["lore", "description"].iter().map(|k| yaml::list_or_one(v, k)).find(|l| !l.is_empty()).unwrap_or_default(),
            model_data: first(&["model_data", "custom-model-data", "custom_model_data", "modelData", "model-data", "customModelData"]),
            glow: first(&["glow", "glowing", "enchantment_glint_override", "glint"]),
            unbreakable: ["unbreakable"].iter().any(|k| yaml::boolean(v, k, false)),
            head: first(&["texture", "url", "skull-owner", "skull_owner", "owner", "head", "base64"]),
            rgb: first(&["rgb", "color", "colour", "dye"]),
            item_model: first(&["item_model", "item-model", "itemModel"]),
            rarity: first(&["rarity"]),
            tooltip_style: first(&["tooltip_style", "tooltip-style"]),
            hide_tooltip: yaml::boolean(v, "hide_tooltip", false) || yaml::boolean(v, "hide-tooltip", false),
            damage: first(&["damage", "durability", "data"]),
            ..Default::default()
        };
        // Enchantments: a list of `NAME:LVL`/`NAME;LVL` or a map NAME: LVL.
        for k in ["enchantments", "enchants", "enchantment"] {
            if yaml::is_section(v, k) {
                for e in yaml::keys(v, k) {
                    s.enchantments.push((enchantment_key(&e), yaml::int(v, &format!("{k}.{e}"), 1).max(1) as u32));
                }
            } else {
                for e in yaml::list(v, k) {
                    let (n, l) = e.split_once([':', ';', ' ']).unwrap_or((&e, "1"));
                    s.enchantments.push((enchantment_key(n), l.trim().parse().unwrap_or(1)));
                }
            }
        }
        for k in ["item_flags", "flags", "itemflags", "item-flags"] {
            s.flags.extend(yaml::list_or_one(v, k));
        }
        for (k, flag) in [("hide_attributes", "HIDE_ATTRIBUTES"), ("hide_enchantments", "HIDE_ENCHANTS"), ("hide_enchants", "HIDE_ENCHANTS"), ("hide_unbreakable", "HIDE_UNBREAKABLE"), ("hide_effects", "HIDE_ADDITIONAL_TOOLTIP")] {
            if yaml::boolean(v, k, false) {
                s.flags.push(flag.into());
            }
        }
        for (k, num) in [("nbt_string", false), ("nbt_int", true)] {
            for e in yaml::list_or_one(v, k).into_iter().chain(yaml::list_or_one(v, &format!("{k}s"))) {
                if let Some((a, b)) = e.split_once(':') {
                    s.nbt.push((a.to_owned(), b.to_owned(), num));
                }
            }
        }
        s
    }

    /// The stack, placeholders filled by `fill`; `None` for air.
    pub fn build(&self, fill: &dyn Fn(&str) -> String) -> Option<ItemStack> {
        let material = fill(&self.material);
        let lower = material.to_ascii_lowercase();
        let mut stack = if let Some(rest) = lower.strip_prefix("basehead-").or_else(|| lower.strip_prefix("texture-")).or_else(|| lower.strip_prefix("head-")) {
            head(&material[material.len() - rest.len()..])
        } else if let Some(h) = &self.head
            && (lower == "player_head" || lower == "skull" || lower.contains("head"))
        {
            head(&fill(h))
        } else {
            let m = Material::get_material(&material.to_ascii_uppercase().replace([' ', '-'], "_")).unwrap_or_else(|| Material::of("STONE"));
            if m.is_air() {
                return None;
            }
            ItemStack::new(m, 1)
        };
        let mat = stack.get_type().key().to_owned();
        if let Some(a) = &self.amount
            && let Ok(n) = fill(a).trim().parse::<f64>()
        {
            stack.set_amount((n as i32).clamp(1, 99));
        }
        if let Some(n) = &self.name {
            name(&mut stack, &fill(n));
        }
        if !self.lore.is_empty() {
            let lines: Vec<String> = self.lore.iter().map(|l| fill(l)).collect();
            lore(&mut stack, &lines);
        }
        if let Some(md) = &self.model_data
            && let Ok(n) = fill(md).trim().parse::<i32>()
        {
            stack.set_custom_model_data(Some(n));
        }
        if let Some(d) = &self.damage
            && let Ok(n) = fill(d).trim().parse::<i32>()
            && n > 0
        {
            stack.set_damage(n);
        }
        if !self.enchantments.is_empty() {
            if mat == "minecraft:enchanted_book" {
                let levels: Map<String, Value> = self.enchantments.iter().map(|(k, l)| (k.clone(), json!(l))).collect();
                stack.set_component("minecraft:stored_enchantments", Value::Object(levels));
            } else {
                for (k, l) in &self.enchantments {
                    stack.add_enchant(k, *l);
                }
            }
        }
        if let Some(g) = &self.glow {
            let on = fill(g).trim().eq_ignore_ascii_case("true");
            stack.set_component("minecraft:enchantment_glint_override", json!(on));
        }
        if self.unbreakable {
            stack.set_unbreakable(true);
        }
        if let Some(c) = self.rgb.as_ref().and_then(|c| rgb(&fill(c))) {
            if mat.contains("potion") || mat == "minecraft:tipped_arrow" {
                stack.set_component("minecraft:potion_contents", json!({ "custom_color": c }));
            } else if mat.starts_with("minecraft:leather_") || mat == "minecraft:wolf_armor" {
                stack.set_component("minecraft:dyed_color", json!(c));
            }
        }
        if let Some(k) = self.item_model.as_ref().and_then(|t| key_of(&fill(t))) {
            stack.set_component("minecraft:item_model", json!(k));
        }
        if let Some(r) = &self.rarity {
            let r = fill(r).to_ascii_lowercase();
            if matches!(r.as_str(), "common" | "uncommon" | "rare" | "epic") {
                stack.set_component("minecraft:rarity", json!(r));
            }
        }
        if let Some(k) = self.tooltip_style.as_ref().and_then(|t| key_of(&fill(t))) {
            stack.set_component("minecraft:tooltip_style", json!(k));
        }
        let hidden: Vec<&str> = self.flags.iter().flat_map(|f| hidden_by(f).iter().copied()).collect();
        if !hidden.is_empty() || self.hide_tooltip {
            let mut d = Map::new();
            if self.hide_tooltip {
                d.insert("hide_tooltip".into(), json!(true));
            }
            if !hidden.is_empty() {
                d.insert("hidden_components".into(), json!(hidden));
            }
            stack.set_component("minecraft:tooltip_display", Value::Object(d));
        }
        if !self.nbt.is_empty() {
            let mut data = stack.get_component("minecraft:custom_data").and_then(Value::as_object).cloned().unwrap_or_default();
            for (k, v, num) in &self.nbt {
                let v = fill(v);
                let value = if *num { v.trim().parse::<i64>().map_or(Value::Null, |n| json!(n)) } else { json!(v) };
                if !value.is_null() {
                    data.insert(k.clone(), value);
                }
            }
            stack.set_component("minecraft:custom_data", Value::Object(data));
        }
        Some(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_and_specs() {
        for s in ["", "a", "ab", "abc", "hello world"] {
            assert_eq!(b64_decode(&b64_encode(s.as_bytes())).unwrap(), s.as_bytes());
        }
        assert_eq!(b64_encode(b"Man"), "TWFu");
        let y: Yaml = serde_yaml::from_str("material: DIAMOND_SWORD\ndisplay_name: '&bBlade'\nlore: ['a', 'b']\nenchantments: ['DAMAGE_ALL:5']\nhide_attributes: true\nmodel_data: 7").unwrap();
        let s = Spec::from_yaml(&y);
        assert_eq!(s.material, "DIAMOND_SWORD");
        assert_eq!(s.enchantments, vec![("minecraft:sharpness".to_owned(), 5)]);
        assert_eq!(s.flags, vec!["HIDE_ATTRIBUTES".to_owned()]);
        assert_eq!(s.model_data.as_deref(), Some("7"));
        assert_eq!(text_component("&cX")["italic"], json!(false));
        assert_eq!(enchantment_key("DURABILITY"), "minecraft:unbreaking");
    }
}

/// The item draws with the pack's model `id` (`myplugin:coin`): any picture for any item.
pub fn model(stack: &mut ItemStack, id: &str) {
    stack.set_item_model(id);
}

/// Forces the enchantment shine on or off.
pub fn glint(stack: &mut ItemStack, on: bool) {
    stack.set_enchantment_glint_override(on);
}
