//! `Commandhome`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("home") };

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    let charge = Trade::of_command("home");
    let mut player = user.clone();
    let mut home_name = String::new();
    if let Some(first) = args.first() {
        let name_parts = java_split(first, ':');
        let head = name_parts.first().cloned().unwrap_or_default();
        if head.len() == first.len() || !user.is_authorized("essentials.home.others") {
            home_name = head;
        } else {
            player = get_player_raw(None, &name_parts, 0, true, true)?;
            if let Some(name) = name_parts.get(1) {
                home_name.clone_from(name);
            }
        }
    }
    let attempt = if home_name.eq_ignore_ascii_case("bed") { go_to_bed(user, &player, label, &charge) } else { go_home(user, &player, &home_name.to_lowercase(), &charge, label) };
    match attempt {
        Err(EssError::NotEnoughArguments(_)) => {
            let bed = player.player().filter(|base| base.is_online()).and_then(|base| base.get_respawn_location());
            list_homes(user, &player, bed, &charge, label);
            Err(EssError::NoCharge)
        }
        Err(error) => Err(error),
        Ok(()) => Err(EssError::NoCharge),
    }
}

fn go_to_bed(user: &User, player: &User, label: &str, charge: &Trade) -> EssResult {
    fail_if!(!user.is_authorized("essentials.home.bed"), EssError::tl("noAccessCommand", &[]));
    fail_if!(!player.is_online(), EssError::tl("bedOffline", &[]));
    let source = user.get_source();
    match player.player().and_then(|base| base.get_respawn_location()) {
        Some(location) => {
            let future = get_new_exception_future(&source, label);
            let told = user.clone();
            future.then_accept(move |success| {
                if success {
                    told.send_tl("teleportHome", crate::args!["bed"]);
                }
            });
            user.get_async_teleport().teleport(&location, Some(charge), "COMMAND", future);
        }
        None => show_error(&source, &EssError::tl("bedMissing", &[]), label),
    }
    Ok(())
}

/// What's left when no home was named: the bed or spawn, the only home, or the list.
fn list_homes(user: &User, player: &User, bed: Option<horizon_bukkit::Location>, charge: &Trade, label: &str) {
    let source = user.get_source();
    let mut homes = player.get_homes();
    let same = player == user;
    let no_home = || show_error(&source, &EssError::tl("noHomeSetPlayer", &[]), label);
    if homes.is_empty() && same {
        if !settings().is_spawn_if_no_home() {
            no_home();
            return;
        }
        let future = get_new_exception_future(&source, label);
        if bed.is_some() && user.is_authorized("essentials.home.bed") {
            user.get_async_teleport().respawn(Some(charge), "COMMAND", future);
        } else if let Some(world) = player.get_world() {
            user.get_async_teleport().teleport(&world.get_spawn_location(), Some(charge), "COMMAND", future);
        }
    } else if homes.is_empty() || !player.has_valid_homes() {
        no_home();
    } else if homes.len() == 1 && same {
        if let Err(error) = go_home(user, player, &homes[0], charge, label) {
            show_error(&source, &error, label);
        }
    } else {
        let count = homes.len();
        if user.is_authorized("essentials.home.bed") {
            homes.push(user.player_tl(if bed.is_some() { "bed" } else { "bedNull" }, &[]));
        }
        user.send_tl("homes", crate::args![parsed(join_list(&homes)), count, home_limit(player)]);
    }
}

fn home_limit(player: &User) -> String {
    if !player.is_online() {
        return "?".to_owned();
    }
    if player.is_authorized("essentials.sethome.multiple.unlimited") {
        return "*".to_owned();
    }
    settings().get_home_limit(player).to_string()
}

fn go_home(user: &User, player: &User, home: &str, charge: &Trade, label: &str) -> EssResult {
    fail_if!(home.is_empty(), EssError::not_enough());
    let location = player.get_home(home).ok_or_else(EssError::not_enough)?;
    if user.get_world().as_ref() != Some(location.get_world()) && settings().is_world_home_permissions() {
        let node = format!("essentials.worlds.{}", location.get_world().get_name());
        fail_if!(!user.is_authorized(&node), EssError::tl("noPerm", crate::args![node]));
    }
    let future = get_new_exception_future(&user.get_source(), label);
    user.get_async_teleport().teleport(&location, Some(charge), "COMMAND", future.clone());
    let (told, name) = (user.clone(), home.to_owned());
    future.then_accept(move |success| {
        if success {
            told.send_tl("teleportHome", crate::args![name]);
        }
    });
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let [first] = args else {
        return Some(Vec::new());
    };
    let can_visit_others = user.is_authorized("essentials.home.others");
    let can_visit_bed = user.is_authorized("essentials.home.bed");
    let mut homes = user.get_homes();
    if can_visit_bed {
        homes.push("bed".to_owned());
    }
    if can_visit_others {
        match first.split_once(':') {
            None => homes.extend(get_players(&user.get_source()).into_iter().map(|player| format!("{player}:"))),
            Some((name, _)) => {
                let Ok(other) = get_player_raw(None, &[name.to_owned()], 0, true, true) else { return Some(homes) };
                homes.extend(other.get_homes().into_iter().map(|home| format!("{name}:{home}")));
                if can_visit_bed {
                    homes.push(format!("{name}:bed"));
                }
            }
        }
    }
    Some(homes)
}
