//! `Commandtime`.
use crate::prelude::*;
use crate::utils::tick_format;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("time") };

fn ticks_of(text: &str) -> EssResult<i64> {
    let desc = if is_int(text) { format!("{text}t") } else { text.to_owned() };
    tick_format::parse(&desc).ok_or_else(EssError::not_enough)
}

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    let mut add = false;
    let (time_tick, worlds) = match args {
        [] => {
            let worlds = get_selected_worlds(sender, None)?;
            if label.ends_with("day") || label.ends_with("night") {
                (tick_format::parse(&label.to_lowercase().replace('e', "")).unwrap_or(0), worlds)
            } else {
                show_worlds_time(sender, &worlds);
                return Ok(());
            }
        }
        [one] => (ticks_of(one)?, get_selected_worlds(sender, None)?),
        [first, second, rest @ ..] if first.eq_ignore_ascii_case("set") || first.eq_ignore_ascii_case("add") => {
            add = first.eq_ignore_ascii_case("add");
            (ticks_of(second)?, get_selected_worlds(sender, rest.first().map(String::as_str))?)
        }
        [first, second, ..] => (ticks_of(first)?, get_selected_worlds(sender, Some(second))?),
    };
    fail_if!(!sender.is_authorized("essentials.time.set"), EssError::tl("timeSetPermission", &[]));
    for world in &worlds {
        if !can_update_world(sender, world) {
            let name = sender.get_player().map(|player| player.get_world().get_name().to_owned());
            return Err(EssError::tl("timeSetWorldPermission", crate::args![name]));
        }
    }
    let mut names = Vec::new();
    for world in &worlds {
        // Paper throws this from `setTime`: only the overworld has a clock in 26.3.
        fail_if!(world.get_environment() != "NORMAL", EssError::Message("Cannot set time in world without world clock".to_owned()));
        let shifted: Vec<_> = world.get_players().into_iter().filter(|player| player.get_player_time_offset() != 0 && player.is_player_time_relative()).map(|player| (player.get_player_time(), player)).collect();
        let mut time = world.get_time();
        if !add {
            time -= time % 24_000;
        }
        let target = time + if add { 0 } else { 24_000 } + time_tick;
        world.set_time(target);
        for (player_time, player) in shifted {
            player.set_player_time(player_time - target.rem_euclid(24_000), true);
        }
        names.push(world.get_name().to_owned());
    }
    sender.send_tl(if add { "timeWorldAdd" } else { "timeWorldSet" }, crate::args![tick_format::format_ticks(time_tick), names.join(", ")]);
    Ok(())
}

fn show_worlds_time(sender: &CommandSource, worlds: &[World]) {
    if let [world] = worlds {
        sender.send_component(&deserialize_mini_message(&tick_format::format(world.get_time())));
        return;
    }
    for world in worlds {
        sender.send_tl("timeWorldCurrent", crate::args![world.get_name(), parsed(tick_format::format(world.get_time()))]);
    }
}

fn get_selected_worlds(sender: &CommandSource, selector: Option<&str>) -> EssResult<Vec<World>> {
    let mut worlds = match selector {
        None => match sender.get_player() {
            Some(player) => vec![player.get_world()],
            None => get_worlds(),
        },
        Some(selector) => match horizon_bukkit::get_world(selector) {
            Some(world) => vec![world],
            None if selector.eq_ignore_ascii_case("*") || selector.eq_ignore_ascii_case("all") => get_worlds(),
            None => return Err(EssError::tl("invalidWorld", &[])),
        },
    };
    worlds.sort_by(|a, b| a.get_name().cmp(b.get_name()));
    Ok(worlds)
}

fn normalize_world_name(world: &World) -> String {
    world.get_name().to_lowercase().split_whitespace().collect::<Vec<_>>().join("_")
}

fn can_update_world(sender: &CommandSource, world: &World) -> bool {
    !settings().is_world_time_permissions() || sender.is_authorized("essentials.time.world.all") || sender.is_authorized(&format!("essentials.time.world.{}", normalize_world_name(world)))
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let list = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
    Some(match args.len() {
        1 if sender.is_authorized("essentials.time.set") => list(&["add", "set"]),
        2 if args[0].eq_ignore_ascii_case("set") => list(&["sunrise", "day", "morning", "noon", "afternoon", "sunset", "night", "midnight"]),
        2 if args[0].eq_ignore_ascii_case("add") => list(&["1000", "2000", "3000", "4000", "5000"]),
        3 if args[0].eq_ignore_ascii_case("set") || args[0].eq_ignore_ascii_case("add") => {
            let mut worlds: Vec<String> = get_worlds().iter().filter(|world| sender.is_authorized(&format!("essentials.time.world.{}", normalize_world_name(world)))).map(|world| world.get_name().to_owned()).collect();
            if sender.is_authorized("essentials.time.world.all") {
                worlds.push("*".to_owned());
            }
            worlds
        }
        _ => Vec::new(),
    })
}
