//! `Commandthunder`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("thunder") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let Some(world) = user.get_world() else { return Ok(()) };
    let set = first.eq_ignore_ascii_case("true");
    world.set_thundering(set);
    let Some(duration) = args.get(1) else {
        user.send_tl("thunder", &[enable_disable(&user.get_source(), set)]);
        return Ok(());
    };
    let duration = parse_int(duration)?;
    world.set_thunder_duration(duration.wrapping_mul(20));
    user.send_tl("thunderDuration", &[enable_disable(&user.get_source(), set), duration.into()]);
    Ok(())
}

fn tab(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => vec!["true".to_owned(), "false".to_owned()],
        2 => COMMON_DATE_DIFFS.iter().map(|diff| (*diff).to_owned()).collect(),
        _ => Vec::new(),
    })
}
