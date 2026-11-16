//! `Commandkit`.
use crate::prelude::*;
use crate::kits::Kit;
use crate::utils::string::sanitize_string;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("kit") };

fn kit_list(sender: &CommandSource, user: Option<&User>) -> EssResult {
    let list = ess().kits().list_kits(user)?;
    sender.send_tl(if list.is_empty() { "noKits" } else { "kits" }, crate::args![parsed(list)]);
    Err(EssError::NoCharge)
}

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    let Some(first) = args.first() else {
        return kit_list(&user.get_source(), Some(user));
    };
    let names = sanitize_string(&first.to_lowercase()).trim().to_owned();
    if args.len() > 1 && user.is_authorized("essentials.kit.others") {
        give_kits(&get_player_for(user, args, 1)?, user, &names)
    } else {
        give_kits(user, user, &names)
    }
}

fn run_source(sender: &CommandSource, _label: &str, args: &[String]) -> EssResult {
    if args.len() < 2 {
        return kit_list(sender, None);
    }
    let user_to = get_player_raw(None, args, 1, true, false)?;
    for name in args[0].to_lowercase().split(',') {
        Kit::new(name)?.expand_items(&user_to)?;
        sender.send_tl("kitGiveTo", crate::args![name, user_to.get_display_name()]);
        user_to.send_tl("kitReceive", crate::args![name]);
    }
    Ok(())
}

fn give_kits(user_to: &User, user_from: &User, names: &str) -> EssResult {
    fail_if!(names.is_empty(), EssError::tl("kitNotFound", &[]));
    let mut kits = Vec::new();
    for name in crate::utils::string::java_split(names, ',') {
        fail_if!(name.is_empty(), EssError::tl("kitNotFound", &[]));
        let kit = Kit::new(&name)?;
        kit.check_perms(user_from)?;
        kit.check_delay(user_from)?;
        kit.check_affordable(user_from)?;
        kits.push(kit);
    }
    for kit in kits {
        let result = (|| -> EssResult {
            kit.check_delay(user_from)?;
            kit.check_affordable(user_from)?;
            if !kit.expand_items(user_to)? {
                return Ok(());
            }
            kit.set_time(user_from);
            kit.charge_user(user_to)?;
            if user_from != user_to {
                user_from.send_tl("kitGiveTo", crate::args![kit.get_name(), user_to.get_display_name()]);
            }
            user_to.send_tl("kitReceive", crate::args![kit.get_name()]);
            Ok(())
        })();
        match result {
            Err(EssError::NoCharge) => {
                if settings().is_debug() {
                    crate::info(&format!("Soft kit error, abort spawning {}", kit.get_name()));
                }
            }
            Err(error) => ess().show_error(&user_from.get_source(), &error, &format!("\\ kit: {}", kit.get_name())),
            Ok(()) => {}
        }
    }
    Ok(())
}

fn tab_user(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => ess().kits().get_kit_keys().into_iter().filter(|kit| user.is_authorized(&format!("essentials.kits.{kit}"))).collect(),
        2 if user.is_authorized("essentials.kit.others") => get_players(&user.get_source()),
        _ => Vec::new(),
    })
}

fn tab_source(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => ess().kits().get_kit_keys(),
        2 => get_players(sender),
        _ => Vec::new(),
    })
}
