//! `Essentials`, as `IEssentials`: one instance, reached with `ess()`. Everything hanging off it lives
//! as long as the plugin, so its getters give plain references.
use crate::prelude::*;
use crate::backup::Backup;
use crate::random_teleport::RandomTeleport;
use crate::balance_top::BalanceTop;
use crate::items::db::ItemDb;
use crate::jails::Jails;
use crate::kits::Kits;
use crate::settings::Settings;
use crate::textreader::{KeywordReplacer, SimpleTextInput};
use crate::timer::EssentialsTimer;
use crate::user_map::UserMap;
use crate::warps::Warps;
use crate::worth::Worth;
use crate::{adventure, args};
use horizon_bukkit::{Plugin, get_online_players, get_player, scheduler};
use std::sync::{Mutex, OnceLock};

pub struct Essentials {
    plugin: Plugin,
    settings: Settings,
    users: UserMap,
    jails: Jails,
    warps: Warps,
    worth: Worth,
    kits: Kits,
    item_db: ItemDb,
    backup: Backup,
    random_teleport: RandomTeleport,
    timer: EssentialsTimer,
    balance_top: BalanceTop,
    vanished: Mutex<Vec<String>>,
    started: i64,
}

static ESS: OnceLock<Essentials> = OnceLock::new();
/// Set while the plugin is being built: `Settings` reads the item database before it exists.
static ITEM_DB_READY: OnceLock<()> = OnceLock::new();

/// The plugin. Only called once `on_enable` has built it.
pub fn ess() -> &'static Essentials {
    ESS.get().expect("Essentials is enabled before anything uses it")
}

/// `ess.getSettings()`.
pub fn settings() -> &'static Settings {
    ess().settings()
}

/// `ess.getItemDb()`.
pub fn item_db() -> &'static ItemDb {
    ess().item_db()
}

/// `ess.getItemDb()`, for `Settings`, which runs before it's loaded.
pub fn try_item_db() -> Option<&'static ItemDb> {
    ITEM_DB_READY.get().and_then(|()| ESS.get()).map(Essentials::item_db)
}

/// `getStatistic(PLAY_ONE_MINUTE)`: ticks played, from the stats file.
pub fn play_one_tick(player: &Player) -> i64 {
    horizon_bukkit::get_offline_player(player.get_name()).get_statistic("minecraft:custom", "minecraft:play_time")
}

/// `getOffline().getStatistic(PLAY_ONE_TICK)`, for someone who may be away.
pub fn play_one_tick_of(user: &User) -> i64 {
    horizon_bukkit::get_offline_player(&user.get_name()).get_statistic("minecraft:custom", "minecraft:play_time")
}

/// The modules' `IConf`s.
static RELOAD_LISTENERS: std::sync::Mutex<Vec<fn()>> = std::sync::Mutex::new(Vec::new());

/// Builds the plugin in the order `onEnable` does, and calls `reload`.
pub(crate) fn init(plugin: Plugin) {
    crate::set_plugin(plugin);
    let settings = Settings::new();
    settings.reload_config();
    let essentials = Essentials {
        plugin,
        settings,
        users: UserMap::new(),
        jails: Jails::new(),
        warps: Warps::new(),
        worth: Worth::new(),
        kits: Kits::new(),
        item_db: ItemDb::new(),
        backup: Backup::default(),
        random_teleport: RandomTeleport::new(plugin.get_data_folder().join("tpr.yml")),
        timer: EssentialsTimer::new(),
        balance_top: BalanceTop::default(),
        vanished: Mutex::new(Vec::new()),
        started: crate::now_ms(),
    };
    if ESS.set(essentials).is_err() {
        crate::severe("Essentials was enabled twice");
    }
    crate::i18n::update_locale(&ess().settings.get_locale());
    ess().item_db.reload_config();
    let _ = ITEM_DB_READY.set(());
    ess().settings.late_load_item_spawn_blacklist();
}

/// The parts of the plugin, each reached by its name.
macro_rules! parts {
    ($($name:ident $kind:ty),*) => {
        impl Essentials { $(pub fn $name(&self) -> &$kind { &self.$name })* }
    };
}

parts!(settings Settings, users UserMap, jails Jails, warps Warps, worth Worth, kits Kits, item_db ItemDb, random_teleport RandomTeleport, backup Backup, balance_top BalanceTop);

