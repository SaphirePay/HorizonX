//! `EssentialsBlockListener`: unlimited items come back, and a converted spawner keeps its mob.
use crate::prelude::*;
use crate::mob;
use crate::utils::material::is_spawner;
use horizon_bukkit::Plugin;
use horizon_bukkit::enums::event_priority::LOW;
use horizon_bukkit::fam_ev_world::BlockPlaceEvent;
use horizon_bukkit::enums::event_priority::LOWEST;
use horizon_bukkit::fam_ev_world::PaperServerListPingEvent;

pub fn register(plugin: Plugin) {
    // `EssentialsServerListener`: the vanished aren't in the list.
    plugin.register_event_with(LOWEST, false, |event: &mut PaperServerListPingEvent| {
        event.get_listed_players().retain(|listed| !ess().users().get_user_by_name(listed.name()).is_some_and(|user| user.is_vanished()));
    });
    plugin.register_event_with(LOW, true, |event: &mut BlockPlaceEvent| {
        let Some(stack) = event.get_item_in_hand().cloned() else { return };
        let Some(user) = ess().get_user(event.get_player()) else { return };
        if is_spawner(stack.get_type())
            && stack.get_component("minecraft:custom_data").is_some_and(|data| data["convert"].is_string())
            && let Some(entity) = stack.get_component("minecraft:block_entity_data").and_then(|data| data["SpawnData"]["entity"]["id"].as_str())
            && let Some(found) = mob::from_entity(entity)
            && user.is_authorized(&format!("essentials.spawnerconvert.{}", found.key.to_lowercase()))
        {
            let spawner = event.get_block_placed().get_location();
            horizon_bukkit::dispatch_command(&format!("execute in {} run data merge block {} {} {} {{SpawnData:{{entity:{{id:\"minecraft:{}\"}}}}}}", spawner.get_world().get_name(), spawner.get_block_x(), spawner.get_block_y(), spawner.get_block_z(), found.entity));
        }
        if user.has_unlimited(&stack) && user.player().is_some_and(|player| player.get_game_mode() == "SURVIVAL") {
            let target = user.clone();
            ess().schedule_sync_delayed_task(move || {
                if let Some(player) = target.player().filter(|_| !stack.get_type().is_air()) {
                    inventories::add_item(&player, 0, false, &[stack.as_one()]);
                }
            });
        }
    });
}
