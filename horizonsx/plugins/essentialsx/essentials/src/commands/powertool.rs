//! `Commandpowertool`.
use crate::prelude::*;

pub const COMMAND: Command = Command { run_user: Some(run_user), run_source: Some(run_source), tab_user: Some(tab_user), tab_source: Some(tab_source), ..Command::new("powertool") };

fn run_user(user: &User, _label: &str, args: &[String]) -> EssResult {
    powertool(&user.get_source(), user, user.get_item_in_hand().as_ref(), &get_final_arg(args, 0))
}

fn run_source(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.len() < 3, EssError::Message(format!("When running from console, usage is: /{label} <player> <itemid> <command>")));
    let user = get_player_raw(None, args, 0, true, true)?;
    powertool(sender, &user, Some(&item_db().get(&args[1])?), &get_final_arg(args, 2))
}

fn powertool(sender: &CommandSource, user: &User, stack: Option<&ItemStack>, command: &str) -> EssResult {
    if command.eq_ignore_ascii_case("d:") {
        user.clear_all_powertools();
        sender.send_tl("powerToolClearAll", &[]);
        return Ok(());
    }
    let stack = stack.filter(|stack| !stack.get_type().is_air()).ok_or_else(|| EssError::tl("powerToolAir", &[]))?;
    let item_name = stack.get_type().name().to_lowercase().replace('_', " ");
    let mut powertools = user.get_powertool(stack.get_type()).unwrap_or_default();
    if command.is_empty() {
        powertools.clear();
        sender.send_tl("powerToolRemoveAll", crate::args![item_name]);
    } else {
        if command.eq_ignore_ascii_case("l:") {
            fail_if!(powertools.is_empty(), EssError::tl("powerToolListEmpty", crate::args![item_name]));
            sender.send_tl("powerToolList", crate::args![join_list(&powertools), item_name]);
            return Err(EssError::NoCharge);
        }
        if let Some(removed) = command.strip_prefix("r:") {
            let Some(index) = powertools.iter().position(|tool| tool == removed) else {
                return Err(EssError::tl("powerToolNoSuchCommandAssigned", crate::args![removed, item_name]));
            };
            powertools.remove(index);
            sender.send_tl("powerToolRemove", crate::args![removed, item_name]);
        } else {
            let added = if let Some(added) = command.strip_prefix("a:") {
                fail_if!(sender.get_user().is_some_and(|by| !by.is_authorized("essentials.powertool.append")), EssError::tl("noPerm", crate::args!["essentials.powertool.append"]));
                fail_if!(powertools.iter().any(|tool| tool == added), EssError::tl("powerToolAlreadySet", crate::args![added, item_name]));
                added
            } else {
                powertools.clear();
                command
            };
            powertools.push(added.to_owned());
            sender.send_tl("powerToolAttach", crate::args![join_list(&powertools), item_name]);
        }
    }
    if !user.are_power_tools_enabled() {
        user.set_power_tools_enabled(true);
        user.send_tl("powerToolsEnabled", &[]);
    }
    user.set_powertool(stack, Some(powertools));
    Ok(())
}

fn append_options(options: &mut Vec<String>) {
    options.extend(get_commands().into_iter().map(|command| format!("a:{command}")));
}

fn tab_user(user: &User, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() == 1 {
        let mut options = ["d:", "c:", "l:"].map(str::to_owned).to_vec();
        if user.is_authorized("essentials.powertool.append") {
            append_options(&mut options);
        }
        if let Some(stack) = user.get_item_in_hand() {
            options.extend(user.get_powertool(stack.get_type()).unwrap_or_default().into_iter().map(|tool| format!("r:{tool}")));
        }
        return Some(options);
    }
    Some(args[0].strip_prefix("a:").map_or_else(Vec::new, |command| tab_complete_command(&user.get_source(), command, args, 1)))
}

fn tab_source(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(match args.len() {
        1 => get_players(sender),
        2 => get_items(args),
        3 => {
            let mut options = ["d:", "c:", "l:"].map(str::to_owned).to_vec();
            append_options(&mut options);
            if let (Ok(user), Ok(stack)) = (get_player_raw(None, args, 0, true, true), item_db().get(&args[1])) {
                options.extend(user.get_powertool(stack.get_type()).unwrap_or_default().into_iter().map(|tool| format!("r:{tool}")));
            }
            options
        }
        _ => args[2].strip_prefix("a:").map_or_else(Vec::new, |command| tab_complete_command(sender, command, args, 3)),
    })
}
