//! `JDADiscordService`, the half of `DiscordUtil` that works on it, and the Discord side of
//! `DiscordListener` and `DiscordCommandDispatcher`. None of this touches the game itself: what's for
//! the game goes through `Host`, on the main thread.
use essentials::prelude::*;
use crate::api::{DiscordMessageEvent, InteractionChannel, InteractionMember, InteractionRole, MessageType, default_types};
use crate::client::model::{self, Channel, Interaction, MANAGE_ROLES, MANAGE_WEBHOOKS, MESSAGE_EMBED_LINKS, MESSAGE_SEND, Member, Mentions, Message, NICKNAME_MANAGE, Role, VIEW_CHANNEL, Webhook};
use crate::client::pending::Pending;
use crate::client::rest::RestError;
use crate::client::secret::Secret;
use crate::client::{ClientOptions, Discord, EventSink, LoginError};
use crate::console::ConsoleInjector;
use crate::emoji::parse_to_aliases;
use crate::host::{self, Feedback, Host, Level, Relay};
use crate::interactions::{InteractionController, has_roles};
use crate::settings::{DiscordSettings, clean};
use crate::webhook::{DEFAULT_QUEUE_CAPACITY, Sender, WebhookDispatcher, WebhookMessage};
use essentials::utils::number::is_long;
use essentials::utils::string::abbreviate;
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, RwLock, Weak};

pub const ADVANCED_RELAY_NAME: &str = "EssX Advanced Relay";
pub const CONSOLE_RELAY_NAME: &str = "EssX Console Relay";

/// `WebhookClientBuilder.WEBHOOK_PATTERN`.
static WEBHOOK_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:https?://)?(?:\w+\.)?discord(?:app)?\.com/api(?:/v\d+)?/webhooks/(\d+)/([\w-]+)(?:/(?:\w+)?)?$").expect("regex"));

/// What the service needs of a message, in the channel Java got by `getChannel`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TextChannel {
    id: u64,
    name: String,
    can_talk: bool,
}

/// The sink the client delivers to: the service's listeners.
struct Listeners(Weak<JdaDiscordService>);

impl EventSink for Listeners {
    fn on_message(&self, message: &Message) {
        if let Some(service) = self.0.upgrade() {
            service.on_message_received(message);
            service.on_console_message(message);
        }
    }
    fn on_interaction(&self, interaction: Interaction) {
        if let Some(service) = self.0.upgrade() {
            service.interaction_controller.on_slash_command(interaction);
        }
    }
    fn on_fatal(&self, reason: &str) {
        if let Some(service) = self.0.upgrade() {
            host::log(&service.host, Level::Severe, &tl_literal("discordErrorLogin", &[Arg::from(reason)]));
        }
    }
}

type MessageListener = fn(&mut DiscordMessageEvent);

pub struct JdaDiscordService {
    host: Arc<dyn Host>,
    settings: Arc<DiscordSettings>,
    /// The REST and gateway addresses, for a test to change.
    configure: Mutex<Box<dyn Fn(&mut ClientOptions) + Send + Sync>>,
    discord: RwLock<Option<Discord>>,
    guild_id: AtomicU64,
    primary_channel: AtomicU64,
    console_webhook: Mutex<Option<Arc<WebhookDispatcher>>>,
    last_console_id: Mutex<Option<u64>>,
    registered_types: Mutex<BTreeMap<String, MessageType>>,
    type_to_channel_id: Mutex<HashMap<String, u64>>,
    channel_id_to_webhook: Mutex<HashMap<u64, Arc<WebhookDispatcher>>>,
    injector: Mutex<Option<ConsoleInjector>>,
    /// `commandDispatcher`: the channel it reads, while there is one.
    command_channel: Mutex<Option<u64>>,
    interaction_controller: InteractionController,
    invalid_startup: AtomicBool,
    active_webhooks: Mutex<Vec<u64>>,
    /// Those who hear `DiscordMessageEvent`: DiscordLink. The send itself comes last, at `MONITOR`.
    message_listeners: Mutex<Vec<MessageListener>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// `getHighestRole`.
fn highest_role<'a>(settings: &DiscordSettings, roles: &'a [Role], fail: impl Fn(&Role) -> bool) -> Option<&'a Role> {
    let blacklist = settings.get_discord_roles_blacklist();
    let invert = settings.get_invert_discord_role_blacklist();
    roles.iter().find(|role| {
        let blacklisted = blacklist.contains(&role.name) || blacklist.contains(&role.id.to_string());
        blacklisted == invert && !fail(role)
    })
}

