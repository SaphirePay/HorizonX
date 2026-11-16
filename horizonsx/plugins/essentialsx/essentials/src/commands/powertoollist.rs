//! `Commandpowertoollist`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("powertoollist") };

fn run(user: &User, _label: &str, _args: &[String]) -> EssResult {
    if !user.has_power_tools() {
        user.send_tl("noPowerTools", &[]);
        return Ok(());
    }
    for (item, commands) in user.get_all_powertools() {
        user.send_tl("powerToolList", crate::args![join_list(&commands), item.to_lowercase().replace('_', " ")]);
    }
    Ok(())
}
