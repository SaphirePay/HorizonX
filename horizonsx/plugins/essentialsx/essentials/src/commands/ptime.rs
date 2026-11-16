//! `Commandptime`.
use crate::prelude::*;
use crate::utils::tick_format;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("ptime") };

const GET_ALIASES: [&str; 4] = ["get", "list", "show", "display"];

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    if args.first().is_none_or(|first| GET_ALIASES.contains(&first.to_lowercase().as_str())) {
        if let Some(target) = args.get(1) {
            if target == "*" || target == "**" {
                sender.send_tl("pTimePlayers", &[]);
            }
            return loop_online_players(sender, false, true, target, |player| {
                get_user_time(sender, player);
                Ok(())
            });
        }
        if args.len() == 1 || sender.is_player() {
            let user = sender.get_user().ok_or_else(EssError::not_enough)?;
            get_user_time(sender, &user);
            return Ok(());
        }
        if ess().get_online_players().len() > 1 {
            sender.send_tl("pTimePlayers", &[]);
        }
        for player in ess().get_online_users() {
            get_user_time(sender, &player);
        }
    }
    if args.len() > 1 && !sender.is_authorized("essentials.ptime.others") && !args[1].eq_ignore_ascii_case(&sender.get_self_selector()) {
        sender.send_tl("pTimeOthersPermission", &[]);
        return Ok(());
    }
    let first = args.first().ok_or_else(|| EssError::Message("Index 0 out of bounds for length 0".to_owned()))?;
    let fixed = first.starts_with('@');
    let time = first.strip_prefix('@').unwrap_or(first);
    let ticks = if tick_format::means_reset(time) { None } else { Some(tick_format::parse(time).ok_or_else(EssError::not_enough)?) };
    let selector = args.get(1).cloned().unwrap_or_else(|| sender.get_self_selector());
    let mut names = Vec::new();
    loop_online_players(sender, false, true, &selector, |player| {
        set_user_time(player, ticks, !fixed);
        names.push(player.get_name());
        Ok(())
    })?;
    let names = names.join(", ");
    match ticks {
        None => sender.send_tl("pTimeReset", crate::args![names]),
        Some(ticks) => sender.send_tl(if fixed { "pTimeSetFixed" } else { "pTimeSet" }, crate::args![parsed(tick_format::format(ticks)), names]),
    }
    Ok(())
}

fn get_user_time(sender: &CommandSource, user: &User) {
    let Some(player) = user.player() else { return };
    if player.get_player_time_offset() == 0 {
        sender.send_tl("pTimeNormal", crate::args![user.get_name()]);
        return;
    }
    let time = tick_format::format(player.get_player_time());
    sender.send_tl(if player.is_player_time_relative() { "pTimeCurrent" } else { "pTimeCurrentFixed" }, crate::args![user.get_name(), parsed(time)]);
}

fn set_user_time(user: &User, ticks: Option<i64>, relative: bool) {
    let Some(player) = user.player() else { return };
    let Some(ticks) = ticks else {
        player.reset_player_time();
        return;
    };
    let mut time = player.get_player_time();
    time -= time % 24_000;
    time += 24_000 + ticks;
    if relative {
        time -= player.get_world().get_time();
    }
    player.set_player_time(time, relative);
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => ["get", "reset", "sunrise", "day", "morning", "noon", "afternoon", "sunset", "night", "midnight"].map(str::to_owned).to_vec(),
        2 if GET_ALIASES.contains(&args[0].as_str()) || sender.get_user().is_none_or(|user| user.is_authorized("essentials.ptime.others")) => get_players(sender),
        _ => Vec::new(),
    })
}