/// `getRoleFormat`.
pub fn role_format(settings: &DiscordSettings, roles: &[Role]) -> String {
    highest_role(settings, roles, |_| false).map_or_else(String::new, |role| settings.get_role_alias(role.id, &role.name))
}

/// `getRoleColorFormat`.
pub fn role_color_format(settings: &DiscordSettings, roles: &[Role]) -> String {
    match highest_role(settings, roles, |role| role.color_raw() == model::DEFAULT_COLOR_RAW) {
        Some(role) => replace_format(&format!("&#{:06x}", role.color & 0xff_ffff)),
        None => String::new(),
    }
}

impl JdaDiscordService {
    pub fn new(settings: Arc<DiscordSettings>, host: Arc<dyn Host>) -> Arc<Self> {
        let service = Arc::new(Self {
            host,
            settings,
            configure: Mutex::new(Box::new(|_| {})),
            discord: RwLock::new(None),
            guild_id: AtomicU64::new(0),
            primary_channel: AtomicU64::new(0),
            console_webhook: Mutex::new(None),
            last_console_id: Mutex::new(None),
            registered_types: Mutex::new(BTreeMap::new()),
            type_to_channel_id: Mutex::new(HashMap::new()),
            channel_id_to_webhook: Mutex::new(HashMap::new()),
            injector: Mutex::new(None),
            command_channel: Mutex::new(None),
            interaction_controller: InteractionController::default(),
            invalid_startup: AtomicBool::new(false),
            active_webhooks: Mutex::new(Vec::new()),
            message_listeners: Mutex::new(Vec::new()),
        });
        service.interaction_controller.attach(&service);
        for message_type in default_types::values() {
            lock(&service.registered_types).insert(message_type.get_key().to_owned(), message_type);
        }
        service
    }
    /// Changes the client's options before `startup`. Tests point it at their fake server with this.
    pub fn set_client_options(&self, change: impl Fn(&mut ClientOptions) + Send + Sync + 'static) {
        *lock(&self.configure) = Box::new(change);
    }
    pub fn host(&self) -> &Arc<dyn Host> {
        &self.host
    }
    pub fn settings(&self) -> &Arc<DiscordSettings> {
        &self.settings
    }
    pub fn discord(&self) -> Option<Discord> {
        self.discord.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn guild_id(&self) -> u64 {
        self.guild_id.load(Ordering::SeqCst)
    }
    /// `getInteractionController`.
    pub fn interaction_controller(&self) -> &InteractionController {
        &self.interaction_controller
    }
    pub fn is_invalid_startup(&self) -> bool {
        self.invalid_startup.load(Ordering::SeqCst)
    }
    /// Live: startup got all the way through, and it hasn't shut down.
    pub fn is_live(&self) -> bool {
        !self.is_invalid_startup() && self.discord().is_some()
    }
    pub fn is_debug(&self) -> bool {
        self.host.is_debug()
    }
    fn log(&self, level: Level, text: &str) {
        host::log(&self.host, level, text);
    }
    /// `Plugin.registerEvents` for `DiscordMessageEvent`, which is cancellable.
    pub fn add_message_listener(&self, listener: MessageListener) {
        lock(&self.message_listeners).push(listener);
    }
    fn text_channel(&self, id: u64) -> Option<TextChannel> {
        self.discord()?.with_guild(self.guild_id(), |guild| guild.text_channel(id).map(|channel| TextChannel { id: channel.id, name: channel.name.clone(), can_talk: guild.can_talk(channel) })).flatten()
    }
    /// `getChannel`.
    fn get_channel(&self, key: &str, primary_fallback: bool) -> Option<TextChannel> {
        if is_long(key) {
            return self.get_defined_channel(key, primary_fallback);
        }
        self.get_defined_channel(&self.settings.get_message_channel(key), primary_fallback)
    }
    /// `getDefinedChannel`.
    fn get_defined_channel(&self, key: &str, primary_fallback: bool) -> Option<TextChannel> {
        let resolved_id = self.settings.get_channel_id(key);
        if self.is_debug() {
            self.log(Level::Info, &format!("Channel definition {key} resolved as {resolved_id}"));
        }
        let channel = self.text_channel(resolved_id);
        if channel.is_none() && primary_fallback {
            if self.is_debug() {
                self.log(Level::Warning, &format!("Resolved channel id {resolved_id} was not found! Falling back to primary channel."));
            }
            return self.text_channel(self.primary_channel.load(Ordering::SeqCst));
        }
        channel
    }
    /// `/discordbroadcast`'s channel, and what it needs of it: its id, name and `canTalk`.
    pub fn broadcast_channel(&self, key: &str) -> Option<(u64, String, bool)> {
        self.get_defined_channel(key, false).map(|channel| (channel.id, channel.name, channel.can_talk))
    }
    fn self_avatar(&self) -> Option<String> {
        self.discord()?.self_user()?.avatar_url()
    }
    fn self_effective_name(&self) -> String {
        self.discord().and_then(|discord| discord.with_guild(self.guild_id(), |guild| guild.self_member.as_ref().map(|me| me.effective_name().to_owned()))).flatten().unwrap_or_default()
    }
    /// `{botname}`: `guild.getMember(jda.getSelfUser()).getEffectiveName()`.
    pub fn bot_name(&self) -> String {
        self.self_effective_name()
    }
    /// `getWebhookMessage(message)`.
    pub fn console_webhook_message(&self, message: String) -> WebhookMessage {
        WebhookMessage { content: message, username: self.settings.get_console_webhook_name(), avatar_url: self.self_avatar(), mentions: Mentions::NoGroups }
    }
    /// `sendMessage(event, message, groupMentions)`.
    pub fn send_event_message(&self, event: &DiscordMessageEvent, message: &str, group_mentions: bool) {
        let channel = self.get_channel(event.message_type.get_key(), true);
        let stripped_content = strip_format(message);
        let webhook = lock(&self.type_to_channel_id).get(event.message_type.get_key()).and_then(|id| lock(&self.channel_id_to_webhook).get(id).cloned());
        if let Some(dispatcher) = webhook {
            let avatar_url = event.avatar_url.clone().or_else(|| self.self_avatar());
            let username = event.name.clone().unwrap_or_else(|| self.self_effective_name());
            let _ = dispatcher.send(WebhookMessage { content: stripped_content, username, avatar_url, mentions: if group_mentions { Mentions::All } else { Mentions::NoGroups } });
            return;
        }
        let (Some(channel), Some(discord)) = (channel, self.discord()) else {
            return;
        };
        if !channel.can_talk {
            self.log(Level::Warning, &tl_literal("discordNoSendPermission", &[Arg::from(&channel.name)]));
            return;
        }
        let host = self.host.clone();
        discord.send_message(channel.id, &stripped_content, if group_mentions { Mentions::All } else { Mentions::NoGroups }, None).then(move |answer| {
            if let Err(error) = answer {
                host::log(&host, Level::Warning, &format!("Failed to send message to channel {}: {error}", channel.name));
            }
        });
    }
    /// `callEvent(DiscordMessageEvent)`: the listeners, then `BukkitListener.onDiscordMessage`.
    fn call_message_event(&self, mut event: DiscordMessageEvent) {
        let listeners = lock(&self.message_listeners).clone();
        for listener in listeners {
            listener(&mut event);
        }
        if !event.cancelled {
            self.send_event_message(&event, &event.message, event.allow_group_mentions);
        }
    }
    /// `callEvent`, or `runTask` from any other thread.
    fn raise(self: &Arc<Self>, event: DiscordMessageEvent) {
        if self.host.is_main() {
            self.call_message_event(event);
        } else {
            let service = self.clone();
            self.host.run_main(Box::new(move || service.call_message_event(event)));
        }
    }
    /// `DiscordUtil.dispatchDiscordMessage`.
    pub fn dispatch_discord_message(self: &Arc<Self>, message_type: MessageType, message: &str, allow_ping: bool, avatar_url: Option<String>, name: Option<String>, uuid: Option<String>) {
        if self.settings.get_message_channel(message_type.get_key()).eq_ignore_ascii_case("none") {
            return;
        }
        let event = DiscordMessageEvent { message_type, message: strip_format(message), allow_group_mentions: allow_ping, avatar_url, name: name.map(|name| strip_format(&name)), uuid, cancelled: false };
        // If the server is stopping, we cannot dispatch events.
        if event.message_type == default_types::SERVER_STOP {
            self.send_event_message(&event, &event.message, event.allow_group_mentions);
            return;
        }
        self.raise(event);
    }
    /// `DiscordService.sendMessage`.
    pub fn send_message(self: &Arc<Self>, message_type: MessageType, message: &str, allow_group_mentions: bool) {
        let key = message_type.get_key();
        if !self.is_registered(key) && !is_long(key) {
            self.log(Level::Warning, &format!("Sending message to channel \"{key}\" which is an unregistered type! If you are a plugin author, you should be registering your MessageType before using them."));
        }
        self.raise(DiscordMessageEvent::new(message_type, strip_format(message), allow_group_mentions));
    }
    /// `isRegistered`.
    pub fn is_registered(&self, key: &str) -> bool {
        lock(&self.registered_types).contains_key(key)
    }
    /// `registerMessageType`.
    pub fn register_message_type(&self, message_type: MessageType) -> Result<(), String> {
        let key = message_type.get_key();
        fail_if!(!key.chars().next().is_some_and(|c| c.is_ascii_lowercase()), "MessageType key must match \"^[a-z][a-z0-9-]*$\"".to_owned());
        let mut registered = lock(&self.registered_types);
        fail_if!(registered.contains_key(key), "A MessageType with that key is already registered!".to_owned());
        registered.insert(key.to_owned(), message_type);
        Ok(())
    }
    /// `startup`. On failure, `discordErrorLogin` is the caller's to report and `shutdown` to call.
    pub fn startup(self: &Arc<Self>) -> Result<(), String> {
        self.shutdown();
        self.invalid_startup.store(true, Ordering::SeqCst);
        self.log(Level::Info, &tl_literal("discordLoggingIn", &[]));
        let token = self.settings.get_bot_token();
        fail_if!(token.replace("INSERT-TOKEN-HERE", "").trim().is_empty(), tl_literal("discordErrorNoToken", &[]));
        let mut options = ClientOptions::new(Secret::new(&token), self.settings.get_presence());
        let proxy = self.settings.get_http_proxy_server();
        options.proxy = (!proxy.trim().is_empty()).then_some(proxy);
        (lock(&self.configure))(&mut options);
        let sink: Arc<dyn EventSink> = Arc::new(Listeners(Arc::downgrade(self)));
        let discord = Discord::connect(options, self.host.clone(), sink).map_err(|error| match error {
            LoginError::InvalidToken | LoginError::Other(_) => error.to_string(),
        })?;
        *self.discord.write().unwrap_or_else(|e| e.into_inner()) = Some(discord.clone());
        self.invalid_startup.store(false, Ordering::SeqCst);
        let me = discord.self_user().ok_or("Discord didn't say who the bot is")?;
        self.log(Level::Info, &tl_literal("discordLoggingInDone", &[Arg::from(me.as_tag())]));
        if discord.guild_ids().is_empty() {
            self.invalid_startup.store(true, Ordering::SeqCst);
            return Err(tl_literal("discordErrorNoGuildSize", &[]));
        }
        let guild_id = self.settings.get_guild_id();
        if discord.with_guild(guild_id, |_| ()).is_none() {
            self.invalid_startup.store(true, Ordering::SeqCst);
            return Err(tl_literal("discordErrorNoGuild", &[]));
        }
        self.guild_id.store(guild_id, Ordering::SeqCst);
        discord.load_self_member(guild_id);
        let required = [(MANAGE_WEBHOOKS, "Manage Webhooks"), (MANAGE_ROLES, "Manage Roles"), (NICKNAME_MANAGE, "Manage Nicknames"), (VIEW_CHANNEL, "View Channels"), (MESSAGE_SEND, "Send Messages"), (MESSAGE_EMBED_LINKS, "Embed Links")];
        let missing: Vec<&str> = required.iter().filter(|(permission, _)| discord.with_guild(guild_id, |guild| guild.self_has(*permission)) != Some(true)).map(|(_, name)| *name).collect();
        if !missing.is_empty() {
            self.invalid_startup.store(true, Ordering::SeqCst);
            return Err(tl_literal("discordErrorInvalidPerms", &[Arg::from(missing.join(", "))]));
        }
        // Load emotes into cache: the gateway keeps them current from here on.
        let host = self.host.clone();
        discord.retrieve_emojis(guild_id).then(move |answer| {
            if let Err(error) = answer {
                host::log(&host, Level::Warning, &format!("Failed to retrieve emojis from guild: {error}"));
            }
        });
        self.update_primary_channel()?;
        self.update_console_relay();
        self.update_types_relay();
        Ok(())
    }
    /// The `SERVER_START` line, a tick after `startup`.
    pub fn send_start_message(self: &Arc<Self>, seconds: f32) {
        self.dispatch_discord_message(default_types::SERVER_START, &self.settings.get_start_message(seconds), true, None, None, None);
    }
    /// `updatePrimaryChannel`.
    pub fn update_primary_channel(&self) -> Result<(), String> {
        let discord = self.discord().ok_or("no client")?;
        let channel = match self.text_channel(self.settings.get_primary_channel_id()) {
            Some(channel) => channel,
            None => {
                let default = discord.with_guild(self.guild_id(), |guild| guild.default_channel().filter(|channel| channel.kind == model::CHANNEL_TEXT).map(|channel| channel.id)).flatten();
                let channel = default.and_then(|id| self.text_channel(id)).ok_or_else(|| tl_literal("discordErrorNoPerms", &[]))?;
                self.log(Level::Warning, &tl_literal("discordErrorNoPrimary", &[Arg::from(&channel.name)]));
                channel
            }
        };
        fail_if!(!channel.can_talk, tl_literal("discordErrorNoPrimaryPerms", &[Arg::from(&channel.name)]));
        self.primary_channel.store(channel.id, Ordering::SeqCst);
        Ok(())
    }
    /// `parseMessageEmotes`.
    pub fn parse_message_emotes(&self, message: &str) -> String {
        let mut out = message.to_owned();
        if let Some(emojis) = self.discord().and_then(|discord| discord.with_guild(self.guild_id(), |guild| guild.emojis.clone())) {
            for emoji in emojis {
                out = out.replace(&format!(":{}:", emoji.name), &emoji.as_mention());
            }
        }
        out
    }
    /// `/discordbroadcast`'s tab list.
    pub fn emoji_names(&self) -> Vec<String> {
        self.discord().and_then(|discord| discord.with_guild(self.guild_id(), |guild| guild.emojis.iter().map(|emoji| emoji.name.clone()).collect())).unwrap_or_default()
    }
    /// `/discordbroadcast`'s send.
    pub fn broadcast(&self, channel: u64, message: &str, ping: bool) {
        if let Some(discord) = self.discord() {
            let _ = discord.send_message(channel, message, if ping { Mentions::All } else { Mentions::NoGroups }, None);
        }
    }
    /// `updatePresence`.
    pub fn update_presence(&self) {
        if let Some(discord) = self.discord() {
            discord.set_presence(&self.settings.get_presence());
        }
    }
    fn dispatcher_for(&self, webhook: &Webhook, capacity: usize) -> Option<Arc<WebhookDispatcher>> {
        let (discord, token) = (self.discord()?, webhook.token.clone()?);
        let id = webhook.id;
        let send: Sender = Box::new(move |message| discord.execute_webhook(id, &token, message.to_json()).map(|_| ()));
        Some(Arc::new(WebhookDispatcher::new(id, send, capacity, self.host.clone())))
    }
    /// `getOrCreateWebhook`, which Java joins: `None` if the bot may not, or it failed.
    fn get_or_create_webhook(&self, channel: &TextChannel, name: &str) -> Option<Webhook> {
        let discord = self.discord()?;
        if discord.with_guild(self.guild_id(), |guild| guild.channels.get(&channel.id).is_some_and(|known: &Channel| guild.self_has_in(known, MANAGE_WEBHOOKS))) != Some(true) {
            return None;
        }
        let webhooks = match discord.retrieve_webhooks(channel.id) {
            Ok(webhooks) => webhooks,
            Err(error) => {
                self.log(Level::Warning, &format!("Failed to retrieve webhooks from channel {}: {error}", channel.name));
                return None;
            }
        };
        let webhook = match webhooks.into_iter().find(|webhook| webhook.name == name && webhook.token.is_some()) {
            Some(webhook) => webhook,
            None => match discord.create_webhook(channel.id, name) {
                Ok(webhook) => webhook?,
                Err(error) => {
                    self.log(Level::Warning, &format!("Failed to create webhook {name} in channel {}: {error}", channel.name));
                    return None;
                }
            },
        };
        let mut active = lock(&self.active_webhooks);
        if !active.contains(&webhook.id) {
            active.push(webhook.id);
        }
        Some(webhook)
    }
    /// `updateTypesRelay`.
    pub fn update_types_relay(&self) {
        if !self.settings.is_show_avatar() && !self.settings.is_custom_bot_name() {
            for dispatcher in lock(&self.channel_id_to_webhook).values() {
                dispatcher.close();
            }
            lock(&self.type_to_channel_id).clear();
            lock(&self.channel_id_to_webhook).clear();
            return;
        }
        for message_type in default_types::values() {
            if !message_type.is_player() {
                continue;
            }
            let Some(channel) = self.get_channel(message_type.get_key(), true) else { continue };
            if lock(&self.type_to_channel_id).get(message_type.get_key()) == Some(&channel.id) {
                continue;
            }
            let Some(webhook) = self.get_or_create_webhook(&channel, ADVANCED_RELAY_NAME) else {
                if let Some(current) = lock(&self.channel_id_to_webhook).remove(&channel.id) {
                    current.close();
                }
                continue;
            };
            lock(&self.type_to_channel_id).insert(message_type.get_key().to_owned(), channel.id);
            if let Some(dispatcher) = self.dispatcher_for(&webhook, DEFAULT_QUEUE_CAPACITY) {
                let old = lock(&self.channel_id_to_webhook).insert(channel.id, dispatcher);
                if let Some(old) = old {
                    old.close();
                }
            }
        }
    }
    /// `updateConsoleRelay`.
    pub fn update_console_relay(self: &Arc<Self>) {
        let console_def = self.settings.get_console_channel_def();
        let webhook = if let Some(found) = WEBHOOK_PATTERN.captures(&console_def) {
            *lock(&self.command_channel) = None;
            Webhook { id: found[1].parse().unwrap_or(0), name: String::new(), token: Some(Secret::new(&found[2])) }
        } else if let Some(channel) = self.get_channel(&console_def, false) {
            *lock(&self.command_channel) = self.settings.is_console_command_relay().then_some(channel.id);
            if *lock(&self.last_console_id) == Some(channel.id) {
                return;
            }
            let Some(webhook) = self.get_or_create_webhook(&channel, CONSOLE_RELAY_NAME) else {
                self.log(Level::Info, &tl_literal("discordErrorLoggerNoPerms", &[]));
                return;
            };
            *lock(&self.last_console_id) = Some(channel.id);
            webhook
        } else {
            if console_def != "none" && !console_def.starts_with('0') {
                self.log(Level::Info, &tl_literal("discordErrorLoggerInvalidChannel", &[]));
            }
            self.shutdown_console_relay(true);
            return;
        };
        self.shutdown_console_relay(false);
        *lock(&self.console_webhook) = self.dispatcher_for(&webhook, 50);
        let mut injector = lock(&self.injector);
        if injector.as_ref().is_none_or(ConsoleInjector::is_removed) {
            *injector = Some(ConsoleInjector::start(self));
        }
    }
    /// `getConsoleWebhook`.
    pub fn get_console_webhook(&self) -> Option<Arc<WebhookDispatcher>> {
        lock(&self.console_webhook).clone()
    }
    /// `shutdownConsoleRelay`.
    fn shutdown_console_relay(&self, close_injector: bool) {
        if let Some(webhook) = lock(&self.console_webhook).take() {
            webhook.close();
        }
        if close_injector {
            let injector = lock(&self.injector).take();
            if let Some(injector) = injector {
                injector.remove();
            }
            *lock(&self.command_channel) = None;
        }
    }
    /// `shutdown`: every thread this started is gone when it returns.
    pub fn shutdown(self: &Arc<Self>) {
        self.interaction_controller.shutdown();
        let Some(discord) = self.discord() else { return };
        if !self.is_invalid_startup() {
            self.dispatch_discord_message(default_types::SERVER_STOP, &self.settings.get_stop_message(), true, None, None, None);
        }
        self.shutdown_console_relay(true);
        let dispatchers: Vec<Arc<WebhookDispatcher>> = lock(&self.channel_id_to_webhook).drain().map(|(_, dispatcher)| dispatcher).collect();
        for dispatcher in dispatchers {
            dispatcher.close();
        }
        lock(&self.type_to_channel_id).clear();
        *lock(&self.last_console_id) = None;
        *self.discord.write().unwrap_or_else(|e| e.into_inner()) = None;
        discord.shutdown();
    }
    /// `getMemberById`.
    pub fn get_member_by_id(&self, id: &str) -> Pending<Option<InteractionMember>> {
        let Some(discord) = self.discord() else { return Pending::ready(None) };
        let guild_id = self.guild_id();
        let client = discord.clone();
        discord.retrieve_member(guild_id, id).map(move |answer| {
            let member = answer.ok().flatten()?;
            let guild = client.guild(guild_id)?;
            Some(InteractionMember::of(&client, &guild, member))
        })
    }
    /// `getMemberById`, keeping a failed lookup apart from someone who isn't there.
    pub fn try_member_by_id(&self, id: &str) -> Pending<Result<Option<InteractionMember>, RestError>> {
        let Some(discord) = self.discord() else {
            return Pending::ready(Err(RestError::Shutdown));
        };
        let guild_id = self.guild_id();
        let client = discord.clone();
        discord.retrieve_member(guild_id, id).map(move |answer| {
            let member = answer?;
            Ok(member.and_then(|member| Some(InteractionMember::of(&client, &client.guild(guild_id)?, member))))
        })
    }
    /// `getRole`.
    pub fn get_role(&self, id: &str) -> Option<InteractionRole> {
        let id = id.parse().ok()?;
        self.discord()?.with_guild(self.guild_id(), |guild| guild.roles.get(&id).map(|role| InteractionRole::of(guild, role))).flatten()
    }
    /// `modifyMemberRoles`.
    pub fn modify_member_roles(&self, member: &InteractionMember, add_roles: &[InteractionRole], remove_roles: &[InteractionRole]) -> Pending<()> {
        if add_roles.is_empty() && remove_roles.is_empty() {
            return Pending::ready(());
        }
        let Some(discord) = self.discord() else { return Pending::ready(()) };
        let mut roles = member.role_ids();
        roles.retain(|role| !remove_roles.iter().any(|gone| gone.id_number() == *role));
        for role in add_roles {
            if !roles.contains(&role.id_number()) {
                roles.push(role.id_number());
            }
        }
        let host = self.host.clone();
        discord.modify_member_roles(self.guild_id(), member.id_number(), &roles).map(move |answer| {
            if let Err(error) = answer {
                host::log(&host, Level::Warning, &format!("Failed to modify member roles: {error}"));
            }
        })
    }
    /// `getInviteUrl`.
    pub fn get_invite_url(&self) -> String {
        self.settings.get_discord_url()
    }
    /// `isActiveWebhook`: `ACTIVE_WEBHOOKS.contains`.
    fn is_active_webhook(&self, id: u64) -> bool {
        lock(&self.active_webhooks).contains(&id)
    }
    /// `DiscordListener.onMessageReceived`, up to what needs the game, which waits for the main thread.
    fn on_message_received(self: &Arc<Self>, message: &Message) {
        let Some(discord) = self.discord() else { return };
        let Some(guild) = message.guild_id.and_then(|id| discord.guild(id)) else { return };
        let Some(channel) = guild.text_channel(message.channel_id) else { return };
        let settings = &self.settings;
        let me = discord.self_user().map(|user| user.id);
        if message.author.bot && !message.is_webhook() && (!settings.is_show_bot_messages() || Some(message.author.id) == me) {
            return;
        }
        if message.is_webhook() && (!settings.is_show_webhook_messages() || self.is_active_webhook(message.author.id)) {
            return;
        }
        // Get list of channel names that have this channel id mapped.
        let Some(keys) = settings.get_keys_from_channel_id(message.channel_id).filter(|keys| !keys.is_empty()) else {
            if self.is_debug() {
                self.log(Level::Info, &format!("Skipping message due to no channel keys for id {}!", message.channel_id));
            }
            return;
        };
        let member = message.member.as_ref();
        let effective_name = member.map_or_else(|| message.author.name.clone(), |member: &Member| member.effective_name().to_owned());
        let display = message.content_display(Some(&guild));
        if let Some(filter) = settings.get_discord_filters().iter().find(|pattern| pattern.is_match(&display)) {
            if self.is_debug() {
                self.log(Level::Info, &format!("Skipping message {} with content, \"{display}\" as it matched the filter {}!", message.id, filter.as_str()));
            }
            return;
        }
        let mut text = display;
        if settings.is_show_discord_attachments() {
            for attachment in &message.attachments {
                text.push(' ');
                text.push_str(attachment);
            }
        }
        // Strip message.
        let newline = if settings.is_chat_filter_newlines() { '\n' } else { ' ' };
        let stripped = abbreviate(text.replace(newline, " ").trim(), settings.get_chat_discord_max_length());
        // Apply or strip color formatting.
        let roles: Vec<Role> = member.map(|member| guild.roles_of(member).into_iter().cloned().collect()).unwrap_or_default();
        let admin = member.is_some_and(|member| guild.permissions_of(member) & model::ADMINISTRATOR != 0);
        let formatted_text = if member.is_none_or(|member| has_roles(admin, member.user.id, &roles, &settings.get_permitted_formatting_roles())) { replace_format(&stripped) } else { strip_format(&stripped) };
        // Don't send blank messages.
        if formatted_text.trim().is_empty() {
            if self.is_debug() {
                self.log(Level::Info, &format!("Skipping finalized empty message {}", message.id));
            }
            return;
        }
        let args = [
            Arg::from(&channel.name),
            Arg::from(&message.author.name),
            Arg::from(&message.author.discriminator),
            Arg::from(message.author.as_tag()),
            Arg::from(effective_name),
            Arg::from(role_color_format(settings, &roles)),
            Arg::from(formatted_text),
            Arg::from(role_format(settings, &roles)),
        ];
        let formatted = parse_to_aliases(&settings.discord_to_mc_format().format(&args));
        let relay_list = settings.get_relay_to_console_list();
        if keys.iter().any(|group| relay_list.contains(group)) {
            self.log(Level::Info, &formatted);
        }
        let relay = Relay {
            keys,
            formatted,
            raw: message.content.clone(),
            member: message.member.clone().map(|member| InteractionMember::of(&discord, &guild, member)),
            channel: InteractionChannel::of(channel),
        };
        let host = self.host.clone();
        self.host.run_main(Box::new(move || host.deliver(relay)));
    }
    /// `DiscordCommandDispatcher.onMessageReceived`.
    fn on_console_message(self: &Arc<Self>, message: &Message) {
        let Some(discord) = self.discord() else { return };
        if self.get_console_webhook().is_none() || *lock(&self.command_channel) != Some(message.channel_id) {
            return;
        }
        if discord.with_guild(self.guild_id(), |guild| guild.text_channel(message.channel_id).is_some()) != Some(true) {
            return;
        }
        let me = discord.self_user().map(|user| user.id);
        if (message.is_webhook() || message.author.bot) && (!self.settings.is_console_bot_command_relay() || self.is_active_webhook(message.author.id) || Some(message.author.id) == me) {
            return;
        }
        let (command, channel, id) = (message.content.clone(), message.channel_id, message.id);
        let host = self.host.clone();
        self.host.run_main(Box::new(move || {
            let reply: Feedback = Box::new(move |text| {
                let _ = discord.send_message(channel, &text, Mentions::All, Some(id));
            });
            host.dispatch_command(&command, reply);
        }));
    }
}

/// `MessageUtil.formatMessage`'s arguments for a line with two names: `sanitizeDiscordMarkdown` on each.
pub fn names(username: &str, display_name: &str) -> [Arg; 2] {
    [clean(username), clean(display_name)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_webhook_pattern_takes_id_and_token() {
        let found = WEBHOOK_PATTERN.captures("https://discord.com/api/webhooks/123/abc-DEF_1").unwrap();
        assert_eq!((&found[1], &found[2]), ("123", "abc-DEF_1"));
        assert!(WEBHOOK_PATTERN.is_match("https://canary.discordapp.com/api/v10/webhooks/1/x"));
        assert!(!WEBHOOK_PATTERN.is_match("000000000000000000"));
    }
}
