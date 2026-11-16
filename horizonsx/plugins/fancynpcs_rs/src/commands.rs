//! `/npc ...` and `/fancynpcs ...`, with FancyNpcs' messages (languages/default.yml) and permissions
//! (`fancynpcs.command.npc.<sub>`).

use crate::npc::{Action, Data, Npc, TRIGGERS};
use crate::{actions, mark_dirty, name_json, net, say, send_mini, tr, view};
use horizon_bukkit::{CommandSender, Location, Player, Plugin};

const SUBS: &[&str] = &[
    "action", "attribute", "center", "collidable", "copy", "create", "displayname", "equipment", "fix", "glowing", "help", "info", "interaction_cooldown", "list", "move_here", "move_to", "nearby", "remove", "rotate", "scale", "show_in_tab", "skin", "teleport", "turn_to_player", "turn_to_player_distance", "type", "visibility", "visibility_distance",
];

const ATTRIBUTES: &[(&str, &[&str], bool)] = &[
    ("on_fire", &["true", "false"], false),
    ("invisible", &["true", "false"], false),
    ("silent", &["true", "false"], false),
    ("shaking", &["true", "false"], false),
    ("on_ground", &["true", "false"], false),
    ("use_item", &["main_hand", "off_hand", "none"], false),
    ("pose", &["standing", "crouching", "sleeping", "swimming", "sitting"], true),
];

const ENTITY_TYPES: &[&str] = &[
    "PLAYER", "ALLAY", "ARMADILLO", "ARMOR_STAND", "AXOLOTL", "BAT", "BEE", "BLAZE", "BOGGED", "BREEZE", "CAMEL", "CAT", "CAVE_SPIDER", "CHICKEN", "COD", "COPPER_GOLEM", "COW", "CREAKING", "CREEPER", "DOLPHIN", "DONKEY", "DROWNED", "ELDER_GUARDIAN", "ENDERMAN", "ENDERMITE", "ENDER_DRAGON", "EVOKER", "FOX", "FROG", "GHAST", "GLOW_SQUID", "GOAT", "GUARDIAN", "HAPPY_GHAST", "HOGLIN", "HORSE", "HUSK", "ILLUSIONER", "IRON_GOLEM", "LLAMA", "MAGMA_CUBE", "MANNEQUIN", "MOOSHROOM", "MULE", "OCELOT", "PANDA", "PARROT", "PHANTOM", "PIG", "PIGLIN", "PIGLIN_BRUTE", "PILLAGER", "POLAR_BEAR", "PUFFERFISH", "RABBIT", "RAVAGER", "SALMON", "SHEEP", "SHULKER", "SILVERFISH", "SKELETON", "SKELETON_HORSE", "SLIME", "SNIFFER", "SNOW_GOLEM", "SPIDER", "SQUID", "STRAY", "STRIDER", "TADPOLE", "TRADER_LLAMA", "TROPICAL_FISH", "TURTLE", "VEX", "VILLAGER", "VINDICATOR", "WANDERING_TRADER", "WARDEN", "WITCH", "WITHER", "WITHER_SKELETON", "WOLF", "ZOGLIN", "ZOMBIE", "ZOMBIE_HORSE", "ZOMBIE_VILLAGER", "ZOMBIFIED_PIGLIN", "TEXT_DISPLAY", "ITEM_DISPLAY", "BLOCK_DISPLAY", "INTERACTION",
];

pub fn register(plugin: &Plugin) {
    if !crate::cfg_bool("register_commands", true) {
        return;
    }
    plugin.set_executor("npc", |s, _, a| {
        npc_command(s, a);
        true
    });
    plugin.set_tab_completer("npc", |s, _, a| complete(s, a));
    plugin.set_executor("fancynpcs", |s, _, a| {
        fancynpcs_command(s, a);
        true
    });
    plugin.set_tab_completer("fancynpcs", |_, _, a| {
        if a.len() <= 1 {
            ["version", "reload", "save", "feature_flags"].iter().filter(|o| o.starts_with(&a.first().map(|x| x.to_ascii_lowercase()).unwrap_or_default())).map(|o| o.to_string()).collect()
        } else {
            Vec::new()
        }
    });
}

fn has(s: &CommandSender, perm: &str) -> bool {
    s.has_permission(perm) || s.has_permission("fancynpcs.admin") || s.has_permission("fancynpcs.command.npc.*")
}

fn player(s: &CommandSender) -> Option<Player> {
    match s {
        CommandSender::Player(p) => Some(p.clone()),
        _ => None,
    }
}

fn syntax(s: &CommandSender, key: &str) {
    send_mini(s, &tr(&format!("command_syntax.{key}"), &[]), true);
}

fn bool_text(b: bool) -> String {
    tr(if b { "true" } else { "false" }, &[])
}

/// Changes one NPC by name: `f` gets it (under the NPC lock); None = no such NPC (told).
fn with_npc<R>(s: &CommandSender, name: &str, f: impl FnOnce(&mut Npc) -> R) -> Option<R> {
    let mut all = view::npcs();
    match all.iter_mut().find(|n| n.data.name.eq_ignore_ascii_case(name)) {
        Some(n) => Some(f(n)),
        None => {
            drop(all);
            say(s, "command_invalid_npc", &[("input", name)]);
            None
        }
    }
}

fn parse_bool(s: &CommandSender, v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" => Some(true),
        "false" | "off" | "no" => Some(false),
        _ => {
            say(s, "command_invalid_boolean", &[("input", v)]);
            None
        }
    }
}

fn parse_num(s: &CommandSender, v: &str) -> Option<f64> {
    match v.parse::<f64>() {
        Ok(n) => Some(n),
        Err(_) => {
            say(s, "command_invalid_number", &[("input", v)]);
            None
        }
    }
}

fn blocked(value: &str) -> bool {
    let plain = horizon_kit::mini::plain(value).to_ascii_lowercase();
    let blocked = crate::files(|f| f.config.list("blocked_commands"));
    blocked.iter().any(|b| {
        let b = b.to_ascii_lowercase();
        plain.split_whitespace().any(|w| w.trim_start_matches('/').rsplit(':').next() == Some(b.as_str()))
    })
}

