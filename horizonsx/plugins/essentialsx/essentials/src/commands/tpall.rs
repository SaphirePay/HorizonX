//! `Commandtpall`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab_player_first), ..Command::new("tpall") };

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    if args.is_empty() {
        let user = sender.get_user().ok_or_else(EssError::not_enough)?;
        teleport_all_players(sender, &user, label);
        return Ok(());
    }
    teleport_all_players(sender, &get_player(sender, args, 0)?, label);
    Ok(())
}

fn teleport_all_players(sender: &CommandSource, target: &User, label: &str) {
    sender.send_tl("teleportAll", &[]);
    let Some(location) = target.get_location() else { return };
    let own = sender.get_player().is_some_and(|player| target.player().as_ref() == Some(player));
    for player in ess().get_online_users() {
        if player == *target {
            continue;
        }
        if own && target.get_world() != player.get_world() && settings().is_world_teleport_permissions() && target.get_world().is_some_and(|world| !target.is_authorized(&format!("essentials.worlds.{}", world.get_name()))) {
            continue;
        }
        player.get_async_teleport().now(&location, false, "COMMAND", get_new_exception_future(sender, label));
    }
}
