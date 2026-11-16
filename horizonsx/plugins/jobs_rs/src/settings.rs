//! generalConfig.yml, locale/messages_<lang>.yml, jobs/*.yml (or the older jobConfig.yml), restrictedBlocks.yml,
//! shopItems.yml and schedule.yml, read into tables the handlers only look up.

use crate::eq::{self, Expr};
use horizon_kit::Config;
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, RwLock};

const GENERAL: &str = include_str!("../data/generalConfig.yml");
const MESSAGES_EN: &str = include_str!("../data/locale/messages_en.yml");
const RESTRICTED: &str = include_str!("../data/restrictedBlocks.yml");
const SHOP: &str = include_str!("../data/shopItems.yml");
const SCHEDULE: &str = include_str!("../data/schedule.yml");
const DEFAULT_JOBS: [(&str, &str); 14] = [
    ("_EXAMPLE.yml", include_str!("../data/jobs/_EXAMPLE.yml")),
    ("brewer.yml", include_str!("../data/jobs/brewer.yml")),
    ("builder.yml", include_str!("../data/jobs/builder.yml")),
    ("crafter.yml", include_str!("../data/jobs/crafter.yml")),
    ("digger.yml", include_str!("../data/jobs/digger.yml")),
    ("enchanter.yml", include_str!("../data/jobs/enchanter.yml")),
    ("explorer.yml", include_str!("../data/jobs/explorer.yml")),
    ("farmer.yml", include_str!("../data/jobs/farmer.yml")),
    ("fisherman.yml", include_str!("../data/jobs/fisherman.yml")),
    ("hunter.yml", include_str!("../data/jobs/hunter.yml")),
    ("miner.yml", include_str!("../data/jobs/miner.yml")),
    ("none.yml", include_str!("../data/jobs/none.yml")),
    ("weaponsmith.yml", include_str!("../data/jobs/weaponsmith.yml")),
    ("woodcutter.yml", include_str!("../data/jobs/woodcutter.yml")),
];

/// The actions Jobs pays for (ActionType), by their config section names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Break,
    StripLogs,
    TntBreak,
    Place,
    Kill,
    CustomKill,
    Fish,
    Craft,
    VTrade,
    Smelt,
    Brew,
    Enchant,
    Repair,
    Breed,
    Tame,
    Dye,
    Shear,
    Milk,
    Explore,
    Eat,
    Collect,
    Bake,
    Bucket,
    Brush,
    Vax,
}

pub const ACTIONS: [(Action, &str); 25] = [
    (Action::Break, "Break"),
    (Action::StripLogs, "StripLogs"),
    (Action::TntBreak, "TNTBreak"),
    (Action::Place, "Place"),
    (Action::Kill, "Kill"),
    (Action::CustomKill, "custom-kill"),
    (Action::Fish, "Fish"),
    (Action::Craft, "Craft"),
    (Action::VTrade, "VTrade"),
    (Action::Smelt, "Smelt"),
    (Action::Brew, "Brew"),
    (Action::Enchant, "Enchant"),
    (Action::Repair, "Repair"),
    (Action::Breed, "Breed"),
    (Action::Tame, "Tame"),
    (Action::Dye, "Dye"),
    (Action::Shear, "Shear"),
    (Action::Milk, "Milk"),
    (Action::Explore, "Explore"),
    (Action::Eat, "Eat"),
    (Action::Collect, "Collect"),
    (Action::Bake, "Bake"),
    (Action::Bucket, "Bucket"),
    (Action::Brush, "Brush"),
    (Action::Vax, "Vax"),
];

impl Action {
    pub fn name(self) -> &'static str {
        ACTIONS.iter().find(|(a, _)| *a == self).map(|(_, n)| *n).unwrap_or("Break")
    }
    pub fn parse(s: &str) -> Option<Action> {
        ACTIONS.iter().find(|(_, n)| n.eq_ignore_ascii_case(s)).map(|(a, _)| *a)
    }
}

#[derive(Clone)]
pub struct Info {
    pub income: f64,
    pub points: f64,
    pub exp: f64,
    pub from: i32,
    pub until: i32,
    pub soft_income: Option<i32>,
    pub soft_exp: Option<i32>,
    pub soft_points: Option<i32>,
}

