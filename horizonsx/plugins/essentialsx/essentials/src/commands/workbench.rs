//! `Commandworkbench`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("workbench") };

fn run(user: &User, _label: &str, _args: &[String]) -> EssResult {
    if let Some(player) = user.player() {
        crate::inventories::after_turn(move || {
            player.open_workbench(None, true);
        });
    }
    Ok(())
}
