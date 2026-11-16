//! `Commandbanip`.
use crate::prelude::*;
use horizon_bukkit::extra::get_ban_list;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("banip") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let (sender_name, sender_display) = ban_names(sender);
    let address = ip_of(args)?;
    let reason = if args.len() > 1 { reason_of(&get_final_arg(args, 1)) } else { tl_literal("defaultBanReason", &[]) };
    let display = mini_to_legacy(&tl_literal("banFormat", crate::args![reason.clone(), sender_display.clone()]));
    get_ban_list("IP").add_ban_until(&address, &reason, None, &sender_name);
    let notice = crate::args![sender_display, address.clone(), reason];
    crate::info(&mini_to_legacy(&tl_literal("playerBanIpAddress", notice)));
    kick_address(&address, &display);
    ess().broadcast_tl(None, Some(&|other: &User| !other.is_authorized("essentials.banip.notify")), false, "playerBanIpAddress", notice);
    Ok(())
}
