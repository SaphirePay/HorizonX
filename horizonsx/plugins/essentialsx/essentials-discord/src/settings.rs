//! `DiscordSettings`. PlaceholderAPI is left out, as there's no such plugin here, so the formats take no
//! player and `{...}` placeholders of its own stay as text: see GAPS.md.
use essentials::prelude::*;
use crate::client::Presence;
use crate::host::{self, Host, Level};
use crate::message_util::sanitize_discord_markdown;
use essentials::config::{Config, scalar_string};
use essentials::hooks::ChatType;
use essentials::i18n::{current_locale, message_format};
use regex::Regex;
use serde_yaml::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, RwLock, RwLockReadGuard};

/// `MessageFormat`, made from a pattern and used as often as wanted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Format(String);

impl Format {
    /// `MessageUtil.formatMessage`.
    pub fn format(&self, args: &[Arg]) -> String {
        message_format(&current_locale(), &self.0, args)
    }
}

static NAMED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([^0-9]+)\}").expect("regex"));

/// `generateMessageFormat`.
fn generate(content: Option<String>, default: &str, format: bool, arguments: &[&str]) -> Format {
    let content = content.unwrap_or_else(|| default.to_owned());
    let mut content = if format { replace_format(&content) } else { strip_format(&content) }.replace('\'', "''");
    for (index, argument) in arguments.iter().enumerate() {
        content = content.replace(&format!("{{{argument}}}"), &format!("{{{index}}}"));
        content = content.replace(&format!("{{{}}}", argument.to_uppercase()), &format!("{{{index}}}"));
    }
    Format(NAMED.replace_all(&content, "'{$1}'").into_owned())
}

/// `Level.toLevel(name, INFO)`, as an `intLevel`.
fn log_level(name: Option<&str>) -> i32 {
    match name.unwrap_or_default().trim().to_uppercase().as_str() {
        "OFF" => 0,
        "FATAL" => 100,
        "ERROR" => 200,
        "WARN" => 300,
        "DEBUG" => 500,
        "TRACE" => 600,
        "ALL" => i32::MAX,
        _ => 400,
    }
}

/// `Activity.ActivityType.valueOf`: a name it doesn't know plays.
fn activity_type(name: &str) -> u8 {
    match name {
        "STREAMING" => 1,
        "LISTENING" => 2,
        "WATCHING" => 3,
        "CUSTOM_STATUS" => 4,
        "COMPETING" => 5,
        _ => 0,
    }
}

/// The cached half of `reloadConfig`.
#[derive(Default)]
struct Cache {
    name_to_channel_id: HashMap<String, i64>,
    channel_id_to_names: HashMap<i64, Vec<String>>,
    presence: Option<Presence>,
    discord_filter: Vec<Regex>,
    role_aliases: HashMap<String, String>,
    console_format: Format,
    console_log_level: i32,
    console_filter: Vec<Regex>,
    discord_to_mc_format: Format,
    temp_mute_format: Format,
    temp_mute_reason_format: Format,
    perm_mute_format: Format,
    perm_mute_reason_format: Format,
    unmute_format: Format,
    kick_format: Format,
    pm_to_discord_format: Format,
}

pub struct DiscordSettings {
    config: RwLock<Config>,
    cache: RwLock<Cache>,
    host: Arc<dyn Host>,
}

macro_rules! cached_format {
    ($($name:ident),* $(,)?) => {
        $(pub fn $name(&self) -> Format {
            self.cached().$name.clone()
        })*
    };
}

macro_rules! player_format {
    ($($name:ident $node:literal $default:literal [$($argument:literal),*];)*) => {
        $(pub fn $name(&self) -> Format {
            generate(self.format_string($node), $default, false, &[$($argument),*])
        })*
    };
}

