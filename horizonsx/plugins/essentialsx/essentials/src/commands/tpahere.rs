//! `Commandtpahere`.
use super::tpa::{requested_player, send_request};
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_source: Some(tab_player_first), ..Command::new("tpahere") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let player = requested_player(user, args, true)?;
    send_request(user, &player, true);
    user.send_tl("typeTpacancel", &[]);
    Ok(())
}
