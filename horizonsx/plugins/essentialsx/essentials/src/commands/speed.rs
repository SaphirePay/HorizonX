//! `Commandspeed`.
use crate::prelude::*;
use horizon_bukkit::world::match_player;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("speed") };

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let (is_fly, speed) = (is_fly_mode(&args[0])?, get_move_speed(&args[1])?);
    let name = args.get(2).ok_or_else(|| EssError::Message("Index 2 out of bounds for length 2".to_owned()))?;
    speed_other_players(sender, is_fly, true, speed, name)
}

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let Some(player) = user.player() else { return Ok(()) };
    let is_bypass = user.is_authorized("essentials.speed.bypass");
    let (is_fly, speed);
    if let [only] = args {
        let inferred = if is_fly_alias(label) {
            true
        } else if is_walk_alias(label) {
            false
        } else {
            player.is_flying()
        };
        is_fly = fly_perm_check(user, inferred);
        speed = get_move_speed(only)?;
    } else {
        is_fly = fly_perm_check(user, is_fly_mode(&args[0])?);
        speed = get_move_speed(&args[1])?;
        if let Some(name) = args.get(2).filter(|_| user.is_authorized("essentials.speed.others")) {
            fail_if!(name.trim().chars().count() < 2, EssError::player_not_found());
            return speed_other_players(&user.get_source(), is_fly, is_bypass, speed, name);
        }
    }
    if is_fly {
        player.set_fly_speed(get_real_move_speed(speed, true, is_bypass));
        user.send_tl("moveSpeed", crate::args![parsed(user.player_tl("flying", &[])), speed, user.get_display_name()]);
    } else {
        player.set_walk_speed(get_real_move_speed(speed, false, is_bypass));
        user.send_tl("moveSpeed", crate::args![parsed(user.player_tl("walking", &[])), speed, user.get_display_name()]);
    }
    Ok(())
}

fn speed_other_players(sender: &CommandSource, is_fly: bool, is_bypass: bool, speed: f32, name: &str) -> EssResult {
    let skip = skip_hidden(sender);
    let mut found = false;
    for player in match_player(name) {
        let Some(user) = ess().get_user(&player) else { continue };
        if skip && hidden_from(sender, &user) {
            continue;
        }
        found = true;
        if is_fly {
            player.set_fly_speed(get_real_move_speed(speed, true, is_bypass));
            sender.send_tl("moveSpeed", crate::args![parsed(sender.tl("flying", &[])), speed, user.get_display_name()]);
        } else {
            player.set_walk_speed(get_real_move_speed(speed, false, is_bypass));
            sender.send_tl("moveSpeed", crate::args![parsed(sender.tl("walking", &[])), speed, user.get_display_name()]);
        }
    }
    if found { Ok(()) } else { Err(EssError::player_not_found()) }
}

fn fly_perm_check(user: &User, input: bool) -> bool {
    let can_fly = user.is_authorized("essentials.speed.fly");
    let can_walk = user.is_authorized("essentials.speed.walk");
    if (input && can_fly) || (!input && can_walk) || (!can_fly && !can_walk) { input } else { !can_walk }
}

fn is_fly_alias(label: &str) -> bool {
    label.contains("fly") || label.eq_ignore_ascii_case("fspeed") || label.eq_ignore_ascii_case("efspeed")
}

fn is_walk_alias(label: &str) -> bool {
    label.contains("walk") || label.eq_ignore_ascii_case("wspeed") || label.eq_ignore_ascii_case("ewspeed")
}

fn is_fly_mode(mode: &str) -> EssResult<bool> {
    if mode.contains("fly") || mode.eq_ignore_ascii_case("f") {
        Ok(true)
    } else if mode.contains("walk") || mode.contains("run") || mode.eq_ignore_ascii_case("w") || mode.eq_ignore_ascii_case("r") {
        Ok(false)
    } else {
        Err(EssError::not_enough())
    }
}

fn get_move_speed(text: &str) -> EssResult<f32> {
    Ok(parse_float(text).ok_or_else(EssError::not_enough)?.clamp(0.0001, 10.0))
}

fn get_real_move_speed(speed: f32, is_fly: bool, is_bypass: bool) -> f32 {
    let default = if is_fly { 0.1 } else { 0.2 };
    let max = if is_bypass {
        1.0
    } else if is_fly {
        settings().get_max_fly_speed() as f32
    } else {
        settings().get_max_walk_speed() as f32
    };
    if speed < 1.0 { default * speed } else { (speed - 1.0) / 9.0 * (max - default) + default }
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let list = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect();
    Some(match args.len() {
        1 => list(&["walk", "fly", "1", "1.5", "1.75", "2"]),
        2 => list(&["1", "1.5", "1.75", "2"]),
        3 if sender.is_authorized("essentials.speed.others") => get_players(sender),
        _ => Vec::new(),
    })
}
