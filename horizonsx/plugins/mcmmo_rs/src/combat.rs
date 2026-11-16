//! Combat: Swords, Axes, Unarmed, Archery, Crossbows, Tridents, Maces, Spears and Taming on hits (damage
//! bonuses, procs, super abilities, combat XP as mcMMO's processCombatXP), Acrobatics on falls and dodges, the
//! Rupture bleed, arrow retrieval, spawn-reason XP multipliers and the hardcore death penalty / vampirism.

use crate::blocks::{rand_unit, roll};
use crate::mmo::{self, Out, Reason};
use crate::profile;
use crate::settings::{self, Settings};
use crate::skills::{Ability, Skill, Tool};
use horizon_bukkit::fam_ev_entity::{Actor, EntityDamageByEntityEvent, EntityDamageEvent};
use horizon_bukkit::entity::Entity;
use horizon_bukkit::{ItemStack, Material, Player, PotionEffect};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Entity id -> its type, asked once per entity (a type read is a whole-state query).
static KINDS: LazyLock<Mutex<HashMap<u64, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// Entity id -> the XP multiplier its spawn gave (spawner, egg, portal, bred, tamed).
static SPAWN_MULT: LazyLock<Mutex<HashMap<u64, f64>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// Projectile id -> (shooter uuid, crossbow?, launch place).
static SHOTS: LazyLock<Mutex<HashMap<u64, (String, bool, (f64, f64, f64))>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// Mob id -> (archer uuid, arrows stuck in it) for Arrow Retrieval.
static ARROWS: LazyLock<Mutex<HashMap<u64, (String, u32)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Player victim -> (attacking player, when): the killer for the hardcore rules.
static LAST_HIT: LazyLock<Mutex<HashMap<String, (String, i64)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn note_hit(victim: &Actor, attacker: &str) {
    if let Some(vu) = uuid_of(victim) {
        LAST_HIT.lock().unwrap_or_else(|e| e.into_inner()).insert(vu, (attacker.to_owned(), profile::now_ms()));
    }
}

/// The player who hit this one in the last 10 s, if online.
pub fn last_attacker(victim: &str) -> Option<Player> {
    let (who, at) = LAST_HIT.lock().unwrap_or_else(|e| e.into_inner()).remove(victim)?;
    if profile::now_ms() - at > 10_000 {
        return None;
    }
    mmo::sessions().get(&who).map(|s| s.player.clone())
}

struct Bleed {
    target: Actor,
    attacker: String,
    ticks: u32,
    damage: f64,
}
static BLEEDS: Mutex<Vec<Bleed>> = Mutex::new(Vec::new());

pub fn kind_of(e: &Entity) -> String {
    let id = e.get_entity_id();
    if let Some(k) = KINDS.lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
        return k.clone();
    }
    let k = e.get_type();
    let mut m = KINDS.lock().unwrap_or_else(|e| e.into_inner());
    if m.len() > 100_000 {
        m.clear();
    }
    m.insert(id, k.clone());
    k
}

pub fn remember_kind(id: u64, kind: &str) {
    KINDS.lock().unwrap_or_else(|e| e.into_inner()).insert(id, kind.to_owned());
}

pub fn forget(id: u64) {
    KINDS.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    SPAWN_MULT.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
}

/// A spawn: mobs from spawners, eggs, portals and breeding give the configured share of XP.
pub fn on_spawn(id: u64, reason: &str) {
    let s = settings::get();
    let mult = match reason {
        "SPAWNER" | "TRIAL_SPAWNER" => s.spawner_mult,
        "SPAWNER_EGG" | "EGG" | "DISPENSE_EGG" => s.egg_mult,
        "NETHER_PORTAL" => s.portal_mult,
        "BREEDING" => s.bred_mult,
        _ => return,
    };
    if (mult - 1.0).abs() < f64::EPSILON {
        return;
    }
    let mut m = SPAWN_MULT.lock().unwrap_or_else(|e| e.into_inner());
    if m.len() > 200_000 {
        m.clear();
    }
    m.insert(id, mult);
}

pub fn on_tame(id: u64) {
    let s = settings::get();
    SPAWN_MULT.lock().unwrap_or_else(|e| e.into_inner()).insert(id, s.tamed_mult);
}

pub fn on_shoot(projectile: u64, shooter: &Player, crossbow: bool) {
    let at = shooter.get_location();
    let mut m = SHOTS.lock().unwrap_or_else(|e| e.into_inner());
    if m.len() > 50_000 {
        m.clear();
    }
    m.insert(projectile, (shooter.get_unique_id().to_ascii_lowercase(), crossbow, (at.get_x(), at.get_y(), at.get_z())));
}

fn uuid_of(a: &Actor) -> Option<String> {
    a.as_player().map(|p| p.get_unique_id().to_ascii_lowercase())
}