impl Essentials {
    pub fn plugin(&self) -> Plugin {
        self.plugin
    }
    pub fn timer_average_tps(&self) -> f64 {
        self.timer.get_average_tps()
    }
    pub(crate) fn timer(&self) -> &EssentialsTimer {
        &self.timer
    }
    /// When the plugin started: Java's `getRuntimeMXBean().getStartTime()` for `{UPTIME}`.
    pub fn start_time(&self) -> i64 {
        self.started
    }
    /// `addReloadListener`, for a module's own `IConf`.
    pub fn add_reload_listener(&self, listener: fn()) {
        RELOAD_LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).push(listener);
    }
    /// `reload`: every `IConf`, in `confList`'s order, then the locale.
    pub fn reload(&self) {
        let listeners = RELOAD_LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for listener in listeners {
            listener();
        }
        crate::trade::Trade::close_log();
        self.settings.reload_config();
        self.kits.reload_config();
        self.warps.reload_config();
        self.worth.reload_config();
        self.item_db.reload_config();
        self.random_teleport.reload_config();
        self.jails.reload_config();
        self.settings.late_load_item_spawn_blacklist();
        crate::i18n::update_locale(&self.settings.get_locale());
    }
    /// `getUser(Player)`.
    pub fn get_user(&self, base: &Player) -> Option<User> {
        Some(self.users.get_user(base))
    }
    /// `getUser(UUID)`.
    pub fn get_user_by_uuid(&self, uuid: &str) -> Option<User> {
        self.users.get_user_by_uuid(uuid)
    }
    /// `getOfflineUser(String)` and `getUser(String)`.
    pub fn get_offline_user(&self, name: &str) -> Option<User> {
        self.users.get_user_by_name(name)
    }
    /// `getOnlineUsers`.
    pub fn get_online_users(&self) -> Vec<User> {
        get_online_players().iter().filter_map(|player| self.get_user(player)).collect()
    }
    /// `getOnlinePlayers`.
    pub fn get_online_players(&self) -> Vec<Player> {
        get_online_players()
    }
    /// `getJailedPlayers`.
    pub fn get_jailed_players(&self) -> Vec<Player> {
        self.get_online_users().iter().filter(|user| user.is_jailed()).filter_map(User::player).collect()
    }
    /// `getWorld`: a number is an index into the worlds.
    pub fn get_world(&self, name: &str) -> Option<World> {
        if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit())
            && let Some(world) = name.parse::<usize>().ok().and_then(|index| get_worlds().into_iter().nth(index))
        {
            return Some(world);
        }
        horizon_bukkit::get_world(name)
    }
    /// `server.getPlayer(String)`: exact, or the nearest name that starts so.
    pub fn get_player_partial(&self, name: &str) -> Option<Player> {
        if let Some(player) = get_player(name) {
            return Some(player);
        }
        let lower = name.to_lowercase();
        get_online_players().into_iter().filter(|player| player.get_name().to_lowercase().starts_with(&lower)).min_by_key(|player| player.get_name().len())
    }
    /// `canInteractWith(CommandSource, User)`.
    pub fn can_interact_with_source(&self, interactor: Option<&CommandSource>, interactee: &User) -> bool {
        match interactor {
            None => !interactee.is_hidden(),
            Some(source) => match source.get_user() {
                Some(user) => self.can_interact_with(Some(&user), interactee),
                None => true,
            },
        }
    }
    /// `canInteractWith(User, User)`.
    pub fn can_interact_with(&self, interactor: Option<&User>, interactee: &User) -> bool {
        let Some(interactor) = interactor else { return !interactee.is_hidden() };
        if interactor == interactee {
            return true;
        }
        interactor.player().is_none_or(|base| !interactee.is_hidden_from(&base))
    }
    /// `matchUser`.
    pub fn match_user(&self, source_user: Option<&User>, search_term: &str, get_hidden: bool, get_offline: bool) -> Result<User, EssError> {
        if let Some(user) = source_user.filter(|_| search_term == "@p" || search_term == "@s") {
            return Ok(user.clone());
        }
        let found = match crate::utils::string::to_uuid(search_term) {
            Some(uuid) => horizon_bukkit::fam_ess::get_player_by_id(&uuid),
            None if get_offline => get_player(search_term),
            None => self.get_player_partial(search_term),
        };
        let user = match &found {
            Some(player) => self.get_user(player),
            None => self.get_offline_user(search_term),
        };
        if let Some(user) = user {
            fail_if!(!get_offline && !user.is_online(), EssError::player_not_found());
            if get_hidden || self.can_interact_with(source_user, &user) || (get_offline && user.get_name().eq_ignore_ascii_case(search_term)) {
                return Ok(user);
            }
            return Err(EssError::player_not_found());
        }
        let matches = horizon_bukkit::world::match_player(search_term);
        if matches.is_empty() {
            let text = search_term.to_lowercase();
            for user in self.get_online_users() {
                if (get_hidden || self.can_interact_with(source_user, &user)) && strip_format(&user.get_display_name()).to_lowercase().contains(&text) {
                    return Ok(user);
                }
            }
            return Err(EssError::player_not_found());
        }
        let users: Vec<User> = matches.iter().filter_map(|player| self.get_user(player)).collect();
        let allowed = |user: &&User| get_hidden || self.can_interact_with(source_user, user);
        users
            .iter()
            .filter(allowed)
            .find(|user| user.get_name().eq_ignore_ascii_case(search_term))
            .or_else(|| users.iter().filter(allowed).find(|user| user.get_display_name().starts_with(search_term)))
            .or_else(|| users.first().filter(allowed))
            .cloned()
            .ok_or_else(EssError::player_not_found)
    }
    /// `showError`.
    pub fn show_error(&self, sender: &CommandSource, error: &EssError, label: &str) {
        match error {
            EssError::Tl { key, args } => {
                let message = sender.tl(key, args);
                sender.send_tl("errorWithMessage", args![crate::i18n::parsed(message)]);
            }
            other => sender.send_tl("errorWithMessage", args![other.get_message()]),
        }
        if self.settings.is_debug() {
            crate::info(&adventure::mini_to_legacy(&tl_literal("errorCallingCommand", args![label])));
        }
    }
    /// `broadcastMessage(message)`.
    pub fn broadcast_message(&self, message: &str) -> usize {
        self.broadcast(None, None, message, true)
    }
    /// `broadcastMessage(sender, message)`.
    pub fn broadcast_message_from(&self, sender: &User, message: &str) -> usize {
        self.broadcast(Some(sender), None, message, false)
    }
    /// `broadcastMessage(permission, message)`.
    pub fn broadcast_message_to(&self, permission: &str, message: &str) -> usize {
        self.broadcast(None, Some(permission), message, false)
    }
    fn broadcast(&self, sender: Option<&User>, permission: Option<&str>, message: &str, keywords: bool) -> usize {
        if sender.is_some_and(User::is_hidden) {
            return 0;
        }
        let users = self.get_online_users();
        for user in &users {
            let wanted = match permission {
                None => sender.is_none_or(|sender| !user.is_ignored_player(sender)),
                Some(node) => user.is_authorized(node),
            };
            if !wanted {
                continue;
            }
            let text = SimpleTextInput::new(message);
            let lines = if keywords { KeywordReplacer::new(text, &user.get_source(), false, false).get_lines().to_vec() } else { text.lines };
            for line in lines {
                user.send_message(&line);
            }
        }
        users.len()
    }
    /// `broadcastTl`, with all its overloads.
    pub fn broadcast_tl(&self, sender: Option<&User>, should_exclude: Option<&dyn Fn(&User) -> bool>, parse_keywords: bool, key: &str, args: &[Arg]) {
        if sender.is_some_and(User::is_hidden) {
            return;
        }
        for user in self.get_online_users() {
            if sender.is_some_and(|sender| user.is_ignored_player(sender)) || should_exclude.is_some_and(|exclude| exclude(&user)) {
                continue;
            }
            if parse_keywords {
                let processed: Vec<Arg> = args
                    .iter()
                    .map(|arg| match arg {
                        Arg::Text(text) => Arg::Text(KeywordReplacer::new(SimpleTextInput::new(text), &user.get_source(), false, false).get_lines().first().cloned().unwrap_or_default()),
                        other => other.clone(),
                    })
                    .collect();
                user.send_tl(key, &processed);
            } else {
                user.send_tl(key, args);
            }
        }
    }
    /// `broadcastTl(sender, permission, key, args)`.
    pub fn broadcast_tl_permission(&self, sender: Option<&User>, permission: &str, key: &str, args: &[Arg]) {
        self.broadcast_tl(sender, Some(&|user: &User| !user.is_authorized(permission)), false, key, args);
    }
    /// `scheduleSyncDelayedTask(run)`.
    pub fn schedule_sync_delayed_task(&self, task: impl FnOnce() + Send + 'static) -> u32 {
        let mut task = Some(task);
        scheduler::run_task(move || {
            if let Some(task) = task.take() {
                task();
            }
        })
    }
    /// `scheduleSyncDelayedTask(run, delay)`.
    pub fn schedule_sync_delayed_task_later(&self, task: impl FnOnce() + Send + 'static, delay: u64) -> u32 {
        let mut task = Some(task);
        scheduler::run_task_later(delay, move || {
            if let Some(task) = task.take() {
                task();
            }
        })
    }
    /// `runTaskAsynchronously`.
    pub fn run_task_asynchronously(&self, task: impl FnOnce() + Send + 'static) {
        scheduler::run_task_asynchronously(task);
    }
    /// `getVanishedPlayersNew`.
    pub fn get_vanished_players(&self) -> Vec<String> {
        self.vanished.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub(crate) fn vanished_add(&self, name: &str) {
        let mut vanished = self.vanished.lock().unwrap_or_else(|e| e.into_inner());
        if !vanished.iter().any(|known| known == name) {
            vanished.push(name.to_owned());
        }
    }
    pub(crate) fn vanished_remove(&self, name: &str) {
        self.vanished.lock().unwrap_or_else(|e| e.into_inner()).retain(|known| known != name);
    }
}
