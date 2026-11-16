//! `LinkBukkitListener`.
use essentials::prelude::*;
use crate::manager::DiscordLinkStatusChangeEvent;
use crate::settings::LinkPolicy;
use crate::{link_manager, link_settings};
use essentials::hooks::{self, Hook};
use essentials_discord::api::{DiscordMessageEvent, default_types};
use essentials_discord::message_util::sanitize_discord_markdown;
use horizon_bukkit::Plugin;
use horizon_bukkit::adventure::legacy;
use horizon_bukkit::enums::event_priority::{HIGH, LOW};
use horizon_bukkit::event::{PlayerCommandPreprocessEvent, PlayerInteractEvent};
use horizon_bukkit::events::AsyncPlayerPreLoginEvent;
use horizon_bukkit::fam_ev_player::AsyncPlayerChatEvent;
use std::sync::Once;

fn invite_url() -> String {
    essentials_discord::service().map_or_else(String::new, |service| service.get_invite_url())
}

/// `/link <code>`, as the messages show it.
fn link_line(code: &str) -> String {
    format!("/link {code}")
}

/// Is this player under the `freeze` policy and not linked?
fn is_frozen_out(uuid: &str) -> bool {
    link_settings().is_some_and(|settings| settings.get_link_policy() == LinkPolicy::Freeze) && link_manager().is_some_and(|manager| !manager.is_uuid_linked(uuid))
}

fn prompt(user: &User) {
    if let Some(manager) = link_manager() {
        let code = manager.code_for(&user.get_uuid());
        user.send_tl("discordLinkLoginPrompt", essentials::args![link_line(&code), invite_url()]);
    }
}

/// `onMail`.
fn on_mail(recipient: &User, sender_name: Option<&str>, message: &str) {
    if !link_settings().is_some_and(|settings| settings.is_relay_mail()) {
        return;
    }
    let (Some(manager), Some(api)) = (link_manager(), essentials_discord::service()) else {
        return;
    };
    let Some(discord_id) = manager.get_discord_id(&recipient.get_uuid()) else { return };
    let line = tl_literal("discordMailLine", essentials::args![sanitize_discord_markdown(sender_name.unwrap_or_default()), sanitize_discord_markdown(&strip_format(message))]);
    api.get_member_by_id(&discord_id).then(move |member| {
        if let Some(member) = member {
            let _ = member.send_private_message(&line);
        }
    });
}

/// `onUserDataLoad`.
fn on_user_data_load(user: &User) {
    if is_frozen_out(&user.get_uuid()) {
        user.set_freeze(true);
        prompt(user);
    }
}

fn on_hook(hook: &Hook<'_>) {
    match *hook {
        Hook::Mail { recipient, sender_name, message } => on_mail(recipient, sender_name, message),
        Hook::UserDataLoad { user, .. } => on_user_data_load(user),
        _ => {}
    }
}

/// `onDiscordMessage`, at `HIGH`: `MessageType` compares by value here.
fn on_discord_message(event: &mut DiscordMessageEvent) {
    if !link_settings().is_some_and(|settings| settings.is_block_unlinked_chat()) || event.message_type != default_types::CHAT {
        return;
    }
    let Some(manager) = link_manager() else { return };
    if event.uuid.as_deref().is_none_or(|uuid| !manager.is_uuid_linked(uuid)) {
        event.cancelled = true;
    }
}

/// `onUserLinkStatusChange`. On the main thread.
pub fn on_user_link_status_change(event: &DiscordLinkStatusChangeEvent) {
    let Some(settings) = link_settings() else { return };
    let policy = settings.get_link_policy();
    if event.linked || policy == LinkPolicy::None {
        if let Some(user) = &event.user {
            user.set_freeze(false);
        }
        return;
    }
    let Some(user) = event.user.as_ref().filter(|user| user.is_online()) else { return };
    let Some(manager) = link_manager() else { return };
    let code = manager.code_for(&user.get_uuid());
    match policy {
        LinkPolicy::Kick => {
            if let Some(player) = user.player() {
                player.kick_player(&mini_to_legacy(&user.player_tl("discordLinkLoginKick", essentials::args![link_line(&code), invite_url()])));
            }
        }
        LinkPolicy::Freeze => {
            user.send_tl("discordLinkLoginPrompt", essentials::args![link_line(&code), invite_url()]);
            user.set_freeze(true);
        }
        LinkPolicy::None => {}
    }
}

/// `registerEvents`, once.
pub fn register(plugin: Plugin) {
    plugin.register_event_with(HIGH, false, |event: &mut AsyncPlayerPreLoginEvent| {
        let uuid = event.get_unique_id();
        if link_settings().is_none_or(|settings| settings.get_link_policy() != LinkPolicy::Kick) {
            return;
        }
        if let Some(manager) = link_manager().filter(|manager| !manager.is_uuid_linked(&uuid)) {
            let code = manager.code_for(&uuid);
            event.disallow("KICK_OTHER", &legacy(&mini_to_legacy(&tl_literal("discordLinkLoginKick", essentials::args![link_line(&code), invite_url()]))));
        }
    });
    plugin.register_event_with(LOW, false, |event: &mut PlayerInteractEvent| {
        if is_frozen_out(&event.get_player().get_unique_id()) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(LOW, false, |event: &mut PlayerCommandPreprocessEvent| {
        if is_frozen_out(&event.get_player().get_unique_id()) {
            event.set_cancelled(true);
            if let Some(user) = ess().get_user(event.get_player()) {
                prompt(&user);
            }
        }
    });
    plugin.register_event_with(LOW, false, |event: &mut AsyncPlayerChatEvent| {
        if is_frozen_out(&event.get_player().get_unique_id()) {
            event.set_cancelled(true);
            if let Some(user) = ess().get_user(event.get_player()) {
                prompt(&user);
            }
        }
    });
    static LISTENING: Once = Once::new();
    LISTENING.call_once(|| hooks::listen(on_hook));
    if let Some(service) = essentials_discord::service() {
        service.add_message_listener(on_discord_message);
    }
}
