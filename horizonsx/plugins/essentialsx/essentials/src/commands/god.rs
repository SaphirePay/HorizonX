//! `Commandgod`. `GodStatusChangeEvent` isn't fired: nothing could listen.
use crate::prelude::*;

toggle_command!("god", "essentials.god.others");

fn toggle_player(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult {
    let enabled = enabled.unwrap_or_else(|| !user.is_god_mode_enabled());
    user.set_god_mode_enabled(enabled);
    if let Some(player) = user.player().filter(|player| enabled && player.get_health() != 0.0) {
        player.set_health(player.get_max_health());
        player.set_food_level(20);
    }
    let args = crate::args![enable_disable(&user.get_source(), enabled)];
    user.send_tl("godMode", args);
    if !is_self(sender, user) {
        sender.send_tl("godMode", args);
    }
    Ok(())
}
