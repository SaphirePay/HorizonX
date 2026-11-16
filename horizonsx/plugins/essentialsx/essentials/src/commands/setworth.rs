//! `Commandsetworth`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), ..Command::new("setworth") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    let (stack, price) = match args {
        [] => return Err(EssError::not_enough()),
        [price] => (user.get_item_in_hand().unwrap_or_else(inventories::air), price),
        [item, price, ..] => (item_db().get(item)?, price),
    };
    ess().worth().set_price(&stack, parse_double(price)?);
    user.send_tl("worthSet", &[]);
    Ok(())
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    ess().worth().set_price(&item_db().get(&args[0])?, parse_double(&args[1])?);
    sender.send_tl("worthSet", &[]);
    Ok(())
}
