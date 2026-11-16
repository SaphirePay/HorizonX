//! `Commandban`.
use crate::prelude::*;
use horizon_bukkit::extra::get_ban_list;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("ban") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let mut no_match = false;
    let user = match get_player_raw(None, args, 0, true, true) {
        Ok(user) => user,
        Err(error) if error.is_player_not_found() => {
            no_match = true;
            ess().users().stub_user(&args[0])
        }
        Err(error) => return Err(error),
    };
    if !user.is_online() {
        fail_if!(sender.get_user().is_some_and(|by| !by.is_authorized("essentials.ban.offline")), EssError::tl("banExemptOffline", &[]));
    } else if user.is_authorized("essentials.ban.exempt") && sender.is_player() {
        return Err(EssError::tl("banExempt", &[]));
    }
    let (sender_name, sender_display) = ban_names(sender);
    let reason = if args.len() > 1 { reason_of(&get_final_arg(args, 1)) } else { tl_literal("defaultBanReason", &[]) };
    get_ban_list("NAME").add_ban_until(&user.get_name(), &reason, None, &sender_name);
    let display = tl_literal("banFormat", crate::args![reason.clone(), sender_display.clone()]);
    if let Some(player) = user.player() {
        player.kick_player(&mini_to_legacy(&display));
    }
    crate::info(&mini_to_legacy(&tl_literal("playerBanned", crate::args![sender_display.clone(), user.get_name(), display])));
    if no_match {
        sender.send_tl("userUnknown", crate::args![user.get_name()]);
    }
    ess().broadcast_tl(None, Some(&|other: &User| !other.is_authorized("essentials.ban.notify")), false, "playerBanned", crate::args![sender_display, user.get_name(), reason]);
    Ok(())
}
