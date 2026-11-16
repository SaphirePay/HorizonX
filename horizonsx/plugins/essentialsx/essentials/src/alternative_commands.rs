//! `AlternativeCommandsHandler`: a word another plugin has too, which Essentials gives way to unless
//! `overridden-commands` says otherwise. `EssentialsPluginListener` kept Java's list current as plugins
//! came and went; a plugin can't hear that here, so each look first checks the list of plugins.
use crate::ess::{ess, settings};
use horizon_bukkit::fam_core::{PluginCommand, get_command_map, get_plugin_command, get_plugin_manager};
use horizon_bukkit::CommandSender;
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

/// A `Command` of Java's list: looked up by `key` when it's wanted, so it's never a stale one.
struct Alternative {
    /// `plugin:word`.
    key: String,
    /// `getName`, which an alias doesn't change.
    name: String,
}

struct Handler {
    /// The enabled plugins this list was made from.
    plugins: Vec<String>,
    alt_commands: BTreeMap<String, Vec<(String, Alternative)>>,
    disabled_list: BTreeMap<String, String>,
    /// The engine's plugin-list number this was last checked against, and when.
    checked: Option<(u64, std::time::Instant)>,
}

static HANDLER: Mutex<Handler> = Mutex::new(Handler { plugins: Vec::new(), alt_commands: BTreeMap::new(), disabled_list: BTreeMap::new(), checked: None });

fn handler() -> MutexGuard<'static, Handler> {
    HANDLER.lock().unwrap_or_else(|e| e.into_inner())
}

/// `getMain().contains("com.earth2me.essentials")`: a library here has no main class.
fn is_essentials(plugin: &str) -> bool {
    plugin == ess().plugin().get_name() || plugin.starts_with("Essentials")
}

impl Handler {
    /// `addPlugin`.
    fn add_plugin(&mut self, plugin: &str) {
        let space = format!("{}:", plugin.to_ascii_lowercase());
        for (key, command) in get_command_map().get_known_commands() {
            let Some(word) = key.strip_prefix(&space) else { continue };
            let commands = self.alt_commands.entry(word.to_lowercase()).or_default();
            if !commands.iter().any(|(owner, _)| owner == plugin) {
                commands.push((plugin.to_owned(), Alternative { name: command.get_name().to_owned(), key: key.clone() }));
            }
        }
    }
    /// `removePlugin`.
    fn remove_plugin(&mut self, plugin: &str) {
        for commands in self.alt_commands.values_mut() {
            commands.retain(|(owner, _)| owner != plugin);
        }
        self.alt_commands.retain(|_, commands| !commands.is_empty());
    }
    /// What `onPluginEnable` and `onPluginDisable` did.
    fn refresh(&mut self) {
        // Java rebuilt this on PluginEnableEvent/PluginDisableEvent only. Reading the whole plugin list on every
        // look made a join (its command list filtered label by label) take ~1.5 s: read it again only when the
        // engine's number for the list moved, or once a second in case a plugin turned itself off.
        let version = horizon_bukkit::fam_core::plugins_version();
        let now = std::time::Instant::now();
        if version.is_some() && self.checked.is_some_and(|(known, at)| Some(known) == version && now.duration_since(at).as_millis() < 1000) {
            return;
        }
        if let Some(version) = version {
            self.checked = Some((version, now));
        }
        let mut enabled: Vec<String> = get_plugin_manager().get_plugins().into_iter().filter(|plugin| plugin.is_enabled()).map(|plugin| plugin.get_name()).filter(|plugin| !is_essentials(plugin)).collect();
        enabled.sort();
        if enabled == self.plugins {
            return;
        }
        for plugin in std::mem::take(&mut self.plugins) {
            if !enabled.contains(&plugin) {
                self.remove_plugin(&plugin);
            }
        }
        for plugin in &enabled {
            self.add_plugin(plugin);
        }
        self.plugins = enabled;
    }
    /// `getAlternative`.
    fn get_alternative(&self, label: &str) -> Option<&Alternative> {
        let commands = self.alt_commands.get(&label.to_lowercase())?;
        // The first that isn't an alias, then the first.
        commands.iter().find(|(_, command)| commands.len() > 1 && command.name.eq_ignore_ascii_case(label)).or_else(|| commands.first()).map(|(_, command)| command)
    }
}

/// `getAlternative`.
pub fn get_alternative(label: &str) -> Option<PluginCommand> {
    let mut handler = handler();
    handler.refresh();
    get_plugin_command(&handler.get_alternative(label)?.key)
}

/// The test at the head of `onCommandEssentials` and `onTabCompleteEssentials`, then the look.
/// `e`-words are Essentials' own spellings, so another plugin has no claim on them.
pub fn for_command(name: &str, label: &str) -> Option<PluginCommand> {
    if settings().is_command_overridden(name) || (label.starts_with('e') && !label.eq_ignore_ascii_case(name)) {
        return None;
    }
    if settings().is_debug() {
        crate::info(&format!("Searching for alternative to: {label}"));
    }
    get_alternative(label)
}

/// `executed`.
pub fn executed(label: &str, command: &PluginCommand) {
    let alternative = format!("{}:{}", command.get_plugin_name(), command.get_command().get_name());
    if settings().is_debug() {
        crate::info(&format!("Essentials: Alternative command {label} found, using {alternative}"));
    }
    handler().disabled_list.insert(label.to_owned(), alternative);
}

/// `disabledCommands`.
pub fn disabled_commands() -> BTreeMap<String, String> {
    handler().disabled_list.clone()
}

/// `_addAlternativeCommand`: with `disabled-commands` taking Essentials' command away, the word goes
/// to whoever else has it, the game included. `false` if nobody does.
pub fn run_in_place_of_disabled(sender: &CommandSender, label: &str, words: &[&str]) -> bool {
    if let Some(alternative) = get_alternative(label) {
        alternative.execute(sender, label, words);
        return true;
    }
    let game = format!("minecraft:{label}");
    if get_command_map().get_command(&game).is_none() {
        return false;
    }
    let line = std::iter::once(game.as_str()).chain(words.iter().copied()).collect::<Vec<_>>().join(" ");
    match sender.as_player() {
        Some(player) => player.perform_command(&line),
        None => horizon_bukkit::dispatch_command(&line),
    };
    true
}
