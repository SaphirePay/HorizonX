//! `User`, `UserData` and `PlayerExtension`. A `User` is a cheap handle: clones share one record, as
//! Java's references do. No lock here is held across a call out of this file.
use crate::prelude::*;
use crate::config::Config;
use crate::config_entities::{CommandCooldown, JavaPattern, LazyLocation, MailMessage};
use crate::i18n;
use crate::i18n::{Locale, tl_locale};
use crate::messaging::RecipientState;
use crate::settings::TeleportWhenFreePolicy;
use crate::teleport::{AsyncTeleport, TeleportState};
use crate::user_data::UserConfigHolder;
use crate::utils::string::safe_string;
use crate::{adventure, args};
use horizon_bukkit::Component;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// `TpaRequest`.
#[derive(Clone, Debug)]
pub struct TpaRequest {
    pub name: String,
    pub uuid: String,
    pub time: i64,
    pub location: Option<Location>,
    pub here: bool,
}

/// What `AfkStatusChangeEvent.Cause` tells apart. Nothing reads it until a listener can.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfkCause {
    Activity,
    Move,
    Interact,
    Chat,
    Join,
    Quit,
    Command,
    Unknown,
}

/// Everything that lives only while the record is loaded.
#[derive(Default)]
pub struct Transient {
    pub base: Option<Player>,
    /// `OfflinePlayerStub`'s name.
    pub offline_name: Option<String>,
    pub money: BigDecimal,
    pub confirming_payments: HashMap<String, BigDecimal>,
    pub confirming_clear_command: Option<String>,
    pub last_home_confirmation: Option<String>,
    pub tpa_queue: Vec<(String, TpaRequest)>,
    pub vanished: bool,
    pub hidden: bool,
    pub leaving_hidden: bool,
    pub right_click_jump: bool,
    pub inv_see: bool,
    pub inv_see_owner: Option<String>,
    pub recipe_see: bool,
    pub ender_see: bool,
    pub ignore_msg: bool,
    pub toggle_shout: Option<bool>,
    pub freeze: bool,
    pub afk_message: Option<String>,
    pub afk_since: i64,
    pub afk_position: Option<Location>,
    pub last_online_activity: i64,
    pub last_throttled_action: i64,
    pub last_activity: i64,
    pub teleport_invulnerability_timestamp: i64,
    pub last_notified_about_mails_ms: i64,
    pub last_home_confirmation_timestamp: i64,
    pub sign_copy: [String; 4],
    pub last_vanish_time: i64,
    pub flight_tick: i64,
    pub last_locale_string: Option<String>,
    pub player_locale: Option<Locale>,
    pub recipient: RecipientState,
}

struct Inner {
    uuid: String,
    config: Mutex<Config>,
    holder: Mutex<UserConfigHolder>,
    state: Mutex<Transient>,
    teleport: Mutex<TeleportState>,
}

#[derive(Clone)]
pub struct User(Arc<Inner>);

impl PartialEq for User {
    /// `equals`: by name, ignoring case.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.get_name().eq_ignore_ascii_case(&other.get_name())
    }
}

impl std::fmt::Debug for User {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "User({})", self.0.uuid)
    }
}

fn user_file(uuid: &str) -> std::path::PathBuf {
    crate::data_folder().join("userdata").join(format!("{uuid}.yml"))
}

