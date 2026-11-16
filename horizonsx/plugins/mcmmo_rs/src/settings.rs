//! config.yml, experience.yml, advanced.yml, skillranks.yml, sounds.yml, treasures, repair/salvage and
//! level_up_commands.yml, read once (start, /mcmmo reload) into tables the event handlers only look up.

use crate::skills::{ABILITIES, Ability, COUNT, Skill};
use horizon_kit::Config;
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, RwLock};

const CONFIG: &str = include_str!("../data/config.yml");
const EXPERIENCE: &str = include_str!("../data/experience.yml");
const ADVANCED: &str = include_str!("../data/advanced.yml");
const SKILLRANKS: &str = include_str!("../data/skillranks.yml");
const SOUNDS: &str = include_str!("../data/sounds.yml");
const TREASURES: &str = include_str!("../data/treasures.yml");
const FISHING_TREASURES: &str = include_str!("../data/fishing_treasures.yml");
const REPAIR: &str = include_str!("../data/repair.vanilla.yml");
const SALVAGE: &str = include_str!("../data/salvage.vanilla.yml");
const LEVEL_UP_COMMANDS: &str = include_str!("../data/level_up_commands.yml");
const PARTY: &str = include_str!("../data/party.yml");
const CHAT: &str = include_str!("../data/chat.yml");

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    Linear,
    Exponential,
}

#[derive(Clone)]
pub struct XpBar {
    pub enabled: bool,
    pub color: String,
    pub style: String,
}

#[derive(Clone)]
pub struct Treasure {
    pub item: String,
    pub amount: i32,
    pub xp: i32,
    pub chance: f64,
    pub level: i32,
    pub from: Vec<String>,
    pub rarity: String,
}

#[derive(Clone)]
pub struct Repairable {
    pub min_level: i32,
    pub xp_mult: f64,
    pub material: String,
    pub quantity: i32,
}

#[derive(Clone)]
pub struct LevelUpCommand {
    pub skills: Vec<Skill>,
    pub levels: Vec<i32>,
    pub commands: Vec<String>,
    pub as_player: bool,
    pub power: bool,
}

#[derive(Clone)]
pub struct SoundSpec {
    pub enabled: bool,
    pub volume: f32,
    pub pitch: f32,
    pub id: String,
}

