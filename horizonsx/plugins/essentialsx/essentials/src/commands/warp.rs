//! `Commandwarp`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("warp") };

const WARPS_PER_PAGE: usize = 20;

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    if args.first().is_none_or(|first| is_digits(first)) {
        fail_if!(!user.is_authorized("essentials.warp.list"), EssError::tl("warpListPermission", &[]));
        warp_list(&user.get_source(), args, Some(user))?;
        return Err(EssError::NoCharge);
    }
    if args.len() == 2 && (user.is_authorized("essentials.warp.otherplayers") || user.is_authorized("essentials.warp.others")) {
        let other = get_player_raw(Some(user), args, 1, user.can_interact_vanished(), false)?;
        warp_user(user, &other, &args[0], label)?;
        return Err(EssError::NoCharge);
    }
    warp_user(user, user, &args[0], label)?;
    Err(EssError::NoCharge)
}

fn run_source(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    if args.len() < 2 || is_int(&args[0]) {
        warp_list(sender, args, None)?;
        return Err(EssError::NoCharge);
    }
    let other = get_player_raw(None, args, 1, true, false)?;
    other.get_async_teleport().warp(&other, &args[0], None, "COMMAND", get_new_exception_future(sender, label));
    Err(EssError::NoCharge)
}

fn warp_list(sender: &CommandSource, args: &[String], user: Option<&User>) -> EssResult {
    let names = available_warps_for(user);
    fail_if!(names.is_empty(), EssError::tl("noWarpsDefined", &[]));
    let mut page = args.first().filter(|first| is_int(first)).and_then(|first| first.parse::<i32>().ok()).unwrap_or(1);
    let max_pages = names.len().div_ceil(WARPS_PER_PAGE) as i32;
    if page > max_pages {
        page = max_pages;
    }
    let start = (page - 1) as usize * WARPS_PER_PAGE;
    let list = join_list(&names[start..start + (names.len() - start).min(WARPS_PER_PAGE)]);
    if names.len() > WARPS_PER_PAGE {
        sender.send_tl("warpsCount", crate::args![names.len(), page, max_pages]);
        sender.send_tl("warpList", crate::args![list]);
    } else {
        sender.send_tl("warps", crate::args![list]);
    }
    Ok(())
}

fn warp_user(owner: &User, user: &User, name: &str, label: &str) -> EssResult {
    let charge_warp = Trade::of_command(&format!("warp-{}", name.to_lowercase().replace('_', "-")));
    let charge_command = Trade::of_command("warp");
    let charge = Trade::of_money(charge_warp.get_command_cost(user).add(&charge_command.get_command_cost(user)));
    charge.is_affordable_for(owner)?;
    fail_if!(settings().get_per_warp_permission() && !owner.is_authorized(&format!("essentials.warps.{name}")), EssError::tl("warpUsePermission", &[]));
    owner.get_async_teleport().warp(user, name, Some(&charge), "COMMAND", get_new_exception_future(&user.get_source(), label));
    Ok(())
}

fn available_warps_for(user: Option<&User>) -> Vec<String> {
    let all = ess().warps().get_list();
    match user.filter(|_| settings().get_per_warp_permission()) {
        Some(user) => all.into_iter().filter(|warp| user.is_authorized(&format!("essentials.warps.{warp}"))).collect(),
        None => all,
    }
}

fn tab_user(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && user.is_authorized("essentials.warp.list") {
        available_warps_for(Some(user))
    } else if args.len() == 2 && (user.is_authorized("essentials.warp.otherplayers") || user.is_authorized("essentials.warp.others")) {
        get_players(&user.get_source())
    } else {
        Vec::new()
    })
}

fn tab_source(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => ess().warps().get_list(),
        2 => get_players(sender),
        _ => Vec::new(),
    })
}
