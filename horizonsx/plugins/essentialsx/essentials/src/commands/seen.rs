//! `Commandseen`. Java's two asynchronous steps read user files here on a thread of their own, and
//! answer on the server's.
use crate::prelude::*;
use crate::adventure::legacy_to_mini;
use crate::ban_lookup;
use crate::utils::date::format_date;
use crate::utils::format::valid_ip;
use crate::utils::string::{join_list_skip, to_uuid};
use horizon_bukkit::extra::get_ban_list;
use horizon_bukkit::get_offline_player;

pub const COMMAND: Command = Command { run_source: Some(run), ..Command::new("seen") };

#[derive(Clone, Copy)]
struct Show {
    accounts: bool,
    ban: bool,
    ip: bool,
    location: bool,
    whitelist: bool,
}

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let show = Show {
        accounts: label.contains("alts") && sender.is_authorized("essentials.seen.alts"),
        ban: sender.is_authorized("essentials.seen.banreason"),
        ip: sender.is_authorized("essentials.seen.ip"),
        location: sender.is_authorized("essentials.seen.location"),
        whitelist: sender.is_authorized("essentials.seen.whitelist"),
    };
    let player = match to_uuid(first) {
        Some(uuid) => ess().get_user_by_uuid(&uuid),
        None => ess().get_offline_user(first),
    };
    if let Some(player) = player {
        show_seen_message(sender, &player, show);
        return Ok(());
    }
    if !show.accounts {
        let banned = get_ban_list("IP").is_banned(first);
        if sender.is_authorized("essentials.seen.ipsearch") && valid_ip(first) {
            if banned {
                sender.send_tl("isIpBanned", crate::args![first]);
            }
            seen_ip(sender, first, first);
            return Ok(());
        } else if banned {
            sender.send_tl("isIpBanned", crate::args![first]);
            return Ok(());
        } else if let Some(entry) = ban_lookup::get_ban_entry(first) {
            let reason: Arg = if show.ban { entry.get_reason().map(str::to_owned).into() } else { sender.tl("true", &[]).into() };
            sender.send_tl("whoisBanned", &[reason]);
            return Ok(());
        }
    }
    let (sender, label, args) = (sender.clone(), label.to_owned(), args.to_vec());
    let mut task = Some(move || {
        let found = match ess().users().get_user_by_name(&args[0]) {
            Some(user) => Ok(user),
            None => get_player(&sender, &args, 0).map_err(|error| if error.is_player_not_found() { EssError::tl("playerNeverOnServer", crate::args![&args[0]]) } else { error }),
        };
        match found {
            Ok(user) => show_seen_message(&sender, &user, show),
            Err(error) => ess().show_error(&sender, &error, &label),
        }
    });
    ess().schedule_sync_delayed_task(move || {
        if let Some(task) = task.take() {
            task();
        }
    });
    Ok(())
}

fn show_seen_message(sender: &CommandSource, player: &User, show: Show) {
    if show.accounts {
        seen_ip(sender, &player.get_last_login_address(), &player.get_display_name());
    } else if player.is_online() && ess().can_interact_with_source(Some(sender), player) {
        seen_online(sender, player, show);
    } else {
        seen_offline(sender, player, show);
    }
}

fn mute_lines(sender: &CommandSource, user: &User) {
    if !user.is_muted() {
        return;
    }
    let timeout = user.get_mute_timeout();
    let when = if timeout > 0 { format_date_diff(timeout).into() } else { true_false(sender, true) };
    if user.has_mute_reason() {
        sender.send_tl("whoisMutedReason", &[when, user.get_mute_reason().into()]);
    } else {
        sender.send_tl("whoisMuted", &[when]);
    }
}

fn geo_line(sender: &CommandSource, user: &User) {
    if let Some(location) = user.get_geo_location()
        && sender.get_user().is_none_or(|by| by.is_authorized("essentials.geoip.show"))
    {
        sender.send_tl("whoisGeoLocation", crate::args![location]);
    }
}