fn changed(n: &mut Npc) {
    n.dirty = true;
    mark_dirty();
}

pub fn npc_command(s: &CommandSender, a: &[&str]) {
    if !has(s, "fancynpcs.command.npc") {
        return say(s, "command_missing_permissions", &[]);
    }
    let Some(sub) = a.first().map(|x| x.to_ascii_lowercase()) else { return syntax(s, "npc") };
    if !SUBS.contains(&sub.as_str()) {
        return say(s, "command_wrong_usage", &[]);
    }
    let base = format!("fancynpcs.command.npc.{sub}");
    // action / attribute / equipment check their own nodes.
    if !matches!(sub.as_str(), "action" | "attribute" | "equipment" | "help") && !has(s, &base) {
        return say(s, "command_missing_permissions", &[]);
    }
    let rest = &a[1..];
    match sub.as_str() {
        "help" => help(s, rest.first().and_then(|p| p.parse().ok()).unwrap_or(1)),
        "create" => create(s, rest),
        "list" => list(s, rest, false),
        "nearby" => list(s, rest, true),
        _ => {
            let Some(name) = rest.first() else { return syntax(s, &format!("npc_{sub}")) };
            let args = &rest[1..];
            npc_sub(s, &sub, name, args);
        }
    }
}

fn npc_sub(s: &CommandSender, sub: &str, name: &str, args: &[&str]) {
    match sub {
        "remove" => {
            let mut all = view::npcs();
            let Some(at) = all.iter().position(|n| n.data.name.eq_ignore_ascii_case(name)) else {
                drop(all);
                return say(s, "command_invalid_npc", &[("input", name)]);
            };
            let mut n = all.remove(at);
            drop(all);
            view::remove_all(&mut n);
            mark_dirty();
            say(s, "npc_remove_success", &[("npc", &n.data.name)]);
        }
        "copy" => {
            let Some(new_name) = args.first() else { return syntax(s, "npc_copy") };
            if view::npcs().iter().any(|n| n.data.name.eq_ignore_ascii_case(new_name)) {
                return say(s, "npc_create_failure_already_exists", &[("npc", new_name)]);
            }
            let Some(mut data) = with_npc(s, name, |n| n.data.clone()) else { return };
            data.id = crate::new_uuid_string();
            data.name = (*new_name).to_owned();
            let mut n = Npc::new(data);
            n.name_json = name_json(&n.data.display_name, None);
            let old = name.to_owned();
            view::npcs().push(n);
            mark_dirty();
            crate::FORCE_VISIBILITY.store(true, std::sync::atomic::Ordering::Release);
            say(s, "npc_copy_success", &[("npc", &old), ("new_npc", new_name)]);
        }
        "info" => info(s, name),
        "teleport" => {
            let Some(p) = player(s) else { return say(s, "command_player_only", &[]) };
            if let Some(at) = with_npc(s, name, |n| (n.data.world.clone(), n.data.pos, n.data.yaw, n.data.pitch))
                && let Some(w) = horizon_bukkit::get_world(&at.0)
            {
                p.teleport(&Location::new(w, at.1[0], at.1[1], at.1[2], at.2, at.3));
            }
        }
        "move_here" => {
            let Some(p) = player(s) else { return say(s, "command_player_only", &[]) };
            let l = p.get_location();
            let done = with_npc(s, name, |n| {
                let world_changed = n.data.world != l.world.get_name();
                n.data.world = l.world.get_name().to_owned();
                n.data.pos = [l.x, l.y, l.z];
                n.data.yaw = l.yaw;
                n.data.pitch = l.pitch;
                changed(n);
                if world_changed { view::respawn_all(n) } else { view::update_all(n, false) }
                n.data.name.clone()
            });
            if let Some(npc) = done {
                crate::FORCE_VISIBILITY.store(true, std::sync::atomic::Ordering::Release);
                say(s, "npc_move_here_success", &[("npc", &npc)]);
            }
        }
        "move_to" => {
            if args.len() < 3 {
                return syntax(s, "npc_move_to");
            }
            let here = player(s).map(|p| p.get_location());
            let coord = |i: usize, base: Option<f64>| -> Option<f64> {
                let a = args[i];
                match (a.strip_prefix('~'), base) {
                    (Some(r), Some(b)) => Some(b + if r.is_empty() { 0.0 } else { r.parse().ok()? }),
                    _ => a.parse().ok(),
                }
            };
            let (Some(x), Some(y), Some(z)) = (coord(0, here.as_ref().map(|l| l.x)), coord(1, here.as_ref().map(|l| l.y)), coord(2, here.as_ref().map(|l| l.z))) else {
                return say(s, "command_invalid_location", &[("input", &args[..3].join(" "))]);
            };
            let world = match args.get(3).filter(|w| !w.starts_with("--")) {
                Some(w) => match horizon_bukkit::get_world(w) {
                    Some(w) => w.get_name().to_owned(),
                    None => return say(s, "command_invalid_world", &[("input", w)]),
                },
                None => match &here {
                    Some(l) => l.world.get_name().to_owned(),
                    None => return say(s, "npc_move_to_failure_must_specify_world", &[]),
                },
            };
            let look = args.iter().any(|a| a.eq_ignore_ascii_case("--look-in-my-direction"));
            let done = with_npc(s, name, |n| {
                n.data.world = world.clone();
                n.data.pos = [x, y, z];
                if look && let Some(l) = &here {
                    n.data.yaw = l.yaw;
                    n.data.pitch = l.pitch;
                }
                changed(n);
                view::respawn_all(n);
                n.data.name.clone()
            });
            if let Some(npc) = done {
                say(s, "npc_move_to_success", &[("npc", &npc), ("x", &format!("{x:.2}")), ("y", &format!("{y:.2}")), ("z", &format!("{z:.2}")), ("world", &world)]);
            }
        }
        "center" => {
            if let Some((npc, p)) = with_npc(s, name, |n| {
                n.data.pos = [n.data.pos[0].floor() + 0.5, n.data.pos[1], n.data.pos[2].floor() + 0.5];
                changed(n);
                view::update_all(n, false);
                (n.data.name.clone(), n.data.pos)
            }) {
                send_mini(s, &format!("Centered NPC <color:#ffcc24>{npc}</color> at <color:#ffcc24>{:.1}, {:.1}, {:.1}</color>.", p[0], p[1], p[2]), true);
            }
        }
        "rotate" => {
            let (Some(yaw), Some(pitch)) = (args.first().and_then(|v| parse_num(s, v)), args.get(1).and_then(|v| parse_num(s, v))) else { return syntax(s, "npc_rotate") };
            if let Some(npc) = with_npc(s, name, |n| {
                n.data.yaw = yaw as f32;
                n.data.pitch = pitch as f32;
                changed(n);
                view::update_all(n, false);
                n.data.name.clone()
            }) {
                say(s, "npc_rotate_set_success", &[("npc", &npc), ("yaw", &format!("{yaw}")), ("pitch", &format!("{pitch}"))]);
            }
        }
        "displayname" => {
            if args.is_empty() {
                return syntax(s, "npc_displayname");
            }
            let value = args.join(" ");
            if blocked(&value) {
                return say(s, "command_input_contains_blocked_command", &[]);
            }
            let value = if value.eq_ignore_ascii_case("@none") { "<empty>".to_owned() } else { value };
            if let Some(npc) = with_npc(s, name, |n| {
                n.data.display_name = value.clone();
                n.name_json = name_json(&value, None);
                changed(n);
                view::update_all(n, false);
                n.data.name.clone()
            }) {
                if value.eq_ignore_ascii_case("<empty>") {
                    say(s, "npc_displayname_set_empty", &[("npc", &npc)]);
                } else {
                    say(s, "npc_displayname_set_name", &[("npc", &npc), ("name", &value)]);
                }
            }
        }
        "type" => {
            let Some(kind) = args.first() else { return syntax(s, "npc_type") };
            let up = kind.to_ascii_uppercase();
            if !ENTITY_TYPES.contains(&up.as_str()) && horizon_kit::wire::ids().is_none_or(|i| !i.entity_types.contains_key(&format!("minecraft:{}", up.to_ascii_lowercase()))) {
                return say(s, "command_invalid_entity_type", &[("input", kind)]);
            }
            if let Some(npc) = with_npc(s, name, |n| {
                n.data.kind = up.clone();
                changed(n);
                view::respawn_all(n);
                n.data.name.clone()
            }) {
                send_mini(s, &format!("NPC <color:#ffcc24>{npc}</color> is now of type <color:#ffcc24>{up}</color>."), true);
            }
        }
        "skin" => skin(s, name, args),
        "glowing" => {
            let arg = args.first().map(|a| a.to_ascii_lowercase());
            let out = with_npc(s, name, |n| {
                match arg.as_deref() {
                    None => n.data.glowing = !n.data.glowing,
                    Some("disabled" | "false" | "off") => n.data.glowing = false,
                    Some(c) => match net::color_id(c) {
                        Some(_) => {
                            n.data.glowing = true;
                            n.data.glowing_color = c.to_owned();
                        }
                        None => return Err(c.to_owned()),
                    },
                }
                changed(n);
                view::update_all(n, false);
                Ok((n.data.name.clone(), n.data.glowing, n.data.glowing_color.clone()))
            });
            match out {
                Some(Ok((npc, true, color))) if arg.is_some() => say(s, "npc_glowing_set_color_success", &[("npc", &npc), ("color", &tr(&format!("color_{color}"), &[]))]),
                Some(Ok((npc, true, _))) => say(s, "npc_glowing_set_true", &[("npc", &npc)]),
                Some(Ok((npc, false, _))) => say(s, "npc_glowing_set_false", &[("npc", &npc)]),
                Some(Err(c)) => say(s, "npc_glowing_set_color_failure_invalid_color", &[("input", &c)]),
                None => {}
            }
        }
        "collidable" | "show_in_tab" | "turn_to_player" => {
            let state = match args.first() {
                Some(v) => match parse_bool(s, v) {
                    Some(b) => Some(b),
                    None => return,
                },
                None => None,
            };
            let sub2 = sub.to_owned();
            if let Some((npc, now)) = with_npc(s, name, |n| {
                let field = match sub2.as_str() {
                    "collidable" => &mut n.data.collidable,
                    "show_in_tab" => &mut n.data.show_in_tab,
                    _ => &mut n.data.turn_to_player,
                };
                *field = state.unwrap_or(!*field);
                let now = *field;
                changed(n);
                if sub2 == "show_in_tab" { view::respawn_all(n) } else { view::update_all(n, false) }
                (n.data.name.clone(), now)
            }) {
                let key = match sub {
                    "collidable" => format!("npc_collidable_set_{now}"),
                    "show_in_tab" => format!("npc_show_in_tab_set_{now}"),
                    _ => format!("npc_turn_to_player_set_{now}"),
                };
                say(s, &key, &[("npc", &npc)]);
            }
        }
        "turn_to_player_distance" => {
            let Some(v) = args.first() else { return syntax(s, "npc_turn_to_player_distance") };
            let d = if v.eq_ignore_ascii_case("default") { -1 } else {
                match parse_num(s, v) {
                    Some(d) => d as i32,
                    None => return,
                }
            };
            if let Some(npc) = with_npc(s, name, |n| {
                n.data.turn_to_player_distance = d;
                changed(n);
                n.data.name.clone()
            }) {
                say(s, "npc_turn_to_player_distance_set", &[("npc", &npc), ("distance", &d.to_string())]);
            }
        }
        "scale" => {
            let Some(f) = args.first().and_then(|v| parse_num(s, v)) else { return syntax(s, "npc_scale") };
            if let Some(npc) = with_npc(s, name, |n| {
                n.data.scale = f as f32;
                changed(n);
                view::update_all(n, false);
                n.data.name.clone()
            }) {
                say(s, "npc_scale_set_success", &[("npc", &npc), ("scale", &format!("{f}"))]);
            }
        }
        "interaction_cooldown" => {
            let Some(v) = args.first() else { return syntax(s, "npc_interaction_cooldown") };
            let secs = if v.eq_ignore_ascii_case("disabled") { 0.0 } else {
                match actions::interval(v) {
                    Some(x) => x,
                    None => return say(s, "command_invalid_interval", &[("input", v)]),
                }
            };
            if with_npc(s, name, |n| {
                n.data.interaction_cooldown = secs as f32;
                changed(n);
            })
            .is_some()
            {
                if secs <= 0.0 {
                    say(s, "npc_interaction_cooldown_disabled", &[]);
                } else {
                    say(s, "npc_interaction_cooldown_set", &[("time", &actions::format_seconds(secs))]);
                }
            }
        }
        "visibility" => {
            let Some(v) = args.first() else { return syntax(s, "npc_visibility") };
            let up = v.to_ascii_uppercase();
            if !["ALL", "PERMISSION_REQUIRED", "MANUAL"].contains(&up.as_str()) {
                return say(s, "command_invalid_npc_visibility", &[("input", v)]);
            }
            if let Some(npc) = with_npc(s, name, |n| {
                n.data.visibility = up.clone();
                changed(n);
                n.data.name.clone()
            }) {
                crate::FORCE_VISIBILITY.store(true, std::sync::atomic::Ordering::Release);
                say(s, "npc_visibility_set", &[("npc", &npc), ("visibility", &up.to_ascii_lowercase())]);
            }
        }
        "visibility_distance" => {
            let Some(v) = args.first() else { return syntax(s, "npc_visibility_distance") };
            let d = match v.to_ascii_lowercase().as_str() {
                "default" => -1,
                "not_visible" => 0,
                "always_visible" => i32::MAX,
                _ => match parse_num(s, v) {
                    Some(d) => d as i32,
                    None => return,
                },
            };
            if let Some(npc) = with_npc(s, name, |n| {
                n.data.visibility_distance = d;
                changed(n);
                n.data.name.clone()
            }) {
                crate::FORCE_VISIBILITY.store(true, std::sync::atomic::Ordering::Release);
                match d {
                    -1 => say(s, "npc_visibility_distance_set_default", &[("npc", &npc), ("distance", &crate::cfg_int("visibility_distance", 20).to_string())]),
                    0 => say(s, "npc_visibility_distance_set_not_visible", &[("npc", &npc)]),
                    i32::MAX => say(s, "npc_visibility_distance_set_always_visible", &[("npc", &npc)]),
                    _ => say(s, "npc_visibility_distance_set_value", &[("npc", &npc), ("distance", &d.to_string())]),
                }
            }
        }
        "fix" => {
            if let Some(npc) = with_npc(s, name, |n| {
                view::respawn_all(n);
                n.data.name.clone()
            }) {
                say(s, "npc_fix_success", &[("npc", &npc)]);
            }
        }
        "equipment" => equipment(s, name, args),
        "attribute" => attribute(s, name, args),
        "action" => action(s, name, args),
        _ => say(s, "command_wrong_usage", &[]),
    }
}

