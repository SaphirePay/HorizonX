//! `BukkitListener`, `BukkitChatListener`, `EssentialsChatListener` and `PaperChatListener`, which is
//! the same steps on one chat event here; and the game's half of `DiscordListener`. All on the main
//! thread. They're registered once, and each asks whether Discord is up.
use essentials::prelude::*;
use crate::api::{MessageType, default_types};
use crate::host::{Feedback, Relay};
use crate::message_util::{sanitize_discord_markdown, sanitize_option};
use crate::service;
use crate::service::{JdaDiscordService, names};
use crate::settings::{Format, clean};
use essentials::command_source::console_display_name;
use essentials::hooks::{self, ChatType, Hook};
use essentials::messaging::Recipient;
use essentials::perm;
use essentials::utils::format::strip_essentials_format;
use horizon_bukkit::enums::event_priority::MONITOR;
use horizon_bukkit::event::{PlayerDeathEvent, PlayerQuitEvent};
use horizon_bukkit::fam_core::create_command_sender;
use horizon_bukkit::fam_ev_player::{AsyncPlayerChatEvent, PlayerAdvancementDoneEvent, PlayerKickEvent};
use horizon_bukkit::{Component, Plugin};
use std::sync::{Arc, Mutex, Once};

fn live() -> Option<Arc<JdaDiscordService>> {
    service().filter(|service| service.is_live())
}

fn world_of(player: &Player) -> String {
    settings().get_world_alias(&player.get_world().get_name())
}

/// `getPrefix` and `getSuffix`, with `stripEssentialsFormat`.
fn prefix_suffix(player: &Player) -> (String, String) {
    (strip_essentials_format(&perm::get_prefix(player)), strip_essentials_format(&perm::get_suffix(player)))
}

/// `DiscordUtil.getAvatarUrl`.
fn avatar_url(service: &JdaDiscordService, player: &Player) -> String {
    service.settings().get_avatar_url().replace("{uuid}", &player.get_unique_id()).replace("{name}", player.get_name())
}

/// `getMcToDiscordNameFormat`, filled in.
fn name_of(service: &JdaDiscordService, player: &Player, user: &User) -> String {
    let (prefix, suffix) = prefix_suffix(player);
    let args: [Arg; 6] = [player.get_name().into(), user.get_display_name().into(), world_of(player).into(), prefix.into(), suffix.into(), service.bot_name().into()];
    service.settings().mc_to_discord_name_format().format(&args)
}

/// `sendChatMessage`.
pub fn send_chat_message(service: &Arc<JdaDiscordService>, chat_type: ChatType, player: &Player, chat_message: &str) {
    let Some(user) = ess().get_user(player) else { return };
    let (prefix, suffix) = prefix_suffix(player);
    let message = if user.is_authorized("essentials.discord.markdown") { chat_message.to_owned() } else { sanitize_discord_markdown(chat_message) };
    let [username, display_name] = names(player.get_name(), &user.get_display_name());
    let formatted = service.settings().mc_to_discord_format(chat_type).format(&[username, display_name, message.into(), clean(&world_of(player)), clean(&prefix), clean(&suffix)]);
    let message_type = match chat_type {
        ChatType::Shout => default_types::SHOUT,
        ChatType::Question => default_types::QUESTION,
        ChatType::Local => default_types::LOCAL,
        ChatType::Unknown => default_types::CHAT,
    };
    let name = name_of(service, player, &user);
    service.dispatch_discord_message(message_type, &formatted, user.is_authorized("essentials.discord.ping"), Some(avatar_url(service, player)), Some(name), Some(player.get_unique_id()));
}

/// `sendDiscordMessage`.
fn send_discord_message(service: &Arc<JdaDiscordService>, message_type: MessageType, message: &str, player: Option<&Player>) {
    let (mut avatar, mut name, mut uuid) = (None, None, None);
    if let Some(player) = player {
        if service.settings().is_show_avatar() {
            avatar = Some(avatar_url(service, player));
        }
        if let Some(user) = ess().get_user(player) {
            name = Some(name_of(service, player, &user));
        }
        uuid = Some(player.get_unique_id());
    }
    service.dispatch_discord_message(message_type, message, false, avatar, name, uuid);
}

/// `isVanishHide`.
fn is_vanish_hide(service: &JdaDiscordService, user: &User) -> bool {
    service.settings().is_vanish_hide_messages() && (user.is_hidden() || user.is_leaving_hidden())
}

/// `isSilentJoinQuit`.
fn is_silent_join_quit(user: &User, kind: &str) -> bool {
    settings().allow_silent_join_quit() && user.is_authorized(&format!("essentials.silent{kind}"))
}

