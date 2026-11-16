//! `Commandrenamehome`. `HomeModifyEvent` isn't fired: nothing here can listen.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_source: Some(tab), ..Command::new("renamehome") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let mut users_home = user.clone();
    let (old_name, new_name) = match args {
        [first, second] => {
            let new_name = second.to_lowercase();
            if let Some((player, home)) = first.split_once(':') {
                if user.is_authorized("essentials.renamehome.others") {
                    users_home = get_player_term_raw(player, true, true)?;
                }
                (home.to_lowercase(), new_name)
            } else {
                (first.to_lowercase(), new_name)
            }
        }
        [player, old_name, new_name] => {
            fail_if!(!user.is_authorized("essentials.renamehome.others"), EssError::not_enough());
            users_home = get_player_term_raw(player, true, true)?;
            (old_name.to_lowercase(), new_name.to_lowercase())
        }
        _ => return Err(EssError::not_enough()),
    };
    fail_if!(new_name == "bed" || is_int(&new_name) || old_name == "bed" || is_int(&old_name), EssError::tl("invalidHomeName", &[]));
    users_home.rename_home(&old_name, &new_name)?;
    user.send_tl("homeRenamed", crate::args![old_name, new_name]);
    users_home.set_last_home_confirmation(None);
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() != 1 {
        return Some(Vec::new());
    }
    let mut homes = sender.get_user().map(|user| user.get_homes()).unwrap_or_default();
    if sender.is_authorized("essentials.renamehome.others") {
        match args[0].split_once(':') {
            None => homes.extend(get_players(sender).into_iter().map(|player| format!("{player}:"))),
            Some((name, _)) => match get_player_term_raw(name, true, true) {
                Ok(other) => homes.extend(other.get_homes().into_iter().map(|home| format!("{name}:{home}"))),
                Err(_) => return Some(homes),
            },
        }
    }
    Some(homes)
}