fn help(s: &CommandSender, page: usize) {
    let lines = crate::files(|f| f.lang.list("messages.npc_help_contents"));
    let per = 6;
    let max = lines.len().div_ceil(per).max(1);
    let page = page.clamp(1, max);
    send_mini(s, &tr("npc_help_page_header", &[("page", &page.to_string()), ("max_page", &max.to_string())]), false);
    for l in lines.iter().skip((page - 1) * per).take(per) {
        send_mini(s, &crate::colors(l), false);
    }
    send_mini(s, &tr("npc_help_page_footer", &[]), false);
}

fn create(s: &CommandSender, a: &[&str]) {
    let Some(name) = a.first() else { return syntax(s, "npc_create") };
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/')) {
        return say(s, "npc_create_failure_invalid_name", &[]);
    }
    if view::npcs().iter().any(|n| n.data.name.eq_ignore_ascii_case(name)) {
        return say(s, "npc_create_failure_already_exists", &[("npc", name)]);
    }
    let mut kind = "PLAYER".to_owned();
    let mut world = None;
    let mut at: Option<[f64; 3]> = None;
    let mut i = 1;
    while i < a.len() {
        match a[i].to_ascii_lowercase().as_str() {
            "--type" if i + 1 < a.len() => {
                let up = a[i + 1].to_ascii_uppercase();
                if !ENTITY_TYPES.contains(&up.as_str()) {
                    return say(s, "command_invalid_entity_type", &[("input", a[i + 1])]);
                }
                kind = up;
                i += 2;
            }
            "--world" if i + 1 < a.len() => {
                match horizon_bukkit::get_world(a[i + 1]) {
                    Some(w) => world = Some(w.get_name().to_owned()),
                    None => return say(s, "command_invalid_world", &[("input", a[i + 1])]),
                }
                i += 2;
            }
            "--location" | "--position" if i + 3 < a.len() => {
                let p: Option<Vec<f64>> = a[i + 1..i + 4].iter().map(|v| v.parse().ok()).collect();
                match p {
                    Some(p) => at = Some([p[0], p[1], p[2]]),
                    None => return say(s, "command_invalid_location", &[("input", &a[i + 1..i + 4].join(" "))]),
                }
                i += 4;
            }
            _ => i += 1,
        }
    }
    let here = player(s).map(|p| p.get_location());
    let world = match (world, &here) {
        (Some(w), _) => w,
        (None, Some(l)) => l.world.get_name().to_owned(),
        (None, None) => return say(s, "npc_create_failure_must_specify_world", &[]),
    };
    let (pos, yaw, pitch) = match (at, &here) {
        (Some(p), _) => (p, 0.0, 0.0),
        (None, Some(l)) => ([l.x, l.y, l.z], l.yaw, l.pitch),
        (None, None) => return say(s, "npc_create_failure_must_specify_location", &[]),
    };
    let creator = player(s).map_or_else(|| "00000000-0000-0000-0000-000000000000".to_owned(), |p| p.get_unique_id());
    let mut data = Data::new(name, &creator, &world, pos, yaw, pitch);
    data.kind = kind;
    let mut n = Npc::new(data);
    n.name_json = name_json(&n.data.display_name, None);
    view::npcs().push(n);
    mark_dirty();
    crate::FORCE_VISIBILITY.store(true, std::sync::atomic::Ordering::Release);
    say(s, "npc_create_success", &[("npc", name)]);
}

