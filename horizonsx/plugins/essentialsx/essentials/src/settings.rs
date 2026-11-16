//! `Settings`, every getter of `ISettings` over `config.yml`, under the same keys with the same defaults.
//! Java reads some on demand and caches the rest in `reloadConfig`: which is which doesn't show from
//! outside, except that a cached one waits for `/essentials reload`, so the split is kept.
use crate::prelude::*;
use crate::adventure::from_char_name;
use crate::config::{Config, entries, scalar_decimal, scalar_f64, scalar_string};
use crate::config_entities::JavaPattern;
use crate::textreader::SimpleTextInput;
use crate::utils::format as format_util;
use crate::utils::number::{self, DecimalFormat};
use crate::utils::string as string_util;
use serde_yaml::Value;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Mutex, RwLock};

/// `getPriority`.
fn priority_of(word: &str) -> Option<String> {
    let word = word.to_lowercase();
    let name = match word.as_str() {
        "none" => return None,
        "lowest" | "low" | "normal" | "high" | "highest" => word.to_uppercase(),
        _ => "NORMAL".to_owned(),
    };
    Some(name)
}

const DEFAULT_MAX_MONEY: &str = "10000000000000";
const DEFAULT_MIN_MONEY: &str = "-10000000000000";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeepInvPolicy {
    Keep,
    Delete,
    Drop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeleportWhenFreePolicy {
    Spawn,
    Back,
    Off,
}

/// What `reloadConfig` keeps ready.
#[derive(Clone)]
struct Cache {
    chat_radius: i32,
    chat_shout: char,
    chat_question: char,
    teleport_safety: bool,
    force_disable_teleport_safety: bool,
    disabled_commands: HashSet<String>,
    overridden_commands: Vec<String>,
    player_commands: Vec<String>,
    command_costs: Option<HashMap<String, BigDecimal>>,
    social_spy_commands: HashSet<String>,
    mute_commands: HashSet<String>,
    nickname_prefix: String,
    reset_nick_on_name_change: bool,
    operator_color: Option<String>,
    item_spawn_bl: Vec<Material>,
    enabled_signs: Vec<String>,
    signs_enabled: bool,
    warn_on_build_disallow: bool,
    config_debug: bool,
    economy_disabled: bool,
    max_money: BigDecimal,
    min_money: BigDecimal,
    economy_log: bool,
    economy_log_uuid: bool,
    economy_log_update: bool,
    change_display_name: bool,
    change_player_list_name: bool,
    prefix_suffix_configured: bool,
    add_prefix_suffix: bool,
    disable_prefix: bool,
    disable_suffix: bool,
    freeze_afk_players: bool,
    cancel_afk_on_move: bool,
    cancel_afk_on_interact: bool,
    sleep_ignores_afk_players: bool,
    afk_list_name: String,
    is_afk_list_name: bool,
    broadcast_afk_message: bool,
    vanishing_item_policy: KeepInvPolicy,
    binding_item_policy: KeepInvPolicy,
    no_god_worlds: HashSet<String>,
    register_back_in_listener: bool,
    disable_item_pickup_while_afk: bool,
    teleport_invulnerability_time: i64,
    teleport_invulnerability: bool,
    login_attack_delay: i64,
    sign_use_per_second: i32,
    mails_per_minute: i32,
    economy_lag_warning: i64,
    permissions_lag_warning: i64,
    allow_silent_join: bool,
    custom_join_message: String,
    custom_quit_message: String,
    custom_new_username_message: String,
    spawn_on_join_groups: Vec<String>,
    command_cooldowns: Option<Vec<(JavaPattern, i64)>>,
    npcs_in_balance_ranking: bool,
    currency_format: DecimalFormat,
    unprotected_signs: Vec<String>,
    default_enabled_confirm_commands: Vec<String>,
    teleport_when_free_policy: TeleportWhenFreePolicy,
    is_compass_towards_home_perm: bool,
    is_allow_world_in_broadcastworld: bool,
    item_db_type: String,
    allow_old_id_signs: bool,
    is_water_safe: bool,
    is_safe_usermap: bool,
    log_command_block_commands: bool,
    log_console_commands: bool,
    nick_blacklist: Vec<JavaPattern>,
    max_projectile_speed: f64,
    remove_effects_on_heal: bool,
    world_aliases: HashMap<String, String>,
    primary_color: String,
    secondary_color: String,
    multiplier_perms: Option<Vec<String>>,
    default_multiplier: BigDecimal,
    afk_timeout_commands: Vec<String>,
    currency_symbol: String,
}

pub struct Settings {
    config: Mutex<Config>,
    cache: RwLock<Option<Cache>>,
    /// `/essentials debug`, on top of `debug` in the file.
    debug: Mutex<bool>,
    essentials_chat_active: Mutex<bool>,
}

/// Getters read at each call, as one table: name, key, default.
macro_rules! live {
    (bool: $($name:ident $key:literal $default:literal,)*) => {
        impl Settings { $(pub fn $name(&self) -> bool { self.config().get_boolean($key, $default) })* }
    };
    (int: $($name:ident $key:literal $default:literal,)*) => {
        impl Settings { $(pub fn $name(&self) -> i32 { self.config().get_int($key, $default) })* }
    };
    (long: $($name:ident $key:literal $default:literal,)*) => {
        impl Settings { $(pub fn $name(&self) -> i64 { self.config().get_long($key, $default) })* }
    };
    (double: $($name:ident $key:literal $default:literal,)*) => {
        impl Settings { $(pub fn $name(&self) -> f64 { self.config().get_double($key, $default) })* }
    };
    (string: $($name:ident $key:literal $default:literal,)*) => {
        impl Settings { $(pub fn $name(&self) -> String { self.config().string_or($key, $default) })* }
    };
}

live!(bool:
    get_respawn_at_home "respawn-at-home" false,
    is_respawn_at_anchor "respawn-at-anchor" false,
    get_update_bed_at_daytime "update-bed-at-daytime" true,
    is_shout_default "chat.shout-default" false,
    is_persist_shout "chat.persist-shout" false,
    is_chat_question_enabled "chat.question-enabled" true,
    is_use_paper_chat_event "chat.paper-chat-events" true,
    is_always_teleport_safety "force-safe-teleport-location" false,
    is_teleport_passenger_dismount "teleport-passenger-dismount" true,
    is_force_passenger_teleport "force-passenger-teleportation" false,
    is_verbose_command_usages "verbose-command-usages" true,
    get_social_spy_listen_muted_players "socialspy-listen-muted-players" true,
    is_social_spy_messages "socialspy-messages" true,
    is_social_spy_display_names "socialspy-uses-displaynames" true,
    is_skipping_used_one_time_kits_from_kit_list "skip-used-one-time-kits-from-kit-list" false,
    show_non_ess_commands_in_help "non-ess-in-help" true,
    hide_permissionless_help "hide-permissionless-help" true,
    is_always_run_backup "backup.always-run" false,
    get_per_warp_permission "per-warp-permission" false,
    warn_on_smite "warn-on-smite" true,
    permission_based_item_spawn "permission-based-item-spawn" false,
    is_per_player_locale "per-player-locale" false,
    is_currency_symbol_suffixed "currency-symbol-suffix" false,
    real_names_on_list "real-names-on-list" false,
    remove_god_on_disconnect "remove-god-on-disconnect" false,
    change_tab_complete_name "change-tab-complete-name" false,
    use_bukkit_permissions "use-bukkit-permissions" false,
    cancel_afk_on_chat "cancel-afk-on-chat" true,
    cancel_afk_on_fish "cancel-afk-on-fish" true,
    sleep_ignores_vanished_players "sleep-ignores-vanished-player" true,
    are_death_messages_enabled "death-messages" true,
    get_repair_enchanted "repair-enchanted" true,
    allow_unsafe_enchantments "unsafe-enchantments" false,
    is_world_teleport_permissions "world-teleport-permissions" false,
    is_world_home_permissions "world-home-permissions" false,
    ignore_colors_in_max_length "ignore-colors-in-max-nick-length" false,
    hide_display_name_in_vanish "hide-displayname-in-vanish" false,
    is_custom_server_full_message "use-custom-server-full-message" true,
    is_custom_whitelist_message "use-custom-whitelist-message" true,
    is_notify_no_new_mail "notify-no-new-mail" true,
    is_drop_items_if_full "drop-items-if-full" false,
    is_last_message_reply_recipient "last-message-reply-recipient" false,
    is_reply_to_vanished "last-message-reply-vanished" true,
    is_pay_excludes_ignore_list "pay-excludes-ignore-list" false,
    is_milk_bucket_easter_egg_enabled "milk-bucket-easter-egg" true,
    is_send_fly_enable_on_join "send-fly-enable-on-join" true,
    is_world_time_permissions "world-time-permissions" false,
    is_teleport_to_center_location "teleport-to-center" true,
    is_kit_auto_equip "kit-auto-equip" false,
    is_pastebin_create_kit "pastebin-createkit" false,
    is_use_better_kits "use-nbt-serialization-in-createkit" false,
    is_allow_bulk_buy_sell "allow-bulk-buy-sell" false,
    is_allow_sell_named_items "allow-selling-named-items" false,
    is_adding_prefix_in_playerlist "add-prefix-in-playerlist" false,
    is_adding_suffix_in_playerlist "add-suffix-in-playerlist" false,
    is_direct_hat_allowed "allow-direct-hat" true,
    is_world_change_fly_reset_enabled "world-change-fly-reset" true,
    is_world_change_preserve_flying "world-change-preserve-flying" true,
    is_gamemode_change_preserve_flying "gamemode-change-preserve-flying" false,
    is_world_change_speed_reset_enabled "world-change-speed-reset" true,
    is_jail_online_time "jail-online-time" false,
    is_spawn_if_no_home "spawn-if-no-home" true,
    is_confirm_home_overwrite "confirm-home-overwrite" false,
    info_after_death "send-info-after-death" false,
    is_respawn_at_bed "respawn-at-home-bed" true,
    is_update_check_enabled "update-check" true,
    show_zero_baltop "show-zero-baltop" true,
);

live!(int:
    get_near_radius "near-radius" 200,
    get_oversized_stack_size "oversized-stacksize" 64,
    get_default_stack_size "default-stack-size" -1,
    get_spawn_mob_limit "spawnmob-limit" 10,
    get_protect_creeper_max_height "protect.creeper.max-height" -1,
    get_max_tree_command_range "tree-command-range-limit" 300,
    get_tpa_max_requests "tpa-max-requests" 5,
    get_max_nick_length "max-nick-length" 30,
    get_join_quit_message_player_count "hide-join-quit-messages-above" -1,
    get_notify_player_of_mail_cooldown "notify-player-of-mail-cooldown" 0,
    get_motd_delay "delay-motd" 0,
    get_baltop_entry_limit "baltop-entry-limit" -1,
    get_max_item_lore "max-itemlore-lines" 10,
);

live!(long:
    get_auto_afk "auto-afk" 300,
    get_tpa_accept_cancellation "tpa-accept-cancellation" 120,
    get_max_mute "max-mute-time" -1,
    get_max_tempban "max-tempban-time" -1,
    get_max_user_cache_value_expiry "max-user-cache-value-expiry" 600,
    get_last_message_reply_recipient_timeout "last-message-reply-recipient-timeout" 180,
    get_baltop_min_playtime "baltop-requirements.minimum-playtime" 0,
);

live!(double:
    get_teleport_delay "teleport-delay" 0.0,
    get_teleport_cooldown "teleport-cooldown" 0.0,
    get_heal_cooldown "heal-cooldown" 0.0,
);

live!(string:
    get_random_spawn_location "random-spawn-location" "none",
    get_random_respawn_location "random-respawn-location" "none",
    get_new_player_kit "newbies.kit" "",
    get_newbie_spawn "newbies.spawnpoint" "default",
    get_locale "locale" "",
    get_nick_regex "allowed-nicks-regex" "^[a-zA-Z_0-9\u{a7}]+$",
);

/// `getPriority`: `none` is a listener left out.
fn is_none(priority: &str) -> bool {
    priority == "none"
}

fn lower_set(values: Vec<String>) -> HashSet<String> {
    values.iter().map(|value| value.to_lowercase()).collect()
}

fn policy(text: &str) -> KeepInvPolicy {
    match text.to_uppercase().as_str() {
        "DELETE" => KeepInvPolicy::Delete,
        "DROP" => KeepInvPolicy::Drop,
        _ => KeepInvPolicy::Keep,
    }
}

/// Getters of what `reloadConfig` keeps ready, as one table: name, field, type.
macro_rules! cached {
    (copy: $($(#[$doc:meta])* $name:ident $field:ident $kind:ty,)*) => {
        impl Settings { $($(#[$doc])* pub fn $name(&self) -> $kind { self.cached(|cache| cache.$field) })* }
    };
    (clone: $($(#[$doc:meta])* $name:ident $field:ident $kind:ty,)*) => {
        impl Settings { $($(#[$doc])* pub fn $name(&self) -> $kind { self.cached(|cache| cache.$field.clone()) })* }
    };
}

cached!(copy:
    get_chat_radius chat_radius i32,
    get_chat_shout chat_shout char,
    get_chat_question chat_question char,
    is_teleport_safety_enabled teleport_safety bool,
    is_force_disable_teleport_safety force_disable_teleport_safety bool,
    is_reset_nick_on_name_change reset_nick_on_name_change bool,
    warn_on_build_disallow warn_on_build_disallow bool,
    is_eco_disabled economy_disabled bool,
    is_eco_log_enabled economy_log bool,
    is_eco_log_uuid_enabled economy_log_uuid bool,
    is_eco_log_update_enabled economy_log_update bool,
    change_display_name change_display_name bool,
    change_player_list_name change_player_list_name bool,
    disable_prefix disable_prefix bool,
    disable_suffix disable_suffix bool,
    get_freeze_afk_players freeze_afk_players bool,
    cancel_afk_on_move cancel_afk_on_move bool,
    cancel_afk_on_interact cancel_afk_on_interact bool,
    sleep_ignores_afk_players sleep_ignores_afk_players bool,
    is_afk_list_name is_afk_list_name bool,
    broadcast_afk_message broadcast_afk_message bool,
    get_vanishing_items_policy vanishing_item_policy KeepInvPolicy,
    get_binding_items_policy binding_item_policy KeepInvPolicy,
    register_back_in_listener register_back_in_listener bool,
    get_disable_item_pickup_while_afk disable_item_pickup_while_afk bool,
    get_teleport_invulnerability teleport_invulnerability_time i64,
    is_teleport_invulnerability teleport_invulnerability bool,
    get_login_attack_delay login_attack_delay i64,
    get_sign_use_per_second sign_use_per_second i32,
    get_mails_per_minute mails_per_minute i32,
    get_economy_lag_warning economy_lag_warning i64,
    get_permissions_lag_warning permissions_lag_warning i64,
    allow_silent_join_quit allow_silent_join bool,
    is_npcs_in_balance_ranking npcs_in_balance_ranking bool,
    get_teleport_when_free_policy teleport_when_free_policy TeleportWhenFreePolicy,
    is_compass_towards_home_perm is_compass_towards_home_perm bool,
    is_allow_world_in_broadcastworld is_allow_world_in_broadcastworld bool,
    allow_old_id_signs allow_old_id_signs bool,
    is_water_safe is_water_safe bool,
    is_safe_usermap is_safe_usermap bool,
    log_command_block_commands log_command_block_commands bool,
    log_console_commands log_console_commands bool,
    get_max_projectile_speed max_projectile_speed f64,
    is_removing_effects_on_heal remove_effects_on_heal bool,
);

cached!(clone:
    get_disabled_commands disabled_commands HashSet<String>,
    get_social_spy_commands social_spy_commands HashSet<String>,
    get_mute_commands mute_commands HashSet<String>,
    get_nickname_prefix nickname_prefix String,
    get_operator_color operator_color Option<String>,
    item_spawn_blacklist item_spawn_bl Vec<Material>,
    /// `enabledSigns`: as names, until the sign classes are ported.
    enabled_signs enabled_signs Vec<String>,
    get_currency_symbol currency_symbol String,
    get_max_money max_money BigDecimal,
    get_min_money min_money BigDecimal,
    get_afk_timeout_commands afk_timeout_commands Vec<String>,
    get_afk_list_name afk_list_name String,
    get_no_god_worlds no_god_worlds HashSet<String>,
    get_custom_join_message custom_join_message String,
    get_custom_quit_message custom_quit_message String,
    get_custom_new_username_message custom_new_username_message String,
    get_spawn_on_join_groups spawn_on_join_groups Vec<String>,
    get_currency_format currency_format DecimalFormat,
    get_unprotected_sign_names unprotected_signs Vec<String>,
    get_item_db_type item_db_type String,
    get_nick_blacklist nick_blacklist Vec<JavaPattern>,
    get_primary_color primary_color String,
    get_secondary_color secondary_color String,
);

impl Settings {
    pub fn new() -> Self {
        let config = Config::with_template(crate::data_folder().join("config.yml"), include_str!("../resources/config.yml"));
        Self { config: Mutex::new(config), cache: RwLock::new(None), debug: Mutex::new(false), essentials_chat_active: Mutex::new(false) }
    }
    fn config(&self) -> std::sync::MutexGuard<'_, Config> {
        self.config.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn cached<R>(&self, read: impl FnOnce(&Cache) -> R) -> R {
        let cache = self.cache.read().unwrap_or_else(|e| e.into_inner());
        read(cache.as_ref().expect("settings are read after reload_config"))
    }
    /// `reloadConfig`.
    pub fn reload_config(&self) {
        let cache = {
            let mut config = self.config();
            config.load();
            read_cache(&config)
        };
        crate::utils::location::set_is_water_safe(cache.is_water_safe);
        number::internal_set_pretty_format(cache.currency_format.clone());
        crate::adventure::set_colors(&cache.primary_color, &cache.secondary_color);
        *self.cache.write().unwrap_or_else(|e| e.into_inner()) = Some(cache);
    }
    /// `_lateLoadItemSpawnBlacklist`: the list needs the item database, loaded after these settings.
    pub fn late_load_item_spawn_blacklist(&self) {
        let list = self.get_item_spawn_blacklist();
        if let Some(cache) = self.cache.write().unwrap_or_else(|e| e.into_inner()).as_mut() {
            cache.item_spawn_bl = list;
        }
    }
    fn get_item_spawn_blacklist(&self) -> Vec<Material> {
        let mut list = Vec::new();
        let Some(database) = crate::ess::try_item_db() else { return list };
        for name in self.config().string_or("item-spawn-blacklist", "").split(',') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            match database.get(name) {
                Ok(stack) => list.push(stack.get_type().clone()),
                Err(_) => crate::severe(&crate::adventure::mini_to_legacy(&crate::i18n::tl_literal("unknownItemInList", crate::args![name, "item-spawn-blacklist"]))),
            }
        }
        list
    }
    pub fn get_config_file(&self) -> std::path::PathBuf {
        self.config().get_file().to_owned()
    }
    /// `getMultipleHomes`.
    pub fn get_multiple_homes(&self) -> Option<Vec<String>> {
        self.config().section_keys("sethome-multiple")
    }
    /// `getHomeLimit(User)`.
    pub fn get_home_limit(&self, user: &User) -> i32 {
        let mut limit = 1;
        if user.is_authorized("essentials.sethome.multiple") {
            limit = self.get_home_limit_of("default");
        }
        for set in self.get_multiple_homes().unwrap_or_default() {
            if user.is_authorized(&format!("essentials.sethome.multiple.{set}")) && limit < self.get_home_limit_of(&set) {
                limit = self.get_home_limit_of(&set);
            }
        }
        limit
    }
    /// `getHomeLimit(String)`.
    pub fn get_home_limit_of(&self, set: &str) -> i32 {
        let config = self.config();
        config.get_int(&format!("sethome-multiple.{set}"), config.get_int("sethome-multiple.default", 3))
    }
    /// `getChatFormat`: `chat_key` is `ChatType.key()`.
    pub fn get_chat_format(&self, group: Option<&str>, chat_key: &str) -> String {
        let config = self.config();
        let group_path = format!("chat.group-formats.{}", group.unwrap_or("Default"));
        let format = config.get_string(&format!("{group_path}.{chat_key}"), None).or_else(|| config.get_string(&group_path, None)).or_else(|| config.get_string(&format!("chat.format.{chat_key}"), None)).or_else(|| config.get_string("chat.format", None));
        let format = format.unwrap_or_else(|| "&7[{GROUP}]&r {DISPLAYNAME}&7:&r {MESSAGE}".to_owned());
        let mut format = format_util::replace_format(&format);
        for (word, mark) in [("{DISPLAYNAME}", "%1$s"), ("{MESSAGE}", "%2$s"), ("{GROUP}", "{0}"), ("{WORLD}", "{1}"), ("{WORLDNAME}", "{1}"), ("{SHORTWORLDNAME}", "{2}"), ("{TEAMPREFIX}", "{3}"), ("{TEAMSUFFIX}", "{4}"), ("{TEAMNAME}", "{5}"), ("{PREFIX}", "{6}"), ("{SUFFIX}", "{7}"), ("{USERNAME}", "{8}"), ("{NICKNAME}", "{9}")] {
            format = format.replace(word, mark);
        }
        format!("\u{a7}r{format}")
    }
    /// `getRespawnPriority`: the name `register_event_with` takes, `None` for no listener.
    pub fn get_respawn_priority(&self) -> Option<String> {
        priority_of(&self.config().string_or("respawn-listener-priority", "normal"))
    }
    /// `getSpawnJoinPriority`.
    pub fn get_spawn_join_priority(&self) -> Option<String> {
        priority_of(&self.config().string_or("spawn-join-listener-priority", "normal"))
    }
    pub fn get_starting_balance(&self) -> BigDecimal {
        self.config().get_big_decimal("starting-balance", BigDecimal::zero())
    }
    pub fn is_command_disabled(&self, label: &str) -> bool {
        self.cached(|cache| cache.disabled_commands.contains(label))
    }
    pub fn is_player_command(&self, label: &str) -> bool {
        self.cached(|cache| cache.player_commands.iter().any(|command| command.eq_ignore_ascii_case(label)))
    }
    pub fn is_command_overridden(&self, name: &str) -> bool {
        self.cached(|cache| cache.overridden_commands.iter().any(|command| command.eq_ignore_ascii_case(name))) || self.config().get_boolean(&format!("override-{}", name.to_lowercase()), false)
    }
    pub fn get_command_cost(&self, name: &str) -> BigDecimal {
        let name = name.replace(['.', '/'], "_");
        self.cached(|cache| cache.command_costs.as_ref().and_then(|costs| costs.get(&name).cloned())).unwrap_or_else(BigDecimal::zero)
    }
    /// `getKitSection`: kits still in config.yml, from before kits.yml.
    pub fn get_kit_section(&self) -> Option<Value> {
        self.config().get_section("kits").cloned()
    }
    pub fn are_signs_disabled(&self) -> bool {
        self.cached(|cache| !cache.signs_enabled)
    }
    pub fn get_backup_interval(&self) -> i64 {
        i64::from(self.config().get_int("backup.interval", 1440))
    }
    pub fn get_backup_command(&self) -> Option<String> {
        self.config().get_string("backup.command", None)
    }
    pub fn get_world_alias(&self, world: &str) -> String {
        self.cached(|cache| cache.world_aliases.get(&world.to_lowercase()).cloned()).unwrap_or_else(|| world.to_owned())
    }
    pub fn get_announce_new_players(&self) -> bool {
        !self.config().string_or("newbies.announce-format", "-").is_empty()
    }
    pub fn get_announce_new_player_format(&self) -> SimpleTextInput {
        SimpleTextInput::new(&format_util::replace_format(&self.config().string_or("newbies.announce-format", "&dWelcome {DISPLAYNAME} to the server!")))
    }
    /// `getListGroupConfig`: `(group, value as text)`, in file order.
    pub fn get_list_group_config(&self) -> Vec<(String, String)> {
        let config = self.config();
        if config.get_section("list").is_some_and(Value::is_mapping) {
            let values: Vec<(String, String)> = config.section_map("list").into_iter().map(|(key, value)| (key, scalar_string(&value).unwrap_or_default())).collect();
            if !values.is_empty() {
                return values;
            }
        }
        if config.get_boolean("sort-list-by-groups", false) { vec![("ListByGroup".to_owned(), "ListByGroup".to_owned())] } else { vec![("Players".to_owned(), "*".to_owned())] }
    }
    pub fn is_debug(&self) -> bool {
        *self.debug.lock().unwrap_or_else(|e| e.into_inner()) || self.cached(|cache| cache.config_debug)
    }
    pub fn set_debug(&self, debug: bool) {
        *self.debug.lock().unwrap_or_else(|e| e.into_inner()) = debug;
    }
    /// `isTradeInStacks(Material)`.
    pub fn is_trade_in_stacks(&self, material: &Material) -> bool {
        self.config().get_boolean(&format!("trade-in-stacks.{}", material.name().to_lowercase().replace('_', "")), false)
    }
    pub fn get_protect_prevent_spawn(&self, creature: &str) -> bool {
        self.config().get_boolean(&format!("protect.prevent.spawn.{creature}"), false)
    }
    pub fn get_protect_list_raw(&self, name: &str) -> Vec<String> {
        self.config().string_or(name, "").split(',').map(str::trim).filter(|item| !item.is_empty()).map(str::to_owned).collect()
    }
    pub fn get_protect_list(&self, name: &str) -> Vec<Material> {
        let mut list = Vec::new();
        for item in self.get_protect_list_raw(name) {
            let material = crate::ess::try_item_db().and_then(|database| database.get(&item).ok()).map(|stack| stack.get_type().clone());
            match material {
                Some(material) => list.push(material),
                None => crate::severe(&crate::adventure::mini_to_legacy(&crate::i18n::tl_literal("unknownItemInList", crate::args![item, name]))),
            }
        }
        list
    }
    pub fn get_protect_string(&self, name: &str) -> Option<String> {
        self.config().get_string(name, None)
    }
    pub fn get_protect_boolean(&self, name: &str, default: bool) -> bool {
        self.config().get_boolean(name, default)
    }
    pub fn set_essentials_chat_active(&self, active: bool) {
        *self.essentials_chat_active.lock().unwrap_or_else(|e| e.into_inner()) = active;
    }
    /// `getPluginManager().getPlugin("EssentialsChat") != null`, in one library.
    pub fn is_essentials_chat_active(&self) -> bool {
        *self.essentials_chat_active.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn add_prefix_suffix(&self) -> bool {
        let (configured, value) = self.cached(|cache| (cache.prefix_suffix_configured, cache.add_prefix_suffix));
        if configured { value } else { *self.essentials_chat_active.lock().unwrap_or_else(|e| e.into_inner()) }
    }
    pub fn get_auto_afk_timeout(&self) -> i64 {
        let config = self.config();
        config.get_long("auto-afk-timeout", config.get_long("auto-afk-kick", -1))
    }
    /// False for `none`: the listener isn't registered.
    pub fn has_respawn_priority(&self) -> bool {
        !is_none(&self.config().string_or("respawn-listener-priority", "normal").to_lowercase())
    }
    pub fn has_spawn_join_priority(&self) -> bool {
        !is_none(&self.config().string_or("spawn-join-listener-priority", "normal").to_lowercase())
    }
    pub fn get_max_fly_speed(&self) -> f64 {
        let speed = self.config().get_double("max-fly-speed", 0.8);
        if speed > 1.0 { 1.0 } else { speed.abs() }
    }
    pub fn get_max_walk_speed(&self) -> f64 {
        let speed = self.config().get_double("max-walk-speed", 0.8);
        if speed > 1.0 { 1.0 } else { speed.abs() }
    }
    pub fn is_custom_join_message(&self) -> bool {
        self.cached(|cache| cache.custom_join_message != "none")
    }
    pub fn is_custom_quit_message(&self) -> bool {
        self.cached(|cache| cache.custom_quit_message != "none")
    }
    pub fn is_custom_new_username_message(&self) -> bool {
        self.cached(|cache| cache.custom_new_username_message != "none")
    }
    pub fn has_join_quit_message_player_count(&self) -> bool {
        self.get_join_quit_message_player_count() >= 0
    }
    /// `getMaxUserCacheCount`: only a size for a cache, so memory's share of it doesn't matter here.
    pub fn get_max_user_cache_count(&self) -> i32 {
        self.config().get_int("max-user-cache-count", 4096)
    }
    /// `getMinimumPayAmount`: a bad value is Java's `NumberFormatException`.
    pub fn get_minimum_pay_amount(&self) -> Option<BigDecimal> {
        BigDecimal::parse(&self.config().string_or("minimum-pay-amount", "0.001"))
    }
    pub fn is_spawn_on_join(&self) -> bool {
        self.cached(|cache| !cache.spawn_on_join_groups.is_empty())
    }
    pub fn is_user_in_spawn_on_join_group(&self, user: &User) -> bool {
        self.get_spawn_on_join_groups().iter().any(|group| group == "*" || user.in_group(group))
    }
    pub fn is_command_cooldowns_enabled(&self) -> bool {
        self.cached(|cache| cache.command_cooldowns.is_some())
    }
    pub fn get_command_cooldown_ms(&self, label: &str) -> i64 {
        self.get_command_cooldown_entry(label).map_or(-1, |(_, cooldown)| cooldown)
    }
    pub fn get_command_cooldown_entry(&self, label: &str) -> Option<(JavaPattern, i64)> {
        self.cached(|cache| cache.command_cooldowns.as_ref().and_then(|all| all.iter().find(|(pattern, _)| pattern.matches(label)).cloned()))
    }
    pub fn is_command_cooldown_persistent(&self, _label: &str) -> bool {
        self.config().get_boolean("command-cooldown-persistence", true)
    }
    pub fn is_confirm_command_enabled_by_default(&self, name: &str) -> bool {
        self.cached(|cache| cache.default_enabled_confirm_commands.contains(&name.to_lowercase()))
    }
    /// `getMultiplier(User)`.
    pub fn get_multiplier(&self, user: &User) -> BigDecimal {
        let (default, perms) = self.cached(|cache| (cache.default_multiplier.clone(), cache.multiplier_perms.clone()));
        let Some(perms) = perms else { return default };
        let mut multiplier = default;
        for perm in perms {
            if user.is_authorized(&format!("essentials.sell.multiplier.{perm}")) {
                let value = self.config().get_big_decimal(&format!("sell-multipliers.{perm}"), BigDecimal::zero());
                if value > multiplier {
                    multiplier = value;
                }
            }
        }
        multiplier
    }
    pub fn get_baltop_min_balance(&self) -> BigDecimal {
        self.config().get_big_decimal("baltop-requirements.minimum-balance", BigDecimal::zero())
    }
}

fn operator_color(config: &Config) -> Option<String> {
    let name = match config.get_string("ops-name-color", None) {
        None => return Some(horizon_bukkit::chat_color::RED.to_owned()),
        Some(name) => name,
    };
    if name.eq_ignore_ascii_case("none") || name.is_empty() {
        return None;
    }
    if let Ok(color) = format_util::parse_hex_color(&name) {
        return Some(color);
    }
    let upper = name.to_uppercase();
    const NAMES: [(&str, char); 22] = [
        ("BLACK", '0'), ("DARK_BLUE", '1'), ("DARK_GREEN", '2'), ("DARK_AQUA", '3'), ("DARK_RED", '4'), ("DARK_PURPLE", '5'), ("GOLD", '6'), ("GRAY", '7'), ("DARK_GRAY", '8'), ("BLUE", '9'), ("GREEN", 'a'),
        ("AQUA", 'b'), ("RED", 'c'), ("LIGHT_PURPLE", 'd'), ("YELLOW", 'e'), ("WHITE", 'f'), ("MAGIC", 'k'), ("BOLD", 'l'), ("STRIKETHROUGH", 'm'), ("UNDERLINE", 'n'), ("ITALIC", 'o'), ("RESET", 'r'),
    ];
    if let Some((_, code)) = NAMES.iter().find(|(known, _)| *known == upper) {
        return Some(format!("\u{a7}{code}"));
    }
    // `ChatColor.getByChar(String)`.
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(code), None) if "0123456789abcdefklmnor".contains(code) => Some(format!("\u{a7}{code}")),
        _ => None,
    }
}

fn tag_color(color: &str) -> Option<String> {
    if color.len() == 7 && color.starts_with('#') && number::is_hexadecimal(&color[1..]) {
        return Some(color.to_owned());
    }
    let mut chars = color.chars();
    if let (Some(code), None) = (chars.next(), chars.next()) {
        return from_char_name(code).map(str::to_owned);
    }
    const NAMES: [&str; 16] = ["black", "dark_blue", "dark_green", "dark_aqua", "dark_red", "dark_purple", "gold", "gray", "dark_gray", "blue", "green", "aqua", "red", "light_purple", "yellow", "white"];
    let lower = color.to_lowercase();
    NAMES.contains(&lower.as_str()).then_some(lower)
}

fn command_costs(config: &Config, symbol: &str) -> Option<HashMap<String, BigDecimal>> {
    let section = config.section_map("command-costs");
    if section.is_empty() {
        return None;
    }
    let mut costs = HashMap::new();
    for (command, node) in section {
        if command.starts_with('/') {
            crate::warning(&format!("Invalid command cost. '{command}' should not start with '/'."));
        }
        match &node {
            Value::Number(number) => {
                let cost = number.as_i64().map_or_else(|| BigDecimal::from_f64(scalar_f64(&node)), BigDecimal::from_i64);
                costs.insert(command.to_lowercase(), cost);
            }
            Value::String(text) => {
                let cleaned: String = text.trim().replace('$', "").replace(symbol, "").chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                match cleaned.parse::<f64>() {
                    Ok(cost) => {
                        costs.insert(command.to_lowercase(), BigDecimal::from_f64(cost));
                    }
                    Err(_) => crate::warning(&format!("Invalid command cost for: {command}")),
                }
            }
            _ => crate::warning(&format!("Invalid command cost for: {command}")),
        }
    }
    Some(costs)
}

fn command_cooldowns(config: &Config) -> Option<Vec<(JavaPattern, i64)>> {
    let section = config.get_section("command-cooldowns")?;
    let mut result = Vec::new();
    for (entry, value) in entries(Some(section)) {
        let pattern = if let Some(regex) = entry.strip_prefix('^') {
            let pattern = JavaPattern::new(regex);
            if !pattern.is_valid() {
                crate::warning(&format!("Command cooldown error: bad pattern {regex}"));
            }
            pattern
        } else {
            let entry = entry.strip_prefix("\\^").map_or_else(|| entry.clone(), |rest| format!("^{rest}"));
            JavaPattern::new(&format!("{}( .*)?", entry.replace('*', ".*")))
        };
        let cooldown = match &value {
            Value::Number(_) => Some(scalar_f64(&value)),
            Value::String(text) => text.parse::<f64>().ok(),
            _ => None,
        };
        let Some(cooldown) = cooldown else {
            crate::warning(&format!("Command cooldown error: '{}' is not a valid cooldown", scalar_string(&value).unwrap_or_default()));
            continue;
        };
        if cooldown < 1.0 {
            crate::warning(&format!("Command cooldown with very short {cooldown} cooldown."));
        }
        // Java truncates before it multiplies.
        result.push((pattern, (cooldown as i64) * 1000));
    }
    Some(result)
}

fn spawn_on_join_groups(config: &Config) -> Vec<String> {
    if !config.has_property("spawn-on-join") {
        return Vec::new();
    }
    if config.is_list("spawn-on-join") {
        return config.get_string_list("spawn-on-join");
    }
    if config.is_boolean("spawn-on-join") {
        return if config.get_boolean("spawn-on-join", true) { vec!["*".to_owned()] } else { Vec::new() };
    }
    let value = config.string_or("spawn-on-join", "");
    if value.is_empty() { Vec::new() } else { vec![value] }
}

fn teleport_when_free(config: &Config) -> TeleportWhenFreePolicy {
    if config.has_property("teleport-back-when-freed-from-jail") {
        return if config.get_boolean("teleport-back-when-freed-from-jail", true) { TeleportWhenFreePolicy::Back } else { TeleportWhenFreePolicy::Off };
    }
    if config.has_property("teleport-when-freed") {
        return match config.string_or("teleport-when-freed", "back").replace("false", "off").to_uppercase().as_str() {
            "SPAWN" => TeleportWhenFreePolicy::Spawn,
            "OFF" => TeleportWhenFreePolicy::Off,
            "BACK" => TeleportWhenFreePolicy::Back,
            value => {
                // Java throws here, which stops the plugin: the safe reading of a bad value.
                crate::severe(&format!("Invalid value \"{value}\" for config option \"teleport-when-freed\"!"));
                TeleportWhenFreePolicy::Back
            }
        };
    }
    TeleportWhenFreePolicy::Back
}

fn sign_names(config: &Config, key: &str, enabled: Option<&mut bool>) -> Vec<String> {
    let mut any = false;
    let mut names = Vec::new();
    for name in config.get_string_list(key) {
        let name = name.trim().to_uppercase();
        if name.is_empty() {
            continue;
        }
        if name == "COLOR" || name == "COLOUR" {
            any = enabled.is_some();
            continue;
        }
        any = true;
        names.push(name);
    }
    if let Some(flag) = enabled {
        *flag = any;
    }
    names
}

fn read_cache(config: &Config) -> Cache {
    let mut disabled_commands = lower_set(config.get_string_list("disabled-commands"));
    for key in config.get_keys() {
        if let Some(name) = key.strip_prefix("disable-") {
            disabled_commands.insert(name.to_lowercase());
        }
    }
    let mut signs_enabled = false;
    let enabled_signs = sign_names(config, "enabledSigns", Some(&mut signs_enabled));
    let currency_symbol = {
        let value = config.string_or("currency-symbol", "$").trim().to_owned();
        if value.chars().count() > 1 || value.chars().all(|c| c.is_ascii_digit()) && !value.is_empty() { "$".to_owned() } else { value }
    };
    let social_spy_commands = if config.is_list("socialspy-commands") {
        lower_set(config.get_string_list("socialspy-commands"))
    } else {
        ["msg", "r", "mail", "m", "whisper", "emsg", "t", "tell", "er", "reply", "ereply", "email", "action", "describe", "eme", "eaction", "edescribe", "etell", "ewhisper", "pm"].iter().map(|name| (*name).to_owned()).collect()
    };
    let min_money = {
        let min = config.get_big_decimal("min-money", BigDecimal::parse(DEFAULT_MIN_MONEY).expect("a number"));
        if min.signum() > 0 { min.negate() } else { min }
    };
    let afk_list_name = format_util::replace_format(&config.string_or("afk-list-name", "none"));
    let currency_format = {
        let pattern = config.string_or("currency-format", "#,##0.00");
        let locale = config.get_string("currency-symbol-format-locale", None);
        DecimalFormat::with_locale(&pattern, locale.as_deref())
    };
    let color = |key: &str, default: &str, fallback: &str| tag_color(&config.string_or(key, default)).unwrap_or_else(|| fallback.to_owned());
    let nick_blacklist = config
        .get_string_list("nick-blacklist")
        .iter()
        .map(|entry| {
            let pattern = JavaPattern::new(entry);
            if !pattern.is_valid() {
                crate::warning(&format!("Invalid nickname blacklist regex: {entry}"));
            }
            pattern
        })
        .filter(JavaPattern::is_valid)
        .collect();
    let world_aliases = config.section_map("chat.world-aliases").into_iter().map(|(key, value)| (key.to_lowercase(), format_util::replace_format(&scalar_string(&value).unwrap_or_default()))).collect();
    Cache {
        chat_radius: config.get_int("chat.radius", config.get_int("chat-radius", 0)),
        chat_shout: config.string_or("chat.shout", "!").chars().next().unwrap_or('!'),
        chat_question: config.string_or("chat.question", "?").chars().next().unwrap_or('?'),
        teleport_safety: config.get_boolean("teleport-safety", true),
        force_disable_teleport_safety: config.get_boolean("force-disable-teleport-safety", false),
        disabled_commands,
        overridden_commands: config.get_string_list("overridden-commands"),
        player_commands: config.get_string_list("player-commands"),
        command_costs: command_costs(config, &currency_symbol),
        social_spy_commands,
        mute_commands: if config.is_list("mute-commands") { lower_set(config.get_string_list("mute-commands")) } else { HashSet::new() },
        nickname_prefix: config.string_or("nickname-prefix", "~"),
        reset_nick_on_name_change: config.get_boolean("reset-nick-on-name-change", false),
        operator_color: operator_color(config),
        item_spawn_bl: Vec::new(),
        enabled_signs,
        signs_enabled,
        warn_on_build_disallow: config.get_boolean("protect.disable.warn-on-build-disallow", false),
        config_debug: config.get_boolean("debug", false),
        economy_disabled: config.get_boolean("disable-eco", false),
        max_money: config.get_big_decimal("max-money", BigDecimal::parse(DEFAULT_MAX_MONEY).expect("a number")),
        min_money,
        economy_log: config.get_boolean("economy-log-enabled", false),
        economy_log_uuid: config.get_boolean("economy-log-uuids", false),
        economy_log_update: config.get_boolean("economy-log-update-enabled", false),
        change_display_name: config.get_boolean("change-displayname", true),
        change_player_list_name: config.get_boolean("change-playerlist", false),
        prefix_suffix_configured: config.has_property("add-prefix-suffix"),
        add_prefix_suffix: config.get_boolean("add-prefix-suffix", false),
        disable_prefix: config.get_boolean("disablePrefix", false),
        disable_suffix: config.get_boolean("disableSuffix", false),
        freeze_afk_players: config.get_boolean("freeze-afk-players", false),
        cancel_afk_on_move: config.get_boolean("cancel-afk-on-move", true),
        cancel_afk_on_interact: config.get_boolean("cancel-afk-on-interact", true),
        sleep_ignores_afk_players: config.get_boolean("sleep-ignores-afk-players", true),
        is_afk_list_name: !afk_list_name.eq_ignore_ascii_case("none"),
        afk_list_name,
        broadcast_afk_message: config.get_boolean("broadcast-afk-message", true),
        vanishing_item_policy: policy(&config.string_or("vanishing-items-policy", "keep")),
        binding_item_policy: policy(&config.string_or("binding-items-policy", "keep")),
        no_god_worlds: config.get_string_list("no-god-in-worlds").into_iter().collect(),
        register_back_in_listener: config.get_boolean("register-back-in-listener", false),
        disable_item_pickup_while_afk: config.get_boolean("disable-item-pickup-while-afk", false),
        teleport_invulnerability_time: config.get_long("teleport-invulnerability", 0) * 1000,
        teleport_invulnerability: config.get_long("teleport-invulnerability", 0) > 0,
        login_attack_delay: config.get_long("login-attack-delay", 0) * 1000,
        sign_use_per_second: {
            let per_second = config.get_int("sign-use-per-second", 4);
            if per_second > 0 { per_second } else { 1 }
        },
        mails_per_minute: config.get_int("mails-per-minute", 1000),
        economy_lag_warning: (config.get_double("economy-lag-warning", 25.0) * 1_000_000.0) as i64,
        permissions_lag_warning: (config.get_double("permissions-lag-warning", 25.0) * 1_000_000.0) as i64,
        allow_silent_join: config.get_boolean("allow-silent-join-quit", false),
        custom_join_message: format_util::replace_format(&config.string_or("custom-join-message", "none")),
        custom_quit_message: format_util::replace_format(&config.string_or("custom-quit-message", "none")),
        custom_new_username_message: format_util::replace_format(&config.string_or("custom-new-username-message", "none")),
        spawn_on_join_groups: spawn_on_join_groups(config),
        command_cooldowns: command_cooldowns(config),
        npcs_in_balance_ranking: config.get_boolean("npcs-in-balance-ranking", false),
        currency_format,
        unprotected_signs: sign_names(config, "unprotected-sign-names", None),
        default_enabled_confirm_commands: config.get_string_list("default-enabled-confirm-commands").iter().map(|name| name.to_lowercase()).collect(),
        teleport_when_free_policy: teleport_when_free(config),
        is_compass_towards_home_perm: config.get_boolean("compass-towards-home-perm", false),
        is_allow_world_in_broadcastworld: config.get_boolean("allow-world-in-broadcastworld", false),
        item_db_type: config.string_or("item-db-type", "auto"),
        allow_old_id_signs: config.get_boolean("allow-old-id-signs", false),
        is_water_safe: config.get_boolean("is-water-safe", false),
        is_safe_usermap: config.get_boolean("safe-usermap-names", true),
        log_command_block_commands: config.get_boolean("log-command-block-commands", true),
        log_console_commands: config.get_boolean("log-console-commands", true),
        nick_blacklist,
        max_projectile_speed: config.get_double("max-projectile-speed", 8.0),
        remove_effects_on_heal: config.get_boolean("remove-effects-on-heal", true),
        world_aliases,
        primary_color: color("message-colors.primary", "#ffaa00", "gold"),
        secondary_color: color("message-colors.secondary", "#ff5555", "red"),
        multiplier_perms: config.section_keys("sell-multipliers"),
        default_multiplier: config.get_big_decimal("sell-multipliers.default", BigDecimal::one()),
        afk_timeout_commands: config.get_string_list("afk-timeout-commands"),
        currency_symbol,
    }
}

/// A list of material names, as `Set<Material>` is kept in user data.
pub fn material_names(names: &BTreeSet<String>) -> Vec<Material> {
    names.iter().map(|name| Material::of(name)).collect()
}

/// `scalar_decimal` for those holding a raw node of this file.
pub fn decimal_of(value: &Value) -> Option<BigDecimal> {
    scalar_decimal(value)
}

/// `StringUtil.safeString`, here for `Settings`' callers.
pub fn safe_name(name: &str) -> String {
    string_util::safe_string(name)
}
