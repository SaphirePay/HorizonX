//! `EssentialsDiscord`: the Java's module, on a small Discord client of its own in `client`. It's an
//! rlib like the other modules, and `DiscordService` is `service()`, which DiscordLink calls directly.
pub mod api;
pub mod client;
pub mod commands;
pub mod console;
pub mod emoji;
pub mod host;
pub mod interaction_commands;
pub mod interactions;
pub mod listeners;
pub mod message_util;
pub mod recipient;
pub mod service;
pub mod settings;
pub mod webhook;

use essentials::prelude::*;
use horizon_bukkit::Plugin;
use host::{GameHost, Host};
use service::JdaDiscordService;
use settings::DiscordSettings;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// The file a server gets if it has none.
pub const CONFIG_TEMPLATE: &str = include_str!("../config.yml");

static SERVICE: RwLock<Option<Arc<JdaDiscordService>>> = RwLock::new(None);

/// `getServicesManager().load(DiscordService.class)`.
pub fn service() -> Option<Arc<JdaDiscordService>> {
    SERVICE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

fn folder() -> PathBuf {
    essentials::data_folder().parent().map_or_else(|| PathBuf::from("plugins/EssentialsDiscord"), |plugins| plugins.join("EssentialsDiscord"))
}

/// `DiscordSettings.reloadConfig`, then `onReload`.
fn on_reload() {
    let Some(service) = service() else { return };
    if service.is_invalid_startup() {
        service.settings().warn_reload_invalid();
        return;
    }
    service.settings().reload_config();
    if service.is_live() {
        service.update_presence();
        if let Err(error) = service.update_primary_channel() {
            essentials::severe(&error);
        }
        service.update_console_relay();
        service.update_types_relay();
    }
}

/// `onEnable`. Like the Java this waits for Discord: DiscordLink's `on_enable` needs the guild's roles.
pub fn on_enable(plugin: Plugin) {
    let host: Arc<dyn Host> = Arc::new(GameHost);
    let settings = Arc::new(DiscordSettings::new(folder().join("config.yml"), CONFIG_TEMPLATE, host.clone()));
    settings.reload_config();
    ess().add_reload_listener(on_reload);
    console::set_log_file(horizon_bukkit::server::server_folder().join("logs/latest.log"));
    let service = JdaDiscordService::new(settings, host);
    *SERVICE.write().unwrap_or_else(|e| e.into_inner()) = Some(service.clone());
    register_commands(plugin, &[commands::DISCORD, commands::DISCORD_BROADCAST]);
    listeners::register(plugin);
    match service.startup() {
        Ok(()) => {
            interaction_commands::register(&service);
            let task = service.clone();
            ess().schedule_sync_delayed_task(move || {
                task.interaction_controller().process_batch_registration();
                let seconds = (essentials::now_ms() - ess().start_time()).abs() as f32 / 1000.0;
                task.send_start_message(seconds);
            });
        }
        Err(error) => {
            essentials::severe(&mini_to_legacy(&tl_literal("discordErrorLogin", essentials::args![error])));
            service.shutdown();
        }
    }
}

/// `onDisable`: no thread of this module outlives it.
pub fn on_disable(_plugin: Plugin) {
    let service = SERVICE.write().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(service) = service {
        service.shutdown();
    }
}
