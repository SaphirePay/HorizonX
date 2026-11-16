//! Fishing (XP, Treasure Hunter, Shake, the over-fishing guard, vanilla XP boost), Smelting, Alchemy, Taming
//! (taming XP), Repair and Salvage on their anvils.

use crate::blocks::{mat, roll};
use crate::mmo::{self, Reason};
use crate::profile;
use crate::settings::{self, Settings};
use crate::skills::Skill;
use horizon_bukkit::fam_ev_player::PlayerFishEvent;
use horizon_bukkit::{Block, ItemStack, Material, Player};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

fn level(uuid: &str, skill: Skill) -> Option<(i32, bool, f64, bool)> {
    let ss = mmo::sessions();
    let sess = ss.get(uuid)?;
    let st = profile::store();
    let p = st.get(uuid)?;
    Some((p.level(skill), sess.skill_perm(skill), sess.lucky(skill), sess.creative))
}

// ---- Fishing ----

const RARITIES: [&str; 6] = ["MYTHIC", "LEGENDARY", "EPIC", "RARE", "UNCOMMON", "COMMON"];

pub fn on_fish(e: &mut PlayerFishEvent) {
    let Some(player) = e.get_player() else { return };
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let state = e.get_state().to_owned();
    let s = settings::get();
    let Some((lvl, perm, lucky, creative)) = level(&uuid, Skill::Fishing) else { return };
    if !perm || creative {
        return;
    }
    match state.as_str() {
        "CAUGHT_FISH" => {
            // Over-fishing the same spot: no XP and no treasure.
            if s.fishing_exploit
                && let Some(hook) = e.get_hook()
                && let Some(at) = hook.get_location()
            {
                let mut ss = mmo::sessions();
                if let Some(sess) = ss.get_mut(&uuid) {
                    let here = (at.get_x(), at.get_y(), at.get_z());
                    let same = sess.last_fish.is_some_and(|(x, y, z, _)| ((x - here.0).powi(2) + (y - here.1).powi(2) + (z - here.2).powi(2)).sqrt() <= s.fishing_range);
                    let count = if same { sess.last_fish.map(|f| f.3 + 1).unwrap_or(1) } else { 1 };
                    sess.last_fish = Some((here.0, here.1, here.2, count));
                    if count > s.overfish {
                        drop(ss);
                        player.send_message(&crate::locale::get("Fishing.Scared", &[]));
                        return;
                    }
                }
            }
            let Some(caught) = e.get_caught() else { return };
            let item_entity = caught.cast::<horizon_bukkit::entities::Item>();
            let stack = item_entity.get_item_stack();
            let id = stack.as_ref().map(|i| mat(i.get_type().key()).to_owned()).unwrap_or_default();
            let mut xp = s.fish.get(id.as_str()).copied().unwrap_or(0.0);
            // Treasure Hunter: a rarity rolled from the tier's rates, a treasure of it in place of the catch.
            let tier = s.rank("Fishing.TreasureHunter", lvl) as i32;
            if tier > 0 && !s.fishing_treasures.is_empty() {
                let mut pick = None;
                for rarity in RARITIES {
                    let chance = s.fishing_rates.get(&(tier, rarity.to_owned())).copied().unwrap_or(0.0) * lucky;
                    if roll(chance) {
                        let pool: Vec<&crate::settings::Treasure> = s.fishing_treasures.iter().filter(|t| t.rarity == rarity).collect();
                        if !pool.is_empty() {
                            pick = Some(pool[(crate::blocks::rand_unit() * pool.len() as f64) as usize % pool.len()].clone());
                        }
                        break;
                    }
                }
                if let Some(t) = pick {
                    xp += t.xp as f64;
                    item_entity.set_item_stack(&ItemStack::new(Material::of(&t.item), t.amount.max(1)));
                }
            }
            if xp > 0.0 {
                mmo::award(&uuid, Skill::Fishing, xp as f32, Reason::Gathering);
            }
            let boost = s.by_rank("Skills.Fishing.VanillaXPMultiplier", s.rank("Fishing.InnerPeace", lvl).max(s.rank("Fishing.TreasureHunter", lvl)), 1.0).max(1.0);
            let exp = e.get_exp_to_drop();
            if boost > 1.0 && exp > 0 {
                e.set_exp_to_drop((exp as f64 * boost) as i32);
            }
        }
        "CAUGHT_ENTITY" => {
            let Some(target) = e.get_caught() else { return };
            let rank = s.rank("Fishing.Shake", lvl);
            if rank == 0 {
                return;
            }
            let kind = crate::combat::kind_of(&target);
            let k = kind.trim_start_matches("minecraft:").to_owned();
            let Some(drops) = s.shake.get(&k).cloned() else { return };
            let chance = s.by_rank("Skills.Fishing.ShakeChance", rank, 15.0) * lucky;
            if !roll(chance) {
                return;
            }
            let total: f64 = drops.iter().filter(|d| lvl >= d.level).map(|d| d.chance).sum();
            if total <= 0.0 {
                return;
            }
            let mut r = crate::blocks::rand_unit() * total;
            let mut chosen = None;
            for d in drops.iter().filter(|d| lvl >= d.level) {
                if r < d.chance {
                    chosen = Some(d.clone());
                    break;
                }
                r -= d.chance;
            }
            let Some(d) = chosen else { return };
            if let Some(at) = target.get_location() {
                let item = if d.item == "player_head" || d.item == "skull" { "player_head".to_owned() } else { d.item.clone() };
                at.get_world().drop_item_naturally(&at, &ItemStack::new(Material::of(&item), d.amount.max(1)));
            }
            let hp = target.get_health();
            target.damage((hp / 4.0).max(1.0).min(hp - 1.0).max(0.0));
            mmo::award(&uuid, Skill::Fishing, s.shake_xp as f32, Reason::Gathering);
        }
        _ => {}
    }
}

