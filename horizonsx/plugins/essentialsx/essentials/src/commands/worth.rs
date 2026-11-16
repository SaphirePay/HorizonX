//! `Commandworth`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("worth") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut stacks = item_db().get_matching(user, &words)?;
    let mut count = 0;
    let is_bulk = stacks.len() > 1;
    let mut total = BigDecimal::zero();
    for index in 0..stacks.len() {
        let stack = stacks[index].clone();
        if stack.get_amount() <= 0 {
            continue;
        }
        match item_worth(&user.get_source(), Some(user), &stack, &words) {
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
    if count > 1 {
        let shown = display_currency(&total);
        let key = if words.first().is_some_and(|first| first.eq_ignore_ascii_case("blocks")) { "totalSellableBlocks" } else { "totalSellableAll" };
        user.send_tl(key, crate::args![parsed(shown.clone()), parsed(shown)]);
    }
    Ok(())
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    item_worth(sender, None, &item_db().get(&args[0])?, &words)?;
    Ok(())
}

fn item_worth(sender: &CommandSource, user: Option<&User>, stack: &ItemStack, args: &[&str]) -> EssResult<BigDecimal> {
    let mut amount = 1;
    match user {
        None => {
            if let Some(text) = args.get(1) {
                let digits: String = text.chars().filter(char::is_ascii_digit).collect();
                amount = digits.parse().map_err(|_| EssError::not_enough())?;
            }
        }
        Some(user) => amount = ess().worth().get_amount(user, Some(stack), args, true)?,
    }
    let worth = ess().worth().get_price(stack).ok_or_else(|| EssError::tl("itemCannotBeSold", &[]))?;
    let amount = amount.max(0);
    let result = worth.multiply(&BigDecimal::from_i64(i64::from(amount)));
    let type_name = stack.get_type().name().to_lowercase().replace('_', "");
    let (result_shown, worth_shown) = (parsed(display_currency(&result)), parsed(display_currency(&worth)));
    if stack.get_damage() != 0 {
        sender.send_tl("worthMeta", crate::args![type_name, stack.get_damage(), result_shown, amount, worth_shown]);
    } else {
        sender.send_tl("worth", crate::args![type_name, result_shown, amount, worth_shown]);
    }
    Ok(result)
}

fn tab_user(_user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_matching_items(args),
        2 => vec!["1".to_owned(), "64".to_owned()],
        _ => Vec::new(),
    })
}

fn tab_source(_sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_items(args),
        2 => vec!["1".to_owned(), "64".to_owned()],
        _ => Vec::new(),
    })
}
