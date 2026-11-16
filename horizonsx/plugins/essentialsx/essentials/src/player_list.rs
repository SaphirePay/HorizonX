//! `PlayerList`: `/list`'s grouping and output.
use crate::prelude::*;
use crate::adventure::legacy_to_mini;
use crate::utils::format::strip_essentials_format;
use std::collections::BTreeMap;

pub type GroupedUsers = BTreeMap<String, Vec<User>>;

/// `listUsers`.
pub fn list_users(mut users: Vec<User>, separator: &str) -> String {
    users.sort_by(User::compare_to);
    let mut text = String::new();
    for (index, user) in users.iter().enumerate() {
        if index > 0 {
            text.push_str(separator);
        }
        if user.is_afk() {
            text.push_str(&tl_literal("listAfkTag", &[]));
        }
        if user.is_hidden() {
            text.push_str(&tl_literal("listHiddenTag", &[]));
        }
        user.set_display_nick();
        text.push_str(&legacy_to_mini(&user.get_display_name()));
        let stripped = user.get_nickname().map(|nick| strip_format(&nick));
        if settings().real_names_on_list() && stripped.is_some_and(|nick| nick != user.get_name()) {
            text.push(' ');
            text.push_str(&tl_literal("listRealName", crate::args![user.get_name()]));
        }
        text.push_str("<white>");
    }
    text
}

/// `listSummary`.
pub fn list_summary(user: Option<&User>, show_hidden: bool) -> String {
    let mut player_hidden = 0;
    let mut hidden_count = 0;
    let online = ess().get_online_users();
    let viewer = user.and_then(User::player);
    for player in &online {
        let from_viewer = viewer.as_ref().is_some_and(|viewer| player.is_hidden_from(viewer));
        if player.is_hidden() || from_viewer {
            player_hidden += 1;
            if show_hidden || (viewer.is_some() && !from_viewer) {
                hidden_count += 1;
            }
        }
    }
    let max = horizon_bukkit::server::get_max_players();
    let shown = online.len() - player_hidden;
    let say = |key: &str, args: &[crate::i18n::Arg]| user.map_or_else(|| tl_literal(key, args), |user| user.player_tl(key, args));
    if hidden_count > 0 { say("listAmountHidden", crate::args![shown, hidden_count, max]) } else { say("listAmount", crate::args![shown, max]) }
}

/// `getPlayerLists`.
pub fn get_player_lists(sender: Option<&User>, show_hidden: bool) -> GroupedUsers {
    let mut lists = GroupedUsers::new();
    let viewer = sender.and_then(User::player);
    for user in ess().get_online_users() {
        let hidden = match &viewer {
            None => user.is_hidden(),
            Some(viewer) => user.is_hidden_from(viewer),
        };
        if !show_hidden && hidden {
            continue;
        }
        let group = strip_format(&strip_essentials_format(&user.get_group().to_lowercase()));
        lists.entry(group).or_default().push(user);
    }
    lists
}

/// `getMergedList`.
pub fn get_merged_list(lists: &mut GroupedUsers, group_name: &str) -> Vec<User> {
    let mut users = Vec::new();
    for (config_group, value) in settings().get_list_group_config() {
        if config_group.eq_ignore_ascii_case(group_name) {
            for value in value.trim().split(' ') {
                let value = value.to_lowercase();
                if value.is_empty() {
                    continue;
                }
                if let Some(found) = lists.remove(value.trim()).filter(|found| !found.is_empty()) {
                    users.extend(found);
                }
            }
        }
    }
    users
}

/// `listGroupUsers`.
pub fn list_group_users(lists: &mut GroupedUsers, group_name: &str) -> EssResult<String> {
    let mut users = get_merged_list(lists, group_name);
    if let Some(group) = lists.get(group_name) {
        users.extend(group.iter().cloned());
    }
    fail_if!(users.is_empty(), EssError::tl("groupDoesNotExist", &[]));
    let mut chars = group_name.chars();
    let display: String = chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default();
    Ok(output_format(&display, &list_users(users, ", ")))
}

/// `outputFormat`.
pub fn output_format(group: &str, message: &str) -> String {
    format!("{}{message}", tl_literal("listGroupTag", crate::args![replace_format(group)]))
}

/// `prepareGroupedList`.
pub fn prepare_grouped_list(source: Option<&CommandSource>, command_label: &str, lists: &mut GroupedUsers) -> Vec<String> {
    let mut output = Vec::new();
    let mut asterisk = Vec::new();
    for (original, value) in settings().get_list_group_config() {
        let value = value.trim().to_owned();
        let group = original.to_lowercase();
        if value == "*" {
            asterisk.push(original);
            continue;
        }
        if value.eq_ignore_ascii_case("hidden") {
            lists.remove(&group);
            continue;
        }
        if is_int(&value)
            && let Some(matched) = lists.remove(&group).filter(|matched| !matched.is_empty())
        {
            let limit = value.parse::<usize>().unwrap_or(0);
            if matched.len() > limit {
                let args = crate::args![matched.len(), command_label, strip_format(&group)];
                output.push(output_format(&original, &source.map_or_else(|| tl_literal("groupNumber", args), |source| source.tl("groupNumber", args))));
            } else {
                output.push(output_format(&original, &list_users(matched, ", ")));
            }
            continue;
        }
        let users = get_merged_list(lists, &group);
        if !users.is_empty() {
            output.push(output_format(&original, &list_users(users, ", ")));
        }
    }
    let online_groups: Vec<String> = lists.keys().cloned().collect();
    let groups: Vec<String> = if asterisk.is_empty() {
        online_groups
    } else {
        let everyone: Vec<User> = lists.values().flatten().cloned().collect();
        for key in &asterisk {
            lists.insert(key.clone(), everyone.clone());
        }
        asterisk.clone()
    };
    for group in groups {
        let Some(users) = lists.get(&group).filter(|users| !users.is_empty()).cloned() else { continue };
        let mut name = if asterisk.is_empty() { users[0].get_group() } else { group };
        if crate::perm::get_name() == "ConfigPermissions" {
            name = source.map_or_else(|| tl_literal("connectedPlayers", &[]), |source| source.tl("connectedPlayers", &[]));
        }
        output.push(output_format(&name, &list_users(users, ", ")));
    }
    output
}