fn list(s: &CommandSender, a: &[&str], nearby: bool) {
    let flag = |f: &str| a.iter().position(|x| x.eq_ignore_ascii_case(f)).and_then(|i| a.get(i + 1)).map(|v| v.to_ascii_lowercase());
    let kind = flag("--type").map(|t| t.to_ascii_uppercase());
    let here = player(s).map(|p| p.get_location());
    if nearby && here.is_none() {
        return say(s, "command_player_only", &[]);
    }
    let radius: Option<f64> = flag("--radius").and_then(|r| r.parse().ok());
    let all = view::npcs();
    let total = all.len();
    let mut rows: Vec<(String, [f64; 3], String, f64, String)> = all
        .iter()
        .filter(|n| kind.as_ref().is_none_or(|k| n.data.kind == *k))
        .map(|n| {
            let dist = here.as_ref().map_or(f64::MAX, |l| if l.world.get_name() == n.data.world { ((l.x - n.data.pos[0]).powi(2) + (l.y - n.data.pos[1]).powi(2) + (l.z - n.data.pos[2]).powi(2)).sqrt() } else { f64::MAX });
            (n.data.name.clone(), n.data.pos, n.data.world.clone(), dist, n.data.creator.clone())
        })
        .filter(|r| !nearby || (r.3 < f64::MAX && radius.is_none_or(|rad| r.3 <= rad)))
        .collect();
    drop(all);
    match flag("--sort").as_deref() {
        Some("name") | Some("name_ascending") => rows.sort_by(|x, y| x.0.to_ascii_lowercase().cmp(&y.0.to_ascii_lowercase())),
        Some("name_descending") => rows.sort_by(|x, y| y.0.to_ascii_lowercase().cmp(&x.0.to_ascii_lowercase())),
        Some("farthest") => rows.sort_by(|x, y| y.3.total_cmp(&x.3)),
        _ if nearby => rows.sort_by(|x, y| x.3.total_cmp(&y.3)),
        _ => {}
    }
    let pre = if nearby { "npc_nearby" } else { "npc_list" };
    send_mini(s, &tr(&format!("{pre}_header"), &[]), false);
    for (i, r) in rows.iter().enumerate() {
        send_mini(s, &tr(&format!("{pre}_entry"), &[("number", &(i + 1).to_string()), ("npc", &r.0), ("location_x", &format!("{:.2}", r.1[0])), ("location_y", &format!("{:.2}", r.1[1])), ("location_z", &format!("{:.2}", r.1[2])), ("world", &r.2), ("distance", &format!("{:.1}", r.3))]), false);
    }
    send_mini(s, &tr(&format!("{pre}_footer"), &[("count_formatted", &rows.len().to_string()), ("total_formatted", &total.to_string())]), false);
}

