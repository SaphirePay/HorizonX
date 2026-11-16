//! `Commandbigtree`.
use crate::prelude::*;
use crate::utils::location::get_target_within;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("bigtree") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let tree = match args.first().map(|first| first.to_lowercase()).as_deref() {
        Some("redwood") => "TALL_REDWOOD",
        Some("tree") => "BIG_TREE",
        Some("jungle") => "JUNGLE",
        Some("darkoak") => "DARK_OAK",
        _ => return Err(EssError::not_enough()),
    };
    let Some(player) = user.player() else { return Ok(()) };
    let mut at = get_target_within(&player, settings().get_max_tree_command_range())?;
    at.add(0.0, 1.0, 0.0);
    fail_if!(at.get_block().is_solid() || !at.world.generate_tree(&at, tree), EssError::tl("bigTreeFailure", &[]));
    user.send_tl("bigTreeSuccess", &[]);
    Ok(())
}

fn tab(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { ["redwood", "tree", "jungle", "darkoak"].map(str::to_owned).to_vec() } else { Vec::new() })
}
