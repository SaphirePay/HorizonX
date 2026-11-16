//! `Commanddeljail`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("deljail") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let name = args.first().ok_or_else(EssError::not_enough)?;
    ess().jails().get_jail(Some(name))?;
    ess().jails().remove_jail(name);
    sender.send_tl("deleteJail", crate::args![name]);
    Ok(())
}

fn tab(_sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { ess().jails().get_list() } else { Vec::new() })
}