fn health(a: &Actor) -> f64 {
    match a {
        Actor::Player(p) => p.get_health(),
        Actor::Entity(e) => e.get_health(),
    }
}

fn hurt(a: &Actor, amount: f64) {
    match a {
        Actor::Player(p) => p.damage(amount, "minecraft:generic"),
        Actor::Entity(e) => e.damage(amount),
    }
}

fn effect(a: &Actor, id: &str, ticks: i32, amp: i32) {
    let fx = PotionEffect::new(id, ticks, amp, false, true);
    match a {
        Actor::Player(p) => {
            p.give_potion_effect(&fx);
        }
        Actor::Entity(e) => e.add_potion_effect(&fx),
    }
}

/// The player that owns a tamed animal (its saved `Owner`).
fn owner_of(e: &Entity) -> Option<String> {
    let v = e.data_at("Owner");
    let ints: Vec<i64> = v.as_array()?.iter().filter_map(|x| x.as_i64()).collect();
    if ints.len() != 4 {
        return v.as_str().map(str::to_ascii_lowercase);
    }
    let hex: String = ints.iter().map(|i| format!("{:08x}", *i as u32)).collect();
    Some(format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32]))
}

fn skill_of_tool(tool: Option<Tool>) -> Option<Skill> {
    Some(match tool? {
        Tool::Sword => Skill::Swords,
        Tool::Axe => Skill::Axes,
        Tool::Fists => Skill::Unarmed,
        Tool::Trident => Skill::Tridents,
        Tool::Mace => Skill::Maces,
        Tool::Spear => Skill::Spears,
        _ => return None,
    })
}

struct Me {
    level: i32,
    perm: bool,
    lucky: f64,
    active: [bool; 8],
    creative: bool,
}

fn me(uuid: &str, skill: Skill) -> Option<Me> {
    let ss = mmo::sessions();
    let sess = ss.get(uuid)?;
    let st = profile::store();
    let p = st.get(uuid)?;
    Some(Me { level: p.level(skill), perm: sess.skill_perm(skill), lucky: sess.lucky(skill), active: crate::skills::ABILITIES.map(|a| sess.ability_active(a)), creative: sess.creative })
}

fn rank(s: &Settings, key: &str, level: i32) -> usize {
    s.rank(key, level)
}

/// Combat XP for `dealt` damage on `victim` (processCombatXP + AwardCombatXpTask).
fn combat_xp(s: &Settings, uuid: &str, victim: &Actor, victim_kind: &str, skill: Skill, dealt: f64, multiplier: f64) {
    let base = match victim {
        Actor::Player(p) => {
            let vu = p.get_unique_id().to_ascii_lowercase();
            if vu == uuid || !s.pvp_rewards || crate::party::same_party(uuid, &vu) {
                return;
            }
            let respawned = mmo::sessions().get(&vu).map(|x| x.respawn_at).unwrap_or(0);
            if profile::now_ms() - respawned < 5000 {
                return;
            }
            20.0 * s.pvp_mult
        }
        Actor::Entity(e) => {
            let mut b = s.combat_mult(victim_kind);
            if let Some(m) = SPAWN_MULT.lock().unwrap_or_else(|e| e.into_inner()).get(&e.get_entity_id()) {
                b *= m;
            }
            b * 10.0
        }
    };
    let mut dmg = dealt;
    if let Some(cap) = s.combat_ceiling {
        dmg = dmg.min(cap);
    }
    let xp = (dmg * base * multiplier) as i64;
    if xp > 0 {
        mmo::award(uuid, skill, xp as f32, if matches!(victim, Actor::Player(_)) { Reason::Pvp } else { Reason::Pve });
    }
}

fn nearby(victim: &Actor, attacker: &str, radius: f64) -> Vec<Actor> {
    let Some(at) = victim.get_location() else { return Vec::new() };
    let victim_id = match victim {
        Actor::Entity(e) => Some(e.get_entity_id()),
        Actor::Player(_) => None,
    };
    let victim_uuid = uuid_of(victim);
    let online: HashMap<String, Player> = horizon_bukkit::get_online_players().into_iter().map(|p| (p.get_unique_id().to_ascii_lowercase(), p)).collect();
    let mut out = Vec::new();
    for e in at.get_nearby_entities(radius, radius, radius) {
        let u = e.get_unique_id().to_ascii_lowercase();
        if Some(e.get_entity_id()) == victim_id || u == attacker || Some(&u) == victim_uuid.as_ref() {
            continue;
        }
        if let Some(p) = online.get(&u) {
            if crate::party::same_party(attacker, &u) {
                continue;
            }
            out.push(Actor::Player(p.clone()));
            continue;
        }
        let k = kind_of(&e);
        if matches!(k.trim_start_matches("minecraft:"), "item" | "experience_orb" | "arrow" | "armor_stand" | "item_frame" | "painting" | "falling_block" | "tnt" | "trident" | "spectral_arrow" | "marker" | "text_display" | "item_display" | "block_display" | "interaction") {
            continue;
        }
        out.push(Actor::Entity(e));
    }
    out
}

