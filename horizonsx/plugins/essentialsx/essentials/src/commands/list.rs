//! `Commandlist`.
use crate::prelude::*;
use crate::player_list;

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("list") };

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    let user = sender.get_user();
    let show_hidden = user.as_ref().is_none_or(|user| user.is_authorized("essentials.list.hidden") || user.can_interact_vanished());
    sender.send_component(&deserialize_mini_message(&player_list::list_summary(user.as_ref(), show_hidden)));
    let mut lists = player_list::get_player_lists(user.as_ref(), show_hidden);
    if let Some(group) = args.first() {
        sender.send_component(&deserialize_mini_message(&player_list::list_group_users(&mut lists, &group.to_lowercase())?));
    } else {
        for line in player_list::prepare_grouped_list(Some(sender), label, &mut lists) {
            sender.send_component(&deserialize_mini_message(&line));
        }
    }
    Ok(())
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 { player_list::get_player_lists(sender.get_user().as_ref(), false).keys().cloned().collect() } else { Vec::new() })
}
