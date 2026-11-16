//! `Commanddelwarp`. `WarpModifyEvent` isn't fired: nothing here can listen.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("delwarp") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let name = args.first().ok_or_else(EssError::not_enough)?;
    fail_if!(!ess().warps().is_warp(name), EssError::tl("warpNotExist", &[]));
    ess().warps().remove_warp(name)?;
    sender.send_tl("deleteWarp", crate::args![name]);
    Ok(())
}

fn tab(_sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { ess().warps().get_list() } else { Vec::new() })
}