// ---- Smelting ----

/// A furnace result -> the ore it most likely came from (experience.yml lists sources).
fn smelt_source(result: &str) -> &str {
    match result {
        "iron_ingot" => "raw_iron",
        "gold_ingot" => "raw_gold",
        "copper_ingot" => "raw_copper",
        "netherite_scrap" => "ancient_debris",
        "diamond" => "diamond_ore",
        "emerald" => "emerald_ore",
        "lapis_lazuli" => "lapis_ore",
        "redstone" => "redstone_ore",
        "quartz" => "nether_quartz_ore",
        "coal" => "coal_ore",
        "deepslate" => "cobbled_deepslate",
        "copper_nugget" => "copper_nugget",
        other => other,
    }
}

/// Taking smelted items out of a furnace: Smelting XP per item, Second Smelt, the vanilla XP boost.
pub fn on_extract(player: &Player, result: &str, amount: i32) {
    let s = settings::get();
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let Some((lvl, _, lucky, creative)) = level(&uuid, Skill::Smelting) else { return };
    if creative {
        return;
    }
    let src = smelt_source(result);
    let per = s.xp[Skill::Smelting.idx()].get(src).copied().unwrap_or(0.0);
    if per > 0.0 && amount > 0 {
        mmo::award(&uuid, Skill::Smelting, (per * amount as f64) as f32, Reason::Passive);
        let second = s.rank("Smelting.SecondSmelt", lvl) > 0;
        if second {
            let extra = (0..amount).filter(|_| roll(s.chance("Skills.Smelting.SecondSmelt", lvl) * lucky)).count() as i32;
            if extra > 0 {
                let left = player.get_inventory().add_item(&ItemStack::new(Material::of(result), extra));
                if let Some(left) = left {
                    let at = player.get_location();
                    at.get_world().drop_item_naturally(&at, &left);
                }
            }
        }
        let boost = s.by_rank("Skills.Smelting.VanillaXPMultiplier", s.rank("Smelting.UnderstandingTheArt", lvl), 1.0);
        if boost > 1.0 {
            player.give_exp(((boost - 1.0) * amount as f64 * 0.7).round() as i32);
        }
    }
}

// ---- Alchemy ----

/// Brewing stand -> the player who last used it (BrewEvent names no player).
static STANDS: LazyLock<Mutex<HashMap<(String, i32, i32, i32), String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn used_stand(player: &Player, b: &Block) {
    let mut m = STANDS.lock().unwrap_or_else(|e| e.into_inner());
    if m.len() > 20_000 {
        m.clear();
    }
    m.insert((b.get_world().get_name().to_owned(), b.get_x(), b.get_y(), b.get_z()), player.get_unique_id().to_ascii_lowercase());
}