#[derive(Clone)]
pub struct LevelCommand {
    pub commands: Vec<String>,
    pub from: i32,
    pub until: i32,
}

#[derive(Clone)]
pub struct Job {
    pub key: String,
    pub name: String,
    pub display: String,
    pub short: String,
    pub description: String,
    pub full_description: Vec<String>,
    pub color: String,
    pub max_level: i32,
    pub vip_max_level: i32,
    pub slots: i32,
    pub rejoin_cooldown: i64,
    pub max_exp: Expr,
    pub income_eq: Expr,
    pub points_eq: Expr,
    pub exp_eq: Expr,
    pub gui_item: String,
    pub actions: HashMap<Action, HashMap<String, Info>>,
    pub commands: Vec<LevelCommand>,
    pub max_level_commands: Vec<String>,
    pub world_blacklist: Vec<String>,
    pub world_whitelist: bool,
    pub ignore_max: bool,
    pub cmd_on_join: Vec<String>,
    pub cmd_on_leave: Vec<String>,
    pub permissions: Vec<(String, bool, i32)>,
    pub soft_income: Option<i32>,
    pub soft_exp: Option<i32>,
    pub soft_points: Option<i32>,
    pub is_none: bool,
}

impl Job {
    pub fn max_exp(&self, level: i32, numjobs: usize) -> f64 {
        self.max_exp.eval(&[("joblevel", level as f64), ("numjobs", numjobs as f64)]).max(1.0)
    }
    pub fn info(&self, action: Action, key: &str, level: i32) -> Option<&Info> {
        let i = self.actions.get(&action)?.get(key)?;
        if level < i.from || (i.until > 0 && level > i.until) {
            return None;
        }
        Some(i)
    }
    /// (income, points, exp) for an action at a level.
    pub fn pay(&self, info: &Info, level: i32, numjobs: usize, maxjobs: i32) -> (f64, f64, f64) {
        let lvl = |soft: Option<i32>, own: Option<i32>| -> f64 {
            let cap = own.or(soft);
            match cap {
                Some(c) if level > c => c as f64,
                _ => level as f64,
            }
        };
        let base = |eq: &Expr, name: &str, v: f64, l: f64| -> f64 {
            if v == 0.0 {
                return 0.0;
            }
            eq.eval(&[("joblevel", l), ("numjobs", numjobs as f64), ("maxjobs", maxjobs as f64), (name, v)])
        };
        (
            base(&self.income_eq, "baseincome", info.income, lvl(self.soft_income, info.soft_income)),
            base(&self.points_eq, "basepoints", info.points, lvl(self.soft_points, info.soft_points)),
            base(&self.exp_eq, "baseexperience", info.exp, lvl(self.soft_exp, info.soft_exp)),
        )
    }
    pub fn works_in(&self, world: &str) -> bool {
        let listed = self.world_blacklist.iter().any(|w| w.eq_ignore_ascii_case(world) || world.ends_with(&format!(":{}", w.to_ascii_lowercase())));
        if self.world_whitelist { listed } else { !listed }
    }
}

#[derive(Clone)]
pub struct Limit {
    pub enabled: bool,
    pub equation: Expr,
    pub seconds: i64,
    pub stop_with: Vec<char>,
}

#[derive(Clone)]
pub struct ShopItem {
    pub key: String,
    pub points: f64,
    pub icon: String,
    pub icon_name: String,
    pub lore: Vec<String>,
    pub perms: Vec<String>,
    pub job_levels: Vec<(String, i32)>,
    pub total_levels: i32,
    pub commands: Vec<String>,
    pub give: Vec<String>,
    pub hide_without_perm: bool,
}