fn start_bleed(s: &Settings, target: &Actor, attacker: &str, rupture_rank: usize) {
    let vs_player = matches!(target, Actor::Player(_));
    let section = if vs_player { "Skills.Swords.Rupture.Rupture_Mechanics.Tick_Interval_Damage.Against_Players" } else { "Skills.Swords.Rupture.Rupture_Mechanics.Tick_Interval_Damage.Against_Mobs" };
    let damage = s.by_rank(section, rupture_rank, 0.5);
    let secs = s.adv(if vs_player { "Skills.Swords.Rupture.Rupture_Mechanics.Duration_In_Seconds.Against_Players" } else { "Skills.Swords.Rupture.Rupture_Mechanics.Duration_In_Seconds.Against_Mobs" }, 5.0);
    let mut b = BLEEDS.lock().unwrap_or_else(|e| e.into_inner());
    if b.iter().any(|x| x.target == *target) {
        return;
    }
    b.push(Bleed { target: target.clone(), attacker: attacker.to_owned(), ticks: (secs * 2.0) as u32, damage });
    drop(b);
    if let Actor::Player(p) = target {
        p.send_message(&crate::locale::get("Swords.Combat.Bleeding.Started", &[]));
    }
}

/// Rupture's damage, twice a second; it never kills.
pub fn tick_bleeds() {
    let work: Vec<(Actor, f64, bool)> = {
        let mut b = BLEEDS.lock().unwrap_or_else(|e| e.into_inner());
        if b.is_empty() {
            return;
        }
        let mut out = Vec::new();
        b.retain_mut(|x| {
            x.ticks = x.ticks.saturating_sub(1);
            out.push((x.target.clone(), x.damage, x.ticks == 0));
            let _ = &x.attacker;
            x.ticks > 0
        });
        out
    };
    for (target, damage, ended) in work {
        let hp = health(&target);
        if hp <= 0.0 {
            continue;
        }
        if hp - damage > 0.5 {
            match &target {
                Actor::Player(p) => p.set_health(hp - damage),
                Actor::Entity(e) => {
                    e.set_health(hp - damage);
                }
            }
        }
        if ended && let Actor::Player(p) = &target {
            p.send_message(&crate::locale::get("Swords.Combat.Bleeding.Stopped", &[]));
        }
    }
}

/// The main hit handler (EntityDamageByEntityEvent).
pub fn on_hit(e: &mut EntityDamageByEntityEvent) {
    let Some(victim) = e.get_entity() else { return };
    let Some(damager) = e.get_damager() else { return };
    let s = settings::get();
    let cause = e.get_cause();
    // ---- The defender's side: god mode, dodge, counter attack, arrow deflect ----
    if let Some(vu) = uuid_of(&victim) {
        defend(&s, e, &victim, &vu, &damager, cause);
        if e.is_cancelled() {
            return;
        }
    }
    let victim_kind = match &victim {
        Actor::Player(_) => "minecraft:player".to_owned(),
        Actor::Entity(en) => kind_of(en),
    };
    if matches!(victim_kind.trim_start_matches("minecraft:"), "armor_stand" | "mannequin" | "item_frame" | "painting") {
        return;
    }
    match &damager {
        Actor::Player(attacker) => melee(&s, e, attacker, &victim, &victim_kind, cause),
        Actor::Entity(proj) => {
            let pk = kind_of(proj);
            let pk = pk.trim_start_matches("minecraft:");
            match pk {
                "arrow" | "spectral_arrow" | "trident" => ranged(&s, e, proj, pk, &victim, &victim_kind),
                "wolf" => wolf(&s, e, proj, &victim, &victim_kind),
                _ => {}
            }
        }
    }
}

