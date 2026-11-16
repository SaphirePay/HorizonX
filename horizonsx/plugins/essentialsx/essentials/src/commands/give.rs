//! `Commandgive`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("give") };

fn run(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let mut stack = item_db().get(&args[1])?;
    let item_key = stack.get_type().name().to_lowercase().replace('_', "");
    fail_if!(sender.get_user().is_some_and(|user| !user.can_spawn_item(stack.get_type())), EssError::tl("cantSpawnItem", crate::args![item_key]));
    let requested = match args.get(2) {
        Some(text) => Some(text.parse::<i32>().map_err(|_| EssError::not_enough())?),
        None => None,
    };
    if let Some(amount) = requested.filter(|amount| *amount > 0) {
        stack.set_amount(amount);
    } else if settings().get_default_stack_size() > 0 {
        stack.set_amount(settings().get_default_stack_size());
    } else if settings().get_oversized_stack_size() > 0 && sender.is_authorized("essentials.oversizedstacks") {
        stack.set_amount(settings().get_oversized_stack_size());
    }
    let mut meta = MetaItemStack::new(stack.clone());
    fail_if!(!meta.can_spawn(), EssError::tl("unableToSpawnItem", crate::args![item_key]));
    if args.len() > 3 {
        let mut allow_unsafe = settings().allow_unsafe_enchantments();
        if sender.get_user().is_some_and(|user| !user.is_authorized("essentials.enchantments.allowunsafe")) {
            allow_unsafe = false;
        }
        let start = if is_int(&args[3]) { 4 } else { 3 };
        if args.len() > start {
            meta.parse_string_meta(Some(sender), allow_unsafe, args, start)?;
        }
        stack = meta.into_item_stack();
    }
    fail_if!(stack.get_type().is_air(), EssError::tl("cantSpawnItem", crate::args!["Air"]));
    let item_name = stack.get_type().name().to_lowercase().replace('_', " ");
    let drop_if_full = settings().is_drop_items_if_full();
    loop_online_players(sender, false, true, &args[0], |player| {
        sender.send_tl("giveSpawn", crate::args![stack.get_amount(), item_name.clone(), player.get_display_name()]);
        let Some(base) = player.player() else { return Ok(()) };
        let max = if player.is_authorized("essentials.oversizedstacks") { settings().get_oversized_stack_size() } else { 0 };
        for item in inventories::add_item(&base, max, false, std::slice::from_ref(&stack)).into_values() {
            if drop_if_full {
                base.get_world().drop_item_naturally(&base.get_location(), &item);
            } else {
                sender.send_tl("giveSpawnFailure", crate::args![item.get_amount(), item_name.clone(), player.get_display_name()]);
            }
        }
        Ok(())
    })
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_players(sender),
        2 => get_items(args),
        3 => vec!["1".to_owned(), "64".to_owned()],
        4 => vec!["0".to_owned()],
        _ => Vec::new(),
    })
}
