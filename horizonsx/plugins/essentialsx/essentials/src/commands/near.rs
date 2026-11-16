//! `Commandnear`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("near") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    let mut max_radius = i64::from(settings().get_near_radius());
    if max_radius == 0 {
        max_radius = 200;
    }
    let mut radius = max_radius;
    let mut other = None;
    if let Some(first) = args.first() {
        match first.parse::<i64>() {
            Ok(number) => radius = number,
            Err(_) => other = get_player_for(user, args, 0).ok(),
        }
        if other.is_some()
            && let Some(number) = args.get(1).and_then(|text| text.parse::<i64>().ok())
        {
            radius = number;
        }
    }
    radius = radius.wrapping_abs();
    if radius > max_radius && !user.is_authorized("essentials.near.maxexempt") {
        user.send_tl("radiusTooBig", crate::args![max_radius]);
        radius = max_radius;
    }
    let target = other.filter(|_| user.is_authorized("essentials.near.others")).unwrap_or_else(|| user.clone());
    user.send_tl("nearbyPlayers", crate::args![parsed(get_local(&user.get_source(), &target, radius))]);
    Ok(())
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let other = get_player_raw(None, args, 0, true, false)?;
    let radius = args.get(1).and_then(|text| text.parse::<i64>().ok()).unwrap_or(200);
    sender.send_tl("nearbyPlayers", crate::args![parsed(get_local(sender, &other, radius))]);
    Ok(())
}

fn get_local(source: &CommandSource, user: &User, radius: i64) -> String {
    let (Some(location), Some(base)) = (user.get_location(), user.player()) else {
        return source.tl("none", &[]);
    };
    let radius_squared = radius.wrapping_mul(radius);
    let show_hidden = user.can_interact_vanished();
    let mut nearby = Vec::new();
    for player in ess().get_online_users() {
        if player == *user || player.is_authorized("essentials.near.exclude") || (player.is_hidden_to(&base) && !show_hidden && player.is_hidden_from(&base)) {
            continue;
        }
        let Some(at) = player.get_location().filter(|at| at.world == location.world) else { continue };
        if (at.distance_squared(&location) as i64) < radius_squared {
            nearby.push((at.distance(&location), player));
        }
    }
    nearby.sort_by(|a, b| a.0.total_cmp(&b.0));
    if nearby.is_empty() {
        return source.tl("none", &[]);
    }
    nearby.iter().map(|(distance, player)| user.player_tl("nearbyPlayersList", crate::args![player.get_display_name(), *distance as i64])).collect::<Vec<_>>().join(", ")
}

fn near_radius() -> Vec<String> {
    vec![settings().get_near_radius().to_string()]
}

fn tab_user(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match (user.is_authorized("essentials.near.others"), args.len()) {
        (true, 1) => get_players(&user.get_source()),
        (true, 2) | (false, 1) => near_radius(),
        _ => Vec::new(),
    })
}

fn tab_source(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_players(sender),
        2 => near_radius(),
        _ => Vec::new(),
    })
}
