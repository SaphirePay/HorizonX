//! Gathering: Mining, Woodcutting, Excavation and Herbalism on block breaks (XP from experience.yml, bonus drops,
//! Super Breaker, Tree Feller, Giga Drill Breaker treasures, Green Terra / Green Thumb, Blast Mining), and
//! placed-block tracking. A break costs one block-data read and one held-item read; the rest is table lookups.

use crate::mmo::{self, Out, Reason};
use crate::placed;
use crate::profile;
use crate::settings::{self, Settings};
use crate::skills::{Ability, Skill, Tool};
use horizon_bukkit::{Block, ItemStack, Material, Player};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

/// `minecraft:oak_log[axis=y]` -> `oak_log`.
pub fn mat(data: &str) -> &str {
    let s = data.trim_start_matches("minecraft:");
    s.split('[').next().unwrap_or(s)
}

/// A block-state property (`age`).
pub fn prop<'a>(data: &'a str, key: &str) -> Option<&'a str> {
    let inner = data.split_once('[')?.1.trim_end_matches(']');
    inner.split(',').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
}

/// A block's middle.
pub fn center(b: &Block) -> horizon_bukkit::Location {
    let mut at = b.get_location();
    at.add(0.5, 0.5, 0.5);
    at
}

fn rand() -> f64 {
    use std::cell::Cell;
    thread_local!(static SEED: Cell<u64> = Cell::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7) | 1));
    SEED.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}

/// A roll of `chance` percent (0-100).
pub fn roll(chance: f64) -> bool {
    rand() * 100.0 < chance
}

pub fn rand_unit() -> f64 {
    rand()
}

fn crop_max_age(m: &str) -> Option<i32> {
    Some(match m {
        "wheat" | "carrots" | "potatoes" => 7,
        "beetroots" | "nether_wart" | "sweet_berry_bush" | "frosted_ice" => 3,
        "cocoa" => 2,
        "torchflower_crop" => 1,
        "pitcher_crop" => 4,
        _ => return None,
    })
}

fn is_tall_plant(m: &str) -> bool {
    matches!(m, "sugar_cane" | "cactus" | "bamboo" | "kelp" | "kelp_plant" | "twisting_vines" | "twisting_vines_plant" | "weeping_vines" | "weeping_vines_plant" | "cave_vines" | "cave_vines_plant")
}

pub fn is_log(m: &str) -> bool {
    m.ends_with("_log") || m.ends_with("_wood") || m.ends_with("_stem") || m.ends_with("_hyphae")
}

fn is_leaves(m: &str) -> bool {
    m.ends_with("_leaves") || m == "nether_wart_block" || m == "warped_wart_block" || m == "shroomlight" || m == "mangrove_roots" || m.ends_with("_mushroom_block") || m == "mushroom_stem"
}

fn seed_of(m: &str) -> Option<&'static str> {
    Some(match m {
        "wheat" => "wheat_seeds",
        "carrots" => "carrot",
        "potatoes" => "potato",
        "beetroots" => "beetroot_seeds",
        "nether_wart" => "nether_wart",
        "cocoa" => "cocoa_beans",
        _ => return None,
    })
}

/// The skill whose XP table has the block (Herbalism first, as mcMMO checks it first).
fn skill_of(s: &Settings, m: &str) -> Option<Skill> {
    for skill in [Skill::Herbalism, Skill::Mining, Skill::Woodcutting, Skill::Excavation] {
        if s.xp[skill.idx()].contains_key(m) {
            return Some(skill);
        }
    }
    None
}

fn tool_fits(skill: Skill, tool: Option<Tool>) -> bool {
    match skill {
        Skill::Mining => matches!(tool, Some(Tool::Pickaxe | Tool::Hoe)),
        Skill::Woodcutting => tool == Some(Tool::Axe),
        Skill::Excavation => tool == Some(Tool::Shovel),
        _ => true,
    }
}

struct View {
    level: i32,
    perm: bool,
    creative: bool,
    lucky: f64,
    active: [bool; 8],
}

fn view(uuid: &str, skill: Skill) -> Option<View> {
    let ss = mmo::sessions();
    let sess = ss.get(uuid)?;
    let st = profile::store();
    let p = st.get(uuid)?;
    Some(View { level: p.level(skill), perm: sess.skill_perm(skill), creative: sess.creative, lucky: sess.lucky(skill), active: crate::skills::ABILITIES.map(|a| sess.ability_active(a)) })
}

