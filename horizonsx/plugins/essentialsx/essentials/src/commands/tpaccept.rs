//! `Commandtpaccept`. `TeleportRequestResponseEvent` isn't fired: nothing could listen.
use crate::prelude::*;
use crate::teleport::TeleportType;
use crate::user::TpaRequest;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("tpaccept") };

fn no_request() -> EssError {
    EssError::tl("noPendingRequest", &[])
}

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    let accept_all = args.first().is_some_and(|first| first == "*" || first.eq_ignore_ascii_case("all"));
    fail_if!(!user.has_pending_tpa_requests(true, accept_all), no_request());
    if accept_all {
        accept_all_requests(user, label);
        return Err(EssError::NoCharge);
    }
    if args.is_empty() {
        user.send_tl("requestAccepted", &[]);
        handle_teleport(user, user.get_next_tpa_request(true, false, false), label)?;
    } else {
        user.send_tl("requestAccepted", &[]);
        let name = get_player_for(user, args, 0)?.get_name();
        handle_teleport(user, user.get_outstanding_tpa_request(&name, true), label)?;
    }
    Err(EssError::NoCharge)
}

fn accept_all_requests(user: &User, label: &str) {
    let mut count = 0;
    while let Some(request) = user.get_next_tpa_request(true, true, true) {
        match handle_teleport(user, Some(request.clone()), label) {
            Ok(()) => count += 1,
            Err(error) => ess().show_error(&user.get_source(), &error, label),
        }
        user.remove_tpa_request(&request.name);
    }
    user.send_tl("requestAcceptedAll", crate::args![count]);
}

pub fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() != 1 {
        return Some(Vec::new());
    }
    let mut options = user.get_pending_tpa_keys();
    options.push("*".to_owned());
    Some(options)
}

fn handle_teleport(user: &User, request: Option<TpaRequest>, label: &str) -> EssResult {
    let request = request.ok_or_else(|| no_request())?;
    let requester = ess().get_user_by_uuid(&request.uuid).filter(User::is_online);
    let Some(requester) = requester else {
        user.remove_tpa_request(&request.name);
        return Err(no_request());
    };
    let differs = user.get_world() != requester.get_world() && settings().is_world_teleport_permissions();
    let lacks = |world: Option<horizon_bukkit::World>| world.is_some_and(|world| !user.is_authorized(&format!("essentials.worlds.{}", world.get_name())));
    fail_if!(request.here && ((!requester.is_authorized("essentials.tpahere") && !requester.is_authorized("essentials.tpaall")) || (differs && lacks(user.get_world()))), no_request());
    fail_if!(!request.here && (!requester.is_authorized("essentials.tpa") || (differs && lacks(requester.get_world()))), no_request());
    let charge = Trade::of_command("tpaccept");
    requester.send_tl("requestAcceptedFrom", crate::args![user.get_display_name()]);
    let future = get_new_exception_future(&requester.get_source(), label);
    let told = user.clone();
    future.exceptionally(move |_| told.send_tl("pendingTeleportCancelled", &[]));
    let teleport = requester.get_async_teleport();
    teleport.set_tp_type(TeleportType::Tpa);
    if request.here {
        let location = request.location.clone().ok_or_else(|| no_request())?;
        let (told, place) = (requester.clone(), location.clone());
        future.then_accept(move |success| {
            if success {
                told.send_tl("teleporting", &teleporting_args(&place));
            }
        });
        teleport.teleport_player(user, &location, Some(&charge), "COMMAND", future);
    } else if let Some(base) = user.player() {
        teleport.teleport_player_target(&base, Some(&charge), "COMMAND", future);
    }
    user.remove_tpa_request(&request.name);
    Ok(())
}
