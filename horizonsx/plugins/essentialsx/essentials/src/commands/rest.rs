//! `Commandrest`. Until the game can set an online player's `TIME_SINCE_REST` (see "Engine requests"
//! in GAPS.md) this says `unsupportedFeature`, Java's own answer on a server without it.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("rest") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty() && !sender.is_player(), EssError::not_enough());
    if let Some(first) = args.first().filter(|_| sender.is_authorized("essentials.rest.others")) {
        return loop_online_players(sender, false, true, first, |user| {
            if rest_player(sender, user) {
                sender.send_tl("restOther", crate::args![user.get_display_name()]);
            }
            Ok(())
        });
    }
    if let Some(user) = sender.get_user() {
        rest_player(sender, &user);
    }
    Ok(())
}

fn rest_player(sender: &CommandSource, user: &User) -> bool {
    let rested = user.player().is_some_and(|player| player.set_time_since_rest(0));
    if rested {
        user.send_tl("rest", &[]);
    } else {
        sender.send_tl("unsupportedFeature", &[]);
    }
    rested
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && sender.is_authorized("essentials.rest.others") { get_players(sender) } else { Vec::new() })
}
