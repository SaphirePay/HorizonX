//! `Commandantioch`.
use crate::prelude::*;
use crate::utils::location::get_target;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("antioch") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    if !args.is_empty() {
        ess().broadcast_message_from(user, "...lobbest thou thy Holy Hand Grenade of Antioch towards thy foe,");
        ess().broadcast_message_from(user, "who being naughty in My sight, shall snuff it.");
    }
    let player = user.player().ok_or_else(EssError::player_not_found)?;
    let target = get_target(&player)?;
    target.world.spawn(&target, "tnt");
    Ok(())
}
