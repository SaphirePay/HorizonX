//! `Commandworld`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("world") };

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    let worlds = get_worlds();
    let world = if args.is_empty() {
        let Some(nether) = worlds.iter().find(|world| world.get_environment() == "NETHER").cloned() else { return Ok(()) };
        if user.get_world().as_ref() == Some(&nether) { worlds[0].clone() } else { nether }
    } else {
        match ess().get_world(&get_final_arg(args, 0)) {
            Some(world) => world,
            None => {
                user.send_tl("invalidWorld", &[]);
                user.send_tl("possibleWorlds", crate::args![worlds.len() - 1]);
                user.send_tl("typeWorldName", &[]);
                return Err(EssError::NoCharge);
            }
        }
    };
    let node = format!("essentials.worlds.{}", world.get_name());
    fail_if!(settings().is_world_teleport_permissions() && !user.is_authorized(&node), EssError::tl("noPerm", crate::args![node]));
    let (Some(from), Some(at)) = (user.get_world(), user.get_location()) else {
        return Err(EssError::player_not_found());
    };
    let factor = match (from.get_environment(), world.get_environment()) {
        ("NETHER", "NORMAL") => 8.0,
        ("NORMAL", "NETHER") => 1.0 / 8.0,
        _ => 1.0,
    };
    let target = Location::new(world, f64::from(at.get_block_x()) * factor + 0.5, f64::from(at.get_block_y()), f64::from(at.get_block_z()) * factor + 0.5, 0.0, 0.0);
    let charge = Trade::of_command("world");
    charge.is_affordable_for(user)?;
    user.get_async_teleport().teleport(&target, Some(&charge), "COMMAND", get_new_exception_future(&user.get_source(), label));
    Err(EssError::NoCharge)
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() != 1 {
        return Some(Vec::new());
    }
    Some(get_worlds().iter().filter(|world| !settings().is_world_teleport_permissions() || user.is_authorized(&format!("essentials.worlds.{}", world.get_name()))).map(|world| world.get_name().to_owned()).collect())
}
