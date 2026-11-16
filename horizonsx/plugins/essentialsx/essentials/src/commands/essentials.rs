//! `Commandessentials`, the tree of `commands/essentials`. `EssentialsTreeCommand`'s nodes are the
//! arms of the two matches at the end.
use crate::prelude::*;
use crate::utils::string::to_uuid;
use horizon_bukkit::get_online_players;
use std::sync::atomic::{AtomicU64, Ordering};

pub const COMMAND: Command = Command { run_source: Some(run), tab_source: Some(tab), ..Command::new("essentials") };

/// `publicNodes`, in `registerNode`'s order.
const PUBLIC_NODES: [&str; 13] = ["version", "ver", "debug", "verbose", "cmd", "commands", "dump", "reload", "cleanup", "homes", "usermap", "", ""];

const OFFICIAL: [&str; 8] = ["EssentialsAntiBuild", "EssentialsChat", "EssentialsDiscord", "EssentialsDiscordLink", "EssentialsGeoIP", "EssentialsProtect", "EssentialsSpawn", "EssentialsXMPP"];

fn version() -> String {
    ess().plugin().get_version().to_owned()
}

fn run_version(sender: &CommandSource) -> EssResult {
    if sender.get_user().is_some_and(|user| !user.is_authorized("essentials.version")) {
        return Ok(());
    }
    let own = version();
    // `getServerSupportStatus`: 2.22.0 doesn't list this game version, on Paper as here, so `OUTDATED`.
    sender.send_tl("versionOutputWarn", crate::args!["Server", horizon_bukkit::get_version()]);
    sender.send_tl("versionOutputWarn", crate::args!["Brand", horizon_bukkit::get_name()]);
    sender.send_tl("versionOutputFine", crate::args!["EssentialsX", own.clone()]);
    let others = horizon_bukkit::lead2::get_plugins();
    for name in crate::modules().iter().map(|name| (*name).to_owned()).chain(others) {
        if OFFICIAL.iter().any(|official| official.eq_ignore_ascii_case(&name)) {
            sender.send_tl("versionOutputFine", crate::args![name.replace("Essentials", "EssentialsX"), own.clone()]);
        }
    }
    let layer = if settings().is_eco_disabled() { "Disabled" } else { "None" };
    sender.send_tl("versionOutputEconLayer", crate::args![layer]);
    sender.send_tl("versionOutputVaultMissing", &[]);
    sender.send_tl("serverUnsupported", &[]);
    sender.send_tl("versionFetching", &[]);
    let told = sender.clone();
    ess().run_task_asynchronously(move || {
        let messages = crate::update_check::version_messages(true, true);
        ess().schedule_sync_delayed_task(move || {
            for (key, args) in messages {
                told.send_tl(key, &args);
            }
        });
    });
    Ok(())
}

/// The plugins in a dump: `alphabetical`, by upper case name.
fn dump_plugins() -> (serde_json::Value, serde_json::Value) {
    use serde_json::json;
    let mut names: Vec<String> = horizon_bukkit::lead2::get_plugins().into_iter().chain(crate::modules().iter().map(|name| (*name).to_owned())).collect();
    names.sort_by_key(|name| name.to_uppercase());
    let version = version();
    let row = |name: &String| json!({ "name": name, "version": version, "enabled": true, "official": name == "Essentials" || OFFICIAL.contains(&name.as_str()), "unsupported": false, "authors": [] });
    let addons: Vec<_> = names.iter().filter(|name| name.starts_with("Essentials") && *name != "Essentials").map(row).collect();
    let plugins: Vec<_> = names.iter().map(row).collect();
    (json!(addons), json!(plugins))
}

