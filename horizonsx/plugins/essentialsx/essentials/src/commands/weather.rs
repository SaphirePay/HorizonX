//! `Commandweather`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("weather") };

fn apply(source: &CommandSource, world: &World, is_storm: bool, duration: Option<&String>) -> EssResult {
    world.set_storm(is_storm);
    if let Some(duration) = duration {
        world.set_weather_duration(parse_int(duration)?.wrapping_mul(20));
        source.send_tl(if is_storm { "weatherStormFor" } else { "weatherSunFor" }, crate::args![world.get_name(), duration]);
        return Ok(());
    }
    source.send_tl(if is_storm { "weatherStorm" } else { "weatherSun" }, crate::args![world.get_name()]);
    Ok(())
}

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    let is_storm = match args.first() {
        Some(first) => first.eq_ignore_ascii_case("storm"),
        None if label.ends_with("sun") => false,
        None if label.ends_with("storm") || label.ends_with("rain") => true,
        None => return Err(EssError::not_enough()),
    };
    let Some(world) = user.get_world() else { return Ok(()) };
    apply(&user.get_source(), &world, is_storm, args.get(1))
}

fn run_source(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::Message(format!("When running from console, usage is: /{label} <world> <storm/sun> [duration]")));
    let world = horizon_bukkit::get_world(&args[0]).ok_or_else(|| EssError::tl("weatherInvalidWorld", crate::args![&args[0]]))?;
    apply(sender, &world, args[1].eq_ignore_ascii_case("storm"), args.get(2))
}

fn durations() -> Vec<String> {
    COMMON_DURATIONS.iter().map(|duration| (*duration).to_owned()).collect()
}

fn tab_user(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => vec!["storm".to_owned(), "sun".to_owned()],
        2 => durations(),
        _ => Vec::new(),
    })
}

fn tab_source(_sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_worlds().iter().map(|world| world.get_name().to_owned()).collect(),
        2 => vec!["storm".to_owned(), "sun".to_owned()],
        3 => durations(),
        _ => Vec::new(),
    })
}
