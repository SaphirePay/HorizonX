//! `Commandtpaall`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab_player_first), ..Command::new("tpaall") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    if args.is_empty() {
        let user = sender.get_user().ok_or_else(EssError::not_enough)?;
        tpa_all(sender, &user);
        return Ok(());
    }
    tpa_all(sender, &get_player(sender, args, 0)?);
    Ok(())
}

fn tpa_all(sender: &CommandSource, target: &User) {
    sender.send_tl("teleportAAll", &[]);
    let own = sender.get_player().is_some_and(|player| target.player().as_ref() == Some(player));
    for player in ess().get_online_users() {
        if player == *target || !player.is_teleport_enabled() {
            continue;
        }
        if own && target.get_world() != player.get_world() && settings().is_world_teleport_permissions() && target.get_world().is_some_and(|world| !target.is_authorized(&format!("essentials.worlds.{}", world.get_name()))) {
            continue;
        }
        player.request_teleport(target, true);
        player.send_tl("teleportHereRequest", crate::args![target.get_display_name()]);
        player.send_tl("typeTpaccept", &[]);
        if settings().get_tpa_accept_cancellation() != 0 {
            player.send_tl("teleportRequestTimeoutInfo", crate::args![settings().get_tpa_accept_cancellation()]);
        }
    }
}
