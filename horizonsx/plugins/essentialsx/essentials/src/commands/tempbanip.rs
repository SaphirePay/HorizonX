//! `Commandtempbanip`.
use crate::prelude::*;
use super::tempban::{ban_terms, tab};
use horizon_bukkit::extra::get_ban_list;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("tempbanip") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let (sender_name, sender_display) = ban_names(sender);
    let address = ip_of(args)?;
    let Some((timestamp, reason)) = ban_terms(sender, args)? else { return Ok(()) };
    get_ban_list("IP").add_ban_until(&address, &reason, Some(timestamp), &sender_name);
    let display = mini_to_legacy(&tl_literal("banFormat", crate::args![reason.clone(), sender_display.clone()]));
    kick_address(&address, &display);
    let notice = crate::args![sender_display, address, reason.clone(), format_date_diff(timestamp), reason];
    crate::info(&mini_to_legacy(&tl_literal("playerTempBanIpAddress", notice)));
    ess().broadcast_tl_permission(None, "essentials.banip.notify", "playerTempBanIpAddress", notice);
    Ok(())
}
