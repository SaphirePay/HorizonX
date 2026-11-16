//! `EssentialsSpawn`. `UserTeleportSpawnEvent` isn't fired: nothing here can listen.
use essentials::prelude::*;
use essentials::command_source::console_display_name;
use essentials::config::Config;
use essentials::config_entities::LazyLocation;
use essentials::kits::Kit;
use essentials::teleport::RESPAWN_HOOK;
use essentials::textreader::KeywordReplacer;
use horizon_bukkit::event::{PlayerJoinEvent, PlayerRespawnEvent};
use horizon_bukkit::Plugin;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// `SpawnStorage`.
struct SpawnStorage {
    config: Config,
    spawns: HashMap<String, LazyLocation>,
}

static STORAGE: LazyLock<Mutex<SpawnStorage>> = LazyLock::new(|| {
    let mut config = Config::new(essentials::data_folder().join("spawn.yml"));
    config.load();
    let spawns = read_spawns(&config);
    Mutex::new(SpawnStorage { config, spawns })
});

/// `getLocationSectionMap("spawns")`.
fn read_spawns(config: &Config) -> HashMap<String, LazyLocation> {
    config.section_map("spawns").into_iter().filter_map(|(group, node)| Some((group, LazyLocation::read(&node)?))).collect()
}

fn set_spawn(location: &Location, group: &str) {
    let group = group.to_lowercase();
    let mut storage = STORAGE.lock().unwrap_or_else(|e| e.into_inner());
    let lazy = LazyLocation::from_location(location);
    storage.config.set_location(Some(&format!("spawns.{group}")), &lazy);
    storage.spawns.insert(group, lazy);
    storage.config.save();
}

fn world_spawn() -> Option<Location> {
    let worlds = get_worlds();
    worlds.iter().find(|world| world.get_environment() == "NORMAL").or(worlds.first()).map(horizon_bukkit::World::get_spawn_location)
}

/// `getSpawn`.
fn get_spawn(group: Option<&str>) -> Option<Location> {
    let Some(group) = group else { return world_spawn() };
    let mut storage = STORAGE.lock().unwrap_or_else(|e| e.into_inner());
    let group = group.to_lowercase();
    if let Some(found) = storage.spawns.get_mut(&group) {
        return found.location();
    }
    match storage.spawns.get_mut("default") {
        Some(fallback) => fallback.location(),
        None => {
            drop(storage);
            world_spawn()
        }
    }
}

const SET_SPAWN: Command = Command { run_user: Some(run_setspawn), tab_source: Some(|_, _, _| Some(Vec::new())), ..Command::new("setspawn") };

fn run_setspawn(user: &User, _label: &str, args: &[String]) -> EssResult {
    let group = if args.is_empty() { "default".to_owned() } else { get_final_arg(args, 0) };
    if let Some(location) = user.get_location() {
        set_spawn(&location, &group);
    }
    user.send_tl("spawnSet", essentials::args![group]);
    Ok(())
}

const SPAWN: Command = Command { run_user: Some(run_spawn_user), run_source: Some(run_spawn_source), tab_source: Some(tab_spawn), ..Command::new("spawn") };

fn run_spawn_user(user: &User, label: &str, args: &[String]) -> EssResult {
    let charge = Trade::of_command("spawn");
    charge.is_affordable_for(user)?;
    let future = EssFuture::new();
    let target = if !args.is_empty() && user.is_authorized("essentials.spawn.others") {
        let other = get_player_for(user, args, 0)?;
        if other != *user {
            let (told, name) = (other.clone(), user.get_display_name());
            future.then_accept(move |success| {
                if success {
                    told.send_tl("teleportAtoB", essentials::args![name, "spawn"]);
                }
            });
        }
        other
    } else {
        user.clone()
    };
    respawn(&user.get_source(), Some(user), &target, Some(&charge), label, future);
    Err(EssError::NoCharge)
}

fn run_spawn_source(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    fail_if!(args.is_empty(), EssError::not_enough());
    let user = get_player_raw(None, args, 0, true, false)?;
    let future = EssFuture::new();
    let told = user.clone();
    future.then_accept(move |success| {
        if success {
            told.send_tl("teleportAtoB", essentials::args![console_display_name(), "spawn"]);
        }
    });
    respawn(sender, None, &user, None, label, future);
    Ok(())
}

fn respawn(sender: &CommandSource, owner: Option<&User>, teleportee: &User, charge: Option<&Trade>, label: &str, future: EssFuture) {
    let Some(spawn) = get_spawn(Some(&teleportee.get_group())) else { return };
    let (failed, label) = (sender.clone(), label.to_owned());
    future.exceptionally(move |error| ess().show_error(&failed, error, &label));
    let (done, at) = (sender.clone(), spawn.clone());
    future.then_accept(move |success| {
        if success {
            done.send_tl("teleporting", essentials::args![at.world.get_name(), at.get_block_x(), at.get_block_y(), at.get_block_z()]);
        }
    });
    match owner {
        None => teleportee.get_async_teleport().now(&spawn, false, "COMMAND", future),
        Some(owner) => owner.get_async_teleport().teleport_player(teleportee, &spawn, charge, "COMMAND", future),
    }
}

