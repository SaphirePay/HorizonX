//! `Commandjails`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("jails") };

fn run(sender: &CommandSource, _label: &str, _args: &[String]) -> EssResult {
    if ess().jails().get_count() == 0 {
        sender.send_tl("noJailsDefined", &[]);
    } else {
        sender.send_tl("jailList", crate::args![ess().jails().get_list().join(" ")]);
    }
    Ok(())
}
