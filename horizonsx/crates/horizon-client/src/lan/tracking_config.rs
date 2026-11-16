//! HorizonsX: how far and how often each client is sent the entities round it, from `spigot.yml` (read again
//! every 5 s, so a change needs no restart). Every server gets these defaults; a server changes them in its
//! `spigot.yml` under `world-settings: default:`.
//!
//! - `entity-tracking-range:` (Spigot's, in blocks): `players` 48, `animals` 48, `monsters` 48, `misc` 32 (item
//!   frames, paintings, dropped items, experience), `display` 128 (text, item and block displays), `other` 64
//!   (boats, carts, projectiles, armour stands...). As on Spigot, a type's own range is replaced by its group's
//!   (one of 0 stays 0), then `entity-broadcast-range-percentage` scales it and the view distance caps it.
//! - `max-players-seen-per-player` (default 100; 0: no limit): a client is sent at most this many other
//!   players, the nearest. One it already sees keeps its place until another is 8 blocks nearer, so a crowd's
//!   edge doesn't flicker. Hidden players stay in the tab list.
//! - `far-entity-distance` (default 32 blocks) and `far-entity-update-multiplier` (default 2; 1 or 0: off):
//!   players and mobs farther from a client than the distance are sent to it this many times less often (a
//!   player every 4 ticks instead of 2, a mob every 6 instead of 3), as one position sync each time.

/// An entity's group for its tracking range (Spigot's `TrackingRange`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Players,
    Animals,
    Monsters,
    Misc,
    Display,
    Other,
    /// Keeps its type's own range (the ender dragon: Paper sends it as far as the view distance).
    Own,
}

impl Group {
    pub fn of(kind: &str) -> Self {
        let name = kind.strip_prefix("minecraft:").unwrap_or(kind);
        match name {
            // Citizens' NPCs are players on Paper; HorizonsX draws them as mannequins.
            "player" | "mannequin" => Self::Players,
            "item_frame" | "glow_item_frame" | "painting" | "item" | "experience_orb" => Self::Misc,
            "block_display" | "item_display" | "text_display" => Self::Display,
            "ender_dragon" => Self::Own,
            _ if MONSTERS.binary_search(&name).is_ok() => Self::Monsters,
            _ if ANIMALS.binary_search(&name).is_ok() => Self::Animals,
            _ => Self::Other,
        }
    }

    /// Players and mobs: the entities whose far updates are thinned.
    pub fn is_mob_or_player(self) -> bool {
        matches!(self, Self::Players | Self::Animals | Self::Monsters)
    }
}

/// Spigot's MONSTER, FLYING_MONSTER and RAIDER (`Enemy`), sorted.
const MONSTERS: &[&str] = &[
    "blaze", "bogged", "breeze", "cave_spider", "creaking", "creeper", "drowned", "elder_guardian", "enderman",
    "endermite", "evoker", "ghast", "giant", "guardian", "hoglin", "husk", "illusioner", "magma_cube", "parched",
    "phantom", "piglin", "piglin_brute", "pillager", "ravager", "shulker", "silverfish", "skeleton", "slime",
    "spider", "stray", "vex", "vindicator", "warden", "witch", "wither", "wither_skeleton", "zoglin", "zombie",
    "zombie_villager", "zombified_piglin",
];

