//! `Commandtpauto`.
use crate::prelude::*;

toggle_command!("tpauto", "essentials.tpauto.others");

fn toggle_player(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult {
    let enabled = enabled.unwrap_or_else(|| !user.is_auto_teleport_enabled());
    user.set_auto_teleport_enabled(enabled);
    user.send_tl(if enabled { "autoTeleportEnabled" } else { "autoTeleportDisabled" }, &[]);
    if enabled && !user.is_teleport_enabled() {
        user.send_tl("teleportationDisabledWarning", &[]);
    }
    if !is_self(sender, user) {
        sender.send_tl(if enabled { "autoTeleportEnabledFor" } else { "autoTeleportDisabledFor" }, crate::args![user.get_display_name()]);
    }
    Ok(())
}
