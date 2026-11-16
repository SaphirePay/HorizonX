//! `Commanddepth`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("depth") };

fn run(user: &User, _label: &str, _args: &[String]) -> EssResult {
    let (Some(location), Some(world)) = (user.get_location(), user.get_world()) else {
        return Ok(());
    };
    let depth = location.get_block_y() - world.get_sea_level();
    if depth > 0 {
        user.send_tl("depthAboveSea", crate::args![depth]);
    } else if depth < 0 {
        user.send_tl("depthBelowSea", crate::args![-depth]);
    } else {
        user.send_tl("depth", &[]);
    }
    Ok(())
}
