//! `Commandkick`. `UserKickEvent` isn't fired: nothing here can listen.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("kick") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let target = get_player_raw(None, args, 0, true, false)?;
    if let (Some(user), Some(player)) = (sender.get_user(), sender.get_player()) {
        fail_if!(target.is_hidden_to(player) && !user.can_interact_vanished() && target.is_hidden_from(player), EssError::player_not_found());
        fail_if!(target.is_authorized("essentials.kick.exempt"), EssError::tl("kickExempt", &[]));
    }
    let reason = if args.len() > 1 { get_final_arg(args, 1) } else { mini_to_legacy(&tl_literal("kickDefault", &[])) };
    let reason = reason_of(&reason);
    if let Some(player) = target.player() {
        player.kick_player(&reason);
    }
    let (_, sender_display) = ban_names(sender);
    let notice = crate::args![sender_display, target.get_name(), reason];
    crate::info(&mini_to_legacy(&tl_literal("playerKicked", notice)));
    ess().broadcast_tl_permission(None, "essentials.kick.notify", "playerKicked", notice);
    Ok(())
}