/// `DumpCommand`: the server's facts, and the files asked for, as a paste.
fn run_dump(sender: &CommandSource, args: &[String]) -> EssResult {
    use serde_json::json;
    sender.send_tl("dumpCreating", &[]);
    let player = sender.get_player().cloned();
    let (addons, plugins) = dump_plugins();
    let dump = json!({
        "meta": { "timestamp": now_ms(), "sender": player.as_ref().map(|player| player.get_name().to_owned()), "senderUuid": player.as_ref().map(horizon_bukkit::Player::get_unique_id) },
        "server-data": {
            "bukkit-version": horizon_bukkit::get_version(), "server-version": horizon_bukkit::get_version(), "server-brand": horizon_bukkit::get_name(),
            "online-mode": if horizon_bukkit::server::get_online_mode() { "Online Mode" } else { "Offline Mode" },
            "support-status": { "status": "OUTDATED", "supported": false, "trigger": serde_json::Value::Null },
        },
        "environment": { "operating-system": std::env::consts::OS, "uptime": format_date_diff(ess().start_time()) },
        "ess-data": {
            "version": version(),
            "update-data": { "id": version(), "branch": "2.x", "dev": false },
            "economy-layer": { "enabled": !settings().is_eco_disabled(), "selected-layer": false, "name": "null", "layer-version": "null", "backend-name": "null" },
            "addons": addons,
        },
        "plugins": plugins,
    });
    let wants = |word: &str| args.iter().any(|arg| arg == "*" || arg.eq_ignore_ascii_case("all") || arg.eq_ignore_ascii_case(word));
    let (config, discord, kits, log, worth, tpr, spawns, commands) = (wants("config"), wants("discord"), wants("kits"), wants("log"), wants("worth"), wants("tpr"), wants("spawns"), wants("commands"));
    let known: Vec<(String, String, String)> = horizon_bukkit::fam_core::get_command_map().get_known_commands().iter().map(|(word, command)| (word.clone(), command.get_name().to_owned(), command.get_description().to_owned())).collect();
    let disabled = crate::alternative_commands::disabled_commands();
    let folder = crate::data_folder();
    let plugins_folder = folder.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let sender = sender.clone();
    ess().run_task_asynchronously(move || {
        let mut files = vec![("dump.json".to_owned(), dump.to_string())];
        let mut failures = Vec::new();
        let mut add = |name: &str, path: std::path::PathBuf, censor: fn(String) -> String| match std::fs::read_to_string(&path) {
            Ok(text) => files.push((name.to_owned(), censor(text))),
            Err(error) => failures.push((name.to_owned(), error.to_string())),
        };
        if config {
            add("config.yml", folder.join("config.yml"), |text| text);
        }
        if discord && crate::modules().contains(&"EssentialsDiscord") {
            add("discord-config.yml", plugins_folder.join("EssentialsDiscord").join("config.yml"), censor_token);
            if crate::modules().contains(&"EssentialsDiscordLink") {
                add("discord-link-config.yml", plugins_folder.join("EssentialsDiscordLink").join("config.yml"), |text| text);
            }
        }
        if kits {
            add("kits.yml", folder.join("kits.yml"), |text| text);
        }
        if log {
            add("latest.log", std::path::PathBuf::from("logs/latest.log"), censor_log);
        }
        if worth {
            add("worth.yml", folder.join("worth.yml"), |text| text);
        }
        if tpr {
            add("tpr.yml", folder.join("tpr.yml"), |text| text);
        }
        if spawns && crate::modules().contains(&"EssentialsSpawn") {
            add("spawn.yml", folder.join("spawn.yml"), |text| text);
        }
        if commands {
            add("commands.yml", std::path::PathBuf::from("commands.yml"), |text| text);
            let map: serde_json::Map<String, serde_json::Value> = known.iter().map(|(word, name, description)| (word.clone(), serde_json::json!({ "name": name, "description": description }))).collect();
            files.push(("commandmap.json".to_owned(), serde_json::to_string_pretty(&map).unwrap_or_default()));
            files.push(("commandoverride.json".to_owned(), format!("{disabled:?}")));
        }
        let result = crate::paste::create_paste(&files);
        ess().schedule_sync_delayed_task(move || {
            for (name, error) in failures {
                sender.send_tl("dumpErrorUpload", crate::args![name, error]);
            }
            match result {
                Ok((id, _)) => {
                    let url = format!("https://essentialsx.net/dump?bytebin={id}");
                    sender.send_tl("dumpUrl", crate::args![url.clone()]);
                    if sender.is_player() {
                        crate::info(&mini_to_legacy(&tl_literal("dumpConsoleUrl", crate::args![url])));
                    }
                }
                Err(error) => sender.send_tl("dumpError", crate::args![error]),
            }
        });
    });
    Ok(())
}

