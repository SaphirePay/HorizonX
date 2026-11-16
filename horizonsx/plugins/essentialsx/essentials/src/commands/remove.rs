//! `Commandremove`. Java's `instanceof` over the entity classes is a list of entities here.
use crate::prelude::*;
use crate::mob;
use horizon_bukkit::entity::Entity;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("remove") };

const TO_REMOVE: [&str; 19] = ["drops", "arrows", "boats", "minecarts", "xp", "paintings", "itemframes", "endercrystals", "hostile", "monsters", "passive", "animals", "ambient", "mobs", "entities", "all", "custom", "tamed", "named"];

const MONSTERS: &[&str] = &[
    "blaze", "bogged", "breeze", "cave_spider", "creaking", "creeper", "drowned", "elder_guardian", "enderman", "endermite", "evoker", "giant", "guardian", "hoglin", "husk", "illusioner", "magma_cube", "parched", "piglin", "piglin_brute", "pillager", "ravager", "shulker", "silverfish", "skeleton", "spider", "stray", "vex", "vindicator", "warden",
    "witch", "wither", "wither_skeleton", "zoglin", "zombie", "zombie_villager", "zombified_piglin",
];
const FLYING: &[&str] = &["ghast", "phantom", "happy_ghast"];
const COMPLEX: &[&str] = &["ender_dragon"];
const SLIMES: &[&str] = &["slime", "magma_cube"];
const ANIMALS: &[&str] = &[
    "armadillo", "axolotl", "bee", "camel", "camel_husk", "cat", "chicken", "cow", "donkey", "fox", "frog", "goat", "hoglin", "horse", "llama", "mooshroom", "mule", "nautilus", "ocelot", "panda", "parrot", "pig", "polar_bear", "rabbit", "sheep", "skeleton_horse", "sniffer", "strider", "trader_llama", "turtle", "wolf", "zombie_horse", "zombie_nautilus",
];
const NPCS: &[&str] = &["villager", "wandering_trader"];
const WATER_MOBS: &[&str] = &["cod", "dolphin", "glow_squid", "pufferfish", "salmon", "squid", "tadpole", "tropical_fish"];
const PROJECTILES: &[&str] = &[
    "arrow", "breeze_wind_charge", "dragon_fireball", "egg", "ender_pearl", "experience_bottle", "eye_of_ender", "fireball", "firework_rocket", "fishing_bobber", "lingering_potion", "llama_spit", "shulker_bullet", "small_fireball", "snowball", "spectral_arrow", "splash_potion", "trident", "wind_charge", "wither_skull",
];

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let mut world = user.get_world();
    let mut radius = 0;
    if let Some(second) = args.get(1) {
        match second.parse() {
            Ok(number) => radius = number,
            Err(_) => world = ess().get_world(second),
        }
    }
    if let Some(third) = args.get(2) {
        radius = 0;
        world = ess().get_world(third);
    }
    parse_command(&user.get_source(), args, world, radius)
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    parse_command(sender, args, ess().get_world(&args[1]), 0)
}

fn parse_command(sender: &CommandSource, args: &[String], world: Option<World>, radius: i32) -> EssResult {
    let world = world.ok_or_else(|| EssError::tl("invalidWorld", &[]))?;
    let mut types = Vec::new();
    let mut custom = Vec::new();
    if args[0] == "*" || args[0] == "all" {
        types.push("all".to_owned());
    } else {
        for word in java_split(&args[0], ',') {
            let lower = word.to_lowercase();
            if TO_REMOVE.contains(&lower.as_str()) {
                types.push(lower);
            } else if TO_REMOVE.contains(&format!("{lower}s").as_str()) {
                types.push(format!("{lower}s"));
            } else {
                types.push("custom".to_owned());
                custom.push(word);
            }
        }
    }
    remove_handler(sender, &types, &custom, &world, radius);
    Ok(())
}

fn is_any(id: &str, groups: &[&[&str]]) -> bool {
    groups.iter().any(|group| group.contains(&id))
}

fn is_tamed(entity: &Entity) -> bool {
    !entity.data_at("Owner").is_null() || entity.data_at("Tame").as_i64() == Some(1)
}

fn remove_handler(sender: &CommandSource, types: &[String], custom: &[String], world: &World, radius: i32) {
    let radius = if radius > 0 { radius.wrapping_mul(radius) } else { radius };
    let mut removed = 0;
    let mut custom_types = Vec::new();
    let mut warn = false;
    for word in custom {
        match mob::from_name(word) {
            Some(found) => custom_types.push(found),
            None => warn = true,
        }
    }
    if warn {
        sender.send_tl("invalidMob", &[]);
    }
    let keeps_tamed = !types.iter().any(|kind| kind == "tamed");
    let keeps_named = !types.iter().any(|kind| kind == "named");
    let here = sender.get_player().map(|player| player.get_location());
    // One question for the world: one per loaded chunk stalls the game for seconds.
    for entity in world.get_entities() {
        if radius > 0
            && let (Some(here), Some(at)) = (&here, entity.get_location())
            && here.distance_squared(&at) > f64::from(radius)
        {
            continue;
        }
        let id = entity.get_type().trim_start_matches("minecraft:").to_lowercase();
        if id == "player" {
            continue;
        }
        let living = !is_any(&id, &[PROJECTILES]) && !matches!(id.as_str(), "item" | "experience_orb" | "painting" | "item_frame" | "glow_item_frame" | "end_crystal") && !id.ends_with("minecart") && !id.ends_with("boat") && !id.ends_with("raft");
        let named = living && entity.get_custom_name().is_some();
        for kind in types {
            if keeps_tamed && is_tamed(&entity) {
                continue;
            }
            if named && keeps_named {
                continue;
            }
            let matched = match kind.as_str() {
                "tamed" => is_tamed(&entity),
                "named" => named,
                "drops" => id == "item",
                "arrows" => is_any(&id, &[PROJECTILES]),
                "boats" => id.ends_with("boat") || id.ends_with("raft"),
                "minecarts" => id.ends_with("minecart"),
                "xp" => id == "experience_orb",
                "paintings" => id == "painting",
                "itemframes" => id == "item_frame" || id == "glow_item_frame",
                "endercrystals" => id == "end_crystal",
                "ambient" => is_any(&id, &[FLYING]),
                "hostile" | "monsters" => is_any(&id, &[MONSTERS, COMPLEX, FLYING, SLIMES]),
                "passive" | "animals" => is_any(&id, &[ANIMALS, NPCS, &["snow_golem"], WATER_MOBS, &["bat"]]),
                "mobs" => is_any(&id, &[ANIMALS, NPCS, &["snow_golem"], WATER_MOBS, MONSTERS, COMPLEX, FLYING, SLIMES, &["bat"]]),
                "entities" | "all" => true,
                "custom" => {
                    let hits = custom_types.iter().filter(|found| found.entity == id).count();
                    for _ in 0..hits {
                        entity.remove();
                        removed += 1;
                    }
                    false
                }
                _ => false,
            };
            if matched {
                entity.remove();
                removed += 1;
            }
        }
    }
    sender.send_tl("removed", crate::args![removed]);
}

fn tab(_sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => TO_REMOVE.map(str::to_owned).to_vec(),
        2 => get_worlds().iter().map(|world| world.get_name().to_owned()).collect(),
        _ => Vec::new(),
    })
}
