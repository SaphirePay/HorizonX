//! `Commanddelhome`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("delhome") };

fn delete_home(sender: &CommandSource, user: &User, home: &str) {
    match user.del_home(home) {
        Ok(()) => sender.send_tl("deleteHome", crate::args![home]),
        Err(_) => sender.send_tl("invalidHome", crate::args![home]),
    }
}

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let mut user = sender.get_user();
    // Both `/delhome khobbits house` and `/delhome khobbits:house`.
    let name_parts = java_split(first, ':');
    let expanded = if name_parts.first().map_or(0, String::len) != first.len() { name_parts } else { args.to_vec() };
    let name;
    if expanded.len() > 1 && user.as_ref().is_none_or(|user| user.is_authorized("essentials.delhome.others")) {
        user = Some(get_player_raw(None, &expanded, 0, true, true)?);
        name = expanded[1].to_lowercase();
    } else if user.is_none() {
        return Err(EssError::not_enough());
    } else {
        name = expanded.first().map_or_else(String::new, |only| only.to_lowercase());
    }
    let user = user.ok_or_else(EssError::not_enough)?;
    match name.as_str() {
        "bed" => return Err(EssError::tl("invalidHomeName", &[])),
        "*" => {
            for home in user.get_homes() {
                delete_home(sender, &user, &home);
            }
        }
        _ => delete_home(sender, &user, &name),
    }
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let [first] = args else {
        return Some(Vec::new());
    };
    let mut homes = sender.get_user().map_or_else(Vec::new, |user| user.get_homes());
    if sender.is_authorized("essentials.delhome.others") {
        match first.split_once(':') {
            None => homes.extend(get_players(sender).into_iter().map(|player| format!("{player}:"))),
            Some((name, _)) => {
                let Ok(other) = get_player_raw(None, &[name.to_owned()], 0, true, true) else { return Some(homes) };
                homes.extend(other.get_homes().into_iter().map(|home| format!("{name}:{home}")));
                homes.push(format!("{name}:*"));
            }
        }
    }
    Some(homes)
}
