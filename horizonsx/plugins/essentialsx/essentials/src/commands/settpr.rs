//! `Commandsettpr`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("settpr") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let teleport = ess().random_teleport();
    if args[1].eq_ignore_ascii_case("center") {
        if let Some(location) = user.get_location() {
            teleport.set_center(&args[0], &location);
        }
        user.send_tl("settpr", &[]);
    } else if let Some(value) = args.get(2) {
        if args[1].eq_ignore_ascii_case("minrange") {
            teleport.set_min_range(&args[0], parse_double(value)?);
        } else if args[1].eq_ignore_ascii_case("maxrange") {
            teleport.set_max_range(&args[0], parse_double(value)?);
        }
        user.send_tl("settprValue", crate::args![args[1].to_lowercase(), value.to_lowercase()]);
    } else {
        return Err(EssError::not_enough());
    }
    Ok(())
}

fn tab(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_worlds().iter().map(|world| world.get_name().to_owned()).collect(),
        2 => ["center", "minrange", "maxrange"].map(str::to_owned).to_vec(),
        _ => Vec::new(),
    })
}
