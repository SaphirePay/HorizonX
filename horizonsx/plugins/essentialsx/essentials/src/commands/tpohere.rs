//! `Commandtpohere`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_source: Some(tab_player_first), ..Command::new("tpohere") };

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let player = get_player_for(user, args, 0)?;
    if let Some(world) = user.get_world() {
        check_world_permission(user, user.get_world() != player.get_world(), &world)?;
    }
    if let Some(base) = user.player() {
        player.get_async_teleport().now_player(&base, false, "COMMAND", get_new_exception_future(&user.get_source(), label));
    }
    Ok(())
}
