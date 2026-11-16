//! `EssentialsDiscordLink`. `DiscordLinkService` is `link_manager()`. It needs `essentials-discord`
//! up: linked into the one library, it calls that crate directly, where Java asks the services manager.
pub mod commands;
pub mod listeners;
pub mod manager;
pub mod rolesync;
pub mod settings;
pub mod storage;

use essentials::prelude::*;
use horizon_bukkit::Plugin;
use manager::AccountLinkManager;
use rolesync::RoleSyncManager;
use settings::DiscordLinkSettings;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use storage::AccountStorage;

const TEMPLATE: &str = include_str!("../config.yml");

/// What's up while the module is.
#[derive(Clone)]
struct State {
    settings: Arc<DiscordLinkSettings>,
    accounts: Arc<AccountStorage>,
    role_sync: Arc<RoleSyncManager>,
    link_manager: Arc<AccountLinkManager>,
}

static STATE: RwLock<Option<State>> = RwLock::new(None);

fn state() -> Option<State> {
    STATE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// `getLinkManager`: the service.
pub fn link_manager() -> Option<Arc<AccountLinkManager>> {
    state().map(|state| state.link_manager)
}

/// `getSettings`.
pub fn link_settings() -> Option<Arc<DiscordLinkSettings>> {
    state().map(|state| state.settings)
}

/// `getAccountStorage`.
pub fn account_storage() -> Option<Arc<AccountStorage>> {
    state().map(|state| state.accounts)
}

pub(crate) fn role_sync() -> Option<Arc<RoleSyncManager>> {
    state().map(|state| state.role_sync)
}

fn folder() -> PathBuf {
    essentials::data_folder().parent().map_or_else(|| PathBuf::from("plugins/EssentialsDiscordLink"), |plugins| plugins.join("EssentialsDiscordLink"))
}

/// `DiscordLinkSettings.reloadConfig`, then `onReload`.
fn on_reload() {
    if let Some(state) = state() {
        state.settings.reload_config();
        state.role_sync.on_reload();
    }
}

/// `onEnable`. With Discord down the Java dies on a null `api` here: this says so and stays off.
pub fn on_enable(plugin: Plugin) {
    let Some(api) = essentials_discord::service().filter(|service| service.is_live()) else {
        essentials::warning("EssentialsDiscordLink needs EssentialsDiscord connected: it stays off.");
        return;
    };
    let settings = Arc::new(DiscordLinkSettings::new(folder().join("config.yml"), TEMPLATE));
    ess().add_reload_listener(on_reload);
    let accounts = match AccountStorage::new(folder(), api.host().clone()) {
        Ok(accounts) => Arc::new(accounts),
        Err(error) => {
            essentials::severe(&format!("Unable to create link accounts file: {error}"));
            return;
        }
    };
    let role_sync = RoleSyncManager::new(settings.clone());
    let link_manager = Arc::new(AccountLinkManager::new(accounts.clone(), role_sync.clone()));
    *STATE.write().unwrap_or_else(|e| e.into_inner()) = Some(State { settings, accounts, role_sync: role_sync.clone(), link_manager: link_manager.clone() });
    role_sync.start(plugin);
    listeners::register(plugin);
    register_commands(plugin, &[commands::LINK, commands::UNLINK]);
    if let Err(error) = commands::register_interactions(&link_manager) {
        essentials::severe(&error.0);
        on_disable(plugin);
    }
}

/// `onDisable`.
pub fn on_disable(_plugin: Plugin) {
    let state = STATE.write().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(state) = state {
        state.role_sync.shutdown();
        state.accounts.shutdown();
    }
}
