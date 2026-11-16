//! config.yml, read once at enable and on `/horizonsmp reload`, kept as plain values every part reads.

use horizon_kit::{Config, app, yaml};
use serde_yaml::Value as Yaml;
use std::sync::{Arc, RwLock};

pub const DEFAULT: &str = include_str!("../data/config.yml");

/// Money, crate keys and claim blocks: what a daily, playtime or first-join reward gives.
#[derive(Clone, Default, Debug)]
pub struct Reward {
    pub money: f64,
    pub keys: Vec<(String, u32)>,
    pub claim_blocks: i64,
}

impl Reward {
    fn read(node: Option<&Yaml>) -> Reward {
        let Some(n) = node else { return Reward::default() };
        let mut keys = Vec::new();
        if let Some(Yaml::Mapping(m)) = n.get("keys") {
            for (k, v) in m {
                if let (Some(k), Some(v)) = (yaml::key_str(k), v.as_u64()) {
                    keys.push((k.to_ascii_lowercase(), v as u32));
                }
            }
        }
        Reward { money: n.get("money").and_then(Yaml::as_f64).unwrap_or(0.0), keys, claim_blocks: n.get("claim-blocks").and_then(Yaml::as_i64).unwrap_or(0) }
    }
    /// `$500 • 1× Common Key • 500 claim blocks`, as lore lines.
    pub fn lore(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.money > 0.0 {
            out.push(format!("<#4ade80>  + {}", crate::ui::money(self.money)));
        }
        for (k, n) in &self.keys {
            out.push(format!("<#facc15>  + {n}× {} Key", crate::crates::display_name(k)));
        }
        if self.claim_blocks > 0 {
            out.push(format!("<#22d3ee>  + {} claim blocks", crate::ui::num(self.claim_blocks)));
        }
        out
    }
}

#[derive(Clone, Debug)]
pub struct PlaytimeReward {
    pub id: String,
    pub hours: u64,
    pub name: String,
    pub icon: String,
    pub reward: Reward,
}

#[derive(Clone, Default, Debug)]
pub struct Settings {
    pub server: String,
    pub website: String,
    pub store: String,
    pub discord: String,

    pub claims: bool,
    pub claim_tool: String,
    pub inspect_tool: String,
    pub starting_blocks: i64,
    pub blocks_per_hour: i64,
    pub max_earned_blocks: i64,
    pub block_price: f64,
    pub min_width: i32,
    pub min_area: i64,
    pub default_radius: i32,
    pub max_claims: usize,
    pub team_trusted: bool,
    pub claim_pvp_default: bool,
    pub claim_disabled_worlds: Vec<String>,
    pub show_seconds: u64,

    pub combat: bool,
    pub tag_seconds: u64,
    pub kill_on_logout: bool,
    pub blocked_commands: Vec<String>,
    pub protection_minutes: u64,

    pub daily: bool,
    pub daily_days: Vec<Reward>,
    pub playtime_rewards: Vec<PlaytimeReward>,

    pub first_join: bool,
    pub first_join_broadcast: bool,
    pub first_join_kit: Vec<String>,
    pub first_join_reward: Reward,

    pub bounty: bool,
    pub bounty_min: f64,
    pub bounty_max: f64,

    pub teams: bool,
    pub team_max: usize,
    pub team_name_min: usize,
    pub team_name_max: usize,
    pub team_warmup: u64,

    pub announce: bool,
    pub announce_minutes: u64,
    pub announce_lines: Vec<String>,

    pub motd: bool,
    pub motd1: String,
    pub motd2: String,

    pub rules: Vec<String>,
}

