//! `Commandkill`. The damage event Java raises first can't be raised here: only god mode, this
//! plugin's own listener, can refuse it, so that's what is checked in its place.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("kill") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let forced = || sender.get_user().is_some_and(|by| by.is_authorized("essentials.kill.force"));
    loop_online_players(sender, true, true, first, |user| {
        let Some(player) = user.player() else { return Ok(()) };
        fail_if!(sender.is_player() && user.is_authorized("essentials.kill.exempt") && !forced(), EssError::player_exempt("killExempt", crate::args![user.get_display_name()]));
        if user.is_god_mode_enabled() && sender.is_player() && !forced() {
            return Ok(());
        }
        player.set_health(0.0);
        sender.send_tl("kill", crate::args![user.get_display_name()]);
        Ok(())
    })
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { get_players(sender) } else { Vec::new() })
}
