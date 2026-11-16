//! `EssentialsPlayerListener`: join and quit, AFK, mute, cooldowns, powertools, world changes.
use crate::prelude::*;
use crate::config_entities::JavaPattern;
use crate::textreader::{KeywordReplacer, TextPager, text_input};
use crate::user::AfkCause;
use crate::utils::location::{self, should_fly};
use crate::utils::number::DecimalFormat;
use crate::utils::material::{is_bed, is_skull};
use horizon_bukkit::event::{PlayerCommandPreprocessEvent, PlayerInteractEvent, PlayerJoinEvent, PlayerLoginEvent, PlayerQuitEvent, PlayerRespawnEvent};
use horizon_bukkit::events::{EntityPickupItemEvent, PlayerEggThrowEvent, PlayerMoveEvent, PlayerTeleportEvent};
use horizon_bukkit::fam_ev_player::{AsyncPlayerChatEvent, InventoryClickEvent, InventoryCloseEvent, PlayerChangedWorldEvent, PlayerFishEvent, PlayerGameModeChangeEvent};
use horizon_bukkit::extra::get_ban_list;
use horizon_bukkit::fam_ev_player::{InventoryDragEvent, PlayerBucketEmptyEvent};
use horizon_bukkit::fam_ev_world::BlockReceiveGameEvent;
use horizon_bukkit::{Plugin, scheduler};
use horizon_bukkit::enums::event_priority::{HIGH, HIGHEST, LOW, LOWEST, MONITOR, NORMAL};
use std::collections::HashMap;
use std::sync::Mutex;

static PENDING_MOTD: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);

fn number(value: usize) -> String {
    DecimalFormat::new("#,##0", crate::big_decimal::Rounding::HalfEven).format(&BigDecimal::from_i64(value as i64))
}

fn hide_join_quit_messages() -> bool {
    settings().has_join_quit_message_player_count() && ess().get_online_players().len() as i32 > settings().get_join_quit_message_player_count()
}

fn tell_muted(user: &User) {
    let date_diff = (user.get_mute_timeout() > 0).then(|| format_date_diff(user.get_mute_timeout()));
    match (date_diff, user.get_mute_reason()) {
        (None, Some(reason)) => user.send_tl("voiceSilencedReason", crate::args![reason]),
        (None, None) => user.send_tl("voiceSilenced", &[]),
        (Some(date), Some(reason)) => user.send_tl("voiceSilencedReasonTime", crate::args![date, reason]),
        (Some(date), None) => user.send_tl("voiceSilencedTime", crate::args![date]),
    }
}

/// `updateCompass`: at the first home, or the bed.
fn update_compass(user: &User) {
    if settings().is_compass_towards_home_perm() && !user.is_authorized("essentials.home.compass") {
        return;
    }
    let Some(player) = user.player() else { return };
    let home = user.get_location().and_then(|at| user.get_home_near(&at));
    if let Some(target) = home.or_else(|| player.get_respawn_location()) {
        player.set_compass_target(&target);
    }
}

fn motd_flow(user: &User) {
    if let Some(pending) = PENDING_MOTD.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        pending.remove(&user.get_uuid());
    }
    if settings().is_command_disabled("motd") {
        return;
    }
    let source = user.get_source();
    let input = match text_input(&source, "motd", true) {
        Ok(input) => input,
        Err(error) => {
            crate::warning(&error);
            return;
        }
    };
    if input.lines.is_empty() || !user.is_authorized("essentials.motd") {
        return;
    }
    let (chapters, bookmarks) = (input.chapters.clone(), input.bookmarks.clone());
    let output = KeywordReplacer::new(input, &source, true, false);
    TextPager { lines: output.get_lines(), chapters: &chapters, bookmarks: &bookmarks, one_page: true }.show_page(Some("1"), None, Some("motd"), &source);
}