fn seen_online(sender: &CommandSource, user: &User, show: Show) {
    user.set_display_nick();
    sender.send_tl("seenOnline", crate::args![user.get_display_name(), format_date_diff(user.get_last_login())]);
    let history = user.get_past_usernames();
    if !history.is_empty() {
        sender.send_tl("seenAccounts", crate::args![join_list_skip(&user.get_name(), &history)]);
    }
    if sender.is_authorized("essentials.seen.uuid") {
        sender.send_tl("whoisUuid", crate::args![user.get_uuid()]);
    }
    if sender.is_authorized("essentials.seen.firstlogin") {
        sender.send_tl("whoisFirstLogin", crate::args![format_date(get_offline_player(&user.get_name()).get_first_played())]);
    }
    if user.is_afk() {
        sender.send_tl("whoisAFK", &[true_false(sender, true)]);
    }
    if user.is_jailed() {
        let shown = if user.get_jail_timeout() > 0 { user.get_formatted_jail_time().into() } else { true_false(sender, true) };
        sender.send_tl("whoisJail", &[shown]);
    }
    mute_lines(sender, user);
    geo_line(sender, user);
    if show.ip
        && let Some(address) = user.player().and_then(|player| player.get_address())
    {
        sender.send_tl("whoisIPAddress", crate::args![format!("/{address}")]);
    }
}

fn seen_offline(sender: &CommandSource, user: &User, show: Show) {
    user.set_display_nick();
    if user.get_last_logout() > 0 {
        sender.send_tl("seenOffline", crate::args![user.get_name(), format_date_diff(user.get_last_logout())]);
        let history = user.get_past_usernames();
        if history.len() > 1 {
            sender.send_tl("seenAccounts", crate::args![join_list_skip(&user.get_name(), &history)]);
        }
        if sender.is_authorized("essentials.seen.uuid") {
            sender.send_tl("whoisUuid", crate::args![user.get_uuid()]);
        }
    } else {
        sender.send_tl("userUnknown", crate::args![user.get_name()]);
    }
    if show.whitelist {
        sender.send_tl("whoisWhitelist", &[true_false(sender, get_offline_player(&user.get_name()).is_whitelisted())]);
    }
    if let Some(entry) = ban_lookup::get_ban_entry(&user.get_name()) {
        let reason: Arg = if show.ban { entry.get_reason().map(str::to_owned).into() } else { true_false(sender, true) };
        sender.send_tl("whoisBanned", &[reason]);
        if let Some(expiry) = ban_lookup::expiration(&entry) {
            let shown: Arg = if expiry > now_ms() { format_date_diff(expiry).into() } else { parsed(sender.tl("now", &[])) };
            sender.send_tl("whoisTempBanned", &[shown]);
        }
    }
    mute_lines(sender, user);
    geo_line(sender, user);
    if show.ip && !user.get_last_login_address().is_empty() {
        sender.send_tl("whoisIPAddress", crate::args![user.get_last_login_address()]);
    }
    if show.location
        && let Some(location) = user.get_logout_location()
    {
        sender.send_tl("whoisLocation", crate::args![location.world.get_name(), location.get_block_x(), location.get_block_y(), location.get_block_z()]);
    }
}

fn seen_ip(sender: &CommandSource, address: &str, display: &str) {
    sender.send_tl("runningPlayerMatch", crate::args![parsed(legacy_to_mini(display))]);
    let (sender, address) = (sender.clone(), address.to_owned());
    ess().run_task_asynchronously(move || {
        let mut matches = Vec::new();
        for uuid in ess().users().get_all_user_uuids() {
            let Some(user) = ess().users().load_uncached_user(&uuid) else { continue };
            let known = user.get_last_login_address();
            if !known.is_empty() && known.eq_ignore_ascii_case(&address) {
                matches.push(user.get_name());
            }
        }
        let mut answer = Some(move || {
            if matches.is_empty() {
                sender.send_tl("noMatchingPlayers", &[]);
            } else {
                sender.send_tl("matchingIPAddress", &[]);
                sender.send_tl("matchingAccounts", crate::args![join_list(&matches)]);
            }
        });
        ess().schedule_sync_delayed_task(move || {
            if let Some(answer) = answer.take() {
                answer();
            }
        });
    });
}
