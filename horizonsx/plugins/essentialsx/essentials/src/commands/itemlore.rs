//! `Commanditemlore`.
use crate::prelude::*;
use crate::utils::format::unformat_string_for;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("itemlore") };

fn keep(user: &User, item: &ItemStack) {
    if let Some(player) = user.player() {
        inventories::set_item_in_hand(&player, item);
    }
}

fn run(user: &User, _label: &str, args: &[String]) -> EssResult {
    let mut item = user.get_item_in_hand().filter(|item| !item.get_type().is_air()).ok_or_else(|| EssError::tl("itemloreInvalidItem", &[]))?;
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let mut lore = item.get_lore();
    if first.eq_ignore_ascii_case("add") && args.len() > 1 {
        fail_if!(lore.len() >= settings().get_max_item_lore() as usize && !user.is_authorized("essentials.itemlore.bypass"), EssError::tl("itemloreMaxLore", &[]));
        let line = format_string(user, "essentials.itemlore", &get_final_arg(args, 1)).trim().to_owned();
        lore.push(line.clone());
        item.set_lore(lore);
        keep(user, &item);
        user.send_tl("itemloreSuccess", crate::args![line]);
    } else if first.eq_ignore_ascii_case("clear") {
        item.set_lore(Vec::new());
        keep(user, &item);
        user.send_tl("itemloreClear", &[]);
    } else if first.eq_ignore_ascii_case("set") && args.len() > 2 && is_int(&args[1]) {
        fail_if!(lore.is_empty(), EssError::tl("itemloreNoLore", &[]));
        let line: i32 = args[1].parse().unwrap_or(0);
        let text = format_string(user, "essentials.itemlore", &get_final_arg(args, 2)).trim().to_owned();
        let place = usize::try_from(line - 1).ok().and_then(|index| lore.get_mut(index)).ok_or_else(|| EssError::tl("itemloreNoLine", crate::args![line]))?;
        place.clone_from(&text);
        item.set_lore(lore);
        keep(user, &item);
        user.send_tl("itemloreSuccessLore", crate::args![line, text]);
    } else {
        return Err(EssError::not_enough());
    }
    Ok(())
}

fn tab(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let lore = || user.get_item_in_hand().filter(|item| !item.get_type().is_air()).map(|item| item.get_lore()).unwrap_or_default();
    Some(match args.len() {
        1 => ["add", "set", "clear"].map(str::to_owned).to_vec(),
        2 if args[0].eq_ignore_ascii_case("set") => (1..=lore().len()).map(|line| line.to_string()).collect(),
        3 if args[0].eq_ignore_ascii_case("set") && is_int(&args[1]) => {
            let line: usize = args[1].parse().unwrap_or(0);
            line.checked_sub(1).and_then(|index| lore().get(index).cloned()).map(|text| unformat_string_for(user, "essentials.itemlore", &text)).into_iter().collect()
        }
        _ => Vec::new(),
    })
}
