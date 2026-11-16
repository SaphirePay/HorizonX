//! `MaterialUtil`: the sets of materials, by Bukkit name.
use crate::prelude::*;

fn name(material: &Material) -> String {
    material.name()
}

const HELMETS: [&str; 8] = ["LEATHER_HELMET", "CHAINMAIL_HELMET", "IRON_HELMET", "GOLDEN_HELMET", "DIAMOND_HELMET", "NETHERITE_HELMET", "TURTLE_HELMET", "COPPER_HELMET"];
const CHESTPLATES: [&str; 8] = ["LEATHER_CHESTPLATE", "CHAINMAIL_CHESTPLATE", "IRON_CHESTPLATE", "GOLDEN_CHESTPLATE", "DIAMOND_CHESTPLATE", "NETHERITE_CHESTPLATE", "ELYTRA", "COPPER_CHESTPLATE"];
const LEGGINGS: [&str; 7] = ["LEATHER_LEGGINGS", "CHAINMAIL_LEGGINGS", "IRON_LEGGINGS", "GOLDEN_LEGGINGS", "DIAMOND_LEGGINGS", "NETHERITE_LEGGINGS", "COPPER_LEGGINGS"];
const BOOTS: [&str; 7] = ["LEATHER_BOOTS", "CHAINMAIL_BOOTS", "IRON_BOOTS", "GOLDEN_BOOTS", "DIAMOND_BOOTS", "NETHERITE_BOOTS", "COPPER_BOOTS"];
const COLORS: [&str; 16] = ["WHITE", "ORANGE", "MAGENTA", "LIGHT_BLUE", "YELLOW", "LIME", "PINK", "GRAY", "LIGHT_GRAY", "CYAN", "PURPLE", "BLUE", "BROWN", "GREEN", "RED", "BLACK"];
const MOB_HEADS: [&str; 12] = ["SKELETON_SKULL", "SKELETON_WALL_SKULL", "WITHER_SKELETON_SKULL", "WITHER_SKELETON_WALL_SKULL", "CREEPER_HEAD", "CREEPER_WALL_HEAD", "ZOMBIE_HEAD", "ZOMBIE_WALL_HEAD", "DRAGON_HEAD", "DRAGON_WALL_HEAD", "PIGLIN_HEAD", "PIGLIN_WALL_HEAD"];
const WOODS: [&str; 12] = ["ACACIA", "BIRCH", "DARK_OAK", "JUNGLE", "OAK", "SPRUCE", "CRIMSON", "WARPED", "MANGROVE", "CHERRY", "BAMBOO", "PALE_OAK"];

pub fn is_helmet(material: &Material) -> bool {
    HELMETS.contains(&name(material).as_str())
}
pub fn is_chestplate(material: &Material) -> bool {
    CHESTPLATES.contains(&name(material).as_str())
}
pub fn is_leggings(material: &Material) -> bool {
    LEGGINGS.contains(&name(material).as_str())
}
pub fn is_boots(material: &Material) -> bool {
    BOOTS.contains(&name(material).as_str())
}
pub fn is_armor(material: &Material) -> bool {
    is_helmet(material) || is_chestplate(material) || is_leggings(material) || is_boots(material)
}
pub fn is_bed(material: &Material) -> bool {
    name(material).strip_suffix("_BED").is_some_and(|color| COLORS.contains(&color))
}
pub fn is_banner(material: &Material) -> bool {
    let name = name(material);
    name == "SHIELD" || name.strip_suffix("_BANNER").is_some_and(|color| COLORS.contains(&color))
}
pub fn is_firework(material: &Material) -> bool {
    name(material) == "FIREWORK_ROCKET"
}
pub fn is_firework_charge(material: &Material) -> bool {
    name(material) == "FIREWORK_STAR"
}
pub fn is_leather_armor(material: &Material) -> bool {
    name(material).starts_with("LEATHER_") && is_armor(material)
}
pub fn is_player_head(stack: &ItemStack) -> bool {
    matches!(name(stack.get_type()).as_str(), "PLAYER_HEAD" | "PLAYER_WALL_HEAD")
}
pub fn is_potion(material: &Material) -> bool {
    matches!(name(material).as_str(), "POTION" | "SPLASH_POTION" | "LINGERING_POTION" | "TIPPED_ARROW")
}
pub fn is_sign_post(material: &Material) -> bool {
    name(material).strip_suffix("_SIGN").is_some_and(|wood| WOODS.contains(&wood))
}
pub fn is_wall_sign(material: &Material) -> bool {
    name(material).strip_suffix("_WALL_SIGN").is_some_and(|wood| WOODS.contains(&wood))
}
pub fn is_hanging_sign(material: &Material) -> bool {
    name(material).strip_suffix("_HANGING_SIGN").is_some_and(|wood| WOODS.contains(&wood))
}
pub fn is_wall_hanging_sign(material: &Material) -> bool {
    name(material).strip_suffix("_WALL_HANGING_SIGN").is_some_and(|wood| WOODS.contains(&wood))
}
pub fn is_sign(material: &Material) -> bool {
    is_sign_post(material) || is_wall_sign(material) || is_hanging_sign(material) || is_wall_hanging_sign(material)
}
pub fn is_editable_book(material: &Material) -> bool {
    matches!(name(material).as_str(), "WRITTEN_BOOK" | "WRITABLE_BOOK")
}
pub fn is_skull(material: &Material) -> bool {
    matches!(name(material).as_str(), "PLAYER_HEAD" | "PLAYER_WALL_HEAD")
}
pub fn is_mob_head(material: &Material) -> bool {
    MOB_HEADS.contains(&name(material).as_str())
}
pub fn is_air(material: &Material) -> bool {
    material.is_air()
}
pub fn is_spawner(material: &Material) -> bool {
    name(material) == "SPAWNER"
}
/// `getColorOf`: the dye in the name, or white.
pub fn get_color_of(material: &Material) -> &'static str {
    let name = name(material);
    ["WHITE", "ORANGE", "MAGENTA", "LIGHT_BLUE", "YELLOW", "LIME", "PINK", "GRAY", "LIGHT_GRAY", "CYAN", "PURPLE", "BLUE", "BROWN", "GREEN", "RED", "BLACK"].into_iter().find(|color| name.contains(color)).unwrap_or("WHITE")
}
/// `getDamage`.
pub fn get_damage(stack: &ItemStack) -> i32 {
    stack.get_damage()
}
/// `setDamage`.
pub fn set_damage(stack: &mut ItemStack, damage: i32) {
    stack.set_damage(damage);
}