pub struct Settings {
    pub retro: bool,
    pub power_cap: i32,
    pub level_cap: [i32; COUNT],
    pub save_minutes: u64,
    pub level_up_sounds: bool,
    pub broadcast_levelups: bool,
    pub broadcast_interval: i32,
    pub broadcast_power: bool,
    pub broadcast_power_interval: i32,
    pub broadcast_radius: Option<f64>,
    pub broadcast_same_world: bool,
    pub truncate: bool,
    pub leaderboard_refresh_secs: u64,
    pub use_scoreboards: bool,
    pub board_time: HashMap<&'static str, (bool, bool, i64)>,
    pub abilities_enabled: bool,
    pub ability_messages: bool,
    pub only_sneak: bool,
    pub cooldown: [i64; 8],
    pub max_seconds: [i64; 8],
    pub tree_feller_threshold: usize,
    pub durability_loss: i32,
    pub pvp: [bool; COUNT],
    pub pve: [bool; COUNT],
    pub detonator: String,
    pub death_penalty: f64,
    pub death_threshold: i32,
    pub death_skills: [bool; COUNT],
    pub vampirism: f64,
    pub vampirism_skills: [bool; COUNT],
    // party
    pub party_enabled: bool,
    pub party_max: i32,
    pub party_share_base: f64,
    pub party_share_increase: f64,
    pub party_share_cap: f64,
    pub party_range: f64,
    pub party_level_cap: i32,
    pub party_curve: i32,
    pub party_near_needed: bool,
    pub party_unlock: [i32; 5],
    pub party_friendly_fire: bool,
    pub chat_party: bool,
    pub chat_admin: bool,
    pub chat_display_names: bool,
    pub chat_console: bool,
    // experience.yml
    pub curve: Curve,
    pub base: i64,
    pub mult: f64,
    pub exponent: f64,
    pub cumulative: bool,
    pub global_mult: f64,
    pub pvp_mult: f64,
    pub spawner_mult: f64,
    pub egg_mult: f64,
    pub portal_mult: f64,
    pub bred_mult: f64,
    pub tamed_mult: f64,
    pub skill_mult: [f64; COUNT],
    pub custom_boost: f64,
    pub early_boost: bool,
    pub bars: Vec<XpBar>,
    pub bars_enabled: bool,
    pub bars_party: bool,
    pub bars_passive: bool,
    pub xp: [HashMap<String, f64>; COUNT],
    pub combat: HashMap<String, f64>,
    pub animals: f64,
    pub taming: HashMap<String, f64>,
    pub fish: HashMap<String, f64>,
    pub shake_xp: f64,
    pub dodge_xp: f64,
    pub roll_xp: f64,
    pub fall_xp: f64,
    pub feather_mult: f64,
    pub alchemy_stage: [f64; 5],
    pub archery_distance: f64,
    pub repair_base: f64,
    pub repair_tier: HashMap<String, f64>,
    pub pvp_rewards: bool,
    pub combat_ceiling: Option<f64>,
    pub tree_feller_reduced: bool,
    pub fishing_exploit: bool,
    pub fishing_range: f64,
    pub overfish: u32,
    // advanced.yml (dotted numbers) and the ranks
    pub adv: HashMap<String, f64>,
    pub adv_bool: HashMap<String, bool>,
    pub ranks: HashMap<String, Vec<i32>>,
    pub ability_length_cap: i32,
    pub ability_length_step: i32,
    pub notify_others: bool,
    pub action_bar: HashMap<String, (bool, bool)>,
    pub sounds: HashMap<String, SoundSpec>,
    pub master_volume: f32,
    pub excavation: Vec<Treasure>,
    pub fishing_treasures: Vec<Treasure>,
    pub fishing_rates: HashMap<(i32, String), f64>,
    pub shake: HashMap<String, Vec<Treasure>>,
    pub repairables: HashMap<String, Repairable>,
    pub salvageables: HashMap<String, Repairable>,
    pub level_up_commands: Vec<LevelUpCommand>,
    pub repair_anvil: String,
    pub salvage_anvil: String,
    pub repair_confirm: bool,
    pub salvage_confirm: bool,
}

static CURRENT: LazyLock<RwLock<Arc<Settings>>> = LazyLock::new(|| RwLock::new(Arc::new(build(&Path::new("")))));

/// The settings in force (cheap: an Arc clone).
pub fn get() -> Arc<Settings> {
    CURRENT.read().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn load(folder: &Path) {
    let s = build(folder);
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(s);
}

fn file(folder: &Path, name: &str, default: &str) -> Config {
    if folder.as_os_str().is_empty() {
        return Config::parse(default);
    }
    Config::load(folder, name, default)
}

/// Every number and bool of a document by dotted path.
fn flatten(v: &Yaml, prefix: &str, nums: &mut HashMap<String, f64>, bools: &mut HashMap<String, bool>) {
    match v {
        Yaml::Mapping(m) => {
            for (k, v) in m {
                let k = match k {
                    Yaml::String(s) => s.clone(),
                    Yaml::Number(n) => n.to_string(),
                    _ => continue,
                };
                let path = if prefix.is_empty() { k } else { format!("{prefix}.{k}") };
                flatten(v, &path, nums, bools);
            }
        }
        Yaml::Number(n) => {
            if let Some(f) = n.as_f64() {
                nums.insert(prefix.to_owned(), f);
            }
        }
        Yaml::Bool(b) => {
            bools.insert(prefix.to_owned(), *b);
        }
        Yaml::String(s) => {
            if let Ok(f) = s.trim().parse::<f64>() {
                nums.insert(prefix.to_owned(), f);
            }
        }
        _ => {}
    }
}

/// A section's `Key: number` pairs, the keys lower-cased (materials, entities).
fn table(c: &Config, path: &str) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    if let Some(Yaml::Mapping(m)) = c.at(path) {
        for (k, v) in m {
            if let (Some(k), Some(n)) = (k.as_str(), v.as_f64().or_else(|| v.as_i64().map(|i| i as f64))) {
                out.insert(k.to_ascii_lowercase(), n);
            }
        }
    }
    out
}

