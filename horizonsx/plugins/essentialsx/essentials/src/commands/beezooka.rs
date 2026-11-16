//! `Commandbeezooka`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("beezooka") };

fn run(user: &User, _label: &str, _args: &[String]) -> EssResult {
    let Some(player) = user.player() else { return Ok(()) };
    let eyes = player.get_eye_location();
    let Some(bee) = eyes.world.spawn(&eyes, "bee") else { return Ok(()) };
    let direction = eyes.get_direction();
    bee.set_velocity(direction.x * 2.0, direction.y * 2.0, direction.z * 2.0);
    ess().schedule_sync_delayed_task_later(
        move || {
            if let Some(at) = bee.get_location() {
                bee.remove();
                at.world.create_explosion(&at, 0.0);
            }
        },
        20,
    );
    Ok(())
}
