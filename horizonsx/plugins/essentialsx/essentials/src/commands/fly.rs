//! `Commandfly`, an `EssentialsToggleCommand`.
//!
//! `FlyStatusChangeEvent` isn't fired: nothing could listen.
use crate::prelude::*;

toggle_command!("fly", "essentials.fly.others");

fn toggle_player(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult {
    let Some(player) = user.player() else { return Ok(()) };
    let enabled = enabled.unwrap_or_else(|| !player.get_allow_flight());
    player.set_fall_distance(0.0);
    player.set_allow_flight(enabled);
    user.set_fly_mode_enabled(enabled);
    if !enabled {
        player.set_flying(false);
    }
    let args = crate::args![enable_disable(&user.get_source(), enabled), user.get_display_name()];
    user.send_tl("flyMode", args);
    if !is_self(sender, user) {
        sender.send_tl("flyMode", args);
    }
    Ok(())
}
