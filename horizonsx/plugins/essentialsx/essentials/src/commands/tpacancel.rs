//! `Commandtpacancel`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("tpacancel") };

fn cancel_teleport_request(user: &User, requester: &User) -> bool {
    user.remove_tpa_request(&requester.get_name()).is_some()
}

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    if args.is_empty() {
        let cancellations = ess().get_online_users().iter().filter(|online| *online != user && cancel_teleport_request(online, user)).count();
        fail_if!(cancellations == 0, EssError::tl("noPendingRequest", &[]));
        user.send_tl("teleportRequestAllCancelled", crate::args![cancellations]);
    } else {
        let target = get_player_for(user, args, 0)?;
        if cancel_teleport_request(&target, user) {
            user.send_tl("teleportRequestSpecificCancelled", crate::args![target.get_name()]);
        }
    }
    Ok(())
}