/// Plain fields, as one table: `holder` is saved with the record, `state` lives while it's loaded.
/// Each row is the lock, the getter, the setter (`_` for none), the type and the field.
macro_rules! fields {
    (@set holder _ $kind:ty, $($field:ident).+) => {};
    (@set state _ $kind:ty, $($field:ident).+) => {};
    (@set holder $set:ident $kind:ty, $($field:ident).+) => {
        pub fn $set(&self, value: $kind) {
            self.change(|holder| holder.$($field).+ = value);
        }
    };
    (@set state $set:ident $kind:ty, $($field:ident).+) => {
        pub fn $set(&self, value: $kind) {
            self.state().$($field).+ = value;
        }
    };
    (copy: $($(#[$doc:meta])* $lock:ident $get:ident $set:tt $kind:ty, $($field:ident).+;)*) => {
        impl User {
            $($(#[$doc])* pub fn $get(&self) -> $kind { self.$lock().$($field).+ }
            fields!(@set $lock $set $kind, $($field).+);)*
        }
    };
    (clone: $($(#[$doc:meta])* $lock:ident $get:ident $set:tt $kind:ty, $($field:ident).+;)*) => {
        impl User {
            $($(#[$doc])* pub fn $get(&self) -> $kind { self.$lock().$($field).+.clone() }
            fields!(@set $lock $set $kind, $($field).+);)*
        }
    };
}

fields!(copy:
    holder get_last_teleport_timestamp set_last_teleport_timestamp i64, timestamps.last_teleport;
    holder get_last_heal_timestamp set_last_heal_timestamp i64, timestamps.last_heal;
    holder is_teleport_enabled set_teleport_enabled bool, teleport_enabled;
    holder is_auto_teleport_enabled set_auto_teleport_enabled bool, teleport_auto;
    /// `isGodModeEnabledRaw`.
    holder is_god_mode_enabled_raw set_god_mode_enabled bool, god_mode;
    holder is_fly_mode_enabled set_fly_mode_enabled bool, fly_mode;
    holder is_muted set_muted bool, muted;
    holder get_mute_timeout set_mute_timeout i64, timestamps.mute;
    holder is_jailed set_jailed bool, jailed;
    holder get_jail_timeout set_jail_timeout i64, timestamps.jail;
    holder get_online_jailed_time set_online_jailed_time i64, timestamps.online_jail;
    holder get_last_login _ i64, timestamps.login;
    holder get_last_logout set_last_logout i64, timestamps.logout;
    holder is_afk _ bool, afk;
    holder is_social_spy_enabled set_social_spy_enabled bool, social_spy;
    holder is_npc set_npc bool, npc;
    holder are_power_tools_enabled set_power_tools_enabled bool, power_tools_enabled;
    holder is_accepting_pay set_accepting_pay bool, accepting_pay;
    holder is_baltop_exclude_cache set_baltop_exempt_cache bool, baltop_exempt;
    state is_hidden _ bool, hidden;
    state is_leaving_hidden set_leaving_hidden bool, leaving_hidden;
    state get_last_online_activity set_last_online_activity i64, last_online_activity;
    state get_last_activity_time _ i64, last_activity;
    state is_inv_see set_inv_see bool, inv_see;
    state is_ender_see set_ender_see bool, ender_see;
    state is_recipe_see set_recipe_see bool, recipe_see;
    state is_ignore_msg set_ignore_msg bool, ignore_msg;
    state is_vanished _ bool, vanished;
    state is_fly_click_jump set_right_click_jump bool, right_click_jump;
    state is_freeze set_freeze bool, freeze;
    state get_flight_tick set_flight_tick i64, flight_tick;
    state get_afk_since _ i64, afk_since;
    state get_last_home_confirmation_timestamp _ i64, last_home_confirmation_timestamp;
    state get_last_vanish_time _ i64, last_vanish_time;
);

fields!(clone:
    /// `getBase`, while there's a player: a stub in Java for someone offline.
    state player _ Option<Player>, base;
    holder get_nickname set_nickname Option<String>, nickname;
    holder get_unlimited _ Vec<String>, unlimited;
    holder get_all_powertools _ Vec<(String, Vec<String>)>, powertools;
    holder get_jail set_jail Option<String>, jail;
    holder get_mail_messages set_mail_list Vec<MailMessage>, mail;
    holder get_ignored_players set_ignored_player_uuids Vec<String>, ignore;
    holder get_mute_reason set_mute_reason Option<String>, mute_reason;
    holder get_last_login_address _ String, ip_address;
    holder get_geo_location set_geo_location Option<String>, geolocation;
    holder get_last_account_name _ Option<String>, last_account_name;
    holder get_past_usernames _ Vec<String>, past_usernames;
    /// `top.getHolder()` of a live `/invsee`, by UUID. `None` for the nine-slot equipment chest.
    state get_inv_see_owner set_inv_see_owner Option<String>, inv_see_owner;
    state get_afk_position _ Option<Location>, afk_position;
    state get_afk_message _ Option<String>, afk_message;
    state get_confirming_clear_command set_confirming_clear_command Option<String>, confirming_clear_command;
    state get_last_home_confirmation set_last_home_confirmation Option<String>, last_home_confirmation;
    state get_sign_copy set_sign_copy [String; 4], sign_copy;
);

impl User {
    /// `new User(base, ess)`, or a stub's for someone offline.
    pub(crate) fn load(uuid: &str, name: Option<&str>, base: Option<Player>) -> Self {
        let file = user_file(uuid);
        migrate(&file, uuid, name);
        let mut config = Config::new(file);
        config.load();
        let holder = UserConfigHolder::read(config.root());
        let last_activity = now_ms();
        let online = base.is_some();
        let state = Transient {
            base,
            offline_name: name.map(str::to_owned),
            last_online_activity: if online { last_activity } else { 0 },
            last_activity,
            last_vanish_time: last_activity,
            flight_tick: -1,
            ..Default::default()
        };
        let user = Self(Arc::new(Inner { uuid: uuid.to_owned(), config: Mutex::new(config), holder: Mutex::new(holder), state: Mutex::new(state), teleport: Mutex::new(TeleportState::default()) }));
        user.init_money();
        if user.is_afk() {
            let place = user.get_location();
            user.state().afk_position = place;
        }
        user
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, Transient> {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub(crate) fn holder(&self) -> MutexGuard<'_, UserConfigHolder> {
        self.0.holder.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub(crate) fn teleport_state(&self) -> MutexGuard<'_, TeleportState> {
        self.0.teleport.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn config(&self) -> MutexGuard<'_, Config> {
        self.0.config.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `config.save()`, with `setRootHolder` as its save hook.
    pub fn save(&self) {
        let holder = self.holder().clone();
        let mut config = self.config();
        holder.write(config.root_mut());
        config.save();
    }
    fn blocking_save(&self) {
        let holder = self.holder().clone();
        let mut config = self.config();
        holder.write(config.root_mut());
        config.blocking_save();
    }
    fn change(&self, edit: impl FnOnce(&mut UserConfigHolder)) {
        edit(&mut self.holder());
        self.save();
    }
    /// `startTransaction`.
    pub fn start_transaction(&self) {
        self.config().start_transaction();
    }
    /// `stopTransaction`.
    pub fn stop_transaction(&self) {
        let holder = self.holder().clone();
        let mut config = self.config();
        holder.write(config.root_mut());
        config.stop_transaction(false);
    }
    /// `reset`.
    pub fn reset(&self) {
        self.blocking_save();
        let file = self.config().get_file().to_owned();
        if std::fs::remove_file(&file).is_err() {
            crate::warning(&format!("Unable to delete data file for {}", file.display()));
        }
        ess().users().invalidate(&self.0.uuid);
        if self.is_npc() {
            let name = self.get_name();
            let name = if settings().is_safe_usermap() { safe_string(&name) } else { name };
            ess().users().invalidate(&crate::utils::string::name_uuid_from_bytes(format!("NPC:{name}").as_bytes()));
        }
    }
    /// `cleanup`.
    pub fn cleanup(&self) {
        self.blocking_save();
        ess().users().remove_cache(&self.0.uuid);
    }
    /// `reloadConfig`.
    pub fn reload_config(&self) {
        let mut config = self.config();
        config.load();
        *self.holder() = UserConfigHolder::read(config.root());
        drop(config);
        self.init_money();
    }
    fn init_money(&self) {
        let (start, max, min) = (settings().get_starting_balance(), settings().get_max_money(), settings().get_min_money());
        let mut holder = self.holder();
        let mut result = if holder.npc { BigDecimal::zero() } else { start };
        if let Some(money) = &holder.money {
            result = money.clone();
        }
        if result > max {
            result = max;
        }
        if result < min {
            result = min;
        }
        holder.money = Some(result.clone());
        drop(holder);
        self.state().money = result;
    }

    // ---- PlayerExtension ----

    /// `update`.
    pub fn update(&self, base: Player) {
        self.state().base = Some(base);
    }
    /// `getBase().isOnline()`.
    pub fn is_online(&self) -> bool {
        self.player().is_some_and(|player| player.is_online())
    }
    /// `getBase() instanceof OfflinePlayerStub`.
    pub fn is_offline_stub(&self) -> bool {
        self.state().base.is_none()
    }
    pub fn get_name(&self) -> String {
        let state = self.state();
        match &state.base {
            Some(player) => player.get_name().to_owned(),
            None => state.offline_name.clone().unwrap_or_default(),
        }
    }
    /// Whether this record is already this player's.
    pub(crate) fn is_player(&self, base: &Player) -> bool {
        self.state().base.as_ref() == Some(base)
    }
    /// `getUUID`: dashed, as the file is named.
    pub fn get_uuid(&self) -> String {
        self.0.uuid.clone()
    }
    pub fn get_location(&self) -> Option<Location> {
        self.player().filter(Player::is_online).map(|player| player.get_location())
    }
    pub fn get_world(&self) -> Option<World> {
        self.player().filter(Player::is_online).map(|player| player.get_world())
    }
    /// `getSource`.
    pub fn get_source(&self) -> CommandSource {
        self.player().map_or_else(CommandSource::console, CommandSource::of_player)
    }

    // ---- Permissions ----

    /// `isAuthorized(String)`.
    pub fn is_authorized(&self, node: &str) -> bool {
        let Some(player) = self.player() else { return false };
        let result = crate::perm::has_permission(&player, node);
        if settings().is_debug() {
            crate::info(&format!("checking if {} has {node} - {result}", player.get_name()));
        }
        result
    }
    /// `isAuthorized(IEssentialsCommand, prefix)`.
    pub fn is_authorized_command(&self, command: &str, prefix: &str) -> bool {
        self.is_authorized(&format!("{prefix}{}", if command == "r" { "msg" } else { command }))
    }
    /// `isAuthorizedCached`.
    pub fn is_authorized_cached(&self, node: &str) -> bool {
        self.is_authorized(node)
    }
    /// `isPermissionSet`.
    pub fn is_permission_set(&self, node: &str) -> bool {
        self.player().is_some_and(|player| crate::perm::is_permission_set(&player, node))
    }
    /// `NumberUtil.parseStringToBDecimal`, in this player's language when `per-player-locale` is on.
    pub fn parse_amount(&self, text: &str) -> EssResult<BigDecimal> {
        if settings().is_per_player_locale() {
            let code = self.player().map(|player| player.get_locale());
            if let Some(locale) = self.get_player_locale(code.as_deref()) {
                return crate::utils::number::parse_string_to_big_decimal_in(text, locale.language());
            }
        }
        crate::utils::number::parse_string_to_big_decimal(text)
    }
    /// `isAuthorizedExact`: `None` is `TriState.UNSET`.
    pub fn is_authorized_exact(&self, node: &str) -> Option<bool> {
        self.player().and_then(|player| crate::perm::is_permission_set_exact(&player, node))
    }
    pub fn get_group(&self) -> String {
        self.player().map_or_else(|| "default".to_owned(), |player| crate::perm::get_group(&player))
    }
    pub fn in_group(&self, group: &str) -> bool {
        self.player().is_some_and(|player| crate::perm::in_group(&player, group))
    }
    pub fn can_build(&self) -> bool {
        self.player().is_some_and(|player| player.is_op() || crate::perm::can_build(&player))
    }

    // ---- Messages ----

    /// `sendMessage(String)`.
    pub fn send_message(&self, message: &str) {
        if !message.is_empty()
            && let Some(player) = self.player()
        {
            player.send_message(message);
        }
    }
    /// `sendComponent`.
    pub fn send_component(&self, component: &Component) {
        if let Some(player) = self.player() {
            player.send_component(component);
        }
    }
    /// `playerTl`.
    pub fn player_tl(&self, key: &str, args: &[Arg]) -> String {
        if settings().is_per_player_locale() {
            let code = self.player().map(|player| player.get_locale());
            let locale = match code {
                Some(code) => self.get_player_locale(Some(&code)),
                None => self.state().player_locale.clone(),
            };
            if let Some(locale) = locale {
                return tl_locale(&locale, key, args);
            }
        }
        tl_literal(key, args)
    }
    /// `tlComponent`.
    pub fn tl_component(&self, key: &str, args: &[Arg]) -> Component {
        adventure::deserialize_mini_message(&self.player_tl(key, args))
    }
    /// `sendTl`.
    pub fn send_tl(&self, key: &str, args: &[Arg]) {
        let translation = self.player_tl(key, args);
        if !translation.trim().is_empty() {
            self.send_component(&adventure::deserialize_mini_message(&translation));
        }
    }
    /// `getPlayerLocale`.
    pub fn get_player_locale(&self, code: Option<&str>) -> Option<Locale> {
        let mut state = self.state();
        match code {
            Some(code) if state.last_locale_string.as_deref() != Some(code) => {
                state.last_locale_string = Some(code.to_owned());
                state.player_locale = Some(i18n::get_locale(code));
                state.player_locale.clone()
            }
            _ => state.player_locale.clone(),
        }
    }

    // ---- Money ----

    /// `getMoney`: `_getMoney`, with no economy layer as there's no Vault here.
    pub fn get_money(&self) -> BigDecimal {
        if settings().is_eco_disabled() {
            if settings().is_debug() {
                crate::info("Internal economy functions disabled, aborting balance check.");
            }
            return BigDecimal::zero();
        }
        self.state().money.clone()
    }
    /// `UserData.setMoney`.
    fn store_money(&self, value: BigDecimal, throw_error: bool) -> EssResult {
        let (max, min) = (settings().get_max_money(), settings().get_min_money());
        let mut money = value;
        if money > max {
            fail_if!(throw_error, EssError::MaxMoney);
            money = max;
        }
        if money < min {
            money = min;
        }
        self.state().money = money.clone();
        self.holder().money = Some(money);
        self.stop_transaction();
        Ok(())
    }
    /// `setMoney(value, cause)`.
    pub fn set_money(&self, value: BigDecimal) -> EssResult {
        if settings().is_eco_disabled() {
            if settings().is_debug() {
                crate::info("Internal economy functions disabled, aborting balance change.");
            }
            return Ok(());
        }
        self.store_money(value.clone(), true)?;
        Trade::log("Update", "Set", "API", Some(&self.get_name()), Some(&Trade::of_money(value.clone())), None, None, None, Some(&value));
        Ok(())
    }
    /// `updateMoneyCache`: nothing to do with no layer selected.
    pub fn update_money_cache(&self, _value: &BigDecimal) {}
    /// `giveMoney`.
    pub fn give_money(&self, value: &BigDecimal, initiator: Option<&CommandSource>) -> EssResult {
        if value.signum() == 0 {
            return Ok(());
        }
        self.set_money(self.get_money().add(value))?;
        self.send_tl("addedToAccount", args![parsed(display_currency(value))]);
        if let Some(initiator) = initiator {
            initiator.send_tl("addedToOthersAccount", args![parsed(display_currency(value)), self.get_display_name(), parsed(display_currency(&self.get_money()))]);
        }
        Ok(())
    }
    /// `takeMoney`.
    pub fn take_money(&self, value: &BigDecimal, initiator: Option<&CommandSource>) {
        if value.signum() == 0 {
            return;
        }
        if self.set_money(self.get_money().subtract(value)).is_err() {
            crate::warning("Invalid call to takeMoney, total balance can't be more than the max-money limit.");
        }
        self.send_tl("takenFromAccount", args![parsed(display_currency(value))]);
        if let Some(initiator) = initiator {
            initiator.send_tl("takenFromOthersAccount", args![parsed(display_currency(value)), self.get_display_name(), parsed(display_currency(&self.get_money()))]);
        }
    }
    /// `payUser`.
    pub fn pay_user(&self, receiver: &User, value: &BigDecimal) -> EssResult {
        fail_if!(value.signum() <= 0, EssError::Message(self.player_tl("payMustBePositive", &[])));
        fail_if!(!self.can_afford(value), EssError::charge("notEnoughMoney", args![parsed(display_currency(value))]));
        self.set_money(self.get_money().subtract(value))?;
        receiver.set_money(receiver.get_money().add(value))?;
        self.send_tl("moneySentTo", args![parsed(display_currency(value)), receiver.get_display_name()]);
        receiver.send_tl("moneyRecievedFrom", args![parsed(display_currency(value)), self.get_display_name()]);
        Ok(())
    }
    /// `canAfford`.
    pub fn can_afford(&self, cost: &BigDecimal) -> bool {
        self.can_afford_checked(cost, true)
    }
    pub fn can_afford_checked(&self, cost: &BigDecimal, perm_check: bool) -> bool {
        if cost.signum() <= 0 {
            return true;
        }
        let remaining = self.get_money().subtract(cost);
        if !perm_check || self.is_authorized("essentials.eco.loan") {
            return remaining >= settings().get_min_money();
        }
        remaining.signum() >= 0
    }

    // ---- Homes ----

    fn get_home_name(&self, search: &str) -> String {
        if is_int(search)
            && let Ok(index) = search.parse::<usize>()
            && let Some(name) = index.checked_sub(1).and_then(|at| self.get_homes().get(at).cloned())
        {
            return name;
        }
        search.to_owned()
    }
    /// `getHome(String)`.
    pub fn get_home(&self, name: &str) -> Option<Location> {
        let search = self.get_home_name(name);
        self.holder().homes.iter_mut().find(|(home, _)| *home == search).and_then(|(_, place)| place.location())
    }
    /// `hasValidHomes`.
    pub fn has_valid_homes(&self) -> bool {
        self.holder().homes.iter_mut().any(|(_, place)| place.location().is_some())
    }
    /// `getHome(Location)`: the first in that world, or the first.
    pub fn get_home_near(&self, near: &Location) -> Option<Location> {
        let mut holder = self.holder();
        if holder.homes.is_empty() {
            return None;
        }
        for (_, place) in &mut holder.homes {
            if let Some(home) = place.location()
                && home.get_world() == near.get_world()
            {
                return Some(home);
            }
        }
        holder.homes[0].1.location()
    }
    /// `getHomes`.
    pub fn get_homes(&self) -> Vec<String> {
        self.holder().homes.iter().map(|(name, _)| name.clone()).collect()
    }
    /// `setHome`.
    pub fn set_home(&self, name: &str, location: &Location) {
        let name = safe_string(name);
        let place = LazyLocation::from_location(location);
        self.change(|holder| match holder.homes.iter_mut().find(|(home, _)| *home == name) {
            Some((_, slot)) => *slot = place,
            None => holder.homes.push((name, place)),
        });
    }
    /// `delHome`.
    pub fn del_home(&self, name: &str) -> EssResult {
        let mut search = self.get_home_name(name);
        if !self.has_home(&search) {
            search = safe_string(&search);
        }
        if self.has_home(&search) {
            self.change(|holder| holder.homes.retain(|(home, _)| *home != search));
            Ok(())
        } else {
            Err(EssError::tl("invalidHome", args![search]))
        }
    }
    /// `renameHome`.
    pub fn rename_home(&self, name: &str, new_name: &str) -> EssResult {
        let found = self.holder().homes.iter().position(|(home, _)| home == name);
        let at = found.ok_or_else(|| EssError::tl("invalidHome", args![name]))?;
        self.change(|holder| {
            let (_, place) = holder.homes.remove(at);
            holder.homes.push((safe_string(new_name), place));
        });
        Ok(())
    }
    pub fn has_home_any(&self) -> bool {
        !self.holder().homes.is_empty()
    }
    /// `hasHome(String)`.
    pub fn has_home(&self, name: &str) -> bool {
        self.holder().homes.iter().any(|(home, _)| home == name)
    }

    // ---- Nick, display name ----

    /// `getNick(withPrefix, withSuffix)`.
    pub fn get_nick(&self, with_prefix: bool, with_suffix: bool) -> String {
        let name = self.get_name();
        let mut prefix = String::new();
        let mut suffix = String::new();
        let nick = self.get_nickname().unwrap_or_default();
        let nickname = if settings().is_command_disabled("nick") || nick.is_empty() || nick == name {
            name.clone()
        } else if nick.eq_ignore_ascii_case(&name) {
            nick
        } else {
            suffix = "\u{a7}r".to_owned();
            if self.is_authorized("essentials.nick.hideprefix") { nick } else { format!("{}{nick}", replace_format(&settings().get_nickname_prefix())) }
        };
        if self.player().is_some_and(|player| player.is_op())
            && let Some(color) = settings().get_operator_color().filter(|color| !color.is_empty())
        {
            prefix.insert_str(0, &color);
            "\u{a7}r".clone_into(&mut suffix);
        }
        if settings().add_prefix_suffix() {
            if with_prefix || !settings().disable_prefix() {
                let text = self.player().map(|player| replace_format(&crate::perm::get_prefix(&player))).unwrap_or_default();
                prefix.insert_str(0, &text);
                "\u{a7}r".clone_into(&mut suffix);
            }
            if with_suffix || !settings().disable_suffix() {
                let text = self.player().map(|player| replace_format(&crate::perm::get_suffix(&player))).unwrap_or_default();
                suffix = format!("{text}\u{a7}r").replace("\u{a7}f\u{a7}r", "\u{a7}r").replace("\u{a7}r\u{a7}r", "\u{a7}r");
            }
        }
        let output = format!("{prefix}{nickname}{suffix}");
        output.strip_suffix('\u{a7}').map_or_else(|| output.clone(), str::to_owned)
    }
    /// `getNick()`.
    pub fn get_nick_full(&self) -> String {
        self.get_nick(true, true)
    }
    /// `setDisplayNick`.
    pub fn set_display_nick(&self) {
        let Some(player) = self.player().filter(Player::is_online) else { return };
        if settings().change_display_name() {
            player.set_display_name(&self.get_nick_full());
            if self.is_afk() {
                self.update_afk_list_name();
            } else if settings().change_player_list_name() {
                player.set_player_list_name(Some(&self.get_nick(settings().is_adding_prefix_in_playerlist(), settings().is_adding_suffix_in_playerlist())));
            }
        }
    }
    /// `getDisplayName`.
    pub fn get_display_name(&self) -> String {
        let Some(player) = self.player() else { return self.get_name() };
        if settings().hide_display_name_in_vanish() && self.is_hidden() {
            return player.get_name().to_owned();
        }
        player.display_name().to_legacy()
    }
    /// `getFormattedNickname`.
    pub fn get_formatted_nickname(&self) -> Option<String> {
        self.get_nickname().map(|nick| replace_format(&format!("{}{nick}", settings().get_nickname_prefix())))
    }

    // ---- Simple fields ----

    pub fn has_unlimited(&self, stack: &ItemStack) -> bool {
        self.holder().unlimited.contains(&stack.get_type().name())
    }
    pub fn set_unlimited(&self, stack: &ItemStack, state: bool) {
        let name = stack.get_type().name();
        let mut holder = self.holder();
        let had = holder.unlimited.contains(&name);
        if state && !had {
            holder.unlimited.push(name);
        } else if !state && had {
            holder.unlimited.retain(|known| *known != name);
        } else {
            return;
        }
        drop(holder);
        self.save();
    }
    pub fn clear_all_powertools(&self) {
        self.change(|holder| holder.powertools.clear());
    }
    /// `getPowertool(Material)`.
    pub fn get_powertool(&self, material: &Material) -> Option<Vec<String>> {
        let key = material.name().to_lowercase();
        self.holder().powertools.iter().find(|(item, _)| *item == key).map(|(_, commands)| commands.clone())
    }
    /// `setPowertool`.
    pub fn set_powertool(&self, stack: &ItemStack, commands: Option<Vec<String>>) {
        let key = stack.get_type().name().to_lowercase();
        self.change(|holder| {
            holder.powertools.retain(|(item, _)| *item != key);
            if let Some(commands) = commands.filter(|commands| !commands.is_empty()) {
                holder.powertools.push((key, commands));
            }
        });
    }
    pub fn has_power_tools(&self) -> bool {
        !self.holder().powertools.is_empty()
    }
    pub fn get_last_location(&self) -> Option<Location> {
        self.holder().last_location.as_mut().and_then(LazyLocation::location)
    }
    /// `setLastLocation(Location)`.
    pub fn set_last_location_at(&self, location: &Location) {
        self.change(|holder| holder.last_location = Some(LazyLocation::from_location(location)));
    }
    /// `setLastLocation()`.
    pub fn set_last_location(&self) {
        if let Some(location) = self.get_location() {
            self.set_last_location_at(&location);
        }
    }
    pub fn get_logout_location(&self) -> Option<Location> {
        self.holder().logout_location.as_mut().and_then(LazyLocation::location)
    }
    /// `setLogoutLocation()`.
    pub fn set_logout_location(&self) {
        if let Some(location) = self.get_location() {
            self.change(|holder| holder.logout_location = Some(LazyLocation::from_location(&location)));
        }
    }
    pub fn get_mail_amount(&self) -> usize {
        self.holder().mail.iter().filter(|mail| !mail.is_expired()).count()
    }
    pub fn get_unread_mail_amount(&self) -> usize {
        self.holder().mail.iter().filter(|mail| !mail.read && !mail.is_expired()).count()
    }
    /// `isIgnoredPlayer(IUser)`.
    pub fn is_ignored_player(&self, user: &User) -> bool {
        self.holder().ignore.contains(&user.get_uuid()) && !user.is_ignore_exempt()
    }
    pub fn set_ignored_player(&self, user: &User, set: bool) {
        let uuid = user.get_uuid();
        self.change(|holder| {
            if set {
                if !holder.ignore.contains(&uuid) {
                    holder.ignore.push(uuid);
                }
            } else {
                holder.ignore.retain(|known| *known != uuid);
            }
        });
    }
    pub fn is_ignore_exempt(&self) -> bool {
        self.is_authorized("essentials.chat.ignoreexempt")
    }
    /// `User.isGodModeEnabled`.
    pub fn is_god_mode_enabled(&self) -> bool {
        if self.is_god_mode_enabled_raw() && !self.get_world().is_some_and(|world| settings().get_no_god_worlds().contains(world.get_name())) {
            return true;
        }
        self.is_afk() && settings().get_freeze_afk_players()
    }
    pub fn has_mute_reason(&self) -> bool {
        self.holder().mute_reason.is_some()
    }
    pub fn toggle_jailed(&self) -> bool {
        let jailed = !self.is_jailed();
        self.set_jailed(jailed);
        jailed
    }
    /// `setLastLogin`, which also keeps the address.
    pub fn set_last_login(&self, time: i64) {
        let address = self.player().and_then(|player| player.get_address());
        self.change(|holder| {
            holder.timestamps.login = time;
            if let Some(address) = address {
                holder.ip_address = address;
            }
        });
    }
    /// `setLastAccountName`.
    pub fn set_last_account_name(&self, name: &str) {
        self.change(|holder| {
            if let Some(old) = holder.last_account_name.clone().filter(|old| old != name) {
                holder.past_usernames.insert(0, old);
            }
            holder.last_account_name = Some(name.to_owned());
        });
    }
    pub fn toggle_power_tools_enabled(&self) -> bool {
        let enabled = !self.are_power_tools_enabled();
        self.set_power_tools_enabled(enabled);
        enabled
    }
    fn kit_key(name: &str) -> String {
        name.replace(['.', '/'], "_").to_lowercase()
    }
    pub fn get_kit_timestamp(&self, name: &str) -> i64 {
        let key = Self::kit_key(name);
        self.holder().timestamps.kits.iter().find(|(kit, _)| *kit == key).map_or(0, |(_, time)| *time)
    }
    pub fn set_kit_timestamp(&self, name: &str, time: i64) {
        let key = Self::kit_key(name);
        self.change(|holder| match holder.timestamps.kits.iter_mut().find(|(kit, _)| *kit == key) {
            Some((_, slot)) => *slot = time,
            None => holder.timestamps.kits.push((key, time)),
        });
    }
    /// `getCommandCooldowns`: patterns with their expiry.
    pub fn get_command_cooldowns(&self) -> Vec<(JavaPattern, i64)> {
        self.holder().timestamps.command_cooldowns.iter().map(|cooldown| (cooldown.pattern.clone(), cooldown.value)).collect()
    }
    /// `getCommandCooldownExpiry`.
    pub fn get_command_cooldown_expiry(&self, label: &str) -> Option<i64> {
        self.holder().timestamps.command_cooldowns.iter().find(|cooldown| cooldown.pattern.matches(label)).map(|cooldown| cooldown.value)
    }
    /// `addCommandCooldown`.
    pub fn add_command_cooldown(&self, pattern: JavaPattern, expires_at: i64, save: bool) {
        self.holder().timestamps.command_cooldowns.push(CommandCooldown { pattern, value: expires_at });
        if save {
            self.save();
        }
    }
    /// `clearCommandCooldown`.
    pub fn clear_command_cooldown(&self, pattern: &JavaPattern) -> bool {
        let mut holder = self.holder();
        let before = holder.timestamps.command_cooldowns.len();
        holder.timestamps.command_cooldowns.retain(|cooldown| cooldown.pattern != *pattern);
        let changed = holder.timestamps.command_cooldowns.len() != before;
        drop(holder);
        if changed {
            self.save();
        }
        changed
    }
    pub fn is_prompting_pay_confirm(&self) -> bool {
        self.holder().confirm_pay.unwrap_or_else(|| settings().is_confirm_command_enabled_by_default("pay"))
    }
    pub fn set_prompting_pay_confirm(&self, prompt: bool) {
        self.change(|holder| holder.confirm_pay = Some(prompt));
    }
    pub fn is_prompting_clear_confirm(&self) -> bool {
        self.holder().confirm_clear.unwrap_or_else(|| settings().is_confirm_command_enabled_by_default("clearinventory"))
    }
    pub fn set_prompting_clear_confirm(&self, prompt: bool) {
        self.change(|holder| holder.confirm_clear = Some(prompt));
    }
    pub fn is_last_message_reply_recipient(&self) -> bool {
        self.holder().last_message_reply_recipient.unwrap_or_else(|| settings().is_last_message_reply_recipient())
    }
    pub fn set_last_message_reply_recipient(&self, enabled: bool) {
        self.change(|holder| holder.last_message_reply_recipient = Some(enabled));
    }
    /// `isBaltopExempt`.
    pub fn is_baltop_exempt(&self) -> bool {
        if self.is_online() {
            let exempt = self.is_authorized("essentials.balancetop.exclude");
            self.set_baltop_exempt_cache(exempt);
            return exempt;
        }
        self.is_baltop_exclude_cache()
    }
    pub fn add_past_username(&self, name: &str) {
        self.change(|holder| holder.past_usernames.insert(0, name.to_owned()));
    }
    pub fn is_shouting(&self) -> bool {
        let mut holder = self.holder();
        *holder.shouting.get_or_insert_with(|| settings().is_shout_default())
    }
    pub fn set_shouting(&self, shouting: bool) {
        self.change(|holder| holder.shouting = Some(shouting));
    }
    /// `setToggleShout`.
    pub fn set_toggle_shout(&self, shout: bool) {
        self.state().toggle_shout = Some(shout);
        if settings().is_persist_shout() {
            self.set_shouting(shout);
        }
    }
    /// `isToggleShout`.
    pub fn is_toggle_shout(&self) -> bool {
        if settings().is_persist_shout() {
            let shout = self.is_shouting();
            self.state().toggle_shout = Some(shout);
            return shout;
        }
        let shout = self.state().toggle_shout;
        shout.unwrap_or_else(|| {
            let shout = settings().is_shout_default();
            self.state().toggle_shout = Some(shout);
            shout
        })
    }
    pub fn get_config_uuid(&self) -> String {
        self.0.uuid.clone()
    }
    /// `setConfigProperty`: under `info`, for plugins, kept across saves.
    pub fn set_config_property(&self, node: &str, value: serde_yaml::Value) {
        self.config().set(&format!("info.{node}"), value);
        self.save();
    }
    /// `getConfigMap`.
    pub fn get_config_map(&self) -> Vec<(String, serde_yaml::Value)> {
        self.config().section_map("info")
    }

    // ---- Transient ----

    /// `setHidden`.
    pub fn set_hidden(&self, hidden: bool) {
        self.state().hidden = hidden;
        if hidden {
            self.set_last_logout(self.get_last_online_activity());
        }
    }
    /// `isHiddenFrom`.
    pub fn is_hidden_from(&self, player: &Player) -> bool {
        match self.player() {
            None => true,
            Some(base) => !player.can_see(&base),
        }
    }
    /// `isHidden(Player)`.
    pub fn is_hidden_to(&self, player: &Player) -> bool {
        self.is_hidden() || self.is_hidden_from(player)
    }
    pub fn set_afk_message(&self, message: Option<String>) {
        if self.is_afk() {
            self.state().afk_message = message;
        }
    }
    /// `getConfirmingPayments`, by the other user's uuid.
    pub fn get_confirming_payment(&self, other: &User) -> Option<BigDecimal> {
        self.state().confirming_payments.get(&other.get_uuid()).cloned()
    }
    pub fn set_confirming_payment(&self, other: &User, amount: Option<BigDecimal>) {
        let uuid = other.get_uuid();
        let mut state = self.state();
        match amount {
            Some(amount) => {
                state.confirming_payments.insert(uuid, amount);
            }
            None => {
                state.confirming_payments.remove(&uuid);
            }
        }
    }
    pub fn clear_confirming_payments(&self) {
        self.state().confirming_payments.clear();
    }
    pub fn set_last_home_confirmation_timestamp(&self) {
        self.state().last_home_confirmation_timestamp = now_ms();
    }
    /// `getSignCopy().set(index, line)`.
    pub fn set_sign_copy_line(&self, index: usize, line: String) {
        self.state().sign_copy[index] = line;
    }
    /// `dispose`.
    pub fn dispose(&self) {
        if !self.is_online() {
            let mut state = self.state();
            if let Some(player) = state.base.take() {
                state.offline_name = Some(player.get_name().to_owned());
            }
        }
        self.cleanup();
    }
    /// `getItemInHand`.
    pub fn get_item_in_hand(&self) -> Option<ItemStack> {
        self.player().and_then(|player| crate::inventories::get_item_in_hand(&player))
    }
    /// `getTargetBlock`.
    pub fn get_target_block(&self, max_distance: i32) -> Option<horizon_bukkit::Block> {
        let player = self.player()?;
        // `getTargetBlockExact`, or where the line of sight ends.
        player.get_target_block(max_distance).or_else(|| Some(crate::utils::location::line_of_sight(&player, max_distance, Material::is_air)))
    }
    /// `canSpawnItem`.
    pub fn can_spawn_item(&self, material: &Material) -> bool {
        if settings().permission_based_item_spawn() {
            let name = material.name().to_lowercase().replace('_', "");
            return self.is_authorized("essentials.itemspawn.item-all") || self.is_authorized(&format!("essentials.itemspawn.item-{name}"));
        }
        self.is_authorized("essentials.itemspawn.exempt") || !settings().item_spawn_blacklist().contains(material)
    }
    /// `healCooldown`.
    pub fn heal_cooldown(&self) -> EssResult {
        let now = now_ms();
        let last = self.get_last_heal_timestamp();
        if last > 0 {
            let cooldown = settings().get_heal_cooldown();
            let until = last + (cooldown as i32 as i64) * 1000 + ((cooldown * 1000.0) % 1000.0) as i32 as i64;
            fail_if!(until > now && !self.is_authorized("essentials.heal.cooldown.bypass"), EssError::tl("timeBeforeHeal", args![format_date_diff(until)]));
        }
        self.set_last_heal_timestamp(now);
        Ok(())
    }
    /// `getAsyncTeleport`.
    pub fn get_async_teleport(&self) -> AsyncTeleport {
        AsyncTeleport::new(self.clone())
    }

    // ---- Teleport requests ----

    /// `requestTeleport`.
    pub fn request_teleport(&self, player: &User, here: bool) {
        let name = player.get_name();
        let location = if here { player.get_location() } else { self.get_location() };
        let max = settings().get_tpa_max_requests() as usize;
        let mut state = self.state();
        let mut request = match state.tpa_queue.iter().position(|(key, _)| *key == name) {
            Some(at) => state.tpa_queue.remove(at).1,
            None => TpaRequest { name: name.clone(), uuid: player.get_uuid(), time: 0, location: None, here: false },
        };
        request.time = now_ms();
        request.here = here;
        request.location = location;
        if state.tpa_queue.len() >= max {
            state.tpa_queue.pop();
        }
        state.tpa_queue.push((name, request));
    }
    pub fn get_pending_tpa_keys(&self) -> Vec<String> {
        self.state().tpa_queue.iter().map(|(key, _)| key.clone()).collect()
    }
    pub fn has_pending_tpa_requests(&self, inform: bool, exclude_here: bool) -> bool {
        self.get_next_tpa_request(inform, false, exclude_here).is_some()
    }
    pub fn has_outstanding_tpa_request(&self, name: &str, here: bool) -> bool {
        self.get_outstanding_tpa_request(name, false).is_some_and(|request| request.here == here)
    }
    fn tell_timed_out(&self, request: &TpaRequest) {
        let name = ess().get_user_by_uuid(&request.uuid).map_or_else(|| request.name.clone(), |user| user.get_display_name());
        self.send_tl("requestTimedOutFrom", args![name]);
    }
    /// `getOutstandingTpaRequest`.
    pub fn get_outstanding_tpa_request(&self, name: &str, inform: bool) -> Option<TpaRequest> {
        let timeout = settings().get_tpa_accept_cancellation();
        let found = self.state().tpa_queue.iter().find(|(key, _)| key == name).map(|(_, request)| request.clone())?;
        if timeout < 1 || now_ms() - found.time <= timeout * 1000 {
            return Some(found);
        }
        self.remove_tpa_request(name);
        if inform {
            self.tell_timed_out(&found);
        }
        None
    }
    pub fn remove_tpa_request(&self, name: &str) -> Option<TpaRequest> {
        let mut state = self.state();
        let at = state.tpa_queue.iter().position(|(key, _)| key == name)?;
        Some(state.tpa_queue.remove(at).1)
    }
    /// `getNextTpaRequest`.
    pub fn get_next_tpa_request(&self, inform: bool, ignore_expirations: bool, exclude_here: bool) -> Option<TpaRequest> {
        let timeout = settings().get_tpa_accept_cancellation();
        let queue: Vec<(String, TpaRequest)> = self.state().tpa_queue.iter().rev().cloned().collect();
        let mut next = None;
        for (key, request) in queue {
            if timeout < 1 || now_ms() - request.time <= timeout * 1000 {
                if exclude_here && request.here {
                    continue;
                }
                if ignore_expirations {
                    return Some(request);
                }
                if next.is_none() {
                    next = Some(request);
                }
            } else {
                if inform {
                    self.tell_timed_out(&request);
                }
                self.remove_tpa_request(&key);
            }
        }
        next
    }

    // ---- AFK ----

    /// `setAfk`.
    pub fn set_afk(&self, set: bool, _cause: AfkCause) {
        crate::hooks::fire(crate::hooks::Hook::Afk { user: self, value: set });
        if let Some(player) = self.player() {
            player.set_sleeping_ignored(self.is_authorized("essentials.sleepingignored") || set && settings().sleep_ignores_afk_players());
        }
        let was = self.is_afk();
        if set && !was {
            let place = self.get_location();
            let mut state = self.state();
            state.afk_position = place;
            state.afk_since = now_ms();
        } else if !set && was {
            let mut state = self.state();
            state.afk_position = None;
            state.afk_message = None;
            state.afk_since = 0;
        }
        self.change(|holder| holder.afk = set);
        self.update_afk_list_name();
    }
    fn update_afk_list_name(&self) {
        if !settings().is_afk_list_name() {
            return;
        }
        let Some(player) = self.player() else { return };
        if self.is_afk() {
            let name = settings().get_afk_list_name().replace("{PLAYER}", &self.get_display_name()).replace("{USERNAME}", &self.get_name());
            player.set_player_list_name(Some(&name));
        } else {
            player.set_player_list_name(None);
            self.set_display_nick();
        }
    }
    /// `toggleAfk`.
    pub fn toggle_afk(&self, cause: AfkCause) -> bool {
        self.set_afk(!self.is_afk(), cause);
        self.is_afk()
    }
    /// `updateActivity`.
    pub fn update_activity(&self, broadcast: bool, cause: AfkCause) {
        if self.is_afk() {
            self.set_afk(false, cause);
            if broadcast && !self.is_hidden() && !self.is_afk() {
                self.set_display_nick();
                if settings().broadcast_afk_message() {
                    let me = self.clone();
                    ess().broadcast_tl(Some(self), Some(&move |user: &User| *user == me), false, "userIsNotAway", args![self.get_display_name()]);
                }
                self.send_tl("userIsNotAwaySelf", args![self.get_display_name()]);
            }
        }
        self.state().last_activity = now_ms();
    }
    /// `updateActivityOnMove`.
    pub fn update_activity_on_move(&self, broadcast: bool) {
        if settings().cancel_afk_on_move() {
            self.update_activity(broadcast, AfkCause::Move);
        }
    }
    /// `updateActivityOnInteract`.
    pub fn update_activity_on_interact(&self, broadcast: bool) {
        if settings().cancel_afk_on_interact() {
            self.update_activity(broadcast, AfkCause::Interact);
        }
    }
    /// `updateActivityOnChat`.
    pub fn update_activity_on_chat(&self, broadcast: bool) {
        if settings().cancel_afk_on_chat() {
            let user = self.clone();
            ess().schedule_sync_delayed_task(move || user.update_activity(broadcast, AfkCause::Chat));
        }
    }
    /// `checkActivity`.
    pub fn check_activity(&self) {
        let now = now_ms();
        let last = self.get_last_activity_time();
        if now - last <= 10_000 {
            return;
        }
        let timeout = settings().get_auto_afk_timeout();
        if timeout > 0 && last > 0 && last + timeout * 1000 < now && !self.is_authorized("essentials.kick.exempt") && !self.is_authorized("essentials.afk.kickexempt") {
            self.state().last_activity = 0;
            let kick_time = timeout as f64 / 60.0;
            if settings().get_afk_timeout_commands().is_empty() {
                if let Some(player) = self.player() {
                    player.kick_player(&adventure::mini_to_legacy(&self.player_tl("autoAfkKickReason", args![kick_time])));
                }
                for user in ess().get_online_users() {
                    if user.is_authorized("essentials.kick.notify") {
                        user.send_tl("playerKicked", args![crate::command_source::console_display_name(), self.get_name(), user.player_tl("autoAfkKickReason", args![kick_time])]);
                    }
                }
            } else {
                use horizon_bukkit::JavaString;
                for command in settings().get_afk_timeout_commands() {
                    if command.is_empty() {
                        continue;
                    }
                    horizon_bukkit::dispatch_command(&command.replace("{USERNAME}", &self.get_name()).replace("{KICKTIME}", &kick_time.java_string()));
                }
            }
        }
        let auto = settings().get_auto_afk();
        let last = self.get_last_activity_time();
        if !self.is_afk() && auto > 0 && last + auto * 1000 < now_ms() && self.is_authorized_cached("essentials.afk.auto") {
            self.set_afk(true, AfkCause::Activity);
            if self.is_afk() && !self.is_hidden() {
                self.set_display_nick();
                if settings().broadcast_afk_message() {
                    let me = self.clone();
                    ess().broadcast_tl(Some(self), Some(&move |user: &User| *user == me), false, "userIsAway", args![self.get_display_name()]);
                }
                self.send_tl("userIsAwaySelf", args![self.get_display_name()]);
            }
        }
    }

    // ---- Jail and mute timers ----

    /// `getFormattedJailTime`.
    pub fn get_formatted_jail_time(&self) -> String {
        format_date_diff(if self.get_online_jailed_time() > 0 { self.get_online_jail_expire_time() } else { self.get_jail_timeout() })
    }
    fn play_ticks(&self) -> i64 {
        self.player().map_or(0, |player| crate::ess::play_one_tick(&player))
    }
    fn get_online_jail_expire_time(&self) -> i64 {
        (self.get_online_jailed_time() - self.play_ticks()) * 50 + now_ms()
    }
    /// `checkJailTimeout`.
    pub fn check_jail_timeout(&self, current_time: i64) -> bool {
        if self.get_jail_timeout() <= 0 {
            return false;
        }
        if self.get_online_jailed_time() > 0 && self.get_online_jailed_time() > self.play_ticks() {
            return false;
        }
        if self.get_jail_timeout() < current_time && self.is_jailed() {
            self.set_jail_timeout(0);
            self.set_online_jailed_time(0);
            self.set_jailed(false);
            self.send_tl("haveBeenReleased", &[]);
            self.set_jail(None);
            match settings().get_teleport_when_free_policy() {
                TeleportWhenFreePolicy::Back => {
                    let future = crate::future::EssFuture::new();
                    let user = self.clone();
                    future.exceptionally(move |_| user.get_async_teleport().respawn(None, "PLUGIN", crate::future::EssFuture::new()));
                    self.get_async_teleport().back_now(future);
                }
                TeleportWhenFreePolicy::Spawn => self.get_async_teleport().respawn(None, "PLUGIN", crate::future::EssFuture::new()),
                TeleportWhenFreePolicy::Off => {}
            }
            return true;
        }
        false
    }
    /// `checkMuteTimeout`.
    pub fn check_mute_timeout(&self, current_time: i64) -> bool {
        if self.get_mute_timeout() > 0 && self.get_mute_timeout() < current_time && self.is_muted() {
            let reason = self.get_mute_reason();
            crate::hooks::fire(crate::hooks::Hook::Mute { affected: self, controller: None, value: false, timestamp: Some(self.get_mute_timeout()), reason: reason.as_deref() });
            self.set_mute_timeout(0);
            self.send_tl("canTalkAgain", &[]);
            self.set_muted(false);
            self.set_mute_reason(None);
            return true;
        }
        false
    }
    /// `enableInvulnerabilityAfterTeleport`.
    pub fn enable_invulnerability_after_teleport(&self) {
        let time = settings().get_teleport_invulnerability();
        if time > 0 {
            self.state().teleport_invulnerability_timestamp = now_ms() + time;
        }
    }
    /// `resetInvulnerabilityAfterTeleport`.
    pub fn reset_invulnerability_after_teleport(&self) {
        let mut state = self.state();
        if state.teleport_invulnerability_timestamp != 0 && state.teleport_invulnerability_timestamp < now_ms() {
            state.teleport_invulnerability_timestamp = 0;
        }
    }
    /// `hasInvulnerabilityAfterTeleport`.
    pub fn has_invulnerability_after_teleport(&self) -> bool {
        let state = self.state();
        state.teleport_invulnerability_timestamp != 0 && state.teleport_invulnerability_timestamp >= now_ms()
    }
    /// `canInteractVanished`.
    pub fn can_interact_vanished(&self) -> bool {
        self.is_authorized("essentials.vanish.interact")
    }
    /// `setVanished`.
    pub fn set_vanished(&self, set: bool) {
        self.state().vanished = set;
        let Some(player) = self.player() else { return };
        let name = self.get_name();
        if set {
            for user in ess().get_online_users() {
                if !user.is_authorized("essentials.vanish.see")
                    && let Some(viewer) = user.player()
                {
                    viewer.hide_player(&player);
                }
            }
            self.set_hidden(true);
            self.state().last_vanish_time = now_ms();
            ess().vanished_add(&name);
            if self.is_authorized("essentials.vanish.effect") {
                player.add_potion_effect("minecraft:invisibility", 1_000_000, 1);
            }
            if settings().sleep_ignores_vanished_players() {
                player.set_sleeping_ignored(true);
            }
        } else {
            for viewer in ess().get_online_players() {
                viewer.show_player(&player);
            }
            self.set_hidden(false);
            ess().vanished_remove(&name);
            if self.is_authorized("essentials.vanish.effect") {
                player.remove_potion_effect("minecraft:invisibility");
            }
            if settings().sleep_ignores_vanished_players() && !self.is_authorized("essentials.sleepingignored") {
                player.set_sleeping_ignored(false);
            }
        }
    }
    /// `checkSignThrottle`.
    pub fn check_sign_throttle(&self) -> bool {
        if self.is_sign_throttled() {
            return true;
        }
        self.update_throttle();
        false
    }
    /// `isSignThrottled`.
    pub fn is_sign_throttled(&self) -> bool {
        now_ms() < self.state().last_throttled_action + i64::from(1000 / settings().get_sign_use_per_second())
    }
    pub fn update_throttle(&self) {
        self.state().last_throttled_action = now_ms();
    }
    /// `notifyOfMail`.
    pub fn notify_of_mail(&self) {
        let unread = self.get_unread_mail_amount();
        if unread != 0 {
            let cooldown = i64::from(settings().get_notify_player_of_mail_cooldown()) * 1000;
            if now_ms() - self.state().last_notified_about_mails_ms >= cooldown {
                self.send_tl("youHaveNewMail", args![unread]);
                self.state().last_notified_about_mails_ms = now_ms();
            }
        }
    }
    /// `compareTo`: by display name, without colors, ignoring case.
    pub fn compare_to(&self, other: &User) -> std::cmp::Ordering {
        strip_format(&self.get_display_name()).to_lowercase().cmp(&strip_format(&other.get_display_name()).to_lowercase())
    }
}

/// `EssentialsUserConfiguration`: an old `<name>.yml`, or the offline-mode id's file, becomes this one.
fn migrate(file: &std::path::Path, uuid: &str, name: Option<&str>) {
    let Some(name) = name.filter(|_| !file.exists()) else { return };
    let folder = crate::data_folder().join("userdata");
    let _ = std::fs::create_dir_all(&folder);
    let legacy = folder.join(format!("{name}.yml"));
    if legacy.exists() {
        if let Err(error) = std::fs::rename(&legacy, file) {
            crate::warning(&format!("Failed to migrate user: {name}: {error}"));
            return;
        }
        let mut config = Config::new(file.to_owned());
        config.load();
        config.set("last-account-name", serde_yaml::Value::from(name));
        config.save();
        return;
    }
    if name != name.to_lowercase() {
        let alt = folder.join(format!("{}.yml", crate::utils::string::name_uuid_from_bytes(format!("OfflinePlayer:{}", name.to_lowercase()).as_bytes())));
        if alt.exists()
            && let Err(error) = std::fs::rename(&alt, file)
        {
            crate::warning(&format!("Failed to migrate user: {name}: {uuid} {error}"));
        }
    }
}
