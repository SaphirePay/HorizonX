//! `Commandsuicide`. The damage event Java raises first can't be raised here, and changes nothing:
//! it only passes the cause on to the death message.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("suicide") };

fn run(user: &User, _label: &str, _args: &[String]) -> EssResult {
    if let Some(player) = user.player() {
        player.set_health(0.0);
    }
    user.send_tl("suicideMessage", &[]);
    user.set_display_nick();
    ess().broadcast_tl(Some(user), None, false, "suicideSuccess", crate::args![user.get_display_name()]);
    Ok(())
}

fn tab(_user: &User, _label: &str, _args: &[String]) -> Option<Vec<String>> {
    Some(Vec::new())
}
