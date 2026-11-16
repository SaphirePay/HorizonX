//! `Mob`: the table of what `/spawnmob` knows, with `MobCompat`'s names settled to the modern game.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enemies {
    Friendly,
    Neutral,
    Enemy,
    AdultEnemy,
}

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/mob_table.rs"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mob {
    pub key: &'static str,
    pub name: &'static str,
    pub kind: Enemies,
    pub suffix: &'static str,
    /// The entity's id without `minecraft:`.
    pub entity: &'static str,
}

fn row(entry: &'static (&'static str, &'static str, Enemies, &'static str, &'static str)) -> Mob {
    Mob { key: entry.0, name: entry.1, kind: entry.2, suffix: entry.3, entity: entry.4 }
}

impl Mob {
    /// `spawn`.
    pub fn spawn(&self, location: &horizon_bukkit::Location) -> Option<horizon_bukkit::entity::Entity> {
        location.get_world().spawn(location, self.entity)
    }
}

/// `getMobList`.
pub fn get_mob_list() -> Vec<String> {
    MOBS.iter().map(|entry| entry.1.to_lowercase()).collect()
}

/// `fromName`.
pub fn from_name(name: &str) -> Option<Mob> {
    let lower = name.to_lowercase();
    MOBS.iter().find(|entry| entry.1.eq_ignore_ascii_case(&lower)).map(row)
}

/// `fromBukkitType`: the last row of an entity wins, as `bukkitMap.put` has it.
pub fn from_entity(entity: &str) -> Option<Mob> {
    let id = entity.trim_start_matches("minecraft:").to_lowercase();
    MOBS.iter().rev().find(|entry| entry.4 == id).map(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_came_through() {
        assert!(MOBS.len() > 100);
        assert_eq!(from_name("PigZombie").map(|mob| mob.entity), Some("zombified_piglin"));
        assert_eq!(from_name("sheep").map(|mob| mob.suffix), Some(""));
        assert_eq!(from_name("fox").map(|mob| mob.suffix), Some("es"));
    }
}
