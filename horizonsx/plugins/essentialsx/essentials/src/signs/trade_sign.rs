//! `SignTrade`.
use crate::prelude::*;
use super::{EssentialsSign, ISign, set_owner, get_big_decimal, get_big_decimal_positive, get_integer, get_integer_positive, get_item_stack, get_money, is_owner, min_transaction, split_line};
use crate::trade::{OverflowType, TradeType};
use crate::utils::material::is_potion;
use crate::utils::number::{format_as_currency, short_currency};

const MAX_STOCK_LINE_LENGTH: usize = 15;

#[derive(Clone, Copy, PartialEq)]
enum AmountType {
    Total,
    Rounded,
    Cost,
}

pub struct SignTrade;

fn at(sign: &dyn ISign) -> horizon_bukkit::Location {
    sign.get_block().get_location()
}

/// `sign.getLine(3).substring(2)`: the owner's name without its color.
fn owner_name(line: &str) -> String {
    line.chars().skip(2).collect()
}

fn validate_sign_length(line: &str) -> EssResult {
    fail_if!(line.encode_utf16().count() > MAX_STOCK_LINE_LENGTH, EssError::tl("tradeSignFull", &[]));
    Ok(())
}

fn is_exp(word: &str) -> bool {
    word.eq_ignore_ascii_case("exp") || word.eq_ignore_ascii_case("xp")
}

fn invalid_line(index: usize) -> EssError {
    EssError::tl("invalidSignLine", crate::args![index + 1])
}

fn empty_line(index: usize) -> EssError {
    EssError::tl("emptySignLine", crate::args![index + 1])
}

fn validate_trade(sign: &mut dyn ISign, index: usize, amount_needed: bool) -> EssResult {
    let line = sign.get_line(index).trim().to_owned();
    fail_if!(line.is_empty(), empty_line(index));
    let split = split_line(&line);
    if split.len() == 1
        && !amount_needed
        && let Some(money) = get_money(&split[0])?
    {
        let new_line = format!("{}:0", short_currency(&money));
        validate_sign_length(&new_line)?;
        sign.set_line(index, &new_line);
        return Ok(());
    }
    if split.len() == 2 && amount_needed {
        let money = get_money(&split[0])?;
        let amount = get_big_decimal_positive(&split[1])?;
        if let Some(money) = money {
            let amount = amount.subtract(&amount.remainder(&money));
            fail_if!(amount < min_transaction() || money < min_transaction(), EssError::tl("moreThanZero", &[]));
            let new_line = format!("{}:{}", short_currency(&money), format_as_currency(&amount));
            validate_sign_length(&new_line)?;
            sign.set_line(index, &new_line);
            return Ok(());
        }
    }
    if split.len() == 2 && !amount_needed {
        let amount = get_integer_positive(&split[0])?;
        fail_if!(!is_exp(&split[1]) && get_item_stack(&split[1], amount)?.get_type().is_air(), EssError::tl("moreThanZero", &[]));
        let new_line = format!("{amount} {}:0", split[1]);
        validate_sign_length(&new_line)?;
        sign.set_line(index, &new_line);
        return Ok(());
    }
    if split.len() == 3 && amount_needed {
        let stack_amount = get_integer_positive(&split[0])?;
        let mut amount = get_integer_positive(&split[2])?;
        amount -= amount % stack_amount;
        fail_if!(amount < 1 || stack_amount < 1, EssError::tl("moreThanZero", &[]));
        fail_if!(!is_exp(&split[1]) && get_item_stack(&split[1], stack_amount)?.get_type().is_air(), EssError::tl("moreThanZero", &[]));
        let new_line = format!("{stack_amount} {}:{amount}", split[1]);
        validate_sign_length(&new_line)?;
        sign.set_line(index, &new_line);
        return Ok(());
    }
    Err(invalid_line(index))
}

fn get_trade(sign: &dyn ISign, index: usize, kind: AmountType, not_empty: bool) -> EssResult<Trade> {
    let line = sign.get_line(index).trim().to_owned();
    fail_if!(line.is_empty(), empty_line(index));
    let split = split_line(&line);
    if split.len() == 2 {
        let empty = |_| EssError::tl("tradeSignEmpty", &[]);
        let money = get_money(&split[0]).map_err(empty)?;
        let amount = if not_empty { get_big_decimal_positive(&split[1]) } else { get_big_decimal(&split[1]) }.map_err(empty)?;
        if let Some(money) = money {
            return Ok(Trade::of_money(if kind == AmountType::Cost { money } else { amount }));
        }
    }
    if split.len() == 3 {
        let stack_amount = get_integer_positive(&split[0])?;
        let mut amount = get_integer(&split[2])?;
        if kind == AmountType::Rounded {
            amount -= amount % stack_amount;
        }
        if is_exp(&split[1]) {
            fail_if!(not_empty && (amount < 1 || stack_amount < 1), EssError::tl("tradeSignEmpty", &[]));
            return Ok(Trade::of_exp(if kind == AmountType::Cost { stack_amount } else { amount }));
        }
        let mut item = get_item_stack(&split[1], stack_amount)?;
        fail_if!(not_empty && (amount < 1 || stack_amount < 1 || item.get_type().is_air() || amount < stack_amount), EssError::tl("tradeSignEmpty", &[]));
        item.set_amount(if kind == AmountType::Cost { stack_amount } else { amount });
        return Ok(Trade::of_item(item));
    }
    Err(invalid_line(index))
}

