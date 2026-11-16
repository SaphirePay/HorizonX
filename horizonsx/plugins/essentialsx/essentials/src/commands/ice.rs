//! `Commandice`. Until the game can set a player's freeze ticks (see "Engine requests" in GAPS.md)
//! this says `unsupportedFeature`, Java's own answer on a server without them.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("ice") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty() && !sender.is_player(), EssError::not_enough());
    if let Some(first) = args.first().filter(|_| sender.is_authorized("essentials.ice.others")) {
        return loop_online_players(sender, false, true, first, |user| {
            if freeze_player(sender, user) {
                sender.send_tl("iceOther", crate::args![user.get_display_name()]);
            }
            Ok(())
        });
    }
    if let Some(user) = sender.get_user() {
        freeze_player(sender, &user);
    }
    Ok(())
}

fn freeze_player(sender: &CommandSource, user: &User) -> bool {
    let frozen = user.player().is_some_and(|player| player.set_freeze_ticks(player.get_max_freeze_ticks()));
    if frozen {
        user.send_tl("ice", &[]);
    } else {
        sender.send_tl("unsupportedFeature", &[]);
    }
    frozen
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && sender.is_authorized("essentials.ice.others") { get_players(sender) } else { Vec::new() })
}
