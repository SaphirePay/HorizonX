//! The menus as `DeluxeMenusConfig` reads them: config.yml's `gui_menus`, each from its own file in
//! `gui_menus/` (or, deprecated, inline in config.yml). Parsing is pure data (no game queries), so a reload reads
//! and parses on a background thread and swaps the result in on the server thread.

use crate::yaml::{self, at};
use serde_yaml::Value as Yaml;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

/// `ActionType`: the tag that starts an action line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Meta,
    Console,
    Player,
    PlayerCommandEvent,
    MiniMessage,
    MiniBroadcast,
    Message,
    Broadcast,
    Chat,
    OpenGuiMenu,
    OpenMenu,
    Connect,
    Close,
    Refresh,
    BroadcastSound,
    BroadcastWorldSound,
    PlaySound,
    TakeMoney,
    GiveMoney,
    TakeExp,
    GiveExp,
    TakePerm,
    GivePerm,
    JsonMessage,
    JsonBroadcast,
    BroadcastJson,
    Placeholder,
}

pub const ACTIONS: [(&str, ActionKind); 27] = [
    ("[meta]", ActionKind::Meta),
    ("[console]", ActionKind::Console),
    ("[player]", ActionKind::Player),
    ("[commandevent]", ActionKind::PlayerCommandEvent),
    ("[minimessage]", ActionKind::MiniMessage),
    ("[minibroadcast]", ActionKind::MiniBroadcast),
    ("[message]", ActionKind::Message),
    ("[broadcast]", ActionKind::Broadcast),
    ("[chat]", ActionKind::Chat),
    ("[openguimenu]", ActionKind::OpenGuiMenu),
    ("[openmenu]", ActionKind::OpenMenu),
    ("[connect]", ActionKind::Connect),
    ("[close]", ActionKind::Close),
    ("[refresh]", ActionKind::Refresh),
    ("[broadcastsoundworld]", ActionKind::BroadcastWorldSound),
    ("[broadcastsound]", ActionKind::BroadcastSound),
    ("[sound]", ActionKind::PlaySound),
    ("[takemoney]", ActionKind::TakeMoney),
    ("[givemoney]", ActionKind::GiveMoney),
    ("[takeexp]", ActionKind::TakeExp),
    ("[giveexp]", ActionKind::GiveExp),
    ("[takepermission]", ActionKind::TakePerm),
    ("[givepermission]", ActionKind::GivePerm),
    ("[jsonbroadcast]", ActionKind::JsonBroadcast),
    ("[broadcastjson]", ActionKind::BroadcastJson),
    ("[json]", ActionKind::JsonMessage),
    ("[placeholder]", ActionKind::Placeholder),
];

/// One `ClickAction`: its kind, the text after the tag, and its `<delay=..>` / `<chance=..>`.
#[derive(Clone, Debug)]
pub struct Action {
    pub kind: ActionKind,
    pub exec: String,
    pub delay: Option<String>,
    pub chance: Option<String>,
}

impl Action {
    /// `ActionType.getByStart` + the delay and chance tags taken out, as `getClickHandler` does.
    pub fn parse(line: &str) -> Option<Action> {
        let lower = line.to_ascii_lowercase();
        let (tag, kind) = ACTIONS.iter().find(|(tag, _)| lower.starts_with(tag))?;
        let mut command = line[tag.len()..].to_owned();
        if command.starts_with(' ') {
            command = command.trim().to_owned();
        }
        let delay = take_tag(&mut command, "delay");
        let chance = take_tag(&mut command, "chance");
        Some(Action { kind: *kind, exec: command, delay, chance })
    }
}

/// `<name=value>` (any case) taken out of the text, its value given back.
fn take_tag(text: &mut String, name: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find(&format!("<{name}="))?;
    let rest = &text[start + name.len() + 2..];
    let end = rest.find('>')?;
    let value = rest[..end].to_owned();
    if value.contains('<') {
        return None;
    }
    text.replace_range(start..start + name.len() + 3 + end, "");
    Some(value)
}

