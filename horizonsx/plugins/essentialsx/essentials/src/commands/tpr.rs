//! `Commandtpr`. `UserRandomTeleportEvent` isn't fired: nothing here can listen.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("tpr") };

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    let charge = Trade::of_command("tpr");
    charge.is_affordable_for(user)?;
    let teleport = ess().random_teleport();
    let (name, target) = if let Some(first) = args.first() {
        fail_if!(!teleport.has_location(first), EssError::tl("tprNotExist", &[]));
        fail_if!(teleport.is_per_location_permission() && !user.is_authorized(&format!("essentials.tpr.location.{first}")), EssError::tl("tprNoPermission", &[]));
        let target = if args.len() > 1 && user.is_authorized("essentials.tpr.others") { get_player_for(user, args, 1)? } else { user.clone() };
        (first.clone(), target)
    } else {
        let world = user.get_location().map(|at| at.world.get_name().to_owned()).unwrap_or_default();
        (teleport.get_default_location().replace("{world}", &world), user.clone())
    };
    target.send_tl("tprSuccess", &[]);
    if target != *user {
        user.send_tl("tprOtherUser", crate::args![target.get_display_name()]);
    }
    let (source, label) = (user.get_source(), label.to_owned());
    teleport.get_random_location(&name, move |location| {
        let future = get_new_exception_future(&source, &label);
        let done = target.clone();
        future.then_accept(move |success| {
            if success {
                done.send_tl("tprSuccessDone", &[]);
            }
        });
        target.get_async_teleport().teleport(&location, Some(&charge), "COMMAND", future);
    });
    Err(EssError::NoCharge)
}

fn run_source(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let teleport = ess().random_teleport();
    let target = get_player(sender, args, 1)?;
    fail_if!(!teleport.has_location(&args[0]), EssError::tl("tprNotExist", &[]));
    target.send_tl("tprSuccess", &[]);
    sender.send_tl("tprOtherUser", crate::args![target.get_display_name()]);
    let (source, label) = (sender.clone(), label.to_owned());
    teleport.get_random_location(&args[0], move |location| {
        let future = get_new_exception_future(&source, &label);
        let done = target.clone();
        future.then_accept(move |success| {
            if success {
                done.send_tl("tprSuccessDone", &[]);
            }
        });
        target.get_async_teleport().now(&location, false, "COMMAND", future);
    });
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let teleport = ess().random_teleport();
    Some(match args.len() {
        1 => {
            let mut list = teleport.list_locations();
            if teleport.is_per_location_permission() {
                list.retain(|name| sender.is_authorized(&format!("essentials.tpr.location.{name}")));
            }
            list
        }
        2 if sender.is_authorized("essentials.tpr.others") => get_players(sender),
        _ => Vec::new(),
    })
}