fn defend(s: &Settings, e: &mut EntityDamageByEntityEvent, victim: &Actor, vu: &str, damager: &Actor, cause: &str) {
    let (god, level_acro, level_swords, level_unarmed, lucky, acro_perm, swords_perm, unarmed_perm) = {
        let ss = mmo::sessions();
        let Some(sess) = ss.get(vu) else { return };
        let st = profile::store();
        let Some(p) = st.get(vu) else { return };
        (sess.god, p.level[Skill::Acrobatics.idx()], p.level[Skill::Swords.idx()], p.level[Skill::Unarmed.idx()], sess.lucky(Skill::Acrobatics), sess.skill_perm(Skill::Acrobatics), sess.skill_perm(Skill::Swords), sess.skill_perm(Skill::Unarmed))
    };
    if god {
        e.set_cancelled(true);
        return;
    }
    if let Some(sess) = mmo::sessions().get_mut(vu) {
        sess.recently_hurt = profile::now_ms();
    }
    let Some(Actor::Player(vp)) = Some(victim.clone()) else { return };
    let from_player = matches!(damager, Actor::Player(_));
    let pvp = from_player;
    // Arrow Deflect: bare hands vs arrows.
    if let Actor::Entity(proj) = damager {
        let pk = kind_of(proj);
        if pk.ends_with("arrow") && unarmed_perm && rank(s, "Unarmed.ArrowDeflect", level_unarmed) > 0 {
            let empty = vp.get_inventory().get_item_in_main_hand().is_none_or(|i| i.get_type().is_air());
            if empty && roll(s.chance("Skills.Unarmed.ArrowDeflect", level_unarmed) * lucky) {
                e.set_cancelled(true);
                vp.send_message(&crate::locale::get("Combat.ArrowDeflect", &[]));
                return;
            }
        }
    }
    // Dodge: halves the damage.
    if acro_perm && cause != "FALL" && ((pvp && s.pvp[Skill::Acrobatics.idx()]) || (!pvp && s.pve[Skill::Acrobatics.idx()])) && rank(s, "Acrobatics.Dodge", level_acro) > 0 {
        let dmg = e.get_damage();
        if roll(s.chance("Skills.Acrobatics.Dodge", level_acro) * lucky) {
            let modifier = s.adv("Skills.Acrobatics.Dodge.DamageModifier", 2.0);
            e.set_damage(dmg / modifier);
            vp.send_action_bar(&horizon_bukkit::adventure::legacy(&crate::locale::get("Acrobatics.Combat.Proc", &[])));
            let xp = dmg / modifier * s.dodge_xp;
            mmo::award(vu, Skill::Acrobatics, xp as f32, Reason::Pve);
        }
    }
    // Counter Attack: a sword-holder struck in melee strikes back.
    if swords_perm && rank(s, "Swords.CounterAttack", level_swords) > 0 && (matches!(damager, Actor::Player(_)) || matches!(damager, Actor::Entity(_)) && cause == "ENTITY_ATTACK") {
        let held = vp.get_inventory().get_item_in_main_hand().map(|i| i.get_type().key().to_owned()).unwrap_or_default();
        if Tool::of(&held) == Some(Tool::Sword) && roll(s.chance("Skills.Swords.CounterAttack", level_swords) * lucky) {
            let back = e.get_damage() / s.adv("Skills.Swords.CounterAttack.DamageModifier", 2.0);
            let attacker = damager.clone();
            vp.send_message(&crate::locale::get("Swords.Combat.Countered", &[]));
            if let Actor::Player(ap) = &attacker {
                ap.send_message(&crate::locale::get("Swords.Combat.Counter.Hit", &[]));
            }
            crate::later(move || hurt(&attacker, back));
        }
    }
}