fn info(s: &CommandSender, name: &str) {
    let Some(d) = with_npc(s, name, |n| n.data.clone()) else { return };
    let creator_name = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_unique_id() == d.creator).map_or_else(|| tr("unknown", &[]), |p| p.get_name().to_owned());
    let glow = if d.glowing { tr(&format!("color_{}", d.glowing_color.to_ascii_lowercase()), &[]) } else { tr("disabled", &[]) };
    let vis = match d.visibility_distance {
        -1 => tr("default", &[("value", &crate::cfg_int("visibility_distance", 20).to_string())]),
        0 => tr("not_visible", &[]),
        i32::MAX => tr("always_visible", &[]),
        v => v.to_string(),
    };
    let actions_total: usize = d.actions.values().map(Vec::len).sum();
    let text = tr(
        "npc_info_general",
        &[
            ("id", &d.id),
            ("creator_uuid", &d.creator),
            ("creator_uuid_short", d.creator.get(..8).unwrap_or(&d.creator)),
            ("creator_name", &creator_name),
            ("name", &d.name),
            ("displayname", &d.display_name),
            ("type", &d.kind),
            ("location_x", &format!("{:.2}", d.pos[0])),
            ("location_y", &format!("{:.2}", d.pos[1])),
            ("location_z", &format!("{:.2}", d.pos[2])),
            ("world", &d.world),
            ("glow", &glow),
            ("is_turn_to_player", &bool_text(d.turn_to_player)),
            ("is_show_in_tab", &bool_text(d.show_in_tab)),
            ("is_collidable", &bool_text(d.collidable)),
            ("is_skin_mirror", &bool_text(d.mirror_skin)),
            ("interaction_cooldown", &if d.interaction_cooldown > 0.0 { actions::format_seconds(d.interaction_cooldown as f64) } else { tr("disabled", &[]) }),
            ("scale", &format!("{}", d.scale)),
            ("visibility_distance", &vis),
            ("actions_total", &actions_total.to_string()),
        ],
    );
    // One message, as Java sends it: splitting on `<newline>` would cut the hover tooltips (they hold some too).
    send_mini(s, &text, false);
}

fn skin(s: &CommandSender, name: &str, args: &[&str]) {
    let Some(input) = args.first() else { return syntax(s, "npc_skin") };
    let slim = args.iter().any(|a| a.eq_ignore_ascii_case("--slim"));
    let input = (*input).to_owned();
    let npc_name = match with_npc(s, name, |n| n.data.name.clone()) {
        Some(n) => n,
        None => return,
    };
    if !with_npc(s, name, |n| n.data.is_player()).unwrap_or(false) {
        return say(s, "command_unsupported_npc_type", &[]);
    }
    match input.to_ascii_lowercase().as_str() {
        "@none" => {
            with_npc(s, name, |n| {
                n.data.skin = None;
                n.data.mirror_skin = false;
                changed(n);
                view::respawn_all(n);
            });
            return say(s, "npc_skin_set_none", &[("npc", &npc_name)]);
        }
        "@mirror" => {
            with_npc(s, name, |n| {
                n.data.skin = None;
                n.data.mirror_skin = true;
                changed(n);
                view::respawn_all(n);
            });
            return say(s, "npc_skin_set_mirror", &[("npc", &npc_name)]);
        }
        _ => {}
    }
    let identifier = if input.contains('%') { player(s).map_or(input.clone(), |p| horizon_kit::hooks::placeholders(&input, Some(&p))) } else { input.clone() };
    if crate::skins::get(&identifier).is_none() {
        say(s, "npc_skin_set_later", &[]);
    }
    let to = s.clone();
    let shown = input.clone();
    let id2 = identifier.clone();
    crate::skins::request(&identifier, slim, move |result| match result {
        Ok(_) => {
            let mut all = view::npcs();
            if let Some(n) = all.iter_mut().find(|n| n.data.name.eq_ignore_ascii_case(&npc_name)) {
                n.data.skin = Some(crate::npc::SkinRef { identifier: id2.clone(), variant: if slim { "SLIM".into() } else { "AUTO".into() } });
                n.data.mirror_skin = false;
                changed(n);
                view::respawn_all(n);
            }
            drop(all);
            say(&to, "npc_skin_set", &[("npc", &npc_name), ("name", &shown)]);
        }
        Err(e) => {
            crate::log(&format!("Could not load the skin '{shown}': {e}"));
            send_mini(&to, &format!("<color:#E36666>Could not load the skin <color:#ffcc24>{shown}</color>: {e}"), true);
        }
    });
}

