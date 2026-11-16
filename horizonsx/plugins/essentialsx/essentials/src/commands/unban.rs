//! `Commandunban`.
use crate::prelude::*;
use horizon_bukkit::extra::get_ban_list;
use horizon_bukkit::get_offline_player;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("unban") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let name = match get_player_raw(None, args, 0, true, true) {
        Ok(user) => user.get_name(),
        Err(error) if error.is_player_not_found() => {
            let player = get_offline_player(&args[0]);
            fail_if!(!player.is_banned(), EssError::tl("playerNeverOnServer", crate::args![&args[0]]));
            player.get_name().to_owned()
        }
        Err(error) => return Err(error),
    };
    get_ban_list("NAME").pardon(&name);
    let (_, sender_display) = ban_names(sender);
    let notice = crate::args![sender_display, name];
    crate::info(&mini_to_legacy(&tl_literal("playerUnbanned", notice)));
    ess().broadcast_tl_permission(None, "essentials.ban.notify", "playerUnbanned", notice);
    Ok(())
}