/// A bot's token, in a Discord config.
fn censor_token(text: String) -> String {
    use std::sync::LazyLock;
    static TOKEN: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"[A-Za-z\d]{24}\.[\w-]{6}\.[\w-]{27}").expect("a valid pattern"));
    TOKEN.replace_all(&text, "<censored token>").into_owned()
}

/// A log, with no addresses in it.
fn censor_log(text: String) -> String {
    use std::sync::LazyLock;
    static ADDRESS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"(?:[0-9]{1,3}\.){3}[0-9]{1,3}").expect("a valid pattern"));
    ADDRESS.replace_all(&text, "<censored ip address>").into_owned()
}

fn run_debug(sender: &CommandSource, args: &[String]) -> EssResult {
    let word = args.first().map(|first| first.to_lowercase());
    let state = match word.as_deref() {
        Some("on" | "true") => true,
        Some("off" | "false") => false,
        _ => !settings().is_debug(),
    };
    settings().set_debug(state);
    sender.send_message(&format!("Essentials {} debug mode {}", version(), if settings().is_debug() { "enabled" } else { "disabled" }));
    Ok(())
}

fn run_cleanup(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    if args.first().is_none_or(|first| !is_int(first)) {
        sender.send_message("This sub-command will delete users who haven't logged in in the last <days> days.");
        sender.send_message("Optional parameters define the minimum amount required to prevent deletion.");
        sender.send_message("Unless you define larger default values, this command will ignore people who have more than 0 money/homes.");
        return Err(EssError::Message(format!("/{label} cleanup <days> [money] [homes]")));
    }
    sender.send_tl("cleaning", &[]);
    let days: i64 = args[0].parse().unwrap_or(0);
    let money = match args.get(1) {
        Some(text) => parse_double(&text.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect::<String>())?,
        None => 0.0,
    };
    let homes = args.get(2).filter(|text| is_int(text)).and_then(|text| text.parse().ok()).unwrap_or(0_usize);
    let sender = sender.clone();
    ess().run_task_asynchronously(move || {
        let current = now_ms();
        for uuid in ess().users().get_all_user_uuids() {
            let Some(user) = ess().users().load_uncached_user(&uuid) else { continue };
            let mut last = user.get_last_logout();
            if last == 0 {
                last = user.get_last_login();
            }
            if last == 0 {
                user.set_last_login(current);
            }
            if user.is_npc() {
                continue;
            }
            let (home_count, money_count) = (user.get_homes().len(), user.get_money().to_f64());
            if last == 0 || current - last < days * 86_400_000 || home_count > homes || money_count > money {
                continue;
            }
            if settings().is_debug() {
                crate::info(&format!("Deleting user: {} Money: {money_count} Homes: {home_count} Last seen: {}", user.get_name(), format_date_diff(last)));
            }
            user.reset();
        }
        answer(sender, "cleaned", Vec::new());
    });
    Ok(())
}

/// A message from a thread of its own, on the server's.
fn answer(sender: CommandSource, key: &'static str, args: Vec<crate::i18n::Arg>) {
    let mut task = Some(move || sender.send_tl(key, &args));
    ess().schedule_sync_delayed_task(move || {
        if let Some(task) = task.take() {
            task();
        }
    });
}

