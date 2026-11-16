//! `Commandpowertooltoggle`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("powertooltoggle") };

fn run(user: &User, _label: &str, _args: &[String]) -> EssResult {
    if !user.has_power_tools() {
        user.send_tl("noPowerTools", &[]);
        return Ok(());
    }
    user.send_tl(if user.toggle_power_tools_enabled() { "powerToolsEnabled" } else { "powerToolsDisabled" }, &[]);
    Ok(())
}
