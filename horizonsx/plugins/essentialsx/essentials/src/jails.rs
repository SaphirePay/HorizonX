//! `Jails`: `jail.yml`, and the listener that keeps the jailed in. Java registers that listener once
//! a jail exists; here it's always on and each handler finds nobody jailed, which comes to the same.
use crate::prelude::*;
use crate::config::Config;
use crate::config_entities::LazyLocation;
use horizon_bukkit::enums::event_priority::{HIGH, HIGHEST, LOW};
use horizon_bukkit::Plugin;
use horizon_bukkit::event::{BlockBreakEvent, PlayerInteractEvent, PlayerJoinEvent, PlayerRespawnEvent};
use horizon_bukkit::events::PlayerTeleportEvent;
use horizon_bukkit::fam_ev_entity::EntityDamageByEntityEvent;
use horizon_bukkit::fam_ev_player::PlayerGameModeChangeEvent;
use horizon_bukkit::fam_ev_world::BlockDamageEvent;
use std::collections::BTreeMap;
use std::sync::Mutex;

struct State {
    config: Config,
    jails: BTreeMap<String, LazyLocation>,
}

pub struct Jails {
    state: Mutex<State>,
}

impl Jails {
    pub fn new() -> Self {
        let jails = Self { state: Mutex::new(State { config: Config::new(crate::data_folder().join("jail.yml")), jails: BTreeMap::new() }) };
        jails.reload_config();
        jails
    }
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `reloadConfig`.
    pub fn reload_config(&self) {
        let mut state = self.state();
        state.config.load();
        let found: BTreeMap<String, LazyLocation> = state.config.section_map("jails").into_iter().filter_map(|(name, node)| Some((name.to_lowercase(), LazyLocation::read(&node)?))).collect();
        state.jails = found;
    }
    /// `startTransaction`.
    pub fn start_transaction(&self) {
        self.state().config.start_transaction();
    }
    /// `stopTransaction`.
    pub fn stop_transaction(&self, blocking: bool) {
        self.state().config.stop_transaction(blocking);
    }
    /// `getJail`.
    pub fn get_jail(&self, name: Option<&str>) -> EssResult<Location> {
        let name = name.ok_or_else(|| EssError::tl("jailNotExist", &[]))?.to_lowercase();
        let mut state = self.state();
        let place = state.jails.get_mut(&name).ok_or_else(|| EssError::tl("jailNotExist", &[]))?;
        place.location().ok_or_else(|| EssError::tl("jailWorldNotExist", &[]))
    }
    /// `getList`.
    pub fn get_list(&self) -> Vec<String> {
        // `jails` is a `HashMap`, and `/jails` prints it as it comes.
        crate::utils::string::java_hash_map_order(self.state().jails.keys().cloned())
    }
    /// `removeJail`.
    pub fn remove_jail(&self, name: &str) {
        let name = name.to_lowercase();
        let mut state = self.state();
        if state.jails.remove(&name).is_some() {
            state.config.remove(&format!("jails.{name}"));
            state.config.save();
        }
    }
    /// `sendToJail`.
    pub fn send_to_jail(&self, user: &User, jail_name: &str, future: EssFuture) -> EssResult {
        if jail_name.is_empty() {
            return Ok(());
        }
        let jail = jail_name.to_lowercase();
        if self.state().jails.contains_key(&jail) {
            if user.is_online() {
                let location = self.get_jail(Some(&jail))?;
                user.get_async_teleport().now(&location, false, "COMMAND", future.clone());
                let target = user.clone();
                future.then_accept(move |_| target.set_jail(Some(jail)));
                return Ok(());
            }
            user.set_jail(Some(jail));
        }
        Ok(())
    }
    /// `setJail`.
    pub fn set_jail(&self, name: &str, location: &Location) {
        let name = name.to_lowercase();
        let place = LazyLocation::from_location(location);
        let mut state = self.state();
        state.config.set_location(Some(&format!("jails.{name}")), &place);
        state.jails.insert(name, place);
        state.config.save();
    }
    /// `getCount`.
    pub fn get_count(&self) -> usize {
        self.state().jails.len()
    }
}