/// `joinFlow`: the message to show, if it changed.
fn join_flow(user: &User, message: Option<String>) -> Option<Option<String>> {
    let Some(player) = user.player() else { return None };
    user.start_transaction();
    let last_account_name = user.get_last_account_name();
    user.set_last_account_name(player.get_name());
    let new_username = last_account_name.as_deref().is_some_and(|last| last != player.get_name());
    if settings().is_reset_nick_on_name_change() && new_username && user.get_nickname().is_some() {
        user.set_nickname(None);
    }
    user.set_last_login(now_ms());
    user.set_display_nick();
    update_compass(user);
    user.set_leaving_hidden(false);
    if !ess().get_vanished_players().is_empty() && !user.is_authorized("essentials.vanish.see") {
        for name in ess().get_vanished_players() {
            if let Some(vanished) = horizon_bukkit::get_player(&name) {
                player.hide_player(&vanished);
                if settings().is_debug() {
                    crate::info(&format!("Hiding vanished player: {name}"));
                }
            }
        }
    }
    if user.is_authorized("essentials.sleepingignored") {
        player.set_sleeping_ignored(true);
    }
    let silent = settings().allow_silent_join_quit() && (user.is_authorized("essentials.silentjoin") || user.is_authorized("essentials.silentjoin.vanish"));
    let effective = if silent {
        if user.is_authorized("essentials.silentjoin.vanish") {
            user.set_vanished(true);
        }
        None
    } else if message.is_none() || hide_join_quit_messages() {
        None
    } else if settings().is_custom_join_message() {
        let template = if new_username && settings().is_custom_new_username_message() { settings().get_custom_new_username_message() } else { settings().get_custom_join_message() };
        let text = template
            .replace("{PLAYER}", &user.get_display_name())
            .replace("{USERNAME}", &user.get_name())
            .replace("{UNIQUE}", &number(ess().users().get_user_count()))
            .replace("{ONLINE}", &number(ess().get_online_players().len()))
            .replace("{UPTIME}", &format_date_diff(ess().start_time()))
            .replace("{PREFIX}", &replace_format(&crate::perm::get_prefix(&player)))
            .replace("{SUFFIX}", &replace_format(&crate::perm::get_suffix(&player)))
            .replace("{OLDUSERNAME}", last_account_name.as_deref().unwrap_or_default());
        (!text.is_empty()).then_some(text)
    } else {
        message.clone()
    };
    crate::hooks::fire(crate::hooks::Hook::UserDataLoad { user, join_message: effective.as_deref() });
    let change = (effective != message).then(|| effective.clone());
    if settings().get_motd_delay() >= 0 {
        let delay = settings().get_motd_delay() / 50;
        if delay > 0 {
            let target = user.clone();
            let task = ess().schedule_sync_delayed_task_later(move || motd_flow(&target), delay as u64);
            PENDING_MOTD.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().insert(user.get_uuid(), task);
        } else {
            motd_flow(user);
        }
    }
    if !settings().is_command_disabled("mail") && user.is_authorized("essentials.mail") {
        if user.get_unread_mail_amount() == 0 {
            if settings().is_notify_no_new_mail() {
                user.send_tl("noNewMail", &[]);
            }
        } else {
            user.notify_of_mail();
        }
    }
    let fly_word = || parsed(user.player_tl("enabled", &[]));
    if user.is_authorized("essentials.updatecheck") {
        let told = user.clone();
        ess().run_task_asynchronously(move || {
            let messages = crate::update_check::version_messages(false, false);
            ess().schedule_sync_delayed_task(move || {
                for (key, args) in messages {
                    told.send_tl(key, &args);
                }
            });
        });
    }
    let restore_fly = user.is_fly_mode_enabled() && user.is_authorized("essentials.fly");
    if restore_fly {
        player.set_allow_flight(true);
        if settings().is_send_fly_enable_on_join() {
            user.send_tl("flyMode", crate::args![fly_word(), user.get_display_name()]);
        }
    }
    if user.is_authorized("essentials.fly.safelogin") {
        player.set_fall_distance(0.0);
        let at = player.get_location();
        let (chunk_x, chunk_z) = (at.get_block_x() >> 4, at.get_block_z() >> 4);
        let (flyer, told) = (player.clone(), user.clone());
        // Java's world is in by now. Here the chunk may not be, and all air would read as a long drop.
        crate::random_teleport::with_chunk(at.get_world().clone(), chunk_x, chunk_z, Box::new(move |loaded| {
            if loaded && !flyer.is_in_water() && should_fly(&at) {
                flyer.set_allow_flight(true);
                flyer.set_flying(true);
                if !restore_fly && settings().is_send_fly_enable_on_join() {
                    told.send_tl("flyMode", crate::args![parsed(told.player_tl("enabled", &[])), told.get_display_name()]);
                }
            }
        }));
    }
    if !user.is_authorized("essentials.speed") {
        player.set_fly_speed(0.1);
        player.set_walk_speed(0.2);
    }
    if user.is_social_spy_enabled() && !user.is_authorized("essentials.socialspy") {
        user.set_social_spy_enabled(false);
        crate::info(&format!("Set socialspy to false for {} because they had it enabled without permission.", user.get_name()));
    }
    if user.is_fly_mode_enabled() && !user.is_authorized("essentials.fly") {
        user.set_fly_mode_enabled(false);
        crate::info(&format!("Set fly mode to false for {} because they had it enabled without permission.", user.get_name()));
    }
    if user.is_god_mode_enabled() && !user.is_authorized("essentials.god") {
        user.set_god_mode_enabled(false);
        crate::info(&format!("Set god mode to false for {} because they had it enabled without permission.", user.get_name()));
    }
    user.set_confirming_clear_command(None);
    user.clear_confirming_payments();
    user.stop_transaction();
    change
}

