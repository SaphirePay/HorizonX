//! `Commandunlimited`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("unlimited") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let target = if args.len() > 1 && user.is_authorized("essentials.unlimited.others") { get_player_for(user, args, 1)? } else { user.clone() };
    if first.eq_ignore_ascii_case("list") {
        user.get_source().send_component(&deserialize_mini_message(&get_list(user, &target)));
    } else if first.eq_ignore_ascii_case("clear") {
        for material in target.get_unlimited() {
            toggle_unlimited(user, &target, &material)?;
        }
    } else {
        toggle_unlimited(user, &target, first)?;
    }
    Ok(())
}

fn get_list(send_to: &User, target: &User) -> String {
    let mut output = format!("{} ", send_to.player_tl("unlimitedItems", &[]));
    let items = target.get_unlimited();
    if items.is_empty() {
        output.push_str(&send_to.player_tl("none", &[]));
    }
    output.push_str(&items.iter().map(|material| material.to_lowercase().replace('_', "")).collect::<Vec<_>>().join(", "));
    output
}

fn toggle_unlimited(user: &User, target: &User, item: &str) -> EssResult {
    let mut stack = item_db().get(item)?;
    stack.set_amount(stack.get_type().get_max_stack_size().min(2));
    let name = stack.get_type().name();
    let item_name = name.to_lowercase().replace('_', "");
    let bucket = (name == "WATER_BUCKET" || name == "LAVA_BUCKET") && user.is_authorized("essentials.unlimited.item-bucket");
    fail_if!(settings().permission_based_item_spawn() && !user.is_authorized("essentials.unlimited.item-all") && !user.is_authorized(&format!("essentials.unlimited.item-{item_name}")) && !bucket, EssError::tl("unlimitedItemPermission", crate::args![item_name]));
    let enable = !target.has_unlimited(&stack);
    if enable
        && let Some(base) = target.player()
        && !inventories::contains_at_least(&base, &stack, stack.get_amount())
    {
        inventories::add_item(&base, 0, false, std::slice::from_ref(&stack));
    }
    let message = if enable { "enableUnlimited" } else { "disableUnlimited" };
    if user != target {
        user.send_tl(message, crate::args![item_name.clone(), target.get_display_name()]);
    }
    target.send_tl(message, crate::args![item_name, target.get_display_name()]);
    target.set_unlimited(&stack, enable);
    Ok(())
}
