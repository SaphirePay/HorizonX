//! `/mv` and its sub-commands, and the legacy `/mvtp`-style aliases.

use crate::dest;
use crate::util::{Flags, m, on_main, say, split_flags, tell};
use crate::worlds::{self, Kind, MvWorld};
use horizon_bukkit::{CommandSender, Location, Player};
use horizon_kit::cmd::Messages;
use horizon_kit::{Ctx, Res, Router, Sub};
use serde_yaml::Value as Y;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

pub static MV: LazyLock<Router> = LazyLock::new(|| {
    Router::new("mv")
        .messages(Messages {
            no_permission: "&cI'm sorry, but you do not have permission to perform this command.".into(),
            usage: "&cUsage: &b/{label} {usage}".into(),
            unknown: "&cUnknown Command, please type &b/mv help".into(),
            player_only: "&cThis command can only be used by players".into(),
            help_title: "&a====[ Multiverse Help ]====".into(),
            help_line: "&b/{label} {usage} &7- &f{about}".into(),
        })
        .bare(root)
        .sub(Sub::new("help", help).perm("multiverse.core.help").usage("help [filter] [page]").about("Show Multiverse command help"))
        .sub(Sub::new("create", create).perm("multiverse.core.create").args(2).usage("create <name> <environment> [--seed <seed> --generator <generator[:id]> --world-type <worldtype> --no-adjust-spawn --no-structures --generate-bonus-chest --biome <biome> --generator-settings <json> --properties <p=v,...>]").about("Creates a new world and loads it.").tab(tab_create))
        .sub(Sub::new("import", import).perm("multiverse.core.import").args(2).usage("import <name> <environment> [--generator <generator[:id]> --no-adjust-spawn --biome <biome> --skip-folder-check --generator-settings <json>]").about("Imports an existing world folder.").tab(tab_import))
        .sub(Sub::new("load", load).perm("multiverse.core.load").args(1).usage("load <world> [--skip-folder-check]").about("Loads a world.").tab(tab_unloaded))
        .sub(Sub::new("unload", unload).perm("multiverse.core.unload").args(1).usage("unload <world> [--remove-players] [--no-save]").about("Unloads a world from Multiverse.").tab(tab_loaded))
        .sub(Sub::new("remove", remove).perm("multiverse.core.remove").args(1).usage("remove <world> [--remove-players [destination]] [--no-unload-bukkit-world] [--no-save]").about("Unloads a world and removes it from worlds.yml.").tab(tab_worlds))
        .sub(Sub::new("delete", delete).perm("multiverse.core.delete").args(1).usage("delete <world> [--remove-players [destination]]").about("Deletes a world on your server PERMANENTLY.").tab(tab_worlds))
        .sub(Sub::new("regen", regen).perm("multiverse.core.regen").args(1).usage("regen <world> [--seed [seed]] [--reset-world-config] [--reset-gamerules] [--remove-players [destination]]").about("Regenerates a world on your server.").tab(tab_loaded))
        .sub(Sub::new("clone", clone).perm("multiverse.core.clone").args(2).usage("clone <world> <new-world-name> [--reset-world-config --reset-gamerules --no-save]").about("Clones a world.").tab(tab_loaded))
        .sub(Sub::new("list", list).perm("multiverse.core.list.worlds").usage("list [--filter <filter>] [--page <page>] [--raw]").about("Displays a listing of all worlds that you can enter."))
        .sub(Sub::new("info", info).alias("i").perm("multiverse.core.info").usage("info [world] [--page <page>] [--filter <filter>]").about("Displays all properties configured for a world.").tab(tab_loaded))
        .sub(Sub::new("teleport", tp).alias("tp").usage("teleport [player] <destination> [--unsafe --silent]").about("Allows you to teleport to a location on your server!").tab(tab_tp))
        .sub(Sub::new("spawn", spawn).perm("multiverse.core.spawn").usage("spawn [player] [--unsafe]").about("Teleports the specified player to the spawn of the world they are in").tab(horizon_kit::cmd::players))
        .sub(Sub::new("setspawn", setspawn).perm("multiverse.core.spawn.set").usage("setspawn [worldname:x,y,z[,yaw,pitch]] [--unsafe]").about("Sets the spawn location of the specified world"))
        .sub(Sub::new("modify", modify).alias("m").perm("multiverse.core.modify").args(2).usage("modify [world] <set|add|remove|reset> <property> [value]").about("Modifies a world property of a given world.").tab(tab_modify))
        .sub(Sub::new("gamerule", gamerule).alias("rule").args(1).usage("gamerule <set|reset|list> ...").about("Changes or lists gamerules.").tab(tab_gamerule))
        .sub(Sub::new("who", who).perm("multiverse.core.list.who").usage("who [world] [--page <page>] [--filter <filter>]").about("Lists the players in the world specified").tab(tab_loaded))
        .sub(Sub::new("whoall", whoall).perm("multiverse.core.list.who.all").usage("whoall [--page <page>] [--filter <filter>]").about("Lists the players in all worlds"))
        .sub(Sub::new("confirm", confirm).perm("multiverse.core.confirm").usage("confirm [otp]").about("Confirms dangerous commands before executing them."))
        .sub(Sub::new("reload", reload).perm("multiverse.core.reload").about("Reloads config files for all Multiverse modules."))
        .sub(Sub::new("version", version).perm("multiverse.core.version").about("Multiverse-Core version info"))
        .sub(Sub::new("coordinates", coords).alias("coords").alias("coord").alias("co").perm("multiverse.core.coord").player().about("Detailed information on the player's where abouts."))
        .sub(Sub::new("config", config_cmd).alias("conf").perm("multiverse.core.config").args(1).usage("config <name> [value]").about("Show or modify the config.").tab(tab_config))
        .sub(Sub::new("check", check).perm("multiverse.core.check").args(2).usage("check <player> <destination>").about("Checks if a player can teleport to a destination.").tab(horizon_kit::cmd::players))
        .sub(Sub::new("anchor", anchor).alias("anchors").args(1).usage("anchor <set|delete|list> [name] [location]").about("Anchors (named places)").tab(tab_anchor))
});

/// `/mvtp` and the other old names: the router with the sub put in front.
pub const LEGACY: &[(&str, &[&str])] = &[
    ("mvcreate", &["create"]),
    ("mvimport", &["import"]),
    ("mvload", &["load"]),
    ("mvunload", &["unload"]),
    ("mvremove", &["remove"]),
    ("mvdelete", &["delete"]),
    ("mvlist", &["list"]),
    ("mvinfo", &["info"]),
    ("mvtp", &["tp"]),
    ("mvspawn", &["spawn"]),
    ("mvsetspawn", &["setspawn"]),
    ("mvmodify", &["modify"]),
    ("mvgamerule", &["gamerule", "set"]),
    ("mvgamerules", &["gamerule", "list"]),
    ("mvclone", &["clone"]),
    ("mvregen", &["regen"]),
    ("mvreload", &["reload"]),
    ("mvwho", &["who"]),
    ("mvwhoall", &["whoall"]),
    ("mvconfirm", &["confirm"]),
    ("mvversion", &["version"]),
    ("mvhelp", &["help"]),
    ("mvcoord", &["coordinates"]),
    ("mvconfig", &["config"]),
    ("mvcheck", &["check"]),
    ("mvanchor", &["anchor"]),
];

