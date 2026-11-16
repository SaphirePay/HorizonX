//! LuckPerms for Horizon, in Rust: LuckPerms' commands, its JSON storage (the files LuckPerms itself writes and
//! reads), and its rules for working out a player's permissions (inheritance, weights, contexts, temporary
//! nodes, wildcards). The other plugins' checks never reach this plugin: each player's worked-out permissions
//! are handed to the engine's table when they join or when something changes, and checks read that table.
//! A join costs one file read and one pass over the player's nodes, on the connection's own thread first.
pub mod commands;
pub mod model;

use horizon_bukkit::event::{PlayerJoinEvent, PlayerLoginEvent, PlayerQuitEvent};
use horizon_bukkit::fam_ev_player::{AsyncPlayerPreLoginEvent, PlayerChangedWorldEvent, PlayerGameModeChangeEvent};
use horizon_bukkit::{Player, Plugin, bukkit_plugin};
use model::{Context, Store};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

pub(crate) struct State {
    pub store: Store,
    /// Every permission the server's plugins declared, lower case: what wildcards spread over.
    pub known: Vec<String>,
    /// What each online player was last handed (by dashed uuid): only the difference goes to the engine.
    pub applied: HashMap<String, BTreeMap<String, bool>>,
    /// Each online player's chat meta, worked out with their permissions: what Vault's `Chat` service answers
    /// (no game query and no inheritance walk per chat line or tab refresh).
    pub meta: HashMap<String, Meta>,
    /// Online names (lower case) -> dashed uuid, for callers that ask by name.
    pub names: HashMap<String, String>,
}

#[derive(Clone, Default)]
pub(crate) struct Meta {
    pub prefix: String,
    pub suffix: String,
    pub primary: String,
    pub groups: Vec<String>,
    pub prefixes: Vec<String>,
    pub suffixes: Vec<String>,
    /// `meta.KEY.VALUE` nodes, key lower case.
    pub values: BTreeMap<String, String>,
}

