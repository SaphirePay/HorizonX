//! `Commandfeed`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("feed") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    if !user.is_authorized("essentials.feed.cooldown.bypass") {
        user.heal_cooldown()?;
    }
    let source = user.get_source();
    if let Some(first) = args.first().filter(|_| user.is_authorized("essentials.feed.others")) {
        return loop_online_players(&source, true, true, first, |target| update_player(&source, target));
    }
    feed_user(user)?;
    user.send_tl("feed", &[]);
    Ok(())
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    loop_online_players(sender, true, true, first, |target| update_player(sender, target))
}

fn update_player(sender: &CommandSource, user: &User) -> EssResult {
    feed_user(user)?;
    sender.send_tl("feedOther", crate::args![user.get_display_name()]);
    Ok(())
}

/// `feedPlayer`: `FoodLevelChangeEvent` comes first, and Essentials' own listener cancels it in god mode.
fn feed_user(user: &User) -> EssResult {
    fail_if!(user.is_god_mode_enabled(), EssError::QuietAbort);
    if let Some(player) = user.player() {
        feed_player(&player);
    }
    Ok(())
}

fn feed_player(player: &Player) {
    player.set_food_level(20);
    player.set_saturation(10.0);
    player.set_exhaustion(0.0);
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && sender.is_authorized("essentials.feed.others") { get_players_with_wildcards(sender) } else { Vec::new() })
}
