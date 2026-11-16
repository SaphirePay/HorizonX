//! `Commanditemdb`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("itemdb") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    let (stack, held) = match args.first() {
        Some(first) => (item_db().get(first)?, false),
        None => {
            let user = sender.get_user().ok_or_else(EssError::not_enough)?;
            (user.get_item_in_hand().unwrap_or_else(inventories::air), true)
        }
    };
    sender.send_tl("itemType", crate::args![stack.get_type().name(), "none"]);
    if held && !stack.get_type().is_air() {
        let max_uses = i32::from(stack.get_type().get_max_durability());
        if max_uses != 0 {
            sender.send_tl("durability", crate::args![((max_uses + 1) - stack.get_damage()).to_string()]);
        }
    }
    let mut names = item_db().name_list(&stack);
    if let Some(name) = item_db().name(&stack) {
        names.extend(item_db().get_aliases_for(&name));
    }
    if names.is_empty() {
        return Ok(());
    }
    names.sort();
    names.truncate(if names.len() > 15 { 14 } else { 15 });
    sender.send_tl("itemNames", crate::args![names.join(", ")]);
    Ok(())
}

fn tab(_sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { get_items(args) } else { Vec::new() })
}
