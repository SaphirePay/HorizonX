//! `Commandtree`.
use crate::prelude::*;
use crate::utils::location::get_target_within;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("tree") };

/// `TreeType.values()`.
const TREES: [&str; 29] = [
    "TREE", "BIG_TREE", "REDWOOD", "TALL_REDWOOD", "BIRCH", "JUNGLE", "SMALL_JUNGLE", "COCOA_TREE", "JUNGLE_BUSH", "RED_MUSHROOM", "BROWN_MUSHROOM", "SWAMP", "ACACIA", "DARK_OAK", "MEGA_REDWOOD", "MEGA_PINE", "TALL_BIRCH", "CHORUS_PLANT", "CRIMSON_FUNGUS", "WARPED_FUNGUS", "AZALEA", "MANGROVE", "TALL_MANGROVE", "CHERRY", "PALE_OAK",
    "PALE_OAK_CREAKING", "RED_POPLAR", "ORANGE_POPLAR", "YELLOW_POPLAR",
];

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let tree = if first.eq_ignore_ascii_case("jungle") { Some("SMALL_JUNGLE") } else { TREES.iter().find(|tree| tree.replace('_', "").eq_ignore_ascii_case(first)).copied() };
    let tree = tree.ok_or_else(EssError::not_enough)?;
    let Some(player) = user.player() else { return Ok(()) };
    let mut at = get_target_within(&player, settings().get_max_tree_command_range())?;
    at.add(0.0, 1.0, 0.0);
    fail_if!(at.get_block().is_solid(), EssError::tl("treeFailure", &[]));
    user.send_tl(if at.world.generate_tree(&at, tree) { "treeSpawned" } else { "treeFailure" }, &[]);
    Ok(())
}

fn tab(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { TREES.iter().map(|tree| tree.to_lowercase().replace('_', "")).collect() } else { Vec::new() })
}
