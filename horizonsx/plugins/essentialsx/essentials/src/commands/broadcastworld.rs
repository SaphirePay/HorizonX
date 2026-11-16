//! `Commandbroadcastworld`.
use crate::prelude::*;
use crate::adventure::{escape_tags, legacy_to_mini, legacy_to_mini_with_urls};

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("broadcastworld") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let Some(mut world) = user.get_world() else { return Ok(()) };
    let mut message = get_final_arg(args, 0);
    if settings().is_allow_world_in_broadcastworld()
        && let Some(named) = ess().get_world(&args[0])
    {
        world = named;
        message = get_final_arg(args, 1);
    }
    send_broadcast(&world, &user.get_display_name(), &message)
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let world = ess().get_world(&args[0]).ok_or_else(|| EssError::tl("invalidWorld", &[]))?;
    send_broadcast(&world, sender.get_sender().get_name(), &get_final_arg(args, 1))
}

fn send_broadcast(world: &World, name: &str, message: &str) -> EssResult {
    fail_if!(message.is_empty(), EssError::not_enough());
    let formatted = replace_format(message).replace("\\n", "\n");
    let exclude = |user: &User| user.get_world().as_ref() != Some(world);
    ess().broadcast_tl(None, Some(&exclude), true, "broadcast", crate::args![parsed(legacy_to_mini_with_urls(&escape_tags(&formatted))), parsed(legacy_to_mini(name))]);
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && (!sender.is_player() || settings().is_allow_world_in_broadcastworld()) { get_worlds().iter().map(|world| world.get_name().to_owned()).collect() } else { Vec::new() })
}
