//! `Commandtempban`.
use crate::prelude::*;
use crate::utils::date::{parse_date_diff, remove_time_pattern};
use horizon_bukkit::extra::get_ban_list;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("tempban") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let user = get_player_raw(None, args, 0, true, true)?;
    if !user.is_online() && sender.get_user().is_some_and(|by| !by.is_authorized("essentials.tempban.offline")) {
        sender.send_tl("tempbanExemptOffline", &[]);
        return Ok(());
    } else if user.is_authorized("essentials.tempban.exempt") && sender.is_player() {
        sender.send_tl("tempbanExempt", &[]);
        return Ok(());
    }
    let Some((timestamp, reason)) = ban_terms(sender, args)? else { return Ok(()) };
    let (sender_name, sender_display) = ban_names(sender);
    get_ban_list("NAME").add_ban_until(&user.get_name(), &reason, Some(timestamp), &sender_name);
    let expiry = format_date_diff(timestamp);
    let display = user.player_tl("tempBanned", crate::args![expiry.clone(), sender_display.clone(), reason.clone()]);
    if let Some(player) = user.player() {
        player.kick_player(&mini_to_legacy(&display));
    }
    let notice = crate::args![sender_display, user.get_name(), expiry, reason];
    crate::info(&mini_to_legacy(&tl_literal("playerTempBanned", notice)));
    ess().broadcast_tl_permission(None, "essentials.ban.notify", "playerTempBanned", notice);
    Ok(())
}

/// When a temporary ban ends and why, from the words after the target: `None` if it's over the limit,
/// which the sender has been told.
pub fn ban_terms(sender: &CommandSource, args: &[String]) -> EssResult<Option<(i64, String)>> {
    let time = get_final_arg(args, 1);
    let timestamp = parse_date_diff(&time, true)?;
    let reason = replace_format(&remove_time_pattern(&time));
    let max = settings().get_max_tempban() * 1000;
    if max > 0 && timestamp - now_ms() > max && sender.get_user().is_some_and(|by| !by.is_authorized("essentials.tempban.unlimited")) {
        sender.send_tl("oversizedTempban", &[]);
        return Ok(None);
    }
    Ok(Some((timestamp, if reason.encode_utf16().count() < 2 { tl_literal("defaultBanReason", &[]) } else { reason })))
}

pub fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { get_players(sender) } else { COMMON_DATE_DIFFS.iter().map(|diff| (*diff).to_owned()).collect() })
}
