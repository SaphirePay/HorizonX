//! `Commandplaytime`.
use crate::prelude::*;
use crate::adventure::legacy_to_mini;
use crate::ess::{play_one_tick, play_one_tick_of};

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("playtime") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let (display, playtime, key) = if !args.is_empty() && sender.is_authorized("essentials.playtime.others") {
        match get_player(sender, args, 0) {
            Ok(user) => (user.get_display_name(), play_one_tick_of(&user), "playtimeOther"),
            Err(error) if error.is_player_not_found() => {
                let user = get_player_raw(None, args, 0, true, true)?;
                let mut playtime = play_one_tick_of(&user);
                if user.is_online() && user.is_vanished() {
                    playtime -= (now_ms() - user.get_last_vanish_time()) / 50;
                }
                (user.get_name(), playtime, "playtimeOther")
            }
            Err(error) => return Err(error),
        }
    } else if let Some(player) = sender.get_player() {
        (sender.get_display_name(), play_one_tick(player), "playtime")
    } else {
        return Err(EssError::not_enough());
    };
    sender.send_tl(key, crate::args![format_date_diff(now_ms() - playtime * 50), parsed(legacy_to_mini(&display))]);
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && sender.is_authorized("essentials.playtime.others") { get_players(sender) } else { Vec::new() })
}
