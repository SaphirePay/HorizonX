//! `Commandtpa`. `TPARequestEvent` isn't fired: nothing could listen.
use crate::prelude::*;
use crate::teleport::TeleportType;

pub const COMMAND: Command = Command { run_user: Some(run), tab_source: Some(tab_player_first), ..Command::new("tpa") };

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    let player = requested_player(user, args, false)?;
    if player.is_auto_teleport_enabled() && !player.is_ignored_player(user) {
        let charge = Trade::of_command("tpa");
        let teleport = user.get_async_teleport();
        teleport.set_tp_type(TeleportType::Tpa);
        let future = get_new_exception_future(&user.get_source(), label);
        if let Some(base) = player.player() {
            teleport.teleport_player_target(&base, Some(&charge), "COMMAND", future.clone());
        }
        let (target, asker) = (player.clone(), user.clone());
        future.then_accept(move |success| {
            if success {
                target.send_tl("requestAcceptedAuto", crate::args![asker.get_display_name()]);
                asker.send_tl("requestAcceptedFromAuto", crate::args![target.get_display_name()]);
            }
        });
        return Err(EssError::NoCharge);
    }
    send_request(user, &player, false);
    if user.is_authorized("essentials.tpacancel") {
        user.send_tl("typeTpacancel", &[]);
    }
    Ok(())
}

/// Who `/tpa` or `/tpahere` is asking, if they can be asked.
pub fn requested_player(user: &User, args: &[String], here: bool) -> EssResult<User> {
    fail_if!(args.is_empty(), EssError::not_enough());
    let player = get_player_for(user, args, 0)?;
    fail_if!(user.get_name().eq_ignore_ascii_case(&player.get_name()), EssError::not_enough());
    fail_if!(!player.is_authorized("essentials.tpaccept"), EssError::tl("teleportNoAcceptPermission", crate::args![player.get_display_name()]));
    fail_if!(!player.is_teleport_enabled(), EssError::tl("teleportDisabled", crate::args![player.get_display_name()]));
    if let Some(world) = if here { user.get_world() } else { player.get_world() } {
        check_world_permission(user, user.get_world() != player.get_world(), &world)?;
    }
    fail_if!(player.has_outstanding_tpa_request(&user.get_name(), here), EssError::tl("requestSentAlready", crate::args![player.get_display_name()]));
    Ok(player)
}

/// The request, unless they ignore this player, who is told it was sent either way.
pub fn send_request(user: &User, player: &User, here: bool) {
    if !player.is_ignored_player(user) {
        player.request_teleport(user, here);
        player.send_tl(if here { "teleportHereRequest" } else { "teleportRequest" }, crate::args![user.get_display_name()]);
        player.send_tl("typeTpaccept", &[]);
        player.send_tl("typeTpdeny", &[]);
        if settings().get_tpa_accept_cancellation() != 0 {
            player.send_tl("teleportRequestTimeoutInfo", crate::args![settings().get_tpa_accept_cancellation()]);
        }
    }
    user.send_tl("requestSent", crate::args![player.get_display_name()]);
}