/// `sendJoinQuitMessage`.
fn send_join_quit_message(service: &Arc<JdaDiscordService>, user: &User, message: Option<&str>, message_type: MessageType) {
    let Some(player) = user.player() else { return };
    let mut online = ess().get_online_players().len();
    let format = match message_type.get_key() {
        "join" => service.settings().join_format(),
        "first-join" => service.settings().first_join_format(),
        // So that it will always be initialised. Other options shouldn't be possible.
        _ => {
            online = online.saturating_sub(1);
            service.settings().quit_format()
        }
    };
    let [username, display_name] = names(player.get_name(), &user.get_display_name());
    let line = format.format(&[username, display_name, Arg::from(sanitize_option(message)), online.into(), ess().users().get_user_count().into()]);
    send_discord_message(service, message_type, &line, Some(&player));
}

/// `onMute`.
fn on_mute(service: &Arc<JdaDiscordService>, affected: &User, controller: Option<&User>, value: bool, timestamp: Option<i64>, reason: Option<&str>) {
    let settings = service.settings();
    let [username, display_name] = names(&affected.get_name(), &affected.get_display_name());
    let reason = Arg::from(sanitize_option(reason));
    let has_reason = !matches!(reason, Arg::Null);
    let by_name = clean(&controller.map_or_else(|| "Console".to_owned(), User::get_name));
    let by_display = clean(&controller.map_or_else(console_display_name, User::get_display_name));
    let line = if !value {
        settings.unmute_format().format(&[username, display_name])
    } else if let Some(timestamp) = timestamp {
        let format: Format = if has_reason { settings.temp_mute_reason_format() } else { settings.temp_mute_format() };
        format.format(&[username, display_name, by_name, by_display, format_date_diff(timestamp).into(), reason])
    } else {
        let format: Format = if has_reason { settings.perm_mute_reason_format() } else { settings.perm_mute_format() };
        format.format(&[username, display_name, by_name, by_display, reason])
    };
    send_discord_message(service, default_types::MUTE, &line, None);
}

/// `onPrivateMessage`.
fn on_private_message(service: &Arc<JdaDiscordService>, sender: &Recipient, recipient: &Recipient, message: &str) {
    let user = match sender {
        Recipient::User(user) => Some(user),
        _ => None,
    };
    if user.is_some_and(|user| user.is_authorized("essentials.chat.spy.exempt")) {
        return;
    }
    let args = [clean(&sender.get_name()), clean(&sender.get_display_name()), clean(&recipient.get_name()), clean(&recipient.get_display_name()), clean(message)];
    let line = service.settings().pm_to_discord_format().format(&args);
    send_discord_message(service, default_types::PRIVATE_CHAT, &line, user.and_then(User::player).as_ref());
}

/// What `BukkitListener` hears from Essentials itself, and `EssentialsChatListener`.
fn on_hook(hook: &Hook<'_>) {
    let Some(service) = live() else { return };
    let service = &service;
    match *hook {
        Hook::Mute { affected, controller, value, timestamp, reason } => on_mute(service, affected, controller, value, timestamp, reason),
        Hook::PrivateMessage { sender, recipient, message } => on_private_message(service, sender, recipient, message),
        Hook::UserDataLoad { user, join_message } => {
            if !is_silent_join_quit(user, "join") && !is_vanish_hide(service, user) {
                // `hasPlayedBefore`: as in EssentialsSpawn, any time played at all.
                let first = user.player().is_some_and(|player| essentials::ess::play_one_tick(&player) <= 0);
                send_join_quit_message(service, user, join_message, if first { default_types::FIRST_JOIN } else { default_types::JOIN });
            }
        }
        Hook::Vanish { user, value } => {
            if !service.settings().is_vanish_fake_join_leave() || user.is_leaving_hidden() {
                return;
            }
            let (word, message_type) = if value { ("left", default_types::LEAVE) } else { ("joined", default_types::JOIN) };
            send_join_quit_message(service, user, Some(&format!("\u{a7}e{} {word} the game", user.get_name())), message_type);
        }
        Hook::Afk { user, value } => {
            if is_vanish_hide(service, user) {
                return;
            }
            let format = if value { service.settings().afk_format() } else { service.settings().un_afk_format() };
            let [username, display_name] = names(&user.get_name(), &user.get_display_name());
            send_discord_message(service, default_types::AFK, &format.format(&[username, display_name]), user.player().as_ref());
        }
        Hook::UserAction { user, message } => {
            if is_vanish_hide(service, user) {
                return;
            }
            let [username, display_name] = names(&user.get_name(), &user.get_display_name());
            send_discord_message(service, default_types::ACTION, &service.settings().action_format().format(&[username, display_name, message.into()]), user.player().as_ref());
        }
        Hook::Chat { player, message, chat_type } => {
            if service.settings().is_use_essentials_events() {
                let (player, message, service) = (player.clone(), message.to_owned(), service.clone());
                ess().schedule_sync_delayed_task(move || send_chat_message(&service, chat_type, &player, &message));
            }
        }
        Hook::Mail { .. } => {}
    }
}

