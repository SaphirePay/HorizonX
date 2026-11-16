//! `Commandcompass`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("compass") };

fn run(user: &User, _label: &str, _args: &[String]) -> EssResult {
    let Some(location) = user.get_location() else { return Ok(()) };
    let bearing = (location.get_yaw() + 180.0 + 360.0) as i32 % 360;
    let key = match bearing {
        i32::MIN..23 => "north",
        23..68 => "northEast",
        68..113 => "east",
        113..158 => "southEast",
        158..203 => "south",
        203..248 => "southWest",
        248..293 => "west",
        293..338 => "northWest",
        _ => "north",
    };
    user.send_tl("compassBearing", crate::args![user.player_tl(key, &[]), bearing]);
    Ok(())
}
