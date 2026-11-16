//! Essentials' own events, for the modules linked into this library: `AfkStatusChangeEvent`,
//! `MuteStatusChangeEvent` and the rest. Java raises them for any plugin. Another library can't hear
//! these, and none of the listeners here cancels, so each is a call and no more.
use crate::prelude::*;
use crate::messaging::Recipient;
use std::sync::Mutex;

/// `ChatType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatType {
    Unknown,
    Local,
    Question,
    Shout,
}

#[derive(Clone, Copy, Debug)]
pub enum Hook<'a> {
    /// `AfkStatusChangeEvent`.
    Afk { user: &'a User, value: bool },
    /// `MuteStatusChangeEvent`: `timestamp` only for a mute with an end.
    Mute { affected: &'a User, controller: Option<&'a User>, value: bool, timestamp: Option<i64>, reason: Option<&'a str> },
    /// `VanishStatusChangeEvent`, for the player vanishing.
    Vanish { user: &'a User, value: bool },
    /// `PrivateMessageSentEvent`.
    PrivateMessage { sender: &'a Recipient, recipient: &'a Recipient, message: &'a str },
    /// `UserActionEvent`.
    UserAction { user: &'a User, message: &'a str },
    /// `UserMailEvent`.
    Mail { recipient: &'a User, sender_name: Option<&'a str>, message: &'a str },
    /// `AsyncUserDataLoadEvent`, on the main thread here.
    UserDataLoad { user: &'a User, join_message: Option<&'a str> },
    /// `LocalChatEvent` and `GlobalChatEvent`, from EssentialsChat.
    Chat { player: &'a Player, message: &'a str, chat_type: ChatType },
}

type Listener = fn(&Hook<'_>);

static LISTENERS: Mutex<Vec<Listener>> = Mutex::new(Vec::new());

/// Hears every hook from now on. `Plugin.registerEvents`.
pub fn listen(listener: Listener) {
    LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).push(listener);
}

/// `PluginManager.callEvent`.
pub fn fire(hook: Hook<'_>) {
    let listeners = LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for listener in listeners {
        listener(&hook);
    }
}