/// Extra copies of a block's drops, dropped at its place next tick.
fn bonus_drops(block: &Block, tool: Option<&ItemStack>, copies: u32) {
    if copies == 0 {
        return;
    }
    let drops = block.get_drops(tool);
    if drops.is_empty() {
        return;
    }
    let at = center(block);
    crate::later(move || {
        let world = at.get_world();
        for _ in 0..copies {
            for d in &drops {
                world.drop_item_naturally(&at, d);
            }
        }
    });
}

static HELD: std::sync::Mutex<Option<std::collections::HashMap<String, horizon_bukkit::ItemStack>>> = std::sync::Mutex::new(None);

/// Remembers the player's main-hand item (left-click on a block, before the break).
pub fn remember_held(player: &Player) {
    if let Some(h) = player.get_inventory().get_item_in_main_hand() {
        HELD.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(player.get_unique_id(), h);
    } else if let Some(m) = HELD.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        m.remove(&player.get_unique_id());
    }
}

/// The main-hand item now, else the one remembered when digging started.
pub fn held_item(player: &Player) -> Option<horizon_bukkit::ItemStack> {
    player.get_inventory().get_item_in_main_hand().or_else(|| HELD.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&player.get_unique_id()).cloned()))
}

pub fn on_break(player: &Player, block: &Block) {
    let s = settings::get();
    let data = block.get_block_data();
    let m = mat(&data).to_owned();
    let world = block.get_world().get_name().to_owned();
    let (x, y, z) = (block.get_x(), block.get_y(), block.get_z());
    let was_placed = placed::is_placed(&world, x, y, z);
    if was_placed {
        placed::set(&world, x, y, z, false);
    }
    let Some(skill) = skill_of(&s, &m) else { return };
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let Some(v) = view(&uuid, skill) else { return };
    if v.creative || !v.perm {
        return;
    }
    let held = held_item(player);
    let held_id = held.as_ref().map(|h| h.get_type().key().to_owned()).unwrap_or_default();
    let tool = Tool::of(&held_id);
    if skill == Skill::Herbalism {
        herbalism(&s, player, &uuid, block, &data, &m, was_placed, &v, held.as_ref());
        return;
    }
    if was_placed || !tool_fits(skill, tool) {
        return;
    }
    let xp = s.xp[skill.idx()].get(m.as_str()).copied().unwrap_or(0.0);
    match skill {
        Skill::Mining => {
            mmo::award(&uuid, Skill::Mining, xp as f32, Reason::Gathering);
            let silk = held.as_ref().is_some_and(|h| h.get_enchant_level("minecraft:silk_touch") > 0);
            if silk && !s.adv_bool.get("Skills.Mining.DoubleDrops.SilkTouch").copied().unwrap_or(true) {
                return;
            }
            let mut copies = 0;
            if s.rank("Mining.DoubleDrops", v.level) > 0 && roll(s.chance("Skills.Mining.DoubleDrops", v.level) * v.lucky) {
                copies = if v.active[Ability::SuperBreaker.idx()] && s.adv_bool.get("Skills.Mining.SuperBreaker.AllowTripleDrops").copied().unwrap_or(true) { 2 } else { 1 };
            }
            if copies < 2 && s.rank("Mining.MotherLode", v.level) > 0 && roll(s.chance("Skills.Mining.MotherLode", v.level) * v.lucky) {
                copies = 2;
            }
            bonus_drops(block, held.as_ref(), copies);
        }
        Skill::Woodcutting => {
            if v.active[Ability::TreeFeller.idx()] {
                tree_feller(&s, player, &uuid, block, &m, held.as_ref(), &v);
                return;
            }
            mmo::award(&uuid, Skill::Woodcutting, xp as f32, Reason::Gathering);
            woodcutting_bonus(&s, block, held.as_ref(), &v);
        }
        Skill::Excavation => {
            mmo::award(&uuid, Skill::Excavation, xp as f32, Reason::Gathering);
            let rolls = if v.active[Ability::GigaDrillBreaker.idx()] { 3 } else { 1 };
            excavation_treasures(&s, &uuid, block, &m, &v, rolls);
        }
        _ => {}
    }
}