pub struct Settings {
    pub save_minutes: u64,
    pub save_on_quit: bool,
    pub max_jobs: i32,
    pub hide_without_perm: bool,
    pub shop_enabled: bool,
    pub pay_near_spawner: bool,
    pub pay_creative: bool,
    pub add_xp_player: bool,
    pub batch_secs: u64,
    pub min_money: Option<f64>,
    pub min_points: Option<f64>,
    pub min_exp: Option<f64>,
    pub negative_boost: bool,
    pub limits: [Limit; 3],
    pub break_timer: i64,
    pub ignore_ore_generators: bool,
    pub silk_protection: bool,
    pub block_timers: HashMap<String, i64>,
    pub level_loss: f64,
    pub fix_at_max: bool,
    pub action_bars: bool,
    pub bossbar: bool,
    pub bossbar_timer: u64,
    pub default_actionbar: String,
    pub default_bossbar: String,
    pub default_chat: String,
    pub sound_levelup: Option<(f32, f32)>,
    pub sound_title: Option<(f32, f32)>,
    pub broadcast_levelup: bool,
    pub broadcast_levels: Vec<i32>,
    pub top_amount: usize,
    pub hidden_top: Vec<String>,
    pub decimals: [usize; 3],
    pub disabled_worlds: Vec<String>,
    pub disabled_whitelist: bool,
    pub gui_on_browse: bool,
    pub chat_browse: bool,
    pub gui_rows: i64,
    pub disable_payment_max: bool,
    pub tameables_payout: bool,
    pub monster_damage: Option<f64>,
    pub explore_flying: bool,
    pub jobs: Vec<Job>,
    pub none: Option<Job>,
    pub shop: Vec<ShopItem>,
    pub schedule: Vec<Schedule>,
    /// Whether any job pays for an action (skip the event early when none does).
    pub uses: HashMap<Action, bool>,
    pub lang: HashMap<String, String>,
    pub lang_lists: HashMap<String, Vec<String>>,
}

#[derive(Clone)]
pub struct Schedule {
    pub name: String,
    pub enabled: bool,
    pub from: u32,
    pub until: u32,
    pub money: f64,
    pub exp: f64,
    pub points: f64,
    pub days: Vec<String>,
    pub jobs: Vec<String>,
    pub start_msg: Vec<String>,
    pub stop_msg: Vec<String>,
}

static CURRENT: LazyLock<RwLock<Arc<Settings>>> = LazyLock::new(|| RwLock::new(Arc::new(build(Path::new("")))));

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

/// Old names in job files to today's ids (Jobs' CMIMaterial / CMIEntityType did the same).
pub fn modern(key: &str) -> String {
    let k = key.trim().to_ascii_lowercase().trim_start_matches("minecraft:").to_owned();
    let (base, suffix) = match k.split_once('-') {
        Some((b, s)) if s.chars().all(|c| c.is_ascii_digit()) => (b.to_owned(), format!("-{s}")),
        _ => (k.clone(), String::new()),
    };
    let base = match base.as_str() {
        "raw_fish" => "cod",
        "raw_salmon" => "salmon",
        "nether_stalk" | "nether_warts" => "nether_wart",
        "mushroom_cow" => "mooshroom",
        "pig_zombie" => "zombified_piglin",
        "snowman" => "snow_golem",
        "grass" => "short_grass",
        "sulphur" => "gunpowder",
        "ink_sack" => "ink_sac",
        "dig_speed" => "efficiency",
        "damage_all" => "sharpness",
        "damage_undead" => "smite",
        "damage_arthropods" => "bane_of_arthropods",
        "durability" => "unbreaking",
        "loot_bonus_blocks" => "fortune",
        "loot_bonus_mobs" => "looting",
        "protection_environmental" => "protection",
        "protection_fire" => "fire_protection",
        "protection_fall" => "feather_falling",
        "protection_explosions" => "blast_protection",
        "protection_projectile" => "projectile_protection",
        "oxygen" => "respiration",
        "water_worker" => "aqua_affinity",
        "arrow_damage" => "power",
        "arrow_knockback" => "punch",
        "arrow_fire" => "flame",
        "arrow_infinite" => "infinity",
        "luck" => "luck_of_the_sea",
        "honeybottle" => "honey_bottle",
        "bonemeal" => "bone_meal",
        b => {
            if let Some(rest) = b.strip_prefix("wood_") {
                return format!("wooden_{rest}{suffix}");
            }
            if let Some(rest) = b.strip_prefix("gold_").filter(|r| matches!(*r, "sword" | "pickaxe" | "axe" | "shovel" | "hoe" | "helmet" | "chestplate" | "leggings" | "boots")) {
                return format!("golden_{rest}{suffix}");
            }
            return format!("{b}{suffix}");
        }
    };
    format!("{base}{suffix}")
}

