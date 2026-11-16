//! `EssentialsEntityListener`: god mode, keep inventory, back on death, powertools on a hit.
use crate::prelude::*;
use crate::settings::KeepInvPolicy;
use horizon_bukkit::fam_ev_entity::{EntityCombustEvent, EntityDamageByEntityEvent, FoodLevelChangeEvent};
use horizon_bukkit::events::EntityDamageEvent;
use horizon_bukkit::fam_ev_entity::{Actor, EntityShootBowEvent, EntityTargetEvent, PlayerDeathEvent, PotionSplashEvent};
use horizon_bukkit::enums::event_priority::{HIGH, LOW, MONITOR};
use horizon_bukkit::fam_ev_entity::EntityCombustByEntityEvent;
use horizon_bukkit::Plugin;

fn player_of(uuid: Option<String>) -> Option<User> {
    ess().get_user(&horizon_bukkit::fam_ess::get_player_by_id(&uuid?)?)
}

/// `onPlayerVsPlayerDamage`: true cancels.
fn player_vs_player(defender: &Player, attacker: &User) -> bool {
    let mut cancel = false;
    let delay = settings().get_login_attack_delay();
    if delay > 0 && now_ms() < attacker.get_last_login() + delay && !attacker.is_authorized("essentials.pvpdelay.exempt") {
        cancel = true;
    }
    if attacker.player().as_ref() != Some(defender) && (attacker.has_invulnerability_after_teleport() || ess().get_user(defender).is_some_and(|user| user.has_invulnerability_after_teleport())) {
        cancel = true;
    }
    if attacker.is_god_mode_enabled() && !attacker.is_authorized("essentials.god.pvp") {
        cancel = true;
    }
    if attacker.is_hidden() && !attacker.is_authorized("essentials.vanish.pvp") {
        cancel = true;
    }
    if attacker.are_power_tools_enabled() {
        let held = attacker.get_item_in_hand();
        let commands = held.and_then(|stack| attacker.get_powertool(stack.get_type())).unwrap_or_default();
        for template in commands {
            let command = template.replace("{player}", defender.get_name());
            if !command.is_empty() && command != template {
                let target = attacker.clone();
                ess().schedule_sync_delayed_task(move || {
                    if let Some(player) = target.player() {
                        player.chat(&format!("/{command}"));
                    }
                    crate::info(&format!("[PT] {} issued server command: /{command}", target.get_name()));
                });
                return true;
            }
        }
    }
    cancel
}

