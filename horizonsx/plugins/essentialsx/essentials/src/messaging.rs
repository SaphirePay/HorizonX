//! `SimpleMessageRecipient`: `/msg` and `/r` between users and the console.
use crate::prelude::*;
use crate::command_source::console_display_name;
use std::sync::{Arc, Mutex};

/// `IMessageRecipient`, for one that's neither a player nor the console: someone on Discord.
pub trait RemoteRecipient: Send + Sync {
    fn get_name(&self) -> String;
    fn get_display_name(&self) -> String;
    fn is_reachable(&self) -> bool;
    fn on_receive_message(&self, sender: &Recipient, message: &str) -> MessageResponse;
}

/// A `RemoteRecipient`, compared by which one it is.
#[derive(Clone)]
pub struct Remote(pub Arc<dyn RemoteRecipient>);

impl std::fmt::Debug for Remote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Remote").field(&self.0.get_name()).finish()
    }
}

impl PartialEq for Remote {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Recipient {
    User(User),
    Console,
    Remote(Remote),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageResponse {
    Success,
    SuccessButAfk,
    Unreachable,
    MessagesIgnored,
    SenderIgnored,
    EventCancelled,
}

impl MessageResponse {
    /// `isSuccess`.
    pub fn is_success(self) -> bool {
        matches!(self, Self::Success | Self::SuccessButAfk)
    }
}

#[derive(Default)]
pub struct RecipientState {
    last_message_ms: i64,
    reply_recipient: Option<Recipient>,
}

/// The console's own `SimpleMessageRecipient`.
static CONSOLE: Mutex<Option<RecipientState>> = Mutex::new(None);

impl Recipient {
    fn with_state<R>(&self, read: impl FnOnce(&mut RecipientState) -> R) -> R {
        match self {
            Self::User(user) => read(&mut user.state().recipient),
            Self::Console | Self::Remote(_) => read(CONSOLE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default()),
        }
    }
    pub fn send_tl(&self, key: &str, args: &[Arg]) {
        match self {
            Self::User(user) => user.send_tl(key, args),
            Self::Console => CommandSource::console().send_tl(key, args),
            Self::Remote(_) => {}
        }
    }
    pub fn tl_sender(&self, key: &str, args: &[Arg]) -> String {
        match self {
            Self::User(user) => user.player_tl(key, args),
            Self::Console => tl_literal(key, args),
            Self::Remote(_) => String::new(),
        }
    }
    pub fn get_name(&self) -> String {
        match self {
            Self::User(user) => user.get_name(),
            Self::Console => "Console".to_owned(),
            Self::Remote(remote) => remote.0.get_name(),
        }
    }
    pub fn get_display_name(&self) -> String {
        match self {
            Self::User(user) => user.get_display_name(),
            Self::Console => console_display_name(),
            Self::Remote(remote) => remote.0.get_display_name(),
        }
    }
    /// `isReachable`.
    pub fn is_reachable(&self) -> bool {
        match self {
            Self::User(user) => user.is_online(),
            Self::Console => true,
            Self::Remote(remote) => remote.0.is_reachable(),
        }
    }
    /// `getReplyRecipient`.
    pub fn get_reply_recipient(&self) -> Option<Self> {
        if matches!(self, Self::Remote(_)) {
            return None;
        }
        self.with_state(|state| state.reply_recipient.clone())
    }
    /// `setReplyRecipient`.
    pub fn set_reply_recipient(&self, recipient: Option<Self>) {
        if matches!(self, Self::Remote(_)) {
            return;
        }
        self.with_state(|state| state.reply_recipient = recipient);
    }
    fn user(&self) -> Option<&User> {
        match self {
            Self::User(user) => Some(user),
            Self::Console | Self::Remote(_) => None,
        }
    }
    /// `sendMessage(recipient, message)`.
    pub fn send_private(&self, recipient: &Self, message: &str) -> MessageResponse {
        let response = recipient.on_receive_message(self, message);
        match response {
            MessageResponse::Unreachable => self.send_tl("recentlyForeverAlone", crate::args![recipient.get_display_name()]),
            MessageResponse::MessagesIgnored => self.send_tl("msgIgnore", crate::args![recipient.get_display_name()]),
            MessageResponse::SenderIgnored | MessageResponse::EventCancelled => {}
            MessageResponse::Success | MessageResponse::SuccessButAfk => {
                if response == MessageResponse::SuccessButAfk {
                    match recipient.user().and_then(User::get_afk_message) {
                        Some(afk) => self.send_tl("userAFKWithMessage", crate::args![recipient.get_display_name(), afk]),
                        None => self.send_tl("userAFK", crate::args![recipient.get_display_name()]),
                    }
                }
                self.send_tl("msgFormat", &[parsed(self.tl_sender("meSender", &[])), Arg::from(recipient.get_display_name()), Arg::from(message)]);
                if settings().is_social_spy_messages()
                    && let (Some(sender), Some(target)) = (self.user(), recipient.user())
                    && !sender.is_authorized("essentials.chat.spy.exempt")
                    && !target.is_authorized("essentials.chat.spy.exempt")
                {
                    let names = settings().is_social_spy_display_names();
                    let sender_name = if names { self.get_display_name() } else { self.get_name() };
                    let target_name = if names { recipient.get_display_name() } else { recipient.get_name() };
                    for online in ess().get_online_users() {
                        if online.is_social_spy_enabled() && online != *sender && online != *target {
                            let prefix = if sender.is_muted() && settings().get_social_spy_listen_muted_players() { self.tl_sender("socialSpyMutedPrefix", &[]) } else { tl_literal("socialSpyPrefix", &[]) };
                            online.send_component(&deserialize_mini_message(&format!("{prefix}{}", tl_literal("socialSpyMsgFormat", crate::args![sender_name.clone(), target_name.clone(), message]))));
                        }
                    }
                }
            }
        }
        if response.is_success() {
            self.set_reply_recipient(Some(recipient.clone()));
        }
        crate::hooks::fire(crate::hooks::Hook::PrivateMessage { sender: self, recipient, message });
        response
    }
    /// `onReceiveMessage`.
    pub fn on_receive_message(&self, sender: &Self, message: &str) -> MessageResponse {
        if let Self::Remote(remote) = self {
            return remote.0.on_receive_message(sender, message);
        }
        if !self.is_reachable() {
            return MessageResponse::Unreachable;
        }
        let mut afk = false;
        let mut last_reply = settings().is_last_message_reply_recipient();
        if let Self::User(user) = self {
            if user.is_ignore_msg() && sender.user().is_some_and(|from| !from.is_authorized("essentials.msgtoggle.bypass")) {
                return MessageResponse::MessagesIgnored;
            }
            afk = user.is_afk();
            last_reply = user.is_last_message_reply_recipient();
            if sender.user().is_some_and(|from| user.is_ignored_player(from)) {
                return MessageResponse::SenderIgnored;
            }
        }
        self.send_tl("msgFormat", &[Arg::from(sender.get_display_name()), parsed(self.tl_sender("meRecipient", &[])), Arg::from(message)]);
        if last_reply {
            let timeout = settings().get_last_message_reply_recipient_timeout() * 1000;
            let stale = self.get_reply_recipient().is_none_or(|known| !known.is_reachable()) || now_ms() - self.with_state(|state| state.last_message_ms) > timeout;
            if stale {
                self.set_reply_recipient(Some(sender.clone()));
            }
        } else {
            self.set_reply_recipient(Some(sender.clone()));
        }
        self.with_state(|state| state.last_message_ms = now_ms());
        if afk { MessageResponse::SuccessButAfk } else { MessageResponse::Success }
    }
}
