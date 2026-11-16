//! `/rtp` (betterrtp, brtp, randomtp, wild, wildtp) and its sub-commands, as Commands.commandExecuted dispatches.

use crate::rtp::{self, Info, RtpType, has};
use crate::settings::{self as files, Shape, map_list};
use crate::{send, settings};
use horizon_bukkit::{CommandSender, Player};
use serde_yaml::Value as Y;

const SUBS: &[(&str, &str, Option<&str>)] = &[
    ("biome", "betterrtp.biome", Some("Biome")),
    ("edit", "betterrtp.edit", Some("Edit")),
    ("help", "betterrtp.use", Some("Help")),
    ("info", "betterrtp.info", Some("Info")),
    ("location", "betterrtp.location", Some("Location")),
    ("player", "betterrtp.player", Some("Player")),
    ("player_sudo", "betterrtp.player", None),
    ("reload", "betterrtp.reload", Some("Reload")),
    ("version", "betterrtp.version", Some("Version")),
    ("world", "betterrtp.world", Some("World")),
    ("test", "betterrtp.admin", None),
];

fn usage(sender: &CommandSender, key: &str, label: &str) {
    crate::send_raw(sender, &format!("Usage.{key}"), &[("command", label)]);
}

fn online(name: &str) -> Option<Player> {
    horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name))
}

fn world_named(name: &str) -> Option<String> {
    horizon_bukkit::get_world(name).or_else(|| horizon_bukkit::get_world(&name.replace('_', " "))).map(|w| w.get_name().to_owned())
}

/// HelperRTP_Info.getBiomes: the biome words after `start` (needs betterrtp.biome).
fn biomes(sender: &CommandSender, args: &[&str], start: usize) -> Option<Vec<String>> {
    if args.len() <= start || !has(sender, "betterrtp.biome") {
        return None;
    }
    let list: Vec<String> = args[start..].iter().flat_map(|a| a.split(',')).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect();
    (!list.is_empty()).then_some(list)
}

fn teleport(sender: &CommandSender, label: &str, world: Option<String>, biomes: Option<Vec<String>>) {
    match sender {
        CommandSender::Player(p) => rtp::tp(p, sender, world, biomes, RtpType::Command, None, Info::ALL),
        _ => crate::sms(sender, &format!("Must be a player to use this command! Try '/{label} help'")),
    }
}

pub fn run(sender: &CommandSender, label: &str, args: &[&str]) -> bool {
    if !has(sender, "betterrtp.use") {
        send(sender, "NoPermission.Basic", &[("permission", "betterrtp.use")]);
        return true;
    }
    let Some(first) = args.first() else {
        teleport(sender, label, None, None);
        return true;
    };
    let s = settings();
    let Some(&(name, perm, _)) = SUBS.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(first)).filter(|(n, _, _)| *n != "test" || s.debug) else {
        send(sender, "Invalid", &[("command", label)]);
        return true;
    };
    if !has(sender, perm) {
        send(sender, "NoPermission.Basic", &[("permission", perm)]);
        return true;
    }
    match name {
        "biome" => match biomes(sender, args, 1) {
            Some(b) => teleport(sender, label, None, Some(b)),
            None => usage(sender, "Biome", label),
        },
        "world" => match args.get(1) {
            Some(w) => match world_named(w) {
                Some(w) => teleport(sender, label, Some(w), biomes(sender, args, 2)),
                None => send(sender, "NotExist", &[("world", w)]),
            },
            None => usage(sender, "World", label),
        },
        "player" | "player_sudo" => {
            let Some(target) = args.get(1) else { return { usage(sender, "Player", label); true } };
            let Some(p) = online(target) else {
                send(sender, "NotOnline", &[("player", target)]);
                return true;
            };
            let world = match args.get(2) {
                Some(w) => match world_named(w) {
                    Some(w) => w,
                    None => {
                        send(sender, "NotExist", &[("world", w)]);
                        return true;
                    }
                },
                None => p.get_world().get_name().to_owned(),
            };
            let info = if name == "player_sudo" { Info { delay: false, cooldown: true, check_cooldown: false, money: false, hunger: false } } else { Info::ALL };
            rtp::tp(&p, sender, Some(world), biomes(sender, args, 3), RtpType::Forced, None, info);
        }
        "location" => location(sender, label, args),
        "reload" => {
            crate::reload();
            send(sender, "Reload", &[]);
        }
        "version" => crate::sms(sender, &format!("&aVersion #&e{} &7(Rust)", env!("CARGO_PKG_VERSION"))),
        "help" => help(sender, label),
        "info" => info(sender, args),
        "edit" => edit(sender, label, args),
        "test" => {
            if let CommandSender::Player(p) = sender {
                let at = p.get_location();
                crate::effects::sound(p, &s.effects.sound_success);
                crate::effects::particles(p, &at);
                crate::effects::potions(p);
                crate::effects::title(p, "Teleport", &at, 0, 0);
            }
        }
        _ => {}
    }
    true
}