impl Settings {
    pub fn load() -> Settings {
        let c = Config::load(&app::folder(), "config.yml", DEFAULT);
        let mut daily_days = Vec::new();
        for d in 1..=7 {
            daily_days.push(Reward::read(c.at(&format!("daily.days.{d}"))));
        }
        let mut playtime_rewards = Vec::new();
        if let Some(Yaml::Mapping(m)) = c.at("playtime-rewards") {
            for (k, v) in m {
                let Some(id) = yaml::key_str(k) else { continue };
                playtime_rewards.push(PlaytimeReward {
                    id: id.clone(),
                    hours: v.get("hours").and_then(Yaml::as_u64).unwrap_or(1),
                    name: yaml::string(v, "name").unwrap_or_else(|| id.clone()),
                    icon: yaml::string(v, "icon").unwrap_or_else(|| "CLOCK".into()),
                    reward: Reward::read(Some(v)),
                });
            }
        }
        playtime_rewards.sort_by_key(|r| r.hours);
        Settings {
            server: c.str("server-name", "Nyxora"),
            website: c.str("website", ""),
            store: c.str("store", ""),
            discord: c.str("discord", ""),
            claims: c.bool("claims.enabled", true),
            claim_tool: c.str("claims.tool", "GOLDEN_SHOVEL").to_ascii_uppercase(),
            inspect_tool: c.str("claims.inspect-tool", "STICK").to_ascii_uppercase(),
            starting_blocks: c.int("claims.starting-blocks", 1000),
            blocks_per_hour: c.int("claims.blocks-per-hour", 150),
            max_earned_blocks: c.int("claims.max-earned-blocks", 100000),
            block_price: c.num("claims.block-price", 10.0),
            min_width: c.int("claims.min-width", 5) as i32,
            min_area: c.int("claims.min-area", 25),
            default_radius: c.int("claims.default-radius", 10) as i32,
            max_claims: c.int("claims.max-claims", 20).max(1) as usize,
            team_trusted: c.bool("claims.team-members-trusted", true),
            claim_pvp_default: c.bool("claims.pvp-default", false),
            claim_disabled_worlds: c.list("claims.disabled-worlds"),
            show_seconds: c.int("claims.show-seconds", 15).max(3) as u64,
            combat: c.bool("combat.enabled", true),
            tag_seconds: c.int("combat.tag-seconds", 15).max(3) as u64,
            kill_on_logout: c.bool("combat.kill-on-logout", true),
            blocked_commands: c.list("combat.blocked-commands").into_iter().map(|s| s.to_ascii_lowercase()).collect(),
            protection_minutes: c.int("combat.new-player-protection-minutes", 30).max(0) as u64,
            daily: c.bool("daily.enabled", true),
            daily_days,
            playtime_rewards,
            first_join: c.bool("first-join.enabled", true),
            first_join_broadcast: c.bool("first-join.broadcast", true),
            first_join_kit: c.list("first-join.kit"),
            first_join_reward: Reward::read(c.at("first-join")),
            bounty: c.bool("bounty.enabled", true),
            bounty_min: c.num("bounty.min", 100.0),
            bounty_max: c.num("bounty.max", 1_000_000.0),
            teams: c.bool("teams.enabled", true),
            team_max: c.int("teams.max-members", 8).max(2) as usize,
            team_name_min: c.int("teams.name-min", 3).max(1) as usize,
            team_name_max: c.int("teams.name-max", 12).max(3) as usize,
            team_warmup: c.int("teams.home-warmup", 3).max(0) as u64,
            announce: c.bool("announcements.enabled", true),
            announce_minutes: c.int("announcements.minutes", 6).max(1) as u64,
            announce_lines: c.list("announcements.lines"),
            motd: c.bool("motd.enabled", true),
            motd1: c.str("motd.line1", ""),
            motd2: c.str("motd.line2", ""),
            rules: c.list("rules"),
        }
    }
}

static SETTINGS: RwLock<Option<Arc<Settings>>> = RwLock::new(None);

/// A whole-number LuckPerms meta value of a player (`lp group vip meta set horizonsmp.claim-blocks 5000`), cached
/// 30 s by the kit.
pub fn meta_int(uuid: &str, key: &str) -> Option<i64> {
    horizon_kit::hooks::chat::meta_int(uuid, key)
}

pub fn reload() {
    *SETTINGS.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(Settings::load()));
}

pub fn get() -> Arc<Settings> {
    if let Some(s) = SETTINGS.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return s.clone();
    }
    reload();
    SETTINGS.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

/// Hands a reward to an online player: money, keys (into their inventory) and claim blocks. Returns the lines it
/// gave, for the message.
pub fn give(p: &horizon_bukkit::Player, r: &Reward) {
    let name = p.get_name().to_owned();
    if r.money > 0.0 {
        horizon_kit::hooks::economy::deposit(&name, r.money);
    }
    for (k, n) in &r.keys {
        crate::crates::give_keys(p, k, *n);
    }
    if r.claim_blocks > 0 {
        let uuid = p.get_unique_id();
        crate::data::edit(&uuid, |d| d.claim_blocks += r.claim_blocks);
    }
}