pub fn register(plugin: &horizon_bukkit::Plugin) {
    plugin.set_executor("mv", |s, l, a| MV.run(s, l, a));
    plugin.set_tab_completer("mv", |s, _, a| MV.complete(s, a));
    for (name, prefix) in LEGACY {
        let prefix: &'static [&'static str] = prefix;
        plugin.set_executor(name, move |s, _, a| {
            let mut all: Vec<&str> = prefix.to_vec();
            all.extend_from_slice(a);
            MV.run(s, "mv", &all)
        });
        plugin.set_tab_completer(name, move |s, _, a| {
            let mut all: Vec<&str> = prefix.to_vec();
            all.extend_from_slice(a);
            MV.complete(s, &all)
        });
    }
}

// ---- Helpers ----

fn player_of(c: &Ctx) -> Option<Player> {
    match c.sender {
        CommandSender::Player(p) => Some(p.clone()),
        _ => None,
    }
}

fn online(name: &str) -> Option<Player> {
    horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// A world named in the arguments, else the sender's own (`resolve=issuerAware`).
fn world_arg(c: &Ctx, args: &[String], loaded: bool) -> Result<(MvWorld, usize), String> {
    if let Some(first) = args.first()
        && let Some(w) = worlds::find(first)
    {
        if loaded && !w.is_loaded() {
            return Err(m("commands.error.loadedmultiverseworld.input", &[("world", first)]));
        }
        return Ok((w, 1));
    }
    match player_of(c) {
        Some(p) => worlds::find(p.get_world().get_name()).map(|w| (w, 0)).ok_or_else(|| m("commands.error.multiverseworld.issuer", &[])),
        None => match args.first() {
            Some(first) => Err(m("commands.error.multiverseworld.inputconsole", &[("world", first)])),
            None => Err(m("commands.error.multiverseworld.playersonly", &[])),
        },
    }
}

fn world_named(name: &str) -> Result<MvWorld, String> {
    worlds::find(name).ok_or_else(|| m("commands.error.multiverseworld.input", &[("world", name)]))
}

/// Lines shown a page at a time (players: 10 a page; the console: all), filtered by `--filter`.
fn paged(to: &CommandSender, header: &str, lines: Vec<String>, flags: &Flags) {
    let filter = flags.value("--filter").map(str::to_ascii_lowercase);
    let lines: Vec<String> = match &filter {
        Some(f) => lines.into_iter().filter(|l| horizon_kit::text::strip(l).to_ascii_lowercase().contains(f)).collect(),
        None => lines,
    };
    tell(to, header);
    if lines.is_empty() {
        say(to, "contentdisplay.nocontent", &[]);
        return;
    }
    let per = if matches!(to, CommandSender::Player(_)) { 10 } else { usize::MAX };
    let total = lines.len().div_ceil(per.min(lines.len()).max(1));
    let page: usize = flags.value("--page").and_then(|p| p.parse().ok()).unwrap_or(1);
    if page == 0 || page > total {
        say(to, "contentdisplay.invalidpage", &[("total", &total.to_string())]);
        return;
    }
    if total > 1 || filter.is_some() {
        let p = m("contentdisplay.page", &[("current", &page.to_string()), ("total", &total.to_string())]);
        let f = filter.as_ref().map_or_else(String::new, |f| m("contentdisplay.filter", &[("filter", f)]));
        tell(to, &format!("{p} {f}"));
    }
    for line in lines.iter().skip((page - 1).saturating_mul(per)).take(per) {
        tell(to, line);
    }
}

const PAGE_FLAGS: &[(&str, &str, bool)] = &[("--page", "-p", true), ("--filter", "-f", true)];

/// Players out of a world before it goes: to `dest` or the default world's spawn.
fn remove_players(world: &str, dest_input: Option<&str>) -> Result<(), String> {
    let dest = match dest_input {
        Some(d) => Some(dest::parse(d)?),
        None => None,
    };
    let fallback = worlds::find("world").and_then(|w| w.spawn()).ok_or_else(|| m("commands.error.removeplayers.nodefault", &[]))?;
    for p in horizon_bukkit::get_world(world).map(|w| w.get_players()).unwrap_or_default() {
        match &dest {
            Some(d) => {
                let _ = dest::teleport(&p, d, true);
            }
            None => {
                let _ = dest::teleport_to(&p, &fallback, true);
            }
        }
    }
    Ok(())
}

fn later(sender: &CommandSender) -> impl Fn(String) + Send + 'static {
    let s = sender.clone();
    move |text: String| tell(&s, &text)
}

// ---- Confirm queue ----

type Queued = Box<dyn FnOnce(&CommandSender) + Send>;
static QUEUE: Mutex<Option<HashMap<String, (Instant, u32, Queued)>>> = Mutex::new(None);

fn queue(c: &Ctx, prompt: String, run: impl FnOnce(&CommandSender) + Send + 'static) {
    let mode = crate::config::s("command.confirm-mode", "enable").to_ascii_lowercase();
    let is_player = matches!(c.sender, CommandSender::Player(_));
    if mode == "disable" || (mode == "player_only" && !is_player) || (mode == "console_only" && is_player) {
        run(c.sender);
        return;
    }
    let otp = if crate::config::b("command.use-confirm-otp", true) {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(7);
        1000 + nanos % 9000
    } else {
        0
    };
    let timeout = crate::config::i("command.confirm-timeout", 30).max(1);
    QUEUE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(c.sender.get_name().to_owned(), (Instant::now() + Duration::from_secs(timeout as u64), otp, Box::new(run)));
    tell(c.sender, &prompt);
    let command = if otp == 0 { "/mv confirm".to_owned() } else { format!("/mv confirm {otp}") };
    say(c.sender, "queuecommand.prompt", &[("command", &command), ("timeout", &timeout.to_string())]);
}

fn confirm(c: &Ctx) -> Res {
    let mut q = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    let map = q.get_or_insert_with(HashMap::new);
    let name = c.sender.get_name().to_owned();
    let Some((until, otp, _)) = map.get(&name) else { return Err(m("queuecommand.nocommandinqueue", &[])) };
    if Instant::now() > *until {
        map.remove(&name);
        return Err(m("queuecommand.expired", &[]));
    }
    if *otp != 0 && c.arg(0).and_then(|a| a.parse::<u32>().ok()) != Some(*otp) {
        return Err(m("queuecommand.invalidotp", &[("otp", c.arg(0).unwrap_or("0"))]));
    }
    let (_, _, run) = map.remove(&name).expect("queued");
    drop(q);
    run(c.sender);
    Ok(())
}

// ---- Root / help / version / reload ----

fn root(c: &Ctx) -> Res {
    say(c.sender, "root.title", &[("name", "Multiverse-Core"), ("version", env!("CARGO_PKG_VERSION"))]);
    say(c.sender, "root.help", &[]);
    Ok(())
}

fn help(c: &Ctx) -> Res {
    MV.help(c.sender, c.label);
    Ok(())
}

fn version(c: &Ctx) -> Res {
    say(c.sender, "version.mv", &[("version", env!("CARGO_PKG_VERSION"))]);
    say(c.sender, "version.authors", &[("authors", "dumptruckman, Rigby, fernferret, lithium3141, main--, benwoo1110, Zax71")]);
    say(c.sender, "version.secretcode", &[]);
    Ok(())
}

fn reload(c: &Ctx) -> Res {
    say(c.sender, "reload.reloading", &[]);
    crate::reload_all();
    say(c.sender, "reload.success", &[]);
    Ok(())
}

// ---- Create / import / load / unload / remove / delete ----

const CREATE_FLAGS: &[(&str, &str, bool)] = &[
    ("--seed", "-s", true),
    ("--generator", "-g", true),
    ("--generator-settings", "-gs", true),
    ("--world-type", "-t", true),
    ("--no-adjust-spawn", "-n", false),
    ("--no-structures", "-a", false),
    ("--biome", "-b", true),
    ("--properties", "-p", true),
    ("--generate-bonus-chest", "-c", false),
    ("--force-spawn-position", "", true),
];