fn opt_int(v: &Yaml, key: &str) -> Option<i32> {
    v.get(key).and_then(|x| x.as_i64()).map(|n| n as i32)
}

fn num(v: &Yaml, key: &str) -> f64 {
    v.get(key).and_then(|x| x.as_f64().or_else(|| x.as_i64().map(|i| i as f64))).unwrap_or(0.0)
}

fn strs(v: Option<&Yaml>) -> Vec<String> {
    match v {
        Some(Yaml::Sequence(s)) => s.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
        Some(Yaml::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn parse_job(key: &str, v: &Yaml) -> Option<Job> {
    let s = |k: &str, d: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned).unwrap_or_else(|| d.to_owned());
    let name = s("fullname", key);
    let mut actions: HashMap<Action, HashMap<String, Info>> = HashMap::new();
    for (action, section) in ACTIONS {
        let Some(m) = v.get(section).and_then(|x| x.as_mapping()) else { continue };
        let mut table = HashMap::new();
        for (k, i) in m {
            let k = match k {
                Yaml::String(s) => s.clone(),
                Yaml::Number(n) => n.to_string(),
                _ => continue,
            };
            let key = if action == Action::CustomKill { k.to_ascii_lowercase() } else { modern(&k) };
            table.insert(key, Info { income: num(i, "income"), points: num(i, "points"), exp: num(i, "experience"), from: opt_int(i, "from-level").unwrap_or(0), until: opt_int(i, "until-level").unwrap_or(0), soft_income: opt_int(i, "softIncomeLimit"), soft_exp: opt_int(i, "softExpLimit"), soft_points: opt_int(i, "softPointsLimit") });
        }
        actions.insert(action, table);
    }
    let mut commands = Vec::new();
    if let Some(m) = v.get("commands").and_then(|x| x.as_mapping()) {
        for (_, c) in m {
            commands.push(LevelCommand { commands: strs(c.get("command")), from: opt_int(c, "levelFrom").unwrap_or(0), until: opt_int(c, "levelUntil").unwrap_or(0) });
        }
    }
    let mut permissions = Vec::new();
    if let Some(m) = v.get("permissions").and_then(|x| x.as_mapping()) {
        for (_, p) in m {
            if let Some(node) = p.get("permission").and_then(|x| x.as_str()) {
                permissions.push((node.to_owned(), p.get("value").and_then(|x| x.as_bool()).unwrap_or(true), opt_int(p, "levelRequirement").unwrap_or(0)));
            }
        }
    }
    let eqs = |k: &str, d: &str| eq::parse(&s(k, d));
    Some(Job {
        key: key.to_owned(),
        display: s("displayName", &name),
        short: s("shortname", &name.chars().take(1).collect::<String>()),
        description: s("description", ""),
        full_description: strs(v.get("FullDescription")),
        color: s("ChatColour", "WHITE"),
        max_level: opt_int(v, "max-level").unwrap_or(0),
        vip_max_level: opt_int(v, "vip-max-level").unwrap_or(0),
        slots: opt_int(v, "slots").unwrap_or(0),
        rejoin_cooldown: v.get("rejoinCooldown").and_then(|x| x.as_i64()).unwrap_or(0),
        max_exp: eqs("leveling-progression-equation", "10*(joblevel)+(joblevel*joblevel*4)"),
        income_eq: eqs("income-progression-equation", "baseincome"),
        points_eq: eqs("points-progression-equation", "basepoints"),
        exp_eq: eqs("experience-progression-equation", "baseexperience"),
        gui_item: v.get("Gui").and_then(|g| g.get("ItemStack").or_else(|| g.get("Item")).or_else(|| g.get("Id"))).and_then(|x| x.as_str()).unwrap_or("book").to_owned(),
        actions,
        commands,
        max_level_commands: strs(v.get("commands-on-max-level")),
        world_blacklist: strs(v.get("world-blacklist")),
        world_whitelist: v.get("reverse-world-blacklist-functionality").and_then(|x| x.as_bool()).unwrap_or(false),
        ignore_max: v.get("ignore-jobs-max").and_then(|x| x.as_bool()).unwrap_or(false),
        cmd_on_join: strs(v.get("cmd-on-join")),
        cmd_on_leave: strs(v.get("cmd-on-leave")),
        permissions,
        soft_income: opt_int(v, "softIncomeLimit"),
        soft_exp: opt_int(v, "softExpLimit"),
        soft_points: opt_int(v, "softPointsLimit"),
        is_none: key.eq_ignore_ascii_case("none"),
        name,
    })
}

fn load_jobs(folder: &Path) -> (Vec<Job>, Option<Job>) {
    let mut docs: Vec<Yaml> = Vec::new();
    if folder.as_os_str().is_empty() {
        for (_, t) in DEFAULT_JOBS {
            docs.push(horizon_kit::yaml::parse(t).unwrap_or(Yaml::Null));
        }
    } else {
        let old = folder.join("jobConfig.yml");
        if old.exists() {
            let doc: Yaml = std::fs::read_to_string(&old).ok().and_then(|t| horizon_kit::yaml::parse(&t).ok()).unwrap_or(Yaml::Null);
            if let Some(j) = doc.get("Jobs") {
                docs.push(j.clone());
            }
        } else {
            let dir = folder.join("jobs");
            if !dir.exists() {
                let _ = std::fs::create_dir_all(&dir);
                for (n, t) in DEFAULT_JOBS {
                    let _ = std::fs::write(dir.join(n), t);
                }
            }
            let mut files: Vec<_> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "yml")).collect();
            files.sort();
            for f in files {
                if f.file_name().is_some_and(|n| n.to_string_lossy().starts_with('_')) {
                    continue;
                }
                if let Some(doc) = std::fs::read_to_string(&f).ok().and_then(|t| horizon_kit::yaml::parse(&t).ok()) {
                    docs.push(doc);
                }
            }
        }
    }
    let mut jobs = Vec::new();
    let mut none = None;
    for doc in docs {
        let Some(m) = doc.as_mapping() else { continue };
        for (k, v) in m {
            let Some(k) = k.as_str() else { continue };
            if k.starts_with('_') || k.eq_ignore_ascii_case("exampleJob") {
                continue;
            }
            if let Some(j) = parse_job(k, v) {
                if j.is_none {
                    none = Some(j);
                } else {
                    jobs.push(j);
                }
            }
        }
    }
    (jobs, none)
}

