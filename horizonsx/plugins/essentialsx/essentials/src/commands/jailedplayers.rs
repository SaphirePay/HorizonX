//! `Commandjailedplayers`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("jailedplayers") };

fn run(sender: &CommandSource, _label: &str, _args: &[String]) -> EssResult {
    let names: Vec<String> = ess().get_jailed_players().iter().map(|player| player.get_name().to_owned()).collect();
    if names.is_empty() {
        sender.send_tl("jailedPlayersEmpty", &[]);
    } else {
        sender.send_tl("jailedPlayersList", crate::args![join_list(&names)]);
    }
    Ok(())
}
