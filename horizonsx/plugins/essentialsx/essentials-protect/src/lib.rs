//! `EssentialsProtect`. Emergency mode is for Essentials failing to load, which here is the same
//! library failing: there's nothing left to run it.
use essentials::prelude::*;
use essentials::utils::material::is_bed;
use horizon_bukkit::enums::event_priority::HIGHEST;
use horizon_bukkit::event::PlayerInteractEvent;
use horizon_bukkit::events::{BlockBurnEvent, BlockFromToEvent};
use horizon_bukkit::fam_ev_entity::{Actor, EntityBreakDoorEvent, EntityChangeBlockEvent, EntityDamageByBlockEvent, EntityDamageByEntityEvent, EntityDamageEvent, EntityExplodeEvent, EntityTargetEvent, EntityTransformEvent, ExplosionPrimeEvent, PreCreatureSpawnEvent};
use horizon_bukkit::fam_ev_world::{BlockIgniteEvent, HangingBreakByEntityEvent, LightningStrikeEvent, PortalCreateEvent, ThunderChangeEvent, WeatherChangeEvent};
use horizon_bukkit::Plugin;

fn bool_of(key: &str, default: bool) -> bool {
    settings().get_protect_boolean(key, default)
}

fn prevent(name: &str) -> bool {
    bool_of(&format!("protect.prevent.{name}"), false)
}

fn disable(name: &str) -> bool {
    bool_of(&format!("protect.disable.{name}"), false)
}

fn short_id(kind: &str) -> String {
    kind.trim_start_matches("minecraft:").to_lowercase()
}

fn should_be_damaged(user: &User, kind: &str) -> bool {
    user.is_authorized(&format!("essentials.protect.damage.{kind}")) && !user.is_authorized("essentials.protect.damage.disable")
}

/// `!(target instanceof Player && shouldBeDamaged(user, type))`.
fn spared(user: Option<&User>, kind: &str) -> bool {
    !user.is_some_and(|user| should_be_damaged(user, kind))
}

fn user_of(player: &Player) -> Option<User> {
    ess().get_user(player)
}

fn actor_id(actor: &Actor) -> String {
    match actor {
        Actor::Player(_) => "player".to_owned(),
        Actor::Entity(entity) => short_id(&entity.get_type()),
    }
}

/// The start of `onEntityDamage`, and its end, which every damage event shares.
fn plain_damage(victim: Option<&Actor>, cause: &str) -> bool {
    if let Some(Actor::Entity(entity)) = victim
        && short_id(&entity.get_type()) == "villager"
        && prevent("villager-death")
    {
        return true;
    }
    let Some(Actor::Player(player)) = victim else { return false };
    let Some(user) = user_of(player) else { return false };
    match cause {
        "FALL" => disable("fall") && !should_be_damaged(&user, "fall"),
        "SUFFOCATION" => disable("suffocate") && !should_be_damaged(&user, "suffocation"),
        "FIRE" | "FIRE_TICK" => disable("firedmg") && !should_be_damaged(&user, "fire"),
        "DROWNING" => disable("drown") && !should_be_damaged(&user, "drowning"),
        "LIGHTNING" => disable("lightning") && !should_be_damaged(&user, "lightning"),
        "WITHER" => disable("wither") && !should_be_damaged(&user, "wither"),
        _ => false,
    }
}

fn victim_user(victim: Option<&Actor>) -> Option<User> {
    match victim {
        Some(Actor::Player(player)) => user_of(player),
        _ => None,
    }
}

/// The `EntityDamageByBlockEvent` part.
fn damage_by_block(event: &EntityDamageByBlockEvent) -> bool {
    let victim = event.get_entity();
    let user = victim_user(victim.as_ref());
    let cause = event.get_cause();
    (disable("contactdmg") && cause == "CONTACT" && spared(user.as_ref(), "contact"))
        || (disable("lavadmg") && cause == "LAVA" && spared(user.as_ref(), "lava"))
        || (prevent("tnt-explosion") && cause == "BLOCK_EXPLOSION" && event.get_damager().is_some_and(|block| block.get_type().name() == "TNT") && spared(user.as_ref(), "tnt"))
        || plain_damage(victim.as_ref(), cause)
}

