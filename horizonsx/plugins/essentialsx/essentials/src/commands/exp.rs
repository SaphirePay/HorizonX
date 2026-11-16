//! `Commandexp`.
use crate::prelude::*;
use crate::set_exp_fix::{get_exp_to_level, get_exp_until_next_level, get_total_experience, set_total_experience};

pub const COMMAND: Command = Command { run_source: Some(run), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("exp") };

fn has_permission(user: Option<&User>, word: &str) -> bool {
    word == "show" || user.is_none_or(|user| user.is_authorized(&format!("essentials.exp.{word}")))
}

fn has_other_permission(user: Option<&User>, word: &str) -> bool {
    user.is_none_or(|user| user.is_authorized(&format!("essentials.exp.{word}.others")))
}

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    let user = sender.get_user();
    if args.is_empty() || (args.len() < 2 && user.is_none()) {
        let Some(user) = &user else {
            return Err(EssError::not_enough());
        };
        show_exp(sender, user);
        return Ok(());
    }
    let word = args[0].to_lowercase();
    fail_if!(!matches!(word.as_str(), "set" | "give" | "take" | "reset" | "show"), EssError::not_enough());
    if !has_permission(user.as_ref(), &word) {
        if let Some(user) = &user {
            user.send_tl("noAccessSubCommand", crate::args![format!("/{label} {word}")]);
        }
        return Ok(());
    }
    let others = has_other_permission(user.as_ref(), &word);
    let on_others = |amount: String, give: bool| loop_online_players(sender, false, true, &args[1], |player| set_exp(sender, player, &amount, give));
    let self_user = user.as_ref();
    match word.as_str() {
        "set" | "give" => {
            let give = word == "give";
            if args.len() == 3 && others {
                on_others(args[2].clone(), give)
            } else if let (2, Some(user)) = (args.len(), self_user) {
                set_exp(sender, user, &args[1], give)
            } else {
                Err(EssError::not_enough())
            }
        }
        "take" => {
            if args.len() == 3 && others {
                on_others(format!("-{}", args[2]), true)
            } else if let (2, Some(user)) = (args.len(), self_user) {
                set_exp(sender, user, &format!("-{}", args[1]), true)
            } else {
                Err(EssError::not_enough())
            }
        }
        "reset" => {
            if args.len() == 2 && others {
                on_others("0".to_owned(), false)
            } else if let Some(user) = self_user {
                set_exp(sender, user, "0", false)
            } else {
                Err(EssError::not_enough())
            }
        }
        _ => {
            if args.len() == 2 && self_user.is_none_or(|user| user.is_authorized("essentials.exp.others")) {
                show_exp(sender, &get_player_by_term(sender, &args[1])?);
                Ok(())
            } else if let Some(user) = self_user {
                show_exp(sender, user);
                Ok(())
            } else {
                Err(EssError::not_enough())
            }
        }
    }
}

fn show_exp(sender: &CommandSource, target: &User) {
    if let Some(base) = target.player() {
        sender.send_tl("exp", crate::args![target.get_display_name(), get_total_experience(&base), base.get_level(), get_exp_until_next_level(&base)]);
    }
}

fn set_exp(sender: &CommandSource, target: &User, amount: &str, give: bool) -> EssResult {
    let Some(base) = target.player() else { return Ok(()) };
    let text = amount.to_lowercase();
    let mut amount: i64;
    if text.contains('l') {
        let mut needed = parse_int(&text.replace('l', ""))?;
        if give {
            needed = needed.wrapping_add(base.get_level());
        }
        amount = i64::from(get_exp_to_level(needed));
        set_total_experience(&base, 0);
    } else {
        amount = text.parse().map_err(|_| EssError::Message(format!("For input string: \"{text}\"")))?;
        fail_if!(amount > i64::from(i32::MAX) || amount < i64::from(i32::MIN), EssError::not_enough());
    }
    if give {
        amount += i64::from(get_total_experience(&base));
    }
    let amount = amount.clamp(0, i64::from(i32::MAX));
    set_total_experience(&base, amount as i32);
    sender.send_tl("expSet", crate::args![target.get_display_name(), amount]);
    Ok(())
}

fn levels(text: &str) -> Option<String> {
    let without = text.to_lowercase().replace('l', "");
    is_int(&without).then(|| format!("{without}l"))
}

fn tab_user(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let word = args[0].to_lowercase();
    Some(match args.len() {
        1 => {
            let mut options = vec!["show".to_owned()];
            options.extend(["set", "give", "take", "reset", "show"].iter().filter(|word| has_permission(Some(user), word)).map(|word| (*word).to_owned()));
            options
        }
        2 => {
            if matches!(word.as_str(), "set" | "give" | "take")
                && user.is_authorized(&format!("essentials.exp.{word}"))
                && let Some(option) = levels(&args[1])
            {
                return Some(vec![option]);
            }
            if user.is_authorized("essentials.exp.others") { get_players(&user.get_source()) } else { Vec::new() }
        }
        3 if word != "show" && word != "reset" => levels(&args[2]).into_iter().collect(),
        _ => Vec::new(),
    })
}

fn tab_source(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    let word = args[0].to_lowercase();
    let adds = word == "set" || word == "give";
    Some(match args.len() {
        1 => ["set", "give", "take", "reset", "show"].map(str::to_owned).to_vec(),
        2 if adds => {
            let without = args[1].to_lowercase().replace('l', "");
            if is_int(&without) { vec![without, format!("{}l", args[1])] } else { Vec::new() }
        }
        2 => get_players(sender),
        3 if adds => get_players(sender),
        _ => Vec::new(),
    })
}
