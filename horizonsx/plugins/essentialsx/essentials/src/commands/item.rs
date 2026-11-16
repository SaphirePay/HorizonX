//! `Commanditem`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("item") };

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let mut stack = item_db().get(first)?;
    let item_key = stack.get_type().name().to_lowercase().replace('_', "");
    fail_if!(!user.can_spawn_item(stack.get_type()), EssError::tl("cantSpawnItem", crate::args![item_key]));
    let requested = match args.get(1) {
        Some(text) => Some(text.parse::<i32>().map_err(|_| EssError::not_enough())?),
        None => None,
    };
    if let Some(amount) = requested.filter(|amount| *amount > 0) {
        stack.set_amount(amount);
    } else if settings().get_default_stack_size() > 0 {
        stack.set_amount(settings().get_default_stack_size());
    } else if settings().get_oversized_stack_size() > 0 && user.is_authorized("essentials.oversizedstacks") {
        stack.set_amount(settings().get_oversized_stack_size());
    }
    let mut meta = MetaItemStack::new(stack.clone());
    fail_if!(!meta.can_spawn(), EssError::tl("unableToSpawnItem", crate::args![item_key]));
    if args.len() > 2 {
        let allow_unsafe = settings().allow_unsafe_enchantments() && user.is_authorized("essentials.enchantments.allowunsafe");
        meta.parse_string_meta(Some(&user.get_source()), allow_unsafe, args, 2)?;
        stack = meta.into_item_stack();
    }
    fail_if!(stack.get_type().is_air(), EssError::tl("cantSpawnItem", crate::args!["Air"]));
    user.send_tl("itemSpawn", crate::args![stack.get_amount(), stack.get_type().name().to_lowercase().replace('_', " ")]);
    if let Some(player) = user.player() {
        let max = if user.is_authorized("essentials.oversizedstacks") { settings().get_oversized_stack_size() } else { 0 };
        inventories::add_item(&player, max, false, &[stack]);
    }
    Ok(())
}

fn tab(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_items(args),
        2 => vec!["1".to_owned(), "64".to_owned()],
        3 => vec!["0".to_owned()],
        _ => Vec::new(),
    })
}
