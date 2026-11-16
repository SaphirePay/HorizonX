//! `Commandtogglejail`. `JailStatusChangeEvent` isn't fired: nothing here can listen.
use crate::prelude::*;
use crate::ess::play_one_tick_of;
use crate::settings::TeleportWhenFreePolicy;
use crate::utils::date::{parse_date_diff, parse_date_diff_from};

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("togglejail") };

fn notify(key: &str, args: &[Arg]) {
    crate::info(&mini_to_legacy(&tl_literal(key, args)));
    ess().broadcast_tl_permission(None, "essentials.jail.notify", key, args);
}

fn sender_name(sender: &CommandSource) -> String {
    sender.get_sender().get_name().to_owned()
}

fn online_time(player: &User, diff: i64) -> i64 {
    if settings().is_jail_online_time() { play_one_tick_of(player) + diff / 50 } else { 0 }
}

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let player = get_player_raw(None, args, 0, true, true)?;
    if !player.is_jailed() {
        return jail(sender, label, args, player);
    }
    let in_this_jail = args.get(1).is_some_and(|name| player.get_jail().is_some_and(|jail| name.eq_ignore_ascii_case(&jail)));
    if args.len() >= 2 && !in_this_jail {
        sender.send_tl("jailAlreadyIncarcerated", crate::args![player.get_jail()]);
        return Ok(());
    }
    if args.len() >= 2 {
        let unparsed = get_final_arg(args, 2);
        let display_diff = parse_date_diff(&unparsed, true)?;
        let diff = parse_date_diff_from(&unparsed, true, settings().is_jail_online_time())?;
        player.set_jail_timeout(diff);
        player.set_online_jailed_time(online_time(&player, diff));
        let shown = format_date_diff(display_diff);
        sender.send_tl("jailSentenceExtended", crate::args![shown.clone()]);
        notify("jailNotifySentenceExtended", crate::args![player.get_name(), shown, sender_name(sender)]);
        return Ok(());
    }
    release(sender, label, &player);
    Ok(())
}

fn jail(sender: &CommandSource, label: &str, args: &[String], player: User) -> EssResult {
    if !player.is_online() && sender.get_user().is_some_and(|user| !user.is_authorized("essentials.togglejail.offline")) {
        sender.send_tl("mayNotJailOffline", &[]);
        return Ok(());
    }
    if player.is_authorized("essentials.jail.exempt") {
        sender.send_tl("mayNotJail", &[]);
        return Ok(());
    }
    let jail_name = if let Some(name) = args.get(1) {
        name.clone()
    } else if ess().jails().get_count() == 1 {
        ess().jails().get_list().remove(0)
    } else {
        return Err(EssError::not_enough());
    };
    ess().jails().get_jail(Some(&jail_name))?;
    let timed = args.len() > 2;
    let (mut display_time, mut time_diff) = (0, 0);
    if timed {
        let time = get_final_arg(args, 2);
        display_time = parse_date_diff(&time, true)?;
        time_diff = parse_date_diff_from(&time, true, settings().is_jail_online_time())?;
    }
    let future = get_new_exception_future(sender, label);
    let (notify_sender, target, name) = (sender.clone(), player.clone(), jail_name.clone());
    future.then_accept(move |success| {
        if !success {
            return;
        }
        target.set_jailed(true);
        target.send_tl("userJailed", &[]);
        target.set_jail(None);
        target.set_jail(Some(name));
        if timed {
            target.set_jail_timeout(time_diff);
            target.set_online_jailed_time(online_time(&target, time_diff));
        }
        let by = sender_name(&notify_sender);
        if time_diff > 0 {
            let shown = format_date_diff(display_time);
            notify_sender.send_tl("playerJailedFor", crate::args![target.get_name(), shown.clone()]);
            notify("jailNotifyJailedFor", crate::args![target.get_name(), shown, by]);
        } else {
            notify_sender.send_tl("playerJailed", crate::args![target.get_name()]);
            notify("jailNotifyJailed", crate::args![target.get_name(), by.clone(), by]);
        }
    });
    if player.is_online() {
        ess().jails().send_to_jail(&player, &jail_name, future)?;
    } else {
        future.complete(true);
    }
    Ok(())
}

fn release(sender: &CommandSource, label: &str, player: &User) {
    player.set_jailed(false);
    player.set_jail_timeout(0);
    player.send_tl("jailReleasedPlayerNotify", &[]);
    player.set_jail(None);
    if !player.is_online() {
        sender.send_tl("jailReleased", crate::args![player.get_name()]);
        return;
    }
    let future = get_new_exception_future(sender, label);
    let (released_sender, name) = (sender.clone(), player.get_name());
    future.then_accept(move |success| {
        if success {
            released_sender.send_tl("jailReleased", crate::args![name]);
        }
    });
    match settings().get_teleport_when_free_policy() {
        TeleportWhenFreePolicy::Back => {
            let (failed_sender, target) = (sender.clone(), player.clone());
            future.exceptionally(move |_| {
                target.get_async_teleport().respawn(None, "PLUGIN", EssFuture::new());
                failed_sender.send_tl("jailReleased", crate::args![target.get_name()]);
            });
            player.get_async_teleport().back(None, future);
        }
        TeleportWhenFreePolicy::Spawn => player.get_async_teleport().respawn(None, "PLUGIN", future),
        TeleportWhenFreePolicy::Off => {}
    }
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_players(sender),
        2 => ess().jails().get_list(),
        _ => COMMON_DATE_DIFFS.iter().map(|diff| (*diff).to_owned()).collect(),
    })
}
