//! `MobData`: what `/spawnmob pig:saddle` can say. Java asks each class; here what's set is the
//! entity's saved data, which is the same fields, written once the mob is out.
use crate::prelude::*;
use horizon_bukkit::entity::Entity;
use std::sync::LazyLock;

const AGEABLE: &[&str] = &[
    "allay", "armadillo", "axolotl", "bee", "camel", "camel_husk", "cat", "chicken", "cod", "cow", "dolphin", "donkey", "fox", "frog", "goat", "hoglin", "horse", "llama", "mooshroom", "mule", "nautilus", "ocelot", "panda", "parrot", "pig", "piglin", "polar_bear", "rabbit", "sheep", "skeleton_horse", "sniffer",
    "strider", "tadpole", "trader_llama", "turtle", "villager", "wandering_trader", "wolf", "zoglin", "zombie_horse", "zombie_nautilus",
];
const TAMEABLE: &[&str] = &["camel", "camel_husk", "cat", "donkey", "horse", "llama", "mule", "nautilus", "parrot", "skeleton_horse", "trader_llama", "wolf", "zombie_horse", "zombie_nautilus"];
const ZOMBIES: &[&str] = &["drowned", "husk", "zombie", "zombie_villager", "zombified_piglin"];
const RAIDERS: &[&str] = &["evoker", "illusioner", "pillager", "ravager", "vindicator", "witch"];
const DYES: [&str; 16] = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];

/// `type`: an entity, or a class as the entities in it.
#[derive(Clone, Copy)]
enum Kind {
    Is(&'static str),
    Class(&'static [&'static str]),
    Boat,
}