fn help(sender: &CommandSender, label: &str) {
    let s = settings();
    let mut lines = vec![s.lang.str("Help.Prefix", ""), s.lang.str("Help.Main", "")];
    for (name, perm, key) in SUBS {
        if let Some(key) = key
            && has(sender, perm)
            && (*name != "test" || s.debug)
        {
            lines.push(s.lang.str(&format!("Help.{key}"), ""));
        }
    }
    for line in lines.into_iter().filter(|l| !l.is_empty()) {
        sender.send_message(&horizon_kit::text::colorize(&line.replace("%command%", label)));
    }
}

fn location(sender: &CommandSender, label: &str, args: &[&str]) {
    let s = settings();
    match args.len() {
        2 => {
            let CommandSender::Player(p) = sender else {
                sender.send_message("Console is not able to execute this command! Try '/rtp help'");
                return;
            };
            let found = rtp::locations_for(&s, sender, Some(p.get_world().get_name())).into_iter().find(|(n, _)| n.eq_ignore_ascii_case(args[1]));
            match found {
                Some((_, l)) => rtp::tp(p, sender, None, None, RtpType::Command, Some(l), Info::new(true, true, true)),
                None => usage(sender, "Location", label),
            }
        }
        3 if has(sender, "betterrtp.player") => {
            let Some(p) = online(args[2]) else {
                send(sender, "NotOnline", &[("player", args[2])]);
                return;
            };
            match rtp::locations_for(&s, sender, None).into_iter().find(|(n, _)| n.eq_ignore_ascii_case(args[1])) {
                Some((_, l)) => rtp::tp(&p, sender, None, None, RtpType::Command, Some(l), Info::new(true, true, true)),
                None => usage(sender, "Location", label),
            }
        }
        _ => usage(sender, "Location", label),
    }
}