pub fn on_brew(e: &horizon_bukkit::fam_ev_player::BrewEvent) {
    let Some(b) = e.get_block() else { return };
    let owner = STANDS.lock().unwrap_or_else(|e| e.into_inner()).get(&(b.get_world().get_name().to_owned(), b.get_x(), b.get_y(), b.get_z())).cloned();
    let Some(uuid) = owner else { return };
    if !mmo::sessions().contains_key(&uuid) {
        return;
    }
    let s = settings::get();
    let contents = e.get_contents();
    let ingredient = contents.get_item(3).map(|i| mat(i.get_type().key()).to_owned()).unwrap_or_default();
    let potions = (0..3).filter(|i| contents.get_item(*i).is_some_and(|it| !it.get_type().is_air())).count();
    let stage = match ingredient.as_str() {
        "nether_wart" => 0,
        "redstone" | "glowstone_dust" => 2,
        "gunpowder" | "dragon_breath" => 3,
        "fermented_spider_eye" => 4,
        _ => 1,
    };
    let xp = s.alchemy_stage[stage] * potions as f64;
    if xp > 0.0 {
        mmo::award(&uuid, Skill::Alchemy, xp as f32, Reason::Passive);
    }
}

// ---- Taming ----

pub fn on_tame(owner: &Player, kind: &str) {
    let s = settings::get();
    let k = kind.trim_start_matches("minecraft:");
    let xp = s.taming.get(k).copied().unwrap_or(0.0);
    if xp > 0.0 {
        mmo::award(&owner.get_unique_id().to_ascii_lowercase(), Skill::Taming, xp as f32, Reason::Pve);
    }
}

// ---- Repair and Salvage ----

fn repair_tier_mult(s: &Settings, item: &str) -> f64 {
    let tier = crate::skills::tier(item).to_ascii_lowercase();
    s.repair_tier.get(&tier).copied().unwrap_or(1.0)
}

