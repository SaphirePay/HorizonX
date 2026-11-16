//! `Commandignore`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_source: Some(tab_player_first), ..Command::new("ignore") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    if args.is_empty() {
        let names: Vec<String> = user.get_ignored_players().iter().filter_map(|uuid| ess().get_user_by_uuid(uuid)).map(|known| known.get_name()).filter(|name| !name.trim().is_empty()).collect();
        let list = names.join(" ");
        user.send_tl(if list.is_empty() { "noIgnored" } else { "ignoredList" }, crate::args![list]);
        return Ok(());
    }
    let player = get_player_raw(None, args, 0, false, true)?;
    if player.is_ignore_exempt() {
        user.send_tl("ignoreExempt", &[]);
    } else if user.is_ignored_player(&player) {
        user.set_ignored_player(&player, false);
        user.send_tl("unignorePlayer", crate::args![player.get_name()]);
    } else if user.get_uuid() == player.get_uuid() {
        user.send_tl("ignoreYourself", &[]);
    } else {
        user.set_ignored_player(&player, true);
        user.send_tl("ignorePlayer", crate::args![player.get_name()]);
    }
    Ok(())
}