fn flatten(v: &Yaml, prefix: &str, out: &mut HashMap<String, String>, lists: &mut HashMap<String, Vec<String>>) {
    match v {
        Yaml::Mapping(m) => {
            for (k, v) in m {
                let Some(k) = k.as_str() else { continue };
                let p = if prefix.is_empty() { k.to_owned() } else { format!("{prefix}.{k}") };
                flatten(v, &p, out, lists);
            }
        }
        Yaml::String(s) => {
            out.insert(prefix.to_owned(), s.clone());
        }
        Yaml::Sequence(items) => {
            let l: Vec<String> = items.iter().filter_map(|i| i.as_str().map(str::to_owned)).collect();
            out.insert(prefix.to_owned(), l.join("\n"));
            lists.insert(prefix.to_owned(), l);
        }
        Yaml::Number(n) => {
            out.insert(prefix.to_owned(), n.to_string());
        }
        Yaml::Bool(b) => {
            out.insert(prefix.to_owned(), b.to_string());
        }
        _ => {}
    }
}

fn hm(s: &str) -> u32 {
    let mut parts = s.split(':').map(|p| p.trim().parse::<u32>().unwrap_or(0));
    let h = parts.next().unwrap_or(0);
    let m = parts.next().unwrap_or(0);
    let sec = parts.next().unwrap_or(0);
    h * 3600 + m * 60 + sec
}

