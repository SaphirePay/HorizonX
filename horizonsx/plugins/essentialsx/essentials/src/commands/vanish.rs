//! `Commandvanish`.
use crate::prelude::*;

toggle_command!("vanish", "essentials.vanish.others");

fn toggle_player(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult {
    let enabled = enabled.unwrap_or_else(|| !user.is_vanished());
    crate::hooks::fire(crate::hooks::Hook::Vanish { user, value: enabled });
    user.set_vanished(enabled);
    let state = enable_disable(&user.get_source(), enabled);
    user.send_tl("vanish", &[user.get_display_name().into(), state.clone()]);
    if enabled {
        user.send_tl("vanished", &[]);
    }
    if !is_self(sender, user) {
        sender.send_tl("vanish", &[user.get_display_name().into(), state]);
    }
    Ok(())
}