#[derive(Clone, Copy)]
enum Value {
    Adult,
    Baby,
    AdultZombie,
    BabyZombie,
    Tamed,
    Colorable,
    Exp,
    Size,
    HorseColor(i32),
    HorseStyle(i32),
    /// Saddled by the player, as `HORSESADDLE` and the camel's and nautilus's are.
    OwnedSaddle,
    PigSaddle,
    HorseArmor(&'static str),
    Sword(&'static str),
    FishBody,
    FishPattern,
    FishType(i32),
    Angry,
    /// Saved data as it stands.
    Nbt(&'static str),
    /// Not something that can change: the boat's wood is its entity, on this version of the game.
    Nothing,
}

pub struct MobData {
    nickname: &'static str,
    /// The words to match: the nickname, unless this holds more.
    suggestions: Vec<String>,
    kind: Kind,
    value: Value,
    is_public: bool,
}

fn data(nickname: &'static str, kind: Kind, value: Value, is_public: bool) -> MobData {
    MobData { nickname, suggestions: vec![nickname.to_owned()], kind, value, is_public }
}

fn listed(nickname: &'static str, suggestions: Vec<String>, kind: Kind, value: Value) -> MobData {
    MobData { nickname, suggestions, kind, value, is_public: true }
}

fn dyes(suffix: &str) -> Vec<String> {
    DYES.iter().map(|dye| format!("{dye}{suffix}")).collect()
}

fn table() -> Vec<MobData> {
    use Kind::{Boat, Class, Is};
    use Value::{Adult, AdultZombie, Angry, Baby, BabyZombie, Colorable, Exp, FishBody, FishPattern, FishType, HorseArmor, HorseColor, HorseStyle, Nbt, Nothing, OwnedSaddle, PigSaddle, Size, Sword, Tamed};
    let horse = Is("horse");
    let cat = Is("cat");
    let zombie = Class(ZOMBIES);
    let skeleton = Is("skeleton");
    let villager = Is("villager");
    let parrot = Is("parrot");
    let fish = Is("tropical_fish");
    let panda = Is("panda");
    let llama = Is("llama");
    let trader = Is("trader_llama");
    let axolotl = Is("axolotl");
    let frog = Is("frog");
    let wolf = Is("wolf");
    let cow = Is("cow");
    let chicken = Is("chicken");
    let pig = Is("pig");
    let salmon = Is("salmon");
    let nautilus = Is("zombie_nautilus");
    let mut rows = vec![
        data("baby", Class(AGEABLE), Baby, true),
        data("adult", Class(AGEABLE), Adult, true),
        data("kitten", cat, Baby, false),
        data("piglet", pig, Baby, false),
        data("puppy", wolf, Baby, false),
        data("chick", chicken, Baby, false),
        data("colt", horse, Baby, false),
        data("kitten", Is("ocelot"), Baby, false),
        data("lamb", Is("sheep"), Baby, false),
        data("calf", cow, Baby, false),
        data("child", villager, Baby, false),
        data("tamed", Class(TAMEABLE), Tamed, true),
        data("tame", Class(TAMEABLE), Tamed, false),
        data("random", Is("sheep"), Colorable, true),
        listed("", dyes(""), Is("sheep"), Colorable),
        data("polka", horse, HorseStyle(4), true),
        data("sooty", horse, HorseStyle(4), false),
        data("blaze", horse, HorseStyle(1), true),
        data("socks", horse, HorseStyle(1), false),
        data("leopard", horse, HorseStyle(3), true),
        data("appaloosa", horse, HorseStyle(3), false),
        data("paint", horse, HorseStyle(2), true),
        data("milky", horse, HorseStyle(2), false),
        data("splotchy", horse, HorseStyle(2), false),
        data("black", horse, HorseColor(4), true),
        data("chestnut", horse, HorseColor(2), true),
        data("liver", horse, HorseColor(2), false),
        data("creamy", horse, HorseColor(1), true),
        data("flaxen", horse, HorseColor(1), false),
        data("gray", horse, HorseColor(5), true),
        data("dapple", horse, HorseColor(5), false),
        data("buckskin", horse, HorseColor(6), true),
        data("darkbrown", horse, HorseColor(6), false),
        data("dark", horse, HorseColor(6), false),
        data("dbrown", horse, HorseColor(6), false),
        data("bay", horse, HorseColor(3), true),
        data("brown", horse, HorseColor(3), false),
        data("saddle", horse, OwnedSaddle, true),
        data("goldarmor", horse, HorseArmor("golden_horse_armor"), true),
        data("diamondarmor", horse, HorseArmor("diamond_horse_armor"), true),
        data("netheritearmor", horse, HorseArmor("netherite_horse_armor"), true),
        data("armor", horse, HorseArmor("iron_horse_armor"), true),
        data("siamese", cat, Nbt("{variant:\"minecraft:siamese\"}"), true),
        data("white", cat, Nbt("{variant:\"minecraft:white\"}"), false),
        data("red", cat, Nbt("{variant:\"minecraft:red\"}"), true),
        data("orange", cat, Nbt("{variant:\"minecraft:red\"}"), false),
        data("tabby", cat, Nbt("{variant:\"minecraft:tabby\"}"), true),
        data("black", cat, Nbt("{variant:\"minecraft:all_black\"}"), true),
        data("tuxedo", cat, Nbt("{variant:\"minecraft:black\"}"), true),
        data("britishshorthair", cat, Nbt("{variant:\"minecraft:british_shorthair\"}"), true),
        data("calico", cat, Nbt("{variant:\"minecraft:calico\"}"), true),
        data("persian", cat, Nbt("{variant:\"minecraft:persian\"}"), true),
        data("ragdoll", cat, Nbt("{variant:\"minecraft:ragdoll\"}"), true),
        data("jellie", cat, Nbt("{variant:\"minecraft:jellie\"}"), true),
        data("allblack", cat, Nbt("{variant:\"minecraft:all_black\"}"), true),
        data("baby", zombie, BabyZombie, true),
        data("adult", zombie, AdultZombie, true),
    ];
    for group in [zombie, skeleton] {
        rows.extend([
            data("netheritesword", group, Sword("netherite_sword"), true),
            data("diamondsword", group, Sword("diamond_sword"), true),
            data("goldsword", group, Sword("golden_sword"), true),
            data("ironsword", group, Sword("iron_sword"), true),
            data("stonesword", group, Sword("stone_sword"), false),
            data("sword", group, Sword("stone_sword"), true),
        ]);
    }
    rows.extend([
        data("bow", skeleton, Sword("bow"), true),
        data("powered", Is("creeper"), Nbt("{powered:1b}"), true),
        data("electric", Is("creeper"), Nbt("{powered:1b}"), false),
        data("charged", Is("creeper"), Nbt("{powered:1b}"), false),
        data("saddle", pig, PigSaddle, true),
        data("angry", wolf, Angry, true),
        data("rabid", wolf, Angry, false),
    ]);
    for (nickname, profession) in [("villager", "none"), ("armorer", "armorer"), ("butcher", "butcher"), ("cartographer", "cartographer"), ("cleric", "cleric"), ("farmer", "farmer"), ("fisherman", "fisherman"), ("fletcher", "fletcher"), ("leatherworker", "leatherworker"), ("librarian", "librarian"), ("mason", "mason"), ("nitwit", "nitwit"), ("shepherd", "shepherd"), ("toolsmith", "toolsmith"), ("weaponsmith", "weaponsmith")] {
        rows.push(data(nickname, villager, Nbt(leak(format!("{{VillagerData:{{profession:\"minecraft:{profession}\"}}}}"))), true));
    }
    for name in ["desert", "jungle", "plains", "savanna", "snowy", "swamp", "taiga"] {
        rows.push(data(name, villager, Nbt(leak(format!("{{VillagerData:{{type:\"minecraft:{name}\"}}}}"))), true));
    }
    rows.push(listed("", vec!["<1-100>".to_owned()], Is("slime"), Size));
    rows.push(listed("", vec!["<1-2000000000>".to_owned()], Is("experience_orb"), Exp));
    for (name, variant) in [("red", 0), ("green", 2), ("blue", 1), ("cyan", 3), ("gray", 4)] {
        rows.push(data(name, parrot, Nbt(leak(format!("{{Variant:{variant}}}"))), true));
    }
    for (name, pattern) in [("kob", 0), ("sunstreak", 256), ("snooper", 512), ("dasher", 768), ("brinely", 1024), ("spotty", 1280), ("flopper", 1), ("stripey", 257), ("glitter", 513), ("blockfish", 769), ("betty", 1025), ("clayfish", 1281)] {
        rows.push(data(name, fish, FishType(pattern), true));
    }
    rows.push(data("brown", Is("mooshroom"), Nbt("{Type:\"brown\"}"), true));
    rows.push(data("red", Is("mooshroom"), Nbt("{Type:\"red\"}"), true));
    for gene in ["aggressive", "lazy", "worried", "playful", "brown", "weak"] {
        rows.push(data(gene, panda, Nbt(leak(format!("{{MainGene:\"{gene}\"}}"))), true));
    }
    for (name, gene) in [("aggressive_hidden", "aggressive"), ("lazy_hidden", "lazy"), ("worried_hidden", "worried"), ("playful_hidden", "playful"), ("brown_hidden", "brown"), ("weak_hidden", "weak")] {
        rows.push(data(name, panda, Nbt(leak(format!("{{HiddenGene:\"{gene}\"}}"))), true));
    }
    for group in [llama, trader] {
        for (name, variant) in [("creamy", 0), ("white", 1), ("brown", 2), ("gray", 3)] {
            rows.push(data(name, group, Nbt(leak(format!("{{Variant:{variant}}}"))), true));
        }
    }
    rows.push(data("random", Is("shulker"), Colorable, true));
    rows.push(listed("", dyes(""), Is("shulker"), Colorable));
    rows.push(data("red", Is("fox"), Nbt("{Type:\"red\"}"), true));
    rows.push(data("snow", Is("fox"), Nbt("{Type:\"snow\"}"), true));
    rows.push(listed("", vec!["<1-100>".to_owned()], Is("phantom"), Size));
    rows.push(data("leader", Class(RAIDERS), Nbt("{PatrolLeader:1b}"), true));
    rows.push(listed("fish_body_color", dyes("body"), fish, FishBody));
    rows.push(listed("fish_pattern_color", dyes("pattern"), fish, FishPattern));
    rows.extend([
        data("lucy", axolotl, Nbt("{Variant:0}"), true),
        data("leucistic", axolotl, Nbt("{Variant:0}"), false),
        data("pink", axolotl, Nbt("{Variant:0}"), false),
        data("wild", axolotl, Nbt("{Variant:1}"), true),
        data("brown", axolotl, Nbt("{Variant:1}"), false),
        data("gold", axolotl, Nbt("{Variant:2}"), true),
        data("cyan", axolotl, Nbt("{Variant:3}"), true),
        data("blue", axolotl, Nbt("{Variant:4}"), true),
        data("screaming", Is("goat"), Nbt("{IsScreamingGoat:1b}"), true),
        data("temperate", frog, Nbt("{variant:\"minecraft:temperate\"}"), true),
        data("warm", frog, Nbt("{variant:\"minecraft:warm\"}"), true),
        data("cold", frog, Nbt("{variant:\"minecraft:cold\"}"), true),
    ]);
    for name in ["acacia", "birch", "darkoak", "generic", "jungle", "redwood", "mangrove", "oak", "spruce"] {
        rows.push(data(name, Boat, Nothing, true));
    }
    rows.push(data("saddle", Is("camel"), OwnedSaddle, true));
    for name in ["pale", "spotted", "snowy", "black", "ashen", "rusty", "woods", "chestnut", "striped"] {
        // `wolf:PALE` stands for all three of the first, as in Java.
        let variant = if matches!(name, "spotted" | "snowy") { "pale" } else { name };
        rows.push(data(name, wolf, Nbt(leak(format!("{{variant:\"minecraft:{variant}\"}}"))), true));
    }
    for name in ["small", "medium", "large"] {
        rows.push(data(name, salmon, Nbt(leak(format!("{{type:\"{name}\"}}"))), true));
    }
    for group in [cow, chicken, pig] {
        for name in ["temperate", "warm", "cold"] {
            rows.push(data(name, group, Nbt(leak(format!("{{variant:\"minecraft:{name}\"}}"))), true));
        }
    }
    rows.push(data("saddle", Is("camel_husk"), OwnedSaddle, true));
    rows.push(data("temperate", nautilus, Nbt("{variant:\"minecraft:temperate\"}"), true));
    rows.push(data("warm", nautilus, Nbt("{variant:\"minecraft:warm\"}"), true));
    rows.push(data("saddle", Is("nautilus"), OwnedSaddle, true));
    rows.push(data("saddle", nautilus, OwnedSaddle, true));
    rows
}

/// The table lives as long as the plugin, so its few formed strings can too.
fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

static TABLE: LazyLock<Vec<MobData>> = LazyLock::new(table);

fn id_of(spawned: &Entity) -> String {
    spawned.get_type().trim_start_matches("minecraft:").to_lowercase()
}

impl MobData {
    fn fits(&self, id: &str) -> bool {
        match self.kind {
            Kind::Is(name) => name == id,
            Kind::Class(list) => list.contains(&id),
            Kind::Boat => id.ends_with("_boat") || id.ends_with("_raft"),
        }
    }
}

/// `getPossibleData`: a later row of the same nickname takes the place of an earlier one, as in a map.
fn possible(spawned: &Entity, public_only: bool) -> Vec<&'static MobData> {
    let id = id_of(spawned);
    let mut list: Vec<&'static MobData> = Vec::new();
    for row in TABLE.iter().filter(|row| row.fits(&id) && (!public_only || row.is_public)) {
        match list.iter_mut().find(|known| known.nickname == row.nickname) {
            Some(known) => *known = row,
            None => list.push(row),
        }
    }
    list
}

/// `getValidHelp`.
pub fn get_valid_help(spawned: &Entity) -> Vec<String> {
    possible(spawned, true).iter().map(|row| join_list(&row.suggestions)).collect()
}

/// `fromData`.
pub fn from_data(spawned: &Entity, name: &str) -> Option<&'static MobData> {
    if name.is_empty() {
        return None;
    }
    possible(spawned, false).into_iter().find(|row| row.suggestions.iter().any(|suggestion| name.contains(suggestion.as_str())))
}

/// `[I;a,b,c,d]`: an id as saved data has it.
pub fn uuid_snbt(uuid: &str) -> String {
    let hex = uuid.replace('-', "");
    let part = |index: usize| hex.get(index * 8..index * 8 + 8).and_then(|text| u32::from_str_radix(text, 16).ok()).unwrap_or(0) as i32;
    format!("[I;{},{},{},{}]", part(0), part(1), part(2), part(3))
}

fn owned_by(target: &Player) -> String {
    format!("Owner:{}", uuid_snbt(&target.get_unique_id()))
}

fn item_snbt(id: &str) -> String {
    format!("{{id:\"minecraft:{id}\",count:1}}")
}

fn random_below(bound: usize) -> usize {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new().build_hasher().finish() as usize % bound
}

impl MobData {
    /// `setData`: what of `raw` it took, for `/spawnmob` to take off the words.
    pub fn set_data(&self, spawned: &Entity, target: &Player, raw: &str) -> EssResult<String> {
        let mut matched = self.nickname.to_owned();
        match self.value {
            Value::Adult => {
                spawned.merge("{Age:0}");
            }
            Value::Baby => {
                spawned.merge("{Age:-24000}");
            }
            Value::AdultZombie => {
                spawned.merge("{IsBaby:0b}");
            }
            Value::BabyZombie => {
                spawned.merge("{IsBaby:1b}");
            }
            Value::Tamed => {
                let tamed = if id_of(spawned) == "wolf" || id_of(spawned) == "cat" || id_of(spawned) == "parrot" { format!("{{{}}}", owned_by(target)) } else { format!("{{Tame:1b,{}}}", owned_by(target)) };
                spawned.merge(&tamed);
            }
            Value::Colorable => {
                let word = raw.to_uppercase();
                if word == "RANDOM" {
                    spawned.merge(&format!("{{Color:{}b}}", random_below(DYES.len())));
                } else if !word.is_empty() {
                    let dye = DYES.iter().position(|dye| dye.to_uppercase() == word).ok_or_else(|| EssError::tl("sheepMalformedColor", &[]))?;
                    spawned.merge(&format!("{{Color:{dye}b}}"));
                }
                raw.clone_into(&mut matched);
            }
            Value::Exp => {
                let amount: i32 = raw.parse().map_err(|_| EssError::tl("invalidNumber", &[]))?;
                spawned.merge(&format!("{{Value:{}s}}", amount.clamp(0, i32::from(i16::MAX))));
                raw.clone_into(&mut matched);
            }
            Value::Size => {
                let size: i32 = raw.parse().map_err(|_| EssError::tl("slimeMalformedSize", &[]))?;
                let saved = if id_of(spawned) == "phantom" { format!("{{size:{size}}}") } else { format!("{{Size:{}}}", size - 1) };
                spawned.merge(&saved);
                raw.clone_into(&mut matched);
            }
            Value::HorseColor(color) => {
                let style = spawned.data_at("Variant").as_i64().unwrap_or(0) & !0xff;
                spawned.merge(&format!("{{Variant:{}}}", style | i64::from(color)));
            }
            Value::HorseStyle(style) => {
                let color = spawned.data_at("Variant").as_i64().unwrap_or(0) & 0xff;
                spawned.merge(&format!("{{Variant:{}}}", color | (i64::from(style) << 8)));
            }
            Value::OwnedSaddle => {
                spawned.merge(&format!("{{Tame:1b,{},equipment:{{saddle:{}}}}}", owned_by(target), item_snbt("saddle")));
            }
            Value::PigSaddle => {
                spawned.merge(&format!("{{equipment:{{saddle:{}}}}}", item_snbt("saddle")));
            }
            Value::HorseArmor(item) => {
                spawned.merge(&format!("{{Tame:1b,equipment:{{body:{}}}}}", item_snbt(item)));
            }
            Value::Sword(item) => {
                spawned.merge(&format!("{{equipment:{{mainhand:{}}},drop_chances:{{mainhand:0.1f}}}}", item_snbt(item)));
            }
            Value::FishType(pattern) => {
                let colors = spawned.data_at("Variant").as_i64().unwrap_or(0) & !0xffff;
                spawned.merge(&format!("{{Variant:{}}}", colors | i64::from(pattern)));
            }
            Value::FishBody | Value::FishPattern => {
                let (suffix, shift) = if matches!(self.value, Value::FishBody) { ("body", 16) } else { ("pattern", 24) };
                if let Some(word) = self.suggestions.iter().find(|word| raw.contains(word.as_str())) {
                    word.clone_into(&mut matched);
                    let dye = DYES.iter().position(|dye| *dye == word.trim_end_matches(suffix)).unwrap_or(0) as i64;
                    let others = spawned.data_at("Variant").as_i64().unwrap_or(0) & !(0xff << shift);
                    spawned.merge(&format!("{{Variant:{}}}", others | (dye << shift)));
                }
            }
            Value::Angry => {
                spawned.merge(&format!("{{anger_end_time:{}L}}", spawned.get_world().get_full_time() + 400));
            }
            Value::Nbt(snbt) => {
                spawned.merge(snbt);
            }
            Value::Nothing => {}
        }
        Ok(matched)
    }
}
