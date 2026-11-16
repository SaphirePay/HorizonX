//! `net.essentialsx.api.v2.services.discord` and its events. `essentials-discordlink` is linked into
//! the same library and calls this directly, as Java's `getServicesManager().load` lets it.
use essentials::prelude::*;
use crate::client::Discord;
use crate::client::model::{Channel, DEFAULT_COLOR_RAW, Guild, Interaction, Member, Role};
use crate::client::pending::Pending;
use crate::host::{self, Host, Level};
use crate::interactions::has_roles;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

/// `MessageType`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MessageType {
    key: Cow<'static, str>,
    player: bool,
}

impl MessageType {
    /// `new MessageType(key)`.
    pub fn new(key: &str) -> Result<Self, String> {
        fail_if!(!key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "Key must match \"^[a-z0-9-]*$\"".to_owned());
        Ok(Self { key: Cow::Owned(key.to_owned()), player: false })
    }
    const fn default_type(key: &'static str, player: bool) -> Self {
        Self { key: Cow::Borrowed(key), player }
    }
    pub fn get_key(&self) -> &str {
        &self.key
    }
    pub fn is_player(&self) -> bool {
        self.player
    }
}

/// `MessageType.DefaultTypes`.
pub mod default_types {
    use super::MessageType;

    pub const JOIN: MessageType = MessageType::default_type("join", true);
    pub const FIRST_JOIN: MessageType = MessageType::default_type("first-join", true);
    pub const LEAVE: MessageType = MessageType::default_type("leave", true);
    pub const CHAT: MessageType = MessageType::default_type("chat", true);
    pub const PRIVATE_CHAT: MessageType = MessageType::default_type("private-chat", true);
    pub const DEATH: MessageType = MessageType::default_type("death", true);
    pub const AFK: MessageType = MessageType::default_type("afk", true);
    pub const ADVANCEMENT: MessageType = MessageType::default_type("advancement", true);
    pub const ACTION: MessageType = MessageType::default_type("action", true);
    pub const SERVER_START: MessageType = MessageType::default_type("server-start", false);
    pub const SERVER_STOP: MessageType = MessageType::default_type("server-stop", false);
    pub const KICK: MessageType = MessageType::default_type("kick", false);
    pub const MUTE: MessageType = MessageType::default_type("mute", false);
    pub const LOCAL: MessageType = MessageType::default_type("local", true);
    pub const QUESTION: MessageType = MessageType::default_type("question", true);
    pub const SHOUT: MessageType = MessageType::default_type("shout", true);

    /// `values`.
    pub fn values() -> [MessageType; 16] {
        [JOIN, FIRST_JOIN, LEAVE, CHAT, PRIVATE_CHAT, DEATH, AFK, ADVANCEMENT, ACTION, SERVER_START, SERVER_STOP, KICK, MUTE, LOCAL, QUESTION, SHOUT]
    }
}

/// `DiscordMessageEvent`.
#[derive(Clone, Debug)]
pub struct DiscordMessageEvent {
    pub message_type: MessageType,
    pub message: String,
    pub allow_group_mentions: bool,
    pub avatar_url: Option<String>,
    pub name: Option<String>,
    /// `getUUID`, for a message of a player's.
    pub uuid: Option<String>,
    pub cancelled: bool,
}

impl DiscordMessageEvent {
    pub fn new(message_type: MessageType, message: String, allow_group_mentions: bool) -> Self {
        Self { message_type, message, allow_group_mentions, avatar_url: None, name: None, uuid: None, cancelled: false }
    }
}

/// `InteractionCommandArgumentType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionCommandArgumentType {
    String,
    Integer,
    Boolean,
    User,
    Channel,
    Role,
}

impl InteractionCommandArgumentType {
    /// `OptionType`'s number.
    pub fn option_type(self) -> u8 {
        match self {
            Self::String => 3,
            Self::Integer => 4,
            Self::Boolean => 5,
            Self::User => 6,
            Self::Channel => 7,
            Self::Role => 8,
        }
    }
}

/// `InteractionCommandArgument`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionCommandArgument {
    pub name: String,
    pub description: String,
    pub kind: InteractionCommandArgumentType,
    pub required: bool,
}

impl InteractionCommandArgument {
    pub fn new(name: &str, description: &str, kind: InteractionCommandArgumentType, required: bool) -> Self {
        Self { name: name.to_owned(), description: description.to_owned(), kind, required }
    }
}

