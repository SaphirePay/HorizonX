//! `MailServiceImpl`: mail lives on the recipient, in `userdata`.
use crate::prelude::*;
use crate::config_entities::MailMessage;
use chrono::TimeZone;

/// `MailSender`: who a mail says it's from.
#[derive(Clone, Debug)]
pub struct MailSender {
    pub name: String,
    pub uuid: Option<String>,
}

impl MailSender {
    pub fn of_user(user: &User) -> Self {
        Self { name: user.get_name(), uuid: Some(user.get_uuid()) }
    }
    pub fn console() -> Self {
        Self { name: crate::command_source::console_display_name(), uuid: None }
    }
}

fn store(recipient: &User, message: MailMessage) {
    crate::hooks::fire(crate::hooks::Hook::Mail { recipient, sender_name: message.sender_name.as_deref(), message: &message.message });
    let mut messages = recipient.get_mail_messages();
    messages.insert(0, message);
    recipient.set_mail_list(messages);
}

/// `sendMail`.
pub fn send_mail(recipient: &User, sender: &MailSender, message: &str, expire_at: i64) {
    store(recipient, MailMessage { read: false, legacy: false, sender_name: Some(sender.name.clone()), sender_uuid: sender.uuid.clone(), timestamp: now_ms(), expire: expire_at, message: message.to_owned() });
}

/// `sendLegacyMail`.
pub fn send_legacy_mail(recipient: &User, message: &str) {
    store(recipient, MailMessage { read: false, legacy: true, sender_name: None, sender_uuid: None, timestamp: 0, expire: 0, message: message.to_owned() });
}

fn date(time: i64) -> String {
    chrono::Local.timestamp_millis_opt(time).single().map_or_else(String::new, |date| date.format("%Y/%m/%d %H:%M").to_string())
}

/// `getMailTlKey`.
pub fn get_mail_tl_key(message: &MailMessage) -> String {
    if message.legacy {
        return "mailMessage".to_owned();
    }
    format!("{}{}", if message.read { "mailFormatNewRead" } else { "mailFormatNew" }, if message.expire != 0 { "Timed" } else { "" })
}

/// `getMailTlArgs`.
pub fn get_mail_tl_args(message: &MailMessage) -> Vec<Arg> {
    if message.legacy {
        return vec![Arg::from(message.message.clone())];
    }
    vec![Arg::from(date(message.timestamp)), Arg::from(message.sender_name.clone().unwrap_or_default()), Arg::from(message.message.clone())]
}

/// `getMailLine`.
pub fn get_mail_line(message: &MailMessage) -> String {
    tl_literal(&get_mail_tl_key(message), &get_mail_tl_args(message))
}