fn build(folder: &Path) -> Settings {
    let c = file(folder, "generalConfig.yml", GENERAL);
    let lang_name = c.str("locale-language", "en");
    let en: Yaml = horizon_kit::yaml::parse(MESSAGES_EN).unwrap_or(Yaml::Null);
    let mut lang = HashMap::new();
    let mut lang_lists = HashMap::new();
    flatten(&en, "", &mut lang, &mut lang_lists);
    if !folder.as_os_str().is_empty() {
        let dir = folder.join("locale");
        let _ = std::fs::create_dir_all(&dir);
        let en_path = dir.join("messages_en.yml");
        if !en_path.exists() {
            let _ = std::fs::write(&en_path, MESSAGES_EN);
        }
        let path = dir.join(format!("messages_{lang_name}.yml"));
        if let Some(doc) = std::fs::read_to_string(&path).ok().and_then(|t| horizon_kit::yaml::parse(&t).ok()) {
            flatten(&doc, "", &mut lang, &mut lang_lists);
        }
    }
    let r = file(folder, "restrictedBlocks.yml", RESTRICTED);
    let mut block_timers = HashMap::new();
    if let Some(Yaml::Mapping(m)) = r.at("blocksTimer") {
        for (k, v) in m {
            if let (Some(k), Some(t)) = (k.as_str(), v.as_i64()) {
                block_timers.insert(modern(k), t);
            }
        }
    }
    let shop_cfg = file(folder, "shopItems.yml", SHOP);
    let mut shop = Vec::new();
    for key in shop_cfg.keys("Items") {
        let p = format!("Items.{key}");
        let icon_stack = shop_cfg.str(&format!("{p}.Icon.ItemStack"), "");
        let (icon, icon_name, lore) = if icon_stack.is_empty() {
            (shop_cfg.str(&format!("{p}.Icon.Id"), "stone"), shop_cfg.str(&format!("{p}.Icon.Name"), &key), shop_cfg.list(&format!("{p}.Icon.Lore")))
        } else {
            let mut parts = icon_stack.split(';');
            let id = parts.next().unwrap_or("stone").to_owned();
            let name = parts.next().unwrap_or(&key).replace('_', " ");
            let lore = parts.next().map(|l| l.trim().replace('_', " ").split("\\n").map(str::to_owned).collect()).unwrap_or_default();
            (id, name, lore)
        };
        shop.push(ShopItem {
            points: shop_cfg.num(&format!("{p}.pointPrice"), 0.0),
            icon: modern(&icon),
            icon_name,
            lore,
            perms: shop_cfg.list(&format!("{p}.RequiredPermission")),
            job_levels: shop_cfg.list(&format!("{p}.RequiredJobLevels")).iter().filter_map(|e| e.rsplit_once('-').and_then(|(j, l)| Some((j.to_owned(), l.parse().ok()?)))).collect(),
            total_levels: shop_cfg.int(&format!("{p}.RequiredTotalLevels"), 0) as i32,
            commands: shop_cfg.list(&format!("{p}.PerformCommands")),
            give: shop_cfg.list(&format!("{p}.GiveItems")),
            hide_without_perm: shop_cfg.bool(&format!("{p}.Icon.HideWithoutPermission"), false),
            key,
        });
    }
    let sch = file(folder, "schedule.yml", SCHEDULE);
    let mut schedule = Vec::new();
    for key in sch.keys("Boost") {
        let p = format!("Boost.{key}");
        schedule.push(Schedule {
            enabled: sch.bool(&format!("{p}.Enabled"), false),
            from: hm(&sch.str(&format!("{p}.From"), "00:00:00")),
            until: hm(&sch.str(&format!("{p}.Until"), "00:00:00")),
            money: sch.num(&format!("{p}.Money"), 1.0),
            exp: sch.num(&format!("{p}.Exp"), 1.0),
            points: sch.num(&format!("{p}.Points"), 1.0),
            days: sch.list(&format!("{p}.Days")),
            jobs: sch.list(&format!("{p}.Jobs")),
            start_msg: if sch.bool(&format!("{p}.BroadcastOnStart"), true) { sch.list(&format!("{p}.MessageOnStart")) } else { Vec::new() },
            stop_msg: if sch.bool(&format!("{p}.BroadcastOnStop"), true) { sch.list(&format!("{p}.MessageOnStop")) } else { Vec::new() },
            name: key,
        });
    }
    let (jobs, none) = load_jobs(folder);
    let mut uses = HashMap::new();
    for (a, _) in ACTIONS {
        let any = jobs.iter().chain(none.iter()).any(|j| j.actions.get(&a).is_some_and(|t| !t.is_empty()));
        uses.insert(a, any);
    }
    let limit = |kind: &str| Limit {
        enabled: c.bool(&format!("Economy.Limit.{kind}.Use"), false),
        equation: eq::parse(&c.str(&format!("Economy.Limit.{kind}.{}", if kind == "Money" { "MoneyLimit" } else { "Limit" }), "500")),
        seconds: c.int(&format!("Economy.Limit.{kind}.TimeLimit"), 3600),
        stop_with: ["Money", "Exp", "Point"].iter().filter(|o| c.bool(&format!("Economy.Limit.{kind}.StopWith{o}"), false)).map(|o| o.chars().next().unwrap_or('M')).collect(),
    };
    let sound = |p: &str| c.bool(&format!("{p}.use"), true).then(|| (c.num(&format!("{p}.volume"), 1.0) as f32, c.num(&format!("{p}.pitch"), 3.0) as f32));
    Settings {
        save_minutes: c.int("save-period", 10).max(1) as u64,
        save_on_quit: c.bool("save-on-disconnect", false),
        max_jobs: c.int("max-jobs", 3) as i32,
        hide_without_perm: c.bool("hide-jobs-without-permission", false),
        shop_enabled: c.bool("jobs-shop-enabled", true),
        pay_near_spawner: c.bool("enable-pay-near-spawner", true),
        pay_creative: c.bool("enable-pay-creative", true),
        add_xp_player: c.bool("add-xp-player", false),
        batch_secs: c.int("economy-batch-delay", 5).max(1) as u64,
        min_money: c.bool("Economy.MinimumOveralPayment.use", true).then(|| c.num("Economy.MinimumOveralPayment.limit", 0.1)),
        min_points: c.bool("Economy.MinimumOveralPoints.use", true).then(|| c.num("Economy.MinimumOveralPoints.limit", 0.1)),
        min_exp: c.bool("Economy.MinimumOveralExp.use", true).then(|| c.num("Economy.MinimumOveralExp.limit", 0.1)),
        negative_boost: c.bool("Economy.ApplyToNegativeIncome", false),
        limits: [limit("Money"), limit("Exp"), limit("Point")],
        break_timer: if c.bool("ExploitProtections.General.PlaceAndBreak.GlobalBlockTimer.Break.Use", true) { c.int("ExploitProtections.General.PlaceAndBreak.GlobalBlockTimer.Break.Timer", 3) } else { 0 },
        ignore_ore_generators: c.bool("ExploitProtections.General.PlaceAndBreak.IgnoreOreGenerators", true),
        silk_protection: c.bool("ExploitProtections.General.PlaceAndBreak.SilkTouchProtection", false),
        block_timers,
        level_loss: c.num("old-job.level-loss-percentage", 30.0),
        fix_at_max: c.bool("old-job.fix-at-max-level", true),
        action_bars: c.bool("ActionBars.Enabled", true),
        bossbar: c.bool("BossBar.Enabled", true),
        bossbar_timer: c.int("BossBar.Timer", 5).max(1) as u64,
        default_actionbar: c.str("ActionBars.Messages.DefaultState", "Rapid"),
        default_bossbar: c.str("BossBar.Messages.DefaultState", "Batched"),
        default_chat: c.str("ChatText.Messages.DefaultState", "Off"),
        sound_levelup: sound("Sounds.LevelUp"),
        sound_title: sound("Sounds.TitleChange"),
        broadcast_levelup: c.bool("broadcast.on-level-up.use", false),
        broadcast_levels: c.list("broadcast.on-level-up.levels").iter().filter_map(|l| l.parse().ok()).collect(),
        top_amount: c.int("Commands.PageRow.JobsTop.AmountToShow", 15).max(1) as usize,
        hidden_top: c.list("Commands.PageRow.JobsTop.HiddenPlayers"),
        decimals: [c.int("Optimizations.DecimalPlaces.Money", 2) as usize, c.int("Optimizations.DecimalPlaces.Exp", 2) as usize, c.int("Optimizations.DecimalPlaces.Points", 2) as usize],
        disabled_worlds: if c.bool("Optimizations.DisabledWorlds.Use", false) { c.list("Optimizations.DisabledWorlds.List") } else { Vec::new() },
        disabled_whitelist: c.bool("Optimizations.DisabledWorlds.UseAsWhiteList", false),
        gui_on_browse: c.bool("JobsGUI.OpenOnBrowse", true),
        chat_browse: c.bool("JobsGUI.ShowChatBrowse", true),
        gui_rows: c.int("JobsGUI.RowCount", 0),
        disable_payment_max: c.bool("disable-payment-if-max-level-reached", false),
        tameables_payout: c.bool("ExploitProtections.General.TameablesPayout", true),
        monster_damage: c.bool("ExploitProtections.General.MonsterDamage.Use", false).then(|| c.num("ExploitProtections.General.MonsterDamage.Percentage", 60.0)),
        explore_flying: c.bool("enable-pay-for-exploring-when-flying", true),
        jobs,
        none,
        shop,
        schedule,
        uses,
        lang,
        lang_lists,
    }
}

