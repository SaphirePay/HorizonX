//! `CommandSource`: a sender, with translations in its own language.
use crate::prelude::*;
use horizon_bukkit::{CommandSender, Component, get_console_sender};

#[derive(Clone, Debug, PartialEq)]
pub struct CommandSource {
    sender: CommandSender,
}

impl CommandSource {
    pub fn new(sender: CommandSender) -> Self {
        Self { sender }
    }
    pub fn console() -> Self {
        Self::new(get_console_sender())
    }
    pub fn of_player(player: Player) -> Self {
        Self::new(CommandSender::Player(player))
    }
    pub fn get_sender(&self) -> &CommandSender {
        &self.sender
    }
    pub fn get_player(&self) -> Option<&Player> {
        self.sender.as_player()
    }
    pub fn is_player(&self) -> bool {
        self.get_player().is_some()
    }
    /// `getUser`.
    pub fn get_user(&self) -> Option<User> {
        self.get_player().and_then(|player| ess().get_user(player))
    }
    /// `sendTl`.
    pub fn send_tl(&self, key: &str, args: &[Arg]) {
        if let Some(user) = self.get_user() {
            user.send_tl(key, args);
            return;
        }
        let translation = tl_literal(key, args);
        if !translation.is_empty() {
            self.send_component(&deserialize_mini_message(&translation));
        }
    }
    /// `tl`.
    pub fn tl(&self, key: &str, args: &[Arg]) -> String {
        match self.get_user() {
            Some(user) => user.player_tl(key, args),
            None => tl_literal(key, args),
        }
    }
    /// `tlComponent`.
    pub fn tl_component(&self, key: &str, args: &[Arg]) -> Component {
        deserialize_mini_message(&self.tl(key, args))
    }
    /// `sendComponent`.
    pub fn send_component(&self, component: &Component) {
        self.sender.send_component(component);
    }
    /// `sendMessage`.
    pub fn send_message(&self, message: &str) {
        if !message.is_empty() {
            self.sender.send_message(message);
        }
    }
    /// `isAuthorized`.
    pub fn is_authorized(&self, node: &str) -> bool {
        self.get_user().is_none_or(|user| user.is_authorized(node))
    }
    /// `getSelfSelector`.
    pub fn get_self_selector(&self) -> String {
        self.get_player().map_or_else(|| "*".to_owned(), |player| player.get_name().to_owned())
    }
    /// `getDisplayName`.
    pub fn get_display_name(&self) -> String {
        match self.get_player() {
            Some(player) => player.display_name().to_legacy(),
            None => self.sender.get_name().to_owned(),
        }
    }
}

/// `Console.displayName`.
pub fn console_display_name() -> String {
    tl_literal("consoleName", &[])
}
