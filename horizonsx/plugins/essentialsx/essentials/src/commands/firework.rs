//! `Commandfirework`.
use crate::prelude::*;
use crate::utils::material::is_firework;
use horizon_bukkit::entities::Firework;
use horizon_bukkit::meta::FireworkMeta;
use serde_json::json;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("firework") };

const DYES: [&str; 16] = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];

fn explosions(stack: &ItemStack) -> usize {
    stack.get_component("minecraft:fireworks").and_then(|fireworks| fireworks["explosions"].as_array()).map_or(0, Vec::len)
}

fn keep(user: &User, stack: &ItemStack) {
    if let Some(player) = user.player() {
        inventories::set_item_in_hand(&player, stack);
    }
}

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let stack = user.get_item_in_hand().filter(|stack| is_firework(stack.get_type())).ok_or_else(|| EssError::tl("holdFirework", &[]))?;
    if first.eq_ignore_ascii_case("clear") {
        let mut stack = stack;
        let mut fireworks = stack.get_component("minecraft:fireworks").cloned().unwrap_or_else(|| json!({}));
        fireworks["explosions"] = json!([]);
        stack.set_component("minecraft:fireworks", fireworks);
        keep(user, &stack);
        user.send_tl("fireworkEffectsCleared", &[]);
    } else if args.len() > 1 && (first.eq_ignore_ascii_case("power") || first.eq_ignore_ascii_case("p")) {
        let power: i32 = args[1].parse().map_err(|_| EssError::tl("invalidFireworkFormat", crate::args![&args[1], first]))?;
        let mut meta = FireworkMeta(stack);
        meta.set_power(if power > 3 { 4 } else { power });
        keep(user, &meta.0);
    } else if (first.eq_ignore_ascii_case("fire") || first.eq_ignore_ascii_case("f")) && user.is_authorized("essentials.firework.fire") {
        let mut amount = 1;
        let mut direction = false;
        if let Some(second) = args.get(1) {
            if is_int(second) {
                let limit = settings().get_spawn_mob_limit();
                amount = second.parse().unwrap_or(1);
                if amount > limit {
                    amount = limit;
                    user.send_tl("mobSpawnLimit", &[]);
                }
            } else {
                direction = true;
            }
        }
        let Some(player) = user.player() else { return Ok(()) };
        let at = player.get_location();
        for _ in 0..amount {
            let Some(rocket) = at.world.spawn(&at, "firework_rocket") else { continue };
            let mut meta = FireworkMeta(stack.clone());
            if direction {
                let looking = player.get_eye_location().get_direction();
                if meta.get_power().unwrap_or(0) > 1 {
                    meta.set_power(1);
                }
                rocket.set_velocity(looking.x * 0.07, looking.y * 0.07, looking.z * 0.07);
            }
            Firework(rocket).set_item(&meta.0);
        }
    } else {
        let had = explosions(&stack);
        let source = user.get_source();
        let mut meta = MetaItemStack::new(stack);
        for arg in args {
            if let Err(error) = meta.add_firework_meta(Some(&source), true, arg) {
                user.send_tl("fireworkSyntax", &[]);
                return Err(error);
            }
        }
        if !meta.is_valid_firework() {
            user.send_tl("fireworkSyntax", &[]);
            return Err(EssError::tl("fireworkColor", &[]));
        }
        fail_if!(had > 0 && !user.is_authorized("essentials.firework.multiple"), EssError::tl("multipleCharges", &[]));
        meta.finish_firework(Some(&source))?;
        keep(user, meta.get_item_stack());
    }
    Ok(())
}

fn colors(prefix: &str) -> Vec<String> {
    DYES.iter().map(|dye| format!("{prefix}{dye},")).collect()
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let first = args.first().map(String::as_str).unwrap_or_default();
    Some(match args.len() {
        1 if first.starts_with("color:") => colors(first.rfind(',').map_or("color:", |comma| &first[..=comma])),
        1 => {
            let mut options = ["clear", "power", "color:"].map(str::to_owned).to_vec();
            if user.is_authorized("essentials.firework.fire") {
                options.push("fire".to_owned());
            }
            options
        }
        2 if first == "power" => ["1", "2", "3", "4"].map(str::to_owned).to_vec(),
        2 if first == "fire" => vec!["1".to_owned()],
        2 if first.starts_with("color:") => {
            let second = args[1].as_str();
            colors(if second.starts_with("fade:") { second.rfind(',').map_or("fade:", |comma| &second[..=comma]) } else { "fade:" })
        }
        3 if first.starts_with("color:") => ["shape:star", "shape:ball", "shape:large", "shape:creeper", "shape:burst"].map(str::to_owned).to_vec(),
        4 if first.starts_with("color:") => ["effect:trail", "effect:twinkle", "effect:trail,twinkle", "effect:twinkle,trail"].map(str::to_owned).to_vec(),
        _ => Vec::new(),
    })
}