fn equipment(s: &CommandSender, name: &str, args: &[&str]) {
    let op = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    if !has(s, &format!("fancynpcs.command.npc.equipment.{op}")) && !has(s, "fancynpcs.command.npc.equipment") {
        return say(s, "command_missing_permissions", &[]);
    }
    match op.as_str() {
        "set" => {
            let (Some(slot), Some(item)) = (args.get(1), args.get(2)) else { return syntax(s, "npc_equipment_set") };
            let Some(slot_name) = ["MAINHAND", "OFFHAND", "HEAD", "CHEST", "LEGS", "FEET", "BODY"].iter().find(|x| view::slot_id(slot).is_some() && view::slot_id(x) == view::slot_id(slot)) else {
                return say(s, "npc_equipment_set_failure_invalid_slot", &[("input", slot)]);
            };
            let new: Option<(String, i32)> = match item.to_ascii_lowercase().as_str() {
                "@none" | "air" => None,
                "@hand" => {
                    let Some(p) = player(s) else { return say(s, "command_player_only", &[]) };
                    p.get_inventory().get_item_in_main_hand().filter(|i| !i.get_type().is_air()).map(|i| (format!("minecraft:{}", i.get_type().name().to_ascii_lowercase()), i.get_amount()))
                }
                other => {
                    let id = if other.contains(':') { other.to_owned() } else { format!("minecraft:{other}") };
                    if horizon_kit::wire::ids().is_some_and(|i| !i.items.contains_key(&id)) {
                        return say(s, "command_invalid_material", &[("input", item)]);
                    }
                    Some((id, 1))
                }
            };
            let slot_name = (*slot_name).to_owned();
            let shown = new.as_ref().map(|(id, _)| id.clone());
            let ok = with_npc(s, name, |n| {
                n.data.equipment.retain(|(k, _)| *k != slot_name);
                if let Some((id, count)) = &new {
                    n.data.equipment.push((slot_name.clone(), crate::npc::Item { id: id.clone(), count: *count, yaml: crate::npc::item_yaml(id, *count) }));
                } else {
                    // An emptied slot: the client is told it's empty.
                    let viewers: Vec<String> = n.viewers.iter().cloned().collect();
                    if let (Some(slot), false) = (view::slot_id(&slot_name), viewers.is_empty()) {
                        horizon_kit::packets::queue_raw(&viewers, net::equipment(n.entity_id, &[(slot, String::new(), 0)]).unwrap_or_default());
                    }
                }
                changed(n);
                view::update_all(n, false);
            });
            if ok.is_some() {
                match shown {
                    Some(id) => say(s, "npc_equipment_set_item", &[("slot", &slot_name.to_ascii_lowercase()), ("item", &id)]),
                    None => say(s, "npc_equipment_set_empty", &[("slot", &slot_name.to_ascii_lowercase())]),
                }
            }
        }
        "clear" => {
            if with_npc(s, name, |n| {
                let slots: Vec<(u8, String, i32)> = n.data.equipment.iter().filter_map(|(k, _)| view::slot_id(k).map(|s| (s, String::new(), 0))).collect();
                n.data.equipment.clear();
                let viewers: Vec<String> = n.viewers.iter().cloned().collect();
                if !viewers.is_empty() {
                    horizon_kit::packets::queue_raw(&viewers, net::equipment(n.entity_id, &slots).unwrap_or_default());
                    horizon_kit::packets::flush();
                }
                changed(n);
            })
            .is_some()
            {
                say(s, "npc_equipment_clear_success", &[]);
            }
        }
        "list" => {
            let Some(eq) = with_npc(s, name, |n| n.data.equipment.clone()) else { return };
            if eq.is_empty() {
                return say(s, "npc_equipment_list_failure_empty", &[]);
            }
            send_mini(s, &tr("npc_equipment_list_header", &[]), false);
            for (slot, item) in eq {
                send_mini(s, &tr("npc_equipment_list_entry", &[("slot", &slot.to_ascii_lowercase())]).replace("<item>", &format!("{} x{}", item.id, item.count)), false);
            }
            send_mini(s, &tr("npc_equipment_list_footer", &[]), false);
        }
        _ => syntax(s, "npc_equipment"),
    }
}