/// `shouldIgnore`: Citizens' NPCs have version 2 ids.
fn should_ignore(player: &Player) -> bool {
    player.get_unique_id().as_bytes().get(14) == Some(&b'2')
}

fn log_return_error(user: &User, error: &EssError) {
    crate::info(&crate::adventure::mini_to_legacy(&tl_literal("returnPlayerToJailError", crate::args![user.get_name(), error.get_message()])));
}

/// `JailListener`.
pub fn register(plugin: Plugin) {
    let jailed = |player: &Player, node: Option<&str>| {
        if should_ignore(player) {
            return false;
        }
        ess().get_user(player).is_some_and(|user| user.is_jailed() && node.is_none_or(|node| !user.is_authorized(node)))
    };
    plugin.register_event_with(LOW, true, move |event: &mut BlockBreakEvent| {
        if jailed(event.get_player(), Some("essentials.jail.allow-break")) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, true, move |event: &mut horizon_bukkit::fam_ev_world::BlockPlaceEvent| {
        if jailed(event.get_player(), Some("essentials.jail.allow-place")) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, true, move |event: &mut BlockDamageEvent| {
        if event.get_player().is_some_and(|player| jailed(&player, Some("essentials.jail.allow-block-damage"))) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, true, move |event: &mut EntityDamageByEntityEvent| {
        if event.get_damager().is_some_and(|damager| damager.as_player().is_some_and(|player| jailed(player, None))) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, true, move |event: &mut PlayerInteractEvent| {
        if jailed(event.get_player(), Some("essentials.jail.allow-interact")) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, true, move |event: &mut PlayerGameModeChangeEvent| {
        if event.get_player().is_some_and(|player| jailed(&player, None)) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGHEST, false, |event: &mut PlayerRespawnEvent| {
        if should_ignore(event.get_player()) {
            return;
        }
        let Some(user) = ess().get_user(event.get_player()) else { return };
        let jail = user.get_jail().filter(|jail| user.is_jailed() && !jail.is_empty());
        if let Some(jail) = jail {
            match ess().jails().get_jail(Some(&jail)) {
                Ok(location) => event.set_respawn_location(&location),
                Err(error) => log_return_error(&user, &error),
            }
        }
    });
    plugin.register_event_with(HIGH, false, |event: &mut PlayerTeleportEvent| {
        if should_ignore(event.get_player()) {
            return;
        }
        let Some(user) = ess().get_user(event.get_player()) else { return };
        let jail = user.get_jail().filter(|jail| user.is_jailed() && !jail.is_empty());
        if let Some(jail) = jail {
            match ess().jails().get_jail(Some(&jail)) {
                Ok(location) => event.set_to(&location),
                Err(error) => log_return_error(&user, &error),
            }
            user.send_tl("jailMessage", &[]);
        }
    });
    plugin.register_event_with(HIGHEST, false, |event: &mut PlayerJoinEvent| {
        if should_ignore(event.get_player()) {
            return;
        }
        let Some(user) = ess().get_user(event.get_player()) else { return };
        user.check_jail_timeout(now_ms());
        let jail = user.get_jail().filter(|jail| user.is_jailed() && !jail.is_empty());
        let Some(jail) = jail else { return };
        let future = EssFuture::new();
        let (failed, told) = (user.clone(), user.clone());
        future.exceptionally(move |error| log_return_error(&failed, error));
        future.then_accept(move |_| told.send_tl("jailMessage", &[]));
        if let Err(error) = ess().jails().send_to_jail(&user, &jail, future.clone()) {
            future.complete_exceptionally(error);
        }
    });
    if settings().is_debug() {
        crate::info("Registering Jail listener");
    }
}