/// `onChat`, for `BukkitChatListener` and `PaperChatListener`.
fn on_chat(event: &mut AsyncPlayerChatEvent) {
    let Some(service) = live() else { return };
    if service.settings().is_use_essentials_events() && settings().is_essentials_chat_active() {
        return;
    }
    let recipients = event.get_recipients();
    let to_everyone = ess().get_online_players().iter().all(|player| recipients.iter().any(|recipient| recipient.get_name() == player.get_name()));
    if !service.settings().is_show_all_chat() && !to_everyone {
        return;
    }
    let (player, message) = (event.get_player().clone(), event.get_message().to_owned());
    ess().schedule_sync_delayed_task(move || send_chat_message(&service, ChatType::Unknown, &player, &message));
}

fn on_quit(event: &mut PlayerQuitEvent) {
    let Some(service) = live() else { return };
    let Some(user) = ess().get_user(event.get_player()) else { return };
    if !is_silent_join_quit(&user, "quit") && !is_vanish_hide(&service, &user) {
        send_join_quit_message(&service, &user, Some(&event.get_quit_message()), default_types::LEAVE);
    }
}

fn on_death(event: &mut PlayerDeathEvent) {
    let (Some(service), Some(user)) = (live(), ess().get_user(event.get_player())) else {
        return;
    };
    if is_vanish_hide(&service, &user) {
        return;
    }
    let player = event.get_player();
    let show = player.get_world().get_game_rule_value("showDeathMessages");
    let Some(death_message) = event.get_death_message().filter(|message| show.as_deref() != Some("false") && !message.trim().is_empty()) else { return };
    let [username, display_name] = names(player.get_name(), &user.get_display_name());
    let line = service.settings().death_format().format(&[username, display_name, clean(death_message)]);
    send_discord_message(&service, default_types::DEATH, &line, Some(player));
}

/// `PaperAdvancementListenerProvider.onAdvancement`, then `onAdvancement`.
fn on_advancement(event: &mut PlayerAdvancementDoneEvent) {
    let (Some(service), Some(player)) = (live(), event.get_player()) else {
        return;
    };
    let Some(display) = event.get_advancement().and_then(|advancement| advancement.get_display()).filter(|display| display.should_announce_chat()) else { return };
    let Some(user) = ess().get_user(&player) else { return };
    if user.is_vanished() {
        event.set_message(None);
        return;
    }
    if is_vanish_hide(&service, &user) || player.get_world().get_game_rule_value("announceAdvancements").as_deref() == Some("false") {
        return;
    }
    let [username, display_name] = names(player.get_name(), &user.get_display_name());
    let title = Component(display.get_title().clone()).plain();
    send_discord_message(&service, default_types::ADVANCEMENT, &service.settings().advancement_format().format(&[username, display_name, title.into()]), Some(&player));
}

fn on_kick(event: &mut PlayerKickEvent) {
    let (Some(service), Some(player)) = (live(), event.get_player()) else {
        return;
    };
    let Some(user) = ess().get_user(&player) else { return };
    if is_vanish_hide(&service, &user) {
        return;
    }
    let [username, display_name] = names(player.get_name(), &user.get_display_name());
    let line = service.settings().kick_format().format(&[username, display_name, clean(event.get_reason())]);
    send_discord_message(&service, default_types::KICK, &line, None);
}

/// `registerEvents`, once.
pub fn register(plugin: Plugin) {
    plugin.register_event_with(MONITOR, true, on_chat);
    plugin.register_event_with(MONITOR, true, on_quit);
    plugin.register_event_with(MONITOR, true, on_death);
    plugin.register_event_with(MONITOR, true, on_advancement);
    plugin.register_event_with(MONITOR, true, on_kick);
    static LISTENING: Once = Once::new();
    LISTENING.call_once(|| hooks::listen(on_hook));
}

/// The end of `DiscordListener.onMessageReceived`: who sees it, and showing them.
pub fn deliver(relay: Relay) {
    let Some(service) = service() else { return };
    let always = service.settings().is_always_receive_primary();
    for user in ess().get_online_users() {
        let sees = relay.keys.iter().any(|group| {
            let permission = format!("essentials.discord.receive.{group}");
            (always && group.eq_ignore_ascii_case("primary")) || (user.is_permission_set(&permission) && user.is_authorized(&permission))
        });
        if sees {
            user.send_message(&relay.formatted);
        }
    }
}

/// `DiscordCommandSender`, and the dispatch. The SDK gives a command's output when it's done, so there's
/// no need of Java's timer. A command the sender can't run goes to the console, as Java's fallback does.
pub fn dispatch_command(line: &str, feedback: Feedback) {
    let buffer = Arc::new(Mutex::new(String::new()));
    let hook = buffer.clone();
    let sender = create_command_sender(move |message: Component| {
        let mut buffer = hook.lock().unwrap_or_else(|e| e.into_inner());
        if !buffer.is_empty() {
            buffer.push('\n');
        }
        buffer.push_str(&sanitize_discord_markdown(&strip_format(&message.plain())));
    });
    if !sender.dispatch(line) {
        horizon_bukkit::dispatch_command(line);
    }
    let output = std::mem::take(&mut *buffer.lock().unwrap_or_else(|e| e.into_inner()));
    if !output.is_empty() {
        feedback(output);
    }
}
