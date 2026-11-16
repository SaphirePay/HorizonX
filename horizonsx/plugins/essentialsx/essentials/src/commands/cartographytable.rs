//! `Commandcartographytable`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("cartographytable") };

fn run(user: &User, _label: &str, _args: &[String]) -> EssResult {
    if let Some(player) = user.player() {
        crate::inventories::after_turn(move || {
            player.open_cartography_table(None, true);
        });
    }
    Ok(())
}