/// Spigot's ANIMAL, WATER and VILLAGER (the other mobs), sorted.
const ANIMALS: &[&str] = &[
    "allay", "armadillo", "axolotl", "bat", "bee", "camel", "camel_husk", "cat", "chicken", "cod", "copper_golem",
    "cow", "dolphin", "donkey", "fox", "frog", "glow_squid", "goat", "happy_ghast", "horse", "iron_golem", "llama",
    "mooshroom", "mule", "nautilus", "ocelot", "panda", "parrot", "pig", "polar_bear", "pufferfish", "rabbit",
    "salmon", "sheep", "skeleton_horse", "sniffer", "snow_golem", "squid", "strider", "sulfur_cube", "tadpole",
    "trader_llama", "tropical_fish", "turtle", "villager", "wandering_trader", "wolf", "zombie_horse",
    "zombie_nautilus",
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TrackingConfig {
    /// Blocks per group (players, animals, monsters, misc, display, other); None: every type's own range.
    pub ranges: Option<[i32; 6]>,
    pub max_players_seen: usize,
    pub far_distance: f64,
    pub far_multiplier: u32,
}

impl Default for TrackingConfig {
    fn default() -> Self {
        Self { ranges: Some([48, 48, 48, 32, 128, 64]), max_players_seen: 100, far_distance: 32.0, far_multiplier: 2 }
    }
}

impl TrackingConfig {
    /// Vanilla's tracking: each type's own range, no player limit, every update to everyone.
    pub fn vanilla() -> Self {
        Self { ranges: None, max_players_seen: 0, far_distance: 0.0, far_multiplier: 1 }
    }

    /// The range in blocks of an entity of `group` whose type's own range is `own` blocks.
    pub fn range(&self, group: Group, own: i32) -> i32 {
        if own == 0 {
            return 0;
        }
        let Some(r) = self.ranges else { return own };
        match group {
            Group::Players => r[0],
            Group::Animals => r[1],
            Group::Monsters => r[2],
            Group::Misc => r[3],
            Group::Display => r[4],
            Group::Other => r[5],
            Group::Own => own,
        }
    }

    /// Far updates are thinned.
    pub fn thins(&self) -> bool {
        self.far_multiplier > 1 && self.far_distance > 0.0
    }

    /// The settings in a `spigot.yml`'s text; what it doesn't say keeps the default.
    pub fn parse(text: &str) -> Self {
        let mut c = Self::default();
        let number = |key: &str| {
            text.lines()
                .find_map(|line| line.trim().strip_prefix(key))
                .and_then(|value| value.split('#').next()?.trim().parse::<f64>().ok())
                .map(|n| n.max(0.0))
        };
        if let Some(n) = number("max-players-seen-per-player:") {
            c.max_players_seen = n as usize;
        }
        if let Some(n) = number("far-entity-distance:") {
            c.far_distance = n;
        }
        if let Some(n) = number("far-entity-update-multiplier:") {
            c.far_multiplier = (n as u32).max(1);
        }
        let section = section(text, "entity-tracking-range:");
        if let Some(r) = c.ranges.as_mut() {
            for (i, key) in ["players", "animals", "monsters", "misc", "display", "other"].iter().enumerate() {
                if let Some(v) = section.iter().find(|(k, _)| k == key) {
                    r[i] = v.1.max(0);
                }
            }
        }
        c
    }
}

/// The `key: number` lines indented under `header` (a YAML map one level deep).
fn section(text: &str, header: &str) -> Vec<(String, i32)> {
    let indent = |line: &str| line.len() - line.trim_start().len();
    let mut lines = text.lines();
    let Some(head) = lines.find(|line| line.trim() == header) else { return Vec::new() };
    let base = indent(head);
    lines
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .take_while(|line| indent(line) > base)
        .filter_map(|line| {
            let (k, v) = line.trim().split_once(':')?;
            Some((k.trim().to_string(), v.split('#').next()?.trim().parse::<f64>().ok()? as i32))
        })
        .collect()
}

#[cfg(test)]
thread_local! {
    /// Tests: the settings ticks use (vanilla unless a test sets others).
    pub static TEST_CONFIG: std::cell::Cell<TrackingConfig> = std::cell::Cell::new(TrackingConfig::vanilla());
}

/// The server's settings, read from `spigot.yml` at most every 5 s.
pub fn current() -> TrackingConfig {
    #[cfg(test)]
    {
        TEST_CONFIG.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};
        static CACHE: Mutex<Option<(Instant, TrackingConfig)>> = Mutex::new(None);
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((read, c)) = *cache
            && read.elapsed() < Duration::from_secs(5)
        {
            return c;
        }
        let c = TrackingConfig::parse(&std::fs::read_to_string("spigot.yml").unwrap_or_default());
        *cache = Some((Instant::now(), c));
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_are_sorted() {
        assert!(MONSTERS.windows(2).all(|w| w[0] < w[1]));
        assert!(ANIMALS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn groups_follow_spigot() {
        assert_eq!(Group::of("minecraft:player"), Group::Players);
        assert_eq!(Group::of("minecraft:zombie"), Group::Monsters);
        assert_eq!(Group::of("minecraft:cow"), Group::Animals);
        assert_eq!(Group::of("minecraft:villager"), Group::Animals);
        assert_eq!(Group::of("minecraft:item"), Group::Misc);
        assert_eq!(Group::of("minecraft:text_display"), Group::Display);
        assert_eq!(Group::of("minecraft:oak_boat"), Group::Other);
        assert_eq!(Group::of("minecraft:arrow"), Group::Other);
        assert_eq!(Group::of("mymod:thing"), Group::Other);
    }

    #[test]
    fn the_file_overrides_the_defaults_section_by_section() {
        let text = "world-settings:\n  default:\n    entity-activation-range:\n      animals: 32\n      monsters: 32\n    \
                    entity-tracking-range:\n      players: 64 # pvp\n      monsters: 40\n    max-tnt-per-tick: 100\n    \
                    max-players-seen-per-player: 40\n    far-entity-distance: 16\n    far-entity-update-multiplier: 3\n";
        let c = TrackingConfig::parse(text);
        assert_eq!(c.ranges, Some([64, 48, 40, 32, 128, 64]));
        assert_eq!((c.max_players_seen, c.far_distance, c.far_multiplier), (40, 16.0, 3));
        assert_eq!(TrackingConfig::parse(""), TrackingConfig::default());
        assert_eq!(c.range(Group::Players, 512), 64);
        assert_eq!(c.range(Group::Other, 0), 0);
        assert_eq!(c.range(Group::Own, 160), 160);
    }
}