fn tab_spawn(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    Some(if args.len() == 1 && sender.is_authorized("essentials.spawn.others") { get_players(sender) } else { Vec::new() })
}

/// `tryRandomTeleport`.
fn try_random_teleport(user: &User, name: &str) -> bool {
    if !ess().random_teleport().has_location(name) {
        return false;
    }
    let target = user.clone();
    ess().random_teleport().get_random_location(name, move |location| {
        target.get_async_teleport().now(&location, false, "PLUGIN", EssFuture::new());
    });
    true
}

/// `onPlayerRespawn`: where to come back, `None` to leave it as it is.
fn respawn_location(user: &User) -> Option<Location> {
    if user.is_jailed() && user.get_jail().is_some_and(|jail| !jail.is_empty()) {
        return None;
    }
    if settings().get_respawn_at_home() {
        let mut home = None;
        if settings().is_respawn_at_bed() {
            home = user.player().and_then(|player| player.get_respawn_location()).filter(|at| at.get_block().get_type().name() != "RESPAWN_ANCHOR" || settings().is_respawn_at_anchor());
        }
        if let Some(home) = home.or_else(|| user.get_location().and_then(|near| user.get_home_near(&near))) {
            return Some(home);
        }
    }
    if try_random_teleport(user, &settings().get_random_respawn_location()) {
        return None;
    }
    get_spawn(Some(&user.get_group()))
}

fn later(delay: u64, task: impl FnOnce() + Send + 'static) {
    let mut task = Some(task);
    ess().schedule_sync_delayed_task_later(
        move || {
            if let Some(task) = task.take() {
                task();
            }
        },
        delay,
    );
}

/// `delayedJoin`.
fn delayed_join(user: User, played_before: bool) {
    if played_before {
        if !settings().get_spawn_on_join_groups().is_empty() && settings().is_user_in_spawn_on_join_group(&user) && !user.is_authorized("essentials.spawn-on-join.exempt") {
            later(0, move || {
                let Some(spawn) = get_spawn(Some(&user.get_group())) else { return };
                let future = EssFuture::new();
                let source = user.get_source();
                future.exceptionally(move |error| ess().show_error(&source, error, "spawn-on-join"));
                user.get_async_teleport().now_unsafe(&spawn, future);
            });
        }
        return;
    }
    let spawned = try_random_teleport(&user, &settings().get_random_spawn_location());
    if !spawned && !settings().get_newbie_spawn().eq_ignore_ascii_case("none") {
        let target = user.clone();
        later(1, move || {
            if !target.is_online() {
                return;
            }
            if let Some(spawn) = get_spawn(Some(&settings().get_newbie_spawn())) {
                let future = EssFuture::new();
                future.exceptionally(|_| essentials::warning(&essentials::i18n::tl_literal("teleportNewPlayerError", &[])));
                target.get_async_teleport().now(&spawn, false, "PLUGIN", future);
            }
        });
    }
    later(2, move || {
        if !user.is_online() {
            return;
        }
        if settings().get_announce_new_players() {
            let output = KeywordReplacer::new(settings().get_announce_new_player_format(), &user.get_source(), true, false);
            for line in output.get_lines() {
                ess().broadcast_message_from(&user, line);
            }
        }
        let kit = settings().get_new_player_kit();
        if !kit.is_empty()
            && let Err(error) = Kit::new(&kit.to_lowercase()).and_then(|kit| kit.expand_items(&user))
        {
            essentials::warning(&error.get_message());
        }
    });
}

/// `onEnable`.
pub fn on_enable(plugin: Plugin) {
    LazyLock::force(&STORAGE);
    register_commands(plugin, &[SET_SPAWN, SPAWN]);
    let _ = RESPAWN_HOOK.set(|user, target| respawn_location(user).unwrap_or(target));
    if let Some(priority) = settings().get_respawn_priority() {
        let name = horizon_bukkit::enums::event_priority::values().iter().copied().find(|known| *known == priority).unwrap_or("NORMAL");
        plugin.register_event_with(name, false, |event: &mut PlayerRespawnEvent| {
            if let Some(user) = ess().get_user(event.get_player())
                && let Some(location) = respawn_location(&user)
            {
                event.set_respawn_location(&location);
            }
        });
    }
    if let Some(priority) = settings().get_spawn_join_priority() {
        let name = horizon_bukkit::enums::event_priority::values().iter().copied().find(|known| *known == priority).unwrap_or("NORMAL");
        plugin.register_event_with(name, false, |event: &mut PlayerJoinEvent| {
            if let Some(user) = ess().get_user(event.get_player()) {
                // `hasPlayedBefore`: nothing here says it of someone online, so it's any time played at all.
                delayed_join(user, essentials::ess::play_one_tick(event.get_player()) > 0);
            }
        });
    }
}