pub fn register(plugin: Plugin) {
    plugin.register_event_with(LOW, true, |event: &mut horizon_bukkit::fam_ev_entity::EntityRegainHealthEvent| {
        if event.get_regain_reason() == "SATIATED"
            && let Some(horizon_bukkit::fam_ev_entity::Actor::Player(player)) = event.get_entity()
            && ess().get_user(&player).is_some_and(|user| user.is_afk())
            && settings().get_freeze_afk_players()
        {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, false, |event: &mut EntityDamageByEntityEvent| {
        let Some(attacker_player) = event.get_damager().and_then(|damager| damager.as_player().cloned()) else { return };
        let Some(attacker) = ess().get_user(&attacker_player) else { return };
        let Some(target) = event.get_entity() else { return };
        match target.as_player() {
            Some(defender) => {
                if player_vs_player(defender, &attacker) {
                    event.set_cancelled(true);
                }
            }
            None => {
                let hand = inventories::get_item_in_main_hand(&attacker_player);
                if settings().is_milk_bucket_easter_egg_enabled() && hand.as_ref().is_some_and(|hand| hand.get_type().name() == "MILK_BUCKET") && target.as_entity().is_some_and(|entity| horizon_bukkit::data::flag(&entity.data_at("Age").clone())) {
                    if let Some(entity) = target.as_entity() {
                        entity.merge("{Age:-24000}");
                    }
                    let empty = ItemStack::new(horizon_bukkit::Material::of("BUCKET"), 1);
                    inventories::set_item_in_main_hand(&attacker_player, &empty);
                    event.set_cancelled(true);
                }
            }
        }
        attacker.update_activity_on_interact(true);
    });
    plugin.register_event_with(LOW, true, |event: &mut EntityDamageEvent| {
        if let Some(user) = player_of(event.get_entity().map(|entity| entity.get_unique_id()))
            && user.is_god_mode_enabled()
        {
            if let Some(player) = user.player() {
                player.set_fire_ticks(0);
                player.set_remaining_air(player.get_maximum_air());
            }
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, false, |event: &mut PlayerDeathEvent| {
        let Some(user) = event.get_entity().and_then(|player| ess().get_user(&player)) else { return };
        if user.is_authorized("essentials.keepxp") {
            event.set_keep_level(true);
            event.set_should_drop_experience(false);
        }
    });
    plugin.register_event_with(LOW, false, |event: &mut PlayerDeathEvent| {
        let Some(user) = event.get_entity().and_then(|player| ess().get_user(&player)) else { return };
        if user.is_authorized("essentials.keepinv") {
            event.set_keep_inventory(true);
            let Some(player) = user.player() else { return };
            for item in inventories::get_inventory(&player, true).into_iter().flatten() {
                let drops = event.get_drops();
                if let Some(at) = drops.iter().position(|drop| *drop == item) {
                    drops.remove(at);
                }
            }
            let (vanish, bind) = (settings().get_vanishing_items_policy(), settings().get_binding_items_policy());
            if vanish != KeepInvPolicy::Keep || bind != KeepInvPolicy::Keep {
                let mut dropped = Vec::new();
                inventories::remove_items(
                    &player,
                    |stack| {
                        let policy = if vanish != KeepInvPolicy::Keep && stack.has_enchant("vanishing_curse") {
                            vanish
                        } else if bind != KeepInvPolicy::Keep && stack.has_enchant("binding_curse") {
                            bind
                        } else {
                            return false;
                        };
                        if policy == KeepInvPolicy::Drop {
                            dropped.push(stack.clone());
                        }
                        true
                    },
                    true,
                );
                event.get_drops().extend(dropped);
            }
        }
    });
    plugin.register_event_with(HIGH, true, |event: &mut PlayerDeathEvent| {
        let Some(user) = event.get_entity().and_then(|player| ess().get_user(&player)) else { return };
        if settings().info_after_death()
            && let Some(at) = user.get_location()
        {
            user.send_tl("infoAfterDeath", crate::args![at.get_world().get_name(), at.get_block_x(), at.get_block_y(), at.get_block_z()]);
        }
        if user.is_authorized("essentials.back.ondeath") && !settings().is_command_disabled("back") {
            user.set_last_location();
            user.send_tl("backAfterDeath", &[]);
        }
        if !settings().are_death_messages_enabled() {
            event.set_death_message(Some(""));
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut FoodLevelChangeEvent| {
        let Some(user) = event.get_entity().and_then(|entity| entity.as_player().and_then(|player| ess().get_user(player))) else { return };
        if user.is_god_mode_enabled() {
            if user.is_god_mode_enabled_raw()
                && let Some(player) = user.player()
            {
                player.set_food_level(20);
                player.set_saturation(10.0);
            }
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut PotionSplashEvent| {
        for entity in event.get_affected_entities() {
            if let Actor::Player(player) = &entity
                && ess().get_user(player).is_some_and(|user| user.is_god_mode_enabled())
            {
                event.set_intensity(&entity, 0.0);
            }
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut EntityShootBowEvent| {
        if let Some(Actor::Player(player)) = event.get_entity()
            && let Some(user) = ess().get_user(&player)
            && user.is_afk()
        {
            user.update_activity_on_interact(true);
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut EntityTargetEvent| {
        if let Some(Actor::Player(player)) = event.get_target()
            && let Some(user) = ess().get_user(&player)
            && user.is_vanished()
        {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut EntityCombustEvent| {
        if player_of(event.get_entity().and_then(|entity| entity.as_player().map(|player| player.get_unique_id()))).is_some_and(|user| user.is_god_mode_enabled()) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(MONITOR, false, |event: &mut EntityCombustByEntityEvent| {
        let Some(Actor::Entity(arrow)) = event.get_combuster() else { return };
        if !arrow.get_type().ends_with("arrow") || event.get_entity().is_none_or(|entity| entity.as_player().is_none()) {
            return;
        }
        let owner = arrow.data_at("Owner");
        let hex: String = owner.as_array().into_iter().flatten().filter_map(serde_json::Value::as_i64).map(|part| format!("{:08x}", part as i32)).collect();
        if let Some(shooter) = horizon_bukkit::fam_ess::get_player_by_id(&hex).and_then(|player| ess().get_user(&player)) {
            if shooter.is_god_mode_enabled() && !shooter.is_authorized("essentials.god.pvp") {
                event.set_cancelled(true);
            }
            if shooter.is_hidden() && !shooter.is_authorized("essentials.vanish.pvp") {
                event.set_cancelled(true);
            }
        }
    });
}