impl DiscordSettings {
    pub fn new(file: PathBuf, template: &'static str, host: Arc<dyn Host>) -> Self {
        Self { config: RwLock::new(Config::with_template(file, template)), cache: RwLock::new(Cache::default()), host }
    }
    fn config(&self) -> RwLockReadGuard<'_, Config> {
        self.config.read().unwrap_or_else(|e| e.into_inner())
    }
    fn cached(&self) -> RwLockReadGuard<'_, Cache> {
        self.cache.read().unwrap_or_else(|e| e.into_inner())
    }
    fn warning(&self, text: &str) {
        host::log(&self.host, Level::Warning, text);
    }
    fn string(&self, path: &str, default: &str) -> String {
        self.config().string_or(path, default)
    }
    fn boolean(&self, path: &str, default: bool) -> bool {
        self.config().get_boolean(path, default)
    }
    pub fn get_bot_token(&self) -> String {
        self.string("token", "")
    }
    pub fn get_http_proxy_server(&self) -> String {
        self.string("http-proxy-server", "")
    }
    pub fn get_guild_id(&self) -> u64 {
        self.config().get_long("guild", 0) as u64
    }
    pub fn get_discord_url(&self) -> String {
        self.string("discord-url", "https://discord.gg/invite-code")
    }
    pub fn get_primary_channel_id(&self) -> u64 {
        self.config().get_long("channels.primary", 0) as u64
    }
    /// `getChannelId`.
    pub fn get_channel_id(&self, key: &str) -> u64 {
        key.parse::<i64>().ok().or_else(|| self.cached().name_to_channel_id.get(key).copied()).unwrap_or(0) as u64
    }
    pub fn get_channel_names(&self) -> Vec<String> {
        self.cached().name_to_channel_id.keys().cloned().collect()
    }
    pub fn get_keys_from_channel_id(&self, channel: u64) -> Option<Vec<String>> {
        self.cached().channel_id_to_names.get(&(channel as i64)).cloned()
    }
    pub fn get_message_channel(&self, key: &str) -> String {
        self.string(&format!("message-types.{key}"), "none")
    }
    pub fn is_show_discord_attachments(&self) -> bool {
        self.boolean("show-discord-attachments", true)
    }
    pub fn get_discord_roles_blacklist(&self) -> Vec<String> {
        self.config().get_string_list("discord-role-blacklist")
    }
    pub fn get_invert_discord_role_blacklist(&self) -> bool {
        self.boolean("invert-discord-role-blacklist", false)
    }
    /// `getRoleAlias`.
    pub fn get_role_alias(&self, id: u64, name: &str) -> String {
        let cache = self.cached();
        cache.role_aliases.get(&id.to_string()).or_else(|| cache.role_aliases.get(name)).cloned().unwrap_or_else(|| name.to_owned())
    }
    pub fn get_permitted_formatting_roles(&self) -> Vec<String> {
        self.config().get_string_list("permit-formatting-roles")
    }
    /// `getStatus` and `getStatusActivity`.
    pub fn get_presence(&self) -> Presence {
        self.cached().presence.clone().unwrap_or(Presence { status: "online".to_owned(), activity: None })
    }
    pub fn is_always_receive_primary(&self) -> bool {
        self.boolean("always-receive-primary", false)
    }
    pub fn is_use_essentials_events(&self) -> bool {
        self.boolean("use-essentials-events", false)
    }
    pub fn get_chat_discord_max_length(&self) -> usize {
        self.config().get_int("chat.discord-max-length", 2000).max(0) as usize
    }
    pub fn is_chat_filter_newlines(&self) -> bool {
        self.boolean("chat.filter-newlines", true)
    }
    pub fn get_discord_filters(&self) -> Vec<Regex> {
        self.cached().discord_filter.clone()
    }
    pub fn is_show_webhook_messages(&self) -> bool {
        self.boolean("chat.show-webhook-messages", false)
    }
    pub fn is_show_bot_messages(&self) -> bool {
        self.boolean("chat.show-bot-messages", false)
    }
    pub fn is_show_all_chat(&self) -> bool {
        self.boolean("chat.show-all-chat", false)
    }
    pub fn get_relay_to_console_list(&self) -> Vec<String> {
        self.config().get_string_list("chat.relay-to-console")
    }
    pub fn get_console_channel_def(&self) -> String {
        self.string("console.channel", "none")
    }
    pub fn get_console_webhook_name(&self) -> String {
        self.string("console.webhook-name", "EssentialsX Console Relay")
    }
    pub fn is_console_command_relay(&self) -> bool {
        self.boolean("console.command-relay", false)
    }
    pub fn is_console_bot_command_relay(&self) -> bool {
        self.boolean("console.bot-command-relay", false)
    }
    /// `getConsoleLogLevel().intLevel()`.
    pub fn get_console_log_level(&self) -> i32 {
        self.cached().console_log_level
    }
    pub fn get_console_filters(&self) -> Vec<Regex> {
        self.cached().console_filter.clone()
    }
    pub fn is_show_avatar(&self) -> bool {
        self.boolean("show-avatar", false)
    }
    pub fn is_show_name(&self) -> bool {
        self.boolean("show-name", false)
    }
    pub fn is_show_display_name(&self) -> bool {
        self.boolean("show-displayname", false)
    }
    pub fn is_custom_bot_name(&self) -> bool {
        if self.is_show_name() || self.is_show_display_name() {
            return true;
        }
        self.format_string("mc-to-discord-name-format").is_some_and(|format| !format.is_empty() && format != "{botname}")
    }
    pub fn get_avatar_url(&self) -> String {
        self.string("avatar-url", "https://crafthead.net/helm/{uuid}")
    }
    pub fn is_vanish_fake_join_leave(&self) -> bool {
        self.boolean("vanish-fake-join-leave", true)
    }
    pub fn is_vanish_hide_messages(&self) -> bool {
        self.boolean("vanish-hide-messages", true)
    }
    pub fn is_command_enabled(&self, command: &str) -> bool {
        self.boolean(&format!("commands.{command}.enabled"), true)
    }
    pub fn is_command_ephemeral(&self, command: &str) -> bool {
        self.boolean(&format!("commands.{command}.hide-command"), true)
    }
    /// `getCommandSnowflakes`.
    pub fn get_command_snowflakes(&self, command: &str) -> Option<Vec<String>> {
        let path = format!("commands.{command}.allowed-roles");
        let config = self.config();
        config.is_list(&path).then(|| config.get_string_list(&path))
    }
    pub fn get_command_admin_snowflakes(&self, command: &str) -> Vec<String> {
        self.config().get_string_list(&format!("commands.{command}.admin-roles"))
    }
    cached_format!(console_format, discord_to_mc_format, temp_mute_format, temp_mute_reason_format, perm_mute_format, perm_mute_reason_format, unmute_format, kick_format, pm_to_discord_format);
    /// `getMcToDiscordFormat`.
    pub fn mc_to_discord_format(&self, chat_type: ChatType) -> Format {
        let (key, default) = match chat_type {
            ChatType::Local => ("mc-to-discord-local", "**[Local]** {displayname}: {message}"),
            ChatType::Question => ("mc-to-discord-question", "**[Question]** {displayname}: {message}"),
            ChatType::Shout => ("mc-to-discord-shout", "**[Shout]** {displayname}: {message}"),
            ChatType::Unknown => ("mc-to-discord", "{displayname}: {message}"),
        };
        generate(self.format_string(key), default, false, &["username", "displayname", "message", "world", "prefix", "suffix"])
    }
    /// `getLegacyNameFormat`.
    fn get_legacy_name_format(&self) -> Option<String> {
        if self.is_show_display_name() {
            Some("{displayname}".to_owned())
        } else if self.is_show_name() {
            Some("{username}".to_owned())
        } else {
            None
        }
    }
    /// `getMcToDiscordNameFormat`.
    pub fn mc_to_discord_name_format(&self) -> Format {
        let format = self.format_string("mc-to-discord-name-format").or_else(|| self.get_legacy_name_format());
        generate(format, "{botname}", false, &["username", "displayname", "world", "prefix", "suffix", "botname"])
    }
    player_format! {
        join_format "join" ":arrow_right: {displayname} has joined!" ["username", "displayname", "joinmessage", "online", "unique"];
        first_join_format "first-join" ":arrow_right: :first_place: {displayname} has joined the server for the first time!" ["username", "displayname", "joinmessage", "online", "unique"];
        quit_format "quit" ":arrow_left: {displayname} has left!" ["username", "displayname", "quitmessage", "online", "unique"];
        death_format "death" ":skull: {deathmessage}" ["username", "displayname", "deathmessage"];
        afk_format "afk" ":person_walking: {displayname} is now AFK!" ["username", "displayname"];
        un_afk_format "un-afk" ":keyboard: {displayname} is no longer AFK!" ["username", "displayname"];
        advancement_format "advancement" ":medal: {displayname} has completed the advancement **{advancement}**!" ["username", "displayname", "advancement"];
        action_format "action" ":person_biking: {displayname} *{action}*" ["username", "displayname", "action"];
    }
    /// `getStartMessage`: `seconds` since the server began.
    pub fn get_start_message(&self, seconds: f32) -> String {
        generate(self.format_string("server-start"), ":white_check_mark: The server has started in {starttimeseconds} seconds!", false, &["starttimeseconds"]).format(&[Arg::from(format!("{seconds:.2}"))])
    }
    pub fn get_stop_message(&self) -> String {
        self.string("messages.server-stop", ":octagonal_sign: The server has stopped!")
    }
    /// `getFormatString`.
    fn format_string(&self, node: &str) -> Option<String> {
        let path = node.strip_prefix('.').map_or_else(|| format!("messages.{node}"), str::to_owned);
        self.config().get_string(&path, None)
    }
    fn patterns(&self, path: &str) -> Vec<Regex> {
        let is_list = self.config().is_list(path);
        let lines = if is_list { self.config().get_string_list(path) } else { self.config().get_string(path, None).into_iter().collect() };
        let mut patterns = Vec::new();
        for line in lines.iter().filter(|line| !line.trim().is_empty()) {
            match Regex::new(line) {
                Ok(pattern) => patterns.push(pattern),
                // Java names `console.log-level` for the console filter too.
                Err(error) => self.warning(&format!("Invalid pattern for \"{}\": {error}", if path == "chat.discord-filter" { path } else { "console.log-level" })),
            }
        }
        patterns
    }
    /// `reloadConfig`, past the check on `isInvalidStartup`, which is the caller's. `onReload` is too.
    pub fn reload_config(&self) {
        self.config.write().unwrap_or_else(|e| e.into_inner()).load();
        let mut cache = Cache::default();
        for (name, value) in self.config().section_map("channels") {
            // `instanceof Long`: an id, and not the zeros or a short number the file ships with.
            if let Value::Number(number) = &value
                && let Some(id) = number.as_i64().filter(|id| i32::try_from(*id).is_err())
            {
                cache.channel_id_to_names.entry(id).or_default().push(name.clone());
                cache.name_to_channel_id.insert(name, id);
            }
        }
        let status = self.string("presence.status", "online").to_lowercase();
        let status = if matches!(status.as_str(), "online" | "idle" | "dnd" | "invisible" | "offline") { status } else { "online".to_owned() };
        let activity = self.string("presence.activity", "default").trim().to_uppercase().replace("CUSTOM_STATUS", "DEFAULT");
        let activity = (activity != "NONE").then(|| (activity_type(&activity), self.string("presence.message", "Minecraft")));
        cache.presence = Some(Presence { status, activity });
        cache.discord_filter = self.patterns("chat.discord-filter");
        for (name, value) in self.config().section_map("discord-roles-aliases") {
            if let Some(alias) = scalar_string(&value) {
                cache.role_aliases.insert(name, replace_format(&alias));
            }
        }
        cache.console_log_level = log_level(self.config().get_string("console.log-level", None).as_deref());
        cache.console_filter = self.patterns("console.console-filter");
        cache.console_format = generate(self.format_string(".console.format"), "[{timestamp} {level}] {message}", false, &["timestamp", "level", "message"]);
        cache.discord_to_mc_format = generate(self.format_string("discord-to-mc"), "&6[#{channel}] &3{fullname}&7: &f{message}", true, &["channel", "username", "discriminator", "fullname", "nickname", "color", "message", "role"]);
        cache.unmute_format = generate(self.format_string("unmute"), "{displayname} unmuted.", false, &["username", "displayname"]);
        cache.temp_mute_format = generate(self.format_string("temporary-mute"), "{controllerdisplayname} has muted player {displayname} for {time}.", false, &["username", "displayname", "controllername", "controllerdisplayname", "time"]);
        cache.perm_mute_format = generate(self.format_string("permanent-mute"), "{controllerdisplayname} permanently muted {displayname}.", false, &["username", "displayname", "controllername", "controllerdisplayname"]);
        cache.temp_mute_reason_format = generate(self.format_string("temporary-mute-reason"), "{controllerdisplayname} has muted player {displayname} for {time}. Reason: {reason}.", false, &["username", "displayname", "controllername", "controllerdisplayname", "time", "reason"]);
        cache.perm_mute_reason_format = generate(self.format_string("permanent-mute-reason"), "{controllerdisplayname} has muted player {displayname}. Reason: {reason}.", false, &["username", "displayname", "controllername", "controllerdisplayname", "reason"]);
        cache.kick_format = generate(self.format_string("kick"), "{displayname} was kicked with reason: {reason}", false, &["username", "displayname", "reason"]);
        cache.pm_to_discord_format = generate(self.format_string("private-chat"), "[SocialSpy] {sender-username} -> {receiver-username}: {message}", false, &["sender-username", "sender-displayname", "receiver-username", "receiver-displayname", "message"]);
        *self.cache.write().unwrap_or_else(|e| e.into_inner()) = cache;
    }
    /// The warning `reloadConfig` gives when `isInvalidStartup`.
    pub fn warn_reload_invalid(&self) {
        self.warning(&mini_to_legacy(&tl_literal("discordReloadInvalid", &[])));
    }
}

