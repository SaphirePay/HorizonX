//! A small blocking Discord client: what the Java got from JDA and discord-webhooks, and only that. It
//! knows nothing of the game. The gateway and REST run on threads of their own, and `shutdown` stops
//! them all. What arrives goes to an `EventSink`, on the gateway's thread and in order.
pub mod gateway;
pub mod model;
pub mod pending;
pub mod rest;
pub mod secret;

use crate::host::{self, Host, Level};
use model::{Emoji, Guild, Interaction, Member, Mentions, Message, User, Webhook, snowflake};
use pending::Pending;
use rest::{Answer, Call, Rest, RestError, Route};
use secret::Secret;
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const API: &str = "https://discord.com/api/v10";

/// `GatewayIntent.DEFAULT` of `JDABuilder.createDefault`, and `MESSAGE_CONTENT`, which the Java turns on:
/// everything but members, presences, webhooks and typing.
pub const INTENTS: u64 = 1 | 1 << 2 | 1 << 3 | 1 << 6 | 1 << 7 | 1 << 9 | 1 << 10 | 1 << 12 | 1 << 13 | 1 << 15 | 1 << 16 | 1 << 20 | 1 << 21 | 1 << 24 | 1 << 25;

/// `OnlineStatus` and `Activity`, as the gateway takes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presence {
    pub status: String,
    /// The activity's type and message.
    pub activity: Option<(u8, String)>,
}

impl Presence {
    pub fn to_json(&self) -> Value {
        let activities: Vec<Value> = self.activity.iter().map(|(kind, name)| json!({ "name": name, "type": kind })).collect();
        json!({ "since": null, "activities": activities, "status": self.status, "afk": false })
    }
}

#[derive(Clone)]
pub struct ClientOptions {
    pub token: Secret,
    /// The REST root. Only tests change it: nothing reads it from a config.
    pub api_base: String,
    /// Certificates to trust beyond the public roots, as DER. Only tests set it.
    pub extra_roots: Vec<Vec<u8>>,
    /// `http-proxy-server`, for the gateway alone as in the Java.
    pub proxy: Option<String>,
    pub intents: u64,
    pub presence: Presence,
    /// How long `connect` waits for READY and its guilds: `awaitReady`, which in the Java has no limit.
    pub login_timeout: Duration,
    /// The first reconnect's delay, doubled each failure.
    pub reconnect_base: Duration,
    /// Discord allows one IDENTIFY in five seconds.
    pub identify_gap: Duration,
    /// How long after READY a guild may stay unavailable before startup goes on without it.
    pub guild_timeout: Duration,
}

impl ClientOptions {
    pub fn new(token: Secret, presence: Presence) -> Self {
        Self {
            token,
            api_base: API.to_owned(),
            extra_roots: Vec::new(),
            proxy: None,
            intents: INTENTS,
            presence,
            login_timeout: Duration::from_secs(60),
            reconnect_base: Duration::from_secs(1),
            identify_gap: Duration::from_secs(5),
            guild_timeout: Duration::from_secs(15),
        }
    }
}

/// What arrives from the gateway.
pub trait EventSink: Send + Sync + 'static {
    fn on_message(&self, message: &Message);
    fn on_interaction(&self, interaction: Interaction);
    /// A close code that ends it: the bot won't come back by itself.
    fn on_fatal(&self, reason: &str);
}

#[derive(Debug)]
pub enum LoginError {
    /// `InvalidTokenException`: REST or the gateway refused the token.
    InvalidToken,
    Other(String),
}

impl std::fmt::Display for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidToken => f.write_str("The provided token is invalid!"),
            Self::Other(reason) => f.write_str(reason),
        }
    }
}

/// What the gateway has told us.
#[derive(Default)]
pub struct Cache {
    pub self_user: Option<User>,
    pub application_id: u64,
    pub guilds: BTreeMap<u64, Guild>,
}