fn info(sender: &CommandSender, args: &[&str]) {
    let s = settings();
    let say = |t: &str| sender.send_message(&horizon_kit::text::colorize(t));
    match args.get(1).map(|a| a.to_ascii_lowercase()).as_deref() {
        Some("particles") => {
            say("&e&m-----&6&l BetterRTP &8| Particles &e&m-----");
            say("&7POOF, CRIT, EXPLOSION, FLAME, PORTAL, END_ROD, HAPPY_VILLAGER, WITCH, CLOUD, HEART, ENCHANT, DUST, TOTEM_OF_UNDYING");
        }
        Some("shapes") => {
            say("&e&m-----&6&l BetterRTP &8| Shapes &e&m-----");
            say("&7SCAN, TELEPORT, EXPLODE");
        }
        Some("potion_effects") => {
            say("&e&m-----&6&l BetterRTP &8| Potion effects &e&m-----");
            say("&7BLINDNESS, INVISIBILITY, SLOWNESS, SPEED, NIGHT_VISION, RESISTANCE, REGENERATION, SLOW_FALLING, JUMP_BOOST, LEVITATION, NAUSEA, WEAKNESS");
        }
        Some("world") | Some("player") if args.len() >= 3 => {
            let (player, world) = if args[1].eq_ignore_ascii_case("player") {
                let Some(p) = online(args[2]) else { return send(sender, "NotOnline", &[("player", args[2])]) };
                let w = args.get(3).map_or_else(|| p.get_world().get_name().to_owned(), |w| (*w).to_owned());
                (Some(p), w)
            } else {
                (None, args[2].to_owned())
            };
            if horizon_bukkit::get_world(&world).is_none() {
                return send(sender, "NotExist", &[("world", &world)]);
            }
            world_info(sender, player.as_ref(), &world);
        }
        _ => {
            say("&e&m-----&6&l BetterRTP &8| Info &e&m-----");
            for w in horizon_bukkit::get_worlds() {
                let name = w.get_name().to_owned();
                if s.disabled.contains(&name) {
                    say(&format!("&7- &6{name} &7(&cdisabled&7)"));
                } else if let Some(to) = s.overrides.get(&name) {
                    say(&format!("&7- &6{name} &7-> &e{to} &7(overridden)"));
                } else {
                    say(&format!("&7- &6{name} &7(&a{}&7)", if s.custom.contains_key(&name) { "custom" } else { "default" }));
                }
            }
            say("&7Use &e/rtp info world <world>&7 for details.");
        }
    }
}

fn world_info(sender: &CommandSender, player: Option<&Player>, world: &str) {
    let s = settings();
    let t = rtp::target_for(&s, player, sender, world, None, None);
    let say = |t: &str| sender.send_message(&horizon_kit::text::colorize(t));
    say(&format!("&e&m-----&6&l BetterRTP &8| {} &e&m-----", t.world));
    say(&format!("&7- &6Setup Type: &f{}", if s.custom.contains_key(&t.world) { "CUSTOM_WORLD" } else { "DEFAULT" }));
    say(&format!("&7- &6Use World Border: &f{}", t.s.use_world_border));
    say(&format!("&7- &6Center X: &f{}", t.s.center_x));
    say(&format!("&7- &6Center Z: &f{}", t.s.center_z));
    say(&format!("&7- &6Max Radius: &f{}", t.s.max_radius));
    say(&format!("&7- &6Min Radius: &f{}", t.s.min_radius));
    say(&format!("&7- &6Min Y: &f{} &7Max Y: &f{}", t.s.min_y, t.s.max_y));
    say(&format!("&7- &6Price: &f{}", t.s.price));
    say(&format!("&7- &6Shape: &f{}", if t.s.shape == Shape::Circle { "CIRCLE" } else { "SQUARE" }));
    say(&format!("&7- &6World Type: &f{:?}", t.kind).to_ascii_uppercase());
    say(&format!("&7- &6Biomes: &f{}", if t.s.biomes.is_empty() { "all".into() } else { t.s.biomes.join(", ") }));
    say(&format!("&7- &6Cooldown: &f{}{}", t.s.cooldown, player.map_or_else(|| " &cN/A".to_owned(), |p| if has(&CommandSender::Player(p.clone()), "betterrtp.bypass.cooldown") { " &8(bypassing)".into() } else { String::new() })));
}

// ---- /rtp edit ----

const EDIT_SUBS: &[(&str, &str, &str)] = &[
    ("CENTER_X", "CenterX", "INT"),
    ("CENTER_Z", "CenterZ", "INT"),
    ("MAXRAD", "MaxRadius", "INT"),
    ("MINRAD", "MinRadius", "INT"),
    ("MAXY", "MaxY", "INT"),
    ("MINY", "MinY", "INT"),
    ("PRICE", "Price", "INT"),
    ("SHAPE", "Shape", "SHAPE"),
    ("USEWORLDBORDER", "UseWorldBorder", "BOL"),
];