fn melee(s: &Settings, e: &mut EntityDamageByEntityEvent, attacker: &Player, victim: &Actor, victim_kind: &str, cause: &str) {
    if cause == "THORNS" {
        return;
    }
    let uuid = attacker.get_unique_id().to_ascii_lowercase();
    if let Some(vu) = uuid_of(victim)
        && !s.party_friendly_fire
        && crate::party::same_party(&uuid, &vu)
    {
        e.set_cancelled(true);
        return;
    }
    note_hit(victim, &uuid);
    let held = attacker.get_inventory().get_item_in_main_hand();
    let held_id = held.as_ref().map(|h| h.get_type().key().to_owned()).unwrap_or_default();
    let tool = Tool::of(&held_id);
    let Some(skill) = skill_of_tool(tool) else { return };
    let pvp = matches!(victim, Actor::Player(_));
    if (pvp && !s.pvp[skill.idx()]) || (!pvp && !s.pve[skill.idx()]) {
        return;
    }
    let Some(m) = me(&uuid, skill) else { return };
    if !m.perm {
        return;
    }
    // A readied tool's super ability starts on the hit.
    let mut active = m.active;
    if let Some(ab) = skill.super_ability()
        && !active[ab.idx()]
        && let Some(t) = tool
        && mmo::try_activate(&uuid, skill, t)
    {
        active[ab.idx()] = true;
    }
    let base = e.get_damage();
    let mut dmg = base;
    let level = m.level;
    match skill {
        Skill::Swords => {
            let stab = rank(s, "Swords.Stab", level);
            if stab > 0 {
                dmg += s.adv("Skills.Swords.Stab.Base_Damage", 1.0) + stab as f64 * s.adv("Skills.Swords.Stab.Per_Rank_Multiplier", 1.5);
            }
            let rupture = rank(s, "Swords.Rupture", level);
            if rupture > 0 && roll(s.by_rank("Skills.Swords.Rupture.Rupture_Mechanics.Chance_To_Apply_On_Hit", rupture, 15.0) * m.lucky) {
                start_bleed(s, victim, &uuid, rupture);
                attacker.send_action_bar(&horizon_bukkit::adventure::legacy(&crate::locale::get("Swords.Combat.Bleeding", &[])));
            }
            if active[Ability::SerratedStrikes.idx()] {
                let aoe = base / s.adv("Skills.Swords.SerratedStrikes.DamageModifier", 4.0);
                let uuid2 = uuid.clone();
                let victim2 = victim.clone();
                let rupture_rank = rupture.max(1);
                crate::later(move || {
                    let s = settings::get();
                    for other in nearby(&victim2, &uuid2, 2.5) {
                        hurt(&other, aoe);
                        start_bleed(&s, &other, &uuid2, rupture_rank);
                        if let Actor::Player(p) = &other {
                            p.send_message(&crate::locale::get("Swords.Combat.SS.Struck", &[]));
                        }
                    }
                });
            }
        }
        Skill::Axes => {
            let mastery = rank(s, "Axes.AxeMastery", level);
            dmg += mastery as f64 * s.adv("Skills.Axes.AxeMastery.RankDamageMultiplier", 1.0);
            if rank(s, "Axes.CriticalStrikes", level) > 0 && roll(s.chance("Skills.Axes.CriticalStrikes", level) * m.lucky) {
                let modifier = if pvp { s.adv("Skills.Axes.CriticalStrikes.PVP_Modifier", 1.5) } else { s.adv("Skills.Axes.CriticalStrikes.PVE_Modifier", 2.0) };
                dmg += base * (modifier - 1.0);
                attacker.send_action_bar(&horizon_bukkit::adventure::legacy(&crate::locale::get("Axes.Combat.CriticalHit", &[])));
                if let Actor::Player(p) = victim {
                    p.send_message(&crate::locale::get("Axes.Combat.CritStruck", &[]));
                }
            }
            if rank(s, "Axes.GreaterImpact", level) > 0 && roll(s.adv("Skills.Axes.GreaterImpact.Chance", 25.0) * m.lucky) {
                dmg += s.adv("Skills.Axes.GreaterImpact.BonusDamage", 2.0);
                attacker.send_action_bar(&horizon_bukkit::adventure::legacy(&crate::locale::get("Axes.Combat.GI.Proc", &[])));
                if let Actor::Entity(en) = victim {
                    let knock = s.adv("Skills.Axes.GreaterImpact.KnockbackModifier", 1.5);
                    let (from, to) = (attacker.get_location(), en.get_location());
                    if let Some(to) = to {
                        let (dx, dz) = (to.get_x() - from.get_x(), to.get_z() - from.get_z());
                        let len = (dx * dx + dz * dz).sqrt().max(0.01);
                        en.set_velocity(dx / len * knock, 0.4, dz / len * knock);
                    }
                } else if let Actor::Player(p) = victim {
                    p.send_message(&crate::locale::get("Axes.Combat.GI.Struck", &[]));
                }
            }
            if active[Ability::SkullSplitter.idx()] {
                let aoe = base / s.adv("Skills.Axes.SkullSplitter.DamageModifier", 2.0);
                let uuid2 = uuid.clone();
                let victim2 = victim.clone();
                crate::later(move || {
                    for other in nearby(&victim2, &uuid2, 2.5) {
                        hurt(&other, aoe);
                        if let Actor::Player(p) = &other {
                            p.send_message(&crate::locale::get("Axes.Combat.SS.Struck", &[]));
                        }
                    }
                });
            }
        }
        Skill::Unarmed => {
            let r = rank(s, "Unarmed.SteelArmStyle", level) as f64;
            if r > 0.0 {
                let custom = s.adv_bool.get("Skills.Unarmed.SteelArmStyle.Damage_Override").copied().unwrap_or(false);
                dmg += if custom {
                    s.by_rank("Skills.Unarmed.SteelArmStyle.Override", r as usize, 1.0)
                } else {
                    let bonus = if r >= 18.0 { 1.0 + r - 18.0 } else { 0.0 };
                    bonus + 0.5 + (r / 2.0).floor()
                };
            }
            if active[Ability::Berserk.idx()] {
                dmg += base * 0.5;
            }
            if let Actor::Player(vp) = victim
                && rank(s, "Unarmed.Disarm", level) > 0
                && roll(s.chance("Skills.Unarmed.Disarm", level) * m.lucky)
            {
                let vp = vp.clone();
                crate::later(move || {
                    if let Some(item) = vp.get_inventory().get_item_in_main_hand().filter(|i| !i.get_type().is_air()) {
                        vp.get_inventory().set_item_in_main_hand(&ItemStack::new(Material::of("AIR"), 1));
                        let at = vp.get_location();
                        at.get_world().drop_item_naturally(&at, &item);
                        vp.send_message(&crate::locale::get("Skills.Disarmed", &[]));
                    }
                });
            }
        }
        Skill::Tridents => {
            let r = rank(s, "Tridents.Impale", level);
            if r > 0 {
                dmg += s.adv("Skills.Tridents.Impale.Base_Damage", 1.0) + r as f64 * s.adv("Skills.Tridents.Impale.Rank_Damage_Multiplier", 0.5);
            }
        }
        Skill::Maces => {
            let r = rank(s, "Maces.Crush", level);
            if r > 0 {
                dmg += s.adv("Skills.Maces.Crush.Base_Damage", 0.5) + r as f64 * s.adv("Skills.Maces.Crush.Rank_Damage_Multiplier", 1.0);
            }
            let c = rank(s, "Maces.Cripple", level);
            if c > 0 && roll(s.by_rank("Skills.Maces.Cripple.Chance_To_Apply_On_Hit", c, 10.0) * m.lucky) {
                let v = victim.clone();
                crate::later(move || effect(&v, "minecraft:slowness", 40 + 20 * c as i32, c as i32 - 1));
            }
        }
        Skill::Spears => {
            let r = rank(s, "Spears.SpearMastery", level);
            dmg += r as f64 * s.adv("Skills.Spears.SpearMastery.Rank_Damage_Multiplier", 0.4);
        }
        _ => {}
    }
    if (dmg - base).abs() > f64::EPSILON {
        e.set_damage(dmg);
    }
    if m.creative {
        return;
    }
    let before = health(victim);
    let dealt = e.get_final_damage().min(before.max(0.0));
    combat_xp(s, &uuid, victim, victim_kind, skill, dealt, 1.0);
}

