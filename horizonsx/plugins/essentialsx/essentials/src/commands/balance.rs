//! `Commandbalance`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("balance") };

fn other_line(target: &User) -> Vec<crate::i18n::Arg> {
    crate::args![if target.is_hidden() { target.get_name() } else { target.get_display_name() }, parsed(display_currency(&target.get_money()))].to_vec()
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    sender.send_tl("balanceOther", &other_line(&get_player_raw(None, args, 0, true, true)?));
    Ok(())
}

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    if args.len() == 1 && user.is_authorized("essentials.balance.others") {
        user.send_tl("balanceOther", &other_line(&get_player_raw(None, args, 0, false, true)?));
    } else if args.len() < 2 {
        user.send_tl("balance", crate::args![parsed(display_currency(&user.get_money()))]);
    } else {
        return Err(EssError::not_enough());
    }
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && sender.is_authorized("essentials.balance.others") { get_players(sender) } else { Vec::new() })
}