fn set_amount(sign: &mut dyn ISign, index: usize, value: &BigDecimal, validation: bool) -> EssResult {
    let line = sign.get_line(index).trim().to_owned();
    fail_if!(line.is_empty(), empty_line(index));
    let split = split_line(&line);
    if split.len() == 2 {
        let money = get_money(&split[0])?;
        get_big_decimal(&split[1])?;
        if let Some(money) = money {
            let new_line = format!("{}:{}", short_currency(&money), format_as_currency(value));
            validate_sign_length(&new_line)?;
            if !validation {
                sign.set_line(index, &new_line);
            }
            return Ok(());
        }
    }
    if split.len() == 3 {
        let stack_amount = get_integer_positive(&split[0])?;
        if !is_exp(&split[1]) {
            get_item_stack(&split[1], stack_amount)?;
        }
        let new_line = format!("{stack_amount} {}:{}", split[1], value.to_i64());
        validate_sign_length(&new_line)?;
        if !validation {
            sign.set_line(index, &new_line);
        }
        return Ok(());
    }
    Err(invalid_line(index))
}

fn change_amount(sign: &mut dyn ISign, index: usize, value: &BigDecimal, validation: bool) -> EssResult {
    let line = sign.get_line(index).trim().to_owned();
    fail_if!(line.is_empty(), empty_line(index));
    let split = split_line(&line);
    let stored = match split.len() {
        2 => &split[1],
        3 => &split[2],
        _ => return Err(invalid_line(index)),
    };
    let amount = get_big_decimal(stored)?.add(value);
    set_amount(sign, index, &amount, validation)
}

/// What a trade holds, whichever of the three it is.
fn value_of(trade: &Trade) -> BigDecimal {
    if let Some(money) = trade.get_money() {
        return money.clone();
    }
    let count = trade.get_item_stack().map(|item| i64::from(item.get_amount())).or(trade.get_experience().map(i64::from));
    BigDecimal::from_i64(count.unwrap_or(0))
}

fn add_amount(sign: &mut dyn ISign, index: usize, trade: &Trade, validation: bool) -> EssResult {
    change_amount(sign, index, &value_of(trade), validation)
}

fn subtract_amount(sign: &mut dyn ISign, index: usize, trade: &Trade, validation: bool) -> EssResult {
    change_amount(sign, index, &value_of(trade).negate(), validation)
}

fn recharge_sign(sign: &mut dyn ISign, user: &User) -> EssResult<Option<Trade>> {
    let trade = get_trade(sign, 2, AmountType::Cost, false)?;
    let (Some(wanted), Some(held), Some(player)) = (trade.get_item_stack(), user.get_item_in_hand(), user.player()) else {
        return Ok(None);
    };
    if held.get_type().is_air() || wanted.get_type() != held.get_type() || wanted.get_damage() != held.get_damage() || wanted.get_enchants() != held.get_enchants() {
        return Ok(None);
    }
    if is_potion(wanted.get_type()) && !wanted.is_similar(&held) {
        return Ok(None);
    }
    let amount = wanted.get_amount();
    if !inventories::contains_at_least(&player, wanted, amount) {
        return Ok(None);
    }
    let mut stack = held;
    stack.set_amount(amount);
    let store = Trade::of_item(stack);
    add_amount(sign, 2, &store, false)?;
    store.charge(user)?;
    Ok(Some(store))
}