/// `InteractionCommand`.
pub trait InteractionCommand: Send + Sync {
    fn is_disabled(&self) -> bool;
    fn is_ephemeral(&self) -> bool;
    fn get_name(&self) -> String;
    fn get_description(&self) -> String;
    fn get_arguments(&self) -> Vec<InteractionCommandArgument>;
    /// On the main thread.
    fn on_command(&self, event: InteractionEvent);
}

/// `InteractionException`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionException(pub String);

/// `InteractionChannel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionChannel {
    pub id: String,
    pub name: String,
}

impl InteractionChannel {
    pub fn of(channel: &Channel) -> Self {
        Self { id: channel.id.to_string(), name: channel.name.clone() }
    }
}

/// `InteractionRole`, with what it needs from the guild taken when it was made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionRole {
    role: Role,
    public: bool,
    can_interact: bool,
}

impl InteractionRole {
    pub fn of(guild: &Guild, role: &Role) -> Self {
        Self { role: role.clone(), public: role.id == guild.id, can_interact: guild.can_interact(role) }
    }
    pub fn get_name(&self) -> &str {
        &self.role.name
    }
    /// `getAsMention`.
    pub fn get_as_mention(&self) -> String {
        format!("<@&{}>", self.role.id)
    }
    pub fn is_managed(&self) -> bool {
        self.role.managed
    }
    pub fn is_public_role(&self) -> bool {
        self.public
    }
    pub fn get_color_raw(&self) -> i32 {
        self.role.color_raw()
    }
    pub fn is_default_color(&self) -> bool {
        self.role.color_raw() == DEFAULT_COLOR_RAW
    }
    pub fn can_interact(&self) -> bool {
        self.can_interact
    }
    pub fn get_id(&self) -> String {
        self.role.id.to_string()
    }
    pub(crate) fn id_number(&self) -> u64 {
        self.role.id
    }
}

/// `InteractionMember`.
#[derive(Clone)]
pub struct InteractionMember {
    member: Member,
    roles: Vec<Role>,
    admin: bool,
    discord: Discord,
}

impl std::fmt::Debug for InteractionMember {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractionMember").field("id", &self.member.user.id).field("name", &self.member.user.name).finish()
    }
}

impl InteractionMember {
    pub fn of(discord: &Discord, guild: &Guild, member: Member) -> Self {
        let roles = guild.roles_of(&member).into_iter().cloned().collect();
        let admin = guild.permissions_of(&member) & crate::client::model::ADMINISTRATOR != 0;
        Self { member, roles, admin, discord: discord.clone() }
    }
    pub fn get_name(&self) -> &str {
        &self.member.user.name
    }
    pub fn get_discriminator(&self) -> &str {
        &self.member.user.discriminator
    }
    /// `getTag`.
    pub fn get_tag(&self) -> String {
        self.member.user.as_tag()
    }
    /// `getAsMention`.
    pub fn get_as_mention(&self) -> String {
        format!("<@{}>", self.member.user.id)
    }
    pub fn get_effective_name(&self) -> &str {
        self.member.effective_name()
    }
    pub fn get_nickname(&self) -> Option<&str> {
        self.member.nick.as_deref()
    }
    pub fn get_id(&self) -> String {
        self.member.user.id.to_string()
    }
    pub(crate) fn id_number(&self) -> u64 {
        self.member.user.id
    }
    pub fn is_admin(&self) -> bool {
        self.admin
    }
    /// `hasRoles`.
    pub fn has_roles(&self, role_definitions: &[String]) -> bool {
        has_roles(self.admin, self.member.user.id, &self.roles, role_definitions)
    }
    /// `hasRole`.
    pub fn has_role(&self, role_id: &str) -> bool {
        self.roles.iter().any(|role| role.id.to_string() == role_id)
    }
    pub(crate) fn role_ids(&self) -> Vec<u64> {
        self.roles.iter().map(|role| role.id).collect()
    }
    /// `sendPrivateMessage`.
    pub fn send_private_message(&self, content: &str) -> Pending<bool> {
        self.discord.send_private_message(self.member.user.id, content)
    }
}

/// An edit that waits for the interaction's first answer: a reply can't pass the defer.
#[derive(Default)]
struct Hook {
    deferred: bool,
    waiting: Vec<Value>,
}

/// `InteractionEvent`.
#[derive(Clone)]
pub struct InteractionEvent(Arc<EventInner>);

struct EventInner {
    discord: Discord,
    interaction: Interaction,
    member: InteractionMember,
    reply_buffer: Mutex<Vec<String>>,
    hook: Mutex<Hook>,
    host: Arc<dyn Host>,
}