fn run_homes(sender: &CommandSource, args: &[String]) -> EssResult {
    const USAGE: &str = "/<command> homes (fix | delete [world])";
    let Some(first) = args.first() else {
        sender.send_message("This sub-command provides a utility to mass-delete homes based on user options:");
        sender.send_message("Use \"fix\" to delete all homes inside non-existent or unloaded worlds.");
        sender.send_message("Use \"delete\" to delete all existing homes.");
        sender.send_message("Use \"delete <worldname>\" to delete all homes inside a specific world.");
        return Err(EssError::Message(USAGE.to_owned()));
    };
    match first.as_str() {
        "fix" => {
            sender.send_tl("fixingHomes", &[]);
            let sender = sender.clone();
            ess().run_task_asynchronously(move || {
                for_each_home(|user, home| user.get_home(home).is_none());
                answer(sender, "fixedHomes", Vec::new());
            });
        }
        "delete" => {
            let world = args.get(1).cloned();
            if let Some(name) = &world {
                fail_if!(horizon_bukkit::get_world(name).is_none(), EssError::tl("invalidWorld", &[]));
                sender.send_tl("deletingHomesWorld", crate::args![name]);
            } else {
                sender.send_tl("deletingHomes", &[]);
            }
            let sender = sender.clone();
            ess().run_task_asynchronously(move || {
                for_each_home(|user, home| world.as_ref().is_none_or(|name| user.get_home(home).is_some_and(|at| at.world.get_name() == name)));
                match world {
                    Some(name) => answer(sender, "deletedHomesWorld", vec![name.into()]),
                    None => answer(sender, "deletedHomes", Vec::new()),
                }
            });
        }
        _ => return Err(EssError::Message(USAGE.to_owned())),
    }
    Ok(())
}

fn for_each_home(doomed: impl Fn(&crate::user::User, &str) -> bool) {
    for uuid in ess().users().get_all_user_uuids() {
        let Some(user) = ess().users().load_uncached_user(&uuid) else { continue };
        for home in user.get_homes() {
            if doomed(&user, &home) && user.del_home(&home).is_err() {
                crate::info(&format!("Unable to delete home {home} for {}", user.get_name()));
            }
        }
    }
}

fn run_item_test(sender: &CommandSource, args: &[String]) -> EssResult {
    let Some(player) = sender.get_player().filter(|_| sender.is_authorized("essentials.itemtest")) else { return Ok(()) };
    let Some(first) = args.first() else { return Ok(()) };
    let stack = |id: &str, amount: i32| ItemStack::new(Material::of(id), amount);
    match first.as_str() {
        "slot" => {
            if let Some(slot) = args.get(1) {
                crate::inventories::set_slot(player, slot.parse().map_err(|_| EssError::Message(format!("For input string: \"{slot}\"")))?, Some(&stack("DIRT", 1)));
            }
        }
        "overfill" => {
            let left = crate::inventories::add_item(player, 42, false, &[stack("DIAMOND_SWORD", 1), stack("DIRT", 32), stack("DIRT", 32)]);
            sender.send_message(&format!("{left:?}"));
        }
        "overfill2" => {
            let (Some(armor), Some(add)) = (args.get(1), args.get(2)) else {
                return Ok(());
            };
            let armor = armor.eq_ignore_ascii_case("true");
            let items = [stack("DIAMOND_SWORD", 1), stack("DIRT", 32), stack("DIRT", 32), stack("DIAMOND_HELMET", 4), stack("CHAINMAIL_LEGGINGS", 1)];
            if crate::inventories::has_space(player, 0, armor, &items) {
                if add.eq_ignore_ascii_case("true") {
                    sender.send_message(&format!("{:?}", crate::inventories::add_item(player, 0, armor, &items)));
                }
                sender.send_message("SO MUCH SPACE!");
            } else {
                sender.send_message("No space!");
            }
        }
        "remove" => {
            if let Some(armor) = args.get(1) {
                crate::inventories::remove_item_exact(player, &stack("PUMPKIN", 1), armor.eq_ignore_ascii_case("true"));
            }
        }
        _ => {}
    }
    Ok(())
}

const MOO_SOUND: &str = "minecraft:entity.cow.milk";
const CONSOLE_MOO: [&str; 7] = ["         (__)", "         (oo)", "   /------\\/", "  / |    ||", " *  /\\---/\\", "    ~~   ~~", "....\"Have you mooed today?\"..."];
const PLAYER_MOO: [&str; 7] = ["            (__)", "            (oo)", "   /------\\/", "  /  |      | |", " *  /\\---/\\", "    ~~    ~~", "....\"Have you mooed today?\"..."];

fn moo_to(player: &horizon_bukkit::Player) {
    for line in PLAYER_MOO {
        player.send_message(line);
    }
    player.play_sound(&player.get_location(), MOO_SOUND, 1.0, 1.0);
}

