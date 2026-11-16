//! `Commandkickall`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("kickall") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let reason = if args.is_empty() { mini_to_legacy(&tl_literal("kickDefault", &[])) } else { get_final_arg(args, 0) };
    let reason = reason_of(&reason);
    for user in ess().get_online_users() {
        let Some(player) = user.player() else { continue };
        let is_sender = sender.get_player().is_some_and(|own| own.get_name().eq_ignore_ascii_case(player.get_name()));
        if !is_sender && !user.is_authorized("essentials.kickall.exempt") {
            player.kick_player(&reason);
        }
    }
    sender.send_tl("kickedAll", &[]);
    Ok(())
}
