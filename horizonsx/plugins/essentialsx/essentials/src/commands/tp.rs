//! `Commandtp`.
use crate::prelude::*;
use crate::command_source::console_display_name;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("tp") };

fn world_differs(one: &User, other: &User) -> bool {
    one.get_world() != other.get_world()
}

fn check_enabled(user: &User) -> EssResult {
    if user.is_teleport_enabled() { Ok(()) } else { Err(EssError::tl("teleportDisabled", crate::args![user.get_display_name()])) }
}

/// A place from three words, keeping the base player's facing.
fn position(base: &User, words: &[String], invalid: impl FnOnce() -> String) -> EssResult<Location> {
    let at = base.get_location().ok_or_else(EssError::player_not_found)?;
    let (x, y, z) = (coordinate(&words[0], at.get_x())?, coordinate(&words[1], at.get_y())?, coordinate(&words[2], at.get_z())?);
    check_position(x, y, z, invalid)?;
    Ok(Location::new(at.get_world().clone(), x, y, z, at.get_yaw(), at.get_pitch()))
}

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    let future = get_new_exception_future(&user.get_source(), label);
    match args.len() {
        0 => return Err(EssError::not_enough()),
        1 => {
            let player = get_player_for_offline(user, args, 0, true)?;
            check_enabled(&player)?;
            if !player.is_online() {
                fail_if!(user.is_authorized("essentials.tpoffline"), EssError::tl("teleportOffline", crate::args![player.get_display_name()]));
                return Err(EssError::player_not_found());
            }
            if let Some(world) = player.get_world() {
                check_world_permission(user, world_differs(user, &player), &world)?;
            }
            let charge = Trade::of_command("tp");
            charge.is_affordable_for(user)?;
            if let Some(base) = player.player() {
                user.get_async_teleport().teleport_player_target(&base, Some(&charge), "COMMAND", future);
            }
            return Err(EssError::NoCharge);
        }
        3 => {
            fail_if!(!user.is_authorized("essentials.tp.position"), EssError::tl("noPerm", crate::args!["essentials.tp.position"]));
            let location = position(user, args, || user.player_tl("teleportInvalidLocation", &[]))?;
            user.get_async_teleport().now(&location, false, "COMMAND", future.clone());
            let told = user.clone();
            future.then_accept(move |success| {
                if success {
                    told.send_tl("teleporting", &teleporting_args(&location));
                }
            });
        }
        4 => {
            for node in ["essentials.tp.others", "essentials.tp.position"] {
                fail_if!(!user.is_authorized(node), EssError::tl("noPerm", crate::args![node]));
            }
            let target = get_player_for(user, args, 0)?;
            let location = position(&target, &args[1..], || user.player_tl("teleportInvalidLocation", &[]))?;
            check_enabled(&target)?;
            user.send_tl("teleporting", &teleporting_args(&location));
            target.get_async_teleport().now(&location, false, "COMMAND", future.clone());
            let told = target.clone();
            future.then_accept(move |success| {
                if success {
                    told.send_tl("teleporting", &teleporting_args(&location));
                }
            });
        }
        _ => {
            fail_if!(!user.is_authorized("essentials.tp.others"), EssError::tl("noPerm", crate::args!["essentials.tp.others"]));
            let target = get_player_for(user, args, 0)?;
            let to_player = get_player_for(user, args, 1)?;
            check_enabled(&target)?;
            check_enabled(&to_player)?;
            if let Some(world) = to_player.get_world() {
                check_world_permission(user, world_differs(&target, &to_player), &world)?;
            }
            target.send_tl("teleportAtoB", crate::args![user.get_display_name(), to_player.get_display_name()]);
            if let Some(base) = to_player.player() {
                target.get_async_teleport().now_player(&base, false, "COMMAND", future);
            }
        }
    }
    Ok(())
}

fn run_source(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let target = get_player_raw(None, args, 0, true, false)?;
    if args.len() == 2 {
        let to_player = get_player_raw(None, args, 1, true, false)?;
        target.send_tl("teleportAtoB", crate::args![console_display_name(), to_player.get_display_name()]);
        if let Some(base) = to_player.player() {
            target.get_async_teleport().now_player(&base, false, "COMMAND", get_new_exception_future(sender, label));
        }
    } else if args.len() > 3 {
        let location = position(&target, &args[1..], || sender.tl("teleportInvalidLocation", &[]))?;
        sender.send_tl("teleporting", &teleporting_args(&location));
        let future = get_new_exception_future(sender, label);
        target.get_async_teleport().now(&location, false, "COMMAND", future.clone());
        let told = target.clone();
        future.then_accept(move |success| {
            if success {
                told.send_tl("teleporting", &teleporting_args(&location));
            }
        });
    } else {
        return Err(EssError::not_enough());
    }
    Ok(())
}

fn tab_user(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 || (args.len() == 2 && user.is_authorized("essentials.tp.others")) { get_players(&user.get_source()) } else { Vec::new() })
}

fn tab_source(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() <= 2 { get_players(sender) } else { Vec::new() })
}
