//! `Commandhat`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("hat") };

const PERM_PREFIX: &str = "essentials.hat.prevent-type.";

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let Some(player) = user.player() else { return Ok(()) };
    let inventory = player.get_inventory();
    let bound = |head: &horizon_bukkit::ItemStack| head.has_enchant("binding_curse") && !user.is_authorized("essentials.hat.ignore-binding");
    if args.first().is_none_or(|first| !first.contains("rem") && !first.contains("off") && !first.eq_ignore_ascii_case("0")) {
        let Some(hand) = inventories::get_item_in_main_hand(&player).filter(|hand| !hand.get_type().is_air()) else {
            user.send_tl("hatFail", &[]);
            return Ok(());
        };
        let wildcard = user.is_authorized_exact(&format!("{PERM_PREFIX}*"));
        let material = user.is_authorized_exact(&format!("{PERM_PREFIX}{}", hand.get_type().name().to_lowercase()));
        if (wildcard == Some(true) && material != Some(false)) || (wildcard != Some(true) && material == Some(true)) {
            user.send_tl("hatFail", &[]);
            return Ok(());
        }
        if hand.get_type().get_max_durability() != 0 {
            user.send_tl("hatArmor", &[]);
            return Ok(());
        }
        let head = inventory.get_helmet();
        if head.as_ref().is_some_and(bound) {
            user.send_tl("hatCurse", &[]);
            return Ok(());
        }
        inventory.set_helmet(Some(&hand));
        inventories::set_item_in_main_hand(&player, &head.unwrap_or_else(inventories::air));
        user.send_tl("hatPlaced", &[]);
        return Ok(());
    }
    match inventory.get_helmet().filter(|head| !head.get_type().is_air()) {
        None => user.send_tl("hatEmpty", &[]),
        Some(head) if bound(&head) => user.send_tl("hatCurse", &[]),
        Some(head) => {
            inventory.set_helmet(None);
            inventories::add_item(&player, 0, false, &[head]);
            user.send_tl("hatRemoved", &[]);
        }
    }
    Ok(())
}

fn tab(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { vec!["remove".to_owned(), "wear".to_owned()] } else { Vec::new() })
}