impl Settings {
    pub fn job(&self, name: &str) -> Option<&Job> {
        self.jobs.iter().find(|j| j.name.eq_ignore_ascii_case(name) || j.key.eq_ignore_ascii_case(name) || j.short.eq_ignore_ascii_case(name))
    }
    pub fn job_index(&self, name: &str) -> Option<usize> {
        self.jobs.iter().position(|j| j.name.eq_ignore_ascii_case(name) || j.key.eq_ignore_ascii_case(name) || j.short.eq_ignore_ascii_case(name))
    }
    pub fn world_enabled(&self, world: &str) -> bool {
        if self.disabled_worlds.is_empty() {
            return true;
        }
        let w = world.trim_start_matches("minecraft:");
        let listed = self.disabled_worlds.iter().any(|d| d.eq_ignore_ascii_case(world) || d.eq_ignore_ascii_case(w));
        if self.disabled_whitelist { listed } else { !listed }
    }
    pub fn uses(&self, a: Action) -> bool {
        self.uses.get(&a).copied().unwrap_or(false)
    }
    /// A message with `%name%` / `[name]` filled, coloured.
    pub fn msg(&self, key: &str, pairs: &[(&str, &str)]) -> String {
        // Keys Jobs took from CMILib's own language file.
        let fallback = match key {
            "general.error.permission" => "&cYou don't have permission to do this!",
            "general.error.ingame" => "&cYou can use this command only in game!",
            "command.toggle.output.Off" => "&7[type]&a: &cOff",
            "command.reload.output" => "&eJobs configuration and jobs reloaded.",
            _ => key,
        };
        let raw = self.lang.get(key).cloned().unwrap_or_else(|| fallback.to_owned());
        let mut out = raw;
        for (k, v) in pairs {
            out = out.replace(&format!("%{k}%"), v).replace(&format!("[{k}]"), v);
        }
        horizon_kit::text::colorize(&out)
    }
    pub fn money(&self, v: f64) -> String {
        format!("{:.*}", self.decimals[0], v)
    }
    pub fn exp_fmt(&self, v: f64) -> String {
        format!("{:.*}", self.decimals[1], v)
    }
    pub fn points_fmt(&self, v: f64) -> String {
        format!("{:.*}", self.decimals[2], v)
    }
}

pub fn chat_color(name: &str) -> &'static str {
    match name.to_ascii_uppercase().as_str() {
        "BLACK" => "§0",
        "DARK_BLUE" => "§1",
        "DARK_GREEN" => "§2",
        "DARK_AQUA" => "§3",
        "DARK_RED" => "§4",
        "DARK_PURPLE" => "§5",
        "GOLD" => "§6",
        "GRAY" => "§7",
        "DARK_GRAY" => "§8",
        "BLUE" => "§9",
        "GREEN" => "§a",
        "AQUA" => "§b",
        "RED" => "§c",
        "LIGHT_PURPLE" => "§d",
        "YELLOW" => "§e",
        _ => "§f",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn defaults_load() {
        let s = super::get();
        assert!(s.jobs.len() >= 10);
        let miner = s.job("miner").unwrap();
        assert!(miner.actions[&super::Action::Break].contains_key("coal_ore"));
        assert_eq!(super::modern("wood_sword"), "wooden_sword");
        assert_eq!(super::modern("wheat-7"), "wheat-7");
        assert_eq!(super::modern("DIG_SPEED-1"), "efficiency-1");
        assert!(s.lang.contains_key("command.join.success"));
    }
}
