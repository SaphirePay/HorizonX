//! `Commandeco`. `UserBalanceUpdateEvent` isn't fired: nothing here can listen.
use crate::prelude::*;
use crate::utils::number::parse_string_to_big_decimal;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("eco") };

#[derive(Clone, Copy, PartialEq)]
enum Eco {
    Give,
    Take,
    Set,
    Reset,
}

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 2, EssError::not_enough());
    let cmd = match args[0].to_uppercase().as_str() {
        "GIVE" => Eco::Give,
        "TAKE" => Eco::Take,
        "SET" => Eco::Set,
        "RESET" => Eco::Reset,
        _ => return Err(EssError::not_enough()),
    };
    fail_if!(cmd != Eco::Reset && args.len() < 3, EssError::not_enough());
    let is_percent = cmd != Eco::Reset && args[2].ends_with('%');
    let amount = if cmd == Eco::Reset {
        settings().get_starting_balance()
    } else {
        let parsed = match sender.get_user() {
            Some(user) => user.parse_amount(&args[2]),
            None => parse_string_to_big_decimal(&args[2]),
        };
        parsed.map_err(|_| EssError::not_enough())?
    };
    loop_offline_players(sender, false, true, &args[1], |player| {
        let mut user_amount = amount.clone();
        if is_percent {
            user_amount = player.get_money().multiply(&user_amount).scale_by_power_of_ten(-2);
        }
        match cmd {
            Eco::Give => player.give_money(&user_amount, Some(sender))?,
            Eco::Take => {
                let min = settings().get_min_money();
                if player.get_money().subtract(&user_amount) >= min {
                    player.take_money(&user_amount, Some(sender));
                } else {
                    ess().show_error(sender, &EssError::tl("minimumBalanceError", crate::args![parsed(display_currency(&min))]), label);
                }
            }
            Eco::Reset | Eco::Set => {
                let (min, max) = (settings().get_min_money(), settings().get_max_money());
                let clamped = if user_amount < min {
                    min
                } else if user_amount > max {
                    max
                } else {
                    user_amount
                };
                player.set_money(clamped)?;
                player.send_tl("setBal", crate::args![parsed(display_currency(&player.get_money()))]);
                sender.send_tl("setBalOthers", crate::args![player.get_display_name(), parsed(display_currency(&player.get_money()))]);
            }
        }
        Ok(())
    })
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => ["give", "take", "set", "reset"].map(str::to_owned).to_vec(),
        2 => get_players(sender),
        3 if !args[0].eq_ignore_ascii_case("reset") => {
            if args[0].eq_ignore_ascii_case("set") {
                vec!["0".to_owned(), settings().get_starting_balance().to_plain_string()]
            } else {
                ["1", "10", "100", "1000"].map(str::to_owned).to_vec()
            }
        }
        _ => Vec::new(),
    })
}
