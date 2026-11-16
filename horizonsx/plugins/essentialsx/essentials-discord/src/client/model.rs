//! Discord's objects as JDA shows them, and what's worked out from them: permissions, role order and
//! a message's display text.
use regex::Regex;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub const ADMINISTRATOR: u64 = 1 << 3;
pub const VIEW_CHANNEL: u64 = 1 << 10;
pub const MESSAGE_SEND: u64 = 1 << 11;
pub const MESSAGE_EMBED_LINKS: u64 = 1 << 14;
pub const NICKNAME_MANAGE: u64 = 1 << 27;
pub const MANAGE_ROLES: u64 = 1 << 28;
pub const MANAGE_WEBHOOKS: u64 = 1 << 29;
pub const ALL_PERMISSIONS: u64 = u64::MAX;

/// `Role.DEFAULT_COLOR_RAW`.
pub const DEFAULT_COLOR_RAW: i32 = 0x1FFF_FFFF;

pub const CHANNEL_TEXT: u8 = 0;

pub(crate) fn snowflake(value: &Value) -> Option<u64> {
    value.as_str().and_then(|text| text.parse().ok()).or_else(|| value.as_u64())
}

fn snowflakes(value: &Value) -> Vec<u64> {
    value.as_array().into_iter().flatten().filter_map(snowflake).collect()
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn bits(value: &Value) -> u64 {
    value.as_str().and_then(|text| text.parse().ok()).or_else(|| value.as_u64()).unwrap_or(0)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: u64,
    pub name: String,
    pub discriminator: String,
    pub global_name: Option<String>,
    pub avatar: Option<String>,
    pub bot: bool,
}

impl User {
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            id: snowflake(&value["id"])?,
            name: text(&value["username"]),
            discriminator: format!("{:0>4}", value["discriminator"].as_str().unwrap_or("0")),
            global_name: value["global_name"].as_str().map(str::to_owned),
            avatar: value["avatar"].as_str().map(str::to_owned),
            bot: value["bot"].as_bool().unwrap_or(false),
        })
    }
    /// `getAsTag`.
    pub fn as_tag(&self) -> String {
        format!("{}#{}", self.name, self.discriminator)
    }
    /// `getEffectiveName`.
    pub fn effective_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.name)
    }
    /// `getAvatarUrl`.
    pub fn avatar_url(&self) -> Option<String> {
        let avatar = self.avatar.as_ref()?;
        let kind = if avatar.starts_with("a_") { "gif" } else { "png" };
        Some(format!("https://cdn.discordapp.com/avatars/{}/{avatar}.{kind}", self.id))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Role {
    pub id: u64,
    pub name: String,
    pub position: i32,
    pub permissions: u64,
    pub color: u32,
    pub managed: bool,
}

impl Role {
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            id: snowflake(&value["id"])?,
            name: text(&value["name"]),
            position: value["position"].as_i64().unwrap_or(0) as i32,
            permissions: bits(&value["permissions"]),
            color: value["color"].as_u64().unwrap_or(0) as u32,
            managed: value["managed"].as_bool().unwrap_or(false),
        })
    }
    /// `getColorRaw`.
    pub fn color_raw(&self) -> i32 {
        if self.color == 0 { DEFAULT_COLOR_RAW } else { self.color as i32 }
    }
    /// `Role.compareTo`, turned round: the first of a sorted list is the highest role.
    pub fn top_first(&self, other: &Self) -> Ordering {
        other.position.cmp(&self.position).then(self.id.cmp(&other.id))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overwrite {
    pub id: u64,
    pub member: bool,
    pub allow: u64,
    pub deny: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    pub id: u64,
    pub name: String,
    pub kind: u8,
    pub position: i32,
    pub overwrites: Vec<Overwrite>,
}

impl Channel {
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            id: snowflake(&value["id"])?,
            name: text(&value["name"]),
            kind: value["type"].as_u64().unwrap_or(0) as u8,
            position: value["position"].as_i64().unwrap_or(0) as i32,
            overwrites: value["permission_overwrites"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|row| Some(Overwrite { id: snowflake(&row["id"])?, member: row["type"].as_u64() == Some(1), allow: bits(&row["allow"]), deny: bits(&row["deny"]) }))
                .collect(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emoji {
    pub id: u64,
    pub name: String,
    pub animated: bool,
}

impl Emoji {
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Self { id: snowflake(&value["id"])?, name: text(&value["name"]), animated: value["animated"].as_bool().unwrap_or(false) })
    }
    /// `getAsMention`.
    pub fn as_mention(&self) -> String {
        format!("<{}:{}:{}>", if self.animated { "a" } else { "" }, self.name, self.id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub user: User,
    pub nick: Option<String>,
    pub role_ids: Vec<u64>,
}

impl Member {
    /// A guild member object. Those in a message have no `user`: it's the author, passed here.
    pub fn from_json(value: &Value, user: Option<&User>) -> Option<Self> {
        let user = user.cloned().or_else(|| User::from_json(&value["user"]))?;
        Some(Self { user, nick: value["nick"].as_str().map(str::to_owned), role_ids: snowflakes(&value["roles"]) })
    }
    /// `getEffectiveName`.
    pub fn effective_name(&self) -> &str {
        self.nick.as_deref().unwrap_or_else(|| self.user.effective_name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: u64,
    pub channel_id: u64,
    pub guild_id: Option<u64>,
    pub author: User,
    pub member: Option<Member>,
    /// `getContentRaw`.
    pub content: String,
    pub webhook_id: Option<u64>,
    pub attachments: Vec<String>,
    /// Those mentioned, with their nicknames.
    pub mentions: Vec<(User, Option<String>)>,
    pub mention_roles: Vec<u64>,
}

static CHANNEL_MENTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<#(\d+)>").expect("regex"));
static EMOJI_MENTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<a?:([a-zA-Z0-9_]+):\d+>").expect("regex"));

impl Message {
    pub fn from_json(value: &Value) -> Option<Self> {
        let author = User::from_json(&value["author"])?;
        Some(Self {
            id: snowflake(&value["id"])?,
            channel_id: snowflake(&value["channel_id"])?,
            guild_id: snowflake(&value["guild_id"]),
            member: Member::from_json(&value["member"], Some(&author)).filter(|_| value["member"].is_object()),
            author,
            content: text(&value["content"]),
            webhook_id: snowflake(&value["webhook_id"]),
            attachments: value["attachments"].as_array().into_iter().flatten().filter_map(|row| row["url"].as_str().map(str::to_owned)).collect(),
            mentions: value["mentions"].as_array().into_iter().flatten().filter_map(|row| Some((User::from_json(row)?, row["member"]["nick"].as_str().map(str::to_owned)))).collect(),
            mention_roles: snowflakes(&value["mention_roles"]),
        })
    }
    /// `isWebhookMessage`.
    pub fn is_webhook(&self) -> bool {
        self.webhook_id.is_some()
    }
    /// `getContentDisplay`: mentions as they read in the client.
    pub fn content_display(&self, guild: Option<&Guild>) -> String {
        let mut out = self.content.clone();
        for (user, nick) in &self.mentions {
            let name = nick.as_deref().unwrap_or_else(|| user.effective_name());
            out = out.replace(&format!("<@{}>", user.id), &format!("@{name}")).replace(&format!("<@!{}>", user.id), &format!("@{name}"));
        }
        out = EMOJI_MENTION.replace_all(&out, ":$1:").into_owned();
        if let Some(guild) = guild {
            out = CHANNEL_MENTION
                .replace_all(&out, |found: &regex::Captures<'_>| match found[1].parse().ok().and_then(|id: u64| guild.channels.get(&id)) {
                    Some(channel) => format!("#{}", channel.name),
                    None => found[0].to_owned(),
                })
                .into_owned();
            for id in &self.mention_roles {
                if let Some(role) = guild.roles.get(id) {
                    out = out.replace(&format!("<@&{id}>"), &format!("@{}", role.name));
                }
            }
        }
        out
    }
}

/// A guild as the gateway keeps it current.
#[derive(Clone, Debug, Default)]
pub struct Guild {
    pub id: u64,
    pub name: String,
    pub owner_id: u64,
    pub roles: BTreeMap<u64, Role>,
    pub channels: BTreeMap<u64, Channel>,
    pub emojis: Vec<Emoji>,
    /// `getSelfMember`.
    pub self_member: Option<Member>,
}

impl Guild {
    pub fn from_json(value: &Value, self_id: Option<u64>) -> Option<Self> {
        let mut guild = Self { id: snowflake(&value["id"])?, ..Self::default() };
        guild.update(value);
        guild.roles = value["roles"].as_array().into_iter().flatten().filter_map(Role::from_json).map(|role| (role.id, role)).collect();
        guild.channels = value["channels"].as_array().into_iter().flatten().filter_map(Channel::from_json).map(|channel| (channel.id, channel)).collect();
        guild.emojis = value["emojis"].as_array().into_iter().flatten().filter_map(Emoji::from_json).collect();
        guild.self_member = value["members"].as_array().into_iter().flatten().filter_map(|row| Member::from_json(row, None)).find(|member| Some(member.user.id) == self_id);
        Some(guild)
    }
    /// `GUILD_UPDATE`: what it carries of what's kept.
    pub fn update(&mut self, value: &Value) {
        if let Some(name) = value["name"].as_str() {
            self.name = name.to_owned();
        }
        if let Some(owner) = snowflake(&value["owner_id"]) {
            self.owner_id = owner;
        }
    }
    /// `getTextChannelById`: news channels are another type in JDA, and threads aren't kept.
    pub fn text_channel(&self, id: u64) -> Option<&Channel> {
        self.channels.get(&id).filter(|channel| channel.kind == CHANNEL_TEXT)
    }
    /// `getRoles`: the highest first, `@everyone` left out.
    pub fn roles_of(&self, member: &Member) -> Vec<&Role> {
        let mut roles: Vec<&Role> = member.role_ids.iter().filter(|id| **id != self.id).filter_map(|id| self.roles.get(id)).collect();
        roles.sort_by(|a, b| a.top_first(b));
        roles
    }
    /// `Member.getPermissions`.
    pub fn permissions_of(&self, member: &Member) -> u64 {
        if member.user.id == self.owner_id {
            return ALL_PERMISSIONS;
        }
        let everyone = self.roles.get(&self.id).map_or(0, |role| role.permissions);
        let all = member.role_ids.iter().filter_map(|id| self.roles.get(id)).fold(everyone, |sum, role| sum | role.permissions);
        if all & ADMINISTRATOR != 0 { ALL_PERMISSIONS } else { all }
    }
    /// `Member.getPermissions(channel)`: the role, then the roles', then the member's own overwrites.
    pub fn permissions_in(&self, member: &Member, channel: &Channel) -> u64 {
        let base = self.permissions_of(member);
        if base == ALL_PERMISSIONS {
            return base;
        }
        let mut allowed = base;
        if let Some(everyone) = channel.overwrites.iter().find(|row| !row.member && row.id == self.id) {
            allowed = (allowed & !everyone.deny) | everyone.allow;
        }
        let (mut allow, mut deny) = (0, 0);
        for row in channel.overwrites.iter().filter(|row| !row.member && member.role_ids.contains(&row.id)) {
            allow |= row.allow;
            deny |= row.deny;
        }
        allowed = (allowed & !deny) | allow;
        if let Some(own) = channel.overwrites.iter().find(|row| row.member && row.id == member.user.id) {
            allowed = (allowed & !own.deny) | own.allow;
        }
        allowed
    }
    /// `Role.hasPermission(channel, ...)` for `@everyone`.
    fn everyone_can_view(&self, channel: &Channel) -> bool {
        let base = self.roles.get(&self.id).map_or(0, |role| role.permissions);
        if base & ADMINISTRATOR != 0 {
            return true;
        }
        let allowed = channel.overwrites.iter().find(|row| !row.member && row.id == self.id).map_or(base, |row| (base & !row.deny) | row.allow);
        allowed & VIEW_CHANNEL != 0
    }
    /// `getDefaultChannel`: text and news alike, the first `@everyone` can see.
    pub fn default_channel(&self) -> Option<&Channel> {
        self.channels.values().filter(|channel| channel.kind == CHANNEL_TEXT || channel.kind == 5).filter(|channel| self.everyone_can_view(channel)).min_by_key(|channel| (channel.position, channel.id))
    }
    /// `canTalk`, for the bot.
    pub fn can_talk(&self, channel: &Channel) -> bool {
        self.self_member.as_ref().is_some_and(|me| self.permissions_in(me, channel) & (VIEW_CHANNEL | MESSAGE_SEND) == VIEW_CHANNEL | MESSAGE_SEND)
    }
    /// `getSelfMember().hasPermission(...)`.
    pub fn self_has(&self, permission: u64) -> bool {
        self.self_member.as_ref().is_some_and(|me| self.permissions_of(me) & permission == permission)
    }
    /// `getSelfMember().hasPermission(channel, ...)`.
    pub fn self_has_in(&self, channel: &Channel, permission: u64) -> bool {
        self.self_member.as_ref().is_some_and(|me| self.permissions_in(me, channel) & permission == permission)
    }
    /// `getSelfMember().canInteract(role)`.
    pub fn can_interact(&self, role: &Role) -> bool {
        let Some(me) = &self.self_member else { return false };
        me.user.id == self.owner_id || self.roles_of(me).first().is_some_and(|top| top.top_first(role) == Ordering::Less)
    }
}

/// A webhook: `Channel.retrieveWebhooks`.
#[derive(Clone, Debug)]
pub struct Webhook {
    pub id: u64,
    pub name: String,
    pub token: Option<super::secret::Secret>,
}

impl Webhook {
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Self { id: snowflake(&value["id"])?, name: text(&value["name"]), token: value["token"].as_str().map(super::secret::Secret::new) })
    }
}

/// `Message.MentionType`, as `allowed_mentions` takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mentions {
    /// JDA's default, and `AllowedMentions.all()`.
    All,
    /// `NO_GROUP_MENTIONS`: users, and no `@everyone` or roles.
    NoGroups,
}

impl Mentions {
    pub fn to_json(self) -> Value {
        match self {
            Self::All => serde_json::json!({ "parse": ["users", "roles", "everyone"] }),
            Self::NoGroups => serde_json::json!({ "parse": ["users"] }),
        }
    }
}

/// One option of a slash command, with `resolved`'s objects for its id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interaction {
    pub id: u64,
    pub token: super::secret::SecretEq,
    pub application_id: u64,
    pub guild_id: Option<u64>,
    pub channel_id: u64,
    pub member: Option<Member>,
    /// What `member` may do where this came from, as Discord sent it.
    pub member_permissions: u64,
    pub name: String,
    pub options: Vec<(String, Value)>,
    pub resolved: Value,
}

