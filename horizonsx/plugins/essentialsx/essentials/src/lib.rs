//! EssentialsX 2.22.0 in Rust, for Horizon. GPL-3.0, as the original is: see NOTICE.
//!
//! This is the plugin's foundation; the commands sit in `commands/`, one file each. How to add one
//! is in COMMANDS.md, and what isn't done, and why, in GAPS.md.
pub mod adventure;
pub mod alternative_commands;
pub mod backup;
pub mod balance_top;
pub mod ban_lookup;
pub mod big_decimal;
pub mod command_source;
pub mod commands;
pub mod config;
pub mod config_entities;
pub mod economy;
pub mod error;
pub mod help_input;
pub mod hooks;
pub mod ess;
pub mod future;
pub mod i18n;
pub mod inventories;
pub mod items;
pub mod jails;
pub mod kits;
pub mod listeners;
pub mod mail;
pub mod messaging;
pub mod mob;
pub mod perm;
pub mod mob_data;
pub mod paste;
pub mod player_list;
pub mod prelude;
pub mod random_teleport;
pub mod recipes;
pub mod spawn_mob;
pub mod set_exp_fix;
pub mod settings;
pub mod signs;
pub mod teleport;
pub mod textreader;
pub mod timer;
pub mod trade;
pub mod update_check;
pub mod user;
pub mod user_data;
pub mod user_map;
pub mod utils;
pub mod warps;
pub mod worth;

#[cfg(feature = "plugin")]
use horizon_bukkit::bukkit_plugin;
use horizon_bukkit::{Plugin, scheduler};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static PLUGIN: OnceLock<Plugin> = OnceLock::new();
/// The modules linked into this library, in the order they started: each is a plugin to Java.
static MODULES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

/// A module of this library, for what lists plugins.
pub fn add_module(name: &'static str) {
    MODULES.lock().unwrap_or_else(|e| e.into_inner()).push(name);
}

/// `pm.getPlugins()` of Essentials' own family.
pub fn modules() -> Vec<&'static str> {
    MODULES.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub(crate) fn set_plugin(plugin: Plugin) {
    let _ = PLUGIN.set(plugin);
}

/// `System.currentTimeMillis`.
pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_millis() as i64)
}

/// `getDataFolder`.
pub fn data_folder() -> PathBuf {
    PLUGIN.get().map_or_else(|| PathBuf::from("plugins/Essentials"), Plugin::get_data_folder)
}

/// `getLogger().info`.
pub fn info(text: &str) {
    if let Some(plugin) = PLUGIN.get() {
        plugin.info(text);
    }
}

/// `getLogger().warning`.
pub fn warning(text: &str) {
    if let Some(plugin) = PLUGIN.get() {
        plugin.warning(text);
    }
}

/// `getLogger().severe`.
pub fn severe(text: &str) {
    if let Some(plugin) = PLUGIN.get() {
        plugin.severe(text);
    }
}

pub fn on_enable(plugin: Plugin) {
    let _ = std::fs::create_dir_all(plugin.get_data_folder());
    ess::init(plugin);
    ess::ess().run_task_asynchronously(|| {
        info(&adventure::mini_to_legacy(&i18n::tl_literal("versionFetching", &[])));
        for (key, args) in update_check::version_messages(false, true) {
            let line = adventure::mini_to_legacy(&i18n::tl_literal(key, &args));
            if ess::settings().is_update_check_enabled() { warning(&line) } else { info(&line) }
        }
    });
    let essentials = ess::ess();
    listeners::register(plugin);
    jails::register(plugin);
    commands::register(plugin);
    // `scheduleSyncRepeatingTask(timer, 1000, 50)`.
    scheduler::run_task_timer(1000, 50, || ess::ess().timer().run());
    // `ModernUUIDCache`'s writer: every five seconds, off the server thread's way.
    scheduler::run_task_timer(100, 100, || ess::ess().users().save_if_dirty());
    if !essentials.get_online_users().is_empty() || essentials.settings().is_always_run_backup() {
        essentials.backup().start_task();
    }
    essentials.backup().set_pending_shutdown(false);
    economy::provide_vault(plugin);
}

pub fn on_disable(_plugin: Plugin) {
    let essentials = ess::ess();
    essentials.backup().set_pending_shutdown(true);
    for user in essentials.get_online_users() {
        if user.is_vanished() {
            user.set_vanished(false);
            user.send_tl("unvanishedReload", &[]);
        }
        // The hook this runs on is the server stopping, so it's always Java's `stopping` branch.
        user.set_logout_location();
        if !user.is_hidden() {
            user.set_last_logout(now_ms());
        }
        user.cleanup();
    }
    listeners::player::cleanup_open_inventories();
    if essentials.backup().is_active() {
        severe(&adventure::mini_to_legacy(&i18n::tl_literal("backupInProgress", &[])));
    }
    essentials.backup().stop_task();
    trade::Trade::close_log();
    essentials.users().shutdown();
    config::shutdown_executor();
}

#[cfg(feature = "plugin")]
bukkit_plugin!(on_enable, on_disable);
