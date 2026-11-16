//! `Commandtptoggle`.
use crate::prelude::*;

toggle_command!("tptoggle", "essentials.tptoggle.others");

fn toggle_player(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult {
    let enabled = enabled.unwrap_or_else(|| !user.is_teleport_enabled());
    user.set_teleport_enabled(enabled);
    user.send_tl(if enabled { "teleportationEnabled" } else { "teleportationDisabled" }, &[]);
    if !is_self(sender, user) {
        sender.send_tl(if enabled { "teleportationEnabledFor" } else { "teleportationDisabledFor" }, crate::args![user.get_display_name()]);
    }
    Ok(())
}
