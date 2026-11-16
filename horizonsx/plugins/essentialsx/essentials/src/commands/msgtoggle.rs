//! `Commandmsgtoggle`.
use crate::prelude::*;

toggle_command!("msgtoggle", "essentials.msgtoggle.others");

fn toggle_player(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult {
    let enabled = enabled.unwrap_or_else(|| !user.is_ignore_msg());
    user.set_ignore_msg(enabled);
    user.send_tl(if enabled { "msgDisabled" } else { "msgEnabled" }, &[]);
    if !is_self(sender, user) {
        sender.send_tl(if enabled { "msgDisabledFor" } else { "msgEnabledFor" }, crate::args![user.get_display_name()]);
    }
    Ok(())
}
