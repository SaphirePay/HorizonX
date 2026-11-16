//! `Commandext`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("ext") };

fn others(sender: &CommandSource, term: &str) -> EssResult {
    loop_online_players(sender, true, true, term, |player| {
        ext_player(player);
        sender.send_tl("extinguishOthers", crate::args![player.get_display_name()]);
        Ok(())
    })
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    others(sender, first)
}

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    if let Some(first) = args.first().filter(|_| user.is_authorized("essentials.ext.others")) {
        return others(&user.get_source(), first);
    }
    ext_player(user);
    user.send_tl("extinguish", &[]);
    Ok(())
}

fn ext_player(user: &User) {
    if let Some(player) = user.player() {
        player.set_fire_ticks(0);
    }
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { get_players(sender) } else { Vec::new() })
}