/// Vault's `net.milkbowl.vault.chat.Chat` (and LuckPerms' own prefix lookups) for every other plugin: by player
/// name or uuid in `args[0]`. Read from the cache `apply` keeps, so a call is a map lookup.
fn provide_chat(plugin: &Plugin) {
    use serde_json::{Value, json};
    let answer = |method: &str, args: &Value| -> Value {
        let who = args[0].as_str().unwrap_or_default();
        let meta = with(|s| {
            let uuid = if who.len() >= 32 { model::dashed(who) } else { s.names.get(&who.to_ascii_lowercase()).cloned().unwrap_or_default() };
            s.meta.get(&uuid).cloned()
        })
        .flatten()
        .unwrap_or_default();
        match method {
            "isEnabled" => json!(true),
            "getName" => json!("LuckPerms"),
            "getPlayerPrefix" => json!(meta.prefix),
            "getPlayerSuffix" => json!(meta.suffix),
            "getPrimaryGroup" => json!(if meta.primary.is_empty() { "default".to_owned() } else { meta.primary }),
            "getPlayerGroups" => json!(meta.groups),
            "playerInGroup" => json!(meta.groups.iter().any(|g| g.eq_ignore_ascii_case(args[1].as_str().unwrap_or_default()))),
            "getGroupPrefix" | "getGroupSuffix" => {
                let kind = if method == "getGroupPrefix" { "prefix" } else { "suffix" };
                let group = args[0].as_str().unwrap_or_default().to_ascii_lowercase();
                let text = with(|s| {
                    s.store.groups.get(&group).and_then(|g| {
                        g.nodes.iter().filter(|n| n.value).filter_map(|n| n.chat_meta(kind)).max_by_key(|(w, _)| *w).map(|(_, t)| t.to_owned())
                    })
                })
                .flatten()
                .unwrap_or_default();
                json!(text)
            }
            "getGroups" => json!(with(|s| s.store.groups.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()),
            // LuckPerms' CachedMetaData: getPrefixes/getSuffixes (highest first), getMetaValue(key).
            "getPlayerPrefixes" => json!(meta.prefixes),
            "getPlayerSuffixes" => json!(meta.suffixes),
            "getPlayerInfoString" | "getMetaValue" => {
                let key = args[1].as_str().unwrap_or_default().to_ascii_lowercase();
                meta.values.get(&key).map_or(Value::Null, |v| json!(v))
            }
            "getPlayerMeta" => json!(meta.values),
            // Tracks: `trackContains(track, group)`, `getTracks()`, `getTrackGroups(track)`.
            "trackContains" => {
                let (track, group) = (args[0].as_str().unwrap_or_default().to_ascii_lowercase(), args[1].as_str().unwrap_or_default().to_ascii_lowercase());
                json!(with(|s| s.store.tracks.get(&track).is_some_and(|t| t.groups.iter().any(|g| *g == group))).unwrap_or(false))
            }
            "getTracks" => json!(with(|s| s.store.tracks.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()),
            "getTrackGroups" => {
                let track = args[0].as_str().unwrap_or_default().to_ascii_lowercase();
                json!(with(|s| s.store.tracks.get(&track).map(|t| t.groups.clone()).unwrap_or_default()).unwrap_or_default())
            }
            "has" | "playerHas" => {
                // Permission check from the resolved table (no game query).
                let node = args[1].as_str().unwrap_or_default().to_ascii_lowercase();
                let uuid = with(|s| if who.len() >= 32 { model::dashed(who) } else { s.names.get(&who.to_ascii_lowercase()).cloned().unwrap_or_default() }).unwrap_or_default();
                json!(with(|s| s.applied.get(&uuid).and_then(|m| m.get(&node).copied()).unwrap_or(false)).unwrap_or(false))
            }
            _ => Value::Null,
        }
    };
    horizon_bukkit::persistence::provide_service(plugin, "net.milkbowl.vault.chat.Chat", "Highest", answer);
    horizon_bukkit::persistence::provide_service(plugin, "net.milkbowl.vault.permission.Permission", "Highest", answer);
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static PLUGIN: OnceLock<Plugin> = OnceLock::new();

pub(crate) fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_mut().map(f)
}

fn known_permissions() -> Vec<String> {
    let mut all: Vec<String> = horizon_bukkit::fam_core::get_plugin_manager()
        .get_permissions()
        .into_iter()
        .map(|p| p.name.to_ascii_lowercase())
        .collect();
    all.sort();
    all.dedup();
    all
}

pub(crate) fn context_of(player: &Player, server: &str) -> Context {
    let world = player.get_world().get_name().to_ascii_lowercase();
    let dimension = if world.ends_with("_nether") {
        "the_nether"
    } else if world.ends_with("_the_end") {
        "the_end"
    } else {
        "overworld"
    }
    .to_owned();
    Context { server: server.to_owned(), world, gamemode: player.get_game_mode().to_ascii_lowercase(), dimension }
}

/// The player's permissions worked out again and the engine told what changed.
pub(crate) fn apply(player: &Player) {
    let Some(plugin) = PLUGIN.get() else { return };
    let uuid = model::dashed(&player.get_unique_id());
    let (wanted, before) = with(|s| {
        let ctx = context_of(player, &s.store.server.clone());
        let user = s.store.load_user(&uuid, player.get_name()).clone();
        let resolved = s.store.resolve(&user, &ctx);
        let (prefixes, suffixes, values) = s.store.all_meta(&user, &ctx);
        let meta = Meta {
            prefix: prefixes.first().cloned().unwrap_or_default(),
            suffix: suffixes.first().cloned().unwrap_or_default(),
            primary: user.primary_group.clone(),
            groups: resolved.keys().filter_map(|k| k.strip_prefix("group.")).map(str::to_owned).collect(),
            prefixes,
            suffixes,
            values,
        };
        s.meta.insert(uuid.clone(), meta);
        s.names.insert(player.get_name().to_ascii_lowercase(), uuid.clone());
        let wanted = model::expand(&resolved, &s.known);
        let before = s.applied.insert(uuid.clone(), wanted.clone()).unwrap_or_default();
        (wanted, before)
    })
    .unwrap_or_default();
    if wanted == before {
        return;
    }
    let attachment = player.add_attachment(plugin);
    for node in before.keys().filter(|k| !wanted.contains_key(*k)) {
        attachment.unset_permission(node);
    }
    for (node, &value) in &wanted {
        if before.get(node) != Some(&value) {
            attachment.set_permission(node, value);
        }
    }
}

/// Every online player's permissions again (a group changed, a reload).
pub(crate) fn apply_all() {
    for player in horizon_bukkit::get_online_players() {
        apply(&player);
    }
}

/// The online player with this uuid, if any.
pub(crate) fn online(uuid: &str) -> Option<Player> {
    let uuid = model::dashed(uuid);
    horizon_bukkit::get_online_players().into_iter().find(|p| model::dashed(&p.get_unique_id()) == uuid)
}

fn forget(player: &Player) {
    let Some(plugin) = PLUGIN.get() else { return };
    let uuid = model::dashed(&player.get_unique_id());
    let before = with(|s| {
        s.store.unload_user(&uuid);
        s.meta.remove(&uuid);
        s.names.retain(|_, u| *u != uuid);
        s.applied.remove(&uuid).unwrap_or_default()
    })
    .unwrap_or_default();
    let attachment = player.add_attachment(plugin);
    for node in before.keys() {
        attachment.unset_permission(node);
    }
}

fn on_enable(plugin: Plugin) {
    let _ = PLUGIN.set(plugin);
    let folder = plugin.get_data_folder();
    let _ = std::fs::create_dir_all(&folder);
    let config = folder.join("config.yml");
    if !config.exists() {
        let _ = std::fs::write(&config, "# LuckPerms (Horizon, Rust). The storage is LuckPerms' own JSON storage.\nserver: global\nstorage-method: json\n");
    }
    let server = std::fs::read_to_string(&config)
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("server:").map(|v| v.trim().trim_matches('\'').trim_matches('"').to_owned())))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "global".into());
    let started = std::time::Instant::now();
    let store = Store::open(&folder, &server);
    let (groups, tracks) = (store.groups.len(), store.tracks.len());
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(State { store, known: known_permissions(), applied: HashMap::new(), meta: HashMap::new(), names: HashMap::new() });
    provide_chat(&plugin);
    horizon_bukkit::log(&format!(
        "[LuckPerms] Loaded {groups} groups and {tracks} tracks from the JSON storage in {} ms (Rust)",
        started.elapsed().as_millis()
    ));

    // The user's file is read on the connection's own thread, as LuckPerms does (`AsyncPlayerPreLoginEvent`):
    // the join itself only works out their permissions.
    plugin.register_event_with("LOWEST", false, |event: &mut AsyncPlayerPreLoginEvent| {
        let (uuid, name) = (event.get_unique_id(), event.get_name().to_owned());
        with(|s| {
            s.store.load_user(&uuid, &name);
        });
    });
    // Before every other plugin hears the login and the join: their checks see the permissions already.
    plugin.register_event_with("LOWEST", false, |event: &mut PlayerLoginEvent| apply(event.get_player()));
    plugin.register_event_with("LOWEST", false, |event: &mut PlayerJoinEvent| apply(event.get_player()));
    plugin.register_event_with("MONITOR", false, |event: &mut PlayerQuitEvent| forget(event.get_player()));
    plugin.register_event_with("MONITOR", false, |event: &mut PlayerChangedWorldEvent| {
        if let Some(player) = event.get_player() {
            apply(&player);
        }
    });
    plugin.register_event_with("MONITOR", true, |event: &mut PlayerGameModeChangeEvent| {
        let Some(player) = event.get_player().clone() else { return };
        // The new mode holds from the next tick on.
        if let Some(plugin) = PLUGIN.get() {
            plugin.run_task(move || apply(&player));
        }
    });

    // Temporary nodes end on time; plugins enabled later add their permissions to what wildcards reach.
    plugin.run_task_timer(20, 20, || {
        let changed = with(|s| {
            let (users, groups) = s.store.drop_expired();
            (users, !groups.is_empty())
        });
        if let Some((users, groups)) = changed {
            if groups {
                apply_all();
            } else {
                for u in users {
                    if let Some(p) = online(&u) {
                        apply(&p);
                    }
                }
            }
        }
    });
    plugin.run_task_timer(200, 600, || {
        let known = known_permissions();
        let moved = with(|s| {
            let moved = s.known != known;
            s.known = known;
            moved
        });
        if moved == Some(true) {
            apply_all();
        }
    });

    plugin.set_executor("luckperms", |sender, _label, args| {
        commands::run(sender, args);
        true
    });
    plugin.set_tab_completer("luckperms", |_sender, _label, args| commands::complete(args));
    // Everyone already online (a reload) gets theirs now.
    apply_all();
}

fn on_disable(_plugin: Plugin) {
    for player in horizon_bukkit::get_online_players() {
        forget(&player);
    }
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
