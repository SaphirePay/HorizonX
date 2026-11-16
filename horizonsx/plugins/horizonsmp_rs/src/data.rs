//! What HorizonSMP keeps about each player (one JSON file a player, loaded before they join, saved in batches off
//! the server thread) and the small leaderboard table made from it.

use horizon_kit::store::{Docs, File};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// Unix ms.
    pub first_join: u64,
    pub last_seen: u64,
    /// Seconds online, in total.
    pub playtime: u64,
    /// Claim blocks earned by playing and bought (on top of the starting amount).
    pub claim_blocks: i64,
    pub kills: u32,
    pub deaths: u32,
    pub mob_kills: u32,
    pub blocks_mined: u64,
    pub daily_streak: u32,
    /// Unix ms of the last daily claim.
    pub last_daily: u64,
    pub daily_total: u32,
    /// Playtime reward ids already taken.
    pub playtime_claimed: Vec<String>,
    /// The player turned their new-player PvP protection off.
    pub pvp_protection_off: bool,
    pub crates_opened: u32,
    pub money_earned_shop: f64,
}

pub fn players() -> &'static Docs<Profile> {
    static DOCS: OnceLock<Docs<Profile>> = OnceLock::new();
    DOCS.get_or_init(|| Docs::open(&horizon_kit::app::folder().join("players"), Duration::from_secs(5)))
}

pub fn get(uuid: &str) -> Profile {
    players().get(uuid)
}

pub fn edit<R>(uuid: &str, f: impl FnOnce(&mut Profile) -> R) -> R {
    players().edit(uuid, f)
}

/// One row of the leaderboards: what /top sorts, for players who are offline too.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Summary {
    pub name: String,
    pub playtime: u64,
    pub kills: u32,
    pub deaths: u32,
    pub blocks_mined: u64,
    pub mob_kills: u32,
    pub daily_streak: u32,
}

pub fn summaries() -> &'static File<HashMap<String, Summary>> {
    static FILE: OnceLock<File<HashMap<String, Summary>>> = OnceLock::new();
    FILE.get_or_init(|| File::open(&horizon_kit::app::folder().join("leaderboards.json"), Duration::from_secs(30)))
}

/// The leaderboard row of a player brought up to date from their profile.
pub fn summarize(uuid: &str, p: &Profile) {
    let row = Summary { name: p.name.clone(), playtime: p.playtime, kills: p.kills, deaths: p.deaths, blocks_mined: p.blocks_mined, mob_kills: p.mob_kills, daily_streak: p.daily_streak };
    summaries().edit(|m| {
        m.insert(uuid.to_owned(), row);
    });
}

/// Every known player's uuid by lower-case name (for commands about offline players).
pub fn uuid_of(name: &str) -> Option<(String, String)> {
    let lower = name.to_ascii_lowercase();
    if let Some(p) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name)) {
        return Some((p.get_unique_id(), p.get_name().to_owned()));
    }
    summaries().read(|m| m.iter().find(|(_, s)| s.name.to_ascii_lowercase() == lower).map(|(u, s)| (u.clone(), s.name.clone())))
}