fn ranged(s: &Settings, e: &mut EntityDamageByEntityEvent, proj: &Entity, kind: &str, victim: &Actor, victim_kind: &str) {
    let shot = SHOTS.lock().unwrap_or_else(|e| e.into_inner()).get(&proj.get_entity_id()).cloned();
    let (uuid, crossbow, from) = match shot {
        Some(x) => x,
        None => {
            // A trident thrown (no bow event) or a shot from before a reload: the damage source names the thrower.
            let src = e.get_damage_source();
            let Some(causer) = src.get_causing_entity() else { return };
            let u = causer.get_unique_id().to_ascii_lowercase();
            if !mmo::sessions().contains_key(&u) {
                return;
            }
            (u, false, (0.0, 0.0, 0.0))
        }
    };
    let skill = if kind == "trident" {
        Skill::Tridents
    } else if crossbow {
        Skill::Crossbows
    } else {
        Skill::Archery
    };
    let pvp = matches!(victim, Actor::Player(_));
    if (pvp && !s.pvp[skill.idx()]) || (!pvp && !s.pve[skill.idx()]) {
        return;
    }
    if let Some(vu) = uuid_of(victim)
        && (vu == uuid || (!s.party_friendly_fire && crate::party::same_party(&uuid, &vu)))
    {
        if vu != uuid {
            e.set_cancelled(true);
        }
        return;
    }
    note_hit(victim, &uuid);
    let Some(m) = me(&uuid, skill) else { return };
    if !m.perm {
        return;
    }
    let base = e.get_damage();
    let mut dmg = base;
    let mut xp_mult = 1.0;
    match skill {
        Skill::Archery => {
            let r = rank(s, "Archery.SkillShot", m.level) as f64;
            if r > 0.0 {
                let pct = r * s.adv("Skills.Archery.SkillShot.RankDamageMultiplier", 10.0) / 100.0;
                dmg = (base + base * pct).min(base + s.adv("Skills.Archery.SkillShot.MaxDamage", 9.0));
            }
            if let Actor::Player(vp) = victim
                && rank(s, "Archery.Daze", m.level) > 0
                && roll(s.chance("Skills.Archery.Daze", m.level) * m.lucky)
            {
                dmg += s.adv("Skills.Archery.Daze.BonusDamage", 4.0);
                let vp = vp.clone();
                crate::later(move || {
                    vp.give_potion_effect(&PotionEffect::new("minecraft:nausea", 200, 10, false, true));
                    vp.send_message(&crate::locale::get("Combat.TouchedFuzzy", &[]));
                });
            }
            if let Actor::Entity(en) = victim
                && rank(s, "Archery.ArrowRetrieval", m.level) > 0
                && kind == "arrow"
            {
                let mut a = ARROWS.lock().unwrap_or_else(|e| e.into_inner());
                let entry = a.entry(en.get_entity_id()).or_insert_with(|| (uuid.clone(), 0));
                entry.1 += 1;
            }
            if from != (0.0, 0.0, 0.0)
                && let Some(to) = victim.get_location()
            {
                let d = ((to.get_x() - from.0).powi(2) + (to.get_y() - from.1).powi(2) + (to.get_z() - from.2).powi(2)).sqrt();
                xp_mult = 1.0 + d.min(50.0) * s.archery_distance;
            }
        }
        Skill::Crossbows => {
            let r = rank(s, "Crossbows.PoweredShot", m.level) as f64;
            if r > 0.0 {
                let pct = r * s.adv("Skills.Crossbows.PoweredShot.RankDamageMultiplier", 10.0) / 100.0;
                dmg = (base + base * pct).min(base + s.adv("Skills.Crossbows.PoweredShot.MaxDamage", 9.0));
            }
        }
        Skill::Tridents => {
            let r = rank(s, "Tridents.Impale", m.level);
            if r > 0 {
                dmg += s.adv("Skills.Tridents.Impale.Base_Damage", 1.0) + r as f64 * s.adv("Skills.Tridents.Impale.Rank_Damage_Multiplier", 0.5);
            }
        }
        _ => {}
    }
    if (dmg - base).abs() > f64::EPSILON {
        e.set_damage(dmg);
    }
    if m.creative {
        return;
    }
    let before = health(victim);
    let dealt = e.get_final_damage().min(before.max(0.0));
    combat_xp(s, &uuid, victim, victim_kind, skill, dealt, xp_mult);
}

