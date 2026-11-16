//! `Commandenderchest`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("enderchest") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let target = if !args.is_empty() && user.is_authorized("essentials.enderchest.others") { get_player_for(user, args, 0)? } else { user.clone() };
    let (Some(player), Some(base)) = (user.player(), target.player()) else {
        return Ok(());
    };
    let (user, others) = (user.clone(), target != *user);
    inventories::after_turn(move || {
        player.close_inventory();
        if player.open_ender_chest(&base.ender_chest()) {
            user.set_ender_see(others);
        }
    });
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && user.is_authorized("essentials.enderchest.others") { get_players(&user.get_source()) } else { Vec::new() })
}
