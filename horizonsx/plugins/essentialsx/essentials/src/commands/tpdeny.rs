//! `Commandtpdeny`.
use super::tpaccept::tab;
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("tpdeny") };

fn no_request() -> EssError {
    EssError::tl("noPendingRequest", &[])
}

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let deny_all = args.first().is_some_and(|first| first == "*" || first.eq_ignore_ascii_case("all"));
    fail_if!(!user.has_pending_tpa_requests(false, false), no_request());
    if deny_all {
        deny_all_requests(user);
        return Ok(());
    }
    let request = if args.is_empty() {
        user.get_next_tpa_request(false, true, false)
    } else {
        user.get_outstanding_tpa_request(&get_player_for(user, args, 0)?.get_name(), false)
    };
    let request = request.ok_or_else(no_request)?;
    let player = ess().get_user_by_uuid(&request.uuid).filter(User::is_online).ok_or_else(no_request)?;
    user.send_tl("requestDenied", &[]);
    player.send_tl("requestDeniedFrom", crate::args![user.get_display_name()]);
    user.remove_tpa_request(&request.name);
    Ok(())
}

fn deny_all_requests(user: &User) {
    let mut count = 0;
    while let Some(request) = user.get_next_tpa_request(false, true, false) {
        if let Some(player) = ess().get_user_by_uuid(&request.uuid).filter(User::is_online) {
            player.send_tl("requestDeniedFrom", crate::args![user.get_display_name()]);
        }
        user.remove_tpa_request(&request.name);
        count += 1;
    }
    user.send_tl("requestDeniedAll", crate::args![count]);
}