fn list_or_one(v: Option<&Yaml>) -> Vec<String> {
    match v {
        Some(Yaml::Sequence(items)) => items.iter().filter_map(|i| i.as_str().map(str::to_owned).or_else(|| i.as_i64().map(|n| n.to_string()))).collect(),
        Some(Yaml::String(s)) => vec![s.clone()],
        Some(Yaml::Number(n)) => vec![n.to_string()],
        _ => Vec::new(),
    }
}

fn build(folder: &Path) -> Settings {
    let c = file(folder, "config.yml", CONFIG);
    let e = file(folder, "experience.yml", EXPERIENCE);
    let a = file(folder, "advanced.yml", ADVANCED);
    let r = file(folder, "skillranks.yml", SKILLRANKS);
    let snd = file(folder, "sounds.yml", SOUNDS);
    let t = file(folder, "treasures.yml", TREASURES);
    let ft = file(folder, "fishing_treasures.yml", FISHING_TREASURES);
    let rep = file(folder, "repair.vanilla.yml", REPAIR);
    let sal = file(folder, "salvage.vanilla.yml", SALVAGE);
    let luc = file(folder, "level_up_commands.yml", LEVEL_UP_COMMANDS);
    let party = file(folder, "party.yml", PARTY);
    let chat = file(folder, "chat.yml", CHAT);
    for (name, text) in [("coreskills.yml", include_str!("../data/coreskills.yml")), ("potions.yml", include_str!("../data/potions.yml")), ("persistent_data.yml", include_str!("../data/persistent_data.yml")), ("itemweights.yml", include_str!("../data/itemweights.yml"))] {
        if !folder.as_os_str().is_empty() {
            let _ = Config::load(folder, name, text);
        }
    }

    let retro = c.bool("General.RetroMode.Enabled", true);
    let mode = if retro { "RetroMode" } else { "Standard" };

    let mut level_cap = [0; COUNT];
    let mut pvp = [true; COUNT];
    let mut pve = [true; COUNT];
    let mut death_skills = [false; COUNT];
    let mut vampirism_skills = [false; COUNT];
    let mut skill_mult = [1.0; COUNT];
    let mut bars = Vec::new();
    let mut xp: [HashMap<String, f64>; COUNT] = Default::default();
    for s in crate::skills::ALL {
        let n = s.name();
        level_cap[s.idx()] = c.int(&format!("Skills.{n}.Level_Cap"), 0) as i32;
        pvp[s.idx()] = c.bool(&format!("Skills.{n}.Enabled_For_PVP"), true);
        pve[s.idx()] = c.bool(&format!("Skills.{n}.Enabled_For_PVE"), true);
        death_skills[s.idx()] = c.bool(&format!("Hardcore.Death_Stat_Loss.Enabled.{n}"), false);
        vampirism_skills[s.idx()] = c.bool(&format!("Hardcore.Vampirism.Enabled.{n}"), false);
        skill_mult[s.idx()] = e.num(&format!("Experience_Formula.Skill_Multiplier.{n}"), 1.0);
        bars.push(XpBar {
            enabled: e.bool(&format!("Experience_Bars.{n}.Enable"), true),
            color: e.str(&format!("Experience_Bars.{n}.Color"), "GREEN"),
            style: e.str(&format!("Experience_Bars.{n}.BarStyle"), "SEGMENTED_6"),
        });
        xp[s.idx()] = table(&e, &format!("Experience_Values.{n}"));
    }
    let herb_flowers = table(&e, "Experience_Values.Herbalism");
    xp[Skill::Herbalism.idx()] = herb_flowers;

    let mut cooldown = [240; 8];
    let mut max_seconds = [0; 8];
    for ab in ABILITIES {
        cooldown[ab.idx()] = c.int(&format!("Abilities.Cooldowns.{}", ab.config_key()), 240);
        max_seconds[ab.idx()] = c.int(&format!("Abilities.Max_Seconds.{}", ab.config_key()), 0);
    }

    let mut adv = HashMap::new();
    let mut adv_bool = HashMap::new();
    flatten(&a.doc, "", &mut adv, &mut adv_bool);

    let mut ranks = HashMap::new();
    if let Some(Yaml::Mapping(skills)) = Some(&r.doc) {
        for (sk, subs) in skills {
            let (Some(sk), Some(subs)) = (sk.as_str(), subs.as_mapping()) else { continue };
            for (sub, modes) in subs {
                let Some(sub) = sub.as_str() else { continue };
                let mut levels = Vec::new();
                if let Some(Yaml::Mapping(m)) = modes.get(mode) {
                    let mut i = 1;
                    while let Some(v) = m.get(Yaml::String(format!("Rank_{i}"))) {
                        levels.push(v.as_i64().unwrap_or(0) as i32);
                        i += 1;
                    }
                }
                ranks.insert(format!("{sk}.{sub}"), levels);
            }
        }
    }

    let mut action_bar = HashMap::new();
    for key in a.keys("Feedback.ActionBarNotifications") {
        let p = format!("Feedback.ActionBarNotifications.{key}");
        let enabled = a.bool(&format!("{p}.Enabled"), a.bool(&format!("{p}.SendToActionBar"), true));
        let chat = a.bool(&format!("{p}.SendCopyOfMessageToChat"), false);
        action_bar.insert(key, (enabled, chat));
    }

    let mut sounds = HashMap::new();
    for key in snd.keys("Sounds") {
        if key == "MasterVolume" {
            continue;
        }
        let p = format!("Sounds.{key}");
        sounds.insert(key.clone(), SoundSpec { enabled: snd.bool(&format!("{p}.Enable"), true), volume: snd.num(&format!("{p}.Volume"), 1.0) as f32, pitch: snd.num(&format!("{p}.Pitch"), 1.0) as f32, id: snd.str(&format!("{p}.CustomSoundId"), "") });
    }

    let lvl_key = if retro { "Retro_Mode" } else { "Standard_Mode" };
    let mut excavation = Vec::new();
    for item in t.keys("Excavation") {
        let p = format!("Excavation.{item}");
        excavation.push(Treasure {
            item: item.split('|').next().unwrap_or(&item).to_ascii_lowercase(),
            amount: t.int(&format!("{p}.Amount"), 1) as i32,
            xp: t.int(&format!("{p}.XP"), 0) as i32,
            chance: t.num(&format!("{p}.Drop_Chance"), 0.0),
            level: t.int(&format!("{p}.Level_Requirement.{lvl_key}"), 0) as i32,
            from: list_or_one(t.at(&format!("{p}.Drops_From"))).into_iter().map(|s| s.to_ascii_lowercase()).collect(),
            rarity: String::new(),
        });
    }
    let mut fishing_treasures = Vec::new();
    for item in ft.keys("Fishing") {
        let p = format!("Fishing.{item}");
        fishing_treasures.push(Treasure {
            item: item.split('|').next().unwrap_or(&item).to_ascii_lowercase(),
            amount: ft.int(&format!("{p}.Amount"), 1) as i32,
            xp: ft.int(&format!("{p}.XP"), 0) as i32,
            chance: 0.0,
            level: 0,
            from: Vec::new(),
            rarity: ft.str(&format!("{p}.Rarity"), "COMMON").to_ascii_uppercase(),
        });
    }
    let mut fishing_rates = HashMap::new();
    for tier in ft.keys("Item_Drop_Rates") {
        let n: i32 = tier.trim_start_matches("Tier_").parse().unwrap_or(0);
        for rarity in ft.keys(&format!("Item_Drop_Rates.{tier}")) {
            fishing_rates.insert((n, rarity.to_ascii_uppercase()), ft.num(&format!("Item_Drop_Rates.{tier}.{rarity}"), 0.0));
        }
    }
    let mut shake = HashMap::new();
    for mob in ft.keys("Shake") {
        let mut drops = Vec::new();
        for item in ft.keys(&format!("Shake.{mob}")) {
            let p = format!("Shake.{mob}.{item}");
            drops.push(Treasure {
                item: item.split('|').next().unwrap_or(&item).to_ascii_lowercase(),
                amount: ft.int(&format!("{p}.Amount"), 1) as i32,
                xp: ft.int(&format!("{p}.XP"), 0) as i32,
                chance: ft.num(&format!("{p}.Drop_Chance"), 0.0),
                level: ft.int(&format!("{p}.Drop_Level"), 0) as i32,
                from: Vec::new(),
                rarity: String::new(),
            });
        }
        shake.insert(mob.to_ascii_lowercase(), drops);
    }

    let repairable = |cfg: &Config, root: &str| {
        let mut out = HashMap::new();
        for item in cfg.keys(root) {
            let p = format!("{root}.{item}");
            let lower = item.to_ascii_lowercase();
            let material = cfg.str(&format!("{p}.RepairMaterial"), &cfg.str(&format!("{p}.SalvageMaterial"), "")).to_ascii_lowercase();
            out.insert(
                lower.clone(),
                Repairable {
                    min_level: cfg.int(&format!("{p}.MinimumLevel"), 0) as i32,
                    xp_mult: cfg.num(&format!("{p}.XpMultiplier"), 1.0),
                    material: if material.is_empty() { repair_material(&lower).to_owned() } else { material },
                    quantity: cfg.int(&format!("{p}.MaximumQuantity"), cfg.int(&format!("{p}.MinimumQuantity"), default_quantity(&lower))) as i32,
                },
            );
        }
        out
    };

    let mut level_up_commands = Vec::new();
    for key in luc.keys("level_up_commands") {
        let p = format!("level_up_commands.{key}");
        if !luc.bool(&format!("{p}.enabled"), false) {
            continue;
        }
        let names = list_or_one(luc.at(&format!("{p}.skills")));
        let all = names.iter().any(|n| n.eq_ignore_ascii_case("all"));
        let power = names.iter().any(|n| n.eq_ignore_ascii_case("power") || n.eq_ignore_ascii_case("powerlevel"));
        let skills = if all { Skill::non_child().collect() } else { names.iter().filter_map(|n| crate::skills::ALL.iter().copied().find(|s| s.name().eq_ignore_ascii_case(n))).collect() };
        level_up_commands.push(LevelUpCommand {
            skills,
            levels: list_or_one(luc.at(&format!("{p}.levels"))).iter().filter_map(|l| l.parse().ok()).collect(),
            commands: list_or_one(luc.at(&format!("{p}.commands"))),
            as_player: luc.str(&format!("{p}.run_as"), "CONSOLE").eq_ignore_ascii_case("PLAYER"),
            power,
        });
    }

    let mut board_time = HashMap::new();
    for kind in ["Rank", "Top", "Stats", "Inspect", "Cooldown", "Skill"] {
        let p = format!("Scoreboard.Types.{kind}");
        board_time.insert(match kind {
            "Rank" => "Rank",
            "Top" => "Top",
            "Stats" => "Stats",
            "Inspect" => "Inspect",
            "Cooldown" => "Cooldown",
            _ => "Skill",
        }, (c.bool(&format!("{p}.Print"), true), c.bool(&format!("{p}.Board"), true), c.int(&format!("{p}.Display_Time"), 10)));
    }

    let combat = table(&e, "Experience_Values.Combat.Multiplier");
    let animals = combat.get("animals").copied().unwrap_or(1.0);
    let length_mode = if retro { "RetroMode" } else { "Standard" };
    Settings {
        retro,
        power_cap: c.int("General.Power_Level_Cap", 0) as i32,
        level_cap,
        save_minutes: c.int("General.Save_Interval", 10).max(1) as u64,
        level_up_sounds: c.bool("General.LevelUp_Sounds", true),
        broadcast_levelups: c.bool("General.Level_Up_Chat_Broadcasts.Enabled", true),
        broadcast_interval: c.int("General.Level_Up_Chat_Broadcasts.Milestone_Interval", 100).max(1) as i32,
        broadcast_power: c.bool("General.Level_Up_Chat_Broadcasts.Broadcast_Powerlevels.Enabled", true),
        broadcast_power_interval: c.int("General.Level_Up_Chat_Broadcasts.Broadcast_Powerlevels.Milestone_Interval", 100).max(1) as i32,
        broadcast_radius: c.bool("General.Level_Up_Chat_Broadcasts.Broadcast_Targets.Distance_Restrictions.Restrict_Distance", false).then(|| c.num("General.Level_Up_Chat_Broadcasts.Broadcast_Targets.Distance_Restrictions.Restricted_Radius", 100.0)),
        broadcast_same_world: c.bool("General.Level_Up_Chat_Broadcasts.Broadcast_Targets.Only_Same_World", false),
        truncate: c.bool("General.TruncateSkills", true),
        leaderboard_refresh_secs: c.int("General.Leaderboards.Refresh_Interval_Seconds.FlatFile", 600).max(30) as u64,
        use_scoreboards: c.bool("Scoreboard.UseScoreboards", false),
        board_time,
        abilities_enabled: c.bool("Abilities.Enabled", true),
        ability_messages: c.bool("Abilities.Messages", true),
        only_sneak: c.bool("Abilities.Activation.Only_Activate_When_Sneaking", false),
        cooldown,
        max_seconds,
        tree_feller_threshold: c.int("Abilities.Limits.Tree_Feller_Threshold", 1000).max(1) as usize,
        durability_loss: c.int("Abilities.Tools.Durability_Loss", 1) as i32,
        pvp,
        pve,
        detonator: c.str("Skills.Mining.Detonator_Name", "FLINT_AND_STEEL").to_ascii_lowercase(),
        death_penalty: c.num("Hardcore.Death_Stat_Loss.Penalty_Percentage", 75.0),
        death_threshold: c.int("Hardcore.Death_Stat_Loss.Level_Threshold", 0) as i32,
        death_skills,
        vampirism: c.num("Hardcore.Vampirism.Leech_Percentage", 5.0),
        vampirism_skills,
        party_enabled: party.bool("Party.Enabled", true),
        party_max: c.int("Party.MaxSize", -1) as i32,
        party_share_base: c.num("Party.Sharing.ExpShare_bonus_base", 1.1),
        party_share_increase: c.num("Party.Sharing.ExpShare_bonus_increase", 1.05),
        party_share_cap: c.num("Party.Sharing.ExpShare_bonus_cap", 1.5),
        party_range: c.num("Party.Sharing.Range", 75.0),
        party_level_cap: c.int("Party.Leveling.Level_Cap", 10) as i32,
        party_curve: c.int("Party.Leveling.Xp_Curve_Modifier", 3) as i32,
        party_near_needed: c.bool("Party.Leveling.Near_Members_Needed", false),
        party_unlock: [
            c.int("Party.Leveling.Chat_UnlockLevel", 1) as i32,
            c.int("Party.Leveling.Teleport_UnlockLevel", 2) as i32,
            c.int("Party.Leveling.Alliance_UnlockLevel", 5) as i32,
            c.int("Party.Leveling.ItemShare_UnlockLevel", 8) as i32,
            c.int("Party.Leveling.XpShare_UnlockLevel", 10) as i32,
        ],
        party_friendly_fire: c.bool("Party.FriendlyFire", false),
        chat_party: chat.bool("Chat.Enable", true) && chat.bool("Chat.Channels.Party.Enable", true),
        chat_admin: chat.bool("Chat.Enable", true) && chat.bool("Chat.Channels.Admin.Enable", true),
        chat_display_names: chat.bool("Chat.Channels.Party.Use_Display_Names", true),
        chat_console: chat.bool("Chat.Channels.Party.Send_To_Console", true),
        curve: if e.str("Experience_Formula.Curve", "LINEAR").eq_ignore_ascii_case("EXPONENTIAL") { Curve::Exponential } else { Curve::Linear },
        base: if e.str("Experience_Formula.Curve", "LINEAR").eq_ignore_ascii_case("EXPONENTIAL") { e.int("Experience_Formula.Exponential_Values.base", 2000) } else { e.int("Experience_Formula.Linear_Values.base", 1020) },
        mult: if e.str("Experience_Formula.Curve", "LINEAR").eq_ignore_ascii_case("EXPONENTIAL") { e.num("Experience_Formula.Exponential_Values.multiplier", 0.1) } else { e.num("Experience_Formula.Linear_Values.multiplier", 20.0) },
        exponent: e.num("Experience_Formula.Exponential_Values.exponent", 1.8),
        cumulative: e.bool("Experience_Formula.Cumulative_Curve", false),
        global_mult: e.num("Experience_Formula.Multiplier.Global", 1.0),
        pvp_mult: e.num("Experience_Formula.Multiplier.PVP", 1.0),
        spawner_mult: e.num("Experience_Formula.Mobspawners.Multiplier", 0.0),
        egg_mult: e.num("Experience_Formula.Eggs.Multiplier", 0.0),
        portal_mult: e.num("Experience_Formula.Nether_Portal.Multiplier", 0.0),
        bred_mult: e.num("Experience_Formula.Breeding.Multiplier", 1.0),
        tamed_mult: e.num("Experience_Formula.Player_Tamed.Multiplier", 0.0),
        skill_mult,
        custom_boost: e.num("Experience_Formula.Custom_XP_Perk.Boost", 1.25),
        early_boost: e.bool("EarlyGameBoost.Enabled", true),
        bars,
        bars_enabled: e.bool("Experience_Bars.Enable", true),
        bars_party: e.bool("Experience_Bars.Update.Party", true),
        bars_passive: e.bool("Experience_Bars.Update.Passive", true),
        xp,
        combat,
        animals,
        taming: table(&e, "Experience_Values.Taming.Animal_Taming"),
        fish: table(&e, "Experience_Values.Fishing"),
        shake_xp: e.num("Experience_Values.Fishing.Shake", 50.0),
        dodge_xp: e.num("Experience_Values.Acrobatics.Dodge", 800.0),
        roll_xp: e.num("Experience_Values.Acrobatics.Roll", 600.0),
        fall_xp: e.num("Experience_Values.Acrobatics.Fall", 600.0),
        feather_mult: e.num("Experience_Values.Acrobatics.FeatherFall_Multiplier", 2.0),
        alchemy_stage: [1, 2, 3, 4, 5].map(|i| e.num(&format!("Experience_Values.Alchemy.Potion_Brewing.Stage_{i}"), 0.0)),
        archery_distance: e.num("Experience_Values.Archery.Distance_Multiplier", 0.025),
        repair_base: e.num("Experience_Values.Repair.Base", 1000.0),
        repair_tier: table(&e, "Experience_Values.Repair"),
        pvp_rewards: e.bool("Experience_Values.PVP.Rewards", true),
        combat_ceiling: e.bool("ExploitFix.Combat.XPCeiling.Enabled", true).then(|| e.num("ExploitFix.Combat.XPCeiling.Damage_Limit", 100.0)),
        tree_feller_reduced: e.bool("ExploitFix.TreeFellerReducedXP", true),
        fishing_exploit: e.bool("ExploitFix.Fishing", true),
        fishing_range: e.num("Fishing_ExploitFix_Options.MoveRange", 3.0),
        overfish: e.int("Fishing_ExploitFix_Options.OverFishLimit", 10).max(1) as u32,
        ability_length_cap: a.int(&format!("Skills.General.Ability.Length.{length_mode}.CapLevel"), if retro { 1000 } else { 100 }) as i32,
        ability_length_step: a.int(&format!("Skills.General.Ability.Length.{length_mode}.IncreaseLevel"), if retro { 50 } else { 5 }).max(1) as i32,
        notify_others: a.bool("Feedback.Events.AbilityActivation.SendNotificationToOtherPlayers", true),
        adv,
        adv_bool,
        ranks,
        action_bar,
        master_volume: snd.num("Sounds.MasterVolume", 1.0) as f32,
        sounds,
        excavation,
        fishing_treasures,
        fishing_rates,
        shake,
        repairables: repairable(&rep, "Repairables"),
        salvageables: repairable(&sal, "Salvageables"),
        level_up_commands,
        repair_anvil: c.str("Skills.Repair.Anvil_Material", "IRON_BLOCK").to_ascii_lowercase(),
        salvage_anvil: c.str("Skills.Salvage.Anvil_Material", "GOLD_BLOCK").to_ascii_lowercase(),
        repair_confirm: c.bool("Skills.Repair.Confirm_Required", true),
        salvage_confirm: c.bool("Skills.Salvage.Confirm_Required", true),
    }
}

