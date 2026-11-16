//! `Commandpay`.
use crate::prelude::*;
use crate::utils::number::display_currency_exactly;

pub const COMMAND: Command = Command { run_user: Some(run), tab_source: Some(tab), ..Command::new("pay") };

fn minimum() -> EssResult<crate::big_decimal::BigDecimal> {
    settings().get_minimum_pay_amount().ok_or_else(|| EssError::Message("NumberFormatException".to_owned()))
}

fn run(user: &User, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let og = &args[1];
    fail_if!(og.contains('-'), EssError::tl("payMustBePositive", &[]));
    fail_if!(!og.contains(|c: char| c.is_ascii_digit() || c == '.'), EssError::not_enough());
    let amount = user.parse_amount(og)?;
    let minimum = minimum()?;
    fail_if!(amount < minimum, EssError::tl("minimumPayAmount", crate::args![parsed(display_currency_exactly(&minimum))]));
    let mut inform_to_confirm = false;
    let can_pay_offline = user.is_authorized("essentials.pay.offline");
    if !can_pay_offline && args[0] == "**" {
        user.send_tl("payOffline", &[]);
        return Ok(());
    }
    loop_offline_players(&user.get_source(), false, user.is_authorized("essentials.pay.multiple"), &args[0], |player| {
        let hidden = user.player().is_some_and(|base| player.is_hidden_from(&base));
        if player.player().is_some() && hidden && !can_pay_offline || !player.is_online() && player.player().is_some() && !can_pay_offline {
            user.send_tl("payOffline", &[]);
            return Ok(());
        }
        if !player.is_accepting_pay() || (settings().is_pay_excludes_ignore_list() && player.is_ignored_player(user)) {
            user.send_tl("notAcceptingPay", crate::args![player.get_display_name()]);
            return Ok(());
        }
        if user.is_prompting_pay_confirm() && user.get_confirming_payment(player).is_none_or(|confirmed| !amount.equals_exact(&confirmed)) {
            if !inform_to_confirm {
                user.clear_confirming_payments();
                inform_to_confirm = true;
            }
            user.set_confirming_payment(player, Some(amount.clone()));
            return Ok(());
        }
        match user.pay_user(player, &amount) {
            Err(EssError::MaxMoney) => {
                user.send_tl("maxMoney", &[]);
                let _ = user.set_money(user.get_money().add(&amount));
            }
            Err(error) => return Err(error),
            Ok(()) => {
                user.set_confirming_payment(player, None);
                let trade = Trade::of_money(amount.clone());
                Trade::log("Command", "Pay", "Player", Some(&user.get_name()), Some(&trade), Some(&player.get_name()), Some(&trade), user.get_location().as_ref(), Some(&user.get_money()));
            }
        }
        Ok(())
    })?;
    if inform_to_confirm {
        let command = format!("/{label} {}", args.join(" "));
        user.send_tl("confirmPayment", crate::args![parsed(display_currency(&amount)), command]);
    }
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_players(sender),
        2 => minimum().map(|minimum| vec![minimum.to_plain_string()]).unwrap_or_default(),
        _ => Vec::new(),
    })
}
