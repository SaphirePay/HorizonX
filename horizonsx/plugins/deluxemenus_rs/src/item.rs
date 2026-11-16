//! `MenuItem.getItemStack`: a menu item's stack for one viewer, placeholders and arguments filled in.

use crate::Holder;
use crate::menu::Item;
use base64::Engine;
use horizon_bukkit::{ItemStack, Material};
use horizon_kit::text::colorize;
use serde_json::{Map, Value, json};

/// `Enchantment.getByName` (Bukkit's old names) or a key, to the game's id.
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

/// `PotionEffectType.getByName` (old names too) to the game's id.
fn effect_key(name: &str) -> String {
    let modern = match name.to_ascii_uppercase().as_str() {
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
    let id = if modern.is_empty() { name.to_ascii_lowercase() } else { modern.to_owned() };
    if id.contains(':') { id } else { format!("minecraft:{id}") }
}

fn rgb(text: &str) -> Option<i64> {
    let parts: Vec<i64> = text.split(',').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    (parts.len() >= 3).then(|| ((parts[0] & 255) << 16) | ((parts[1] & 255) << 8) | (parts[2] & 255))
}

fn key_of(text: &str) -> Option<String> {
    let t = text.trim().to_ascii_lowercase();
    if t.is_empty() || t.contains(' ') {
        return None;
    }
    Some(if t.contains(':') { t } else { format!("minecraft:{t}") })
}

/// The lore lines, filled, coloured and split on new lines (`getMenuItemLore`).
pub fn lore_of(h: &Holder, lore: &[String]) -> Vec<String> {
    lore.iter().map(|l| colorize(&h.fill(l))).flat_map(|l| l.split('\n').flat_map(|p| p.split("\\n").map(str::to_owned).collect::<Vec<_>>()).collect::<Vec<_>>()).collect()
}

fn head(profile: Value) -> ItemStack {
    let mut stack = ItemStack::new(Material::of("PLAYER_HEAD"), 1);
    stack.set_component("minecraft:profile", profile);
    stack
}

fn textures(value: String) -> Value {
    json!({ "properties": [{ "name": "textures", "value": value }] })
}

/// Item flags -> the components hidden from the tooltip.
fn hidden_by(flag: &str) -> &'static [&'static str] {
    match flag {
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

/// The stack, or `None` for an empty slot (a player item that isn't there).
pub fn build(item: &Item, h: &Holder) -> Option<ItemStack> {
    let mut material = item.material.clone();
    let mut lower = material.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("placeholder-") {
        material = h.fill(&material[material.len() - rest.len()..]);
        lower = material.to_ascii_lowercase();
    }
    let mut amount = 1i64;
    let mut stack: Option<ItemStack> = None;
    let player_item = match lower.as_str() {
        "main_hand" | "off_hand" | "armor_helmet" | "armor_chestplate" | "armor_leggings" | "armor_boots" => true,
        _ => false,
    };
    if player_item {
        let inv = h.viewer.get_inventory();
        let got = match lower.as_str() {
            "main_hand" => inv.get_item_in_main_hand(),
            "off_hand" => inv.get_item_in_off_hand(),
            "armor_helmet" => inv.get_helmet(),
            "armor_chestplate" => inv.get_chestplate(),
            "armor_leggings" => inv.get_leggings(),
            _ => inv.get_boots(),
        };
        let got = got.filter(|s| !s.is_empty())?;
        amount = i64::from(got.get_amount());
        stack = Some(got);
    } else if let Some(rest) = lower.strip_prefix("basehead-") {
        stack = Some(head(textures(h.fill(&material[material.len() - rest.len()..]))));
    } else if let Some(rest) = lower.strip_prefix("texture-") {
        let hash = h.fill(&material[material.len() - rest.len()..]);
        let doc = format!("{{\"textures\":{{\"SKIN\":{{\"url\":\"http://textures.minecraft.net/texture/{hash}\"}}}}}}");
        stack = Some(head(textures(base64::engine::general_purpose::STANDARD.encode(doc))));
    } else if let Some(rest) = lower.strip_prefix("head-") {
        let name = h.fill(&material[material.len() - rest.len()..]);
        stack = Some(head(json!({ "name": name })));
    } else if lower == "water_bottle" {
        let mut s = ItemStack::new(Material::of("POTION"), 1);
        s.set_component("minecraft:potion_contents", json!({ "potion": "minecraft:water" }));
        stack = Some(s);
    }
    let mut stack = stack.unwrap_or_else(|| ItemStack::new(Material::get_material(&material.to_ascii_uppercase()).unwrap_or_else(|| Material::of("STONE")), 1));
    if stack.get_type().is_air() {
        return None;
    }
    let mat = stack.get_type().key().to_owned();
    if mat.ends_with("_banner") && !item.banner.is_empty() {
        let patterns: Vec<Value> = item.banner.iter().map(|(color, pattern)| json!({ "color": color, "pattern": format!("minecraft:{pattern}") })).collect();
        stack.set_component("minecraft:banner_patterns", Value::Array(patterns));
    }
    if mat == "minecraft:shield" {
        if let Some(base) = &item.base_color {
            stack.set_component("minecraft:base_color", json!(base));
        }
        if !item.banner.is_empty() {
            let patterns: Vec<Value> = item.banner.iter().map(|(color, pattern)| json!({ "color": color, "pattern": format!("minecraft:{pattern}") })).collect();
            stack.set_component("minecraft:banner_patterns", Value::Array(patterns));
        }
    }
    let potion = matches!(mat.as_str(), "minecraft:potion" | "minecraft:splash_potion" | "minecraft:lingering_potion" | "minecraft:tipped_arrow");
    if potion {
        let mut contents = stack.get_component("minecraft:potion_contents").and_then(Value::as_object).cloned().unwrap_or_default();
        if let Some(color) = item.rgb.as_ref().and_then(|c| rgb(&h.fill(c))) {
            contents.insert("custom_color".into(), json!(color));
        }
        if !item.potions.is_empty() {
            let effects: Vec<Value> = item.potions.iter().map(|(id, dur, amp)| json!({ "id": effect_key(id), "duration": dur, "amplifier": amp })).collect();
            contents.insert("custom_effects".into(), Value::Array(effects));
        }
        if !contents.is_empty() {
            stack.set_component("minecraft:potion_contents", Value::Object(contents));
        }
    }
    if let Some(d) = &item.damage
        && let Ok(damage) = h.fill(d).trim().parse::<i32>()
        && damage > 0
    {
        stack.set_damage(damage);
    }
    if item.amount != -1 {
        amount = item.amount;
    }
    if let Some(d) = &item.dynamic_amount
        && let Ok(n) = h.fill(d).trim().parse::<f64>()
    {
        amount = (n as i64).max(1);
    }
    stack.set_amount(amount.min(64) as i32);
    if let Some(md) = &item.model_data
        && let Ok(n) = h.fill(md).trim().parse::<i32>()
    {
        stack.set_custom_model_data(Some(n));
    }
    if let Some((colors, flags, floats, strings)) = &item.model_component {
        let mut cmd = Map::new();
        let colors: Vec<i64> = colors.iter().filter_map(|c| rgb(&h.fill(c))).collect();
        if !colors.is_empty() {
            cmd.insert("colors".into(), json!(colors));
        }
        if !flags.is_empty() {
            cmd.insert("flags".into(), json!(flags.iter().map(|f| h.fill(f).eq_ignore_ascii_case("true")).collect::<Vec<_>>()));
        }
        let floats: Vec<f64> = floats.iter().filter_map(|f| h.fill(f).trim().parse().ok()).collect();
        if !floats.is_empty() {
            cmd.insert("floats".into(), json!(floats));
        }
        if !strings.is_empty() {
            cmd.insert("strings".into(), json!(strings.iter().map(|s| h.fill(s)).collect::<Vec<_>>()));
        }
        if !cmd.is_empty() {
            stack.set_component("minecraft:custom_model_data", Value::Object(cmd));
        }
    }
    if let Some(name) = &item.display_name {
        stack.set_display_name(&colorize(&h.fill(name)));
    }
    let own = stack.get_lore();
    let mode = item.lore_append.clone().unwrap_or_else(|| if item.has_lore { "OVERRIDE".into() } else { "IGNORE".into() });
    let lore = match mode.as_str() {
        "IGNORE" => own,
        "TOP" => lore_of(h, &item.lore).into_iter().chain(own).collect(),
        "BOTTOM" => own.into_iter().chain(lore_of(h, &item.lore)).collect(),
        _ => lore_of(h, &item.lore),
    };
    stack.set_lore(lore);
    if item.unbreakable {
        stack.set_unbreakable(true);
    }
    let mut display = Map::new();
    if let Some(t) = &item.hide_tooltip {
        display.insert("hide_tooltip".into(), json!(h.fill(t).eq_ignore_ascii_case("true")));
    }
    if let Some(g) = &item.glint {
        stack.set_component("minecraft:enchantment_glint_override", json!(h.fill(g).eq_ignore_ascii_case("true")));
    }
    if let Some(r) = &item.rarity {
        let r = h.fill(r).to_ascii_lowercase();
        if matches!(r.as_str(), "common" | "uncommon" | "rare" | "epic") {
            stack.set_component("minecraft:rarity", json!(r));
        }
    }
    if let Some(key) = item.tooltip_style.as_ref().and_then(|t| key_of(&h.fill(t))) {
        stack.set_component("minecraft:tooltip_style", json!(key));
    }
    if let Some(key) = item.item_model.as_ref().and_then(|t| key_of(&h.fill(t))) {
        stack.set_component("minecraft:item_model", json!(key));
    }
    if let Some((m, p)) = &item.trim
        && let (Some(m), Some(p)) = (key_of(&h.fill(m)), key_of(&h.fill(p)))
    {
        stack.set_component("minecraft:trim", json!({ "material": m, "pattern": p }));
    }
    if let Some(color) = item.rgb.as_ref().filter(|_| !potion).and_then(|c| rgb(&h.fill(c))) {
        if mat.starts_with("minecraft:leather_") || mat == "minecraft:wolf_armor" {
            stack.set_component("minecraft:dyed_color", json!(color));
        } else if mat == "minecraft:firework_star" {
            stack.set_component("minecraft:firework_explosion", json!({ "shape": "small_ball", "colors": [color] }));
        }
    }
    if !item.enchantments.is_empty() {
        if mat == "minecraft:enchanted_book" {
            let levels: Map<String, Value> = item.enchantments.iter().map(|(k, l)| (k.clone(), json!(l))).collect();
            stack.set_component("minecraft:stored_enchantments", Value::Object(levels));
        } else {
            for (k, l) in &item.enchantments {
                stack.add_enchant(k, *l);
            }
        }
    }
    if let Some(level) = &item.light_level
        && mat == "minecraft:light"
        && let Ok(n) = h.fill(level).trim().parse::<i64>()
    {
        stack.set_component("minecraft:block_state", json!({ "level": n.clamp(0, 15).to_string() }));
    }
    let mut flags = item.item_flags.clone();
    if item.hide_attributes {
        flags.push("HIDE_ATTRIBUTES".into());
    }
    if item.hide_enchants {
        flags.push("HIDE_ENCHANTS".into());
    }
    if item.hide_unbreakable {
        flags.push("HIDE_UNBREAKABLE".into());
    }
    let hidden: Vec<&str> = flags.iter().flat_map(|f| hidden_by(f).iter().copied()).collect();
    if !hidden.is_empty() {
        display.insert("hidden_components".into(), json!(hidden));
    }
    if !display.is_empty() {
        stack.set_component("minecraft:tooltip_display", Value::Object(display));
    }
    if !item.nbt.is_empty() {
        let mut data = stack.get_component("minecraft:custom_data").and_then(Value::as_object).cloned().unwrap_or_default();
        for (kind, raw) in &item.nbt {
            let tag = h.fill(raw);
            let Some((k, v)) = tag.split_once(':') else { continue };
            let value = match kind {
                's' => json!(v),
                _ => match v.trim().parse::<i64>() {
                    Ok(n) => json!(n),
                    Err(_) => continue,
                },
            };
            data.insert(k.to_owned(), value);
        }
        stack.set_component("minecraft:custom_data", Value::Object(data));
    }
    Some(stack)
}
