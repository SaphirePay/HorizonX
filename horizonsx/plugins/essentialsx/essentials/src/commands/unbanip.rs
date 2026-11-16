//! `Commandunbanip`.
use crate::prelude::*;
use horizon_bukkit::extra::get_ban_list;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("unbanip") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let address = ip_of(args)?;
    get_ban_list("IP").pardon(&address);
    let (_, sender_display) = ban_names(sender);
    let notice = crate::args![sender_display, address];
    crate::info(&mini_to_legacy(&tl_literal("playerUnbanIpAddress", notice)));
    ess().broadcast_tl_permission(None, "essentials.banip.notify", "playerUnbanIpAddress", notice);
    Ok(())
}
