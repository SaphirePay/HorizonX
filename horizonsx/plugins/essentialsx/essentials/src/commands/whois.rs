//! `Commandwhois`.
use crate::prelude::*;
use crate::ess::play_one_tick;
use crate::set_exp_fix::get_total_experience;
use horizon_bukkit::get_offline_player;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("whois") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let user = get_player(sender, args, 0)?;
    let base = user.player().ok_or_else(EssError::player_not_found)?;
    sender.send_tl("whoisTop", crate::args![user.get_name()]);
    user.set_display_nick();
    sender.send_tl("whoisNick", crate::args![user.get_display_name()]);
    sender.send_tl("whoisUuid", crate::args![user.get_uuid()]);
    sender.send_tl("whoisHealth", crate::args![base.get_health()]);
    sender.send_tl("whoisHunger", crate::args![base.get_food_level(), base.get_saturation()]);
    sender.send_tl("whoisExp", crate::args![get_total_experience(&base), base.get_level()]);
    let location = base.get_location();
    sender.send_tl("whoisLocation", crate::args![location.world.get_name(), location.get_block_x(), location.get_block_y(), location.get_block_z()]);
    sender.send_tl("whoisPlaytime", crate::args![format_date_diff(now_ms() - play_one_tick(&base) * 50)]);
    if !settings().is_eco_disabled() {
        sender.send_tl("whoisMoney", crate::args![parsed(display_currency(&user.get_money()))]);
    }
    let by = sender.get_user();
    if by.as_ref().is_none_or(|by| by.is_authorized("essentials.whois.ip")) {
        sender.send_tl("whoisIPAddress", crate::args![format!("/{}", base.get_address().unwrap_or_default())]);
    }
    if let Some(geo) = user.get_geo_location()
        && by.as_ref().is_none_or(|by| by.is_authorized("essentials.geoip.show"))
    {
        sender.send_tl("whoisGeoLocation", crate::args![geo]);
    }
    sender.send_tl("whoisGamemode", crate::args![sender.tl(&base.get_game_mode().to_lowercase(), &[])]);
    sender.send_tl("whoisGod", &[true_false(sender, user.is_god_mode_enabled())]);
    sender.send_tl("whoisOp", &[true_false(sender, base.is_op())]);
    let flying = parsed(sender.tl(if base.is_flying() { "flying" } else { "notFlying" }, &[]));
    sender.send_tl("whoisFly", &[true_false(sender, base.get_allow_flight()), flying]);
    sender.send_tl("whoisSpeed", crate::args![if base.is_flying() { base.get_fly_speed() } else { base.get_walk_speed() }]);
    sender.send_tl("whoisWhitelist", &[true_false(sender, get_offline_player(&user.get_name()).is_whitelisted())]);
    if user.is_afk() {
        sender.send_tl("whoisAFKSince", &[true_false(sender, true), format_date_diff(user.get_afk_since()).into()]);
    } else {
        sender.send_tl("whoisAFK", &[true_false(sender, false)]);
    }
    let yes_or = |timeout: i64, active: bool, timed: String| {
        parsed(if !active {
            sender.tl("false", &[])
        } else if timeout > 0 {
            timed
        } else {
            sender.tl("true", &[])
        })
    };
    sender.send_tl("whoisJail", &[yes_or(user.get_jail_timeout(), user.is_jailed(), user.get_formatted_jail_time())]);
    let mute_timeout = user.get_mute_timeout();
    let muted = yes_or(mute_timeout, user.is_muted(), format_date_diff(mute_timeout));
    if user.has_mute_reason() {
        sender.send_tl("whoisMutedReason", &[muted, user.get_mute_reason().into()]);
    } else {
        sender.send_tl("whoisMuted", &[muted]);
    }
    Ok(())
}
