//! `Commandme`.
use crate::prelude::*;
use crate::utils::format::format_message;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab_nothing), ..Command::new("me") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    fail_if!(user.is_muted(), muted_error(user));
    fail_if!(args.is_empty(), EssError::not_enough());
    let message = format_message(user, "essentials.chat", &get_final_arg(args, 0));
    user.set_display_nick();
    let radius = i64::from(settings().get_chat_radius());
    if radius < 1 {
        ess().broadcast_tl(None, None, false, "action", crate::args![user.get_display_name(), message.clone()]);
        crate::hooks::fire(crate::hooks::Hook::UserAction { user, message: &message });
        return Ok(());
    }
    let radius = radius * radius;
    let Some(place) = user.get_location() else { return Ok(()) };
    let mut out = Vec::new();
    for online in ess().get_online_users() {
        if online == *user {
            out.push(online);
            continue;
        }
        let near = online.get_location().is_some_and(|at| at.get_world() == place.get_world() && !online.is_ignored_player(user) && at.distance_squared(&place) <= radius as f64);
        if near || online.is_authorized("essentials.chat.spy") {
            out.push(online);
        }
    }
    if out.len() < 2 {
        user.send_tl("localNoOne", &[]);
    }
    for online in out {
        online.send_tl("action", crate::args![user.get_display_name(), message.clone()]);
    }
    crate::hooks::fire(crate::hooks::Hook::UserAction { user, message: &message });
    Ok(())
}

fn run_source(_sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    ess().broadcast_tl(None, None, false, "action", crate::args!["@", replace_format(&get_final_arg(args, 0))]);
    Ok(())
}
