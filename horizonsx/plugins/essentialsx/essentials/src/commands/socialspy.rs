//! `Commandsocialspy`.
use crate::prelude::*;

toggle_command!("socialspy", "essentials.socialspy.others");

fn toggle_player(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult {
    let enabled = enabled.unwrap_or_else(|| !user.is_social_spy_enabled());
    user.set_social_spy_enabled(enabled);
    let args = crate::args![user.get_display_name(), enable_disable(&user.get_source(), enabled)];
    user.send_tl("socialSpy", args);
    if !is_self(sender, user) {
        sender.send_tl("socialSpy", args);
    }
    Ok(())
}