/// `sanitizeDiscordMarkdown`, for the many format arguments that want it.
pub fn clean(text: &str) -> Arg {
    Arg::from(sanitize_discord_markdown(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(format: &Format, args: &[&str]) -> String {
        format.format(&args.iter().map(|arg| Arg::from(*arg)).collect::<Vec<_>>())
    }

    #[test]
    fn names_become_numbers() {
        let format = generate(Some("{displayname}: {message}".to_owned()), "", false, &["username", "displayname", "message"]);
        assert_eq!(line(&format, &["a", "b", "c"]), "b: c");
    }

    #[test]
    fn upper_case_names_work_too() {
        let format = generate(Some("{USERNAME}!".to_owned()), "", false, &["username"]);
        assert_eq!(line(&format, &["a"]), "a!");
    }

    #[test]
    fn unknown_placeholders_stay_as_text() {
        let format = generate(Some("{nope} {username}".to_owned()), "", false, &["username"]);
        assert_eq!(line(&format, &["a"]), "{nope} a");
    }

    #[test]
    fn apostrophes_survive() {
        let format = generate(Some("it's {username}".to_owned()), "", false, &["username"]);
        assert_eq!(line(&format, &["a"]), "it's a");
    }

    #[test]
    fn a_missing_format_uses_the_default() {
        let format = generate(None, "{username} left", false, &["username"]);
        assert_eq!(line(&format, &["a"]), "a left");
    }

    #[test]
    fn colours_are_stripped_or_kept_as_asked() {
        assert_eq!(line(&generate(Some("&6{username}".to_owned()), "", true, &["username"]), &["a"]), "\u{a7}6a");
        assert_eq!(line(&generate(Some("&6{username}".to_owned()), "", false, &["username"]), &["a"]), "&6a", "`stripFormat` takes off what's a colour already");
        assert_eq!(line(&generate(Some("\u{a7}6{username}".to_owned()), "", false, &["username"]), &["a"]), "a");
    }

    #[test]
    fn log_levels_are_log4j_s() {
        assert_eq!(log_level(Some("warn")), 300);
        assert_eq!(log_level(None), 400);
        assert_eq!(log_level(Some("nonsense")), 400);
    }

    #[test]
    fn activities_default_to_playing() {
        assert_eq!(activity_type("WATCHING"), 3);
        assert_eq!(activity_type("DEFAULT"), 0);
    }
}
