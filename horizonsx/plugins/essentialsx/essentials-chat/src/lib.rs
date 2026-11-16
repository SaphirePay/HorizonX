//! `EssentialsChat`. `LocalChatSpyEvent` isn't fired: nothing here can listen. `GlobalChatEvent` and
//! `LocalChatEvent` are a hook for the modules in this library. The Paper chat handler is the same
//! steps on Paper's event: there's the one chat event here.
use essentials::prelude::*;
use essentials::adventure::{escape_tags, legacy_to_mini, legacy_to_mini_with_urls, strip_color};
use essentials::hooks::{self, ChatType as HookChatType};
use essentials::perm;
use essentials::utils::format::format_message;
use horizon_bukkit::enums::event_priority;
use horizon_bukkit::fam_ev_player::AsyncPlayerChatEvent;
use horizon_bukkit::Plugin;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const OTHERS: &str = "essentials.toggleshout.others";

const TOGGLE_SHOUT: Command = Command {
    run_user: Some(|user, _, args| handle_toggle_with_args(user, args, OTHERS, toggle_player)),
    run_source: Some(|sender, _, args| toggle_other_players(sender, args, toggle_player)),
    tab_user: Some(|user, _, args| toggle_tab_user(user, args, OTHERS)),
    tab_source: Some(|sender, _, args| toggle_tab_source(sender, args)),
    ..Command::new("toggleshout")
};

fn toggle_player(sender: &CommandSource, user: &User, enabled: Option<bool>) -> EssResult {
    let enabled = enabled.unwrap_or_else(|| !user.is_toggle_shout());
    user.set_toggle_shout(enabled);
    user.send_tl(if enabled { "shoutEnabled" } else { "shoutDisabled" }, &[]);
    if !is_self(sender, user) {
        sender.send_tl(if enabled { "shoutEnabledFor" } else { "shoutDisabledFor" }, essentials::args![user.get_display_name()]);
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum ChatType {
    Shout,
    Question,
    Local,
    Unknown,
}

impl ChatType {
    fn key(self) -> &'static str {
        match self {
            Self::Shout => "shout",
            Self::Question => "question",
            Self::Local => "local",
            Self::Unknown => "normal",
        }
    }
    fn long_type(self) -> String {
        if self == Self::Unknown { "chat".to_owned() } else { format!("chat-{}", self.key()) }
    }
}

/// `ProcessedChat`.
#[derive(Clone)]
struct Processed {
    user: User,
    kind: ChatType,
    radius: i64,
}

static CACHE: LazyLock<Mutex<HashMap<String, Processed>>> = LazyLock::new(Default::default);

fn cached(player: &Player) -> Option<Processed> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(&player.get_unique_id()).cloned()
}

fn chat_type(user: &User, message: &str) -> ChatType {
    let mut letters = message.chars();
    let Some(prefix) = letters.next() else { return ChatType::Unknown };
    if letters.next().is_none() {
        return if user.is_toggle_shout() { ChatType::Shout } else { ChatType::Unknown };
    }
    if prefix == settings().get_chat_question() && settings().is_chat_question_enabled() {
        ChatType::Question
    } else if prefix == settings().get_chat_shout() || user.is_toggle_shout() {
        ChatType::Shout
    } else {
        ChatType::Unknown
    }
}

/// `callChatEvent`: `LocalChatEvent` or `GlobalChatEvent`, for whoever listens.
fn call_chat_event(event: &AsyncPlayerChatEvent, kind: ChatType) {
    let chat_type = match kind {
        ChatType::Local => HookChatType::Local,
        ChatType::Question => HookChatType::Question,
        ChatType::Shout => HookChatType::Shout,
        ChatType::Unknown => HookChatType::Unknown,
    };
    hooks::fire(hooks::Hook::Chat { player: event.get_player(), message: event.get_message(), chat_type });
}

fn escaped(text: &str) -> String {
    text.replace('%', "%%")
}