fn parse_edit(kind: &str, v: &str) -> Option<Y> {
    match kind {
        "INT" => v.parse::<i64>().ok().map(Y::from),
        "BOL" => v.parse::<bool>().ok().map(Y::Bool),
        "SHAPE" => ["square", "circle"].iter().find(|s| s.eq_ignore_ascii_case(v)).map(|s| Y::String(s.to_string())),
        _ => Some(Y::String(v.to_owned())),
    }
}

/// Changes config.yml (or locations.yml) and reads everything again.
fn save_file(name: &str, f: impl FnOnce(&mut Y)) -> bool {
    let Some(folder) = crate::folder() else { return false };
    let path = folder.join(name);
    let mut doc: Y = std::fs::read_to_string(&path).ok().and_then(|t| horizon_kit::yaml::parse(&t).ok()).unwrap_or(Y::Null);
    f(&mut doc);
    let Ok(text) = serde_yaml::to_string(&doc) else { return false };
    if std::fs::write(&path, text).is_err() {
        return false;
    }
    crate::reload();
    true
}

/// Sets `key` in the `- name: {...}` entry of a list (made when missing).
fn set_in_list(list: &mut Y, name: &str, key: &str, value: Y) {
    if !list.is_sequence() {
        *list = Y::Sequence(Vec::new());
    }
    let Y::Sequence(items) = list else { return };
    for item in items.iter_mut() {
        if let Y::Mapping(m) = item
            && let Some(section) = m.get_mut(name)
        {
            if !section.is_mapping() {
                *section = Y::Mapping(Default::default());
            }
            if let Y::Mapping(sm) = section {
                sm.insert(key.into(), value);
            }
            return;
        }
    }
    let mut section = serde_yaml::Mapping::new();
    section.insert(key.into(), value);
    let mut entry = serde_yaml::Mapping::new();
    entry.insert(name.into(), Y::Mapping(section));
    items.push(Y::Mapping(entry));
}

fn set_pair_in_list(list: &mut Y, name: &str, value: Option<Y>) {
    if !list.is_sequence() {
        *list = Y::Sequence(Vec::new());
    }
    let Y::Sequence(items) = list else { return };
    items.retain(|i| !matches!(i, Y::Mapping(m) if m.contains_key(name)));
    if let Some(v) = value {
        let mut entry = serde_yaml::Mapping::new();
        entry.insert(name.into(), v);
        items.push(Y::Mapping(entry));
    }
}

