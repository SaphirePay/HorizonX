//! `Commandenchant`.
use crate::prelude::*;
use crate::items::{can_enchant_item, enchantment_by_name, enchantment_entries, enchantment_max_level, enchantment_names, enchantment_real_name};
use std::collections::BTreeSet;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("enchant") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let stack = user.get_item_in_hand().filter(|stack| !stack.get_type().is_air()).ok_or_else(|| EssError::tl("nothingInHand", &[]))?;
    if args.is_empty() {
        let mut usable = BTreeSet::new();
        for (key, real) in enchantment_entries() {
            if usable.contains(real) || (user.is_authorized(&format!("essentials.enchantments.{real}")) && can_enchant_item(real, stack.get_type())) {
                usable.insert(key.to_owned());
            }
        }
        let list: Vec<String> = usable.into_iter().collect();
        return Err(EssError::NotEnoughArguments(user.player_tl("enchantments", crate::args![join_list(&list)])));
    }
    let level = match args.get(1) {
        Some(text) => text.parse::<i32>().map_err(|_| EssError::not_enough())?,
        None => 1,
    };
    let mut meta = MetaItemStack::new(stack);
    let enchantment = meta.get_enchantment(Some(user), &args[0])?.ok_or_else(|| EssError::tl("enchantmentNotFound", &[]))?;
    meta.add_enchantment(settings().allow_unsafe_enchantments() && user.is_authorized("essentials.enchantments.allowunsafe"), &enchantment, level)?;
    if let Some(player) = user.player() {
        inventories::set_item_in_hand(&player, meta.get_item_stack());
    }
    let name = enchantment_real_name(&enchantment).replace('_', " ");
    user.send_tl(if level == 0 { "enchantmentRemoved" } else { "enchantmentApplied" }, crate::args![name]);
    Ok(())
}

fn tab(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => enchantment_names().iter().map(|name| (*name).to_owned()).collect(),
        2 => enchantment_by_name(&args[0]).map_or_else(Vec::new, |enchantment| (1..=enchantment_max_level(&enchantment)).map(|level| level.to_string()).collect()),
        _ => Vec::new(),
    })
}
