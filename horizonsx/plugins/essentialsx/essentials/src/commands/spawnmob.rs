//! `Commandspawnmob`.
use crate::prelude::*;
use crate::mob;
use crate::spawn_mob::{mob_data, mob_parts, spawnmob, spawnmob_at_target};

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab), ..Command::new("spawnmob") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(|| EssError::NotEnoughArguments(user.player_tl("mobsAvailable", crate::args![join_list(&mob::get_mob_list())])))?;
    let parts = mob_parts(first);
    let data = mob_data(first);
    let count = args.get(1).map_or(Ok(1), |second| parse_int(second))?;
    fail_if!(parts.len() > 1 && !user.is_authorized("essentials.spawnmob.stack"), EssError::tl("cannotStackMob", &[]));
    if args.len() >= 3 {
        return spawnmob(&user.get_source(), &get_player_for(user, args, 2)?, parts, &data, count);
    }
    spawnmob_at_target(user, parts, &data, count)
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 3, EssError::NotEnoughArguments(sender.tl("mobsAvailable", crate::args![join_list(&mob::get_mob_list())])));
    spawnmob(sender, &get_player_raw(None, args, 2, true, false)?, mob_parts(&args[0]), &mob_data(&args[0]), parse_int(&args[1])?)
}

fn tab(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { mob::get_mob_list() } else { Vec::new() })
}