/// What an item is repaired with, by its name (mcMMO's defaults when the file doesn't say).
pub fn repair_material(item: &str) -> &'static str {
    let id = item.trim_start_matches("minecraft:");
    if id.starts_with("wooden_") || id == "shield" {
        "oak_planks"
    } else if id.starts_with("stone_") {
        "cobblestone"
    } else if id.starts_with("copper_") {
        "copper_ingot"
    } else if id.starts_with("iron_") || id.starts_with("chainmail_") {
        "iron_ingot"
    } else if id.starts_with("golden_") {
        "gold_ingot"
    } else if id.starts_with("diamond_") {
        "diamond"
    } else if id.starts_with("netherite_") {
        "netherite_scrap"
    } else if id.starts_with("leather_") {
        "leather"
    } else if matches!(id, "bow" | "fishing_rod" | "crossbow" | "carrot_on_a_stick") {
        "string"
    } else if id == "mace" {
        "breeze_rod"
    } else if id == "trident" {
        "prismarine_crystals"
    } else if id == "elytra" {
        "phantom_membrane"
    } else {
        "iron_ingot"
    }
}

/// How much of its material an item is made of (salvage's full yield).
pub fn default_quantity(item: &str) -> i64 {
    let id = item.trim_start_matches("minecraft:");
    match id.rsplit('_').next().unwrap_or(id) {
        "sword" | "hoe" => 2,
        "shovel" | "spear" => 1,
        "pickaxe" | "axe" => 3,
        "helmet" => 5,
        "chestplate" => 8,
        "leggings" => 7,
        "boots" => 4,
        "bow" | "rod" | "crossbow" => 3,
        _ => 1,
    }
}