impl EssentialsSign for SignTrade {
    fn name(&self) -> &'static str {
        "Trade"
    }
    fn on_create(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        validate_trade(sign, 1, false)?;
        validate_trade(sign, 2, true)?;
        let trade = get_trade(sign, 2, AmountType::Rounded, true)?;
        let charge = get_trade(sign, 1, AmountType::Rounded, false)?;
        fail_if!(trade.get_type() == charge.get_type() && (trade.get_type() != TradeType::Item || trade.get_item_stack().zip(charge.get_item_stack()).is_some_and(|(one, other)| one.is_similar(other))), EssError::tl("tradeSignSameType", &[]));
        trade.is_affordable_for(user)?;
        set_owner(user, sign, 3, "\u{a7}8");
        trade.charge(user)?;
        Trade::log("Sign", "Trade", "Create", Some(username), Some(&trade), Some(username), None, Some(&at(sign)), Some(&user.get_money()));
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let place = at(sign);
        if is_owner(user, sign, 3, "\u{a7}8") {
            let store = recharge_sign(sign, user)?;
            let withdrawn = (|| -> EssResult {
                let stored = get_trade(sign, 1, AmountType::Total, true)?;
                subtract_amount(sign, 1, &stored, false)?;
                match stored.pay_with(user, OverflowType::Return)? {
                    None => Trade::log("Sign", "Trade", "Withdraw", Some(username), store.as_ref(), Some(username), None, Some(&place), Some(&user.get_money())),
                    Some(left) => {
                        if let Some(rest) = left.get(&0) {
                            set_amount(sign, 1, &BigDecimal::from_i64(i64::from(rest.get_amount())), false)?;
                            Trade::log("Sign", "Trade", "Withdraw", Some(username), Some(&stored), Some(username), Some(&Trade::of_item(rest.clone())), Some(&place), Some(&user.get_money()));
                        }
                    }
                }
                Ok(())
            })();
            fail_if!(withdrawn.is_err() && store.is_none(), EssError::tl("tradeSignEmptyOwner", &[]));
            Trade::log("Sign", "Trade", "Deposit", Some(username), store.as_ref(), Some(username), None, Some(&place), Some(&user.get_money()));
        } else {
            let charge = get_trade(sign, 1, AmountType::Cost, false)?;
            let trade = get_trade(sign, 2, AmountType::Cost, true)?;
            charge.is_affordable_for(user)?;
            add_amount(sign, 1, &charge, true)?;
            subtract_amount(sign, 2, &trade, true)?;
            add_amount(sign, 1, &charge, false)?;
            subtract_amount(sign, 2, &trade, false)?;
            if !trade.pay(user)? {
                subtract_amount(sign, 1, &charge, false)?;
                add_amount(sign, 2, &trade, false)?;
                return Err(EssError::tl("inventoryFull", &[]));
            }
            charge.charge(user)?;
            let owner = owner_name(&sign.get_line(3));
            Trade::log("Sign", "Trade", "Interact", Some(&owner), Some(&charge), Some(username), Some(&trade), Some(&place), Some(&user.get_money()));
        }
        sign.update_sign();
        Ok(true)
    }
    fn on_break(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let owner_line = sign.get_line(3);
        let owner = owner_name(&owner_line);
        let place = at(sign);
        let mine = is_owner(user, sign, 3, "\u{a7}8");
        let can_break = mine || user.is_authorized("essentials.signs.trade.override");
        let can_collect = mine || user.is_authorized("essentials.signs.trade.override.collect");
        if !can_break {
            return Ok(false);
        }
        let collected = (|| -> EssResult<bool> {
            let stored_1 = get_trade(sign, 1, AmountType::Total, false)?;
            let stored_2 = get_trade(sign, 2, AmountType::Total, false)?;
            if !can_collect {
                Trade::log("Sign", "Trade", "Destroy", Some(&owner), Some(&stored_2), Some(username), Some(&stored_1), Some(&place), Some(&user.get_money()));
                return Ok(true);
            }
            let left_1 = stored_1.pay_with(user, OverflowType::Drop)?;
            let left_2 = stored_2.pay_with(user, OverflowType::Drop)?;
            if left_1.is_none() && left_2.is_none() {
                Trade::log("Sign", "Trade", "Break", Some(&owner), Some(&stored_2), Some(username), Some(&stored_1), Some(&place), Some(&user.get_money()));
                return Ok(true);
            }
            for (index, stored, left) in [(1, &stored_1, &left_1), (2, &stored_2, &left_2)] {
                let rest = left.as_ref().and_then(|left| left.get(&0));
                set_amount(sign, index, &BigDecimal::from_i64(rest.map_or(0, |item| i64::from(item.get_amount()))), false)?;
                let back = rest.map(|item| Trade::of_item(item.clone()));
                Trade::log("Sign", "Trade", "Withdraw", Some(&owner), Some(stored), Some(username), back.as_ref(), Some(&place), Some(&user.get_money()));
            }
            sign.update_sign();
            Ok(false)
        })();
        match collected {
            Err(_) if user.is_authorized("essentials.signs.trade.override") => Ok(true),
            other => other,
        }
    }
}