pub fn actions(c: &Yaml, path: &str) -> Vec<Action> {
    yaml::list(c, path).iter().filter(|l| !l.is_empty()).filter_map(|l| Action::parse(l)).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Compare {
    Contains,
    Equals,
    EqualsIgnoreCase,
    Greater,
    GreaterEq,
    Eq,
    NotEq,
    LessEq,
    Less,
}

/// `has item`'s options (`ItemWrapper`).
#[derive(Clone, Debug, Default)]
pub struct ItemCheck {
    pub material: String,
    pub amount: i64,
    pub data: Option<i64>,
    pub name: Option<String>,
    pub lore: Option<String>,
    pub lore_list: Option<Vec<String>>,
    pub strict: bool,
    pub armor: bool,
    pub offhand: bool,
    pub model_data: i64,
    pub name_contains: bool,
    pub name_ignorecase: bool,
    pub lore_contains: bool,
    pub lore_ignorecase: bool,
}

#[derive(Clone, Debug)]
pub enum Check {
    Permission(String),
    Permissions(Vec<String>, i64),
    Money { amount: f64, placeholder: Option<String> },
    Exp { amount: String, level: bool },
    Item(ItemCheck),
    Meta { key: String, kind: String, value: String },
    Near { location: Option<(String, f64, f64, f64)>, distance: f64 },
    Compare { kind: Compare, input: String, output: String },
    Regex { regex: regex::Regex, input: String },
    Length { input: String, min: i64, max: Option<i64> },
    IsObject { input: String, object: String },
    /// `javascript`: no script engine here; it answers false (and says so once at load).
    Javascript(String),
}

#[derive(Clone, Debug)]
pub struct Requirement {
    pub check: Check,
    pub invert: bool,
    pub optional: bool,
    pub success: Vec<Action>,
    pub deny: Vec<Action>,
}

#[derive(Clone, Debug)]
pub struct RequirementList {
    pub list: Vec<Requirement>,
    pub deny: Vec<Action>,
    pub minimum: usize,
    pub stop_at_success: bool,
}

/// `RequirementType`: its names and whether it is the inverted one.
fn requirement_kind(name: &str) -> Option<(&'static str, bool)> {
    const TYPES: [(&str, &[&str]); 30] = [
        ("meta", &["has meta", "meta"]),
        ("!meta", &["!has meta", "!meta"]),
        ("near", &["is near", "near"]),
        ("!near", &["!is near", "!near"]),
        ("js", &["javascript", "js"]),
        ("item", &["has item", "item", "hasitem"]),
        ("!item", &["!has item", "!item", "!hasitem", "does not have item"]),
        ("money", &["has money", "hasmoney", "money"]),
        ("!money", &["!has money", "!hasmoney", "!money"]),
        ("exp", &["has exp", "hasexp", "exp"]),
        ("!exp", &["!has exp", "!hasexp", "!exp"]),
        ("perm", &["has permission", "has perm", "haspermission", "hasperm", "perm"]),
        ("!perm", &["!has permission", "!has perm", "!haspermission", "!hasperm", "!perm"]),
        ("perms", &["has permissions", "has perms", "haspermissions", "hasperms", "perms"]),
        ("!perms", &["!has permissions", "!has perms", "!haspermissions", "!hasperms", "!perms"]),
        ("contains", &["string contains", "stringcontains", "contains"]),
        ("!contains", &["!string contains", "!stringcontains", "!contains"]),
        ("equals", &["string equals", "stringequals", "equals"]),
        ("!equals", &["!string equals", "!stringequals", "!equals"]),
        ("equalsic", &["stringequalsignorecase", "string equals ignorecase", "equalsignorecase"]),
        ("!equalsic", &["!stringequalsignorecase", "!string equals ignorecase", "!equalsignorecase"]),
        (">", &[">", "greater than", "greaterthan"]),
        (">=", &[">=", "greater than or equal to", "greaterthanorequalto"]),
        ("==", &["==", "equal to", "equalto"]),
        ("!=", &["!=", "not equal to", "notequalto"]),
        ("<=", &["<=", "less than or equal to", "lessthanorequalto"]),
        ("<", &["<", "less than", "lessthan"]),
        ("regex", &["regex matches", "regex"]),
        ("!regex", &["!regex matches", "!regex"]),
        ("length", &["string length"]),
    ];
    if name.eq_ignore_ascii_case("is object") {
        return Some(("object", false));
    }
    let (id, _) = TYPES.iter().find(|(_, names)| names.iter().any(|n| n.eq_ignore_ascii_case(name)))?;
    if *id == "!=" {
        return Some(("!=", false));
    }
    Some(match id.strip_prefix('!') {
        Some(base) => (base, true),
        None => (id, false),
    })
}

/// `getRequirements(c, path)`: the list under `path.requirements`, or `None`.
pub fn requirements(c: &Yaml, path: &str, menu: &str, warn: &mut Vec<String>) -> Option<RequirementList> {
    let base = format!("{path}.requirements");
    if !yaml::contains(c, &base) {
        return None;
    }
    let mut list = Vec::new();
    for key in yaml::keys(c, &base) {
        let r = format!("{base}.{key}");
        let Some(type_name) = yaml::string(c, &format!("{r}.type")) else {
            warn.push(format!("No type set for requirement: {key} for path: {r}"));
            continue;
        };
        let Some((id, invert)) = requirement_kind(&type_name) else {
            warn.push(format!("Requirement type '{type_name}' at path '{r}' is not valid!"));
            continue;
        };
        let s = |k: &str| yaml::string(c, &format!("{r}.{k}"));
        let has = |k: &str| yaml::contains(c, &format!("{r}.{k}"));
        let check = match id {
            "perm" => s("permission").map(Check::Permission),
            "perms" => {
                let perms = yaml::list(c, &format!("{r}.permissions"));
                if perms.is_empty() {
                    None
                } else {
                    let mut min = if has("minimum") { yaml::int(c, &format!("{r}.minimum"), -1) } else { -1 };
                    if min < 1 {
                        min = -1;
                    }
                    min = min.min(perms.len() as i64);
                    Some(Check::Permissions(perms, min))
                }
            }
            "money" => (has("amount") || has("placeholder")).then(|| Check::Money { amount: yaml::num(c, &format!("{r}.amount"), 0.0), placeholder: s("placeholder") }),
            "exp" => s("amount").map(|amount| Check::Exp { amount, level: yaml::boolean(c, &format!("{r}.level"), false) }),
            "item" => s("material").map(|material| {
                let b = |k: &str, alt: &str| if has(k) { yaml::boolean(c, &format!("{r}.{k}"), false) } else { yaml::boolean(c, &format!("{r}.{alt}"), false) };
                Check::Item(ItemCheck {
                    material,
                    amount: yaml::int(c, &format!("{r}.amount"), 1),
                    data: yaml::is_int(c, &format!("{r}.data")).then(|| yaml::int(c, &format!("{r}.data"), 0)),
                    name: yaml::is_string(c, &format!("{r}.name")).then(|| s("name")).flatten(),
                    lore: yaml::is_string(c, &format!("{r}.lore")).then(|| s("lore")).flatten(),
                    lore_list: yaml::is_list(c, &format!("{r}.lore")).then(|| yaml::list(c, &format!("{r}.lore"))),
                    strict: yaml::boolean(c, &format!("{r}.strict"), false),
                    armor: yaml::boolean(c, &format!("{r}.armor"), false),
                    offhand: yaml::boolean(c, &format!("{r}.offhand"), false),
                    model_data: if yaml::is_int(c, &format!("{r}.model_data")) { yaml::int(c, &format!("{r}.model_data"), 0) } else { yaml::int(c, &format!("{r}.modeldata"), 0) },
                    name_contains: b("name_contains", "name-contains"),
                    name_ignorecase: b("name_ignorecase", "name-ignorecase"),
                    lore_contains: b("lore_contains", "lore-contains"),
                    lore_ignorecase: b("lore_ignorecase", "lore-ignorecase"),
                })
            }),
            "meta" => match (s("key"), s("meta_type"), s("value")) {
                (Some(key), Some(kind), Some(value)) => Some(Check::Meta { key, kind: kind.to_ascii_uppercase(), value }),
                _ => None,
            },
            "near" => match (s("location"), has("distance")) {
                (Some(loc), true) => {
                    let parts: Vec<&str> = loc.split(',').map(str::trim).collect();
                    let location = (parts.len() == 4).then(|| (parts[0].to_owned(), parts[1].parse().unwrap_or(0.0), parts[2].parse().unwrap_or(0.0), parts[3].parse().unwrap_or(0.0)));
                    if location.is_none() {
                        warn.push(format!("requirement at path: {r} has an invalid location. Valid Format is: <world>,<x>,<y>,<z>"));
                    }
                    Some(Check::Near { location, distance: yaml::num(c, &format!("{r}.distance"), 0.0) })
                }
                _ => None,
            },
            "js" => s("expression").map(|e| {
                warn.push(format!("Menu {menu}: javascript requirement at {r} is not supported in the Rust DeluxeMenus; it will fail."));
                Check::Javascript(e)
            }),
            "regex" => match (s("input"), s("regex")) {
                (Some(input), Some(re)) => match regex::Regex::new(&re) {
                    Ok(regex) => Some(Check::Regex { regex, input }),
                    Err(e) => {
                        warn.push(format!("Invalid regex at {r}: {e}"));
                        None
                    }
                },
                _ => None,
            },
            "length" => s("input").filter(|_| has("min") || has("max")).map(|input| Check::Length { input, min: yaml::int(c, &format!("{r}.min"), 0), max: has("max").then(|| yaml::int(c, &format!("{r}.max"), 0)) }),
            "object" => match (s("input"), s("object")) {
                (Some(input), Some(object)) => Some(Check::IsObject { input, object }),
                _ => None,
            },
            cmp => match (s("input"), s("output")) {
                (Some(input), Some(output)) => {
                    let kind = match cmp {
                        "contains" => Compare::Contains,
                        "equals" => Compare::Equals,
                        "equalsic" => Compare::EqualsIgnoreCase,
                        ">" => Compare::Greater,
                        ">=" => Compare::GreaterEq,
                        "==" => Compare::Eq,
                        "!=" => Compare::NotEq,
                        "<=" => Compare::LessEq,
                        _ => Compare::Less,
                    };
                    Some(Check::Compare { kind, input, output })
                }
                _ => None,
            },
        };
        let Some(check) = check else {
            warn.push(format!("Requirement at path: {r} is missing its options; skipped."));
            continue;
        };
        list.push(Requirement {
            check,
            invert,
            optional: yaml::boolean(c, &format!("{r}.optional"), false),
            success: actions(c, &format!("{r}.success_commands")),
            deny: actions(c, &format!("{r}.deny_commands")),
        });
    }
    if list.is_empty() {
        return None;
    }
    // Java counts the non-optional ones with `required = required++`, which leaves 0: kept as it behaves.
    let minimum = if yaml::contains(c, &format!("{path}.minimum_requirements")) { yaml::int(c, &format!("{path}.minimum_requirements"), 0).max(0) as usize } else { 0 };
    Some(RequirementList { list, deny: actions(c, &format!("{path}.deny_commands")), minimum, stop_at_success: yaml::boolean(c, &format!("{path}.stop_at_success"), false) })
}

/// A click kind's actions and requirements.
#[derive(Clone, Debug, Default)]
pub struct Click {
    pub actions: Vec<Action>,
    pub requirements: Option<RequirementList>,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub key: String,
    pub material: String,
    pub slot: usize,
    pub priority: i64,
    pub amount: i64,
    pub dynamic_amount: Option<String>,
    pub model_data: Option<String>,
    pub light_level: Option<String>,
    pub display_name: Option<String>,
    pub lore: Vec<String>,
    pub has_lore: bool,
    pub lore_append: Option<String>,
    pub rgb: Option<String>,
    pub base_color: Option<String>,
    pub damage: Option<String>,
    pub unbreakable: bool,
    pub update: bool,
    pub hide_attributes: bool,
    pub hide_unbreakable: bool,
    pub hide_enchants: bool,
    pub item_flags: Vec<String>,
    pub hide_tooltip: Option<String>,
    pub glint: Option<String>,
    pub rarity: Option<String>,
    pub tooltip_style: Option<String>,
    pub item_model: Option<String>,
    pub trim: Option<(String, String)>,
    pub banner: Vec<(String, String)>,
    pub potions: Vec<(String, i64, i64)>,
    pub enchantments: Vec<(String, u32)>,
    pub nbt: Vec<(char, String)>,
    pub model_component: Option<(Vec<String>, Vec<String>, Vec<String>, Vec<String>)>,
    pub view: Option<RequirementList>,
    pub click: Click,
    pub left: Click,
    pub right: Click,
    pub shift_left: Click,
    pub shift_right: Click,
    pub middle: Click,
}

impl Item {
    pub fn name_has_placeholders(&self) -> bool {
        self.display_name.as_deref().is_some_and(has_placeholders)
    }
    pub fn lore_has_placeholders(&self) -> bool {
        self.lore.iter().any(|l| has_placeholders(l))
    }
}

/// `%id_params%` or `{arg}` in the text.
pub fn has_placeholders(text: &str) -> bool {
    text.contains('%') || text.contains('{')
}

#[derive(Debug)]
pub struct Menu {
    pub name: String,
    pub title: String,
    pub kind: String,
    pub size: usize,
    pub commands: Vec<String>,
    pub register: bool,
    pub args: Vec<String>,
    pub arg_requirements: Vec<RequirementList>,
    pub args_usage: Option<String>,
    pub open_requirement: Option<RequirementList>,
    pub open_actions: Vec<Action>,
    pub close_actions: Vec<Action>,
    pub update_interval: u64,
    /// slot -> items by priority (lowest number first), as Java's `TreeMap`.
    pub items: BTreeMap<usize, Vec<Arc<Item>>>,
    pub parse_in_args: bool,
    pub parse_after_args: bool,
    /// The file it came from, relative to gui_menus (`config` when inline).
    pub path: String,
}

const VALID_TYPES: [&str; 10] = ["CHEST", "ANVIL", "BARREL", "BEACON", "BLAST_FURNACE", "BREWING", "CARTOGRAPHY", "DISPENSER", "DROPPER", "ENCHANTING"];
const MORE_TYPES: [&str; 9] = ["ENDER_CHEST", "FURNACE", "GRINDSTONE", "HOPPER", "LOOM", "SHULKER_BOX", "SMITHING", "SMOKER", "WORKBENCH"];

fn type_size(kind: &str) -> usize {
    match kind {
        "HOPPER" | "BREWING" => 5,
        "DISPENSER" | "DROPPER" => 9,
        "WORKBENCH" => 10,
        "FURNACE" | "BLAST_FURNACE" | "SMOKER" | "ANVIL" | "GRINDSTONE" | "MERCHANT" => 3,
        "ENCHANTING" | "CARTOGRAPHY" => 2,
        "BEACON" => 1,
        "LOOM" | "SMITHING" => 4,
        _ => 27,
    }
}

/// `isValidMaterial`: a material name, a player item, a head/hook prefix or a placeholder.
fn valid_material(lower: &str) -> bool {
    const PREFIXES: [&str; 15] = [
        "placeholder-", "head-", "basehead-", "texture-", "hdb-", "itemsadder-", "oraxen-", "nexo-", "mmoitems-", "executableitems-", "executableblocks-", "simpleitemgenerator-", "main_hand", "off_hand", "armor_",
    ];
    lower == "water_bottle" || PREFIXES.iter().any(|p| lower.starts_with(p)) || horizon_bukkit::enums::material::value_of(&lower.to_ascii_uppercase()).is_some()
}

/// `loadMenu`: one menu from its file (`pre` empty) or config.yml (`pre` = `gui_menus.<key>.`).
pub fn load_menu(c: &Yaml, key: &str, pre: &str, path: &str, taken: &[String], warn: &mut Vec<String>) -> Option<Menu> {
    let p = |k: &str| format!("{pre}{k}");
    let title = match at(c, &p("menu_title")) {
        Some(Yaml::Sequence(l)) => l.first().and_then(yaml::scalar),
        Some(v) => yaml::scalar(v),
        None => {
            warn.push(format!("Menu title for menu: {key} is not present! Skipping menu: {key}"));
            return None;
        }
    }
    .filter(|t| !t.is_empty());
    let Some(title) = title else {
        warn.push(format!("Menu title for menu: {key} is invalid! Skipping menu: {key}"));
        return None;
    };
    let mut kind = "CHEST".to_owned();
    if let Some(t) = yaml::string(c, &p("inventory_type")) {
        let up = t.to_ascii_uppercase();
        if VALID_TYPES.contains(&up.as_str()) || MORE_TYPES.contains(&up.as_str()) {
            kind = up;
        } else {
            warn.push(format!("Inventory type for menu: {key} is invalid! Defaulting to CHEST inventory type."));
        }
    }
    let mut commands = Vec::new();
    match at(c, &p("open_command")) {
        Some(Yaml::Sequence(_)) => {
            for cmd in yaml::list(c, &p("open_command")) {
                if taken.iter().any(|t| t.eq_ignore_ascii_case(&cmd)) {
                    warn.push(format!("command: {cmd} specified for menu: {key} already exists for another menu! Skipping command: {cmd} in menu: {key}"));
                } else {
                    commands.push(cmd.to_ascii_lowercase());
                }
            }
        }
        Some(v) => {
            if let Some(cmd) = yaml::scalar(v).filter(|c| !c.is_empty()) {
                if taken.iter().any(|t| t.eq_ignore_ascii_case(&cmd)) {
                    warn.push(format!("open_command specified for menu: {key} already exists for another menu! Skipping menu: {key}"));
                    return None;
                }
                commands.push(cmd.to_ascii_lowercase());
            }
        }
        None => {}
    }
    let register = !commands.is_empty() && yaml::boolean(c, &p("register_command"), false);
    let mut args = Vec::new();
    let mut arg_requirements = Vec::new();
    match at(c, &p("args")) {
        Some(Yaml::Mapping(_)) => {
            for arg in yaml::keys(c, &p("args")) {
                if yaml::contains(c, &format!("{}.{arg}.requirements", p("args")))
                    && let Some(r) = requirements(c, &format!("{}.{arg}", p("args")), key, warn)
                {
                    arg_requirements.push(r);
                }
                args.push(arg);
            }
        }
        Some(Yaml::Sequence(_)) => args = yaml::list(c, &p("args")),
        Some(v) => args.extend(yaml::scalar(v)),
        None => {}
    }
    let mut size = 54usize;
    if kind == "CHEST" {
        if yaml::contains(c, &p("size")) {
            let mut s = yaml::int(c, &p("size"), 54);
            if (s + 1) % 9 == 0 {
                s += 1;
            }
            if (s - 1) % 9 == 0 {
                s -= 1;
            }
            s = s.clamp(9, 54);
            if s % 9 != 0 {
                warn.push(format!("Menu size for menu: {key} is not a multiple of 9 Defaulting to 54."));
                s = 54;
            }
            size = s as usize;
        }
    } else {
        size = type_size(&kind);
    }
    let open_requirement = requirements(c, &p("open_requirement"), key, warn);
    let update_interval = yaml::int(c, &p("update_interval"), 10);
    let items = load_items(c, key, &p("items"), warn);
    if items.is_empty() {
        warn.push(format!("Failed to load menu items for menu: {key} Skipping menu: {key}"));
        return None;
    }
    Some(Menu {
        name: key.to_owned(),
        title,
        kind,
        size,
        commands,
        register,
        args,
        arg_requirements,
        args_usage: yaml::string(c, &p("args_usage_message")),
        open_requirement,
        open_actions: actions(c, &p("open_commands")),
        close_actions: actions(c, &p("close_commands")),
        update_interval: if update_interval > 0 { update_interval as u64 } else { 10 },
        items,
        parse_in_args: yaml::boolean(c, &p("arguments_support_placeholders"), false),
        parse_after_args: yaml::boolean(c, &p("parse_placeholders_after_arguments"), false),
        path: path.to_owned(),
    })
}

fn click(c: &Yaml, base: &str, kind: &str) -> Click {
    let commands = format!("{base}.{kind}_commands");
    if !yaml::contains(c, &commands) {
        return Click::default();
    }
    Click { actions: actions(c, &commands), requirements: requirements(c, &format!("{base}.{kind}_requirement"), "", &mut Vec::new()) }
}

fn load_items(c: &Yaml, menu: &str, items_path: &str, warn: &mut Vec<String>) -> BTreeMap<usize, Vec<Arc<Item>>> {
    let mut out: BTreeMap<usize, Vec<Arc<Item>>> = BTreeMap::new();
    if !yaml::is_section(c, items_path) {
        return out;
    }
    for key in yaml::keys(c, items_path) {
        let b = format!("{items_path}.{key}");
        let s = |k: &str| yaml::string(c, &format!("{b}.{k}"));
        let Some(material) = s("material") else {
            warn.push(format!("Material for item: {key} in menu: {menu} is not present! Skipping item: {key}"));
            continue;
        };
        if !valid_material(&material.to_ascii_lowercase()) {
            warn.push(format!("Material for item: {key} in menu: {menu} is not valid! Skipping item: {key}"));
            continue;
        }
        let pairs = |k: &str, n: usize| -> Vec<Vec<String>> {
            yaml::list(c, &format!("{b}.{k}")).iter().filter_map(|e| {
                let parts: Vec<String> = e.splitn(n, ';').map(|p| p.trim().to_owned()).collect();
                (parts.len() == n).then_some(parts)
            }).collect()
        };
        let mut nbt = Vec::new();
        for (k, t) in [("nbt_string", 's'), ("nbt_byte", 'b'), ("nbt_short", 'h'), ("nbt_int", 'i')] {
            if let Some(v) = s(k) {
                nbt.push((t, v));
            }
        }
        for (k, t) in [("nbt_strings", 's'), ("nbt_bytes", 'b'), ("nbt_shorts", 'h'), ("nbt_ints", 'i')] {
            nbt.extend(yaml::list(c, &format!("{b}.{k}")).into_iter().map(|v| (t, v)));
        }
        let mcomp = format!("{b}.model_data_component");
        let item = Item {
            key: key.clone(),
            material,
            slot: 0,
            priority: yaml::int(c, &format!("{b}.priority"), 1),
            amount: yaml::int(c, &format!("{b}.amount"), -1),
            dynamic_amount: s("dynamic_amount"),
            model_data: s("model_data"),
            light_level: s("light_level"),
            display_name: s("display_name"),
            lore: yaml::list(c, &format!("{b}.lore")),
            has_lore: yaml::contains(c, &format!("{b}.lore")),
            lore_append: s("lore_append_mode").map(|m| m.to_ascii_uppercase()),
            rgb: s("rgb"),
            base_color: s("base_color").map(|m| m.to_ascii_lowercase()),
            damage: s("damage").or_else(|| s("data")),
            unbreakable: yaml::boolean(c, &format!("{b}.unbreakable"), false),
            update: yaml::boolean(c, &format!("{b}.update"), false),
            hide_attributes: yaml::boolean(c, &format!("{b}.hide_attributes"), false),
            hide_unbreakable: yaml::boolean(c, &format!("{b}.hide_unbreakable"), false),
            hide_enchants: yaml::boolean(c, &format!("{b}.hide_enchantments"), false),
            item_flags: yaml::list_or_one(c, &format!("{b}.item_flags")).iter().map(|f| f.to_ascii_uppercase()).collect(),
            hide_tooltip: s("hide_tooltip"),
            glint: s("enchantment_glint_override"),
            rarity: s("rarity"),
            tooltip_style: s("tooltip_style"),
            item_model: s("item_model"),
            trim: s("trim_material").zip(s("trim_pattern")),
            banner: pairs("banner_meta", 2).into_iter().map(|p| (p[0].to_ascii_lowercase(), p[1].to_ascii_lowercase())).collect(),
            potions: pairs("potion_effects", 3).into_iter().filter_map(|p| Some((p[0].to_ascii_lowercase(), p[1].parse().ok()?, p[2].parse().ok()?))).collect(),
            enchantments: pairs("enchantments", 2).into_iter().filter_map(|p| Some((crate::item::enchantment_key(&p[0]), p[1].parse().ok()?))).collect(),
            nbt,
            model_component: yaml::is_section(c, &mcomp).then(|| {
                let l = |k: &str| yaml::list(c, &format!("{mcomp}.{k}"));
                (l("colors"), l("flags"), l("floats"), l("strings"))
            }),
            view: requirements(c, &format!("{b}.view_requirement"), menu, warn),
            click: click(c, &b, "click"),
            left: click(c, &b, "left_click"),
            right: click(c, &b, "right_click"),
            shift_left: click(c, &b, "shift_left_click"),
            shift_right: click(c, &b, "shift_right_click"),
            middle: click(c, &b, "middle_click"),
        };
        let mut slots = Vec::new();
        if yaml::is_list(c, &format!("{b}.slots")) {
            for slot in yaml::list(c, &format!("{b}.slots")) {
                match slot.split_once('-') {
                    Some((a, z)) => {
                        if let (Ok(a), Ok(z)) = (a.trim().parse::<usize>(), z.trim().parse::<usize>()) {
                            slots.extend(a..=z);
                        }
                    }
                    None => slots.extend(slot.trim().parse::<usize>().ok()),
                }
            }
        } else {
            slots.push(yaml::int(c, &format!("{b}.slot"), 0).max(0) as usize);
        }
        for slot in slots {
            let list = out.entry(slot).or_default();
            let mut copy = item.clone();
            copy.slot = slot;
            // A TreeMap by priority: the same priority replaces the earlier item.
            list.retain(|i| i.priority != copy.priority);
            list.push(Arc::new(copy));
            list.sort_by_key(|i| i.priority);
        }
    }
    out
}

/// What a (re)load produced.
pub struct Loaded {
    pub menus: HashMap<String, Arc<Menu>>,
    pub warnings: Vec<String>,
    pub config: Yaml,
}

pub const EXAMPLES: [(&str, &str); 5] = [
    ("basics_menu", include_str!("../data/basics_menu.yml")),
    ("advanced_menu", include_str!("../data/advanced_menu.yml")),
    ("requirements_menu", include_str!("../data/requirements_menu.yml")),
    ("actions_menu", include_str!("../data/actions_menu.yml")),
    ("items_menu", include_str!("../data/items_menu.yml")),
];
pub const DEFAULT_MENU: &str = include_str!("../data/default_menu.yml");

fn read_yaml(path: &Path) -> Result<Yaml, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read file: {} ({e})", path.display()))?;
    if text.trim().is_empty() {
        return Ok(Yaml::Null);
    }
    horizon_kit::yaml::parse(&text).map_err(|e| format!("Detected invalid configuration in file: {} ({e})", path.display()))
}