fn edit(sender: &CommandSender, label: &str, args: &[&str]) {
    let ok = |t: &str, v: &str| send(sender, "Edit.Set", &[("type", t), ("value", v)]);
    let err = || send(sender, "Edit.Error", &[]);
    let sub_of = |s: &str| EDIT_SUBS.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(s)).copied();
    match args.get(1).map(|a| a.to_ascii_uppercase()).as_deref() {
        Some("DEFAULT") => {
            let (Some(sub), Some(v)) = (args.get(2).and_then(|s| sub_of(s)), args.get(3)) else { return usage(sender, "Edit.Default", label) };
            let Some(value) = parse_edit(sub.2, v) else { return err() };
            if save_file("config.yml", |d| files::set_path(d, &format!("Default.{}", sub.1), value)) { ok(sub.1, v) } else { err() }
        }
        Some("CUSTOMWORLD") => {
            let (Some(w), Some(sub), Some(v)) = (args.get(2), args.get(3).and_then(|s| sub_of(s)), args.get(4)) else { return usage(sender, "Edit.World", label) };
            if horizon_bukkit::get_world(w).is_none() {
                return send(sender, "NotExist", &[("world", w)]);
            }
            let Some(value) = parse_edit(sub.2, v) else { return err() };
            let w = (*w).to_owned();
            if save_file("config.yml", |d| {
                let mut list = d.get("CustomWorlds").cloned().unwrap_or(Y::Null);
                set_in_list(&mut list, &w, sub.1, value);
                files::set_path(d, "CustomWorlds", list);
            }) {
                ok(sub.1, v)
            } else {
                err()
            }
        }
        Some("LOCATION") => {
            let (Some(name), Some(sub), Some(v)) = (args.get(2), args.get(3).and_then(|s| sub_of(s)), args.get(4)) else { return usage(sender, "Edit.Location", label) };
            let Some(value) = parse_edit(sub.2, v) else { return err() };
            let name = (*name).to_owned();
            if save_file("locations.yml", |d| {
                let mut list = d.get("Locations").cloned().unwrap_or(Y::Null);
                set_in_list(&mut list, &name, sub.1, value);
                files::set_path(d, "Locations", list);
            }) {
                ok(sub.1, v)
            } else {
                err()
            }
        }
        Some("PERMISSION_GROUP") => {
            let (Some(group), Some(world), Some(sub), Some(v)) = (args.get(2), args.get(3), args.get(4).and_then(|s| sub_of(s)), args.get(5)) else { return usage(sender, "Edit.PermissionGroup", label) };
            let Some(value) = parse_edit(sub.2, v) else { return err() };
            let (group, world) = ((*group).to_owned(), (*world).to_owned());
            if save_file("config.yml", |d| {
                let mut groups = d.get("PermissionGroup").and_then(|p| p.get("Groups")).cloned().unwrap_or(Y::Null);
                let mut worlds = map_list(Some(&groups)).into_iter().find(|(g, _)| *g == group).map(|(_, w)| w).unwrap_or(Y::Null);
                set_in_list(&mut worlds, &world, sub.1, value);
                set_pair_in_list(&mut groups, &group, Some(worlds));
                files::set_path(d, "PermissionGroup.Groups", groups);
            }) {
                ok(sub.1, v)
            } else {
                err()
            }
        }
        Some("WORLD_TYPE") => {
            let (Some(w), Some(t)) = (args.get(2), args.get(3)) else { return usage(sender, "Edit.Worldtype", label) };
            if horizon_bukkit::get_world(w).is_none() {
                return send(sender, "NotExist", &[("world", w)]);
            }
            let t = t.to_ascii_uppercase();
            if t != "NORMAL" && t != "NETHER" {
                return err();
            }
            let w = (*w).to_owned();
            let tt = t.clone();
            if save_file("config.yml", |d| {
                let mut list = d.get("WorldType").cloned().unwrap_or(Y::Null);
                set_pair_in_list(&mut list, &w, Some(Y::String(tt)));
                files::set_path(d, "WorldType", list);
            }) {
                ok("WorldType", &t)
            } else {
                err()
            }
        }
        Some("OVERRIDE") => {
            let (Some(w), Some(to)) = (args.get(2), args.get(3)) else { return usage(sender, "Edit.Override", label) };
            if horizon_bukkit::get_world(w).is_none() {
                return send(sender, "NotExist", &[("world", w)]);
            }
            let remove = to.eq_ignore_ascii_case("REMOVE_OVERRIDE");
            let (w, to) = ((*w).to_owned(), (*to).to_owned());
            let shown = to.clone();
            if save_file("config.yml", |d| {
                let mut list = d.get("Overrides").cloned().unwrap_or(Y::Null);
                set_pair_in_list(&mut list, &w, (!remove).then(|| Y::String(to)));
                files::set_path(d, "Overrides", list);
            }) {
                ok("Override", &shown)
            } else {
                err()
            }
        }
        Some("BLACKLISTEDBLOCKS") => {
            let (Some(action), Some(block)) = (args.get(2), args.get(3)) else { return usage(sender, "Edit.BlacklistedBlocks", label) };
            let add = action.eq_ignore_ascii_case("add");
            if !add && !action.eq_ignore_ascii_case("remove") {
                return usage(sender, "Edit.BlacklistedBlocks", label);
            }
            let block = block.to_ascii_lowercase();
            let shown = block.clone();
            if save_file("config.yml", |d| {
                let mut list: Vec<String> = d.get("BlacklistedBlocks").and_then(Y::as_sequence).map(|s| s.iter().filter_map(files::key_str).collect()).unwrap_or_default();
                list.retain(|b| !b.eq_ignore_ascii_case(&block));
                if add {
                    list.push(block);
                }
                files::set_path(d, "BlacklistedBlocks", Y::Sequence(list.into_iter().map(Y::String).collect()));
            }) {
                ok("BlacklistedBlocks", &shown)
            } else {
                err()
            }
        }
        _ => usage(sender, "Edit.Base", label),
    }
}

