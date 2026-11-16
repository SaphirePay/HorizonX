//! `Commandwarpinfo`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("warpinfo") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let name = args.first().ok_or_else(EssError::not_enough)?;
    let location = ess().warps().get_warp(name)?;
    sender.send_tl("warpInfo", crate::args![name]);
    sender.send_tl("whoisLocation", crate::args![location.world.get_name(), location.get_block_x(), location.get_block_y(), location.get_block_z()]);
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() != 1 {
        return Some(Vec::new());
    }
    let mut list = ess().warps().get_list();
    if settings().get_per_warp_permission() && sender.is_player() {
        list.retain(|warp| sender.is_authorized(&format!("essentials.warps.{warp}")));
    }
    Some(list)
}
