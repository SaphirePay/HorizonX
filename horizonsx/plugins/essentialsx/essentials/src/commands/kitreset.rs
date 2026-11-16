//! `Commandkitreset`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("kitreset") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    let kit = args.first().ok_or_else(EssError::not_enough)?;
    fail_if!(ess().kits().get_kit(kit).is_none(), EssError::tl("kitNotFound", &[]));
    let target = if args.len() > 1 && user.is_authorized("essentials.kitreset.others") { get_player_for(user, args, 1)? } else { user.clone() };
    target.set_kit_timestamp(kit, 0);
    if *user == target {
        user.send_tl("kitReset", crate::args![kit]);
    } else {
        user.send_tl("kitResetOther", crate::args![kit, target.get_display_name()]);
    }
    Ok(())
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    fail_if!(ess().kits().get_kit(&args[0]).is_none(), EssError::tl("kitNotFound", &[]));
    let target = get_player(sender, args, 1)?;
    target.set_kit_timestamp(&args[0], 0);
    sender.send_tl("kitResetOther", crate::args![&args[0], target.get_display_name()]);
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => ess().kits().get_kit_keys(),
        2 if sender.is_authorized("essentials.kitreset.others") => get_players(sender),
        _ => Vec::new(),
    })
}
