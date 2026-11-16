//! `Commandheal`, an `EssentialsLoopCommand`.
//!
//! `EntityRegainHealthEvent` isn't fired: no listener could see it.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_source: Some(tab), ..Command::new("heal") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    if !user.is_authorized("essentials.heal.cooldown.bypass") {
        user.heal_cooldown()?;
    }
    let source = user.get_source();
    if let Some(first) = args.first().filter(|_| user.is_authorized("essentials.heal.others")) {
        return loop_online_players(&source, true, true, first, |target| update_player(&source, target));
    }
    update_player(&source, user)
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    loop_online_players(sender, true, true, first, |target| update_player(sender, target))
}

fn update_player(sender: &CommandSource, user: &User) -> EssResult {
    let Some(player) = user.player() else { return Ok(()) };
    fail_if!(player.get_health() == 0.0, EssError::player_exempt("healDead", &[]));
    player.set_health(player.get_max_health());
    player.set_food_level(20);
    player.set_fire_ticks(0);
    player.set_remaining_air(player.get_maximum_air());
    user.send_tl("heal", &[]);
    if settings().is_removing_effects_on_heal() {
        for effect in player.get_active_potion_effects() {
            player.remove_potion_effect(effect.get_type());
        }
    }
    if sender.get_player() != Some(&player) {
        sender.send_tl("healOther", crate::args![user.get_display_name()]);
    }
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && sender.is_authorized("essentials.heal.others") { get_players_with_wildcards(sender) } else { Vec::new() })
}
