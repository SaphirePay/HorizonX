//! `Commandtppos`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("tppos") };

fn base_location(user: &User, words: &[String]) -> EssResult<(Location, f64, f64, f64)> {
    let at = user.get_location().ok_or_else(EssError::player_not_found)?;
    let (x, y, z) = (coordinate(&words[0], at.get_x())?, coordinate(&words[1], at.get_y())?, coordinate(&words[2], at.get_z())?);
    Ok((Location::new(at.get_world().clone(), x, y, z, at.get_yaw(), at.get_pitch()), x, y, z))
}

/// `ess.getWorld`: Java goes on with `null` and fails on it: this says so, in its words.
fn world_named(name: &str) -> EssResult<World> {
    ess().get_world(name).ok_or_else(|| EssError::tl("invalidWorld", &[]))
}

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 3, EssError::not_enough());
    let (mut location, x, y, z) = base_location(user, args)?;
    let set_world = |location: &mut Location, name: &str| -> EssResult {
        let world = world_named(name)?;
        check_world_permission(user, user.get_world().as_ref() != Some(&world), &world)?;
        location.world = world;
        Ok(())
    };
    if args.len() == 4 {
        set_world(&mut location, &args[3])?;
    }
    if args.len() > 4 {
        location.yaw = (parse_double(&args[3])? as f32 + 360.0) % 360.0;
        location.pitch = parse_double(&args[4])? as f32;
    }
    if args.len() > 5 {
        set_world(&mut location, &args[5])?;
    }
    check_position(x, y, z, || user.player_tl("teleportInvalidLocation", &[]))?;
    let charge = Trade::of_command("tppos");
    charge.is_affordable_for(user)?;
    user.send_tl("teleporting", &teleporting_args(&location));
    user.get_async_teleport().teleport(&location, Some(&charge), "COMMAND", get_new_exception_future(&user.get_source(), label));
    Err(EssError::NoCharge)
}

fn run_source(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 4, EssError::not_enough());
    let user = get_player_raw(None, args, 0, true, false)?;
    let (mut location, x, y, z) = base_location(&user, &args[1..])?;
    if args.len() == 5 {
        location.world = world_named(&args[4])?;
    }
    if args.len() > 5 {
        location.yaw = (parse_double(&args[4])? as f32 + 360.0) % 360.0;
        location.pitch = parse_double(&args[5])? as f32;
    }
    if args.len() > 6 {
        location.world = world_named(&args[6])?;
    }
    check_position(x, y, z, || sender.tl("teleportInvalidLocation", &[]))?;
    sender.send_tl("teleporting", &teleporting_args(&location));
    user.send_tl("teleporting", &teleporting_args(&location));
    user.get_async_teleport().teleport(&location, None, "COMMAND", get_new_exception_future(&user.get_source(), label));
    Ok(())
}

fn options(position: usize) -> Vec<String> {
    match position {
        1..=3 => vec!["~0".to_owned()],
        4 | 5 => vec!["0".to_owned()],
        6 => get_worlds().iter().map(|world| world.get_name().to_owned()).collect(),
        _ => Vec::new(),
    }
}

fn tab_user(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(options(args.len()))
}

fn tab_source(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { get_players(sender) } else { options(args.len() - 1) })
}
