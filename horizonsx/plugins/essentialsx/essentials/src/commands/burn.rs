//! `Commandburn`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("burn") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let user = get_player(sender, args, 0)?;
    let seconds = parse_int(&args[1])?;
    if let Some(player) = user.player() {
        player.set_fire_ticks((i64::from(seconds) * 20).clamp(0, i64::from(i32::MAX)) as i32);
    }
    sender.send_tl("burnMsg", crate::args![user.get_display_name(), seconds.max(0)]);
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_players(sender),
        2 => COMMON_DURATIONS.iter().map(|duration| (*duration).to_owned()).collect(),
        _ => Vec::new(),
    })
}