fn item_damage(attacker: &str, cause: &str) -> bool {
    match cause {
        "FIRE" | "FIRE_TICK" | "LAVA" => prevent("lava-itemdamage"),
        "ENTITY_EXPLOSION" => match attacker {
            "tnt" => prevent("tnt-itemdamage"),
            "creeper" => prevent("creeper-itemdamage"),
            "tnt_minecart" => prevent("tnt-minecart-itemdamage"),
            "wither_skull" => prevent("witherskull-itemdamage"),
            "fireball" | "small_fireball" => prevent("fireball-itemdamage"),
            _ => false,
        },
        _ => false,
    }
}

const PROJECTILES: [&str; 12] = ["arrow", "spectral_arrow", "trident", "snowball", "egg", "ender_pearl", "fireball", "small_fireball", "wither_skull", "dragon_fireball", "shulker_bullet", "llama_spit"];

fn shooter_of(actor: &Actor) -> Option<Player> {
    let Actor::Entity(entity) = actor else {
        return None;
    };
    let owner = entity.data_at("Owner");
    let hex: String = owner.as_array()?.iter().filter_map(serde_json::Value::as_i64).map(|part| format!("{:08x}", part as i32)).collect();
    horizon_bukkit::fam_ess::get_player_by_id(&hex)
}

/// The `EntityDamageByEntityEvent` part, with `onEntityDamageByEntity` for dropped items.
fn damage_by_entity(event: &EntityDamageByEntityEvent) -> bool {
    let victim = event.get_entity();
    let user = victim_user(victim.as_ref());
    let cause = event.get_cause();
    let Some(attack) = event.get_damager() else {
        return plain_damage(victim.as_ref(), cause);
    };
    let attack_id = actor_id(&attack);
    let victim_id = victim.as_ref().map(actor_id).unwrap_or_default();
    if victim_id == "item" && item_damage(&attack_id, cause) {
        return true;
    }
    let pvp_denied = |one: &User, other: &User| !one.is_authorized("essentials.protect.pvp") || !other.is_authorized("essentials.protect.pvp");
    let refused = (attack_id == "creeper" && prevent("creeper-playerdamage") && spared(user.as_ref(), "creeper"))
        || (matches!(victim_id.as_str(), "fireball" | "small_fireball") && prevent("fireball-playerdamage") && spared(user.as_ref(), "fireball"))
        || (victim_id == "wither_skull" && prevent("witherskull-playerdamage") && spared(user.as_ref(), "witherskull"))
        || (attack_id == "tnt" && prevent("tnt-playerdamage") && spared(user.as_ref(), "tnt"))
        || (attack_id == "tnt_minecart" && prevent("tnt-minecart-playerdamage") && spared(user.as_ref(), "tnt-minecart"));
    if refused {
        return true;
    }
    if let Some(user) = &user {
        if let Actor::Player(hitter) = &attack
            && disable("pvp")
            && let Some(hitter) = user_of(hitter)
            && !user.get_name().eq_ignore_ascii_case(&hitter.get_name())
            && pvp_denied(user, &hitter)
        {
            return true;
        }
        if PROJECTILES.contains(&attack_id.as_str()) {
            if disable("projectiles") && !should_be_damaged(user, "projectiles") {
                return true;
            }
            if disable("pvp")
                && let Some(shooter) = shooter_of(&attack).and_then(|player| user_of(&player))
                && pvp_denied(user, &shooter)
            {
                return true;
            }
        }
    }
    plain_damage(victim.as_ref(), cause)
}

