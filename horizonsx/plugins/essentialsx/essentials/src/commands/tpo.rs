//! `Commandtpo`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("tpo") };

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    match args.len() {
        0 => return Err(EssError::not_enough()),
        1 => {
            let player = get_player_for(user, args, 0)?;
            if let Some(world) = player.get_world() {
                check_world_permission(user, user.get_world() != player.get_world(), &world)?;
            }
            if let Some(base) = player.player() {
                user.get_async_teleport().now_player(&base, false, "COMMAND", get_new_exception_future(&user.get_source(), label));
            }
        }
        _ => {
            fail_if!(!user.is_authorized("essentials.tp.others"), EssError::tl("noPerm", crate::args!["essentials.tp.others"]));
            let target = get_player_for(user, args, 0)?;
            let to_player = get_player_for(user, args, 1)?;
            if let Some(world) = to_player.get_world() {
                check_world_permission(user, target.get_world() != to_player.get_world(), &world)?;
            }
            let future = get_new_exception_future(&user.get_source(), label);
            if let Some(base) = to_player.player() {
                target.get_async_teleport().now_player(&base, false, "COMMAND", future.clone());
            }
            let (told, by) = (target.clone(), user.get_display_name());
            future.then_accept(move |success| {
                if success {
                    told.send_tl("teleportAtoB", crate::args![by.clone(), to_player.get_display_name()]);
                }
            });
        }
    }
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 || (args.len() == 2 && user.is_authorized("essentials.tp.others")) { get_players(&user.get_source()) } else { Vec::new() })
}
