//! `Commandsethome`.
use crate::prelude::*;
use crate::utils::location::is_block_unsafe_for_user;

pub const COMMAND: Command = Command { run_user: Some(run), tab_source: Some(|_, _, _| Some(Vec::new())), ..Command::new("sethome") };

const TWO_MINUTES: i64 = 120_000;

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let mut users_home = user.clone();
    let mut name = "home".to_owned();
    if let Some(first) = args.first() {
        // Both `/sethome khobbits house` and `/sethome khobbits:house`.
        let name_parts = java_split(first, ':');
        let args = if name_parts.first().map_or(0, String::len) != first.len() { name_parts } else { args.to_vec() };
        match args.get(1) {
            None => name = args.first().map_or_else(String::new, |only| only.to_lowercase()),
            Some(second) => {
                name = second.to_lowercase();
                if user.is_authorized("essentials.sethome.others") {
                    users_home = get_player_term_raw(&args[0], true, true)?;
                }
            }
        }
    }
    if check_home_limit(user, &users_home, &name)? {
        "home".clone_into(&mut name);
    }
    fail_if!(name == "bed" || is_int(&name), EssError::tl("invalidHomeName", &[]));
    let Some(location) = user.get_location() else { return Ok(()) };
    let world = location.get_world();
    let (x, y, z) = (location.get_block_x(), location.get_block_y(), location.get_block_z());
    fail_if!((!settings().is_teleport_safety_enabled() || !settings().is_force_disable_teleport_safety()) && is_block_unsafe_for_user(&users_home, world, x, y, z), EssError::tl("unsafeTeleportDestination", crate::args![world.get_name(), x, y, z]));
    let last = users_home.get_last_home_confirmation();
    let repeated = last.as_deref() == Some(name.as_str());
    if settings().is_confirm_home_overwrite() && users_home.has_home(&name) && (!repeated || now_ms() - users_home.get_last_home_confirmation_timestamp() > TWO_MINUTES) {
        users_home.set_last_home_confirmation(Some(name.clone()));
        users_home.set_last_home_confirmation_timestamp();
        user.send_tl("homeConfirmation", crate::args![name]);
        return Ok(());
    }
    users_home.set_home(&name, &location);
    user.send_tl("homeSet", crate::args![world.get_name(), x, y, z, name]);
    users_home.set_last_home_confirmation(None);
    Ok(())
}

fn check_home_limit(user: &User, users_home: &User, name: &str) -> EssResult<bool> {
    if user.is_authorized("essentials.sethome.multiple.unlimited") {
        return Ok(false);
    }
    let limit = settings().get_home_limit(user);
    let homes = users_home.get_homes();
    if homes.len() >= limit as usize {
        if homes.iter().any(|home| home == name) {
            return Ok(false);
        }
        return Err(EssError::tl("maxHomes", crate::args![limit]));
    }
    Ok(limit == 1)
}