impl Settings {
    /// A number of advanced.yml by its dotted path.
    pub fn adv(&self, path: &str, default: f64) -> f64 {
        self.adv.get(path).copied().unwrap_or(default)
    }
    /// `X.MaxBonusLevel.<mode>` for a sub-skill section.
    pub fn max_bonus(&self, section: &str) -> f64 {
        let mode = if self.retro { "RetroMode" } else { "Standard" };
        self.adv.get(&format!("{section}.MaxBonusLevel.{mode}")).copied().unwrap_or(if self.retro { 1000.0 } else { 100.0 })
    }
    /// The chance (0-100) of a sub-skill that grows with level to `ChanceMax` at `MaxBonusLevel`.
    pub fn chance(&self, section: &str, level: i32) -> f64 {
        let max = self.adv(&format!("{section}.ChanceMax"), 100.0);
        let cap = self.max_bonus(section).max(1.0);
        max * (level as f64).min(cap) / cap
    }
    /// The rank (0 = locked) a level gives in a sub-skill (`Mining.DoubleDrops`).
    pub fn rank(&self, key: &str, level: i32) -> usize {
        match self.ranks.get(key) {
            Some(levels) => levels.iter().filter(|l| level >= **l).count(),
            None => 1,
        }
    }
    pub fn unlock_level(&self, key: &str) -> i32 {
        self.ranks.get(key).and_then(|l| l.first().copied()).unwrap_or(0)
    }
    /// A `Rank_N` value of a section of advanced.yml.
    pub fn by_rank(&self, section: &str, rank: usize, default: f64) -> f64 {
        if rank == 0 {
            return 0.0;
        }
        self.adv(&format!("{section}.Rank_{rank}"), default)
    }
    /// XP to the next level from `level` (FormulaManager: retro = the formula, standard = ten retro levels summed).
    pub fn xp_to_next(&self, level: i32) -> i64 {
        let one = |l: i64| -> i64 {
            let need = match self.curve {
                Curve::Linear => self.base as f64 + l as f64 * self.mult,
                Curve::Exponential => self.mult * (l as f64).powf(self.exponent) + self.base as f64,
            };
            (need.floor() as i64).clamp(1, i32::MAX as i64)
        };
        if self.retro {
            one(level as i64)
        } else {
            let start = level as i64 * 10 + 1;
            (start..start + 10).map(one).sum::<i64>().min(i32::MAX as i64)
        }
    }
    pub fn cap(&self, skill: Skill) -> i32 {
        let c = self.level_cap[skill.idx()];
        if c <= 0 { i32::MAX } else { c }
    }
    pub fn ability_cooldown(&self, ab: Ability) -> i64 {
        self.cooldown[ab.idx()]
    }
    /// Combat XP multiplier of a mob type (`minecraft:zombie`).
    pub fn combat_mult(&self, kind: &str) -> f64 {
        let k = kind.trim_start_matches("minecraft:");
        match self.combat.get(k) {
            Some(v) => *v,
            None => match k {
                "zombified_piglin" => self.combat.get("pig_zombie").copied().unwrap_or(3.0),
                "mooshroom" => self.combat.get("mushroom_cow").copied().unwrap_or(1.2),
                "snow_golem" => self.combat.get("snowman").copied().unwrap_or(0.0),
                _ => self.animals,
            },
        }
    }
    pub fn notify(&self, kind: &str) -> (bool, bool) {
        self.action_bar.get(kind).copied().unwrap_or((true, false))
    }
}