/// A right click on a repair or salvage anvil with an item in hand. True when it was one (the click is used).
pub fn on_anvil(player: &Player, block_mat: &str, item: Option<ItemStack>) -> bool {
    let s = settings::get();
    let repair = block_mat == s.repair_anvil;
    let salvage = block_mat == s.salvage_anvil;
    if !repair && !salvage {
        return false;
    }
    let Some(mut item) = item.filter(|i| !i.get_type().is_air()) else { return false };
    let id = mat(item.get_type().key()).to_owned();
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let skill = if repair { Skill::Repair } else { Skill::Salvage };
    let Some((lvl, perm, lucky, _)) = level(&uuid, skill) else { return false };
    if !perm {
        return false;
    }
    let table = if repair { &s.repairables } else { &s.salvageables };
    let Some(spec) = table.get(&id).cloned() else { return false };
    if item.get_amount() > 1 {
        player.send_message(&crate::locale::get("Repair.Skills.StackedItems", &[]));
        return true;
    }
    if lvl < spec.min_level {
        let key = if repair { "Repair.Skills.Adept" } else { "Salvage.Skills.Adept.Level" };
        player.send_message(&crate::locale::get(key, &[&spec.min_level.to_string(), &id]));
        return true;
    }
    // Confirm with a second click within 3 s.
    let confirm = if repair { s.repair_confirm } else { s.salvage_confirm };
    if confirm {
        let mut ss = mmo::sessions();
        let Some(sess) = ss.get_mut(&uuid) else { return true };
        let slot = if repair { &mut sess.repair_confirm } else { &mut sess.salvage_confirm };
        let now = profile::now_ms();
        if !slot.as_ref().is_some_and(|(i, at)| *i == id && now - *at < 3000) {
            *slot = Some((id.clone(), now));
            drop(ss);
            let name = crate::locale::get(if repair { "Repair.Pretty.Name" } else { "Salvage.Pretty.Name" }, &[]);
            player.send_message(&crate::locale::get("Skills.ConfirmOrCancel", &[&name]));
            return true;
        }
        *slot = None;
    }
    let max = item.get_type().get_max_durability() as i32;
    let damage = item.get_durability() as i32;
    if repair {
        if damage <= 0 || max <= 0 {
            player.send_message(&crate::locale::get("Repair.Skills.FullDurability", &[]));
            return true;
        }
        let material = ItemStack::new(Material::of(&spec.material), 1);
        if !player.get_inventory().contains_at_least(&material, 1) {
            player.send_message(&crate::locale::get("Skills.NeedMore", &[&spec.material]));
            return true;
        }
        let qty = crate::settings::default_quantity(&id).max(1) as i32;
        let mut amount = (max / qty) as f64;
        if s.rank("Repair.RepairMastery", lvl) > 0 {
            let max_bonus = s.adv("Skills.Repair.RepairMastery.MaxBonusPercentage", 200.0);
            let cap = s.max_bonus("Skills.Repair.RepairMastery");
            let bonus = (max_bonus / cap * (lvl as f64 / 100.0)).min(max_bonus / 100.0);
            amount += amount * bonus;
        }
        if s.rank("Repair.SuperRepair", lvl) > 0 && roll(s.chance("Skills.Repair.SuperRepair", lvl) * lucky) {
            amount *= 2.0;
            player.send_message(&crate::locale::get("Repair.Skills.FeltEasy", &[]));
        }
        let new_damage = (damage - amount as i32).max(0);
        player.get_inventory().remove_item(&material);
        // Arcane Forging: keep, downgrade or lose the enchantments.
        let enchants = item.get_enchants();
        if !enchants.is_empty() {
            let rank = s.rank("Repair.ArcaneForging", lvl);
            let keep = s.by_rank("Skills.Repair.ArcaneForging.Keep_Enchants_Chance", rank, 0.0);
            let may_lose = s.adv_bool.get("Skills.Repair.ArcaneForging.May_Lose_Enchants").copied().unwrap_or(true);
            if may_lose && (rank == 0 || !roll(keep * lucky)) {
                item.remove_enchantments();
                player.send_message(&crate::locale::get("Repair.Arcane.Lost", &[]));
            } else {
                let downgrades = s.adv_bool.get("Skills.Repair.ArcaneForging.Downgrades_Enabled").copied().unwrap_or(true);
                let chance = s.by_rank("Skills.Repair.ArcaneForging.Downgrades_Chance", rank, 0.0);
                let mut downgraded = false;
                if downgrades {
                    for (ench, lv) in enchants {
                        if lv > 1 && roll(chance) {
                            item.remove_enchantment(&ench);
                            item.add_enchant(&ench, lv - 1);
                            downgraded = true;
                        }
                    }
                }
                player.send_message(&crate::locale::get(if downgraded { "Repair.Arcane.Downgrade" } else { "Repair.Arcane.Perfect" }, &[]));
            }
        }
        item.set_durability(new_damage as i16);
        player.get_inventory().set_item_in_main_hand(&item);
        let pct = (damage - new_damage) as f64 / max as f64;
        let xp = pct * spec.xp_mult * s.repair_base * repair_tier_mult(&s, &id);
        mmo::award(&uuid, Skill::Repair, xp as f32, Reason::Pve);
        let mut out = mmo::Out::default();
        out.sound(player, "ANVIL");
        out.run();
    } else {
        if max > 0 && damage > 0 && s.rank("Salvage.ScrapCollector", lvl) == 0 {
            player.send_message(&crate::locale::get("Salvage.Skills.Adept.Damaged", &[]));
            return true;
        }
        let max_qty = spec.quantity.max(1);
        let left = if max > 0 { 1.0 - damage as f64 / max as f64 } else { 1.0 };
        let mut amount = (max_qty as f64 * left).floor() as i32;
        let rank = s.rank("Salvage.ScrapCollector", lvl) as i32;
        if rank > 0 {
            amount = amount.min(rank);
        }
        if amount <= 0 {
            player.send_message(&crate::locale::get("Salvage.Skills.TooDamaged", &[]));
            return true;
        }
        player.get_inventory().set_item_in_main_hand(&ItemStack::new(Material::of("AIR"), 1));
        let at = player.get_location();
        at.get_world().drop_item_naturally(&at, &ItemStack::new(Material::of(&spec.material), amount));
        player.send_message(&crate::locale::get("Salvage.Skills.Lottery.Normal", &[&amount.to_string(), &id]));
        let mut out = mmo::Out::default();
        out.sound(player, "ITEM_BREAK");
        out.run();
    }
    true
}