fn wolf(s: &Settings, e: &mut EntityDamageByEntityEvent, wolf: &Entity, victim: &Actor, victim_kind: &str) {
    let Some(owner) = owner_of(wolf) else { return };
    if !mmo::sessions().contains_key(&owner) {
        return;
    }
    if uuid_of(victim).is_some_and(|v| v == owner) {
        return;
    }
    let Some(m) = me(&owner, Skill::Taming) else { return };
    if !m.perm {
        return;
    }
    let base = e.get_damage();
    let mut dmg = base;
    if rank(s, "Taming.SharpenedClaws", m.level) > 0 {
        dmg += s.adv("Skills.Taming.SharpenedClaws.Bonus", 2.0);
    }
    if rank(s, "Taming.Gore", m.level) > 0 && roll(s.chance("Skills.Taming.Gore", m.level) * m.lucky) {
        dmg *= s.adv("Skills.Taming.Gore.Modifier", 2.0);
        start_bleed(s, victim, &owner, 1);
        if let Some(op) = mmo::sessions().get(&owner).map(|x| x.player.clone()) {
            op.send_message(&crate::locale::get("Combat.Gore", &[]));
        }
        if let Actor::Player(p) = victim {
            p.send_message(&crate::locale::get("Combat.StruckByGore", &[]));
        }
    }
    if (dmg - base).abs() > f64::EPSILON {
        e.set_damage(dmg);
    }
    let before = health(victim);
    let dealt = e.get_final_damage().min(before.max(0.0));
    combat_xp(s, &owner, victim, victim_kind, Skill::Taming, dealt, 1.0);
}

/// Falls: Roll and Graceful Roll, and fall XP (EntityDamageEvent with FALL on a player).
pub fn on_damage(e: &mut EntityDamageEvent) {
    let Some(Actor::Player(p)) = e.get_victim() else { return };
    let uuid = p.get_unique_id().to_ascii_lowercase();
    let god = mmo::sessions().get(&uuid).is_some_and(|s| s.god);
    if god {
        e.set_cancelled(true);
        return;
    }
    if e.get_cause() != "FALL" {
        return;
    }
    let s = settings::get();
    let Some(m) = me(&uuid, Skill::Acrobatics) else { return };
    if !m.perm || m.creative {
        return;
    }
    let teleported = mmo::sessions().get(&uuid).map(|x| x.teleport_at).unwrap_or(0);
    let xp_ok = profile::now_ms() - teleported > (s.adv("Skills.Acrobatics.XP_After_Teleport_Cooldown", 5.0) * 1000.0) as i64;
    let damage = e.get_damage();
    let graceful = p.is_sneaking();
    let feather = p.get_inventory().get_boots().is_some_and(|b| b.get_enchant_level("minecraft:feather_falling") > 0);
    let fmult = if feather { s.feather_mult } else { 1.0 };
    if rank(&s, "Acrobatics.Roll", m.level) > 0 {
        let level = if graceful { m.level * 2 } else { m.level };
        let threshold = if graceful { s.adv("Skills.Acrobatics.GracefulRoll.DamageThreshold", 14.0) } else { s.adv("Skills.Acrobatics.Roll.DamageThreshold", 7.0) };
        if roll(s.chance("Skills.Acrobatics.Roll", level) * m.lucky) {
            let modified = (damage - threshold).max(0.0);
            if modified < p.get_health() {
                e.set_damage(modified);
                if modified <= 0.0 {
                    e.set_cancelled(true);
                }
                let key = if graceful { "Acrobatics.Ability.Proc" } else { "Acrobatics.Roll.Text" };
                p.send_action_bar(&horizon_bukkit::adventure::legacy(&crate::locale::get(key, &[])));
                let mut out = Out::default();
                out.sound(&p, "ROLL_ACTIVATED");
                out.run();
                if xp_ok {
                    mmo::award(&uuid, Skill::Acrobatics, (damage * s.roll_xp * fmult) as f32, Reason::Pve);
                }
                return;
            }
        }
    }
    if xp_ok && damage < p.get_health() {
        mmo::award(&uuid, Skill::Acrobatics, (damage * s.fall_xp * fmult) as f32, Reason::Pve);
    }
}

