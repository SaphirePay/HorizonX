//! `Commandgetpos`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("getpos") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    let Some(own) = user.get_location() else { return Ok(()) };
    if !args.is_empty() && user.is_authorized("essentials.getpos.others") {
        if let Some(other) = get_player_for(user, args, 0)?.get_location() {
            output_position(&user.get_source(), &other, Some(&own));
        }
        return Ok(());
    }
    output_position(&user.get_source(), &own, None);
    Ok(())
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    if let Some(location) = get_player_raw(None, args, 0, true, false)?.get_location() {
        output_position(sender, &location, None);
    }
    Ok(())
}

fn output_position(sender: &CommandSource, coords: &Location, distance: Option<&Location>) {
    sender.send_tl("currentWorld", crate::args![coords.world.get_name()]);
    sender.send_tl("posX", crate::args![coords.get_block_x()]);
    sender.send_tl("posY", crate::args![coords.get_block_y()]);
    sender.send_tl("posZ", crate::args![coords.get_block_z()]);
    sender.send_tl("posYaw", crate::args![(coords.get_yaw() + 360.0) % 360.0]);
    sender.send_tl("posPitch", crate::args![coords.get_pitch()]);
    if let Some(distance) = distance.filter(|distance| distance.world == coords.world) {
        sender.send_tl("distance", crate::args![coords.distance(distance)]);
    }
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && sender.is_authorized("essentials.getpos.others") { get_players(sender) } else { Vec::new() })
}