fn use_powertools(user: &User, material: &horizon_bukkit::Material) -> bool {
    let commands = user.get_powertool(material).unwrap_or_default();
    let mut used = false;
    for command in commands {
        if command.contains("{player}") {
            continue;
        }
        used = true;
        let Some(player) = user.player() else { continue };
        if let Some(chat) = command.strip_prefix("c:") {
            player.chat(chat);
        } else {
            let name = user.get_name();
            ess().schedule_sync_delayed_task(move || {
                player.chat(&format!("/{command}"));
                crate::info(&format!("[PT] {name} issued server command: /{command}"));
            });
        }
    }
    used
}

/// `handlePlayerCommandPreprocess`.
fn handle_command(event: &mut PlayerCommandPreprocessEvent, effective: &str) {
    let cmd = effective.split(' ').next().unwrap_or_default().replace('/', "").to_lowercase();
    let Some(user) = ess().get_user(event.get_player()) else { return };
    let command = crate::commands::TABLE.iter().find(|command| command.name == cmd).map(|command| command.name);
    let spy_commands = settings().get_social_spy_commands();
    if (spy_commands.contains(&cmd) || spy_commands.contains("*")) && !matches!(command, Some("msg" | "r")) && !user.is_authorized("essentials.chat.spy.exempt") {
        let name = if settings().is_social_spy_display_names() { user.get_display_name() } else { user.get_name() };
        for spy in ess().get_online_users() {
            if spy.is_social_spy_enabled() && spy != user {
                let prefix = spy.player_tl(if user.is_muted() && settings().get_social_spy_listen_muted_players() { "socialSpyMutedPrefix" } else { "socialSpyPrefix" }, &[]);
                spy.send_component(&crate::adventure::deserialize_mini_message(&format!("{prefix}{}", spy.player_tl("socialSpyCmdFormat", crate::args![name.clone(), event.get_message()]))));
            }
        }
    }
    let mute_commands = settings().get_mute_commands();
    if user.is_muted() && (mute_commands.contains(&cmd) || mute_commands.contains("*")) {
        event.set_cancelled(true);
        tell_muted(&user);
        crate::info(&crate::adventure::mini_to_legacy(&tl_literal("mutedUserSpeaks", crate::args![user.get_name(), event.get_message()])));
        return;
    }
    let (mut broadcast, mut update) = (true, true);
    match command {
        Some("afk") => {
            update = false;
            broadcast = false;
        }
        Some("vanish") => broadcast = false,
        _ => {}
    }
    if update {
        user.update_activity_on_interact(broadcast);
    }
    if settings().is_command_cooldowns_enabled() && !user.is_authorized("essentials.commandcooldowns.bypass") && command.is_none_or(|name| !user.is_authorized(&format!("essentials.commandcooldowns.bypass.{name}"))) {
        let args = effective.find(' ').map_or(String::new(), |at| format!(" {}", &effective[at..]));
        let full = command.map_or_else(|| effective.to_owned(), |name| format!("{name}{args}"));
        let mut found = false;
        for (pattern, expiry) in user.get_command_cooldowns() {
            if expiry <= now_ms() {
                user.clear_command_cooldown(&pattern);
            } else if pattern.matches(&full) {
                user.send_tl("commandCooldown", crate::args![format_date_diff(expiry)]);
                found = true;
                event.set_cancelled(true);
            }
        }
        if !found && let Some((pattern, cooldown)) = settings().get_command_cooldown_entry(&full) {
            add_cooldown(&user, pattern, cooldown, &full);
        }
    }
}

