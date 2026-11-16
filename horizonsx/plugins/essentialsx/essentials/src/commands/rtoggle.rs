//! `Commandrtoggle`.
use crate::prelude::*;

toggle_command!("rtoggle", "essentials.rtoggle.others");

fn toggle_player(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult {
    let enabled = enabled.unwrap_or_else(|| !user.is_last_message_reply_recipient());
    user.set_last_message_reply_recipient(enabled);
    user.send_tl(if enabled { "replyLastRecipientEnabled" } else { "replyLastRecipientDisabled" }, &[]);
    if !is_self(sender, user) {
        sender.send_tl(if enabled { "replyLastRecipientEnabledFor" } else { "replyLastRecipientDisabledFor" }, crate::args![user.get_display_name()]);
    }
    Ok(())
}