impl Cache {
    fn apply_ready(&mut self, data: &Value) {
        self.self_user = User::from_json(&data["user"]);
        self.application_id = snowflake(&data["application"]["id"]).unwrap_or(0);
    }
    fn apply(&mut self, kind: &str, data: &Value) {
        gateway::apply(self, kind, data);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Readiness {
    Connecting,
    Ready,
    Failed(String),
}

/// Shared by the gateway's thread and everyone holding a `Discord`.
struct Inner {
    options: ClientOptions,
    host: Arc<dyn Host>,
    sink: Arc<dyn EventSink>,
    rest: Rest,
    cache: RwLock<Cache>,
    stop: AtomicBool,
    /// Wakes a pause on `stop`.
    stopping: (Mutex<()>, Condvar),
    readiness: (Mutex<Readiness>, Condvar),
    outbox: Mutex<VecDeque<Value>>,
    presence: Mutex<Value>,
    gateway: Mutex<Option<JoinHandle<()>>>,
}

impl Inner {
    fn sink(&self) -> &Arc<dyn EventSink> {
        &self.sink
    }
    fn is_debug(&self) -> bool {
        self.host.is_debug()
    }
    fn cache_mut(&self) -> RwLockWriteGuard<'_, Cache> {
        self.cache.write().unwrap_or_else(|e| e.into_inner())
    }
    fn cache(&self) -> RwLockReadGuard<'_, Cache> {
        self.cache.read().unwrap_or_else(|e| e.into_inner())
    }
    fn presence(&self) -> Value {
        self.presence.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn take_outbox(&self) -> Vec<Value> {
        self.outbox.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect()
    }
    /// Sleeps, unless told to stop.
    fn pause(&self, length: Duration) {
        let until = Instant::now() + length;
        let mut guard = self.stopping.0.lock().unwrap_or_else(|e| e.into_inner());
        while !self.stop.load(Ordering::SeqCst) {
            let Some(left) = until.checked_duration_since(Instant::now()) else { return };
            guard = self.stopping.1.wait_timeout(guard, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
    fn set_readiness(&self, state: Readiness) {
        let mut readiness = self.readiness.0.lock().unwrap_or_else(|e| e.into_inner());
        if *readiness == Readiness::Connecting {
            *readiness = state;
            self.readiness.1.notify_all();
        }
    }
    fn gateway_failed(&self, reason: &str) {
        let first = *self.readiness.0.lock().unwrap_or_else(|e| e.into_inner()) == Readiness::Connecting;
        self.set_readiness(Readiness::Failed(reason.to_owned()));
        if !first {
            self.sink.on_fatal(reason);
        }
    }
}

/// `JDA`.
#[derive(Clone)]
pub struct Discord(Arc<Inner>);

fn path(template: &str, parts: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut pieces = template.split("{}");
    out.push_str(pieces.next().unwrap_or_default());
    for (piece, part) in pieces.zip(parts) {
        out.push_str(part);
        out.push_str(piece);
    }
    out
}

impl Discord {
    /// `JDABuilder.build().awaitReady()`: the token checked, the gateway up, READY and the guilds in.
    pub fn connect(options: ClientOptions, host: Arc<dyn Host>, sink: Arc<dyn EventSink>) -> Result<Self, LoginError> {
        let rest = Rest::new(&options.api_base, options.token.clone(), host.clone());
        let presence = options.presence.to_json();
        let login_timeout = options.login_timeout;
        let inner = Arc::new(Inner {
            options,
            host,
            sink,
            rest,
            cache: RwLock::new(Cache::default()),
            stop: AtomicBool::new(false),
            stopping: (Mutex::new(()), Condvar::new()),
            readiness: (Mutex::new(Readiness::Connecting), Condvar::new()),
            outbox: Mutex::new(VecDeque::new()),
            presence: Mutex::new(presence),
            gateway: Mutex::new(None),
        });
        let discord = Self(inner);
        match discord.login(login_timeout) {
            Ok(()) => Ok(discord),
            Err(error) => {
                discord.shutdown();
                Err(error)
            }
        }
    }
    fn login(&self, limit: Duration) -> Result<(), LoginError> {
        let refused = |error: RestError| if error.is_unauthorized() { LoginError::InvalidToken } else { LoginError::Other(error.to_string()) };
        self.0.rest.complete(Call::new(Route::new("GET", "/users/@me", 0), "/users/@me".to_owned())).map_err(refused)?;
        let info = self.0.rest.complete(Call::new(Route::new("GET", "/gateway/bot", 0), "/gateway/bot".to_owned())).map_err(refused)?;
        let url = info["url"].as_str().ok_or_else(|| LoginError::Other("Discord gave no gateway address".to_owned()))?.to_owned();
        if info["session_start_limit"]["remaining"].as_u64() == Some(0) {
            let wait = Duration::from_millis(info["session_start_limit"]["reset_after"].as_u64().unwrap_or(0));
            return Err(LoginError::Other(format!("Discord allows no more logins for {} seconds", wait.as_secs())));
        }
        let inner = self.0.clone();
        let thread = std::thread::Builder::new().name("EssX-Discord-Gateway".to_owned()).spawn(move || gateway::run(inner, url)).map_err(|error| LoginError::Other(error.to_string()))?;
        *self.0.gateway.lock().unwrap_or_else(|e| e.into_inner()) = Some(thread);
        let until = Instant::now() + limit;
        let mut readiness = self.0.readiness.0.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match &*readiness {
                Readiness::Ready => return Ok(()),
                Readiness::Failed(reason) if reason == gateway_token_refused() => return Err(LoginError::InvalidToken),
                Readiness::Failed(reason) => return Err(LoginError::Other(reason.clone())),
                Readiness::Connecting => {}
            }
            let Some(left) = until.checked_duration_since(Instant::now()) else {
                return Err(LoginError::Other("Timed out waiting for Discord to be ready".to_owned()));
            };
            readiness = self.0.readiness.1.wait_timeout(readiness, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
    /// `JDA.shutdown` and the wait for it: when this returns, this client's threads are gone.
    pub fn shutdown(&self) {
        self.0.stop.store(true, Ordering::SeqCst);
        self.0.stopping.1.notify_all();
        let started = Instant::now();
        let gateway = self.0.gateway.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(gateway) = gateway {
            let _ = gateway.join();
        }
        self.0.rest.shutdown(Duration::from_secs(3));
        if started.elapsed() > Duration::from_secs(5) {
            host::log(&self.0.host, Level::Warning, "JDA took longer than expected to shutdown, this may have caused some problems.");
        }
    }
    pub fn is_stopped(&self) -> bool {
        self.0.stop.load(Ordering::SeqCst)
    }
    /// `getSelfUser`.
    pub fn self_user(&self) -> Option<User> {
        self.0.cache().self_user.clone()
    }
    pub fn application_id(&self) -> u64 {
        self.0.cache().application_id
    }
    /// `getGuilds`.
    pub fn guild_ids(&self) -> Vec<u64> {
        self.0.cache().guilds.keys().copied().collect()
    }
    /// `getGuildById`: as it is now.
    pub fn guild(&self, id: u64) -> Option<Guild> {
        self.0.cache().guilds.get(&id).cloned()
    }
    /// Reads a guild where it's kept, with no copy.
    pub fn with_guild<R>(&self, id: u64, read: impl FnOnce(&Guild) -> R) -> Option<R> {
        self.0.cache().guilds.get(&id).map(read)
    }
    /// `getPresence().setPresence`.
    pub fn set_presence(&self, presence: &Presence) {
        let value = presence.to_json();
        *self.0.presence.lock().unwrap_or_else(|e| e.into_inner()) = value.clone();
        self.0.outbox.lock().unwrap_or_else(|e| e.into_inner()).push_back(json!({ "op": 3, "d": value }));
    }
    /// Asks REST for the bot's member, for a GUILD_CREATE that left it out.
    pub fn load_self_member(&self, guild_id: u64) {
        let Some(me) = self.self_user() else { return };
        if self.with_guild(guild_id, |guild| guild.self_member.is_some()) != Some(false) {
            return;
        }
        let call = Call::new(Route::new("GET", "/guilds/{}/members/{}", guild_id), path("/guilds/{}/members/{}", &[&guild_id.to_string(), &me.id.to_string()]));
        if let Ok(value) = self.0.rest.complete(call)
            && let Some(guild) = self.0.cache_mut().guilds.get_mut(&guild_id)
        {
            guild.self_member = Member::from_json(&value, None);
        }
    }
    /// `TextChannel.sendMessage`, and `Message.reply` with `reply_to`.
    pub fn send_message(&self, channel: u64, content: &str, mentions: Mentions, reply_to: Option<u64>) -> Pending<Answer> {
        let mut body = json!({ "content": content, "allowed_mentions": mentions.to_json() });
        if let Some(message) = reply_to {
            body["message_reference"] = json!({ "message_id": message.to_string(), "fail_if_not_exists": false });
            body["allowed_mentions"]["replied_user"] = json!(true);
        }
        self.0.rest.queue(Call::new(Route::new("POST", "/channels/{}/messages", channel), path("/channels/{}/messages", &[&channel.to_string()])).body(body))
    }
    /// `TextChannel.retrieveWebhooks`.
    pub fn retrieve_webhooks(&self, channel: u64) -> Result<Vec<Webhook>, RestError> {
        let answer = self.0.rest.complete(Call::new(Route::new("GET", "/channels/{}/webhooks", channel), path("/channels/{}/webhooks", &[&channel.to_string()])))?;
        Ok(answer.as_array().into_iter().flatten().filter_map(Webhook::from_json).collect())
    }
    /// `TextChannel.createWebhook`.
    pub fn create_webhook(&self, channel: u64, name: &str) -> Result<Option<Webhook>, RestError> {
        let answer = self.0.rest.complete(Call::new(Route::new("POST", "/channels/{}/webhooks", channel), path("/channels/{}/webhooks", &[&channel.to_string()])).body(json!({ "name": name })))?;
        Ok(Webhook::from_json(&answer))
    }
    /// `WebhookClient.send`, with `wait=false`, once: a 429 comes back to the caller.
    pub fn execute_webhook(&self, id: u64, token: &Secret, body: Value) -> Answer {
        let call = Call::new(Route::new("POST", "/webhooks/{}/{}", id), format!("/webhooks/{id}/{}?wait=false", token.expose())).body(body).no_auth();
        self.0.rest.call_now(&call)
    }
    /// `Guild.retrieveEmojis`.
    pub fn retrieve_emojis(&self, guild: u64) -> Pending<Result<(), RestError>> {
        let inner = self.0.clone();
        self.0.rest.queue(Call::new(Route::new("GET", "/guilds/{}/emojis", guild), path("/guilds/{}/emojis", &[&guild.to_string()]))).map(move |answer| {
            let emojis: Vec<Emoji> = answer?.as_array().into_iter().flatten().filter_map(Emoji::from_json).collect();
            if let Some(known) = inner.cache_mut().guilds.get_mut(&guild) {
                known.emojis = emojis;
            }
            Ok(())
        })
    }
    /// `Guild.retrieveMemberById`: `None` if they aren't there.
    pub fn retrieve_member(&self, guild: u64, id: &str) -> Pending<Result<Option<Member>, RestError>> {
        let call = Call::new(Route::new("GET", "/guilds/{}/members/{}", guild), path("/guilds/{}/members/{}", &[&guild.to_string(), id]));
        self.0.rest.queue(call).map(|answer| match answer {
            Ok(value) => Ok(Member::from_json(&value, None)),
            Err(error) if error.is_not_found() => Ok(None),
            Err(error) => Err(error),
        })
    }
    /// `Guild.modifyMemberRoles`: the role list that results.
    pub fn modify_member_roles(&self, guild: u64, member: u64, roles: &[u64]) -> Pending<Answer> {
        let roles: Vec<String> = roles.iter().map(u64::to_string).collect();
        let call = Call::new(Route::new("PATCH", "/guilds/{}/members/{}", guild), path("/guilds/{}/members/{}", &[&guild.to_string(), &member.to_string()])).body(json!({ "roles": roles }));
        self.0.rest.queue(call)
    }
    /// `User.openPrivateChannel`, then `sendMessage`: whether it got there.
    pub fn send_private_message(&self, user: u64, content: &str) -> Pending<bool> {
        let (pending, done) = Pending::channel();
        let call = Call::new(Route::new("POST", "/users/@me/channels", 0), "/users/@me/channels".to_owned()).body(json!({ "recipient_id": user.to_string() }));
        let (client, content) = (self.clone(), content.to_owned());
        self.0.rest.queue(call).then(move |answer| match answer.ok().as_ref().and_then(|value| snowflake(&value["id"])) {
            Some(channel) => client.send_message(channel, &content, Mentions::All, None).then(move |sent| done.complete(sent.is_ok())),
            None => done.complete(false),
        });
        pending
    }
    /// `Guild.updateCommands`: the whole list, which replaces what's there.
    pub fn put_guild_commands(&self, guild: u64, commands: Value) -> Pending<Answer> {
        let app = self.application_id();
        self.0.rest.queue(Call::new(Route::new("PUT", "/applications/{}/guilds/{}/commands", guild), path("/applications/{}/guilds/{}/commands", &[&app.to_string(), &guild.to_string()])).body(commands))
    }
    /// `Guild.upsertCommand`.
    pub fn post_guild_command(&self, guild: u64, command: Value) -> Pending<Answer> {
        let app = self.application_id();
        self.0.rest.queue(Call::new(Route::new("POST", "/applications/{}/guilds/{}/commands", guild), path("/applications/{}/guilds/{}/commands", &[&app.to_string(), &guild.to_string()])).body(command))
    }
    /// The first answer to an interaction: `reply` or `deferReply`.
    pub fn interaction_callback(&self, interaction: &Interaction, body: Value) -> Pending<Answer> {
        let call = Call::new(Route::new("POST", "/interactions/{}/{}/callback", interaction.id), format!("/interactions/{}/{}/callback", interaction.id, interaction.token.0.expose())).body(body).no_auth();
        self.0.rest.queue(call)
    }
    /// `InteractionHook.editOriginal`.
    pub fn edit_original(&self, interaction: &Interaction, body: Value) -> Pending<Answer> {
        let app = if interaction.application_id == 0 { self.application_id() } else { interaction.application_id };
        let call = Call::new(Route::new("PATCH", "/webhooks/{}/{}/messages/@original", interaction.id), format!("/webhooks/{app}/{}/messages/@original", interaction.token.0.expose())).body(body).no_auth();
        self.0.rest.queue(call)
    }
}

fn gateway_token_refused() -> &'static str {
    match gateway::classify(4004) {
        gateway::CloseAction::Fatal(reason) => reason,
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_fill_their_template() {
        assert_eq!(path("/guilds/{}/members/{}", &["1", "2"]), "/guilds/1/members/2");
        assert_eq!(path("/users/@me", &[]), "/users/@me");
    }

    #[test]
    fn intents_are_the_ones_the_java_asks_for() {
        const GUILD_MESSAGES: u64 = 1 << 9;
        const MESSAGE_CONTENT: u64 = 1 << 15;
        const GUILD_MEMBERS: u64 = 1 << 1;
        const GUILD_PRESENCES: u64 = 1 << 8;
        assert_eq!(INTENTS & 1, 1);
        assert_ne!(INTENTS & GUILD_MESSAGES, 0);
        assert_ne!(INTENTS & MESSAGE_CONTENT, 0);
        assert_eq!(INTENTS & (GUILD_MEMBERS | GUILD_PRESENCES), 0);
    }

    #[test]
    fn presence_goes_out_as_the_gateway_takes_it() {
        let presence = Presence { status: "dnd".to_owned(), activity: Some((3, "Minecraft".to_owned())) };
        assert_eq!(presence.to_json(), json!({ "since": null, "activities": [{ "name": "Minecraft", "type": 3 }], "status": "dnd", "afk": false }));
        assert_eq!(Presence { status: "online".to_owned(), activity: None }.to_json()["activities"], json!([]));
    }
}
