//! `Commandtpoffline`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("tpoffline") };

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let target = get_player_raw(None, args, 0, true, true)?;
    let Some(logout) = target.get_logout_location() else {
        user.send_tl("teleportOfflineUnknown", crate::args![target.get_display_name()]);
        return Err(EssError::NoCharge);
    };
    check_world_permission(user, user.get_world().as_ref() != Some(logout.get_world()), logout.get_world())?;
    user.send_tl("teleporting", &teleporting_args(&logout));
    user.get_async_teleport().now(&logout, false, "COMMAND", get_new_exception_future(&user.get_source(), label));
    Ok(())
}
