//! `Commandsell`. `UserBalanceUpdateEvent` isn't fired: nothing here can listen.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run), tab_user: Some(tab), ..Command::new("sell") };

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    if first.eq_ignore_ascii_case("hand") && !user.is_authorized("essentials.sell.hand") {
        return Err(EssError::tl("sellHandPermission", &[]));
    } else if (first.eq_ignore_ascii_case("inventory") || first.eq_ignore_ascii_case("invent") || first.eq_ignore_ascii_case("all")) && !user.is_authorized("essentials.sell.bulk") {
        return Err(EssError::tl("sellBulkPermission", &[]));
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut stacks = item_db().get_matching(user, &args)?;
    let mut total = BigDecimal::zero();
    let mut count = 0;
    let is_bulk = stacks.len() > 1;
    let mut not_sold = Vec::new();
    for index in 0..stacks.len() {
        let stack = stacks[index].clone();
        if !settings().is_allow_sell_named_items() && stack.has_display_name() {
            if is_bulk {
                not_sold.push(stack);
                continue;
            }
            return Err(EssError::tl("cannotSellNamedItem", &[]));
        }
        if stack.get_amount() <= 0 {
            continue;
        }
        match sell_item(user, &stack, &args, is_bulk) {
            Ok(worth) => {
                total = total.add(&worth);
                count += 1;
                for zero in stacks.iter_mut().filter(|zero| zero.is_similar(&stack)) {
                    zero.set_amount(0);
                }
            }
            Err(error) if !is_bulk => return Err(error),
            Err(_) => {}
        }
    }
    if !not_sold.is_empty() {
        let names: Vec<String> = not_sold.iter().filter_map(ItemStack::get_display_name).collect();
        show_error(&user.get_source(), &EssError::tl("cannotSellTheseNamedItems", crate::args![names.join("\u{a7}r, ")]), label);
    }
    if count != 1 {
        let shown = display_currency(&total);
        let key = if first_is_blocks(&args) { "totalWorthBlocks" } else { "totalWorthAll" };
        user.send_tl(key, crate::args![parsed(shown.clone()), parsed(shown)]);
    }
    Ok(())
}

fn first_is_blocks(args: &[&str]) -> bool {
    args[0].eq_ignore_ascii_case("blocks")
}

fn sell_item(user: &User, stack: &ItemStack, args: &[&str], is_bulk: bool) -> EssResult<BigDecimal> {
    let amount = ess().worth().get_amount(user, Some(stack), args, is_bulk)?;
    let worth = ess().worth().get_price(stack).map(|price| price.multiply(&settings().get_multiplier(user))).ok_or_else(|| EssError::tl("itemCannotBeSold", &[]))?;
    let type_name = stack.get_type().name().to_lowercase();
    if amount <= 0 {
        if !is_bulk {
            user.send_tl("itemSold", crate::args![parsed(display_currency(&BigDecimal::zero())), BigDecimal::zero(), type_name, display_currency(&worth)]);
        }
        return Ok(BigDecimal::zero());
    }
    let result = worth.multiply(&BigDecimal::from_i64(i64::from(amount)));
    let mut sold = stack.clone();
    sold.set_amount(amount);
    let Some(player) = user.player() else { return Ok(BigDecimal::zero()) };
    fail_if!(!inventories::contains_at_least(&player, &sold, amount), EssError::Message("Trying to remove more items than are available.".to_owned()));
    inventories::remove_item_amount(&player, &sold, amount);
    Trade::log("Command", "Sell", "Item", Some(&user.get_name()), Some(&Trade::of_item(sold.clone())), Some(&user.get_name()), Some(&Trade::of_money(result.clone())), user.get_location().as_ref(), Some(&user.get_money()));
    user.give_money(&result, None)?;
    let (result_shown, worth_shown) = (display_currency(&result), display_currency(&worth));
    user.send_tl("itemSold", crate::args![parsed(result_shown.clone()), amount, type_name.clone(), parsed(worth_shown.clone())]);
    crate::info(&mini_to_legacy(&tl_literal("itemSoldConsole", crate::args![user.get_name(), type_name, mini_to_legacy(&result_shown), amount, mini_to_legacy(&worth_shown), user.get_display_name()])));
    Ok(result)
}

fn tab(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_matching_items(args),
        2 => vec!["1".to_owned(), "64".to_owned()],
        _ => Vec::new(),
    })
}
