//! `Commandrepair`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("repair") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    if args.first().is_none_or(|first| first.eq_ignore_ascii_case("hand")) || !user.is_authorized("essentials.repair.all") {
        repair_hand(user)
    } else if args[0].eq_ignore_ascii_case("all") {
        let charge = Trade::of_command("repair-all");
        charge.is_affordable_for(user)?;
        repair_all(user)?;
        charge.charge(user)
    } else {
        Err(EssError::not_enough())
    }
}

fn get_charge(material: &Material) -> Trade {
    Trade::of_command_with_fallback(&format!("repair-{}", material.name().to_lowercase().replace('_', "-")), Trade::of_command("repair-item"))
}

fn repair_item(item: &mut ItemStack) -> EssResult {
    let material = item.get_type();
    fail_if!(material.is_block() || material.get_max_durability() < 1, EssError::tl("repairInvalidType", &[]));
    fail_if!(item.get_damage() == 0, EssError::tl("repairAlreadyFixed", &[]));
    item.set_damage(0);
    Ok(())
}

pub fn repair_hand(user: &User) -> EssResult {
    let Some(mut item) = user.get_item_in_hand().filter(|item| !item.get_type().is_block() && item.get_damage() != 0) else {
        return Err(EssError::tl("repairInvalidType", &[]));
    };
    fail_if!(item.has_enchants() && !settings().get_repair_enchanted() && !user.is_authorized("essentials.repair.enchanted"), EssError::tl("repairEnchanted", &[]));
    let name = item.get_type().name().to_lowercase();
    let charge = get_charge(item.get_type());
    charge.is_affordable_for(user)?;
    repair_item(&mut item)?;
    charge.charge(user)?;
    if let Some(player) = user.player() {
        inventories::set_item_in_hand(&player, &item);
    }
    user.send_tl("repair", crate::args![name.replace('_', " ")]);
    Ok(())
}

pub fn repair_all(user: &User) -> EssResult {
    let Some(player) = user.player() else { return Ok(()) };
    let mut repaired = Vec::new();
    let mut slots: Vec<i32> = (0..36).chain(std::iter::once(40)).collect();
    if user.is_authorized("essentials.repair.armor") {
        slots.extend(36..40);
    }
    for slot in slots {
        let Some(mut item) = inventories::get_item(&player, slot).filter(|item| !item.get_type().is_block() && item.get_damage() != 0) else { continue };
        let name = item.get_type().name().to_lowercase();
        let charge = get_charge(item.get_type());
        if let Err(error) = charge.is_affordable_for(user) {
            user.send_message(&error.get_message());
            continue;
        }
        if item.has_enchants() && !settings().get_repair_enchanted() && !user.is_authorized("essentials.repair.enchanted") {
            continue;
        }
        if repair_item(&mut item).is_err() {
            continue;
        }
        inventories::set_slot(&player, slot, Some(&item));
        if let Err(error) = charge.charge(user) {
            user.send_message(&error.get_message());
        }
        repaired.push(name.replace('_', " "));
    }
    fail_if!(repaired.is_empty(), EssError::tl("repairNone", &[]));
    user.send_tl("repair", crate::args![join_list(&repaired)]);
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() != 1 {
        return Some(Vec::new());
    }
    let mut options = vec!["hand".to_owned()];
    if user.is_authorized("essentials.repair.all") {
        options.push("all".to_owned());
    }
    Some(options)
}