fn add_cooldown(user: &User, pattern: JavaPattern, cooldown: i64, full: &str) {
    if settings().is_debug() {
        crate::info(&format!("Applying {cooldown}ms cooldown on /{full} for{}.", user.get_name()));
    }
    user.add_command_cooldown(pattern, now_ms() + cooldown, settings().is_command_cooldown_persistent(full));
}

/// `cleanupOpenInventories`.
pub fn cleanup_open_inventories() {
    for user in ess().get_online_users() {
        if user.is_recipe_see() {
            if let Some(player) = user.player() {
                player.close_inventory();
            }
            user.set_recipe_see(false);
        }
        if user.is_inv_see() || user.is_ender_see() {
            if let Some(player) = user.player() {
                player.close_inventory();
            }
            user.set_inv_see(false);
            user.set_ender_see(false);
        }
    }
}

/// `onPlayerLoginBanned`: the kick message says what the ban says.
/// `CommandSendFilter`: what this player can't use doesn't go to their tab list. An alternative of
/// another plugin stays, unless Essentials' own overrides it.
fn filter_commands(user: &User, labels: &mut Vec<String>) {
    let known = horizon_bukkit::fam_core::get_command_map().get_known_commands();
    let ours: std::collections::BTreeMap<&str, &str> = known.iter().filter_map(|(word, command)| word.strip_prefix("essentials:").map(|word| (word, command.get_name()))).collect();
    labels.retain(|label| {
        let word = label.strip_prefix("essentials:").unwrap_or(label);
        let Some(name) = ours.get(word) else { return true };
        let own = settings().is_command_overridden(word) || crate::alternative_commands::get_alternative(word).is_none();
        !own || user.is_authorized(&if *name == "r" { "essentials.msg".to_owned() } else { format!("essentials.{name}") })
    });
}

fn login_banned(event: &mut PlayerLoginEvent) {
    let player = event.get_player().clone();
    let line = |key: &str, timed_key: &str, entry: &horizon_bukkit::core2::BanEntry| match crate::ban_lookup::expiration(entry) {
        Some(expiry) => tl_literal(timed_key, crate::args![format_date_diff(expiry), entry.get_reason().map(str::to_owned)]),
        None => tl_literal(key, crate::args![entry.get_reason().map(str::to_owned)]),
    };
    let text = if let Some(entry) = get_ban_list("NAME").get_ban_entry(player.get_name()) {
        line("banJoin", "tempbanJoin", &entry)
    } else if let Some(entry) = player.get_address().and_then(|address| get_ban_list("IP").get_ban_entry(&address)) {
        line("banIpJoin", "tempbanIpJoin", &entry)
    } else {
        return;
    };
    event.set_kick_message(&crate::adventure::mini_to_legacy(&text));
}

