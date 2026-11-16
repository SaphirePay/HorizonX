//! `SpawnMob`.
use crate::prelude::*;
use crate::mob::{self, Mob};
use crate::mob_data;
use crate::utils::location::{get_safe_destination, get_target};
use horizon_bukkit::entity::Entity;

/// `mobParts`.
pub fn mob_parts(text: &str) -> Vec<String> {
    java_split(text, ',').iter().map(|part| java_split(part, ':').into_iter().next().unwrap_or_default()).collect()
}

/// `mobData`.
pub fn mob_data(text: &str) -> Vec<Option<String>> {
    java_split(text, ',')
        .iter()
        .map(|part| match java_split(part, ':').get(1) {
            Some(data) => Some(data.clone()),
            None => part.contains(':').then(String::new),
        })
        .collect()
}

/// `spawnmob(ess, server, user, parts, data, count)`: at what the player looks at.
pub fn spawnmob_at_target(user: &User, parts: Vec<String>, data: &[Option<String>], count: i32) -> EssResult {
    let Some(player) = user.player() else { return Ok(()) };
    let location = get_target(&player).map_err(|_| EssError::tl("unableToSpawnMob", &[]))?;
    spawnmob_at(&user.get_source(), user, &location, parts, data, count)
}

/// `spawnmob(ess, server, sender, target, parts, data, count)`: at the target.
pub fn spawnmob(sender: &CommandSource, target: &User, parts: Vec<String>, data: &[Option<String>], count: i32) -> EssResult {
    let Some(location) = target.get_location() else { return Ok(()) };
    spawnmob_at(sender, target, &location, parts, data, count)
}

fn spawnmob_at(sender: &CommandSource, target: &User, location: &Location, mut parts: Vec<String>, data: &[Option<String>], mut count: i32) -> EssResult {
    let safe = get_safe_destination(location)?;
    for part in &parts {
        check_spawnable(sender, mob::from_name(part))?;
    }
    let server_limit = settings().get_spawn_mob_limit();
    let mut effective = server_limit / parts.len() as i32;
    if effective < 1 {
        effective = 1;
        parts.truncate(server_limit.max(0) as usize);
    }
    if count > effective {
        count = effective;
        sender.send_tl("mobSpawnLimit", &[]);
    }
    let first = parts.first().and_then(|part| mob::from_name(part)).ok_or_else(|| EssError::tl("invalidMob", &[]))?;
    for _ in 0..count {
        spawn_stack(sender, target, &safe, &parts, data)?;
    }
    sender.send_message(&format!("{} {}{} {}", count * parts.len() as i32, first.name.to_lowercase(), first.suffix, sender.tl("spawned", &[])));
    Ok(())
}

fn spawn_one(sender: &CommandSource, target: &User, mob: &Mob, safe: &Location, data: Option<&String>) -> EssResult<Entity> {
    let spawned = mob.spawn(safe).ok_or_else(|| EssError::tl("unableToSpawnMob", &[]))?;
    default_mob_data(mob.entity, &spawned);
    if let Some(data) = data {
        change_mob_data(sender, mob.entity, &spawned, &data.to_lowercase(), target)?;
    }
    Ok(spawned)
}

fn spawn_stack(sender: &CommandSource, target: &User, safe: &Location, parts: &[String], data: &[Option<String>]) -> EssResult {
    let lookup = |index: usize| mob::from_name(&parts[index]).ok_or_else(|| EssError::tl("invalidMob", &[]));
    let mut below = spawn_one(sender, target, &lookup(0)?, safe, data.first().and_then(Option::as_ref))?;
    for index in 1..parts.len() {
        let mount = spawn_one(sender, target, &lookup(index)?, safe, data.get(index).and_then(Option::as_ref))?;
        below.set_passenger(&mount);
        below = mount;
    }
    Ok(())
}

fn check_spawnable(sender: &CommandSource, mob: Option<Mob>) -> EssResult {
    let mob = mob.ok_or_else(|| EssError::tl("invalidMob", &[]))?;
    fail_if!(settings().get_protect_prevent_spawn(mob.entity), EssError::tl("disabledToSpawnMob", &[]));
    fail_if!(sender.get_user().is_some_and(|user| !user.is_authorized(&format!("essentials.spawnmob.{}", mob.name.to_lowercase()))), EssError::tl("noPermToSpawnMob", &[]));
    Ok(())
}

const ARMORS: [(&str, &str); 5] = [("netherite", "netherite"), ("diamond", "diamond"), ("gold", "golden"), ("leather", "leather"), ("iron", "iron")];

fn change_mob_data(sender: &CommandSource, entity: &str, spawned: &Entity, input: &str, target: &User) -> EssResult {
    let mut data = input.to_owned();
    if data.is_empty() {
        sender.send_tl("mobDataList", crate::args![join_list(&mob_data::get_valid_help(spawned))]);
    }
    let zombie = matches!(entity, "zombie" | "husk" | "drowned" | "zombie_villager" | "zombified_piglin");
    if zombie {
        spawned.merge("{IsBaby:0b}");
    } else if spawned.data_at("Age").is_number() {
        spawned.merge("{Age:0}");
    }
    if (zombie || entity == "skeleton") && (input.contains("armor") || input.contains("armour")) {
        let piece = |id: &str, part: &str| format!("{part}:{{id:\"minecraft:{id}_{part_name}\",count:1}}", part_name = armor_piece(part));
        let material = ARMORS.iter().find(|(word, _)| input.contains(word)).map(|(_, id)| *id);
        let equipment = if input.contains("noarmor") || input.contains("noarmour") {
            Some("{}".to_owned())
        } else {
            material.map(|id| format!("{{{},{},{},{}}}", piece(id, "feet"), piece(id, "legs"), piece(id, "chest"), piece(id, "head")))
        };
        if let Some(equipment) = equipment {
            spawned.merge(&format!("{{equipment:{equipment}}}"));
        }
        spawned.merge("{drop_chances:{feet:0f,legs:0f,chest:0f,head:0f}}");
    }
    let Some(player) = target.player() else { return Ok(()) };
    while let Some(row) = mob_data::from_data(spawned, &data) {
        let matched = row.set_data(spawned, &player, &data)?;
        if matched.is_empty() {
            break;
        }
        data = data.replace(&matched, "");
    }
    Ok(())
}

fn armor_piece(part: &str) -> &'static str {
    match part {
        "feet" => "boots",
        "legs" => "leggings",
        "chest" => "chestplate",
        _ => "helmet",
    }
}

fn default_mob_data(entity: &str, spawned: &Entity) {
    let held = |item: &str| format!("{{equipment:{{mainhand:{{id:\"minecraft:{item}\",count:1}}}},drop_chances:{{mainhand:0.1f}}}}");
    match entity {
        "skeleton" => {
            spawned.merge(&held("bow"));
        }
        "zombified_piglin" => {
            spawned.merge(&held("golden_sword"));
        }
        "horse" => {
            spawned.merge("{attributes:[{id:\"minecraft:jump_strength\",base:1.2d}]}");
        }
        _ => {}
    }
}