fn environment(input: &str) -> Result<String, String> {
    match input.to_ascii_uppercase().as_str() {
        "NORMAL" | "OVERWORLD" | "WORLD" => Ok("NORMAL".into()),
        "NETHER" | "HELL" => Ok("NETHER".into()),
        "END" | "THE_END" | "STARWARS" => Ok("THE_END".into()),
        _ => Err(format!("&cInvalid environment '{input}'. Use NORMAL, NETHER or THE_END.")),
    }
}

/// Java's `String.hashCode` (a seed given as text).
fn java_hash(s: &str) -> i64 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as i32);
    }
    h as i64
}

fn seed_of(s: &str) -> i64 {
    s.parse().unwrap_or_else(|_| java_hash(s))
}

fn exists_check(name: &str, kind: &str) -> Res {
    if !valid_name(name) {
        return Err(m(&format!("{kind}.invalidworldname"), &[("world", name)]));
    }
    if let Some(w) = worlds::find(name) {
        return Err(m(&format!("{kind}.{}", if w.is_loaded() { "worldexistloaded" } else { "worldexistunloaded" }), &[("world", name)]));
    }
    Ok(())
}

fn create(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let name = pos[0].clone();
    let env = environment(pos.get(1).map(String::as_str).unwrap_or("NORMAL"))?;
    let flags = Flags::parse(&rest, CREATE_FLAGS)?;
    exists_check(&name, "createworld")?;
    if worlds::folder_looks_like_world(&worlds::folder_of(&name)) {
        return Err(m("createworld.worldexistfolder", &[("world", &name)]));
    }
    let seed = flags.value("--seed").map(seed_of);
    let generator = flags.value("--generator").unwrap_or("").to_owned();
    let kind = flags.value("--world-type").unwrap_or("NORMAL").to_ascii_uppercase();
    let structures = !flags.has("--no-structures");
    say(c.sender, "create.properties", &[("worldName", &name)]);
    say(c.sender, "create.properties.environment", &[("environment", &env)]);
    say(c.sender, "create.properties.seed", &[("seed", &seed.map_or("RANDOM".into(), |s| s.to_string()))]);
    say(c.sender, "create.properties.worldtype", &[("worldType", &kind)]);
    say(c.sender, "create.properties.adjustspawn", &[("adjustSpawn", &(!flags.has("--no-adjust-spawn")).to_string())]);
    if let Some(b) = flags.value("--biome") {
        say(c.sender, "create.properties.biome", &[("biome", b)]);
    }
    if !generator.is_empty() {
        say(c.sender, "create.properties.generator", &[("generator", &generator)]);
    }
    if let Some(gs) = flags.value("--generator-settings") {
        say(c.sender, "create.properties.generatorsettings", &[("generatorSettings", gs)]);
    }
    say(c.sender, "create.properties.structures", &[("structures", &structures.to_string())]);
    say(c.sender, "create.loading", &[]);

    let mut creator = horizon_bukkit::worlds::WorldCreator::new(&name);
    creator.environment(&env).world_type(&kind).generate_structures(structures).bonus_chest(flags.has("--generate-bonus-chest"));
    if let Some(s) = seed {
        creator.seed(s);
    }
    if let Some(gs) = flags.value("--generator-settings") {
        creator.generator_settings(gs);
    }
    if !generator.is_empty() {
        let g = horizon_bukkit::worlds::get_generator_for_name(&generator).ok_or_else(|| m("worldcreator.invalidchunkgenerator", &[("generator", &generator), ("error", "No such generator.")]))?;
        creator.generator(g);
    }
    let world = creator.create_world().ok_or_else(|| m("worldcreator.bukkitcreationfailed", &[("world", &name), ("error", "the server refused")]))?;
    let mut data = worlds::default_data(&env, Some(world.get_seed()), &generator);
    let mut w = MvWorld { key: name.clone(), name: name.clone(), data: std::mem::take(&mut data) };
    w.set("adjust-spawn", (!flags.has("--no-adjust-spawn")).into());
    if let Some(b) = flags.value("--biome") {
        w.set("biome", b.into());
    }
    if let Some(gs) = flags.value("--generator-settings") {
        w.set("read-only.generator-settings", gs.into());
    }
    if let Some(props) = flags.value("--properties") {
        for pair in props.split(',') {
            if let Some((k, v)) = pair.split_once('=') {
                set_property(&mut w, "set", k.trim(), Some(v.trim()))?;
            }
        }
    }
    if let Some(pos) = flags.value("--force-spawn-position")
        && let Some(at) = parse_xyz(&world, pos)
    {
        w.set_spawn(&at);
    }
    worlds::apply(&w, &world);
    worlds::add(w);
    say(c.sender, "create.success", &[("world", &name)]);
    Ok(())
}

/// `x,y,z[:pitch:yaw]` in a world.
fn parse_xyz(world: &horizon_bukkit::World, s: &str) -> Option<Location> {
    let mut parts = s.split(':');
    let xyz: Vec<f64> = parts.next()?.split(',').map(|v| v.trim().parse().ok()).collect::<Option<_>>()?;
    if xyz.len() != 3 {
        return None;
    }
    let pitch = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let yaw = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    Some(Location::new(world.clone(), xyz[0], xyz[1], xyz[2], yaw, pitch))
}

const IMPORT_FLAGS: &[(&str, &str, bool)] = &[("--generator", "-g", true), ("--generator-settings", "-gs", true), ("--no-adjust-spawn", "-n", false), ("--biome", "-b", true), ("--skip-folder-check", "-f", false)];

fn import(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let name = pos[0].clone();
    let env = environment(pos.get(1).map(String::as_str).unwrap_or("NORMAL"))?;
    let flags = Flags::parse(&rest, IMPORT_FLAGS)?;
    exists_check(&name, "importworld")?;
    let loaded_already = horizon_bukkit::get_world(&name);
    if loaded_already.is_none() && !flags.has("--skip-folder-check") && !worlds::folder_looks_like_world(&worlds::folder_of(&name)) {
        return Err(m("importworld.worldfolderinvalid", &[("world", &name)]));
    }
    if let Some(w) = &loaded_already
        && w.get_environment() != env
    {
        return Err(m("importworld.bukkitenvironmentmismatch", &[("world", &name), ("bukkitEnvironment", w.get_environment()), ("mvEnvironment", &env)]));
    }
    say(c.sender, "import.importing", &[("world", &name)]);
    let generator = flags.value("--generator").unwrap_or("").to_owned();
    let mut w = MvWorld { key: name.clone(), name: name.clone(), data: worlds::default_data(&env, None, &generator) };
    w.set("adjust-spawn", (!flags.has("--no-adjust-spawn")).into());
    if let Some(gs) = flags.value("--generator-settings") {
        w.set("read-only.generator-settings", gs.into());
    }
    if let Some(b) = flags.value("--biome") {
        w.set("biome", b.into());
    }
    let world = worlds::load_in_game(&w)?;
    w.set("read-only.seed", world.get_seed().into());
    worlds::add(w);
    say(c.sender, "import.success", &[("world", &name)]);
    Ok(())
}