fn woodcutting_bonus(s: &Settings, block: &Block, tool: Option<&ItemStack>, v: &View) {
    let mut copies = 0;
    if s.rank("Woodcutting.HarvestLumber", v.level) > 0 && roll(s.chance("Skills.Woodcutting.HarvestLumber", v.level) * v.lucky) {
        copies = 1;
    }
    if s.rank("Woodcutting.CleanCuts", v.level) > 0 && roll(s.chance("Skills.Woodcutting.CleanCuts", v.level) * v.lucky) {
        copies = 2;
    }
    bonus_drops(block, tool, copies);
}

fn excavation_treasures(s: &Settings, uuid: &str, block: &Block, m: &str, v: &View, rolls: u32) {
    if s.rank("Excavation.Archaeology", v.level) == 0 && s.ranks.contains_key("Excavation.Archaeology") {
        return;
    }
    let at = center(block);
    let mut found: Vec<(String, i32)> = Vec::new();
    let mut xp = 0;
    for _ in 0..rolls {
        for t in &s.excavation {
            if v.level >= t.level && t.from.iter().any(|f| f == m) && roll(t.chance * v.lucky) {
                found.push((t.item.clone(), t.amount));
                xp += t.xp;
            }
        }
    }
    if xp > 0 {
        mmo::award(uuid, Skill::Excavation, xp as f32, Reason::Gathering);
    }
    if !found.is_empty() {
        crate::later(move || {
            let world = at.get_world();
            for (item, amount) in found {
                world.drop_item_naturally(&at, &ItemStack::new(Material::of(&item), amount));
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn herbalism(s: &Settings, player: &Player, uuid: &str, block: &Block, data: &str, m: &str, was_placed: bool, v: &View, held: Option<&ItemStack>) {
    let base = s.xp[Skill::Herbalism.idx()].get(m).copied().unwrap_or(0.0);
    let green_terra = v.active[Ability::GreenTerra.idx()];
    let mut xp = 0.0;
    let mut mature_crop = false;
    if let Some(max) = crop_max_age(m) {
        let age: i32 = prop(data, "age").and_then(|a| a.parse().ok()).unwrap_or(0);
        if age < max {
            return;
        }
        mature_crop = true;
        xp = base;
    } else if is_tall_plant(m) {
        // The broken block and those above it that break with it (not ones a player placed).
        let world = block.get_world().get_name().to_owned();
        if !was_placed {
            xp += base;
        }
        for dy in 1..32 {
            let up = block.get_relative(0, dy, 0);
            let ud = up.get_block_data();
            if mat(&ud) != m && !(m.starts_with(mat(&ud).trim_end_matches("_plant")) || mat(&ud).starts_with(m.trim_end_matches("_plant"))) {
                break;
            }
            if placed::is_placed(&world, up.get_x(), up.get_y(), up.get_z()) {
                placed::set(&world, up.get_x(), up.get_y(), up.get_z(), false);
            } else {
                xp += base;
            }
        }
    } else {
        if was_placed {
            return;
        }
        xp = base;
    }
    if xp > 0.0 {
        mmo::award(uuid, Skill::Herbalism, xp as f32, Reason::Gathering);
    }
    // Double drops (triple in Green Terra).
    if s.rank("Herbalism.DoubleDrops", v.level) > 0 && roll(s.chance("Skills.Herbalism.DoubleDrops", v.level) * v.lucky) {
        bonus_drops(block, held, if green_terra { 2 } else { 1 });
    }
    // Green Thumb: a ripe crop replanted (always in Green Terra), a seed taken from the inventory.
    if mature_crop
        && let Some(seed) = seed_of(m)
        && (green_terra || (s.rank("Herbalism.GreenThumb", v.level) > 0 && roll(s.chance("Skills.Herbalism.GreenThumb", v.level) * v.lucky)))
    {
        let stage = s.rank("Herbalism.GreenThumb", v.level).saturating_sub(1).min(crop_max_age(m).unwrap_or(1) as usize - 1);
        let block = block.clone();
        let player = player.clone();
        let base_data = data.split('[').next().unwrap_or(data).to_owned();
        let facing = prop(data, "facing").map(str::to_owned);
        crate::later(move || {
            let seed_stack = ItemStack::new(Material::of(seed), 1);
            if !player.get_inventory().contains_at_least(&seed_stack, 1) {
                return;
            }
            player.get_inventory().remove_item(&seed_stack);
            let state = match facing {
                Some(f) => format!("{base_data}[age={stage},facing={f}]"),
                None => format!("{base_data}[age={stage}]"),
            };
            block.set_block_data(&state);
            player.send_action_bar(&horizon_bukkit::adventure::legacy(&crate::locale::get("Herbalism.Ability.GTh", &[])));
        });
    }
}

fn tree_feller(s: &Settings, player: &Player, uuid: &str, start: &Block, start_mat: &str, tool: Option<&ItemStack>, v: &View) {
    const DIRS: [(i32, i32); 20] = [(-2, -1), (-2, 0), (-2, 1), (-1, -2), (-1, -1), (-1, 0), (-1, 1), (-1, 2), (0, -2), (0, -1), (0, 1), (0, 2), (1, -2), (1, -1), (1, 0), (1, 1), (1, 2), (2, -1), (2, 0), (2, 1)];
    let world = start.get_world().get_name().to_owned();
    let threshold = s.tree_feller_threshold;
    let mut cache: HashMap<(i32, i32, i32), String> = HashMap::new();
    let mut read = |b: &Block| -> String { cache.entry((b.get_x(), b.get_y(), b.get_z())).or_insert_with(|| mat(&b.get_block_data()).to_owned()).clone() };
    let mut found: HashSet<(i32, i32, i32)> = HashSet::new();
    let mut logs: Vec<(Block, String)> = Vec::new();
    let mut leaves: Vec<Block> = Vec::new();
    let mut queue: VecDeque<Block> = VecDeque::new();
    queue.push_back(start.clone());
    found.insert((start.get_x(), start.get_y(), start.get_z()));
    let mut too_big = false;
    let consider = |b: Block, read: &mut dyn FnMut(&Block) -> String, queue: &mut VecDeque<Block>, logs: &mut Vec<(Block, String)>, leaves: &mut Vec<Block>, found: &mut HashSet<(i32, i32, i32)>| -> bool {
        let key = (b.get_x(), b.get_y(), b.get_z());
        if found.contains(&key) || placed::is_placed(&world, key.0, key.1, key.2) {
            return false;
        }
        let m = read(&b);
        if is_log(&m) && s.xp[Skill::Woodcutting.idx()].contains_key(m.as_str()) {
            found.insert(key);
            logs.push((b.clone(), m));
            queue.push_back(b);
            true
        } else if is_leaves(&m) {
            found.insert(key);
            leaves.push(b);
            false
        } else {
            false
        }
    };
    while let Some(b) = queue.pop_front() {
        if found.len() > threshold {
            too_big = true;
            break;
        }
        let up = b.get_relative(0, 1, 0);
        if consider(up, &mut read, &mut queue, &mut logs, &mut leaves, &mut found) {
            for (dx, dz) in DIRS {
                consider(b.get_relative(dx, 0, dz), &mut read, &mut queue, &mut logs, &mut leaves, &mut found);
            }
        } else {
            consider(b.get_relative(0, -1, 0), &mut read, &mut queue, &mut logs, &mut leaves, &mut found);
            for dy in -1..=1 {
                for (dx, dz) in DIRS {
                    consider(b.get_relative(dx, dy, dz), &mut read, &mut queue, &mut logs, &mut leaves, &mut found);
                }
            }
        }
    }
    if too_big {
        player.send_message(&crate::locale::get("Woodcutting.Skills.TreeFeller.Threshold", &[]));
        // The broken log still counts as a plain chop.
        let xp = s.xp[Skill::Woodcutting.idx()].get(start_mat).copied().unwrap_or(0.0);
        mmo::award(uuid, Skill::Woodcutting, xp as f32, Reason::Gathering);
        return;
    }
    // XP: the first log in full, then 5 less for each log after it (ExploitFix.TreeFellerReducedXP).
    let mut xp = s.xp[Skill::Woodcutting.idx()].get(start_mat).copied().unwrap_or(0.0) as i64;
    for (i, (_, m)) in logs.iter().enumerate() {
        let raw = s.xp[Skill::Woodcutting.idx()].get(m.as_str()).copied().unwrap_or(0.0) as i64;
        xp += if s.tree_feller_reduced { (raw - (i as i64 + 1) * 5).max(1) } else { raw };
    }
    mmo::award(uuid, Skill::Woodcutting, xp as f32, Reason::Gathering);
    let tool = tool.cloned();
    let lucky = v.lucky;
    let level = v.level;
    let durability = s.durability_loss;
    let total = logs.len();
    let player = player.clone();
    let harvest = s.rank("Woodcutting.HarvestLumber", level) > 0;
    let chance = s.chance("Skills.Woodcutting.HarvestLumber", level) * lucky;
    crate::later(move || {
        for (b, _) in &logs {
            if harvest && roll(chance) {
                let drops = b.get_drops(tool.as_ref());
                let at = center(b);
                for d in drops {
                    at.get_world().drop_item_naturally(&at, &d);
                }
            }
            b.break_naturally_with(tool.as_ref(), false, false, false);
        }
        for b in &leaves {
            b.break_naturally_with(None, false, false, false);
        }
        // Tool wear: the ability's durability loss per log, as mcMMO's handleDurabilityLoss.
        if durability > 0
            && let Some(mut t) = player.get_inventory().get_item_in_main_hand()
            && t.get_type().get_max_durability() > 0
        {
            let unbreaking = t.get_enchant_level("minecraft:unbreaking") as i32;
            let wear = (durability * total as i32) / (unbreaking + 1);
            let max = t.get_type().get_max_durability() as i32;
            let now = t.get_durability() as i32 + wear;
            if now >= max {
                player.get_inventory().set_item_in_main_hand(&ItemStack::new(Material::of("AIR"), 1));
                player.send_message(&crate::locale::get("Woodcutting.Skills.TreeFeller.Splinter", &[]));
            } else {
                t.set_durability(now as i16);
                player.get_inventory().set_item_in_main_hand(&t);
            }
        }
    });
}

/// A left click on a block with a readied tool: the skill's ability starts if the block is one it works on.
pub fn on_left_click(player: &Player, block: &Block, held_id: &str) {
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let prepared = mmo::sessions().get(&uuid).and_then(|s| s.prepared.map(|p| p.0));
    let Some(tool) = prepared else { return };
    if Tool::of(held_id) != Some(tool) {
        return;
    }
    let s = settings::get();
    let m = mat(&block.get_block_data()).to_owned();
    let skill = match tool {
        Tool::Pickaxe if s.xp[Skill::Mining.idx()].contains_key(m.as_str()) => Skill::Mining,
        Tool::Axe if s.xp[Skill::Woodcutting.idx()].contains_key(m.as_str()) => Skill::Woodcutting,
        Tool::Shovel if s.xp[Skill::Excavation.idx()].contains_key(m.as_str()) => Skill::Excavation,
        Tool::Hoe if s.xp[Skill::Herbalism.idx()].contains_key(m.as_str()) => Skill::Herbalism,
        Tool::Fists => Skill::Unarmed,
        _ => return,
    };
    mmo::try_activate(&uuid, skill, tool);
}

/// Green Thumb on blocks: seeds used on cobblestone, stone bricks, dirt... turn them mossy/grassy.
pub fn green_thumb_block(player: &Player, block: &Block, held_id: &str) -> bool {
    if mat(held_id) != "wheat_seeds" {
        return false;
    }
    let m = mat(&block.get_block_data()).to_owned();
    let to = match m.as_str() {
        "cobblestone" => "mossy_cobblestone",
        "cobblestone_wall" => "mossy_cobblestone_wall",
        "stone_bricks" => "mossy_stone_bricks",
        "dirt" => "grass_block",
        "dirt_path" => "grass_block",
        _ => return false,
    };
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let s = settings::get();
    let Some(v) = view(&uuid, Skill::Herbalism) else { return false };
    if !v.perm || !mmo::sessions().get(&uuid).is_some_and(|s| s.has(mmo::perm::GREEN_THUMB)) {
        return false;
    }
    let seed = ItemStack::new(Material::of("wheat_seeds"), 1);
    player.get_inventory().remove_item(&seed);
    if v.active[Ability::GreenTerra.idx()] || roll(s.chance("Skills.Herbalism.GreenThumb", v.level) * v.lucky) {
        block.set_type(&Material::of(to));
        player.send_action_bar(&horizon_bukkit::adventure::legacy(&crate::locale::get("Herbalism.Ability.GTh", &[])));
    } else {
        player.send_action_bar(&horizon_bukkit::adventure::legacy(&crate::locale::get("Herbalism.Ability.GTh.Fail", &[])));
    }
    true
}

// ---- Blast Mining ----

/// TNT a miner lit with the detonator: (world, x, y, z) -> (uuid, when).
static BLASTS: Mutex<Vec<(String, i32, i32, i32, String, i64)>> = Mutex::new(Vec::new());

/// Sneaking right-click on TNT with the detonator: Blast Mining (when unlocked and off cooldown).
pub fn on_detonate(player: &Player, block: &Block) -> bool {
    let s = settings::get();
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let mut out = Out::default();
    let ok = {
        let mut ss = mmo::sessions();
        let mut st = profile::store();
        let Some(sess) = ss.get_mut(&uuid) else { return false };
        let Some(p) = st.get(&uuid) else { return false };
        if !sess.skill_perm(Skill::Mining) || !sess.has(mmo::perm::ABILITY0 + Ability::BlastMining.idx()) {
            return false;
        }
        let level = p.level[Skill::Mining.idx()];
        if level < s.unlock_level("Mining.BlastMining") {
            out.notify(sess, "RequirementsNotMet", crate::locale::get("Skills.AbilityGateRequirementFail", &[&(s.unlock_level("Mining.BlastMining") - level).to_string(), &mmo::skill_name(Skill::Mining)]));
            false
        } else {
            let left = mmo::cooldown_left(sess, p, Ability::BlastMining);
            if left > 0 {
                out.notify(sess, "AbilityCoolDown", crate::locale::get("Skills.TooTired", &[&left.to_string()]));
                false
            } else {
                if let Some(p) = st.get_mut(&uuid) {
                    p.dats[profile::dats_slot(Ability::BlastMining)] = profile::now_secs();
                }
                sess.informed[Ability::BlastMining.idx()] = true;
                out.notify(sess, "SuperAbilityInteraction", crate::locale::get("Mining.Blast.Boom", &[]));
                true
            }
        }
    };
    out.run();
    if ok {
        BLASTS.lock().unwrap_or_else(|e| e.into_inner()).push((block.get_world().get_name().to_owned(), block.get_x(), block.get_y(), block.get_z(), uuid, profile::now_ms()));
    }
    ok
}

fn is_ore(m: &str) -> bool {
    m.ends_with("_ore") || m == "ancient_debris" || m == "raw_iron_block" || m == "raw_copper_block" || m == "raw_gold_block"
}

/// A TNT explosion: if a miner set it off with Blast Mining, the ores give XP and bonus drops by rank.
pub fn on_explode(at: &horizon_bukkit::Location, blocks: &[Block]) {
    let now = profile::now_ms();
    let world = at.get_world().get_name().to_owned();
    let who = {
        let mut b = BLASTS.lock().unwrap_or_else(|e| e.into_inner());
        b.retain(|e| now - e.5 < 15_000);
        let pos = b.iter().position(|e| e.0 == world && ((e.1 as f64 - at.get_x()).powi(2) + (e.2 as f64 - at.get_y()).powi(2) + (e.3 as f64 - at.get_z()).powi(2)) < 9.0);
        pos.map(|i| b.remove(i).4)
    };
    let Some(uuid) = who else { return };
    let s = settings::get();
    let level = profile::store().get(&uuid).map(|p| p.level[Skill::Mining.idx()]).unwrap_or(0);
    let rank = s.rank("Mining.BlastMining", level);
    let bonus = s.by_rank("Skills.Mining.BlastMining.OreBonus", rank, 35.0);
    let mut xp = 0.0;
    let mut extra: Vec<(Block, u32)> = Vec::new();
    for b in blocks {
        let m = mat(&b.get_block_data()).to_owned();
        if placed::is_placed(&world, b.get_x(), b.get_y(), b.get_z()) {
            placed::set(&world, b.get_x(), b.get_y(), b.get_z(), false);
            continue;
        }
        if is_ore(&m) {
            xp += s.xp[Skill::Mining.idx()].get(m.as_str()).copied().unwrap_or(0.0);
            if roll(bonus) {
                extra.push((b.clone(), 1));
            }
        }
    }
    if xp > 0.0 {
        mmo::award(&uuid, Skill::Mining, xp as f32, Reason::Gathering);
    }
    for (b, n) in extra {
        bonus_drops(&b, None, n);
    }
}
