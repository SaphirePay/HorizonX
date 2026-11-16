//! `Commandlightning`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("lightning") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    if args.is_empty() || !sender.is_authorized("essentials.lightning.others") {
        let (Some(user), Some(player)) = (sender.get_user(), sender.get_player()) else {
            return Err(EssError::not_enough());
        };
        let target = user.get_target_block(600).map_or_else(
            || {
                let mut eyes = player.get_eye_location();
                let direction = eyes.get_direction();
                eyes.add(direction.x * 600.0, direction.y * 600.0, direction.z * 600.0);
                eyes
            },
            |block| block.get_location(),
        );
        player.get_world().strike_lightning(&target);
        return Ok(());
    }
    let power = args.get(1).and_then(|text| text.parse::<i32>().ok()).unwrap_or(5);
    loop_online_players(sender, false, true, &args[0], |player| {
        sender.send_tl("lightningUse", crate::args![player.get_display_name()]);
        if let Some(base) = player.player() {
            base.get_world().strike_lightning_effect(&base.get_location());
            if !player.is_god_mode_enabled() {
                base.damage(f64::from(power), "minecraft:lightning_bolt");
            }
        }
        if settings().warn_on_smite() {
            player.send_tl("lightningSmited", &[]);
        }
        Ok(())
    })
}

fn tab_user(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if user.is_authorized("essentials.lightning.others") { tab_source(&user.get_source(), "", args) } else { Some(Vec::new()) }
}

fn tab_source(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_players(sender),
        2 => vec!["5".to_owned()],
        _ => Vec::new(),
    })
}