fn run_moo(sender: &CommandSource, args: &[String]) -> EssResult {
    if matches!(args, [only] if only == "moo") {
        for line in CONSOLE_MOO {
            crate::info(line);
        }
        for player in get_online_players() {
            moo_to(&player);
        }
    } else if let Some(player) = sender.get_player() {
        moo_to(player);
    } else {
        for line in CONSOLE_MOO {
            sender.send_message(line);
        }
    }
    Ok(())
}

const NOTE_HARP: &str = "minecraft:block.note_block.harp";
const NYAN_TUNE: &str = include_str!("nyan_tune.txt");
/// Which tune is playing: a new one makes the one before it stop.
static CURRENT_TUNE: AtomicU64 = AtomicU64::new(0);

fn note_pitch(note: &str) -> Option<f32> {
    Some(match note {
        "1F#" => 0.5,
        "1G" => 0.53,
        "1G#" => 0.56,
        "1A" => 0.6,
        "1A#" => 0.63,
        "1B" => 0.67,
        "1C" => 0.7,
        "1C#" => 0.76,
        "1D" => 0.8,
        "1D#" => 0.84,
        "1E" => 0.9,
        "1F" => 0.94,
        "2F#" => 1.0,
        "2G" => 1.06,
        "2G#" => 1.12,
        "2A" => 1.18,
        "2A#" => 1.26,
        "2B" => 1.34,
        "2C" => 1.42,
        "2C#" => 1.5,
        "2D" => 1.6,
        "2D#" => 1.68,
        "2E" => 1.78,
        "2F" => 1.88,
        _ => return None,
    })
}

fn play_note(tune: u64, index: usize, delay: u64) {
    ess().schedule_sync_delayed_task_later(
        move || {
            if CURRENT_TUNE.load(Ordering::Relaxed) != tune {
                return;
            }
            let notes: Vec<&str> = NYAN_TUNE.trim().split(',').collect();
            let Some(note) = notes.get(index) else { return };
            if let Some(pitch) = note_pitch(note) {
                for player in get_online_players() {
                    player.play_sound(&player.get_location(), NOTE_HARP, 1.0, pitch);
                }
            }
            play_note(tune, index + 1, 2);
        },
        delay,
    );
}

fn run_nyan() -> EssResult {
    let tune = CURRENT_TUNE.fetch_add(1, Ordering::Relaxed) + 1;
    play_note(tune, 0, 20);
    Ok(())
}

fn run_usermap(sender: &CommandSource, args: &[String]) -> EssResult {
    if !sender.is_authorized("essentials.usermap") {
        return Ok(());
    }
    let users = ess().users();
    sender.send_tl("usermapSize", crate::args![users.get_cached_count(), users.get_user_count(), settings().get_max_user_cache_count()]);
    let Some(first) = args.first() else { return Ok(()) };
    let cache = users.get_name_cache();
    if first == "full" {
        for (name, uuid) in &cache {
            sender.send_tl("usermapEntry", crate::args![name, uuid]);
        }
    } else if first == "purge" {
        let seppuku = args.get(1).is_some_and(|second| second == "iknowwhatimdoing");
        sender.send_tl("usermapPurge", crate::args![seppuku.to_string()]);
        let uuids = users.get_all_user_uuids();
        ess().run_task_asynchronously(move || purge_userdata(&uuids, seppuku));
    } else if first.eq_ignore_ascii_case("cache") {
        sender.send_tl("usermapKnown", crate::args![users.get_all_user_uuids().len(), cache.len()]);
    } else if let Some(uuid) = to_uuid(first) {
        for (name, known) in &cache {
            if *known == uuid {
                sender.send_tl("usermapEntry", crate::args![name, first]);
            }
        }
    } else {
        let name = users.get_sanitized_name(first);
        let uuid = cache.get(&name).cloned();
        sender.send_tl("usermapEntry", crate::args![name, uuid]);
    }
    Ok(())
}

