//! BetterRTP's files read into one `Settings`: config.yml (defaults, custom worlds, overrides, world types,
//! disabled worlds, blacklisted blocks, permission groups), economy.yml, effects.yml, locations.yml, signs.yml,
//! placeholders.yml and the language file.

use horizon_kit::Config;
use serde_yaml::{Mapping, Value as Y};
use std::collections::HashMap;
use std::path::Path;

pub const CONFIG: &str = include_str!("../data/config.yml");
pub const ECONOMY: &str = include_str!("../data/economy.yml");
pub const EFFECTS: &str = include_str!("../data/effects.yml");
pub const LOCATIONS: &str = include_str!("../data/locations.yml");
pub const SIGNS: &str = include_str!("../data/signs.yml");
pub const PLACEHOLDERS: &str = include_str!("../data/placeholders.yml");
pub const PERMISSIONS: &str = include_str!("../data/permissions.yml");

pub const LANGS: &[(&str, &str)] = &[
    ("en.yml", include_str!("../data/lang/en.yml")),
    ("br.yml", include_str!("../data/lang/br.yml")),
    ("chs.yml", include_str!("../data/lang/chs.yml")),
    ("cht.yml", include_str!("../data/lang/cht.yml")),
    ("cs.yml", include_str!("../data/lang/cs.yml")),
    ("da.yml", include_str!("../data/lang/da.yml")),
    ("de.yml", include_str!("../data/lang/de.yml")),
    ("es.yml", include_str!("../data/lang/es.yml")),
    ("fr.yml", include_str!("../data/lang/fr.yml")),
    ("he.yml", include_str!("../data/lang/he.yml")),
    ("hu.yml", include_str!("../data/lang/hu.yml")),
    ("it.yml", include_str!("../data/lang/it.yml")),
    ("ja.yml", include_str!("../data/lang/ja.yml")),
    ("nl.yml", include_str!("../data/lang/nl.yml")),
    ("no.yml", include_str!("../data/lang/no.yml")),
    ("pl.yml", include_str!("../data/lang/pl.yml")),
    ("ro.yml", include_str!("../data/lang/ro.yml")),
    ("ru.yml", include_str!("../data/lang/ru.yml")),
    ("tr.yml", include_str!("../data/lang/tr.yml")),
    ("uk.yml", include_str!("../data/lang/uk.yml")),
    ("vi.yml", include_str!("../data/lang/vi.yml")),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    Square,
    Circle,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum WorldType {
    Normal,
    Nether,
}

/// One place's rtp settings (RTPWorld).
#[derive(Clone, Debug)]
pub struct RtpWorld {
    pub use_world_border: bool,
    pub max_radius: i32,
    pub min_radius: i32,
    pub center_x: i32,
    pub center_z: i32,
    pub price: i32,
    pub biomes: Vec<String>,
    pub shape: Shape,
    pub min_y: i32,
    pub max_y: i32,
    pub rtp_on_death: bool,
    /// Seconds.
    pub cooldown: i64,
    /// The world (locations and permission-group entries name theirs).
    pub world: Option<String>,
    pub priority: i32,
}

#[derive(Clone, Debug, Default)]
pub struct Effects {
    pub sounds: bool,
    pub sound_delay: String,
    pub sound_success: String,
    pub titles: bool,
    pub particles: bool,
    pub particle_types: Vec<String>,
    pub particle_shape: String,
    pub invincible: Option<u64>,
    pub potions: Vec<(String, i32, i32)>,
}

pub struct Settings {
    pub config: Config,
    pub lang: Config,
    pub effects_file: Config,
    pub placeholders: Config,
    pub default: RtpWorld,
    pub custom: HashMap<String, RtpWorld>,
    pub overrides: HashMap<String, String>,
    pub world_types: HashMap<String, WorldType>,
    pub disabled: Vec<String>,
    pub blacklist: Vec<String>,
    /// group -> worlds
    pub groups: Vec<(String, Vec<RtpWorld>)>,
    pub groups_enabled: bool,
    pub locations: Vec<(String, RtpWorld)>,
    pub locations_enabled: bool,
    pub locations_require_permission: bool,
    pub use_location_if_available: bool,
    pub locations_same_world: bool,
    pub max_attempts: u32,
    pub delay: u32,
    pub delay_enabled: bool,
    pub cancel_on_move: bool,
    pub cancel_on_damage: bool,
    pub cooldown_enabled: bool,
    pub cooldown_time: i64,
    pub lock_after: i64,
    pub per_world: bool,
    pub economy_enabled: bool,
    pub economy_price: i32,
    pub custom_prices: HashMap<String, i32>,
    pub hunger: i32,
    pub effects: Effects,
    pub signs_enabled: bool,
    pub sign_title: String,
    pub first_join: Option<(String, bool)>,
    pub debug: bool,
    pub status_messages: bool,
}

/// `- name: {..}` lists (Java's `getMapList`) as (name, section) pairs.
pub fn map_list(v: Option<&Y>) -> Vec<(String, Y)> {
    let mut out = Vec::new();
    match v {
        Some(Y::Sequence(items)) => {
            for item in items {
                if let Y::Mapping(m) = item {
                    for (k, v) in m {
                        if let Some(k) = crate::settings::key_str(k) {
                            out.push((k, v.clone()));
                        }
                    }
                }
            }
        }
        Some(Y::Mapping(m)) => {
            for (k, v) in m {
                if let Some(k) = key_str(k) {
                    out.push((k, v.clone()));
                }
            }
        }
        _ => {}
    }
    out
}

pub fn key_str(k: &Y) -> Option<String> {
    match k {
        Y::String(s) => Some(s.clone()),
        Y::Number(n) => Some(n.to_string()),
        Y::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn int(m: &Y, k: &str) -> Option<i32> {
    m.get(k).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).map(|v| v as i32)
}

fn shape_of(s: &str) -> Shape {
    if s.eq_ignore_ascii_case("circle") { Shape::Circle } else { Shape::Square }
}

impl RtpWorld {
    /// The fields a section sets, over `base`.
    fn over(base: &RtpWorld, m: &Y, take_price: bool) -> RtpWorld {
        let mut w = base.clone();
        if let Some(b) = m.get("UseWorldBorder").and_then(Y::as_bool) {
            w.use_world_border = b;
        }
        if let Some(v) = int(m, "CenterX") {
            w.center_x = v;
        }
        if let Some(v) = int(m, "CenterZ") {
            w.center_z = v;
        }
        if let Some(v) = int(m, "MaxRadius") {
            w.max_radius = if v <= 0 { base.max_radius } else { v };
        }
        if let Some(v) = int(m, "MinRadius") {
            w.min_radius = v;
            if w.min_radius < 0 || w.min_radius >= w.max_radius {
                w.min_radius = base.min_radius;
                if w.min_radius >= w.max_radius {
                    w.max_radius = base.max_radius;
                }
            }
        }
        if let Some(Y::Sequence(b)) = m.get("Biomes") {
            w.biomes = b.iter().filter_map(key_str).collect();
        }
        if take_price && let Some(v) = int(m, "Price") {
            w.price = v;
        }
        if let Some(s) = m.get("Shape").and_then(Y::as_str) {
            w.shape = shape_of(s);
        }
        if let Some(v) = int(m, "MinY") {
            w.min_y = v;
        }
        if let Some(v) = int(m, "MaxY") {
            w.max_y = v;
        }
        if let Some(b) = m.get("RTPOnDeath").and_then(Y::as_bool) {
            w.rtp_on_death = b;
        }
        if let Some(v) = int(m, "Cooldown") {
            w.cooldown = v as i64;
        }
        if let Some(s) = m.get("World").and_then(Y::as_str) {
            w.world = Some(s.to_owned());
        }
        if let Some(v) = int(m, "Priority") {
            w.priority = v;
        }
        w
    }
}

fn load_or_write(folder: &Path, name: &str, default: &str) -> Config {
    Config::load(folder, name, default)
}

impl Settings {
    pub fn load(folder: &Path) -> Settings {
        let config = load_or_write(folder, "config.yml", CONFIG);
        let economy = load_or_write(folder, "economy.yml", ECONOMY);
        let effects_file = load_or_write(folder, "effects.yml", EFFECTS);
        let locations = load_or_write(folder, "locations.yml", LOCATIONS);
        let signs = load_or_write(folder, "signs.yml", SIGNS);
        let placeholders = load_or_write(folder, "placeholders.yml", PLACEHOLDERS);
        let _ = load_or_write(folder, "permissions.yml", PERMISSIONS);
        let lang_dir = folder.join("lang");
        for (name, text) in LANGS {
            if !lang_dir.join(name).exists() {
                let _ = std::fs::create_dir_all(&lang_dir);
                let _ = std::fs::write(lang_dir.join(name), text);
            }
        }
        let lang_name = config.str("Language-File", "en.yml");
        let lang_default = LANGS.iter().find(|(n, _)| *n == lang_name).map_or(LANGS[0].1, |(_, t)| t);
        let mut lang = Config::load(&lang_dir, &lang_name, lang_default);
        // Keys a translation lacks come from English.
        let en = Config::parse(LANGS[0].1);
        merge(&mut lang.doc, &en.doc);

        let economy_enabled = economy.bool("Economy.Enabled", false);
        let economy_price = if economy_enabled { economy.int("Economy.Price", 0) as i32 } else { 0 };
        let mut custom_prices = HashMap::new();
        if economy_enabled && economy.bool("CustomWorlds.Enabled", false) {
            for (k, v) in map_list(economy.at("CustomWorlds.Prices")) {
                if let Some(p) = v.as_i64() {
                    custom_prices.insert(k, p as i32);
                }
            }
        }
        let cooldown_time = config.int("Settings.Cooldown.Time", 600);
        let base = RtpWorld {
            use_world_border: false,
            max_radius: 1000,
            min_radius: 10,
            center_x: 0,
            center_z: 0,
            price: economy_price,
            biomes: Vec::new(),
            shape: Shape::Square,
            min_y: 0,
            max_y: 320,
            rtp_on_death: false,
            cooldown: cooldown_time,
            world: None,
            priority: 0,
        };
        let default = config.at("Default").map_or(base.clone(), |d| RtpWorld::over(&base, d, false));
        let mut custom = HashMap::new();
        for (name, section) in map_list(config.at("CustomWorlds")) {
            custom.insert(name, RtpWorld::over(&default, &section, economy_enabled));
        }
        let mut overrides = HashMap::new();
        for (from, to) in map_list(config.at("Overrides")) {
            if let Some(to) = key_str(&to) {
                overrides.insert(from, to);
            }
        }
        let mut world_types = HashMap::new();
        for (w, t) in map_list(config.at("WorldType")) {
            let t = key_str(&t).unwrap_or_default();
            world_types.insert(w, if t.eq_ignore_ascii_case("NETHER") { WorldType::Nether } else { WorldType::Normal });
        }
        let mut groups = Vec::new();
        for (group, worlds) in map_list(config.at("PermissionGroup.Groups")) {
            let list = map_list(Some(&worlds)).into_iter().map(|(world, s)| {
                let mut w = RtpWorld::over(&default, &s, true);
                w.world = Some(world);
                w
            });
            groups.push((group, list.collect()));
        }
        let mut locs = Vec::new();
        for (name, s) in map_list(locations.at("Locations")) {
            locs.push((name, RtpWorld::over(&default, &s, true)));
        }
        let e = &effects_file;
        let potions = e
            .list("Potions.Types")
            .iter()
            .map(|p| {
                let mut parts = p.split(':');
                let name = parts.next().unwrap_or("").trim().to_ascii_lowercase();
                let dur = parts.next().and_then(|d| d.trim().parse().ok()).unwrap_or(60);
                let amp = parts.next().and_then(|d| d.trim().parse().ok()).unwrap_or(1);
                (name, dur, amp)
            })
            .collect();
        let effects = Effects {
            sounds: e.bool("Sounds.Enabled", true),
            sound_delay: e.str("Sounds.Delay", "entity_tnt_primed"),
            sound_success: e.str("Sounds.Success", "entity_generic_explode"),
            titles: e.bool("Titles.Enabled", true),
            particles: e.bool("Particles.Enabled", true),
            particle_types: { let l = e.list("Particles.Type"); if l.is_empty() { vec![e.str("Particles.Type", "EXPLOSION_NORMAL")] } else { l } },
            particle_shape: e.str("Particles.Shape", "EXPLODE"),
            invincible: e.bool("Invincible.Enabled", true).then(|| e.int("Invincible.Seconds", 5).max(0) as u64),
            potions: if e.bool("Potions.Enabled", true) { potions } else { Vec::new() },
        };
        Settings {
            default,
            custom,
            overrides,
            world_types,
            disabled: config.list("DisabledWorlds"),
            blacklist: config.list("BlacklistedBlocks").into_iter().map(|b| b.to_ascii_uppercase()).collect(),
            groups,
            groups_enabled: config.bool("PermissionGroup.Enabled", false),
            locations: locs,
            locations_enabled: locations.bool("Enabled", false),
            locations_require_permission: locations.bool("RequirePermission", false),
            use_location_if_available: locations.bool("UseLocationIfAvailable", true),
            locations_same_world: locations.bool("UseLocationsInSameWorld", true),
            max_attempts: config.int("Settings.MaxAttempts", 32).max(1) as u32,
            delay: config.int("Settings.Delay.Time", 5).max(0) as u32,
            delay_enabled: config.bool("Settings.Delay.Enabled", true),
            cancel_on_move: config.bool("Settings.Delay.CancelOnMove", true),
            cancel_on_damage: config.bool("Settings.Delay.CancelOnDamage", false),
            cooldown_enabled: config.bool("Settings.Cooldown.Enabled", true),
            cooldown_time,
            lock_after: config.int("Settings.Cooldown.LockAfter", 0),
            per_world: config.bool("Settings.Cooldown.PerWorld", false),
            economy_enabled,
            economy_price,
            custom_prices,
            hunger: if economy.bool("Hunger.Enabled", false) { economy.int("Hunger.Honches", 4) as i32 } else { 0 },
            effects,
            signs_enabled: signs.bool("Settings.Enabled", false),
            sign_title: signs.str("Settings.Title", "[RTP]"),
            first_join: config.bool("Settings.RtpOnFirstJoin.Enabled", false).then(|| (config.str("Settings.RtpOnFirstJoin.World", "world"), config.bool("Settings.RtpOnFirstJoin.SetAsRespawn", false))),
            debug: config.bool("Settings.Debugger", false),
            status_messages: config.bool("Settings.StatusMessages", true),
            config,
            lang,
            effects_file,
            placeholders,
        }
    }

    /// The price in a world with the default settings (economy.yml's custom world prices first).
    pub fn default_price(&self, world: &str) -> i32 {
        self.custom_prices.get(world).copied().unwrap_or(self.economy_price)
    }

    pub fn world_type(&self, world: &str, environment: &str) -> WorldType {
        match self.world_types.get(world) {
            Some(t) => *t,
            None if environment == "NETHER" => WorldType::Nether,
            None => WorldType::Normal,
        }
    }
}

fn merge(doc: &mut Y, defaults: &Y) {
    match (doc, defaults) {
        (Y::Mapping(mine), Y::Mapping(theirs)) => {
            for (k, v) in theirs {
                match mine.get_mut(k) {
                    Some(existing) => merge(existing, v),
                    None => {
                        mine.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (doc @ Y::Null, d) => *doc = d.clone(),
        _ => {}
    }
}

/// Sets a value inside a YAML tree by dotted path (sections made as needed).
pub fn set_path(doc: &mut Y, path: &str, value: Y) {
    let mut v = doc;
    for key in path.split('.') {
        if !v.is_mapping() {
            *v = Y::Mapping(Mapping::new());
        }
        let map = v.as_mapping_mut().expect("mapping");
        v = map.entry(Y::String(key.to_owned())).or_insert(Y::Null);
    }
    *v = value;
}
