//! `Commandpaytoggle`.
use crate::prelude::*;

const OTHERS: &str = "essentials.paytoggle.others";

pub const COMMAND: Command = Command {
    run_user: Some(run_user),
    run_source: Some(|sender, _, args| toggle_other_players(sender, args, toggle_player)),
    tab_user: Some(|user, _, args| toggle_tab_user(user, args, OTHERS)),
    tab_source: Some(|sender, _, args| toggle_tab_source(sender, args)),
    ..Command::new("paytoggle")
};

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    if label.contains("payon") {
        toggle_player(&user.get_source(), user, Some(true))
    } else if label.contains("payoff") {
        toggle_player(&user.get_source(), user, Some(false))
    } else {
        handle_toggle_with_args(user, args, OTHERS, toggle_player)
    }
}

fn toggle_player(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult {
    let enabled = enabled.unwrap_or_else(|| !user.is_accepting_pay());
    user.set_accepting_pay(enabled);
    user.send_tl(if enabled { "payToggleOn" } else { "payToggleOff" }, &[]);
    if !is_self(sender, user) {
        sender.send_tl(if enabled { "payEnabledFor" } else { "payDisabledFor" }, crate::args![user.get_display_name()]);
    }
    Ok(())
}
