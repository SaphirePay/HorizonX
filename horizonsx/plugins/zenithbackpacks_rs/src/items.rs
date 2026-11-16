//! The backpack item and the vault chest item: made, recognised and redrawn for their tier. Both are marked in the
//! item's PersistentDataContainer under `zenithbackpacks:` (the standalone plugin's keys); items minted by the
//! ZenithCore module (`zenithcore:`) are recognised too.

use crate::conf::{self, Settings};
use horizon_bukkit::persistence::NamespacedKey;
use horizon_bukkit::{ItemStack, Material};
use serde_json::{Value, json};

const SPACES: [&str; 2] = ["zenithbackpacks", "zenithcore"];

fn key(space: &str, name: &str) -> NamespacedKey {
    NamespacedKey::from_string(&format!("{space}:{name}")).expect("valid key")
}

fn pdc(stack: &ItemStack, name: &str) -> Option<Value> {
    SPACES.iter().find_map(|s| stack.pdc_get(&key(s, name)))
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => matches!(s.trim_end_matches(['b', 'B']), "1" | "true"),
        _ => false,
    }
}

pub fn is_backpack(stack: Option<&ItemStack>) -> bool {
    stack.is_some_and(|s| !s.get_type().is_air() && pdc(s, "zenith_backpack").is_some_and(|v| truthy(&v)))
}

fn uuid_at(stack: Option<&ItemStack>, name: &str) -> Option<String> {
    let s = stack.filter(|s| !s.get_type().is_air())?;
    let raw = pdc(s, name)?;
    let text = raw.as_str()?.trim_matches('"').to_owned();
    (text.len() == 36 && text.chars().filter(|c| *c == '-').count() == 4).then_some(text)
}

pub fn chest_id(stack: Option<&ItemStack>) -> Option<String> {
    uuid_at(stack, "vault_chest_id")
}

pub fn chest_owner(stack: Option<&ItemStack>) -> Option<String> {
    uuid_at(stack, "vault_chest_owner")
}

fn material(name: &str, fallback: &str) -> Material {
    Material::get_material(&name.trim().to_ascii_uppercase()).unwrap_or_else(|| Material::of(fallback))
}

fn set_text(stack: &mut ItemStack, name: Value, lore: Vec<Value>) {
    stack.set_component("minecraft:custom_name", name);
    stack.set_component("minecraft:lore", Value::Array(lore));
}

/// `prefix` + tier as the item model (`zbackpacks:backpack_tier3`); a blank or malformed key leaves the plain item.
fn tier_model(stack: &mut ItemStack, prefix: &str, tier: i32) {
    let p = prefix.trim();
    match p.split_once(':') {
        Some((ns, path)) if !ns.is_empty() && !path.is_empty() => {
            stack.set_component("minecraft:item_model", json!(format!("{ns}:{path}{tier}")));
        }
        _ => {}
    }
}

/// The `"url"` of a base64 skin blob, as a profile the client draws.
fn skull(stack: &mut ItemStack, texture: &str) {
    if texture.trim().is_empty() || stack.get_type().key() != "minecraft:player_head" {
        return;
    }
    let Some(bytes) = horizon_kit::item::b64_decode(texture.trim()) else { return };
    let decoded = String::from_utf8_lossy(&bytes);
    if !decoded.contains("\"url\"") {
        return;
    }
    stack.set_component("minecraft:profile", json!({ "properties": [{ "name": "textures", "value": texture.trim() }] }));
}

/// A new backpack showing `rows`.
pub fn backpack(s: &Settings, rows: i32) -> ItemStack {
    let mut stack = ItemStack::new(material(&s.item_material, "PLAYER_HEAD"), 1);
    stack.set_component("minecraft:max_stack_size", json!(1));
    stack.pdc_set(&key(SPACES[0], "zenith_backpack"), json!(1));
    backpack_look(s, &mut stack, rows);
    stack
}

/// Name, lore, skull and tier art for the saved row count (`applyAppearance`).
pub fn backpack_look(s: &Settings, stack: &mut ItemStack, saved_rows: i32) {
    if !is_backpack(Some(stack)) {
        return;
    }
    let max = s.max_rows;
    let shown = saved_rows.min(max);
    let maxed = saved_rows >= max;
    let pairs = [("rows", shown.to_string()), ("max", max.to_string()), ("slots", (shown * 9).to_string()), ("maxslots", (max * 9).to_string())];
    let name = conf::mm(&s.gui(if maxed { "item.name-maxed" } else { "item.name-progress" }), &pairs);
    let lore = conf::lore_lines(&s.gui_list(if maxed { "item.lore-maxed" } else { "item.lore-progress" }), &pairs);
    set_text(stack, name, lore);
    skull(stack, &s.item_texture);
    tier_model(stack, &s.item_model_key, shown.clamp(1, max));
}

/// A vault chest item for `chest_id`, owned by `owner`, showing `pages`.
pub fn chest(s: &Settings, owner: &str, chest_id: &str, pages: i32) -> ItemStack {
    let mut stack = ItemStack::new(material(&s.chest_material, "CHEST"), 1);
    stack.set_component("minecraft:max_stack_size", json!(1));
    stack.pdc_set(&key(SPACES[0], "vault_chest_id"), json!(chest_id));
    stack.pdc_set(&key(SPACES[0], "vault_chest_owner"), json!(owner));
    chest_look(s, &mut stack, pages);
    stack
}

pub fn chest_look(s: &Settings, stack: &mut ItemStack, pages: i32) {
    let shown = conf::clamp_pages(pages);
    let maxed = shown >= s.chest_max_pages;
    let pairs = [
        ("pages", shown.to_string()),
        ("max", s.chest_max_pages.to_string()),
        ("slots", conf::capacity(shown).to_string()),
        ("owner", String::new()),
        ("tier", s.tier_name(shown)),
    ];
    let name = conf::mm(&s.gui("chest.item.name"), &pairs);
    let lore = conf::lore_lines(&s.gui_list(if maxed { "chest.item.lore-maxed" } else { "chest.item.lore-progress" }), &pairs);
    set_text(stack, name, lore);
    tier_model(stack, &s.chest_model_key, shown);
}

/// A plain item with a name (and lore): menu buttons.
pub fn button(material_name: &str, fallback: &str, name: Value, lore: Vec<Value>) -> ItemStack {
    let mut stack = ItemStack::new(material(material_name, fallback), 1);
    set_text(&mut stack, name, lore);
    stack
}

pub fn filler(s: &Settings) -> ItemStack {
    let mut stack = ItemStack::new(material(&s.gui("shared.filler-pane"), "GRAY_STAINED_GLASS_PANE"), 1);
    stack.set_component("minecraft:custom_name", json!({ "text": "" }));
    stack
}
