//! `Commandtop`.
use crate::prelude::*;
use crate::utils::location::get_safe_destination;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("top") };

fn run(user: &User, label: &str, _args: &[String]) -> EssResult {
    to_height(user, label, "top", "teleportTop", |world| f64::from(world.get_max_height()))
}

/// `/top` and `/bottom`: straight up or down, to the nearest safe place from that height.
pub fn to_height(user: &User, label: &str, command: &str, message: &'static str, height: fn(&World) -> f64) -> EssResult {
    let at = user.get_location().ok_or_else(EssError::player_not_found)?;
    let world = at.get_world().clone();
    let unsafe_place = Location::new(world.clone(), f64::from(at.get_block_x()), height(&world), f64::from(at.get_block_z()), at.get_yaw(), at.get_pitch());
    let safe = get_safe_destination(&unsafe_place)?;
    let future = get_new_exception_future(&user.get_source(), label);
    let (told, place) = (user.clone(), safe.clone());
    future.then_accept(move |success| {
        if success {
            told.send_tl(message, &teleporting_args(&place));
        }
    });
    user.get_async_teleport().teleport(&safe, Some(&Trade::of_command(command)), "COMMAND", future);
    Ok(())
}
