//! `Commandbottom`.
use super::top::to_height;
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("bottom") };

fn run(user: &User, label: &str, _args: &[String]) -> EssResult {
    to_height(user, label, "bottom", "teleportBottom", |world| f64::from(world.get_min_height()))
}