/// `loadDefConfig` + `loadGUIMenus` (or `loadGUIMenu(only)`): config.yml written with its defaults, the
/// examples made on first start, every menu read. Disk only: runs on a background thread.
pub fn load_all(folder: &Path, only: Option<&str>) -> Result<Loaded, String> {
    let menus_dir = folder.join("gui_menus");
    let _ = std::fs::create_dir_all(&menus_dir);
    let config_path = folder.join("config.yml");
    let mut config = if config_path.exists() { read_yaml(&config_path)? } else { Yaml::Null };
    let mut changed = !config_path.exists();
    if !config.is_mapping() {
        config = Yaml::Mapping(Default::default());
    }
    let map = config.as_mapping_mut().expect("mapping");
    for (k, v) in [("debug", Yaml::from("LOW")), ("check_updates", Yaml::from(true)), ("use_admin_commands_in_menus_list", Yaml::from(false)), ("menus_list_page_size", Yaml::from(10))] {
        if !map.contains_key(k) {
            map.insert(Yaml::from(k), v);
            changed = true;
        }
    }
    if !map.contains_key("gui_menus") {
        let mut menus = serde_yaml::Mapping::new();
        for (name, text) in EXAMPLES {
            let file = menus_dir.join(format!("{name}.yml"));
            if !file.exists() {
                let _ = std::fs::write(&file, text);
            }
            let mut entry = serde_yaml::Mapping::new();
            entry.insert(Yaml::from("file"), Yaml::from(format!("{name}.yml")));
            menus.insert(Yaml::from(name), Yaml::Mapping(entry));
        }
        map.insert(Yaml::from("gui_menus"), Yaml::Mapping(menus));
        changed = true;
    }
    if changed {
        let header = "# DeluxeMenus 1.14.1-Release main configuration file\n#\n# A full wiki on how to use this plugin can be found at:\n# https://wiki.helpch.at/helpchat-plugins/deluxemenus\n#\n";
        if let Ok(text) = serde_yaml::to_string(&config) {
            let _ = std::fs::write(&config_path, format!("{header}{text}"));
        }
    }
    let mut warnings = Vec::new();
    let mut menus: HashMap<String, Arc<Menu>> = HashMap::new();
    let mut taken: Vec<String> = Vec::new();
    for key in yaml::keys(&config, "gui_menus") {
        if only.is_some_and(|o| o != key) {
            continue;
        }
        let loaded = match yaml::string(&config, &format!("gui_menus.{key}.file")) {
            Some(file) => {
                if !file.ends_with(".yml") {
                    warnings.push(format!("Filename specified for menu: {key} is not a .yml file! Skipping loading of menu: {key}"));
                    continue;
                }
                let path = menus_dir.join(&file);
                if !path.exists() {
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(&path, DEFAULT_MENU);
                    warnings.push(format!("{file} created! Add your menu options to this file and use /dm reload to load it!"));
                }
                match read_yaml(&path) {
                    Ok(doc) if doc.as_mapping().is_some_and(|m| !m.is_empty()) => load_menu(&doc, &key, "", &file.replace('/', std::path::MAIN_SEPARATOR_STR), &taken, &mut warnings),
                    Ok(_) => {
                        let _ = std::fs::write(&path, DEFAULT_MENU);
                        warnings.push(format!("Menu config: {file} is empty! Creating default config example..."));
                        None
                    }
                    Err(e) => {
                        warnings.push(e);
                        warnings.push(format!("Menu: {key} in file: {file} not loaded."));
                        None
                    }
                }
            }
            None => {
                warnings.push(format!("Menu: {key} does not have a file specified in config.yml! Creating menus in the config.yml file is deprecated."));
                load_menu(&config, &key, &format!("gui_menus.{key}."), "config", &taken, &mut warnings)
            }
        };
        if let Some(menu) = loaded {
            taken.extend(menu.commands.iter().cloned());
            menus.insert(key.clone(), Arc::new(menu));
        }
    }
    Ok(Loaded { menus, warnings, config })
}
