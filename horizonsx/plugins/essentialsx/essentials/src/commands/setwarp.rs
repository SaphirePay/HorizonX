//! `Commandsetwarp`. `WarpModifyEvent` isn't fired: nothing here can listen.
use crate::prelude::*;
use crate::utils::string::{is_reserved_file_name, safe_string};

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("setwarp") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let name = args.first().ok_or_else(EssError::not_enough)?;
    fail_if!(is_int(name) || name.is_empty() || is_reserved_file_name(name), EssError::tl("invalidWarpName", &[]));
    let warps = ess().warps();
    if warps.is_warp(name) {
        warps.get_warp(name)?;
        fail_if!(!user.is_authorized(&format!("essentials.warp.overwrite.{}", safe_string(name))), EssError::tl("warpOverwrite", &[]));
    }
    let Some(location) = user.get_location() else { return Ok(()) };
    warps.set_warp(Some(user), name, &location)?;
    user.send_tl("warpSet", crate::args![name]);
    Ok(())
}
