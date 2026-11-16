//! `Commanddisposal`.
use crate::prelude::*;
use horizon_bukkit::Inventory;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("disposal") };

fn run(user: &User, _label: &str, _args: &[String]) -> EssResult {
    user.send_tl("openingDisposal", &[]);
    if let Some(player) = user.player() {
        player.open_inventory(&Inventory::create(36, &user.player_tl("disposal", &[])));
    }
    Ok(())
}