impl Interaction {
    /// An `APPLICATION_COMMAND`; other kinds aren't used.
    pub fn from_json(value: &Value) -> Option<Self> {
        if value["type"].as_u64() != Some(2) {
            return None;
        }
        Some(Self {
            id: snowflake(&value["id"])?,
            token: super::secret::SecretEq(super::secret::Secret::new(value["token"].as_str()?)),
            application_id: snowflake(&value["application_id"]).unwrap_or(0),
            guild_id: snowflake(&value["guild_id"]),
            channel_id: snowflake(&value["channel_id"])?,
            member: Member::from_json(&value["member"], None),
            member_permissions: bits(&value["member"]["permissions"]),
            name: text(&value["data"]["name"]),
            options: value["data"]["options"].as_array().into_iter().flatten().filter_map(|row| Some((row["name"].as_str()?.to_owned(), row["value"].clone()))).collect(),
            resolved: value["data"]["resolved"].clone(),
        })
    }
    pub fn option(&self, name: &str) -> Option<&Value> {
        self.options.iter().find(|(key, _)| key == name).map(|(_, value)| value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn guild() -> Guild {
        Guild::from_json(
            &json!({
                "id": "1", "name": "Home", "owner_id": "9",
                "roles": [
                    { "id": "1", "name": "@everyone", "position": 0, "permissions": (VIEW_CHANNEL | MESSAGE_SEND).to_string() },
                    { "id": "2", "name": "Mod", "position": 2, "permissions": MANAGE_ROLES.to_string(), "color": 255 },
                    { "id": "3", "name": "Admin", "position": 3, "permissions": ADMINISTRATOR.to_string() },
                ],
                "channels": [
                    { "id": "10", "name": "general", "type": 0, "position": 1 },
                    { "id": "11", "name": "secret", "type": 0, "position": 0, "permission_overwrites": [
                        { "id": "1", "type": 0, "allow": "0", "deny": VIEW_CHANNEL.to_string() },
                        { "id": "2", "type": 0, "allow": VIEW_CHANNEL.to_string(), "deny": "0" },
                    ] },
                    { "id": "12", "name": "voice", "type": 2, "position": 0 },
                ],
                "members": [{ "user": { "id": "5", "username": "bot", "discriminator": "0", "bot": true }, "roles": ["2"] }],
            }),
            Some(5),
        )
        .unwrap()
    }

    fn member(id: u64, roles: &[u64]) -> Member {
        Member { user: User { id, name: format!("u{id}"), discriminator: "0000".to_owned(), global_name: None, avatar: None, bot: false }, nick: None, role_ids: roles.to_vec() }
    }

    #[test]
    fn user_tag_and_names() {
        let user = User::from_json(&json!({ "id": "7", "username": "name", "discriminator": "0", "global_name": "Shown" })).unwrap();
        assert_eq!(user.as_tag(), "name#0000");
        assert_eq!(user.effective_name(), "Shown");
        let old = User::from_json(&json!({ "id": "7", "username": "name", "discriminator": "1234", "avatar": "a_x" })).unwrap();
        assert_eq!(old.as_tag(), "name#1234");
        assert_eq!(old.avatar_url().unwrap(), "https://cdn.discordapp.com/avatars/7/a_x.gif");
    }

    #[test]
    fn roles_sort_highest_first_without_everyone() {
        let guild = guild();
        let names: Vec<&str> = guild.roles_of(&member(6, &[1, 2, 3])).iter().map(|role| role.name.as_str()).collect();
        assert_eq!(names, ["Admin", "Mod"]);
    }

    #[test]
    fn permissions_work_like_the_client() {
        let guild = guild();
        let plain = member(6, &[]);
        let secret = &guild.channels[&11];
        assert_eq!(guild.permissions_of(&plain), VIEW_CHANNEL | MESSAGE_SEND);
        assert_eq!(guild.permissions_in(&plain, secret) & VIEW_CHANNEL, 0);
        assert_ne!(guild.permissions_in(&member(6, &[2]), secret) & VIEW_CHANNEL, 0);
        assert_eq!(guild.permissions_of(&member(6, &[3])), ALL_PERMISSIONS);
        assert_eq!(guild.permissions_of(&member(9, &[])), ALL_PERMISSIONS);
        assert!(guild.can_talk(secret));
        assert!(guild.can_talk(&guild.channels[&10]));
    }

    #[test]
    fn default_channel_is_the_first_everyone_sees() {
        let guild = guild();
        assert_eq!(guild.default_channel().unwrap().name, "general");
        assert!(guild.text_channel(12).is_none());
    }

    #[test]
    fn can_interact_needs_a_higher_role() {
        let guild = guild();
        assert!(!guild.can_interact(&guild.roles[&3]));
        assert!(!guild.can_interact(&guild.roles[&2]));
        assert!(guild.can_interact(&guild.roles[&1]));
    }

    #[test]
    fn content_display_reads_as_the_client_does() {
        let guild = guild();
        let message = Message::from_json(&json!({
            "id": "1", "channel_id": "10", "guild_id": "1",
            "author": { "id": "6", "username": "a", "discriminator": "0" },
            "content": "hi <@7> <@!8> <#10> <@&2> <:wave:55> <a:spin:56> <#404>",
            "mentions": [
                { "id": "7", "username": "seven", "discriminator": "0", "member": { "nick": "Nick" } },
                { "id": "8", "username": "eight", "discriminator": "0", "global_name": "Eight" },
            ],
            "mention_roles": ["2"],
        }))
        .unwrap();
        assert_eq!(message.content_display(Some(&guild)), "hi @Nick @Eight #general @Mod :wave: :spin: <#404>");
    }

    #[test]
    fn a_webhook_message_has_no_member() {
        let message = Message::from_json(&json!({ "id": "1", "channel_id": "2", "webhook_id": "3", "author": { "id": "3", "username": "hook", "discriminator": "0", "bot": true } })).unwrap();
        assert!(message.is_webhook());
        assert!(message.member.is_none());
    }

    #[test]
    fn emoji_mentions() {
        assert_eq!(Emoji { id: 5, name: "x".to_owned(), animated: true }.as_mention(), "<a:x:5>");
        assert_eq!(Emoji { id: 5, name: "x".to_owned(), animated: false }.as_mention(), "<:x:5>");
    }
}