/// `handleChatFormat`.
fn handle_chat_format(event: &mut AsyncPlayerChatEvent) {
    if event.is_cancelled() {
        return;
    }
    let player = event.get_player().clone();
    let Some(user) = ess().get_user(&player) else {
        event.set_cancelled(true);
        return;
    };
    user.set_display_nick();
    let mut chat = cached(&player).unwrap_or_else(|| Processed { user: user.clone(), kind: chat_type(&user, event.get_message()), radius: 0 });
    chat.radius = i64::from(settings().get_chat_radius()).max(0);
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).insert(player.get_unique_id(), chat.clone());
    let message = format_message(&user, "essentials.chat", event.get_message());
    event.set_message(&message);
    if strip_color(event.get_message()).is_empty() {
        event.set_cancelled(true);
        return;
    }
    let group = user.get_group();
    let world = player.get_world();
    let world_name = world.get_name().to_owned();
    let username = user.get_name();
    let nickname = user.get_formatted_nickname();
    let prefix = replace_format(&perm::get_prefix(&player));
    let suffix = replace_format(&perm::get_suffix(&player));
    let team = match player.get_scoreboard() {
        horizon_bukkit::fam_entity::ShownScoreboard::Game(board) => board.get_entry_team(player.get_name()),
        horizon_bukkit::fam_entity::ShownScoreboard::Plugin(_) => None,
    };
    let kind = if chat.radius > 0 && chat.kind == ChatType::Unknown { ChatType::Local } else { chat.kind };
    let mut format = settings().get_chat_format(Some(&group), kind.key());
    let team_part = |read: fn(&horizon_bukkit::scoreboard::Team) -> String| team.as_ref().map_or_else(String::new, read);
    for (mark, value) in [
        ("{0}", group.clone()),
        ("{1}", settings().get_world_alias(&world_name)),
        ("{2}", world_name.chars().take(1).collect::<String>().to_uppercase()),
        ("{3}", team_part(horizon_bukkit::scoreboard::Team::get_prefix)),
        ("{4}", team_part(horizon_bukkit::scoreboard::Team::get_suffix)),
        ("{5}", team_part(horizon_bukkit::scoreboard::Team::get_display_name)),
        ("{6}", prefix),
        ("{7}", suffix),
        ("{8}", username.clone()),
        ("{9}", nickname.unwrap_or(username)),
    ] {
        format = format.replace(mark, &escaped(&value));
    }
    if chat.radius > 0 && !event.get_message().is_empty() {
        let text = event.get_message().to_owned();
        let first = text.chars().next();
        if text.chars().count() > 1 && ((chat.kind == ChatType::Shout && first == Some(settings().get_chat_shout())) || (chat.kind == ChatType::Question && first == Some(settings().get_chat_question()))) {
            event.set_message(text.chars().skip(1).collect::<String>().trim());
        }
        if strip_color(event.get_message()).is_empty() {
            event.set_cancelled(true);
            return;
        }
        format = if chat.kind == ChatType::Unknown { format!("{}{format}", mini_to_legacy(&tl_literal("chatTypeLocal", &[]))) } else { mini_to_legacy(&tl_literal(&format!("{}Format", chat.kind.key()), essentials::args![format])) };
    }
    event.set_format(&format);
}

fn keep_recipients(event: &mut AsyncPlayerChatEvent, keep: impl Fn(&User) -> bool) {
    let kept = event.get_recipients().into_iter().filter(|player| ess().get_user(player).is_some_and(|user| keep(&user))).collect();
    event.set_recipients(kept);
}

/// `handleChatRecipients`.
fn handle_chat_recipients(event: &mut AsyncPlayerChatEvent) {
    if event.is_cancelled() {
        return;
    }
    let Some(chat) = cached(event.get_player()) else { return };
    if chat.radius < 1 {
        call_chat_event(event, chat.kind);
        return;
    }
    let user = &chat.user;
    if !event.get_message().is_empty() {
        if chat.kind == ChatType::Unknown {
            if !user.is_authorized("essentials.chat.local") {
                user.send_tl("notAllowedToLocal", &[]);
                event.set_cancelled(true);
                return;
            }
            keep_recipients(event, |other| other.is_authorized("essentials.chat.receive.local"));
        } else {
            let key = chat.kind.key();
            if user.is_authorized(&format!("essentials.chat.{key}")) {
                keep_recipients(event, |other| other.is_authorized(&format!("essentials.chat.receive.{key}")));
                call_chat_event(event, chat.kind);
            } else {
                user.send_tl(if chat.kind == ChatType::Shout { "notAllowedToShout" } else { "notAllowedToQuestion" }, &[]);
                event.set_cancelled(true);
            }
            return;
        }
    }
    let Some(location) = user.get_location() else { return };
    let radius_squared = (chat.radius * chat.radius) as f64;
    let mut spies = Vec::new();
    let mut kept = Vec::new();
    for player in event.get_recipients() {
        let Some(other) = ess().get_user(&player) else { continue };
        let near = other == *user || other.get_location().is_some_and(|at| at.world == location.world && at.distance_squared(&location) <= radius_squared);
        if near {
            kept.push(player);
        } else if other.is_authorized("essentials.chat.spy") {
            spies.push(player);
        }
    }
    event.set_recipients(kept);
    if event.get_recipients().len() < 2 {
        user.send_tl("localNoOne", &[]);
    }
    call_chat_event(event, ChatType::Local);
    let local_prefix = tl_literal("chatTypeLocal", &[]);
    let format = legacy_to_mini(event.get_format());
    let format = format.strip_prefix(&local_prefix).unwrap_or(&format);
    let line = format.replace("%1$s", &format!("{}<reset>", legacy_to_mini(&user.get_display_name()))).replace("%2$s", &legacy_to_mini_with_urls(&escape_tags(event.get_message()))).replace("%%", "%");
    let line = mini_to_legacy(&line);
    for spy in spies {
        spy.send_message(&line);
    }
}

/// `handleChatPostFormat`, then `handleChatSubmit`.
fn handle_chat_submit(event: &mut AsyncPlayerChatEvent) {
    let key = event.get_player().get_unique_id();
    if event.is_cancelled() {
        CACHE.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
        return;
    }
    if let Some(chat) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).remove(&key) {
        let name = chat.kind.long_type();
        if let Err(error) = Trade::of_command(&name).charge(&chat.user) {
            ess().show_error(&chat.user.get_source(), &error, &format!("\\ chat {name}"));
            event.set_cancelled(true);
        }
    }
}

/// `onEnable`.
pub fn on_enable(plugin: Plugin) {
    settings().set_essentials_chat_active(true);
    register_commands(plugin, &[TOGGLE_SHOUT]);
    plugin.register_event_with(event_priority::LOWEST, false, handle_chat_format);
    plugin.register_event_with(event_priority::NORMAL, false, handle_chat_recipients);
    plugin.register_event_with(event_priority::HIGHEST, false, handle_chat_submit);
}
