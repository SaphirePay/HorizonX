//! `DiscordMessageRecipient`: someone on Discord, as `/r` sees them after a `/msg` from there.
use essentials::prelude::*;
use crate::api::InteractionMember;
use crate::message_util::sanitize_discord_markdown;
use essentials::messaging::{MessageResponse, Recipient, RemoteRecipient};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct DiscordMessageRecipient {
    member: InteractionMember,
    died: Arc<AtomicBool>,
}

impl DiscordMessageRecipient {
    pub fn new(member: InteractionMember) -> Self {
        Self { member, died: Arc::new(AtomicBool::new(false)) }
    }
}

impl RemoteRecipient for DiscordMessageRecipient {
    fn get_name(&self) -> String {
        self.member.get_tag()
    }
    fn get_display_name(&self) -> String {
        self.member.get_tag()
    }
    fn is_reachable(&self) -> bool {
        !self.died.load(Ordering::SeqCst)
    }
    fn on_receive_message(&self, sender: &Recipient, message: &str) -> MessageResponse {
        if self.died.load(Ordering::SeqCst) {
            sender.set_reply_recipient(None);
            return MessageResponse::Unreachable;
        }
        let clean_message = sanitize_discord_markdown(&strip_format(message));
        let died = self.died.clone();
        self.member.send_private_message(&tl_literal("replyFromDiscord", essentials::args![sender.get_name(), clean_message])).then(move |success| {
            if !success {
                died.store(true, Ordering::SeqCst);
            }
        });
        MessageResponse::Success
    }
}