pub fn register(plugin: Plugin) {
    plugin.register_event_with(NORMAL, false, |event: &mut PlayerRespawnEvent| {
        let Some(user) = ess().get_user(event.get_player()) else { return };
        update_compass(&user);
        user.set_display_nick();
        if settings().is_teleport_invulnerability() {
            user.enable_invulnerability_after_teleport();
        }
    });
    plugin.register_event_with(LOWEST, false, |event: &mut AsyncPlayerChatEvent| {
        let Some(user) = ess().get_user(event.get_player()) else { return };
        if user.is_muted() {
            event.set_cancelled(true);
            tell_muted(&user);
            crate::info(&crate::adventure::mini_to_legacy(&tl_literal("mutedUserSpeaks", crate::args![user.get_name(), event.get_message()])));
        }
        let recipients = event.get_recipients();
        let kept: Vec<Player> = recipients.iter().filter(|player| !ess().get_user(player).is_some_and(|other| other.is_ignored_player(&user))).cloned().collect();
        if kept.len() != recipients.len() {
            event.set_recipients(kept);
        }
        user.update_activity_on_chat(true);
        user.set_display_nick();
    });
    plugin.register_event_with(HIGH, true, |event: &mut PlayerMoveEvent| {
        let (from, to) = (event.get_from().clone(), event.get_to().clone());
        if from.get_block_x() == to.get_block_x() && from.get_block_y() == to.get_block_y() && from.get_block_z() == to.get_block_z() {
            return;
        }
        let Some(user) = ess().get_user(event.get_player()) else { return };
        let held = |to: &horizon_bukkit::Location, from: &horizon_bukkit::Location| {
            let mut kept = to.clone();
            kept.x = from.x;
            kept.y = from.y;
            kept.z = from.z;
            kept
        };
        if user.is_freeze() {
            let kept = held(&to, &from);
            event.set_to(&location::get_safe_destination(&kept).unwrap_or(kept));
            return;
        }
        if !settings().cancel_afk_on_move() && !settings().get_freeze_afk_players() {
            return;
        }
        if user.is_afk() && settings().get_freeze_afk_players() {
            if to.get_y() >= f64::from(from.get_block_y() + 1) {
                user.update_activity_on_move(true);
                return;
            }
            let kept = held(&to, &from);
            let can_fly = event.get_player().get_allow_flight();
            let safe = if can_fly { None } else { location::get_safe_destination(&kept).ok() };
            event.set_to(&safe.unwrap_or(kept));
            return;
        }
        let far = user.get_afk_position().is_none_or(|afk| *to.get_world() != *afk.get_world() || afk.distance_squared(&to) > 9.0);
        if far {
            user.update_activity_on_move(true);
        }
    });
    plugin.register_event_with(HIGHEST, false, |event: &mut PlayerQuitEvent| {
        let Some(user) = ess().get_user(event.get_player()) else { return };
        if let Some(task) = PENDING_MOTD.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|pending| pending.remove(&user.get_uuid())) {
            scheduler::cancel_task(task);
        }
        if hide_join_quit_messages() || (settings().allow_silent_join_quit() && user.is_authorized("essentials.silentquit")) {
            event.set_quit_message(None);
        } else if settings().is_custom_quit_message() {
            let player = event.get_player().clone();
            let text = settings()
                .get_custom_quit_message()
                .replace("{PLAYER}", &player.display_name().to_legacy())
                .replace("{USERNAME}", player.get_name())
                .replace("{ONLINE}", &number(ess().get_online_players().len().saturating_sub(1)))
                .replace("{UPTIME}", &format_date_diff(ess().start_time()))
                .replace("{PREFIX}", &replace_format(&crate::perm::get_prefix(&player)))
                .replace("{SUFFIX}", &replace_format(&crate::perm::get_suffix(&player)));
            event.set_quit_message(if text.is_empty() { None } else { Some(&text) });
        }
        user.start_transaction();
        if settings().remove_god_on_disconnect() && user.is_god_mode_enabled() {
            user.set_god_mode_enabled(false);
        }
        if user.is_vanished() {
            user.set_leaving_hidden(true);
            user.set_vanished(false);
        }
        user.set_logout_location();
        user.update_activity(false, AfkCause::Quit);
        if !user.is_hidden() {
            user.set_last_logout(now_ms());
        }
        user.stop_transaction();
        user.dispose();
    });
    plugin.register_event_with(HIGHEST, false, |event: &mut PlayerJoinEvent| {
        ess().backup().on_player_join();
        let Some(user) = ess().get_user(event.get_player()) else { return };
        user.update(event.get_player().clone());
        let current = now_ms();
        user.start_transaction();
        if user.is_npc() {
            user.set_npc(false);
        }
        user.check_mute_timeout(current);
        user.update_activity(false, AfkCause::Join);
        user.stop_transaction();
        let message = event.get_join_message();
        if let Some(change) = join_flow(&user, Some(message)) {
            event.set_join_message(change.as_deref());
        }
    });
    plugin.register_event_with(LOW, false, |event: &mut horizon_bukkit::fam_ev_player::PlayerPickupArrowEvent| {
        if event.get_arrow().is_some_and(|arrow| arrow.get_scoreboard_tags().iter().any(|tag| tag == crate::commands::fireball::FIREBALL_META_KEY)) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event(|event: &mut horizon_bukkit::fam_ev_player::PlayerCommandSendEvent| {
        let Some(user) = event.get_player().and_then(|player| ess().get_user(player)) else { return };
        filter_commands(&user, event.get_commands());
    });
    plugin.register_event_with(LOW, false, |event: &mut PlayerLoginEvent| {
        if event.get_result() == "KICK_BANNED" {
            login_banned(event);
        }
    });
    plugin.register_event_with(HIGH, false, |event: &mut PlayerLoginEvent| {
        if event.get_result() == "KICK_WHITELIST" {
            if ess().get_user(event.get_player()).is_some_and(|user| user.is_authorized("essentials.whitelist.bypass")) {
                event.allow();
            } else if settings().is_custom_whitelist_message() {
                event.disallow("KICK_WHITELIST", &mini_to_legacy(&tl_literal("whitelistKick", &[])));
            }
            return;
        }
        if event.get_result() != "KICK_FULL" {
            return;
        }
        if ess().get_user(event.get_player()).is_some_and(|user| user.is_authorized("essentials.joinfullserver")) {
            event.allow();
        } else if settings().is_custom_server_full_message() {
            event.disallow("KICK_FULL", &tl_literal("serverFull", &[]));
        }
    });
    plugin.register_event_with(HIGH, true, |event: &mut PlayerTeleportEvent| {
        let cause = event.get_cause();
        if cause != "PLUGIN" && cause != "COMMAND" {
            return;
        }
        let Some(user) = ess().get_user(event.get_player()) else { return };
        if settings().register_back_in_listener() && user.is_authorized("essentials.back.onteleport") {
            user.set_last_location();
        }
        if settings().is_teleport_invulnerability() {
            user.enable_invulnerability_after_teleport();
        }
        // Issue 4325: the game drops flight on a world change. The sign says whether they were in the air.
        let player = event.get_player();
        if settings().is_world_change_preserve_flying() && user.is_authorized("essentials.fly") && event.get_from().get_world() != event.get_to().get_world() && player.get_allow_flight() {
            let tick = horizon_bukkit::server::get_current_tick() as i64;
            user.set_flight_tick(if player.is_flying() { tick } else { -tick });
        }
    });
    plugin.register_event_with(MONITOR, true, |event: &mut PlayerCommandPreprocessEvent| {
        let message = event.get_message().to_owned();
        let cmd = message.split(' ').next().unwrap_or_default().replace('/', "").to_lowercase();
        let args = message.find(' ').map_or(String::new(), |at| message[at..].to_owned());
        handle_command(event, &format!("{cmd}{args}"));
    });
    plugin.register_event_with(NORMAL, false, |event: &mut PlayerChangedWorldEvent| {
        let Some(user) = event.get_player().and_then(|player| ess().get_user(&player)) else { return };
        let Some(player) = user.player() else { return };
        if settings().is_world_change_fly_reset_enabled() && !matches!(player.get_game_mode(), "CREATIVE" | "SPECTATOR") && !user.is_authorized("essentials.fly") {
            player.set_fall_distance(0.0);
            player.set_allow_flight(false);
        }
        if settings().is_world_change_speed_reset_enabled() {
            if !user.is_authorized("essentials.speed") {
                player.set_fly_speed(0.1);
                player.set_walk_speed(0.2);
            } else {
                let bypass = user.is_authorized("essentials.speed.bypass");
                if f64::from(player.get_fly_speed()) > settings().get_max_fly_speed() && !bypass {
                    player.set_fly_speed(settings().get_max_fly_speed() as f32);
                } else {
                    player.set_fly_speed(player.get_fly_speed() * 0.99999);
                }
                if f64::from(player.get_walk_speed()) > settings().get_max_walk_speed() && !bypass {
                    player.set_walk_speed(settings().get_max_walk_speed() as f32);
                }
            }
        }
        // The same tick in Java. Here the event waits for the hub, which can take it into the next.
        let flight = user.get_flight_tick();
        if (flight.abs() - horizon_bukkit::server::get_current_tick() as i64).abs() <= 1 && user.is_authorized("essentials.fly") {
            player.set_allow_flight(true);
            if flight > 0 {
                player.set_flying(true);
            }
        }
        user.set_flight_tick(-1);
    });
    plugin.register_event_with(MONITOR, false, |event: &mut PlayerChangedWorldEvent| {
        let Some(user) = event.get_player().and_then(|player| ess().get_user(&player)) else { return };
        let Some(player) = user.player() else { return };
        let world = player.get_world();
        user.set_display_nick();
        update_compass(&user);
        if settings().get_no_god_worlds().contains(world.get_name()) && user.is_god_mode_enabled_raw() {
            user.send_tl("noGodWorldWarning", &[]);
        }
        if user.is_vanished() {
            user.set_vanished(user.is_authorized("essentials.vanish"));
        }
    });
    plugin.register_event_with(NORMAL, false, |event: &mut PlayerInteractEvent| {
        let mut update = true;
        match event.get_action() {
            "RIGHT_CLICK_BLOCK" => {
                if let Some(block) = event.get_clicked_block().cloned()
                    && is_bed(&block.get_type())
                    && settings().get_update_bed_at_daytime()
                    && block.get_property("occupied").as_deref() != Some("true")
                    && let Some(user) = ess().get_user(event.get_player())
                    && user.is_authorized("essentials.sethome.bed")
                    && user.get_world().is_some_and(|world| world.get_environment() == "NORMAL")
                    && let Some(player) = user.player()
                {
                    player.set_respawn_location(&block.get_location());
                }
            }
            "LEFT_CLICK_AIR" | "LEFT_CLICK_BLOCK" => {
                let action_air = event.get_action() == "LEFT_CLICK_AIR";
                if let Some(user) = ess().get_user(event.get_player()) {
                    if action_air && event.get_player().is_flying() && user.is_fly_click_jump() {
                        if let (Ok(target), Some(player)) = (location::get_target(event.get_player()), user.player()) {
                            ess().schedule_sync_delayed_task(move || {
                                let mut at = player.get_location();
                                at.x = target.get_x();
                                at.z = target.get_z();
                                while location::is_block_damaging(at.get_world(), at.get_block_x(), at.get_block_y() - 1, at.get_block_z()) {
                                    at.y += 1.0;
                                }
                                player.teleport(&at);
                            });
                        }
                    } else if let Some(item) = user.get_item_in_hand().filter(|item| !item.get_type().is_air())
                        && user.has_power_tools()
                        && user.are_power_tools_enabled()
                        && use_powertools(&user, item.get_type())
                    {
                        event.set_cancelled(true);
                    }
                }
            }
            "PHYSICAL" => update = false,
            _ => {}
        }
        if update && let Some(user) = ess().get_user(event.get_player()) {
            user.update_activity_on_interact(true);
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut PlayerFishEvent| {
        if settings().cancel_afk_on_fish()
            && let Some(user) = event.get_player().and_then(|player| ess().get_user(&player))
        {
            user.update_activity_on_interact(true);
        }
    });
    plugin.register_event_with(MONITOR, true, |event: &mut PlayerGameModeChangeEvent| {
        if !settings().is_gamemode_change_preserve_flying() {
            return;
        }
        let Some(player) = event.get_player() else { return };
        if ess().get_user(&player).is_some_and(|user| user.is_authorized("essentials.fly")) && player.is_flying() && player.get_allow_flight() {
            ess().schedule_sync_delayed_task_later(
                move || {
                    player.set_allow_flight(true);
                    player.set_flying(true);
                },
                1,
            );
        }
    });
    plugin.register_event_with(LOW, true, |event: &mut EntityPickupItemEvent| {
        if let Some(horizon_bukkit::fam_ev_entity::Actor::Player(player)) = event.get_entity()
            && let Some(user) = ess().get_user(&player)
            && ((settings().get_disable_item_pickup_while_afk() && user.is_afk()) || (user.is_vanished() && !user.is_authorized_cached("essentials.vanish.pickup")))
        {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(NORMAL, false, |event: &mut BlockReceiveGameEvent| {
        if event.get_entity().and_then(|entity| horizon_bukkit::fam_ess::get_player_by_id(&entity.get_unique_id())).and_then(|player| ess().get_user(&player)).is_some_and(|user| user.is_vanished()) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(HIGH, true, |event: &mut PlayerEggThrowEvent| {
        let Some(user) = event.get_player().and_then(|player| ess().get_user(&player)) else { return };
        let egg = ItemStack::new(Material::of("EGG"), 1);
        if user.has_unlimited(&egg)
            && let Some(player) = user.player()
        {
            inventories::add_item(&player, 0, false, &[egg]);
        }
    });
    plugin.register_event_with(LOWEST, true, |event: &mut InventoryClickEvent| {
        let Some(player) = event.get_who_clicked() else { return };
        let Some(user) = ess().get_user(&player) else { return };
        // Java goes by the top inventory's type. These views are chest windows here, which report
        // `CHEST`, so the flags say which it is; the player's own screen is none of them.
        if event.get_view().get_type() != "CRAFTING" {
            if user.is_inv_see() {
                let refused = match user.get_inv_see_owner() {
                    Some(owner) => !user.is_authorized("essentials.invsee.modify") || ess().get_user_by_uuid(&owner).is_none_or(|owner| owner.is_authorized("essentials.invsee.preventmodify") || !owner.is_online()),
                    None => event.get_click() != "MIDDLE",
                };
                if refused {
                    event.set_cancelled(true);
                }
                return;
            }
            if user.is_ender_see() {
                if !user.is_authorized("essentials.enderchest.modify") {
                    event.set_cancelled(true);
                }
                return;
            }
            if user.is_recipe_see() {
                event.set_cancelled(true);
                return;
            }
        }
        // The direct `/hat`: a block dropped on the helmet slot of the player's own screen.
        let Some(cursor) = event.get_cursor().filter(|cursor| !cursor.get_type().is_air()) else { return };
        if !settings().is_direct_hat_allowed() || event.get_view().get_type() != "CRAFTING" || event.get_click() != "LEFT" || event.get_slot() != 39 {
            return;
        }
        let head = player.get_inventory().get_helmet();
        let bound = head.as_ref().is_some_and(|head| head.has_enchant("binding_curse")) && !user.is_authorized("essentials.hat.ignore-binding");
        if cursor.get_type().get_max_durability() == 0 && !is_skull(cursor.get_type()) && user.is_authorized("essentials.hat") && !user.is_authorized(&format!("essentials.hat.prevent-type.{}", cursor.get_type().name().to_lowercase())) && !bound {
            event.set_cancelled(true);
            player.get_inventory().set_helmet(Some(&cursor));
            event.set_cursor(head.as_ref());
        }
    });
    plugin.register_event_with(LOWEST, true, |event: &mut InventoryDragEvent| {
        let Some(user) = event.get_who_clicked().and_then(|player| ess().get_user(&player)) else { return };
        // Java asks for `InventoryType.PLAYER`: the live `/invsee` is a chest window here, so its owner
        // says it. And `/recipe`'s workbench is only to look at: what's dropped in it wouldn't come back.
        let live_view = user.is_inv_see() && user.get_inv_see_owner().is_some();
        if user.is_recipe_see() || (live_view && event.get_new_items().keys().any(|slot| inventories::is_bottom_inventory_slot(*slot))) {
            event.set_cancelled(true);
        }
    });
    plugin.register_event_with(MONITOR, false, |event: &mut InventoryCloseEvent| {
        let Some(player) = event.get_player() else { return };
        if let Some(user) = ess().get_user(&player) {
            user.set_inv_see(false);
            user.set_ender_see(false);
            user.set_recipe_see(false);
        }
    });
    plugin.register_event_with(HIGH, true, |event: &mut PlayerBucketEmptyEvent| {
        let Some(user) = event.get_player().and_then(|player| ess().get_user(&player)) else { return };
        if let Some(bucket) = event.get_item_stack().filter(|bucket| user.has_unlimited(bucket)) {
            event.set_item_stack(&bucket);
        }
    });
}
