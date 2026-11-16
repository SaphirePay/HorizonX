//! `Commandtphere`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_source: Some(tab_player_first), ..Command::new("tphere") };

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    let player = get_player_for(user, args, 0)?;
    fail_if!(!player.is_teleport_enabled(), EssError::tl("teleportDisabled", crate::args![player.get_display_name()]));
    if let Some(world) = user.get_world() {
        check_world_permission(user, user.get_world() != player.get_world(), &world)?;
    }
    if let Some(base) = user.player() {
        user.get_async_teleport().teleport_player_to(&player, &base, Some(&Trade::of_command("tphere")), "COMMAND", get_new_exception_future(&user.get_source(), label));
    }
    Err(EssError::NoCharge)
}