/// A mob's death: Arrow Retrieval, then the id forgotten.
pub fn on_death(id: u64, at: Option<horizon_bukkit::Location>) {
    let arrows = ARROWS.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    forget(id);
    BLEEDS.lock().unwrap_or_else(|e| e.into_inner()).retain(|b| !matches!(&b.target, Actor::Entity(en) if en.get_entity_id() == id));
    let (Some((uuid, n)), Some(at)) = (arrows, at) else { return };
    let s = settings::get();
    let level = profile::store().get(&uuid).map(|p| p.level[Skill::Archery.idx()]).unwrap_or(0);
    let chance = s.chance("Skills.Archery.ArrowRetrieval", level);
    let back = (0..n).filter(|_| roll(chance)).count() as i32;
    if back > 0 {
        at.get_world().drop_item_naturally(&at, &ItemStack::new(Material::of("ARROW"), back));
    }
}

/// Hardcore: a player's death costs levels (Death_Stat_Loss), a killer steals some (Vampirism).
pub fn on_player_death(victim: &Player, killer: Option<&Player>) {
    let s = settings::get();
    let vu = victim.get_unique_id().to_ascii_lowercase();
    if mmo::sessions().get(&vu).is_some_and(|x| x.has(mmo::perm::BYPASS_HARDCORE)) {
        return;
    }
    let any_loss = s.death_skills.iter().any(|b| *b);
    let any_vamp = s.vampirism_skills.iter().any(|b| *b) && killer.is_some();
    if !any_loss && !any_vamp {
        return;
    }
    let mut lost_total = 0;
    let mut stolen_total = 0;
    {
        let mut st = profile::store();
        let ku = killer.map(|k| k.get_unique_id().to_ascii_lowercase());
        let mut steal: Vec<(Skill, i32)> = Vec::new();
        if let Some(p) = st.get_mut(&vu) {
            for skill in Skill::non_child() {
                let lvl = p.level[skill.idx()];
                if lvl <= s.death_threshold {
                    continue;
                }
                if any_loss && s.death_skills[skill.idx()] {
                    let lost = ((lvl - s.death_threshold) as f64 * s.death_penalty / 100.0) as i32;
                    p.level[skill.idx()] -= lost;
                    p.xp[skill.idx()] = 0.0;
                    lost_total += lost;
                }
                if any_vamp && s.vampirism_skills[skill.idx()] {
                    let take = ((p.level[skill.idx()] - s.death_threshold).max(0) as f64 * s.vampirism / 100.0) as i32;
                    if take > 0 {
                        p.level[skill.idx()] -= take;
                        steal.push((skill, take));
                    }
                }
            }
        }
        if let Some(ku) = ku
            && let Some(k) = st.get_mut(&ku)
        {
            for (skill, n) in steal {
                k.level[skill.idx()] += n;
                stolen_total += n;
            }
        }
    }
    if lost_total > 0 {
        victim.send_message(&crate::locale::get("Hardcore.DeathStatLoss.PlayerDeath", &[&lost_total.to_string()]));
    }
    if let Some(k) = killer {
        if stolen_total > 0 {
            k.send_message(&crate::locale::get("Hardcore.Vampirism.Killer.Success", &[&stolen_total.to_string(), victim.get_name()]));
            victim.send_message(&crate::locale::get("Hardcore.Vampirism.Victim.Success", &[k.get_name(), &stolen_total.to_string()]));
        } else if any_vamp {
            k.send_message(&crate::locale::get("Hardcore.Vampirism.Killer.Failure", &[victim.get_name()]));
        }
    }
    let _ = rand_unit();
}

/// Beast Lore: a bone on a tamed animal shows its health and owner.
pub fn beast_lore(player: &Player, target: &Entity) {
    let uuid = player.get_unique_id().to_ascii_lowercase();
    let Some(m) = me(&uuid, Skill::Taming) else { return };
    let s = settings::get();
    if !m.perm || rank(&s, "Taming.BeastLore", m.level) == 0 {
        return;
    }
    let owner = owner_of(target);
    let owner_name = owner.as_deref().and_then(|u| profile::store().get(u).map(|p| p.name.clone())).unwrap_or_else(|| "?".into());
    player.send_message(&crate::locale::get("Combat.BeastLore", &[]));
    player.send_message(&crate::locale::get("Combat.BeastLoreOwner", &[&owner_name]));
    player.send_message(&crate::locale::get("Combat.BeastLoreHealth", &[&format!("{:.0}", target.get_health()), &format!("{:.0}", target.get_max_health())]));
}
