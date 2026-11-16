//! `Commandpweather`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_user: Some(tab), ..Command::new("pweather") };

const GET_ALIASES: [&str; 4] = ["get", "list", "show", "display"];

fn weather_of(alias: &str) -> Option<&'static str> {
    match alias {
        "sun" | "clear" => Some("CLEAR"),
        "storm" | "thunder" => Some("DOWNFALL"),
        _ => None,
    }
}

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    if args.first().is_none_or(|first| GET_ALIASES.contains(&first.to_lowercase().as_str())) {
        if let Some(target) = args.get(1) {
            if target == "*" || target == "**" {
                sender.send_tl("pWeatherPlayers", &[]);
            }
            return loop_online_players(sender, false, true, target, |player| {
                get_user_weather(sender, player);
                Ok(())
            });
        }
        if args.len() == 1 || sender.is_player() {
            let user = sender.get_user().ok_or_else(EssError::not_enough)?;
            get_user_weather(sender, &user);
            return Ok(());
        }
        if ess().get_online_players().len() > 1 {
            sender.send_tl("pWeatherPlayers", &[]);
        }
        for player in ess().get_online_users() {
            get_user_weather(sender, &player);
        }
    }
    if args.len() > 1 && !sender.is_authorized("essentials.pweather.others") && !args[1].eq_ignore_ascii_case(&sender.get_self_selector()) {
        sender.send_tl("pWeatherOthersPermission", &[]);
        return Ok(());
    }
    let first = args.first().ok_or_else(|| EssError::Message("Index 0 out of bounds for length 0".to_owned()))?;
    let weather = first.to_lowercase();
    fail_if!(weather_of(&weather).is_none() && weather != "reset", EssError::NotEnoughArguments(sender.tl("pWeatherInvalidAlias", &[])));
    let selector = args.get(1).cloned().unwrap_or_else(|| sender.get_self_selector());
    let mut names = Vec::new();
    loop_online_players(sender, false, true, &selector, |player| {
        if let Some(base) = player.player() {
            match weather_of(&weather) {
                Some(kind) => base.set_player_weather(kind),
                None => base.reset_player_weather(),
            }
        }
        names.push(player.get_name());
        Ok(())
    })?;
    if weather == "reset" {
        sender.send_tl("pWeatherReset", crate::args![names.join(", ")]);
    } else {
        sender.send_tl("pWeatherSet", crate::args![weather, names.join(", ")]);
    }
    Ok(())
}

fn get_user_weather(sender: &CommandSource, user: &User) {
    let Some(player) = user.player() else { return };
    match player.get_player_weather() {
        None => sender.send_tl("pWeatherNormal", crate::args![user.get_name()]),
        Some(weather) => sender.send_tl("pWeatherCurrent", crate::args![user.get_name(), weather.to_lowercase()]),
    }
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => ["get", "reset", "storm", "sun"].map(str::to_owned).to_vec(),
        2 if GET_ALIASES.contains(&args[0].as_str()) || user.is_authorized("essentials.pweather.others") => get_players(&user.get_source()),
        _ => Vec::new(),
    })
}
