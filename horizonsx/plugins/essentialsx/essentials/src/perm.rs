//! `PermissionsHandler`, with the two handlers that need no other plugin: `SuperpermsHandler` when
//! `use-bukkit-permissions` is on, `ConfigPermissionsHandler` otherwise. LuckPerms and Vault aren't here.
use crate::prelude::*;
use std::collections::HashMap;
use std::sync::OnceLock;

/// `plugin.yml`'s `default:` of each node that has one.
static DEFAULTS: OnceLock<HashMap<String, String>> = OnceLock::new();

fn defaults() -> &'static HashMap<String, String> {
    DEFAULTS.get_or_init(|| {
        let mut map = HashMap::new();
        if let Some(list) = ess().plugin().get_permissions().as_object() {
            for (node, value) in list {
                map.insert(node.to_lowercase(), horizon_bukkit::config::scalar(&value["default"]).unwrap_or_else(|| "op".to_owned()).to_lowercase());
            }
        }
        map
    })
}

/// `children:` in `plugin.yml`, from the child: Bukkit gives a node's children with the node.
static PARENTS: OnceLock<HashMap<String, Vec<(String, bool)>>> = OnceLock::new();

fn parents() -> &'static HashMap<String, Vec<(String, bool)>> {
    PARENTS.get_or_init(|| {
        let mut map: HashMap<String, Vec<(String, bool)>> = HashMap::new();
        if let Some(list) = ess().plugin().get_permissions().as_object() {
            for (node, value) in list {
                for (child, on) in value["children"].as_object().into_iter().flatten() {
                    map.entry(child.to_lowercase()).or_default().push((node.to_lowercase(), on.as_bool().unwrap_or(true)));
                }
            }
        }
        map
    })
}

/// What a parent says of `node`, if the player holds one. The tree is only a few levels deep.
fn from_parent(player: &Player, node: &str, depth: u32) -> Option<bool> {
    if depth > 8 {
        return None;
    }
    parents().get(&node.to_lowercase())?.iter().find_map(|(parent, on)| (player.has_permission(parent) || from_parent(player, parent, depth + 1) == Some(true)).then_some(*on))
}

/// `isDeniedToOps`: `default: false` in `plugin.yml`.
fn is_denied_to_ops(node: &str) -> bool {
    matches!(defaults().get(&node.to_lowercase()).map(String::as_str), Some("false" | "not op"))
}

/// Bukkit's `isPermissionSet`: a grant, or a default that gives it to this player.
fn is_set(player: &Player, node: &str) -> bool {
    if player.is_permission_set(node) {
        return true;
    }
    match defaults().get(&node.to_lowercase()).map(String::as_str) {
        Some("true") => true,
        Some("op") => player.is_op(),
        Some("not op") => !player.is_op(),
        _ => false,
    }
}

/// `SuperpermsHandler.hasPermission`: `a.b.c`, then `a.b.*`, `a.*` and `*`.
fn superperms_has_permission(player: &Player, node: &str) -> bool {
    let mut node = node.to_owned();
    let mut check = node.clone();
    loop {
        if is_set(player, &check) || is_denied_to_ops(&node) {
            return player.has_permission(&check);
        }
        match node.rfind('.') {
            Some(index) if index >= 1 => {
                node.truncate(index);
                check = format!("{node}.*");
            }
            _ => return player.has_permission("*"),
        }
    }
}

/// `hasPermission`.
pub fn has_permission(player: &Player, node: &str) -> bool {
    if !settings().use_bukkit_permissions() {
        // `ConfigPermissionsHandler`: `player-commands` count for everyone.
        let command = node.split_once('.').map_or(node, |(_, rest)| rest);
        if settings().is_player_command(command) {
            return true;
        }
    }
    if let Some(on) = from_parent(player, node, 0).filter(|_| !player.is_permission_set(node)) {
        return on;
    }
    superperms_has_permission(player, node)
}

/// `isPermissionSet`.
pub fn is_permission_set(player: &Player, node: &str) -> bool {
    is_set(player, node)
}

/// `isPermissionSetExact`: `None` is `TriState.UNSET`.
pub fn is_permission_set_exact(player: &Player, node: &str) -> Option<bool> {
    is_set(player, node).then(|| player.has_permission(node))
}

/// `getGroup`: no group plugin here.
pub fn get_group(_player: &Player) -> String {
    "default".to_owned()
}

/// `getGroups`.
pub fn get_groups(_player: &Player) -> Vec<String> {
    vec!["default".to_owned()]
}

/// `inGroup`.
pub fn in_group(player: &Player, group: &str) -> bool {
    has_permission(player, &format!("group.{group}"))
}

/// `canBuild`.
pub fn can_build(_player: &Player) -> bool {
    !settings().use_bukkit_permissions()
}

/// `getPrefix`.
pub fn get_prefix(_player: &Player) -> String {
    String::new()
}

/// `getSuffix`.
pub fn get_suffix(_player: &Player) -> String {
    String::new()
}

/// `getName`, as `/list` and `PlayerList` compare it.
pub fn get_name() -> &'static str {
    if settings().use_bukkit_permissions() { "SuperpermsHandler" } else { "ConfigPermissions" }
}
