//! `Commandclearinventory`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("clearinventory") };

const EXTENDED_CAP: usize = 8;

fn run_user(user: &User, label: &str, args: &[String]) -> EssResult {
    let allow_all = user.is_authorized("essentials.clearinventory.all") || user.is_authorized("essentials.clearinventory.multiple");
    parse_command(&user.get_source(), label, args, user.is_authorized("essentials.clearinventory.others"), allow_all)
}

fn run_source(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    parse_command(sender, label, args, true, true)
}

fn parse_command(sender: &CommandSource, label: &str, args: &[String], allow_others: bool, allow_all: bool) -> EssResult {
    let sender_user = sender.get_user();
    let mut players: Vec<Player> = sender.get_player().cloned().into_iter().collect();
    let mut previous = None;
    let mut offset = 0;
    if let Some(user) = &sender_user {
        previous = user.get_confirming_clear_command();
        user.set_confirming_clear_command(None);
    }
    if allow_all && args.first().is_some_and(|first| first == "*") {
        sender.send_tl("inventoryClearingFromAll", &[]);
        offset = 1;
        players = ess().get_online_players();
    } else if allow_others && args.first().is_some_and(|first| first.trim().chars().count() > 2) {
        offset = 1;
        players = horizon_bukkit::world::match_player(args[0].trim());
    }
    fail_if!(players.is_empty(), EssError::player_not_found());
    let formatted = if args.is_empty() { format!("/{label}") } else { format!("/{label} {}", args.join(" ")) };
    if let Some(user) = sender_user.as_ref().filter(|user| user.is_prompting_clear_confirm())
        && previous.as_deref() != Some(&formatted)
    {
        user.set_confirming_clear_command(Some(formatted.clone()));
        user.send_tl("confirmClear", crate::args![formatted]);
        return Ok(());
    }
    let extended = players.len() < EXTENDED_CAP;
    for player in &players {
        clear_handler(sender, player, args, offset, extended)?;
    }
    Ok(())
}

fn clear_handler(sender: &CommandSource, player: &Player, args: &[String], offset: usize, show_extended: bool) -> EssResult {
    let display = player.display_name().to_legacy();
    let mut armor = false;
    let mut specific = false;
    let mut items: Vec<Material> = Vec::new();
    let amount = args.get(offset + 1).filter(|text| is_int(text)).and_then(|text| text.parse().ok()).unwrap_or(-1);
    if let Some(word) = args.get(offset) {
        if word == "**" {
            armor = true;
        } else if word != "*" {
            for item in java_split(word, ',') {
                let id = java_split(&item, ':').into_iter().next().unwrap_or_default();
                if let Ok(stack) = item_db().get(&id) {
                    let material = stack.get_type().clone();
                    if !items.contains(&material) {
                        items.push(material);
                    }
                }
            }
            specific = true;
        }
    }
    if !specific {
        if show_extended {
            sender.send_tl(if armor { "inventoryClearingAllArmor" } else { "inventoryClearingAllItems" }, crate::args![display]);
        }
        inventories::remove_items(player, |_| true, armor);
        return Ok(());
    }
    for material in items {
        let stack = ItemStack::new(material, 1);
        let name = stack.get_type().name().to_lowercase();
        fail_if!(amount < -1, EssError::tl("cannotRemoveNegativeItems", &[]));
        if amount == -1 {
            let removed = inventories::remove_item_similar(player, &stack, true);
            if removed > 0 || show_extended {
                sender.send_tl("inventoryClearingStack", crate::args![removed, name, display.clone()]);
            }
        } else if inventories::remove_item_amount(player, &stack, amount) {
            sender.send_tl("inventoryClearingStack", crate::args![amount, name, display.clone()]);
        } else if show_extended {
            sender.send_tl("inventoryClearFail", crate::args![display.clone(), amount, name]);
        }
    }
    Ok(())
}

fn items_and_stars(args: &[String]) -> Vec<String> {
    let mut items = get_items(args);
    items.push("*".to_owned());
    items.push("**".to_owned());
    items
}

fn tab_user(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match (user.is_authorized("essentials.clearinventory.others"), args.len()) {
        (true, 1) => {
            let mut options = get_players(&user.get_source());
            if user.is_authorized("essentials.clearinventory.all") || user.is_authorized("essentials.clearinventory.multiple") {
                options.push("*".to_owned());
            }
            options
        }
        (true, 2) | (false, 1) => items_and_stars(args),
        _ => Vec::new(),
    })
}

fn tab_source(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => {
            let mut options = get_players(sender);
            options.push("*".to_owned());
            options
        }
        2 => items_and_stars(args),
        _ => Vec::new(),
    })
}