fn load(c: &Ctx) -> Res {
    let name = &c.args[0];
    let Some(w) = worlds::find(name) else {
        if worlds::folder_looks_like_world(&worlds::folder_of(name)) {
            return Err(m("loadworld.worldexistfolder", &[("world", name)]));
        }
        return Err(m("loadworld.worldnonexistent", &[("world", name)]));
    };
    if w.is_loaded() {
        return Err(m("loadworld.worldexistloaded", &[("world", &w.name)]));
    }
    say(c.sender, "load.loading", &[("world", &w.name)]);
    worlds::load_in_game(&w)?;
    say(c.sender, "load.success", &[("world", &w.name)]);
    Ok(())
}

const REMOVE_FLAGS: &[(&str, &str, bool)] = &[("--remove-players", "-r", true), ("--no-unload-bukkit-world", "-b", false), ("--no-save", "-n", false)];

fn unload_world(w: &MvWorld, flags: &Flags, save: bool) -> Res {
    if !w.is_loaded() {
        return Ok(());
    }
    if w.name == "world" {
        return Err(m("exception.multiverseworld.unloaddefaultworld", &[("world", &w.name)]));
    }
    if flags.has("--remove-players") {
        remove_players(&w.name, flags.value("--remove-players"))?;
    }
    let count = w.world().map_or(0, |x| x.get_players().len());
    if count > 0 {
        return Err(m("exception.multiverseworld.unloadplayersinworld", &[("count", &count.to_string())]));
    }
    if !horizon_bukkit::worlds::unload_world(&w.name, save) {
        return Err(m("exception.multiverseworld.unloaderror", &[("world", &w.name)]));
    }
    Ok(())
}

fn unload(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let flags = Flags::parse(&rest, REMOVE_FLAGS)?;
    let w = world_named(&pos[0])?;
    if !w.is_loaded() {
        return Err(m("commands.error.loadedmultiverseworld.input", &[("world", &w.name)]));
    }
    say(c.sender, "unload.unloading", &[("world", &w.name)]);
    unload_world(&w, &flags, !flags.has("--no-save"))?;
    say(c.sender, "unload.success", &[("world", &w.name)]);
    Ok(())
}

fn remove(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let flags = Flags::parse(&rest, REMOVE_FLAGS)?;
    let w = world_named(&pos[0])?;
    if !flags.has("--no-unload-bukkit-world") {
        unload_world(&w, &flags, !flags.has("--no-save"))?;
    }
    worlds::remove(&w.name);
    say(c.sender, "remove.success", &[("world", &w.name)]);
    Ok(())
}

fn delete(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let flags = Flags::parse(&rest, &[("--remove-players", "-r", true)])?;
    let w = worlds::find(&pos[0]).ok_or_else(|| m("deleteworld.worldnonexistent", &[("world", &pos[0])]))?;
    let prompt = m("delete.prompt", &[("world", &w.name)]);
    let remove_to = flags.value("--remove-players").map(str::to_owned);
    let removing = flags.has("--remove-players");
    queue(c, prompt, move |sender| {
        say(sender, "delete.deleting", &[("world", &w.name)]);
        let folder = worlds::folder_of(&w.name);
        if !folder.exists() {
            return say(sender, "deleteworld.worldfoldernotfound", &[("world", &w.name)]);
        }
        let flags = Flags { set: if removing { HashMap::from([("--remove-players".to_owned(), remove_to)]) } else { HashMap::new() } };
        if let Err(e) = unload_world(&w, &flags, false) {
            return tell(sender, &e);
        }
        worlds::remove(&w.name);
        let reply = later(sender);
        let name = w.name.clone();
        std::thread::spawn(move || {
            let result = std::fs::remove_dir_all(&folder);
            on_main(move || match result {
                Ok(()) => reply(m("delete.success", &[("world", &name)])),
                Err(e) => reply(m("deleteworld.failedtodeletefolder", &[("world", &name), ("error", &e.to_string())])),
            });
        });
    });
    Ok(())
}

// ---- Regen / clone ----

fn regen(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let flags = Flags::parse(&rest, &[("--seed", "-s", true), ("--reset-world-config", "-wc", false), ("--reset-gamerules", "-gm", false), ("--reset-world-border", "-wb", false), ("--keep-files", "", true), ("--remove-players", "-r", true)])?;
    let w = world_named(&pos[0])?;
    if !w.is_loaded() {
        return Err(m("commands.error.loadedmultiverseworld.input", &[("world", &w.name)]));
    }
    let seed: Option<i64> = if flags.has("--seed") {
        Some(flags.value("--seed").map(seed_of).unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0)))
    } else {
        w.seed()
    };
    let reset_config = flags.has("--reset-world-config");
    let reset_rules = flags.has("--reset-gamerules");
    let remove_to = flags.value("--remove-players").map(str::to_owned);
    let prompt = m("regen.prompt", &[("world", &w.name)]);
    queue(c, prompt, move |sender| {
        say(sender, "regen.regenerating", &[("world", &w.name)]);
        let rules = if reset_rules { Vec::new() } else { gamerules_of(&w.name) };
        let flags = Flags { set: HashMap::from([("--remove-players".to_owned(), remove_to)]) };
        if let Err(e) = unload_world(&w, &flags, false) {
            return tell(sender, &e);
        }
        let folder = worlds::folder_of(&w.name);
        let reply = later(sender);
        let mut neww = w.clone();
        if reset_config {
            neww.data = worlds::default_data(&w.environment(), seed, &w.generator());
        }
        neww.set("read-only.seed", seed.unwrap_or(i64::MIN).into());
        std::thread::spawn(move || {
            let _ = std::fs::remove_dir_all(&folder);
            on_main(move || {
                match worlds::load_in_game(&neww) {
                    Ok(world) => {
                        for (k, v) in rules {
                            world.set_game_rule(&k, &v);
                        }
                        let name = neww.name.clone();
                        worlds::add(neww);
                        reply(m("regen.success", &[("world", &name)]));
                    }
                    Err(e) => reply(e),
                }
            });
        });
    });
    Ok(())
}

