//! `Commandafk`.
use crate::prelude::*;
use crate::user::AfkCause;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("afk") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    if !args.is_empty() && user.is_authorized("essentials.afk.others") {
        // No such player, but words: they're the sender's own message.
        let (target, message) = match get_player_for(user, args, 0) {
            Ok(target) => (target, (args.len() > 1).then(|| get_final_arg(args, 1))),
            Err(EssError::Tl { key, .. }) if key == "playerNotFound" => (user.clone(), Some(get_final_arg(args, 0))),
            Err(error) => return Err(error),
        };
        toggle_afk(Some(user), &target, message)
    } else {
        toggle_afk(Some(user), user, (!args.is_empty()).then(|| get_final_arg(args, 0)))
    }
}

fn run_source(_sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    toggle_afk(None, &get_player_raw(None, args, 0, true, false)?, (args.len() > 1).then(|| get_final_arg(args, 1)))
}

fn toggle_afk(sender: Option<&User>, user: &User, message: Option<String>) -> EssResult {
    if let Some(sender) = sender.filter(|_| message.is_some()) {
        fail_if!(sender.is_muted(), muted_error(sender));
        fail_if!(!sender.is_authorized("essentials.afk.message"), EssError::tl("noPermToAFKMessage", &[]));
    }
    user.set_display_nick();
    let before = user.is_afk();
    let after = user.toggle_afk(AfkCause::Command);
    if before == after {
        return Ok(());
    }
    let (mut key, mut self_key) = ("", "");
    if after {
        if !user.is_hidden() {
            (key, self_key) = if message.is_some() { ("userIsAwayWithMessage", "userIsAwaySelfWithMessage") } else { ("userIsAway", "userIsAwaySelf") };
        }
        user.set_afk_message(message.clone());
    } else {
        if !user.is_hidden() {
            (key, self_key) = ("userIsNotAway", "userIsNotAwaySelf");
        }
        user.update_activity(false, AfkCause::Command);
    }
    let args = [Arg::from(user.get_display_name()), Arg::from(message)];
    if !key.is_empty() && settings().broadcast_afk_message() {
        let me = user.clone();
        ess().broadcast_tl(Some(user), Some(&move |other: &User| *other == me), false, key, &args);
    }
    if !self_key.is_empty() {
        user.send_tl(self_key, &args);
    }
    user.set_display_nick();
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && sender.is_authorized("essentials.afk.others") { get_players(sender) } else { Vec::new() })
}
