//! `Registry.BIOME`, `Registry.MATERIAL`...: the game's registries, as Bukkit's `Registry` fields.
//! An entry comes back the way the plugin holds that type here, as the SDK's constants for it are:
//! Material, EntityType, Particle and PotionType by upper-case name (`STONE`), the rest (Biome, Sound,
//! Enchantment, Attribute...) by key (`minecraft:plains`).
use horizon_bukkit::registry::Registry;
use jrt::{R, V};

pub struct RegistryH {
    registry: Registry,
    /// The entries are a game enum held by name: `minecraft:stone` is `STONE`.
    as_enum: bool,
    /// The SDK's names for a registry of built-in data, when the game lists none.
    builtin: &'static [&'static str],
}

fn of(name: &str) -> Option<(Registry, bool)> {
    Some(match name {
        "BIOME" => (Registry::BIOME, false),
        "MATERIAL" | "ITEM" => (Registry::ITEM, true),
        "BLOCK" => (Registry::BLOCK, true),
        "ENTITY_TYPE" => (Registry::ENTITY_TYPE, true),
        "PARTICLE_TYPE" => (Registry::PARTICLE_TYPE, true),
        "SOUNDS" | "SOUND_EVENT" => (Registry::SOUND_EVENT, false),
        "STRUCTURE_TYPE" => (Registry::STRUCTURE_TYPE, false),
        "VILLAGER_PROFESSION" => (Registry::VILLAGER_PROFESSION, false),
        "VILLAGER_TYPE" => (Registry::VILLAGER_TYPE, false),
        "ATTRIBUTE" => (Registry::ATTRIBUTE, false),
        "FLUID" => (Registry::FLUID, false),
        "GAME_EVENT" => (Registry::GAME_EVENT, false),
        "POTION" => (Registry::POTION, true),
        "ENCHANTMENT" => (Registry::ENCHANTMENT, false),
        "EFFECT" | "MOB_EFFECT" | "POTION_EFFECT_TYPE" => (Registry::MOB_EFFECT, false),
        "DAMAGE_TYPE" => (Registry::DAMAGE_TYPE, false),
        "INSTRUMENT" => (Registry::INSTRUMENT, false),
        "JUKEBOX_SONG" => (Registry::JUKEBOX_SONG, false),
        "PAINTING_VARIANT" | "ART" => (Registry::PAINTING_VARIANT, false),
        "TRIM_MATERIAL" => (Registry::TRIM_MATERIAL, false),
        "TRIM_PATTERN" => (Registry::TRIM_PATTERN, false),
        "BANNER_PATTERN" => (Registry::BANNER_PATTERN, false),
        "WOLF_VARIANT" => (Registry::WOLF_VARIANT, false),
        "CAT_VARIANT" => (Registry::CAT_VARIANT, false),
        "FROG_VARIANT" => (Registry::FROG_VARIANT, false),
        "STRUCTURE" => (Registry::STRUCTURE, false),
        "MENU" => (Registry::MENU, false),
        _ => return None,
    })
}

pub fn field(class: &str, name: &str) -> Option<V> {
    if class != "Registry" {
        return None;
    }
    let (registry, as_enum) = of(name)?;
    let builtin: &'static [&'static str] = match name {
        "BIOME" => horizon_bukkit::enums::biome::values(),
        "ENCHANTMENT" => horizon_bukkit::enums::enchantment::values(),
        _ => &[],
    };
    Some(jrt::native(RegistryH { registry, as_enum, builtin }))
}

/// An enchantment's key from what a plugin holds: a key, today's name, or Bukkit's old name
/// (`DURABILITY` is `minecraft:unbreaking`).
pub fn enchantment_key(held: &str) -> String {
    if held.contains(':') {
        return held.to_owned();
    }
    let name = match held.to_ascii_uppercase().as_str() {
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
        _ => return format!("minecraft:{}", held.to_ascii_lowercase()),
    };
    format!("minecraft:{name}")
}

/// The registry's keys: what the game lists, or (a registry of the game's built-in data, which the
/// data listing doesn't hold) the SDK's own names for it.
fn keys(r: &RegistryH) -> Vec<String> {
    let listed = r.registry.keys();
    if !listed.is_empty() || r.builtin.is_empty() {
        return listed;
    }
    r.builtin.iter().filter(|name| **name != "CUSTOM").map(|name| format!("minecraft:{}", name.to_ascii_lowercase())).collect()
}

fn find(r: &RegistryH, key: &str) -> Option<String> {
    let key = if key.contains(':') { key.to_owned() } else { format!("minecraft:{}", key.trim().to_ascii_lowercase().replace(' ', "_")) };
    keys(r).into_iter().find(|known| *known == key)
}

fn entry(r: &RegistryH, key: &str) -> V {
    if r.as_enum {
        jrt::s(&key.trim_start_matches("minecraft:").replace(['.', '/'], "_").to_ascii_uppercase())
    } else {
        jrt::s(key)
    }
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    let r = this.native::<RegistryH>()?;
    Some(Ok(match (name, a.len()) {
        ("get" | "getOrThrow" | "match", 1) => {
            let key = crate::key_text(&a[0]);
            let found = if name == "match" { find(r, &key.to_ascii_lowercase()) } else { find(r, &key) };
            match found {
                Some(key) => entry(r, &key),
                None if name == "getOrThrow" => return Some(Err(jrt::exc("NoSuchElementException", format!("No value for {key} in the registry")))),
                None => V::Null,
            }
        }
        ("iterator", 0) => return Some(jrt::call(&jrt::list(keys(r).iter().map(|k| entry(r, k)).collect()), "iterator", &[])),
        ("stream", 0) => return Some(jrt::call(&jrt::list(keys(r).iter().map(|k| entry(r, k)).collect()), "stream", &[])),
        ("size", 0) => V::Int(keys(r).len() as i32),
        ("keyStream", 0) => return Some(jrt::call(&jrt::list(keys(r).iter().filter_map(|k| horizon_bukkit::persistence::NamespacedKey::from_string(k).map(crate::wrap)).collect()), "stream", &[])),
        ("hashCode", 0) => V::Int(0),
        ("toString", 0) => jrt::s("Registry"),
        _ => return None,
    }))
}