fn gamerules_of(world: &str) -> Vec<(String, String)> {
    let Some(w) = horizon_bukkit::get_world(world) else { return Vec::new() };
    w.get_game_rules().into_iter().filter_map(|r| w.get_game_rule_value(&r).map(|v| (r, v))).collect()
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "uid.dat" || name == "session.lock" {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn clone(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let flags = Flags::parse(&rest, &[("--reset-world-config", "-wc", false), ("--reset-gamerules", "-gm", false), ("--reset-world-border", "-wb", false), ("--no-save", "-s", false)])?;
    let w = world_named(&pos[0])?;
    let new_name = pos[1].clone();
    if !valid_name(&new_name) {
        return Err(m("cloneworld.invalidworldname", &[("world", &new_name)]));
    }
    if let Some(o) = worlds::find(&new_name) {
        return Err(m(if o.is_loaded() { "cloneworld.worldexistloaded" } else { "cloneworld.worldexistunloaded" }, &[("world", &new_name)]));
    }
    let to = worlds::folder_of(&new_name);
    if to.exists() {
        return Err(m("cloneworld.worldexistfolder", &[("world", &new_name)]));
    }
    let from = worlds::folder_of(&w.name);
    if !worlds::folder_looks_like_world(&from) {
        return Err(m("cloneworld.fromworldfolderinvalid", &[("world", &w.name)]));
    }
    say(c.sender, "clone.cloning", &[("world", &w.name), ("newworld", &new_name)]);
    if !flags.has("--no-save")
        && let Some(world) = w.world()
    {
        world.save();
    }
    let rules = if flags.has("--reset-gamerules") { Vec::new() } else { gamerules_of(&w.name) };
    let mut neww = MvWorld { key: new_name.clone(), name: new_name.clone(), data: if flags.has("--reset-world-config") { worlds::default_data(&w.environment(), w.seed(), &w.generator()) } else { w.data.clone() } };
    neww.set("alias", "".into());
    neww.set("read-only.legacy-world-name", new_name.clone().into());
    let reply = later(c.sender);
    std::thread::spawn(move || {
        let copied = copy_dir(&from, &to);
        on_main(move || {
            if let Err(e) = copied {
                return reply(m("cloneworld.copyfailed", &[("world", &new_name), ("error", &e.to_string())]));
            }
            match worlds::load_in_game(&neww) {
                Ok(world) => {
                    for (k, v) in rules {
                        world.set_game_rule(&k, &v);
                    }
                    worlds::add(neww);
                    reply(m("clone.success", &[("world", &new_name)]));
                }
                Err(e) => reply(e),
            }
        });
    });
    Ok(())
}

// ---- List / info / who ----

fn env_coloured(env: &str) -> String {
    let c = match env {
        "NETHER" => "§c",
        "NORMAL" => "§a",
        "THE_END" => "§b",
        _ => "§6",
    };
    format!("{c}{env}")
}

fn can_access(sender: &CommandSender, w: &MvWorld) -> bool {
    !crate::config::b("world.enforce-access", false) || sender.has_permission(&format!("multiverse.access.{}", w.name))
}

fn list(c: &Ctx) -> Res {
    let flags = Flags::parse(&c.args, &[("--page", "-p", true), ("--filter", "-f", true), ("--raw", "-r", false)])?;
    let raw = flags.has("--raw");
    let all = worlds::all();
    let name = |w: &MvWorld| if raw { w.name.clone() } else { w.alias_or_name() };
    let mut loaded: Vec<String> = all.iter().filter(|w| w.is_loaded() && can_access(c.sender, w) && (!w.hidden() || c.sender.has_permission("multiverse.core.modify"))).map(|w| format!("{}{}§f - {}", if w.hidden() { "§7[H] " } else { "" }, name(w), env_coloured(&w.environment()))).collect();
    loaded.sort();
    let mut unloaded: Vec<String> = all.iter().filter(|w| !w.is_loaded() && can_access(c.sender, w)).map(|w| format!("§7{} - UNLOADED", name(w))).collect();
    unloaded.sort();
    loaded.extend(unloaded);
    if loaded.is_empty() {
        tell(c.sender, &m("list.header", &[]));
        return Err(m("list.nocontent", &[]));
    }
    paged(c.sender, &m("list.header", &[]), loaded, &flags);
    Ok(())
}

fn info(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let flags = Flags::parse(&rest, PAGE_FLAGS)?;
    let (w, _) = world_arg(c, &pos, true)?;
    let world = w.world().ok_or_else(|| m("commands.error.loadedmultiverseworld.input", &[("world", &w.name)]))?;
    let spawn = w.spawn().map(|l| dest::str_coords(&l)).unwrap_or_default();
    let fee = match w.entry_fee() {
        None => ("Entry Fee", "FREE!".to_owned()),
        Some((a, _)) if a > 0.0 => ("Entry Fee", horizon_kit::hooks::economy::format(a)),
        Some((a, _)) => ("Entry Reward", horizon_kit::hooks::economy::format(-a)),
    };
    let rows: Vec<(&str, String)> = vec![
        ("World Key", w.key.clone()),
        ("World Name", w.name.clone()),
        ("World Alias", w.alias()),
        ("World UID", world.get_uid()),
        ("Game Mode", w.gamemode()),
        ("Difficulty", w.s("difficulty", "NORMAL").to_ascii_uppercase()),
        ("Spawn Location", spawn),
        ("Respawn World", w.s("respawn-world", "")),
        ("Bed Respawn", w.b("bed-respawn", true).to_string()),
        ("Anchor Respawn", w.b("anchor-respawn", true).to_string()),
        ("Seed", world.get_seed().to_string()),
        ("Environment", w.environment()),
        ("World Type", world.get_world_type().to_owned()),
        ("Biome", w.s("biome", "")),
        ("Generator", w.generator()),
        ("Generator Settings", w.s("read-only.generator-settings", "")),
        ("Auto Load", w.b("auto-load", true).to_string()),
        ("Keep Spawn In Memory", w.b("keep-spawn-in-memory", true).to_string()),
        ("World Scale", w.f("scale", 1.0).to_string()),
        ("Weather Enabled", w.b("allow-weather", true).to_string()),
        ("Allow Flight", w.b("allow-flight", false).to_string()),
        ("Grant Advancements", w.b("allow-advancement-grant", true).to_string()),
        ("Auto Heal", w.b("auto-heal", true).to_string()),
        ("Hunger Depletes", w.b("hunger", true).to_string()),
        ("PVP Enabled", w.b("pvp", true).to_string()),
        ("Portal Form", w.s("portal-form", "ALL")),
        ("Player Limit", w.player_limit().to_string()),
        ("World Blacklist", w.list("world-blacklist").join(", ")),
        (fee.0, fee.1),
    ];
    let lines = rows.into_iter().map(|(k, v)| format!("§b{k}§f: {}", if v.is_empty() { m("contentdisplay.empty", &[]) } else { v })).collect();
    paged(c.sender, &m("info.header", &[("world", &w.name)]), lines, &flags);
    Ok(())
}

fn who_line(w: &MvWorld) -> Option<String> {
    let players: Vec<String> = w.world()?.get_players().iter().map(|p| p.get_display_name().to_owned()).collect();
    Some(format!("{}§f - {}", w.alias_or_name(), if players.is_empty() { m("who.empty", &[]) } else { players.join("§f, ") }))
}

fn who(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let flags = Flags::parse(&rest, PAGE_FLAGS)?;
    let (w, _) = world_arg(c, &pos, true)?;
    let players: Vec<String> = w.world().map(|x| x.get_players().iter().map(|p| p.get_display_name().to_owned()).collect()).unwrap_or_default();
    let lines = if players.is_empty() { vec![m("who.empty", &[])] } else { players };
    paged(c.sender, &format!("{}\n§a{}§f:", m("who.header", &[]), w.alias_or_name()), lines, &flags);
    Ok(())
}

fn whoall(c: &Ctx) -> Res {
    let flags = Flags::parse(&c.args, PAGE_FLAGS)?;
    let lines = worlds::all().iter().filter(|w| !w.hidden() || c.sender.has_permission("multiverse.core.modify")).filter_map(who_line).collect();
    paged(c.sender, &m("who.header", &[]), lines, &flags);
    Ok(())
}

// ---- Teleports ----

fn tp(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let flags = Flags::parse(&rest, &[("--unsafe", "-u", false), ("--silent", "-s", false)])?;
    let (targets, dest_input): (Vec<Player>, &str) = match pos.as_slice() {
        [d] => (vec![player_of(c).ok_or_else(|| m("commands.error.player.issuerinput", &[("player", d)]))?], d),
        [who, d, ..] => {
            let list: Vec<Player> = if who == "@a" { horizon_bukkit::get_online_players() } else { who.split(',').map(|n| online(n).ok_or_else(|| m("commands.error.players.input", &[("player", n)]))).collect::<Result<_, _>>()? };
            (list, d)
        }
        [] => return Err("&cUsage: &b/mv tp [player] <destination> [--unsafe --silent]".into()),
    };
    let limit = crate::config::i("teleport.concurrent-teleport-limit", 50).max(1) as usize;
    if targets.len() > limit {
        return Err(m("teleport.toomanyplayers", &[("count", &limit.to_string())]));
    }
    let d = dest::parse(dest_input)?;
    let me = player_of(c);
    for p in &targets {
        let own = me.as_ref().is_some_and(|m| m.get_unique_id() == p.get_unique_id());
        let scope = if own { "self" } else { "other" };
        let base = format!("multiverse.teleport.{scope}.{}", d.id());
        if !c.sender.has_permission(&base) && !c.sender.has_permission(&format!("{base}.{}", d.target())) {
            return Err(m(if own { "teleport.nopermission.self" } else { "teleport.nopermission.other" }, &[]));
        }
    }
    let display = d.display();
    let safe = !flags.has("--unsafe");
    for p in targets {
        match dest::teleport(&p, &d, safe) {
            Ok(()) => {
                if !flags.has("--silent") {
                    say(c.sender, "teleport.success", &[("player", p.get_name()), ("destination", &display)]);
                }
            }
            Err(reason) => say(c.sender, "teleport.failed", &[("player", p.get_name()), ("destination", &display), ("reason", &reason)]),
        }
    }
    Ok(())
}

fn spawn(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let flags = Flags::parse(&rest, &[("--unsafe", "-u", false)])?;
    let targets: Vec<Player> = match pos.first() {
        Some(n) if n == "@a" => horizon_bukkit::get_online_players(),
        Some(n) => vec![online(n).ok_or_else(|| m("commands.error.player.input", &[("player", n)]))?],
        None => vec![player_of(c).ok_or_else(|| m("commands.error.playersonly", &[]))?],
    };
    let me = player_of(c);
    for p in targets {
        let own = me.as_ref().is_some_and(|m| m.get_unique_id() == p.get_unique_id());
        if !c.sender.has_permission(if own { "multiverse.core.spawn.self" } else { "multiverse.core.spawn.other" }) && !c.sender.has_permission("multiverse.core.spawn") {
            return Err(m("spawn.nopermission", &[]));
        }
        let wname = p.get_world().get_name().to_owned();
        let Some(w) = worlds::find(&wname) else {
            say(c.sender, "spawn.notmvworld", &[("world", &wname)]);
            continue;
        };
        let at = w.spawn().ok_or_else(|| m("teleportfailurereason.null.location", &[]))?;
        match dest::teleport_to(&p, &at, !flags.has("--unsafe")) {
            Ok(()) => say(c.sender, "spawn.success", &[("player", if own { "you" } else { p.get_name() }), ("world", &w.alias_or_name())]),
            Err(reason) => say(c.sender, "spawn.failed", &[("player", p.get_name()), ("world", &w.alias_or_name()), ("reason", &reason)]),
        }
    }
    Ok(())
}

fn setspawn(c: &Ctx) -> Res {
    let (pos, rest) = split_flags(&c.args);
    let flags = Flags::parse(&rest, &[("--unsafe", "-u", false)])?;
    let at: Location = match pos.first() {
        Some(s) => {
            let (wname, xyz) = s.split_once(':').ok_or_else(|| m("exception.positionparse.invalidcoordinates", &[("format", s)]))?;
            let world = horizon_bukkit::get_world(&worlds::find(wname).map_or_else(|| wname.to_owned(), |w| w.name)).ok_or_else(|| m("destination.shared.failurereason.worldnotfound", &[("world", wname)]))?;
            let n: Vec<f64> = xyz.split(',').map(|v| v.trim().parse().ok()).collect::<Option<_>>().ok_or_else(|| m("exception.positionparse.invalidcoordinates", &[("format", xyz)]))?;
            if n.len() < 3 {
                return Err(m("exception.positionparse.invalidcoordinates", &[("format", xyz)]));
            }
            Location::new(world, n[0], n[1], n[2], n.get(3).copied().unwrap_or(0.0) as f32, n.get(4).copied().unwrap_or(0.0) as f32)
        }
        None => player_of(c).ok_or_else(|| m("commands.error.playersonly", &[]))?.get_location(),
    };
    let wname = at.world.get_name().to_owned();
    if worlds::find(&wname).is_none() {
        return Err(m("setspawn.notmvworld", &[("world", &wname)]));
    }
    if !flags.has("--unsafe") && dest::safe_spot(&at).is_none_or(|s| s != at) {
        return Err(m("setspawn.unsafe", &[]));
    }
    worlds::update(&wname, |w| w.set_spawn(&at));
    at.world.set_spawn_location(&at);
    say(c.sender, "setspawn.success", &[("world", &wname), ("location", &dest::str_coords(&at))]);
    Ok(())
}

fn check(c: &Ctx) -> Res {
    let p = online(&c.args[0]).ok_or_else(|| m("commands.error.player.input", &[("player", &c.args[0])]))?;
    let d = dest::parse(&c.args[1])?;
    let base = format!("multiverse.teleport.self.{}", d.id());
    let display = d.display();
    if p.has_permission(&base) || p.has_permission(&format!("{base}.{}", d.target())) {
        say(c.sender, "check.haspermission", &[("player", p.get_name()), ("destination", &display)]);
    } else {
        say(c.sender, "check.nopermission", &[("player", p.get_name()), ("destination", &display)]);
    }
    if let Ok(at) = d.location(&p) {
        say(c.sender, "check.location", &[("location", &dest::str_coords(&at))]);
    }
    Ok(())
}

fn coords(c: &Ctx) -> Res {
    let p = c.player()?;
    let l = p.get_location();
    let w = worlds::find(p.get_world().get_name()).ok_or_else(|| m("commands.error.multiverseworldonly", &[]))?;
    say(c.sender, "coordinates.info.title", &[]);
    say(c.sender, "coordinates.info.world", &[("world", &w.name)]);
    say(c.sender, "coordinates.info.alias", &[("alias", &w.alias_or_name())]);
    say(c.sender, "coordinates.info.worldscale", &[("scale", &w.f("scale", 1.0).to_string())]);
    say(c.sender, "coordinates.info.coordinates", &[("coordinates", &dest::str_coords(&l))]);
    let dirs = ["south", "southwest", "west", "northwest", "north", "northeast", "east", "southeast"];
    let idx = (((l.yaw as f64).rem_euclid(360.0) + 22.5) / 45.0) as usize % 8;
    say(c.sender, "coordinates.info.direction", &[("direction", dirs[idx])]);
    Ok(())
}

// ---- Modify ----

/// Applies `action` to a property: Err = why not.
pub fn set_property(w: &mut MvWorld, action: &str, name: &str, value: Option<&str>) -> Result<String, String> {
    let (_, path, kind) = *worlds::property(name).ok_or_else(|| m("config.node.notfound", &[("node", name)]))?;
    let action = action.to_ascii_lowercase();
    let parsed: Y = match action.as_str() {
        "reset" => worlds::default_data(&w.environment(), w.seed(), "").get(path.split('.').next().unwrap_or(path)).and_then(|v| if path.contains('.') { v.get(path.split_once('.').map_or("", |p| p.1)).cloned() } else { Some(v.clone()) }).unwrap_or(Y::Null),
        "add" | "remove" if kind == Kind::List => {
            let v = value.unwrap_or_default().to_owned();
            let mut list = w.list(path);
            if action == "add" {
                if !list.iter().any(|x| x.eq_ignore_ascii_case(&v)) {
                    list.push(v);
                }
            } else {
                list.retain(|x| !x.eq_ignore_ascii_case(&v));
            }
            Y::Sequence(list.into_iter().map(Y::String).collect())
        }
        "add" | "remove" => return Err(format!("Property '{name}' is not a list.")),
        "set" => {
            let v = value.unwrap_or_default();
            match kind {
                Kind::Bool => Y::Bool(v.parse().map_err(|_| m("config.string.parser.invalidboolean", &[("input", v)]))?),
                Kind::Num => Y::from(v.parse::<f64>().map_err(|_| m("config.string.parser.invaliddouble", &[("input", v)]))?),
                Kind::Int => Y::from(v.parse::<i64>().map_err(|_| m("config.string.parser.invalidinteger", &[("input", v)]))?),
                Kind::Str => Y::String(v.to_owned()),
                Kind::List => Y::Sequence(v.split(',').map(|s| Y::String(s.trim().to_owned())).filter(|s| s.as_str() != Some("")).collect()),
                Kind::Enum(options) => {
                    let up = v.to_ascii_uppercase();
                    let up = match (path, up.as_str()) {
                        ("gamemode", "0") => "SURVIVAL".into(),
                        ("gamemode", "1") => "CREATIVE".into(),
                        ("gamemode", "2") => "ADVENTURE".into(),
                        ("gamemode", "3") => "SPECTATOR".into(),
                        _ => up,
                    };
                    if !options.contains(&up.as_str()) {
                        return Err(format!("'{v}' is not one of {}", options.join(", ")));
                    }
                    Y::String(up)
                }
            }
        }
        other => return Err(format!("Invalid action '{other}'.")),
    };
    w.set(path, parsed.clone());
    Ok(match parsed {
        Y::Sequence(s) => s.iter().filter_map(worlds::yaml_str).collect::<Vec<_>>().join(", "),
        other => worlds::yaml_str(&other).unwrap_or_default(),
    })
}

/// A property changed: what the game keeps, set again (and players in the world updated).
fn after_change(w: &MvWorld, path: &str) {
    let Some(world) = w.world() else { return };
    match path {
        "pvp" | "allow-weather" | "spawn-location" => worlds::apply(w, &world),
        "difficulty" => world.set_difficulty(&w.s("difficulty", "NORMAL")),
        "gamemode" | "allow-flight" => {
            for p in world.get_players() {
                crate::apply_player(&p, w);
            }
        }
        _ => {}
    }
}

fn modify(c: &Ctx) -> Res {
    let actions = ["set", "add", "remove", "reset"];
    // [world] <action> <property> [value]
    let (w, skip) = if c.args.first().is_some_and(|a| actions.contains(&a.to_ascii_lowercase().as_str())) && (c.args.len() < 2 || worlds::find(&c.args[0]).is_none()) {
        world_arg(c, &[], false)?
    } else {
        let w = world_named(&c.args[0])?;
        (w, 1)
    };
    let args = &c.args[skip..];
    let (action, name, value) = match args {
        [a, p] => (a.to_ascii_lowercase(), p.as_str(), None),
        [a, p, rest @ ..] => (a.to_ascii_lowercase(), p.as_str(), Some(rest.join(" "))),
        _ => return Err("&cUsage: &b/mv modify [world] <set|add|remove|reset> <property> [value]".into()),
    };
    if !actions.contains(&action.as_str()) {
        return Err(format!("&cInvalid action '{action}'! Valid actions are set, add, remove and reset."));
    }
    if matches!(name, "animals" | "monsters") {
        let replacement = format!("/mv entity-spawn-config modify [world] {} set spawn <true|false>", if name == "animals" { "animal" } else { "monster" });
        return Err(m("modify.propertyremoved", &[("property", name), ("replacement", &replacement)]));
    }
    let needs = action != "reset";
    if needs && value.is_none() {
        return Err(m("modify.specifyvalue", &[("action", &action), ("property", name)]));
    }
    if !needs && value.is_some() {
        return Err(m("modify.cannothavevalue", &[("action", &action), ("property", name)]));
    }
    let mut copy = w.clone();
    match set_property(&mut copy, &action, name, value.as_deref()) {
        Ok(v) => {
            let path = worlds::property(name).map_or(name, |p| p.1);
            worlds::update(&w.name, |x| x.data = copy.data.clone());
            after_change(&copy, path);
            say(c.sender, "modify.success", &[("action", &action), ("property", name), ("value", &v), ("world", &w.name)]);
            Ok(())
        }
        Err(e) => match value {
            Some(v) => Err(m("modify.failure", &[("action", &action), ("property", name), ("value", &v), ("world", &w.name), ("error", &e)])),
            None => Err(m("modify.failure.novalue", &[("action", &action), ("property", name), ("world", &w.name), ("error", &e)])),
        },
    }
}

// ---- Gamerules ----

fn target_worlds(c: &Ctx, arg: Option<&String>) -> Result<Vec<MvWorld>, String> {
    match arg.map(String::as_str) {
        Some("*") => Ok(worlds::all().into_iter().filter(MvWorld::is_loaded).collect()),
        Some(list) => list.split(',').map(|n| worlds::find(n).filter(MvWorld::is_loaded).ok_or_else(|| m("commands.error.loadedmultiverseworld.input", &[("world", n)]))).collect(),
        None => Ok(vec![world_arg(c, &[], true)?.0]),
    }
}

fn gamerule(c: &Ctx) -> Res {
    let sub = c.args[0].to_ascii_lowercase();
    let rest = &c.args[1..];
    match sub.as_str() {
        "set" => {
            if !c.sender.has_permission("multiverse.core.gamerule.set") {
                return Err("&cI'm sorry, but you do not have permission to perform this command.".into());
            }
            let [rule, value, more @ ..] = rest else { return Err("&cUsage: &b/mv gamerule set <gamerule> <value> [worlds|*]".into()) };
            let targets = target_worlds(c, more.first())?;
            let mut done = 0;
            for w in &targets {
                let ok = w.world().is_some_and(|x| x.set_game_rule(rule, value));
                if ok {
                    done += 1;
                } else {
                    say(c.sender, "gamerule.set.failed", &[("gamerule", rule), ("value", value), ("world", &w.name), ("type", "valid value")]);
                }
            }
            if done == 1 && targets.len() == 1 {
                say(c.sender, "gamerule.set.success.single", &[("gamerule", rule), ("value", value), ("world", &targets[0].name)]);
            } else if done > 0 {
                say(c.sender, "gamerule.set.success.multiple", &[("gamerule", rule), ("value", value), ("count", &done.to_string())]);
            }
            Ok(())
        }
        "reset" => {
            if !c.sender.has_permission("multiverse.core.gamerule.set") {
                return Err("&cI'm sorry, but you do not have permission to perform this command.".into());
            }
            let [rule, more @ ..] = rest else { return Err("&cUsage: &b/mv gamerule reset <gamerule> [worlds|*]".into()) };
            let targets = target_worlds(c, more.first())?;
            let mut done = 0;
            for w in &targets {
                let ok = w.world().is_some_and(|x| match x.get_game_rule_default(rule) {
                    Some(d) => x.set_game_rule(rule, &d.to_string()),
                    None => false,
                });
                if ok {
                    done += 1;
                } else {
                    say(c.sender, "gamerule.reset.failed", &[("gamerule", rule), ("world", &w.name)]);
                }
            }
            if done == 1 && targets.len() == 1 {
                say(c.sender, "gamerule.reset.success.single", &[("gamerule", rule), ("world", &targets[0].name)]);
            } else if done > 0 {
                say(c.sender, "gamerule.reset.success.multiple", &[("gamerule", rule), ("count", &done.to_string())]);
            }
            Ok(())
        }
        "list" => {
            if !c.sender.has_permission("multiverse.core.gamerule.list") {
                return Err("&cI'm sorry, but you do not have permission to perform this command.".into());
            }
            let (pos, flags) = split_flags(rest);
            let flags = Flags::parse(&flags, PAGE_FLAGS)?;
            let (w, _) = world_arg(c, &pos, true)?;
            let world = w.world().ok_or_else(|| m("commands.error.loadedmultiverseworld.input", &[("world", &w.name)]))?;
            let mut rules = world.get_game_rules();
            rules.sort();
            let lines = rules.into_iter().map(|r| {
                let v = world.get_game_rule_value(&r).unwrap_or_default();
                format!("§b{r}§f: {v}")
            }).collect();
            paged(c.sender, &m("gamerule.list.title", &[("world", &w.alias_or_name())]), lines, &flags);
            Ok(())
        }
        _ => Err("&cUsage: &b/mv gamerule <set|reset|list> ...".into()),
    }
}

// ---- Config ----

fn config_cmd(c: &Ctx) -> Res {
    let name = &c.args[0];
    let path = crate::config::find_path(name).ok_or_else(|| m("config.node.notfound", &[("node", name)]))?;
    match c.args.get(1) {
        None => {
            let v = crate::config::s(&path, "");
            say(c.sender, "config.show.success", &[("name", name), ("value", &v)]);
        }
        Some(v) => {
            crate::config::set(&path, v);
            say(c.sender, "config.set.success", &[("name", name), ("value", v)]);
        }
    }
    Ok(())
}

// ---- Anchors ----

fn anchor(c: &Ctx) -> Res {
    let sub = c.args[0].to_ascii_lowercase();
    match sub.as_str() {
        "set" | "create" => {
            if !c.sender.has_permission("multiverse.core.anchor.create") {
                return Err("&cI'm sorry, but you do not have permission to perform this command.".into());
            }
            let name = c.args.get(1).ok_or("&cUsage: &b/mv anchor set <name> [location]")?;
            let at = match c.args.get(2) {
                Some(s) => dest::string_to_location(s).ok_or_else(|| m("anchor.set.failure", &[("anchor", name)]))?,
                None => c.player()?.get_location(),
            };
            dest::set_anchor(name, &at);
            say(c.sender, "anchor.set.success", &[("anchor", name), ("location", &dest::location_to_string(&at))]);
        }
        "delete" | "remove" => {
            if !c.sender.has_permission("multiverse.core.anchor.delete") {
                return Err("&cI'm sorry, but you do not have permission to perform this command.".into());
            }
            let name = c.args.get(1).ok_or("&cUsage: &b/mv anchor delete <name>")?;
            if dest::delete_anchor(name) {
                say(c.sender, "anchor.delete.success", &[("anchor", name)]);
            } else {
                say(c.sender, "anchor.delete.failure", &[]);
            }
        }
        "list" => {
            if !c.sender.has_permission("multiverse.core.anchor.list") {
                return Err("&cI'm sorry, but you do not have permission to perform this command.".into());
            }
            let flags = Flags::parse(&c.args[1..], PAGE_FLAGS)?;
            let lines = dest::anchors().into_iter().map(|(n, l)| m("anchor.list.entry", &[("anchor", &n), ("location", &l)])).collect();
            paged(c.sender, &m("anchor.list.header", &[]), lines, &flags);
        }
        _ => return Err("&cUsage: &b/mv anchor <set|delete|list>".into()),
    }
    Ok(())
}

// ---- Tab completion ----

fn tab_worlds(c: &Ctx) -> Vec<String> {
    if c.args.len() <= 1 { worlds::names() } else { vec!["--remove-players".into(), "--no-save".into()] }
}
fn tab_loaded(c: &Ctx) -> Vec<String> {
    if c.args.len() <= 1 { worlds::all().into_iter().filter(MvWorld::is_loaded).map(|w| w.name).collect() } else { Vec::new() }
}
fn tab_unloaded(c: &Ctx) -> Vec<String> {
    if c.args.len() <= 1 { worlds::all().into_iter().filter(|w| !w.is_loaded()).map(|w| w.name).collect() } else { vec!["--skip-folder-check".into()] }
}
fn tab_create(c: &Ctx) -> Vec<String> {
    match c.args.len() {
        2 => vec!["normal".into(), "nether".into(), "the_end".into()],
        n if n > 2 => CREATE_FLAGS.iter().map(|f| f.0.to_owned()).collect(),
        _ => Vec::new(),
    }
}
fn tab_import(c: &Ctx) -> Vec<String> {
    match c.args.len() {
        1 => {
            let dims = horizon_bukkit::World::of("world").get_world_folder();
            let mut out: Vec<String> = dims.parent().and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| worlds::find(n).is_none() && !matches!(n.as_str(), "overworld" | "the_nether" | "the_end")).collect();
            out.extend(std::fs::read_dir(horizon_bukkit::server::get_world_container()).into_iter().flatten().flatten().filter(|e| worlds::folder_looks_like_world(&e.path())).filter_map(|e| e.file_name().into_string().ok()).filter(|n| worlds::find(n).is_none()));
            out
        }
        2 => vec!["normal".into(), "nether".into(), "the_end".into()],
        _ => IMPORT_FLAGS.iter().map(|f| f.0.to_owned()).collect(),
    }
}
fn tab_tp(c: &Ctx) -> Vec<String> {
    let mut out: Vec<String> = worlds::all().into_iter().filter(|w| w.is_loaded() && !w.hidden()).map(|w| w.name).collect();
    if c.args.len() <= 1 {
        out.extend(horizon_kit::cmd::players(c));
        out.extend(dest::anchors().into_iter().map(|(n, _)| format!("a:{n}")));
    }
    out
}
fn tab_modify(c: &Ctx) -> Vec<String> {
    let actions = ["set", "add", "remove", "reset"];
    let first_is_world = c.args.first().is_some_and(|a| worlds::find(a).is_some() && !actions.contains(&a.as_str()));
    let pos = if first_is_world { c.args.len() - 1 } else { c.args.len() };
    match pos {
        0 | 1 if !first_is_world && c.args.len() <= 1 => {
            let mut v: Vec<String> = actions.iter().map(|s| s.to_string()).collect();
            v.extend(worlds::names());
            v
        }
        1 => actions.iter().map(|s| s.to_string()).collect(),
        2 => worlds::PROPERTIES.iter().map(|p| p.0.to_owned()).collect(),
        3 => {
            let prop = &c.args[c.args.len() - 2];
            match worlds::property(prop).map(|p| p.2) {
                Some(Kind::Bool) => vec!["true".into(), "false".into()],
                Some(Kind::Enum(o)) => o.iter().map(|s| s.to_ascii_lowercase()).collect(),
                _ if prop.eq_ignore_ascii_case("respawn-world") => worlds::names(),
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}
fn tab_gamerule(c: &Ctx) -> Vec<String> {
    match c.args.len() {
        1 => vec!["set".into(), "reset".into(), "list".into()],
        2 if !c.args[0].eq_ignore_ascii_case("list") => horizon_bukkit::World::of("world").get_game_rules(),
        2 => worlds::names(),
        3 if c.args[0].eq_ignore_ascii_case("set") => vec!["true".into(), "false".into()],
        _ => {
            let mut v = worlds::names();
            v.push("*".into());
            v
        }
    }
}
fn tab_config(c: &Ctx) -> Vec<String> {
    if c.args.len() <= 1 { crate::config::leaf_names() } else { vec!["true".into(), "false".into()] }
}
fn tab_anchor(c: &Ctx) -> Vec<String> {
    match c.args.len() {
        1 => vec!["set".into(), "delete".into(), "list".into()],
        2 if c.args[0].eq_ignore_ascii_case("delete") => dest::anchors().into_iter().map(|(n, _)| n).collect(),
        _ => Vec::new(),
    }
}
