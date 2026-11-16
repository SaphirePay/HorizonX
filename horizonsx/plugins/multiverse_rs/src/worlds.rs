//! worlds.yml: every world Multiverse knows, loaded or not, with its properties (Multiverse 5's format, version 1.3;
//! Multiverse 4's `worlds:` file is moved over on first start, as the Java plugin does). Each world keeps its whole
//! YAML section, so keys this port doesn't use survive a save.

use crate::util::{log, write_file};
use horizon_bukkit::{Location, World};
use serde_yaml::{Mapping, Value as Y};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone)]
pub struct MvWorld {
    /// The key in worlds.yml (dots decoded): a name, or `namespace:key`.
    pub key: String,
    /// Bukkit's name of the world.
    pub name: String,
    pub data: Mapping,
}

pub struct Store {
    pub worlds: Vec<MvWorld>,
    pub file: PathBuf,
}

pub static STORE: Mutex<Store> = Mutex::new(Store { worlds: Vec::new(), file: PathBuf::new() });

fn store() -> std::sync::MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The property names `/mv modify` takes (the node's name, else its path), in worlds.yml order.
pub const PROPERTIES: &[(&str, &str, Kind)] = &[
    ("adjust-spawn", "adjust-spawn", Kind::Bool),
    ("alias", "alias", Kind::Str),
    ("allow-advancement-grant", "allow-advancement-grant", Kind::Bool),
    ("allow-flight", "allow-flight", Kind::Bool),
    ("allow-weather", "allow-weather", Kind::Bool),
    ("anchor-respawn", "anchor-respawn", Kind::Bool),
    ("auto-heal", "auto-heal", Kind::Bool),
    ("auto-load", "auto-load", Kind::Bool),
    ("bed-respawn", "bed-respawn", Kind::Bool),
    ("difficulty", "difficulty", Kind::Enum(&["PEACEFUL", "EASY", "NORMAL", "HARD"])),
    ("entryfee-enabled", "entry-fee.enabled", Kind::Bool),
    ("entryfee-amount", "entry-fee.amount", Kind::Num),
    ("entryfee-currency", "entry-fee.currency", Kind::Str),
    ("gamemode", "gamemode", Kind::Enum(&["SURVIVAL", "CREATIVE", "ADVENTURE", "SPECTATOR"])),
    ("hidden", "hidden", Kind::Bool),
    ("hunger", "hunger", Kind::Bool),
    ("keep-spawn-in-memory", "keep-spawn-in-memory", Kind::Bool),
    ("player-limit", "player-limit", Kind::Int),
    ("portal-form", "portal-form", Kind::Enum(&["ALL", "NETHER", "END", "NONE"])),
    ("pvp", "pvp", Kind::Bool),
    ("respawn-world", "respawn-world", Kind::Str),
    ("scale", "scale", Kind::Num),
    ("world-blacklist", "world-blacklist", Kind::List),
];

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Bool,
    Str,
    Num,
    Int,
    List,
    Enum(&'static [&'static str]),
}

pub fn property(name: &str) -> Option<&'static (&'static str, &'static str, Kind)> {
    PROPERTIES.iter().find(|(n, p, _)| n.eq_ignore_ascii_case(name) || p.eq_ignore_ascii_case(name))
}

fn at<'a>(map: &'a Mapping, path: &str) -> Option<&'a Y> {
    let mut parts = path.split('.');
    let mut v = map.get(parts.next()?)?;
    for p in parts {
        v = v.get(p)?;
    }
    Some(v)
}

fn set_at(map: &mut Mapping, path: &str, value: Y) {
    let (head, rest) = path.split_once('.').map_or((path, None), |(h, r)| (h, Some(r)));
    match rest {
        None => {
            map.insert(Y::String(head.to_owned()), value);
        }
        Some(rest) => {
            let entry = map.entry(Y::String(head.to_owned())).or_insert_with(|| Y::Mapping(Mapping::new()));
            if !entry.is_mapping() {
                *entry = Y::Mapping(Mapping::new());
            }
            if let Y::Mapping(inner) = entry {
                set_at(inner, rest, value);
            }
        }
    }
}