fn ignite(event: &BlockIgniteEvent) -> Option<bool> {
    let block = event.get_block()?;
    if block.get_type().name() == "OBSIDIAN" || block.get_relative(0, -1, 0).get_type().name() == "OBSIDIAN" {
        return Some(prevent("portal-creation"));
    }
    match event.get_cause() {
        "SPREAD" => Some(bool_of("protect.prevent.fire-spread", true)),
        "FLINT_AND_STEEL" => Some(prevent("flint-fire")),
        "LAVA" => Some(bool_of("protect.prevent.lava-fire-spread", true)),
        "LIGHTNING" => Some(bool_of("protect.prevent.lightning-fire-spread", true)),
        "FIREBALL" => Some(prevent("fireball-fire")),
        _ => None,
    }
}

fn transform(reason: &str, entity: &str) -> bool {
    match reason {
        "INFECTION" => prevent("transformation.zombie-villager"),
        "CURED" => prevent("transformation.villager"),
        "LIGHTNING" => match entity {
            "villager" => prevent("transformation.witch"),
            "pig" => prevent("transformation.zombie-pigman"),
            "creeper" => prevent("transformation.charged-creeper"),
            "mooshroom" => prevent("transformation.mooshroom"),
            _ => false,
        },
        "DROWNED" => prevent("transformation.drowned"),
        _ => false,
    }
}

fn interact(event: &mut PlayerInteractEvent) {
    if event.get_action() != "RIGHT_CLICK_BLOCK" {
        return;
    }
    let Some(block) = event.get_clicked_block().cloned() else { return };
    let environment = block.get_world().get_environment();
    if is_bed(&block.get_type()) && environment != "NORMAL" {
        event.set_cancelled(prevent("bed-explosion"));
    }
    if block.get_type().name() == "RESPAWN_ANCHOR" && environment != "NETHER" {
        let charges: i32 = block.get_property("charges").and_then(|text| text.parse().ok()).unwrap_or(0);
        let held_glowstone = essentials::inventories::get_item_in_hand(event.get_player()).is_some_and(|item| item.get_type().name() == "GLOWSTONE");
        if (charges > 0 && !held_glowstone) || charges == 4 {
            event.set_cancelled(prevent("respawn-anchor-explosion"));
        }
    }
}