impl InteractionEvent {
    /// After `deferReply`: `deferred` completes when Discord has answered it.
    pub(crate) fn new(discord: Discord, interaction: Interaction, member: InteractionMember, deferred: Pending<crate::client::rest::Answer>, host: Arc<dyn Host>) -> Self {
        let event = Self(Arc::new(EventInner { discord, interaction, member, reply_buffer: Mutex::new(Vec::new()), hook: Mutex::new(Hook::default()), host }));
        let this = event.clone();
        deferred.then(move |answer| {
            if let Err(error) = answer {
                host::log(&this.0.host, Level::Severe, &format!("Error while deferring Discord command: {error}"));
            }
            let waiting = {
                let mut hook = this.0.hook.lock().unwrap_or_else(|e| e.into_inner());
                hook.deferred = true;
                std::mem::take(&mut hook.waiting)
            };
            for body in waiting {
                this.edit(body);
            }
        });
        event
    }
    fn edit(&self, body: Value) {
        let host = self.0.host.clone();
        self.0.discord.edit_original(&self.0.interaction, body).then(move |answer| {
            if let Err(error) = answer {
                host::log(&host, Level::Severe, &format!("Error while editing command interaction response: {error}"));
            }
        });
    }
    /// `reply`: the buffer so far, as one message.
    pub fn reply(&self, message: &str) {
        let content = {
            let mut buffer = self.0.reply_buffer.lock().unwrap_or_else(|e| e.into_inner());
            buffer.push(strip_format(message).replace('\u{a7}', ""));
            let joined = buffer.join("\n");
            String::from_utf16_lossy(&joined.encode_utf16().take(2000).collect::<Vec<u16>>())
        };
        let body = json!({ "content": content, "allowed_mentions": crate::client::model::Mentions::NoGroups.to_json() });
        {
            let mut hook = self.0.hook.lock().unwrap_or_else(|e| e.into_inner());
            if !hook.deferred {
                hook.waiting.push(body);
                return;
            }
        }
        self.edit(body);
    }
    /// `replyTl`.
    pub fn reply_tl(&self, key: &str, args: &[Arg]) {
        self.reply(&mini_to_legacy(&tl_literal(key, args)));
    }
    pub fn get_member(&self) -> &InteractionMember {
        &self.0.member
    }
    /// `getStringArgument`.
    pub fn get_string_argument(&self, key: &str) -> Option<String> {
        self.0.interaction.option(key).map(|value| value.as_str().map_or_else(|| value.to_string(), str::to_owned))
    }
    pub fn get_integer_argument(&self, key: &str) -> Option<i64> {
        self.0.interaction.option(key).and_then(Value::as_i64)
    }
    pub fn get_boolean_argument(&self, key: &str) -> Option<bool> {
        self.0.interaction.option(key).and_then(Value::as_bool)
    }
    fn resolved(&self, table: &str, key: &str) -> Option<&Value> {
        let id = self.0.interaction.option(key)?;
        self.0.interaction.resolved[table].get(id.as_str()?)
    }
    /// `getUserArgument`.
    pub fn get_user_argument(&self, key: &str) -> Option<InteractionMember> {
        let user = crate::client::model::User::from_json(self.resolved("users", key)?)?;
        let member = Member::from_json(self.resolved("members", key)?, Some(&user))?;
        let guild = self.0.discord.guild(self.0.interaction.guild_id?)?;
        Some(InteractionMember::of(&self.0.discord, &guild, member))
    }
    /// `getChannelArgument`.
    pub fn get_channel_argument(&self, key: &str) -> Option<InteractionChannel> {
        Channel::from_json(self.resolved("channels", key)?).map(|channel| InteractionChannel::of(&channel))
    }
    /// `getRoleArgument`.
    pub fn get_role_argument(&self, key: &str) -> Option<InteractionRole> {
        let role = Role::from_json(self.resolved("roles", key)?)?;
        let guild = self.0.discord.guild(self.0.interaction.guild_id?)?;
        Some(InteractionRole::of(&guild, &role))
    }
    pub fn get_channel_id(&self) -> String {
        self.0.interaction.channel_id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_checked() {
        assert!(MessageType::new("my-type-1").is_ok());
        assert!(MessageType::new("Bad Key").is_err());
    }

    #[test]
    fn default_types_say_which_are_a_player_s() {
        assert_eq!(default_types::values().len(), 16);
        assert!(default_types::CHAT.is_player());
        assert!(!default_types::KICK.is_player());
        assert_eq!(default_types::FIRST_JOIN.get_key(), "first-join");
    }

    #[test]
    fn option_types_are_discord_s() {
        assert_eq!(InteractionCommandArgumentType::String.option_type(), 3);
        assert_eq!(InteractionCommandArgumentType::Role.option_type(), 8);
    }
}
