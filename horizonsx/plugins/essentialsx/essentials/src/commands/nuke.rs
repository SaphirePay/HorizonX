//! `Commandnuke`. `TNTExplodeListener` swaps each of these for a harmless blast of power 0: the
//! explosion event here can't name the TNT, so the power is set on the TNT as it's spawned.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("nuke") };

pub const NUKE_META_KEY: &str = "ess_tnt_proj";

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let targets = if args.is_empty() { ess().get_online_users() } else { (0..args.len()).map(|index| get_player(sender, args, index)).collect::<EssResult<Vec<_>>>()? };
    for user in targets {
        user.send_tl("nuke", &[]);
        let Some(location) = user.get_location() else { continue };
        let world = location.world.clone();
        let (center_x, center_z) = (location.get_block_x(), location.get_block_z());
        let top = world.get_highest_block_y_at(center_x, center_z) + 64;
        for x in (-10..=10).step_by(5) {
            for z in (-10..=10).step_by(5) {
                let at = Location::new(world.clone(), f64::from(center_x + x), f64::from(top), f64::from(center_z + z), 0.0, 0.0);
                if let Some(tnt) = world.spawn(&at, "tnt") {
                    tnt.merge("{explosion_power:0f}");
                    tnt.add_scoreboard_tag(NUKE_META_KEY);
                }
            }
        }
    }
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { get_players(sender) } else { Vec::new() })
}
