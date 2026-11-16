//! `Commandgamemode`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("gamemode") };

const STANDARD_OPTIONS: [&str; 5] = ["creative", "survival", "adventure", "spectator", "toggle"];

/// `matchGameMode`: `None` is a toggle.
fn match_game_mode(text: &str) -> EssResult<Option<&'static str>> {
    let mode = text.to_lowercase();
    let any = |words: &[&str], part: &str| words.contains(&mode.as_str()) || mode.contains(part);
    Ok(Some(if any(&["gmc", "egmc", "1", "c"], "creat") {
        "CREATIVE"
    } else if any(&["gms", "egms", "0", "s"], "survi") {
        "SURVIVAL"
    } else if any(&["gma", "egma", "2", "a"], "advent") {
        "ADVENTURE"
    } else if any(&["gmsp", "egmsp", "3", "sp"], "spec") {
        "SPECTATOR"
    } else if matches!(mode.as_str(), "gmt" | "egmt" | "t") || mode.contains("toggle") || mode.contains("cycle") {
        return Ok(None);
    } else {
        return Err(EssError::not_enough());
    }))
}

fn is_prohibited_change(user: Option<&User>, to: &str) -> bool {
    user.is_some_and(|user| !user.is_authorized("essentials.gamemode.all") && !user.is_authorized(&format!("essentials.gamemode.{}", to.to_lowercase())))
}

fn set_user_gamemode(sender: &CommandSource, mode: Option<&'static str>, user: &User) -> EssResult {
    let mode = mode.ok_or_else(|| EssError::NotEnoughArguments(sender.tl("gameModeInvalid", &[])))?;
    if sender.is_player() && is_prohibited_change(sender.get_user().as_ref(), mode) {
        sender.send_tl("cantGamemode", crate::args![mode]);
        return Ok(());
    }
    if let Some(player) = user.player() {
        player.set_game_mode(mode);
    }
    sender.send_tl("gameMode", crate::args![sender.tl(&mode.to_lowercase(), &[]), user.get_display_name()]);
    Ok(())
}

fn run_source(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    match args {
        [] => Err(EssError::not_enough()),
        [only] => loop_online_players(sender, false, true, only, |user| set_user_gamemode(sender, match_game_mode(label)?, user)),
        [mode, who] => loop_online_players(sender, false, true, who, |user| set_user_gamemode(sender, match_game_mode(mode)?, user)),
        _ => Ok(()),
    }
}

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    let source = user.get_source();
    let mode = if args.is_empty() {
        match_game_mode(label)?
    } else if args.len() > 1 && args[1].trim().chars().count() > 2 && user.is_authorized("essentials.gamemode.others") {
        return loop_online_players(&source, false, true, &args[1], |player| set_user_gamemode(&source, match_game_mode(&args[0])?, player));
    } else {
        match match_game_mode(&args[0]) {
            Ok(mode) => mode,
            Err(_) if user.is_authorized("essentials.gamemode.others") => return loop_online_players(&source, false, true, &args[0], |player| set_user_gamemode(&source, match_game_mode(label)?, player)),
            Err(error) => return Err(error),
        }
    };
    let Some(player) = user.player() else { return Ok(()) };
    let mode = mode.unwrap_or(match player.get_game_mode() {
        "SURVIVAL" => "CREATIVE",
        "CREATIVE" => "ADVENTURE",
        _ => "SURVIVAL",
    });
    if is_prohibited_change(Some(user), mode) {
        user.send_tl("cantGamemode", crate::args![user.player_tl(&mode.to_lowercase(), &[])]);
        return Ok(());
    }
    player.set_game_mode(mode);
    user.send_tl("gameMode", crate::args![user.player_tl(&mode.to_lowercase(), &[]), user.get_display_name()]);
    Ok(())
}

fn tab_source(sender: &CommandSource, label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 if match_game_mode(label).is_ok() => get_players(sender),
        1 => STANDARD_OPTIONS.iter().map(|option| (*option).to_owned()).collect(),
        2 => get_players(sender),
        _ => Vec::new(),
    })
}

fn tab_user(user: &User, label: &str, args: &[String]) -> Option<Vec<String>> {
    let direct = match_game_mode(label).is_ok();
    let others = user.is_authorized("essentials.gamemode.others");
    Some(match args.len() {
        1 if others && direct => get_players(&user.get_source()),
        1 => STANDARD_OPTIONS.iter().map(|option| (*option).to_owned()).collect(),
        2 if others && !direct => get_players(&user.get_source()),
        _ => Vec::new(),
    })
}
