//! `Commandpotion`.
use crate::prelude::*;
use crate::items::{potion_by_name, potion_names};
use std::collections::BTreeSet;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("potion") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let stack = user.get_item_in_hand().unwrap_or_else(inventories::air);
    let Some(first) = args.first() else {
        let mut usable = BTreeSet::new();
        for name in potion_names() {
            let real = potion_by_name(name).unwrap_or_default();
            if usable.contains(real.as_str()) || user.is_authorized(&format!("essentials.potions.{real}")) {
                usable.insert((*name).to_owned());
            }
        }
        let list: Vec<String> = usable.into_iter().collect();
        return Err(EssError::NotEnoughArguments(user.player_tl("potions", crate::args![join_list(&list)])));
    };
    fail_if!(!matches!(stack.get_type().name().as_str(), "POTION" | "SPLASH_POTION" | "LINGERING_POTION" | "TIPPED_ARROW"), EssError::tl("holdPotion", &[]));
    let Some(player) = user.player() else { return Ok(()) };
    if first.eq_ignore_ascii_case("clear") {
        let mut stack = stack;
        let mut contents = stack.get_component("minecraft:potion_contents").cloned().unwrap_or_default();
        if let Some(contents) = contents.as_object_mut() {
            contents.remove("custom_effects");
            stack.set_component("minecraft:potion_contents", serde_json::Value::Object(contents.clone()));
        }
        inventories::set_item_in_hand(&player, &stack);
    } else if first.eq_ignore_ascii_case("apply") && user.is_authorized("essentials.potion.apply") {
        let effects = stack.get_component("minecraft:potion_contents").and_then(|contents| contents["custom_effects"].as_array().cloned()).unwrap_or_default();
        for effect in effects {
            let id = effect["id"].as_str().unwrap_or_default();
            let ticks = effect["duration"].as_i64().unwrap_or(1).max(0);
            player.add_potion_effect(id, (ticks / 20).max(1) as u32, effect["amplifier"].as_i64().unwrap_or(0).clamp(0, 255) as u8);
        }
    } else if args.len() < 3 {
        return Err(EssError::not_enough());
    } else {
        let source = user.get_source();
        let mut meta = MetaItemStack::new(stack);
        for arg in args {
            meta.add_potion_meta(Some(&source), true, arg)?;
        }
        if !meta.complete_potion() {
            user.send_tl("invalidPotion", &[]);
            return Err(EssError::not_enough());
        }
        inventories::set_item_in_hand(&player, meta.get_item_stack());
    }
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let effect = args.first().is_some_and(|first| first.starts_with("effect:"));
    Some(match args.len() {
        1 => {
            let mut options = vec!["clear".to_owned()];
            if user.is_authorized("essentials.potion.apply") {
                options.push("apply".to_owned());
            }
            options.extend(potion_names().iter().filter(|name| user.is_authorized(&format!("essentials.potions.{}", potion_by_name(name).unwrap_or_default()))).map(|name| format!("effect:{name}")));
            options
        }
        2 if effect => ["power:1", "power:2", "power:3", "power:4", "amplifier:0", "amplifier:1", "amplifier:2", "amplifier:3"].map(str::to_owned).to_vec(),
        3 if effect => COMMON_DURATIONS.iter().map(|duration| format!("duration:{duration}")).collect(),
        4 if effect => vec!["splash:true".to_owned(), "splash:false".to_owned()],
        _ => Vec::new(),
    })
}
