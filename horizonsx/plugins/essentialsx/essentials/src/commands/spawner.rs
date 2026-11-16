//! `Commandspawner`.
use crate::prelude::*;
use crate::mob;
use crate::utils::location::get_target;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("spawner") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let name = args.first().filter(|first| first.encode_utf16().count() >= 2).ok_or_else(|| EssError::NotEnoughArguments(user.player_tl("mobsAvailable", crate::args![join_list(&mob::get_mob_list())])))?;
    let Some(player) = user.player() else { return Ok(()) };
    let target = get_target(&player)?;
    let block = target.get_block();
    fail_if!(block.get_type().name() != "SPAWNER", EssError::tl("mobSpawnTarget", &[]));
    let found = mob::from_name(name).ok_or_else(|| EssError::tl("invalidMob", &[]))?;
    fail_if!(settings().get_protect_prevent_spawn(found.entity), EssError::tl("disabledToSpawnMob", &[]));
    let lower = found.name.to_lowercase();
    fail_if!(!user.is_authorized(&format!("essentials.spawner.{lower}")), EssError::tl("noPermToSpawnMob", &[]));
    let delay = args.get(1).filter(|second| is_int(second) && user.is_authorized("essentials.spawner.delay")).and_then(|second| second.parse().ok()).unwrap_or(0);
    let charge = Trade::of_command(&format!("spawner-{lower}"));
    charge.is_affordable_for(user)?;
    let mut spawner = block.tile();
    spawner.set_spawned_entity(&format!("minecraft:{}", found.entity), delay);
    fail_if!(!spawner.update(), EssError::tl("mobSpawnError", &[]));
    charge.charge(user)?;
    user.send_tl("setSpawner", crate::args![found.name]);
    Ok(())
}
