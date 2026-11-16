//! `Commandback`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab), ..Command::new("back") };

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    let sender = user.get_source();
    if !args.is_empty() && user.is_authorized("essentials.back.others") {
        return parse_others(&sender, args, label);
    }
    teleport_back(&sender, user, label)
}

fn run_source(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    parse_others(sender, args, label)
}

fn parse_others(sender: &CommandSource, args: &[String], label: &str) -> EssResult {
    let player = get_player_raw(None, args, 0, true, false)?;
    sender.send_tl("backOther", crate::args![player.get_name()]);
    teleport_back(sender, &player, label)
}

fn teleport_back(sender: &CommandSource, user: &User, label: &str) -> EssResult {
    let last = user.get_last_location().ok_or_else(|| EssError::tl("noLocationFound", &[]))?;
    let requester = sender.get_user();
    if let Some(requester) = &requester {
        check_world_permission(user, user.get_world().as_ref() != Some(last.get_world()), last.get_world())?;
        let node = format!("essentials.back.into.{}", last.get_world().get_name());
        fail_if!(!requester.is_authorized(&node), EssError::tl("noPerm", crate::args![node]));
    }
    let future = get_new_exception_future(sender, label);
    let teleport = user.get_async_teleport();
    match requester {
        None => teleport.back_by(None, None, future),
        Some(requester) if requester != *user => {
            let charge = Trade::of_command("back");
            charge.is_affordable_for(&requester)?;
            teleport.back_by(Some(&requester), Some(&charge), future);
        }
        Some(_) => {
            let charge = Trade::of_command("back");
            charge.is_affordable_for(user)?;
            teleport.back(Some(&charge), future);
        }
    }
    Err(EssError::NoCharge)
}

fn tab(user: &User, label: &str, args: &[String]) -> Option<Vec<String>> {
    if user.is_authorized("essentials.back.others") { tab_player_first(&user.get_source(), label, args) } else { Some(Vec::new()) }
}