fn attribute(s: &CommandSender, name: &str, args: &[&str]) {
    let op = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    if !has(s, &format!("fancynpcs.command.npc.attribute.{op}")) && !has(s, "fancynpcs.command.npc.attribute") {
        return say(s, "command_missing_permissions", &[]);
    }
    match op.as_str() {
        "set" => {
            let (Some(attr), Some(value)) = (args.get(1), args.get(2)) else { return syntax(s, "npc_attribute_set") };
            let Some((attr_name, values, player_only)) = ATTRIBUTES.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(attr)) else {
                return say(s, "command_invalid_attribute", &[("input", attr)]);
            };
            if !values.iter().any(|v| v.eq_ignore_ascii_case(value)) {
                return say(s, "command_invalid_attribute_value", &[("input", value)]);
            }
            let (attr_name, value) = ((*attr_name).to_owned(), value.to_ascii_lowercase());
            let player_only = *player_only;
            let out = with_npc(s, name, |n| {
                if player_only && !n.data.is_player() {
                    return false;
                }
                n.data.attributes.retain(|(k, _)| *k != attr_name);
                n.data.attributes.push((attr_name.clone(), value.clone()));
                changed(n);
                view::update_all(n, false);
                true
            });
            match out {
                Some(true) => say(s, "npc_attribute_set", &[("attribute", &attr_name), ("value", &value)]),
                Some(false) => say(s, "npc_attribute_set_invalid_for_this_entity_type", &[("input", &attr_name)]),
                None => {}
            }
        }
        "list" => {
            let Some(attrs) = with_npc(s, name, |n| n.data.attributes.clone()) else { return };
            if attrs.is_empty() {
                return say(s, "npc_attribute_list_failure_empty", &[]);
            }
            send_mini(s, &tr("npc_attribute_list_header", &[]), false);
            for (k, v) in attrs {
                send_mini(s, &tr("npc_attribute_list_entry", &[("attribute", &k), ("value", &v)]), false);
            }
            send_mini(s, &tr("npc_attribute_list_footer", &[]), false);
        }
        _ => syntax(s, "npc_attribute"),
    }
}

fn renumber(list: &mut [Action]) {
    for (i, a) in list.iter_mut().enumerate() {
        a.order = i as i32 + 1;
    }
}

fn action(s: &CommandSender, name: &str, args: &[&str]) {
    let (Some(trigger), Some(op)) = (args.first(), args.get(1)) else { return syntax(s, "npc_action") };
    let trigger = trigger.to_ascii_uppercase();
    if !TRIGGERS.contains(&trigger.as_str()) {
        return say(s, "command_invalid_action_trigger", &[("input", &trigger)]);
    }
    let op = op.to_ascii_lowercase();
    let perm_op = match op.as_str() {
        "add_before" => "addBefore",
        "add_after" => "addAfter",
        "move_up" => "moveUp",
        "move_down" => "moveDown",
        o => o,
    };
    if !has(s, &format!("fancynpcs.command.npc.action.{perm_op}")) && !has(s, "fancynpcs.command.npc.action") {
        return say(s, "command_missing_permissions", &[]);
    }
    let rest = &args[2..];
    // (index, type, value) for the adding forms.
    let parse_add = |with_index: bool| -> Result<(usize, String, Option<String>), ()> {
        let (index, rest) = if with_index {
            let Some(i) = rest.first().and_then(|i| i.parse::<usize>().ok()) else {
                say(s, "command_invalid_number", &[("input", rest.first().copied().unwrap_or(""))]);
                return Err(());
            };
            (i, &rest[1..])
        } else {
            (0, rest)
        };
        let Some(kind) = rest.first().map(|k| k.to_ascii_lowercase()) else {
            syntax(s, &format!("npc_action_{op}"));
            return Err(());
        };
        if !actions::TYPES.contains(&kind.as_str()) {
            say(s, "command_invalid_action_type", &[("input", &kind)]);
            return Err(());
        }
        if !has(s, "fancynpcs.command.npc.action.add.*") && !has(s, &format!("fancynpcs.command.npc.action.add.{kind}")) {
            say(s, "command_missing_permissions", &[]);
            return Err(());
        }
        let value = (rest.len() > 1).then(|| rest[1..].join(" "));
        if let Some(v) = &value
            && blocked(v)
        {
            say(s, "command_input_contains_blocked_command", &[]);
            return Err(());
        }
        if actions::requires_value(&kind) && value.as_deref().is_none_or(str::is_empty) {
            say(s, "npc_action_requires_value", &[]);
            return Err(());
        }
        if kind == "player_command_as_op" {
            say(s, "npc_action_add_op_warning", &[]);
            return Err(());
        }
        Ok((index, kind, value))
    };
    match op.as_str() {
        "add" | "add_before" | "add_after" | "set" => {
            let Ok((index, kind, value)) = parse_add(op != "add") else { return };
            let trig = trigger.clone();
            let op2 = op.clone();
            let out = with_npc(s, name, move |n| {
                let list = n.data.actions.entry(trig).or_default();
                list.sort_by_key(|a| a.order);
                let a = Action { order: 0, action: kind, value };
                let ok = match op2.as_str() {
                    "add" => {
                        list.push(a);
                        true
                    }
                    "add_before" => {
                        list.insert(index.saturating_sub(1).min(list.len()), a);
                        true
                    }
                    "add_after" => {
                        list.insert(index.min(list.len()), a);
                        true
                    }
                    _ => {
                        if index >= 1 && index <= list.len() {
                            list[index - 1] = a;
                            true
                        } else {
                            false
                        }
                    }
                };
                renumber(list);
                let total = list.len();
                changed(n);
                (ok, total)
            });
            let Some((ok, total)) = out else { return };
            let (t, i) = (total.to_string(), index.to_string());
            match (op.as_str(), ok) {
                ("add", _) => say(s, "npc_action_add_success", &[("total", &t)]),
                ("add_before", _) => say(s, "npc_action_add_before_success", &[("number", &i), ("total", &t)]),
                ("add_after", _) => say(s, "npc_action_add_after_success", &[("number", &i), ("total", &t)]),
                (_, true) => say(s, "npc_action_set_success", &[("number", &i), ("total", &t)]),
                (_, false) => say(s, "npc_action_set_failure", &[("number", &i)]),
            }
        }
        "remove" | "move_up" | "move_down" => {
            let Some(index) = rest.first().and_then(|i| i.parse::<usize>().ok()) else { return syntax(s, &format!("npc_action_{op}")) };
            let trig = trigger.clone();
            let op2 = op.clone();
            let out = with_npc(s, name, move |n| {
                let list = n.data.actions.entry(trig).or_default();
                list.sort_by_key(|a| a.order);
                let ok = match op2.as_str() {
                    "remove" if index >= 1 && index <= list.len() => {
                        list.remove(index - 1);
                        true
                    }
                    "move_up" if index >= 2 && index <= list.len() => {
                        list.swap(index - 1, index - 2);
                        true
                    }
                    "move_down" if index >= 1 && index < list.len() => {
                        list.swap(index - 1, index);
                        true
                    }
                    _ => false,
                };
                renumber(list);
                let total = list.len();
                changed(n);
                (ok, total)
            });
            let Some((ok, total)) = out else { return };
            let key = format!("npc_action_{op}_{}", if ok { "success" } else { "failure" });
            say(s, &key, &[("number", &index.to_string()), ("total", &total.to_string())]);
        }
        "clear" => {
            let trig = trigger.clone();
            if with_npc(s, name, move |n| {
                n.data.actions.remove(&trig);
                changed(n);
            })
            .is_some()
            {
                say(s, "npc_action_clear_success", &[]);
            }
        }
        "list" => {
            let Some(list) = with_npc(s, name, |n| n.data.actions_of(&trigger)) else { return };
            if list.is_empty() {
                return say(s, "npc_action_list_failure_empty", &[]);
            }
            send_mini(s, &tr("npc_action_list_header", &[]), false);
            for (i, a) in list.iter().enumerate() {
                send_mini(s, &tr("npc_action_list_entry", &[("number", &(i + 1).to_string()), ("action", &a.action), ("value", &a.value.clone().unwrap_or_default().replace('<', "\\<"))]), false);
            }
            send_mini(s, &tr("npc_action_list_footer", &[("total", &list.len().to_string())]), false);
        }
        _ => syntax(s, "npc_action"),
    }
}

