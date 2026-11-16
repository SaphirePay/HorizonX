//! `com.earth2me.essentials.items`, `Enchantments`, `Potions` and `MetaItemStack`.
pub mod db;
pub mod meta;
mod enchantment_table;
mod potion_table;

use enchantment_table::ENCHANTMENT_NAMES;
use horizon_bukkit::Material;
use potion_table::POTION_NAMES;

/// `Enchantments.getByName`: the game's id, or an alias from the Java table. The result is the plain id.
pub fn enchantment_by_name(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let lower = name.to_lowercase();
    if ENCHANTMENT_NAMES.iter().any(|(_, real, _)| *real == lower) {
        return Some(lower);
    }
    ENCHANTMENT_NAMES.iter().find(|(key, _, _)| *key == lower).map(|(_, real, _)| (*real).to_owned())
}

/// `Enchantments.getRealName`.
pub fn enchantment_real_name(enchantment: &str) -> String {
    enchantment.trim_start_matches("minecraft:").to_owned()
}

/// `Enchantments.entrySet`: the name, and the real name it stands for.
pub fn enchantment_entries() -> Vec<(&'static str, &'static str)> {
    ENCHANTMENT_NAMES.iter().filter(|(_, _, alias)| !*alias).map(|(key, real, _)| (*key, *real)).collect()
}

/// `Enchantment.canEnchantItem`: the game's `supported_items` tags, by the item's name.
pub fn can_enchant_item(enchantment: &str, material: &Material) -> bool {
    let name = material.name();
    let ends = |words: &[&str]| words.iter().any(|word| name.ends_with(word));
    let is = |word: &str| name == word;
    let armor = || ends(&["_HELMET", "_CHESTPLATE", "_LEGGINGS", "_BOOTS"]);
    let tool = || ends(&["_PICKAXE", "_AXE", "_SHOVEL", "_HOE"]);
    let sword = || ends(&["_SWORD", "_SPEAR"]);
    let weapon = || sword() || name.ends_with("_AXE") || is("MACE");
    let durable = || material.get_max_durability() > 0;
    match enchantment.trim_start_matches("minecraft:") {
        "protection" | "fire_protection" | "blast_protection" | "projectile_protection" => armor(),
        "thorns" => armor(),
        "respiration" | "aqua_affinity" => name.ends_with("_HELMET"),
        "feather_falling" | "depth_strider" | "frost_walker" | "soul_speed" => name.ends_with("_BOOTS"),
        "swift_sneak" => name.ends_with("_LEGGINGS"),
        "sharpness" => sword() || name.ends_with("_AXE"),
        "smite" | "bane_of_arthropods" => weapon(),
        "fire_aspect" => sword() || is("MACE"),
        "knockback" | "looting" => sword(),
        "sweeping_edge" => name.ends_with("_SWORD"),
        "lunge" => name.ends_with("_SPEAR"),
        "efficiency" => tool() || is("SHEARS"),
        "fortune" | "silk_touch" => tool(),
        "power" | "punch" | "flame" | "infinity" => is("BOW"),
        "multishot" | "piercing" | "quick_charge" => is("CROSSBOW"),
        "loyalty" | "riptide" | "channeling" | "impaling" => is("TRIDENT"),
        "density" | "breach" | "wind_burst" => is("MACE"),
        "lure" | "luck_of_the_sea" => is("FISHING_ROD"),
        "unbreaking" | "mending" => durable(),
        "vanishing_curse" => durable() || is("COMPASS") || is("CARVED_PUMPKIN") || name.ends_with("_HEAD") || name.ends_with("_SKULL"),
        "binding_curse" => armor() || is("ELYTRA") || is("CARVED_PUMPKIN") || name.ends_with("_HEAD") || name.ends_with("_SKULL"),
        _ => false,
    }
}

/// `Enchantments.keySet`.
pub fn enchantment_names() -> Vec<&'static str> {
    ENCHANTMENT_NAMES.iter().filter(|(_, _, alias)| !*alias).map(|(key, _, _)| *key).collect()
}

/// `Enchantment.getMaxLevel`, from the game's own table.
pub fn enchantment_max_level(enchantment: &str) -> i32 {
    match enchantment.trim_start_matches("minecraft:") {
        "sharpness" | "smite" | "bane_of_arthropods" | "efficiency" | "power" | "impaling" | "density" => 5,
        "protection" | "fire_protection" | "blast_protection" | "projectile_protection" | "feather_falling" | "piercing" | "breach" => 4,
        "fortune" | "looting" | "unbreaking" | "respiration" | "thorns" | "depth_strider" | "sweeping_edge" | "soul_speed" | "swift_sneak" | "lure" | "luck_of_the_sea" | "loyalty" | "riptide" | "quick_charge" | "wind_burst" | "lunge" => 3,
        "knockback" | "punch" | "fire_aspect" | "frost_walker" => 2,
        _ => 1,
    }
}

/// `Potions.getByName`: the plain id of the effect.
pub fn potion_by_name(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    if POTION_NAMES.iter().any(|(_, real, _)| *real == lower) {
        return Some(lower);
    }
    POTION_NAMES.iter().find(|(key, _, _)| *key == lower).map(|(_, real, _)| (*real).to_owned())
}

/// `Potions.entrySet`'s keys.
pub fn potion_names() -> Vec<&'static str> {
    POTION_NAMES.iter().filter(|(_, _, alias)| !*alias).map(|(key, _, _)| *key).collect()
}
