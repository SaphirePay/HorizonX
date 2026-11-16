//! `Commandmore`.
use crate::prelude::*;
use crate::utils::number::is_positive_int;

pub const COMMAND: Command = Command { run_user: Some(run), ..Command::new("more") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let mut stack = user.get_item_in_hand().filter(|stack| !stack.get_type().is_air()).ok_or_else(|| EssError::tl("cantSpawnItem", crate::args!["Air"]))?;
    let can_oversized = user.is_authorized("essentials.oversizedstacks");
    let cap = if can_oversized { settings().get_oversized_stack_size() } else { stack.get_max_stack_size() };
    fail_if!(stack.get_amount() >= cap, EssError::tl("fullStack", &[]));
    fail_if!(!user.can_spawn_item(stack.get_type()), EssError::tl("cantSpawnItem", crate::args![stack.get_type().name().to_lowercase().replace('_', "")]));
    let new_size = if let Some(first) = args.first() {
        fail_if!(!is_positive_int(first), EssError::tl("nonZeroPosNumber", &[]));
        let wanted = i64::from(stack.get_amount()) + i64::from(first.parse::<i32>().unwrap_or(0));
        if wanted > i64::from(cap) {
            user.send_tl(if can_oversized { "fullStackDefaultOversize" } else { "fullStackDefault" }, crate::args![cap]);
            cap
        } else {
            wanted as i32
        }
    } else {
        cap
    };
    stack.set_amount(new_size);
    if let Some(player) = user.player() {
        inventories::set_item_in_hand(&player, &stack);
    }
    Ok(())
}