// ---- Tab completion ----

const BIOMES: &[&str] = &[
    "plains", "sunflower_plains", "snowy_plains", "ice_spikes", "desert", "swamp", "mangrove_swamp", "forest", "flower_forest", "birch_forest", "dark_forest", "old_growth_birch_forest", "old_growth_pine_taiga", "old_growth_spruce_taiga", "taiga", "snowy_taiga", "savanna", "savanna_plateau", "windswept_hills", "windswept_gravelly_hills", "windswept_forest", "windswept_savanna", "jungle", "sparse_jungle", "bamboo_jungle", "badlands", "eroded_badlands", "wooded_badlands", "meadow", "cherry_grove", "grove", "snowy_slopes", "frozen_peaks", "jagged_peaks", "stony_peaks", "river", "frozen_river", "beach", "snowy_beach", "stony_shore", "warm_ocean", "lukewarm_ocean", "ocean", "cold_ocean", "frozen_ocean", "deep_ocean", "mushroom_fields", "pale_garden", "nether_wastes", "crimson_forest", "warped_forest", "soul_sand_valley", "basalt_deltas",
];

pub fn complete(sender: &CommandSender, args: &[&str]) -> Vec<String> {
    let s = settings();
    let last = args.last().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let worlds = || horizon_bukkit::get_worlds().into_iter().map(|w| w.get_name().to_owned()).collect::<Vec<_>>();
    let out: Vec<String> = match args {
        [] => Vec::new(),
        [_] => SUBS.iter().filter(|(n, p, _)| has(sender, p) && (*n != "test" || s.debug)).map(|(n, _, _)| n.to_string()).collect(),
        [first, rest @ ..] => match first.to_ascii_lowercase().as_str() {
            "world" if rest.len() == 1 => worlds(),
            "world" => BIOMES.iter().map(|b| b.to_string()).collect(),
            "player" | "player_sudo" if rest.len() == 1 => horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()).collect(),
            "player" | "player_sudo" if rest.len() == 2 => worlds(),
            "player" | "player_sudo" => BIOMES.iter().map(|b| b.to_string()).collect(),
            "biome" => BIOMES.iter().map(|b| b.to_string()).collect(),
            "location" if rest.len() == 1 => rtp::locations_for(&s, sender, None).into_iter().map(|(n, _)| n).collect(),
            "info" if rest.len() == 1 => ["particles", "shapes", "potion_effects", "world", "player"].iter().map(|s| s.to_string()).collect(),
            "info" if rest.len() == 2 && rest[0].eq_ignore_ascii_case("world") => worlds(),
            "edit" => match rest.len() {
                1 => ["default", "customworld", "location", "permission_group", "world_type", "override", "blacklistedblocks"].iter().map(|s| s.to_string()).collect(),
                2 if rest[0].eq_ignore_ascii_case("default") => EDIT_SUBS.iter().map(|e| e.0.to_ascii_lowercase()).collect(),
                2 if rest[0].eq_ignore_ascii_case("blacklistedblocks") => vec!["add".into(), "remove".into()],
                2 => worlds(),
                3 if rest[0].eq_ignore_ascii_case("customworld") => EDIT_SUBS.iter().map(|e| e.0.to_ascii_lowercase()).collect(),
                3 if rest[0].eq_ignore_ascii_case("world_type") => vec!["NORMAL".into(), "NETHER".into()],
                3 if rest[0].eq_ignore_ascii_case("override") => {
                    let mut w = worlds();
                    w.push("REMOVE_OVERRIDE".into());
                    w
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        },
    };
    out.into_iter().filter(|o| o.to_ascii_lowercase().starts_with(&last)).collect()
}
