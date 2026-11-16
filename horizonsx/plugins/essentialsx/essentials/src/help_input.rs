//! `HelpInput`. Java's order within a plugin is `HashMultimap`'s, of objects with no hash of their own:
//! it changes with every start, so these stay sorted.
use crate::prelude::*;
use horizon_bukkit::fam_core::{get_command_map, get_plugin_manager};
use std::collections::BTreeMap;

/// The commands of the modules in this library: to Java each is a plugin of its own, with these.
const MODULE_COMMANDS: [(&str, &[&str]); 5] = [("EssentialsSpawn", &["setspawn", "spawn"]), ("EssentialsChat", &["toggleshout"]), ("EssentialsXMPP", &["setxmpp", "xmpp", "xmppspy"]), ("EssentialsDiscord", &["discord", "discordbroadcast"]), ("EssentialsDiscordLink", &["link", "unlink"])];

/// Whose a command of this library is.
fn module_of(command: &str) -> &'static str {
    MODULE_COMMANDS.iter().find(|(_, commands)| commands.contains(&command)).map_or("Essentials", |(module, _)| module)
}

/// The lines for `match`, which is lower case and may be empty.
pub fn help_lines(user: &User, matching: &str) -> Vec<String> {
    let line = |key: &str, args: &[crate::i18n::Arg]| mini_to_legacy(&user.player_tl(key, args));
    let mut lines = Vec::new();
    let mut new_lines = Vec::new();
    if !matching.is_empty() {
        lines.push(line("helpMatching", crate::args![matching]));
    }
    let known = get_command_map().get_known_commands();
    let modules = crate::modules();
    let plugins = get_plugin_manager().get_plugins().iter().map(|plugin| plugin.get_name().to_owned()).collect::<Vec<_>>();
    let plugins = plugins.into_iter().flat_map(|plugin| {
        let own = plugin == "Essentials";
        std::iter::once(plugin).chain(modules.iter().filter(move |_| own).map(|module| (*module).to_owned()))
    });
    for plugin in plugins {
        let family = plugin == "Essentials" || modules.contains(&plugin.as_str());
        let lower = plugin.to_lowercase();
        if lower == matching {
            lines.clear();
            new_lines.clear();
            lines.push(line("helpFrom", crate::args![plugin.clone()]));
        }
        // `pluginCommands.get(p)`: each command once, whatever words it has.
        let space = if family { "essentials:".to_owned() } else { format!("{lower}:") };
        let commands: BTreeMap<&str, _> = known.iter().filter(|(word, command)| word.starts_with(&space) && (!family || module_of(command.get_name()) == plugin)).map(|(_, command)| (command.get_name(), command)).collect();
        let on_whitelist = user.is_authorized(&format!("essentials.help.{lower}"));
        let mut plugin_lines = Vec::new();
        for (name, command) in commands {
            let description = command.get_description();
            if !matching.is_empty() && !lower.contains(matching) && !name.to_lowercase().contains(matching) && !description.to_lowercase().contains(matching) {
                continue;
            }
            let shown = if lower.contains("essentials") {
                !settings().is_command_disabled(name) && user.is_authorized(&format!("essentials.{name}"))
            } else if !settings().show_non_ess_commands_in_help() {
                false
            } else if on_whitelist || user.is_authorized(&format!("essentials.help.{lower}.{name}")) {
                true
            } else {
                match command.get_permission() {
                    Some(permissions) => permissions.split(';').any(|permission| user.is_authorized(permission)),
                    None => !settings().hide_permissionless_help(),
                }
            };
            if shown {
                plugin_lines.push(line("helpLine", crate::args![name.to_owned(), description.to_owned()]));
            }
        }
        if !plugin_lines.is_empty() {
            new_lines.append(&mut plugin_lines);
            if lower == matching {
                break;
            }
            if matching.is_empty() {
                lines.push(line("helpPlugin", crate::args![plugin, lower]));
            }
        }
    }
    lines.append(&mut new_lines);
    lines
}