fn purge_userdata(uuids: &[String], seppuku: bool) {
    let folder = ess().plugin().get_data_folder();
    let userdata = folder.join("userdata");
    let backup = folder.join(format!("userdata-npc-backup-boogaloo-{}", now_ms()));
    if !userdata.is_dir() {
        crate::warning("Missing userdata folder, aborting usermap purge.");
        return;
    }
    if seppuku && std::fs::create_dir(&backup).is_err() {
        crate::warning("Unable to create backup folder, aborting usermap purge.");
        return;
    }
    let mut total = 0;
    for file in std::fs::read_dir(&userdata).into_iter().flatten().flatten() {
        let name = file.file_name().to_string_lossy().into_owned();
        let Some(uuid) = name.strip_suffix(".yml").and_then(to_uuid) else { continue };
        if uuids.contains(&uuid) {
            continue;
        }
        total += 1;
        crate::warning(&format!("Found orphaned userdata file: {name}"));
        if seppuku && std::fs::rename(file.path(), backup.join(&name)).is_err() {
            crate::warning(&format!("Unable to move orphaned userdata file: {name}"));
        }
    }
    crate::info(&format!("Found {total} orphaned userdata files."));
}

fn run(sender: &CommandSource, label: &str, args: &[String]) -> EssResult {
    let first = args.first().ok_or_else(EssError::not_enough)?;
    let rest = &args[1..];
    match first.as_str() {
        "version" | "ver" => run_version(sender),
        "debug" | "verbose" => run_debug(sender, rest),
        "cmd" | "commands" => {
            let disabled = crate::alternative_commands::disabled_commands();
            if disabled.is_empty() {
                sender.send_tl("blockListEmpty", &[]);
                return Ok(());
            }
            sender.send_tl("blockList", &[]);
            for (label, alternative) in disabled {
                sender.send_message(&format!("{label} => {alternative}"));
            }
            Ok(())
        }
        "dump" => run_dump(sender, rest),
        "reload" => {
            ess().reload();
            sender.send_tl("essentialsReload", crate::args![version()]);
            Ok(())
        }
        "cleanup" => run_cleanup(sender, label, rest),
        "homes" => run_homes(sender, rest),
        "usermap" => run_usermap(sender, rest),
        "itemtest" => run_item_test(sender, rest),
        "nyan" | "nya" => run_nyan(),
        "moo" => run_moo(sender, rest),
        _ => Err(EssError::not_enough()),
    }
}

fn tab(sender: &CommandSource, _label: &str, args: &[String]) -> Option<Vec<String>> {
    if args.len() == 1 {
        return Some(PUBLIC_NODES.iter().filter(|node| !node.is_empty()).map(|node| (*node).to_owned()).collect());
    }
    let rest = &args[1..];
    let list = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
    Some(match args[0].as_str() {
        "debug" | "verbose" if rest.len() == 1 => list(&["on", "off"]),
        "debug" | "verbose" | "cmd" | "commands" | "version" | "ver" | "reload" => {
            if matches!(args[0].as_str(), "cmd" | "commands" | "version" | "ver" | "reload") {
                get_players(sender)
            } else {
                Vec::new()
            }
        }
        "cleanup" if rest.len() == 1 => COMMON_DURATIONS.iter().map(|duration| (*duration).to_owned()).collect(),
        "cleanup" if rest.len() < 4 => list(&["-1", "0"]),
        "homes" if rest.len() == 1 => list(&["fix", "delete"]),
        "homes" if rest.len() == 2 && rest[0].eq_ignore_ascii_case("delete") => get_worlds().iter().map(|world| world.get_name().to_owned()).collect(),
        "usermap" if rest.len() == 1 => list(&["full", "purge", "cache"]),
        "moo" if rest.len() == 1 => list(&["moo"]),
        "dump" => {
            let mut options = list(&["config", "kits", "log", "discord", "worth", "tpr", "spawns", "commands", "all"]);
            if rest.iter().any(|word| word == "*" || word.eq_ignore_ascii_case("all")) {
                options.clear();
            }
            options.retain(|option| !rest.iter().any(|word| word.eq_ignore_ascii_case(option)));
            options
        }
        "itemtest" | "nyan" | "nya" => get_players(sender),
        _ => Vec::new(),
    })
}
