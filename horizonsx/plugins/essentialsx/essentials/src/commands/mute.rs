//! `Commandmute`.
use crate::prelude::*;
use crate::utils::date::parse_date_diff;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("mute") };

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    let is_unmute = label.to_lowercase().contains("unmute");
    fail_if!(args.is_empty(), EssError::not_enough());
    let mut nomatch = false;
    let user = match get_player_raw(None, args, 0, true, true) {
        Ok(user) => user,
        Err(EssError::Tl { key, .. }) if key == "playerNotFound" => {
            nomatch = true;
            ess().users().stub_user(&args[0])
        }
        Err(error) => return Err(error),
    };
    if is_unmute && !user.is_muted() {
        sender.send_tl("playerNotMuted", crate::args![user.get_display_name()]);
        return Ok(());
    }
    if !user.is_online() && sender.is_player() {
        fail_if!(!sender.is_authorized("essentials.mute.offline"), EssError::tl("muteExemptOffline", &[]));
    } else if !is_unmute && user.is_authorized("essentials.mute.exempt") {
        return Err(EssError::tl("muteExempt", &[]));
    }
    let mut mute_timestamp = 0;
    let mut reason = None;
    if !is_unmute && args.len() > 1 {
        match parse_date_diff(&args[1], true) {
            Ok(time) => {
                mute_timestamp = time;
                reason = Some(get_final_arg(args, 2));
            }
            Err(_) => reason = Some(get_final_arg(args, 1)),
        }
        let max = settings().get_max_mute() * 1000;
        if max > 0 && mute_timestamp - now_ms() > max && sender.get_user().is_some_and(|user| !user.is_authorized("essentials.mute.unlimited")) {
            sender.send_tl("oversizedMute", &[]);
            return Err(EssError::NoCharge);
        }
    }
    let will_mute = !is_unmute && (args.len() > 1 || !user.is_muted());
    let controller = sender.get_user();
    crate::hooks::fire(crate::hooks::Hook::Mute { affected: &user, controller: controller.as_ref(), value: will_mute, timestamp: (mute_timestamp > 0).then_some(mute_timestamp), reason: reason.as_deref() });
    match reason {
        Some(reason) => {
            user.set_mute_reason(Some(reason).filter(|reason| !reason.is_empty()));
            user.set_muted(true);
        }
        None => {
            user.set_muted(!user.is_muted());
            if !user.is_muted() {
                user.set_mute_reason(None);
            }
        }
    }
    user.set_mute_timeout(mute_timestamp);
    let mute_time = format_date_diff(mute_timestamp);
    if nomatch {
        sender.send_tl("userUnknown", crate::args![user.get_name()]);
    }
    let by = sender.get_sender().get_name().to_owned();
    if !user.is_muted() {
        sender.send_tl("unmutedPlayer", crate::args![user.get_display_name()]);
        user.send_tl("playerUnmuted", &[]);
        let args = crate::args![by, user.get_name()];
        crate::info(&mini_to_legacy(&tl_literal("unmuteNotify", args)));
        ess().broadcast_tl_permission(None, "essentials.mute.notify", "unmuteNotify", args);
        return Ok(());
    }
    match (mute_timestamp > 0, user.get_mute_reason()) {
        (true, None) => {
            sender.send_tl("mutedPlayerFor", crate::args![user.get_display_name(), mute_time.clone()]);
            user.send_tl("playerMutedFor", crate::args![mute_time.clone()]);
        }
        (true, Some(reason)) => {
            sender.send_tl("mutedPlayerForReason", crate::args![user.get_display_name(), mute_time.clone(), reason.clone()]);
            user.send_tl("playerMutedForReason", crate::args![mute_time.clone(), reason]);
        }
        (false, None) => {
            sender.send_tl("mutedPlayer", crate::args![user.get_display_name()]);
            user.send_tl("playerMuted", &[]);
        }
        (false, Some(reason)) => {
            sender.send_tl("mutedPlayerReason", crate::args![user.get_display_name(), reason.clone()]);
            user.send_tl("playerMutedReason", crate::args![reason]);
        }
    }
    let (key, objects): (&str, Vec<Arg>) = match user.get_mute_reason() {
        Some(reason) if mute_timestamp > 0 => ("muteNotifyForReason", crate::args![by, user.get_name(), mute_time, reason].to_vec()),
        Some(reason) => ("muteNotify", crate::args![by, user.get_name(), reason].to_vec()),
        None => (if mute_timestamp > 0 { "muteNotifyFor" } else { "muteNotify" }, crate::args![by, user.get_name(), mute_time].to_vec()),
    };
    crate::info(&mini_to_legacy(&tl_literal(key, &objects)));
    ess().broadcast_tl_permission(None, "essentials.mute.notify", key, &objects);
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { get_players(sender) } else { COMMON_DATE_DIFFS.iter().map(|diff| (*diff).to_owned()).collect() })
}