pub fn yaml_str(v: &Y) -> Option<String> {
    match v {
        Y::String(s) => Some(s.clone()),
        Y::Number(n) => Some(n.to_string()),
        Y::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

impl MvWorld {
    pub fn get(&self, path: &str) -> Option<&Y> {
        at(&self.data, path)
    }
    pub fn b(&self, path: &str, def: bool) -> bool {
        match self.get(path) {
            Some(Y::Bool(b)) => *b,
            Some(Y::String(s)) => s.parse().unwrap_or(def),
            _ => def,
        }
    }
    pub fn s(&self, path: &str, def: &str) -> String {
        self.get(path).and_then(yaml_str).unwrap_or_else(|| def.to_owned())
    }
    pub fn f(&self, path: &str, def: f64) -> f64 {
        self.get(path).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(def)
    }
    pub fn i(&self, path: &str, def: i64) -> i64 {
        self.get(path).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(def)
    }
    pub fn list(&self, path: &str) -> Vec<String> {
        match self.get(path) {
            Some(Y::Sequence(s)) => s.iter().filter_map(yaml_str).collect(),
            _ => Vec::new(),
        }
    }
    pub fn set(&mut self, path: &str, value: Y) {
        set_at(&mut self.data, path, value);
    }
    pub fn alias(&self) -> String {
        self.s("alias", "")
    }
    /// The alias in colour, else the name.
    pub fn alias_or_name(&self) -> String {
        let a = self.alias();
        if a.is_empty() { self.name.clone() } else { horizon_kit::text::colorize(&a) }
    }
    pub fn environment(&self) -> String {
        self.get("read-only.environment").or_else(|| self.get("environment")).and_then(yaml_str).unwrap_or_else(|| "NORMAL".into()).to_ascii_uppercase()
    }
    pub fn seed(&self) -> Option<i64> {
        self.get("read-only.seed").or_else(|| self.get("seed")).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).filter(|s| *s != i64::MIN)
    }
    pub fn generator(&self) -> String {
        let g = self.s("generator", "");
        if g == "@error" || g == "null" { String::new() } else { g }
    }
    pub fn gamemode(&self) -> String {
        self.s("gamemode", "SURVIVAL").to_ascii_uppercase()
    }
    pub fn hidden(&self) -> bool {
        self.b("hidden", false)
    }
    pub fn player_limit(&self) -> i64 {
        self.i("player-limit", -1)
    }
    pub fn entry_fee(&self) -> Option<(f64, String)> {
        (self.b("entry-fee.enabled", false) && self.f("entry-fee.amount", 0.0) != 0.0).then(|| (self.f("entry-fee.amount", 0.0), self.s("entry-fee.currency", "@vault-economy")))
    }
    pub fn is_loaded(&self) -> bool {
        horizon_bukkit::get_world(&self.name).is_some()
    }
    pub fn world(&self) -> Option<World> {
        horizon_bukkit::get_world(&self.name)
    }
    /// The spawn worlds.yml keeps, else the world's own.
    pub fn spawn(&self) -> Option<Location> {
        let world = self.world()?;
        match self.get("spawn-location") {
            Some(Y::Mapping(m)) if m.contains_key("x") => {
                let n = |k: &str| m.get(k).and_then(Y::as_f64).unwrap_or(0.0);
                Some(Location::new(world, n("x"), n("y"), n("z"), n("yaw") as f32, n("pitch") as f32))
            }
            _ => {
                // A world just made has no spawn worked out yet (y 0): the safe-spot search lifts it to the ground.
                let mut at = world.get_spawn_location();
                if at.y <= f64::from(world.get_min_height()) + 1.0 || at.y == 0.0 {
                    at.y = 64.0;
                    at.x = at.x.floor() + 0.5;
                    at.z = at.z.floor() + 0.5;
                }
                Some(at)
            }
        }
    }
    pub fn set_spawn(&mut self, at: &Location) {
        let mut m = Mapping::new();
        m.insert("==".into(), "MVSpawnLocation".into());
        m.insert("x".into(), at.x.into());
        m.insert("y".into(), at.y.into());
        m.insert("z".into(), at.z.into());
        m.insert("pitch".into(), (at.pitch as f64).into());
        m.insert("yaw".into(), (at.yaw as f64).into());
        self.set("spawn-location", Y::Mapping(m));
    }
}

/// A new world's section: every property at its default, as Multiverse writes it.
pub fn default_data(env: &str, seed: Option<i64>, generator: &str) -> Mapping {
    let mut m = Mapping::new();
    let mut put = |k: &str, v: Y| set_at(&mut m, k, v);
    put("adjust-spawn", false.into());
    put("alias", "".into());
    put("allow-advancement-grant", true.into());
    put("allow-flight", false.into());
    put("allow-weather", true.into());
    put("anchor-respawn", true.into());
    put("auto-heal", true.into());
    put("auto-load", true.into());
    put("bed-respawn", true.into());
    put("biome", "".into());
    put("difficulty", "NORMAL".into());
    put("entry-fee.enabled", false.into());
    put("entry-fee.amount", 0.0.into());
    put("entry-fee.currency", "@vault-economy".into());
    put("gamemode", "SURVIVAL".into());
    put("generator", generator.into());
    put("hidden", false.into());
    put("hunger", true.into());
    put("keep-spawn-in-memory", true.into());
    put("player-limit", (-1).into());
    put("portal-form", "ALL".into());
    put("pvp", true.into());
    put("respawn-world", "".into());
    let scale = match env {
        "NETHER" => 8.0,
        "THE_END" => 16.0,
        _ => 1.0,
    };
    put("scale", scale.into());
    put("world-blacklist", Y::Sequence(Vec::new()));
    put("read-only.environment", env.into());
    put("read-only.generator-settings", "".into());
    put("read-only.seed", seed.unwrap_or(i64::MIN).into());
    put("version", 1.3.into());
    m
}

/// The Bukkit name for a worlds.yml key.
pub fn key_to_name(key: &str) -> String {
    match key {
        "minecraft:overworld" => "world".into(),
        "minecraft:the_nether" => "world_nether".into(),
        "minecraft:the_end" => "world_the_end".into(),
        k => k.split_once(':').map_or(k, |(_, p)| p).to_owned(),
    }
}

// ---- The file ----

/// worlds.yml read (moved from Multiverse 4's format first when it is that).
pub fn load_file(folder: &std::path::Path) {
    let file = folder.join("worlds.yml");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let mut doc: Y = horizon_kit::yaml::parse(&text).unwrap_or(Y::Null);
    if doc.get("worlds").is_some_and(Y::is_mapping) {
        let _ = std::fs::write(folder.join(format!("worlds.yml.old.{}", now_ms())), &text);
        doc = migrate_mv4(&doc);
        log("Migrated worlds.yml from the Multiverse 4 format.");
    }
    let mut worlds = Vec::new();
    if let Y::Mapping(top) = &doc {
        for (k, v) in top {
            let (Some(k), Y::Mapping(data)) = (k.as_str(), v) else { continue };
            let key = k.replace("[dot]", ".");
            let mut w = MvWorld { name: key_to_name(&key), key, data: data.clone() };
            migrate_section(&mut w);
            worlds.push(w);
        }
    }
    let mut s = store();
    s.worlds = worlds;
    s.file = file;
}

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

/// Multiverse 4 (`worlds: <name>: ==: MVWorld ...`) -> one section per world, keys renamed as WorldConfig's 1.0
/// migrator does.
fn migrate_mv4(doc: &Y) -> Y {
    let mut out = Mapping::new();
    let Some(Y::Mapping(worlds)) = doc.get("worlds") else { return Y::Mapping(out) };
    for (k, v) in worlds {
        let Y::Mapping(old) = v else { continue };
        let mut m = old.clone();
        m.remove("==");
        let mv = |m: &mut Mapping, from: &str, to: &str| {
            if let Some(v) = at(m, from).cloned() {
                remove_at(m, from);
                set_at(m, to, v);
            }
        };
        for (from, to) in [
            ("adjustSpawn", "adjust-spawn"),
            ("allowFlight", "allow-flight"),
            ("allowWeather", "allow-weather"),
            ("autoHeal", "auto-heal"),
            ("autoLoad", "auto-load"),
            ("bedRespawn", "bed-respawn"),
            ("entryfee.amount", "entry-fee.amount"),
            ("entryfee.currency", "entry-fee.currency"),
            ("gameMode", "gamemode"),
            ("keepSpawnInMemory", "keep-spawn-in-memory"),
            ("playerLimit", "player-limit"),
            ("portalForm", "portal-form"),
            ("respawnWorld", "respawn-world"),
            ("spawnLocation", "spawn-location"),
            ("spawning.animals.spawnrate", "spawning.animals.tick-rate"),
            ("spawning.monsters.spawnrate", "spawning.monsters.tick-rate"),
            ("worldBlacklist", "world-blacklist"),
        ] {
            mv(&mut m, from, to);
        }
        m.remove("entryfee");
        // An alias with a colour of its own: the colour goes in front of the alias.
        if let Some(color) = m.remove("color").and_then(|c| c.as_str().map(str::to_owned)) {
            let alias = m.get("alias").and_then(Y::as_str).unwrap_or("").to_owned();
            if !alias.is_empty() && !alias.starts_with('&') {
                if let Some(code) = color_code(&color) {
                    m.insert("alias".into(), format!("&{code}{alias}").into());
                }
            }
        }
        if let Some(Y::Mapping(spawn)) = m.get_mut("spawn-location") {
            spawn.insert("==".into(), "MVSpawnLocation".into());
        }
        m.remove("style");
        m.insert("version".into(), 1.0.into());
        if let Some(k) = k.as_str() {
            out.insert(k.into(), Y::Mapping(m));
        }
    }
    Y::Mapping(out)
}

fn color_code(name: &str) -> Option<char> {
    let names = ["BLACK", "DARK_BLUE", "DARK_GREEN", "DARK_AQUA", "DARK_RED", "DARK_PURPLE", "GOLD", "GRAY", "DARK_GRAY", "BLUE", "GREEN", "AQUA", "RED", "LIGHT_PURPLE", "YELLOW", "WHITE"];
    names.iter().position(|n| n.eq_ignore_ascii_case(name)).map(|i| "0123456789abcdef".as_bytes()[i] as char)
}

fn remove_at(map: &mut Mapping, path: &str) {
    match path.split_once('.') {
        None => {
            map.remove(path);
        }
        Some((h, r)) => {
            if let Some(Y::Mapping(inner)) = map.get_mut(h) {
                remove_at(inner, r);
            }
        }
    }
}

/// WorldConfig's 1.2 and 1.3 migrators.
fn migrate_section(w: &mut MvWorld) {
    let version = w.f("version", 1.0);
    if version < 1.2 {
        for (from, to) in [("spawning.animals", "spawning.animal"), ("spawning.monsters", "spawning.monster")] {
            if let Some(v) = at(&w.data, from).cloned() {
                remove_at(&mut w.data, from);
                set_at(&mut w.data, to, v);
            }
        }
    }
    if version < 1.3 {
        let name = w.name.clone();
        w.set("read-only.legacy-world-name", name.into());
        for (from, to) in [("environment", "read-only.environment"), ("seed", "read-only.seed")] {
            if let Some(v) = at(&w.data, from).cloned() {
                remove_at(&mut w.data, from);
                set_at(&mut w.data, to, v);
            }
        }
        w.set("version", 1.3.into());
    }
}

/// worlds.yml written (made here, written on the writer thread).
pub fn save() {
    let s = store();
    let mut top = Mapping::new();
    for w in &s.worlds {
        top.insert(Y::String(w.key.replace('.', "[dot]")), Y::Mapping(w.data.clone()));
    }
    if let Ok(text) = serde_yaml::to_string(&Y::Mapping(top)) {
        write_file(s.file.clone(), text);
    }
}

// ---- Lookups ----

/// A world by name or (colourless) alias, any case.
pub fn find(input: &str) -> Option<MvWorld> {
    let s = store();
    s.worlds.iter().find(|w| w.name.eq_ignore_ascii_case(input) || w.key.eq_ignore_ascii_case(input)).or_else(|| s.worlds.iter().find(|w| !w.alias().is_empty() && horizon_kit::text::strip(&horizon_kit::text::colorize(&w.alias())).eq_ignore_ascii_case(input))).cloned()
}

/// A look at one world by exact name (case-insensitive) without copying it: for the hot events.
pub fn peek<R>(name: &str, f: impl FnOnce(&MvWorld) -> R) -> Option<R> {
    let s = store();
    s.worlds.iter().find(|w| w.name.eq_ignore_ascii_case(name)).map(f)
}

pub fn all() -> Vec<MvWorld> {
    store().worlds.clone()
}

pub fn names() -> Vec<String> {
    store().worlds.iter().map(|w| w.name.clone()).collect()
}

/// Changes a world's section (and saves the file).
pub fn update(name: &str, f: impl FnOnce(&mut MvWorld)) -> bool {
    let mut s = store();
    let Some(w) = s.worlds.iter_mut().find(|w| w.name.eq_ignore_ascii_case(name)) else { return false };
    f(w);
    drop(s);
    save();
    true
}

pub fn add(w: MvWorld) {
    let mut s = store();
    s.worlds.retain(|o| !o.name.eq_ignore_ascii_case(&w.name));
    s.worlds.push(w);
    drop(s);
    save();
}

pub fn remove(name: &str) -> bool {
    let mut s = store();
    let before = s.worlds.len();
    s.worlds.retain(|o| !o.name.eq_ignore_ascii_case(name));
    let changed = s.worlds.len() != before;
    drop(s);
    if changed {
        save();
    }
    changed
}

// ---- Worlds in the game ----

/// The folder a world's chunks live in on this server (`<level>/dimensions/minecraft/<name>`), and the old
/// stand-alone folder (`<server>/<name>`), whichever exists first.
pub fn folder_of(name: &str) -> PathBuf {
    if let Some(w) = horizon_bukkit::get_world(name) {
        return w.get_world_folder();
    }
    let overworld = World::of("world").get_world_folder();
    let dims = overworld.parent().map(PathBuf::from).unwrap_or_default();
    let modern = dims.join(name.to_ascii_lowercase());
    if modern.exists() {
        return modern;
    }
    let legacy = horizon_bukkit::server::get_world_container().join(name);
    if legacy.exists() { legacy } else { modern }
}

pub fn folder_looks_like_world(path: &std::path::Path) -> bool {
    path.join("level.dat").exists() || path.join("region").is_dir() || path.join("data").is_dir()
}

pub fn is_default(name: &str) -> bool {
    matches!(name, "world" | "world_nether" | "world_the_end")
}

/// Makes (or loads) the world in the game from its section; its properties applied.
pub fn load_in_game(w: &MvWorld) -> Result<World, String> {
    if let Some(world) = w.world() {
        apply(w, &world);
        return Ok(world);
    }
    let mut creator = horizon_bukkit::worlds::WorldCreator::new(&w.name);
    creator.environment(&w.environment());
    if let Some(seed) = w.seed() {
        creator.seed(seed);
    }
    let settings = w.s("read-only.generator-settings", "");
    if !settings.is_empty() {
        creator.generator_settings(&settings);
    }
    let generator = w.generator();
    if !generator.is_empty() {
        match horizon_bukkit::worlds::get_generator_for_name(&generator) {
            Some(g) => {
                creator.generator(g);
            }
            None => return Err(crate::util::m("worldcreator.invalidchunkgenerator", &[("generator", &generator), ("error", "No such generator.")])),
        }
    }
    let world = creator.create_world().ok_or_else(|| crate::util::m("worldcreator.bukkitcreationfailed", &[("world", &w.name), ("error", "the server refused")]))?;
    apply(w, &world);
    Ok(world)
}

/// The properties the game itself keeps, set on the world.
pub fn apply(w: &MvWorld, world: &World) {
    world.set_pvp(w.b("pvp", true));
    if let Some(Y::Mapping(m)) = w.get("spawn-location")
        && m.contains_key("x")
        && let Some(at) = w.spawn()
    {
        world.set_spawn_location(&at);
    }
    if !w.b("allow-weather", true) && world.has_storm() {
        world.set_storm(false);
    }
    if let Some(false) = w.get("spawning.monster.spawn").and_then(Y::as_bool) {
        world.set_allow_monster_spawning(false);
    }
}

/// Auto-load on enable: every auto-load world made/loaded, the server's own worlds added when missing.
pub fn load_all() {
    if crate::config::b("world.auto-import-default-worlds", true) {
        for world in horizon_bukkit::get_worlds() {
            let name = world.get_name().to_owned();
            if find(&name).is_none() {
                let env = world.get_environment().to_owned();
                let mut data = default_data(&env, Some(world.get_seed()), "");
                set_at(&mut data, "read-only.legacy-world-name", name.clone().into());
                add(MvWorld { key: name.clone(), name, data });
            }
        }
    }
    let mut loaded = 0;
    for w in all() {
        if !w.b("auto-load", true) && !is_default(&w.name) {
            continue;
        }
        match load_in_game(&w) {
            Ok(_) => loaded += 1,
            Err(e) => log(&format!("Failed to load world '{}': {}", w.name, horizon_kit::text::strip(&e))),
        }
    }
    log(&format!("{loaded} - World(s) loaded."));
}