pub fn fancynpcs_command(s: &CommandSender, a: &[&str]) {
    if !s.has_permission("fancynpcs.command.fancynpcs") && !s.has_permission("fancynpcs.admin") {
        return say(s, "command_missing_permissions", &[]);
    }
    match a.first().map(|x| x.to_ascii_lowercase()).as_deref() {
        Some("version") => send_mini(s, "<color:#ffcc24>FancyNpcs</color> version <color:#ffcc24>2.12.1+382</color> (Rust, Horizon)", true),
        Some("reload") => {
            crate::load_files();
            crate::load_npcs();
            say(s, "fancynpcs_reload_success", &[]);
        }
        Some("save") => {
            crate::save_npcs();
            say(s, "fancynpcs_save_success", &[]);
        }
        Some("feature_flags") => {
            send_mini(s, &tr("fancynpcs_feature_flags_header", &[]), false);
            send_mini(s, &tr("fancynpcs_feature_flags_footer", &[("total_formatted", "0")]), false);
        }
        _ => syntax(s, "fancynpcs"),
    }
}

fn complete(s: &CommandSender, a: &[&str]) -> Vec<String> {
    let last = a.last().map(|x| x.to_ascii_lowercase()).unwrap_or_default();
    let names = || view::npcs().iter().map(|n| n.data.name.clone()).collect::<Vec<_>>();
    let out: Vec<String> = match a.len() {
        0 | 1 => SUBS.iter().filter(|x| has(s, &format!("fancynpcs.command.npc.{x}")) || matches!(**x, "help" | "action" | "attribute" | "equipment")).map(|x| x.to_string()).collect(),
        2 => match a[0].to_ascii_lowercase().as_str() {
            "create" | "help" => Vec::new(),
            "list" | "nearby" => vec!["--type".into(), "--sort".into(), "--radius".into()],
            _ => names(),
        },
        n => {
            let sub = a[0].to_ascii_lowercase();
            match (sub.as_str(), n) {
                ("action", 3) => TRIGGERS.iter().map(|t| t.to_ascii_lowercase()).collect(),
                ("action", 4) => ["add", "add_before", "add_after", "set", "remove", "move_up", "move_down", "clear", "list"].iter().map(|x| x.to_string()).collect(),
                ("action", 5) if a[3].eq_ignore_ascii_case("add") => actions::TYPES.iter().map(|x| x.to_string()).collect(),
                ("action", 6) if matches!(a[3].to_ascii_lowercase().as_str(), "add_before" | "add_after" | "set") => actions::TYPES.iter().map(|x| x.to_string()).collect(),
                ("attribute", 3) | ("equipment", 3) => if sub == "attribute" { vec!["set".into(), "list".into()] } else { vec!["set".into(), "clear".into(), "list".into()] },
                ("attribute", 4) => ATTRIBUTES.iter().map(|x| x.0.to_string()).collect(),
                ("attribute", 5) => ATTRIBUTES.iter().find(|x| x.0.eq_ignore_ascii_case(a[3])).map(|x| x.1.iter().map(|v| v.to_string()).collect()).unwrap_or_default(),
                ("equipment", 4) => ["mainhand", "offhand", "head", "chest", "legs", "feet", "body"].iter().map(|x| x.to_string()).collect(),
                ("equipment", 5) => vec!["@hand".into(), "@none".into()],
                ("type", 3) => ENTITY_TYPES.iter().map(|x| x.to_ascii_lowercase()).collect(),
                ("skin", 3) => {
                    let mut v = vec!["@none".to_owned(), "@mirror".to_owned()];
                    v.extend(horizon_bukkit::get_online_players().iter().map(|p| p.get_name().to_owned()));
                    v
                }
                ("glowing", 3) => {
                    let mut v: Vec<String> = net::COLORS.iter().map(|x| x.to_string()).collect();
                    v.push("disabled".into());
                    v
                }
                ("collidable" | "show_in_tab" | "turn_to_player", 3) => vec!["true".into(), "false".into()],
                ("visibility", 3) => vec!["all".into(), "permission_required".into(), "manual".into()],
                ("visibility_distance", 3) => vec!["default".into(), "not_visible".into(), "always_visible".into()],
                ("interaction_cooldown", 3) => vec!["disabled".into(), "5s".into(), "1m".into()],
                ("move_to", 3..=5) => vec!["~".into()],
                ("move_to", 6) => horizon_bukkit::get_worlds().iter().map(|w| w.get_name().to_owned()).collect(),
                ("create", _) => vec!["--type".into(), "--location".into(), "--world".into()],
                _ => Vec::new(),
            }
        }
    };
    out.into_iter().filter(|o| o.to_ascii_lowercase().starts_with(&last)).collect()
}