thread_local! {
    /// While a harmless explosion stands in for a cancelled one.
    static FAKING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `instanceof Fireball`: the skull and the wind charges are fireballs to Bukkit too.
fn is_fireball(kind: &str) -> bool {
    matches!(kind, "fireball" | "small_fireball" | "dragon_fireball" | "wither_skull" | "wind_charge" | "breeze_wind_charge")
}

/// `onEntityExplode`, and the wind charge's of `EssentialsProtectEntityListener_1_21_3_R1`.
fn explode(event: &mut EntityExplodeEvent) {
    // The sound and smoke made below raises this event too, here with the same entity.
    if FAKING.get() {
        return;
    }
    let Some(kind) = event.get_entity_type().map(short_id) else { return };
    let at = event.get_location();
    if kind == "wind_charge" && prevent("windcharge-explosion") {
        event.set_cancelled(true);
    }
    let fake = |at: &Option<Location>| {
        if let Some(at) = at {
            FAKING.set(true);
            at.create_explosion(0.0);
            FAKING.set(false);
        }
    };
    if kind == "ender_dragon" && bool_of("protect.prevent.enderdragon-blockdamage", true) {
        event.set_cancelled(true);
        if bool_of("protect.enderdragon-fakeexplosions", false) {
            fake(&at);
        }
        return;
    }
    let max_height = settings().get_protect_creeper_max_height();
    if kind == "wither" && prevent("wither-spawnexplosion") {
        event.set_cancelled(true);
    } else if kind == "creeper" && (bool_of("protect.prevent.creeper-explosion", true) || prevent("creeper-blockdamage") || (max_height >= 0 && at.as_ref().is_some_and(|at| at.get_block_y() > max_height))) {
        event.set_cancelled(true);
        fake(&at);
    } else if (kind == "tnt" && prevent("tnt-explosion"))
        || (is_fireball(&kind) && kind != "wind_charge" && prevent("fireball-explosion"))
        || (kind == "wither_skull" && prevent("witherskull-explosion"))
        || (kind == "tnt_minecart" && prevent("tnt-minecart-explosion"))
        || (kind == "end_crystal" && prevent("ender-crystal-explosion"))
    {
        event.set_cancelled(true);
    }
}

const TARGET_REASONS: [&str; 7] = ["CLOSEST_PLAYER", "TARGET_ATTACKED_ENTITY", "TARGET_ATTACKED_NEARBY_ENTITY", "RANDOM_TARGET", "DEFEND_VILLAGE", "TARGET_ATTACKED_OWNER", "OWNER_ATTACKED_TARGET"];

/// `onEnable`.
pub fn on_enable(plugin: Plugin) {
    plugin.info("Continuing to enable Protect.");
    plugin.register_event_with(HIGHEST, true, |event: &mut BlockIgniteEvent| {
        if let Some(cancel) = ignite(event) {
            event.set_cancelled(cancel);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut BlockFromToEvent| {
        let Some(block) = event.get_block() else { return };
        match block.get_type().name().as_str() {
            "WATER" => event.set_cancelled(prevent("water-flow")),
            "LAVA" => event.set_cancelled(prevent("lava-flow")),
            "AIR" => event.set_cancelled(prevent("water-bucket-flow")),
            _ => {}
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut BlockBurnEvent| {
        if bool_of("protect.prevent.fire-spread", true) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut PortalCreateEvent| {
        if event.get_reason() == "FIRE" {
            event.set_cancelled(prevent("portal-creation"));
        }
    });
    plugin.register_event_with(HIGHEST, true, interact);
    plugin.register_event_with(HIGHEST, true, |event: &mut EntityDamageEvent| {
        if plain_damage(event.get_victim().as_ref(), event.get_cause()) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut EntityDamageByBlockEvent| {
        if damage_by_block(event) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut EntityDamageByEntityEvent| {
        if damage_by_entity(event) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut PreCreatureSpawnEvent| {
        if settings().get_protect_prevent_spawn(&short_id(event.get_type())) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut EntityTargetEvent| {
        let Some(Actor::Player(player)) = event.get_target() else { return };
        let bypass = user_of(&player).is_some_and(|user| user.is_authorized("essentials.protect.entitytarget.bypass"));
        if TARGET_REASONS.contains(&event.get_reason()) && prevent("entitytarget") && !bypass {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut ExplosionPrimeEvent| {
        if event.get_entity_type().is_some_and(|kind| is_fireball(&short_id(kind))) && prevent("fireball-fire") {
            event.set_fire(false);
        }
    });
    plugin.register_event_with(HIGHEST, true, explode);
    plugin.register_event_with(HIGHEST, true, |event: &mut EntityChangeBlockEvent| {
        let refused = match short_id(event.get_entity_type()).as_str() {
            "enderman" => prevent("enderman-pickup"),
            "wither" => prevent("wither-blockreplace"),
            "sheep" => prevent("sheep-eat-grass"),
            "ravager" => prevent("ravager-thief"),
            _ => false,
        };
        if refused {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut EntityBreakDoorEvent| {
        if event.get_entity().is_some_and(|entity| short_id(&entity.get_type()) == "zombie") && prevent("zombie-door-break") {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut HangingBreakByEntityEvent| {
        let Some(remover) = event.get_remover() else { return };
        let refused = match short_id(&remover.get_type()).as_str() {
            "creeper" => bool_of("protect.prevent.creeper-explosion", true),
            "fireball" | "small_fireball" => prevent("fireball-explosion"),
            "tnt" => prevent("tnt-explosion"),
            "wither_skull" => prevent("witherskull-explosion"),
            _ => false,
        };
        if refused {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut EntityTransformEvent| {
        if transform(event.get_transform_reason(), &short_id(event.get_entity_kind())) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut WeatherChangeEvent| {
        if disable("weather.storm") && event.to_weather_state() {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut LightningStrikeEvent| {
        if disable("weather.lightning") {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, true, |event: &mut ThunderChangeEvent| {
        if disable("weather.thunder") && event.to_thunder_state() {
            event.set_cancelled(true);
        }
    });
}
