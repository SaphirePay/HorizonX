//! `Commandkittycannon`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("kittycannon") };

const CATS: [&str; 11] = ["tabby", "black", "red", "siamese", "british_shorthair", "calico", "persian", "ragdoll", "white", "jellie", "all_black"];

fn run(user: &User, _label: &str, _args: &[String]) -> EssResult {
    use std::hash::{BuildHasher, Hasher};
    let Some(player) = user.player() else { return Ok(()) };
    let eyes = player.get_eye_location();
    let Some(cat) = eyes.world.spawn(&eyes, "cat") else { return Ok(()) };
    let pick = std::collections::hash_map::RandomState::new().build_hasher().finish() as usize % CATS.len();
    cat.merge(&format!("{{variant:\"minecraft:{}\",Age:-24000,Owner:{}}}", CATS[pick], crate::mob_data::uuid_snbt(&player.get_unique_id())));
    let direction = eyes.get_direction();
    cat.set_velocity(direction.x * 2.0, direction.y * 2.0, direction.z * 2.0);
    ess().schedule_sync_delayed_task_later(
        move || {
            if let Some(at) = cat.get_location() {
                cat.remove();
                at.world.create_explosion(&at, 0.0);
            }
        },
        20,
    );
    Ok(())
}
