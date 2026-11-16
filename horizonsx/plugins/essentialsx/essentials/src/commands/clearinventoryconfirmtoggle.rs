//! `Commandclearinventoryconfirmtoggle`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("clearinventoryconfirmtoggle") };

fn run(user: &User, label: &str, _args: &[String]) -> EssResult {
    let label = label.to_lowercase();
    let confirming = if label.ends_with("on") {
        true
    } else if label.ends_with("off") {
        false
    } else {
        !user.is_prompting_clear_confirm()
    };
    user.set_prompting_clear_confirm(confirming);
    user.send_tl(if confirming { "clearInventoryConfirmToggleOn" } else { "clearInventoryConfirmToggleOff" }, &[]);
    user.set_confirming_clear_command(None);
    Ok(())
}
