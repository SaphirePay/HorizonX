//! DeluxeMenus 1.14.1 rewritten in Rust for Horizon. The same `config.yml` + `gui_menus/*.yml`, the same
//! `/dm` commands and permissions, the same actions and requirements; `%deluxemenus_..%` placeholders for
//! other plugins. Menu files are read and parsed on a background thread; everything that asks the game
//! (placeholders, permissions, money, inventories) runs on the server thread, and nothing runs per tick
//! except a one-second check for menus with `update: true` items. The Java it follows is in java/.

pub mod action;
pub mod item;
pub mod menu;
pub mod meta;
pub mod req;
pub mod yaml;

use horizon_bukkit::fam_ev_player::{InventoryClickEvent, InventoryCloseEvent, InventoryDragEvent, InventoryOpenEvent};
use horizon_bukkit::prelude::*;
use horizon_bukkit::{CommandSender, Inventory, Player, Plugin, bukkit_plugin};
use horizon_kit::text::colorize;
use menu::{Click, Item, Menu};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static MENUS: Mutex<Option<HashMap<String, Arc<Menu>>>> = Mutex::new(None);
static SESSIONS: Mutex<Option<HashMap<String, Session>>> = Mutex::new(None);
static LAST: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static COOLDOWN: Mutex<Option<HashMap<String, (Instant, Instant)>>> = Mutex::new(None);
static DEBUG: Mutex<String> = Mutex::new(String::new());
static REGISTERED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
static LIST_PAGE: Mutex<usize> = Mutex::new(10);

type MainJob = Box<dyn FnOnce() + Send>;
static MAIN: Mutex<Vec<MainJob>> = Mutex::new(Vec::new());
static MAIN_PENDING: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Set while this plugin opens a menu, so its own InventoryOpenEvent doesn't close it.
    static OPENING: Cell<bool> = const { Cell::new(false) };
}

pub fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `f` on the server thread at the start of the next tick.
fn on_main(f: impl FnOnce() + Send + 'static) {
    MAIN.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(f));
    MAIN_PENDING.store(true, Ordering::Release);
}

fn drain_main() {
    if MAIN_PENDING.swap(false, Ordering::AcqRel) {
        let jobs = std::mem::take(&mut *MAIN.lock().unwrap_or_else(|e| e.into_inner()));
        for job in jobs {
            job();
        }
    }
}

pub fn warn(text: &str) {
    if let Some(p) = plugin() {
        p.warning(text);
    }
}

/// `debug: LOWEST..HIGHEST` in config.yml: notes only show at LOWEST/LOW.
pub fn debug(text: &str) {
    let level = DEBUG.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if matches!(level.as_str(), "LOWEST" | "LOW")
        && let Some(p) = plugin()
    {
        p.info(text);
    }
}

/// `MenuHolder`'s view of the viewer: who, whose placeholders, the typed arguments and how they're filled.
#[derive(Clone)]
pub struct Holder {
    pub viewer: Player,
    pub placeholder: Option<Player>,
    pub args: Arc<HashMap<String, String>>,
    pub parse_in: bool,
    pub parse_after: bool,
}

impl Holder {
    fn target(&self) -> &Player {
        self.placeholder.as_ref().unwrap_or(&self.viewer)
    }
    pub fn placeholders(&self, text: &str) -> String {
        horizon_kit::hooks::placeholders(text, Some(self.target()))
    }
    pub fn arguments(&self, text: &str) -> String {
        if self.args.is_empty() || !text.contains('{') {
            return text.to_owned();
        }
        let mut out = text.to_owned();
        for (k, v) in self.args.iter() {
            let value = if self.parse_in { self.placeholders(v) } else { v.clone() };
            out = out.replace(&format!("{{{k}}}"), &value);
        }
        out
    }
    /// `setPlaceholdersAndArguments`.
    pub fn fill(&self, text: &str) -> String {
        if self.parse_after { self.placeholders(&self.arguments(text)) } else { self.arguments(&self.placeholders(text)) }
    }
}

struct Session {
    holder: Holder,
    menu: Arc<Menu>,
    inv: Inventory,
    active: BTreeMap<usize, Arc<Item>>,
    updating: bool,
    update: bool,
    next_update: Instant,
}

fn sessions<R>(f: impl FnOnce(&mut HashMap<String, Session>) -> R) -> R {
    f(SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new))
}

/// The open menu's holder for a player (uuid), if any.
pub fn session_holder(uuid: &str) -> Option<Holder> {
    sessions(|s| s.get(uuid).map(|x| x.holder.clone()))
}

pub fn menu_named(name: &str) -> Option<Arc<Menu>> {
    MENUS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, m)| m.clone())
}

fn all_menus() -> Vec<Arc<Menu>> {
    let mut v: Vec<Arc<Menu>> = MENUS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|m| m.values().cloned().collect()).unwrap_or_default();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

fn menu_by_command(cmd: &str) -> Option<Arc<Menu>> {
    MENUS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.values().find(|m| m.commands.iter().any(|c| c.eq_ignore_ascii_case(cmd))).cloned()
}

/// The items each slot shows for this viewer: the first (by priority) whose view requirement passes.
fn active_items(menu: &Menu, h: &Holder) -> BTreeMap<usize, Arc<Item>> {
    let mut active = BTreeMap::new();
    for (slot, list) in &menu.items {
        if *slot >= menu.size {
            continue;
        }
        for item in list {
            if item.view.as_ref().is_none_or(|v| req::evaluate(v, h)) {
                active.insert(*slot, item.clone());
                break;
            }
        }
    }
    active
}

/// `Menu.openMenu`: argument and open requirements, the items, the window.
pub fn open_menu(menu: &Arc<Menu>, viewer: &Player, args: Option<HashMap<String, String>>, placeholder: Option<Player>) {
    let h = Holder { viewer: viewer.clone(), placeholder, args: Arc::new(args.unwrap_or_default()), parse_in: menu.parse_in_args, parse_after: menu.parse_after_args };
    for rl in &menu.arg_requirements {
        if !req::evaluate(rl, &h) {
            action::run(&rl.deny, &h);
            return;
        }
    }
    if let Some(open) = &menu.open_requirement {
        let bypass = viewer.has_permission(&format!("deluxemenus.openrequirement.bypass.{}", menu.name)) || viewer.has_permission("deluxemenus.openrequirement.bypass.*");
        if !bypass && !req::evaluate(open, &h) {
            action::run(&open.deny, &h);
            return;
        }
    }
    let active = active_items(menu, &h);
    if active.is_empty() {
        return;
    }
    action::run(&menu.open_actions, &h);
    let title = colorize(&h.fill(&menu.title));
    let mut inv = if menu.kind == "CHEST" { Inventory::create(menu.size, &title) } else { Inventory::create_of_type(&menu.kind, &title).unwrap_or_else(|| Inventory::create(27, &title)) };
    let mut update = false;
    for (slot, item) in &active {
        if let Some(stack) = item::build(item, &h) {
            update |= item.update;
            inv.set_item(*slot, Some(stack));
        }
    }
    let uuid = viewer.get_unique_id();
    // closeMenu(viewer, false) of what was open, then the new one.
    if let Some(old) = sessions(|s| s.remove(&uuid)) {
        remember_last(&uuid, &old.menu.name);
    }
    OPENING.with(|o| o.set(true));
    viewer.open_inventory(&inv);
    OPENING.with(|o| o.set(false));
    let next_update = Instant::now() + Duration::from_secs(1);
    sessions(|s| s.insert(uuid, Session { holder: h, menu: menu.clone(), inv, active, updating: false, update, next_update }));
}

fn remember_last(uuid: &str, menu: &str) {
    LAST.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(uuid.to_owned(), menu.to_owned());
}

/// `Menu.closeMenu`: the close actions (when asked), the session gone, the window closed next tick (when asked).
pub fn close_menu(player: &Player, close: bool, run_close_actions: bool) {
    let uuid = player.get_unique_id();
    let Some(session) = sessions(|s| s.remove(&uuid)) else { return };
    if run_close_actions {
        action::run(&session.menu.close_actions, &session.holder);
    }
    remember_last(&uuid, &session.menu.name);
    if close && let Some(p) = plugin() {
        let player = player.clone();
        p.run_task(move || player.close_inventory());
    }
}

/// `MenuHolder.refreshMenu`: view requirements again and every item rebuilt.
pub fn refresh(player: &Player) {
    let uuid = player.get_unique_id();
    let Some((h, menu, mut inv)) = sessions(|s| {
        let x = s.get_mut(&uuid)?;
        x.updating = true;
        Some((x.holder.clone(), x.menu.clone(), x.inv.clone()))
    }) else {
        return;
    };
    let menu = menu_named(&menu.name).unwrap_or(menu);
    let active = active_items(&menu, &h);
    if active.is_empty() {
        close_menu(player, true, false);
        return;
    }
    let mut update = false;
    for slot in 0..inv.get_size() {
        match active.get(&slot) {
            Some(item) => {
                update |= item.update;
                inv.set_item(slot, item::build(item, &h));
            }
            None => inv.set_item(slot, None),
        }
    }
    sessions(|s| {
        if let Some(x) = s.get_mut(&uuid).filter(|x| x.inv.same_as(&inv)) {
            x.inv = inv;
            x.active = active;
            x.menu = menu;
            x.update = update;
            x.updating = false;
        }
    });
}

/// Once a second: items with `update: true` get their name, lore and dynamic amount filled again, every
/// `update_interval` seconds of their menu.
fn update_tick() {
    let now = Instant::now();
    let due: Vec<(String, Holder, Inventory, Vec<Arc<Item>>)> = sessions(|s| {
        s.iter_mut()
            .filter(|(_, x)| x.update && !x.updating && x.next_update <= now)
            .map(|(uuid, x)| {
                x.next_update = now + Duration::from_secs(x.menu.update_interval);
                (uuid.clone(), x.holder.clone(), x.inv.clone(), x.active.values().filter(|i| i.update).cloned().collect())
            })
            .collect()
    });
    for (uuid, h, mut inv, items) in due {
        for item in items {
            let Some(mut stack) = inv.get_item(item.slot).cloned() else { continue };
            let mut amount = stack.get_amount();
            if let Some(d) = &item.dynamic_amount {
                match h.fill(d).trim().parse::<i32>() {
                    Ok(n) => amount = n.max(1),
                    Err(_) => warn(&format!("Something went wrong while updating item in slot {}. Invalid dynamic amount: {}", item.slot, h.fill(d))),
                }
            }
            if item.name_has_placeholders()
                && let Some(name) = &item.display_name
            {
                stack.set_display_name(&colorize(&h.fill(name)));
            }
            if item.lore_has_placeholders() {
                stack.set_lore(item::lore_of(&h, &item.lore));
            }
            stack.set_amount(amount);
            if inv.get_item(item.slot) != Some(&stack) {
                inv.set_item(item.slot, Some(stack));
            }
        }
        sessions(|s| {
            if let Some(x) = s.get_mut(&uuid).filter(|x| x.inv.same_as(&inv)) {
                x.inv = inv;
            }
        });
    }
}

fn on_click(event: &mut InventoryClickEvent) {
    let Some(player) = event.get_who_clicked() else { return };
    let uuid = player.get_unique_id();
    let gui = event.get_inventory().gui().cloned();
    let slot = usize::try_from(event.get_raw_slot()).ok();
    let found = sessions(|s| {
        let x = s.get(&uuid)?;
        if !gui.as_ref().is_some_and(|g| g.same_as(&x.inv)) {
            return Some(None);
        }
        Some(Some((x.updating, x.holder.clone(), slot.and_then(|slot| x.active.get(&slot).cloned()))))
    });
    let Some(found) = found else { return };
    let Some((updating, h, item)) = found else {
        // A session for a window that isn't open any more.
        sessions(|s| s.remove(&uuid));
        return;
    };
    event.set_cancelled(true);
    if updating {
        return;
    }
    let Some(item) = item else { return };
    let click = event.get_click();
    let now = Instant::now();
    {
        let mut cd = COOLDOWN.lock().unwrap_or_else(|e| e.into_inner());
        let cd = cd.get_or_insert_with(HashMap::new);
        let past = now.checked_sub(Duration::from_secs(1)).unwrap_or(now);
        let (last, shift) = cd.get(&uuid).copied().unwrap_or((past, past));
        if (last != past && now.duration_since(last) < Duration::from_millis(75)) || (shift != past && now.duration_since(shift) < Duration::from_millis(200)) || click == "DOUBLE_CLICK" {
            return;
        }
        if click == "SHIFT_LEFT" {
            cd.insert(uuid.clone(), (last, now));
        }
    }
    let shift = matches!(click, "SHIFT_LEFT" | "SHIFT_RIGHT");
    let left = matches!(click, "LEFT" | "SHIFT_LEFT");
    let right = matches!(click, "RIGHT" | "SHIFT_RIGHT");
    let _ = handle(&uuid, &h, &item.click)
        || (shift && left && handle(&uuid, &h, &item.shift_left))
        || (shift && right && handle(&uuid, &h, &item.shift_right))
        || (click == "LEFT" && handle(&uuid, &h, &item.left))
        || (click == "RIGHT" && handle(&uuid, &h, &item.right))
        || (click == "MIDDLE" && handle(&uuid, &h, &item.middle));
}

/// `handleClick`: false when the item has no actions for this click.
fn handle(uuid: &str, h: &Holder, click: &Click) -> bool {
    if click.actions.is_empty() {
        return false;
    }
    if let Some(r) = &click.requirements
        && !req::evaluate(r, h)
    {
        action::run(&r.deny, h);
        return true;
    }
    {
        let now = Instant::now();
        let mut cd = COOLDOWN.lock().unwrap_or_else(|e| e.into_inner());
        let cd = cd.get_or_insert_with(HashMap::new);
        let past = now.checked_sub(Duration::from_secs(1)).unwrap_or(now);
        let shift = cd.get(uuid).map_or(past, |c| c.1);
        cd.insert(uuid.to_owned(), (now, shift));
    }
    action::run(&click.actions, h);
    true
}

fn on_drag(event: &mut InventoryDragEvent) {
    let Some(player) = event.get_who_clicked() else { return };
    let uuid = player.get_unique_id();
    let gui = event.get_inventory().gui().cloned();
    if sessions(|s| s.get(&uuid).is_some_and(|x| gui.as_ref().is_some_and(|g| g.same_as(&x.inv)))) {
        event.set_cancelled(true);
    }
}

fn on_close(event: &mut InventoryCloseEvent) {
    let Some(player) = event.get_player() else { return };
    let uuid = player.get_unique_id();
    let gui = event.get_inventory().gui().cloned();
    let closed = sessions(|s| {
        let same = s.get(&uuid).is_some_and(|x| gui.as_ref().is_none_or(|g| g.same_as(&x.inv)));
        if same { s.remove(&uuid) } else { None }
    });
    if let Some(x) = closed {
        remember_last(&uuid, &x.menu.name);
    }
}

fn on_open(event: &mut InventoryOpenEvent) {
    if OPENING.with(Cell::get) {
        return;
    }
    let Some(player) = event.get_player() else { return };
    let uuid = player.get_unique_id();
    if let Some(x) = sessions(|s| s.remove(&uuid)) {
        remember_last(&uuid, &x.menu.name);
    }
}

/// Menu open commands (`open_command`), as `onCommandExecute`; with the menu's `args` typed after it.
fn on_command(event: &mut PlayerCommandPreprocessEvent) {
    if event.is_cancelled() {
        return;
    }
    let line = event.get_message().trim_start_matches('/').to_owned();
    let mut words = line.split(' ').filter(|w| !w.is_empty());
    let Some(first) = words.next() else { return };
    let typed: Vec<String> = words.map(str::to_owned).collect();
    let menu = menu_by_command(&line.to_ascii_lowercase()).or_else(|| menu_by_command(first).filter(|m| !m.args.is_empty()));
    let Some(menu) = menu else { return };
    event.set_cancelled(true);
    let player = event.get_player().clone();
    let typed = if menu.commands.iter().any(|c| c.eq_ignore_ascii_case(&line)) { Vec::new() } else { typed };
    open_with_typed(&menu, &player, &typed);
}

/// `RegistrableMenuCommand.execute`: the typed words given to the menu's arguments.
fn open_with_typed(menu: &Arc<Menu>, player: &Player, typed: &[String]) {
    let mut args = None;
    if !menu.args.is_empty() {
        if typed.len() < menu.args.len() {
            if let Some(usage) = &menu.args_usage {
                player.send_message(&colorize(usage));
            }
            return;
        }
        let mut map = HashMap::new();
        for (i, a) in menu.args.iter().enumerate() {
            if i + 1 == menu.args.len() {
                map.insert(a.clone(), typed[i..].join(" "));
            } else {
                map.insert(a.clone(), typed[i].clone());
            }
        }
        args = Some(map);
    }
    open_menu(menu, player, args, None);
}

struct MenuCommand {
    label: String,
}

impl horizon_bukkit::fam_paper::BasicCommand for MenuCommand {
    fn execute(&self, sender: &CommandSender, args: &[&str]) -> Result<(), String> {
        let Some(player) = sender.as_player() else {
            sender.send_message("Menus can only be opened by players!");
            return Ok(());
        };
        if let Some(menu) = menu_by_command(&self.label) {
            let typed: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
            open_with_typed(&menu, player, &typed);
        }
        Ok(())
    }
}

/// `register_command: true`: the menu's commands go into the server's command tree (once per name: a reload
/// that drops a menu leaves its name there, answering nothing).
fn register_menu_commands() {
    let Some(p) = plugin() else { return };
    for menu in all_menus().into_iter().filter(|m| m.register) {
        let Some(label) = menu.commands.first().cloned() else { continue };
        if !REGISTERED.lock().unwrap_or_else(|e| e.into_inner()).insert(label.clone()) {
            continue;
        }
        let aliases: Vec<&str> = menu.commands[1..].iter().map(String::as_str).collect();
        p.register_basic_command(&label, &aliases, MenuCommand { label: label.clone() });
    }
}

// ---- Loading ----

/// Every menu read again on a background thread; `then` runs on the server thread with how many loaded.
fn load(only: Option<String>, then: impl FnOnce(Result<usize, String>) + Send + 'static) {
    let Some(p) = plugin() else { return };
    let folder = p.get_data_folder();
    std::thread::spawn(move || {
        let loaded = menu::load_all(&folder, only.as_deref());
        on_main(move || {
            let result = loaded.map(|l| {
                for w in &l.warnings {
                    warn(w);
                }
                *DEBUG.lock().unwrap_or_else(|e| e.into_inner()) = yaml::string(&l.config, "debug").unwrap_or_else(|| "LOW".into()).to_ascii_uppercase();
                *LIST_PAGE.lock().unwrap_or_else(|e| e.into_inner()) = yaml::int(&l.config, "menus_list_page_size", 10).max(1) as usize;
                let mut held = MENUS.lock().unwrap_or_else(|e| e.into_inner());
                let menus = held.get_or_insert_with(HashMap::new);
                match &only {
                    Some(name) => {
                        menus.remove(name);
                        menus.extend(l.menus);
                    }
                    None => *menus = l.menus,
                }
                menus.len()
            });
            register_menu_commands();
            then(result);
        });
    });
}

/// `Menu.unload`: who has these menus open gets them closed.
fn close_all(only: Option<&str>) {
    let open: Vec<String> = sessions(|s| s.iter().filter(|(_, x)| only.is_none_or(|o| x.menu.name.eq_ignore_ascii_case(o))).map(|(u, _)| u.clone()).collect());
    for p in horizon_bukkit::get_online_players() {
        if open.contains(&p.get_unique_id()) {
            close_menu(&p, true, false);
        }
    }
}

// ---- /dm ----

const GOLD: &str = "§6";
const RED: &str = "§c";
const GRAY: &str = "§7";
const GREEN: &str = "§a";
const YELLOW: &str = "§e";
const NO_PERMISSION: &str = "§cYou don't have permission to do that!";

fn title() -> &'static str {
    "§6§lDeluxe§e§lMenus"
}

fn tell(s: &CommandSender, text: &str) {
    s.send_message(text);
}

fn wrong_usage(usage: &str) -> String {
    format!("{RED}Incorrect Usage! {GRAY}Use {usage}")
}

fn not_online(name: &str) -> String {
    format!("{RED}Player: {GOLD}{name} {RED}is not online!")
}

fn invalid_menu(name: &str) -> String {
    format!("{RED}Could not find menu: {GOLD}{name}{RED}.")
}

fn exact(name: &str) -> Option<Player> {
    horizon_bukkit::get_player(name).filter(|p| p.get_name().eq_ignore_ascii_case(name))
}

fn dm(sender: &CommandSender, args: &[&str]) -> bool {
    let Some(first) = args.first() else {
        let version = plugin().map(|p| p.get_version().to_owned()).unwrap_or_default();
        tell(sender, &format!("{} {GRAY}version {GOLD}{version}", title()));
        tell(sender, &format!("{GRAY}Created by §fHelpChat"));
        return true;
    };
    let rest: Vec<String> = args[1..].iter().map(|a| (*a).to_owned()).collect();
    match first.to_ascii_lowercase().as_str() {
        "open" => open_cmd(sender, &rest),
        "list" => list_cmd(sender, &rest),
        "reload" => reload_cmd(sender, &rest),
        "execute" => execute_cmd(sender, &rest),
        "meta" => meta_cmd(sender, &rest),
        "help" => help_cmd(sender),
        "dump" => {
            if !sender.has_permission("deluxemenus.admin") {
                tell(sender, NO_PERMISSION);
            } else {
                tell(sender, &format!("{RED}Failed to create and post dump! {GRAY}(the Rust DeluxeMenus posts no dumps)"));
            }
        }
        _ => tell(sender, &wrong_usage("/dm help")),
    }
    true
}

fn help_cmd(sender: &CommandSender) {
    let admin = sender.has_permission("deluxemenus.admin");
    tell(sender, &format!("{} {GRAY}help", title()));
    let lines: &[&str] = if admin {
        &["/dm open <menu-name> [player]", "/dm list [page/all]", "/dm execute <player> <action>", "/dm dump <menu-name/config>", "/dm meta <player> <set/remove/add/subtract/list/show>", "/dm reload [menu-name]"]
    } else {
        &["/dm open <menu-name> [player]", "/dm list [page/all]", "/dm dump <menu-name/config>", "/dm reload [menu-name]"]
    };
    for l in lines {
        tell(sender, &format!("{GOLD}> {YELLOW}{l}"));
    }
}

fn open_cmd(sender: &CommandSender, a: &[String]) {
    if !sender.has_permission("deluxemenus.open") {
        return tell(sender, NO_PERMISSION);
    }
    let is_player = sender.as_player().is_some();
    if a.is_empty() {
        return tell(sender, &wrong_usage("/dm open <menu-name> [player]"));
    }
    if all_menus().is_empty() {
        return tell(sender, &format!("{YELLOW}There are no menus loaded..."));
    }
    let mut placeholder_name = None;
    if a.len() == 2 && a[1].starts_with("-p:") {
        if !sender.has_permission("deluxemenus.placeholdersfor") {
            return tell(sender, &format!("{RED}You don't have permission to use the argument -p:<player>!"));
        }
        placeholder_name = Some(a[1].replacen("-p:", "", 1));
    } else if a.len() >= 3 && a[2].starts_with("-p:") {
        if !sender.has_permission("deluxemenus.placeholdersfor") {
            return tell(sender, &format!("{RED}You don't have permission to use the argument -p:<player>!"));
        }
        placeholder_name = Some(a[2].replacen("-p:", "", 1));
    }
    let viewer = if a.len() >= 2 && (placeholder_name.is_none() || a.len() >= 3) {
        if (is_player || a.len() >= 3) && !sender.has_permission("deluxemenus.open.others") {
            return tell(sender, NO_PERMISSION);
        }
        exact(&a[1])
    } else {
        match sender.as_player() {
            Some(p) => Some(p.clone()),
            None => return tell(sender, &format!("{RED}You must specify a player to open a menu for!")),
        }
    };
    let Some(viewer) = viewer else { return tell(sender, &not_online(&a[1])) };
    let mut placeholder = None;
    if let Some(name) = placeholder_name {
        let Some(p) = exact(&name) else { return tell(sender, &not_online(&name)) };
        if p.has_permission("deluxemenus.placeholdersfor.exempt") {
            return tell(sender, &format!("§f{name} {RED}is exempt from placeholder target arguments."));
        }
        placeholder = Some(p);
    }
    let Some(menu) = menu_named(&a[0]) else { return tell(sender, &invalid_menu(&a[0])) };
    open_menu(&menu, &viewer, None, placeholder);
}

fn list_cmd(sender: &CommandSender, a: &[String]) {
    if !sender.has_permission("deluxemenus.list") {
        return tell(sender, NO_PERMISSION);
    }
    let menus = all_menus();
    if menus.is_empty() {
        return tell(sender, &format!("{YELLOW}There are no menus loaded..."));
    }
    let size = *LIST_PAGE.lock().unwrap_or_else(|e| e.into_inner());
    let console = sender.as_player().is_none();
    let all = console || a.first().is_some_and(|x| x.eq_ignore_ascii_case("all"));
    let pages = menus.len().div_ceil(size).max(1);
    let page = if all { 1 } else { a.first().and_then(|x| x.parse::<usize>().ok()).unwrap_or(1).clamp(1, pages) };
    let shown: Vec<&Arc<Menu>> = if all { menus.iter().collect() } else { menus.iter().skip((page - 1) * size).take(size).collect() };
    tell(sender, &format!("{GRAY}The following {GOLD}{} {GRAY}menus are loaded on the server:", menus.len()));
    let mut by_path: BTreeMap<String, Vec<&Arc<Menu>>> = BTreeMap::new();
    for m in shown {
        by_path.entry(m.path.clone()).or_default().push(m);
    }
    for (path, list) in by_path {
        tell(sender, &format!("{GOLD}{path}"));
        for m in list {
            let cmds = if m.commands.is_empty() { format!("{GRAY}No open command") } else { m.commands.iter().map(|c| format!("{YELLOW}/{c}")).collect::<Vec<_>>().join(&format!("{GRAY}, ")) };
            tell(sender, &format!("  {GRAY}- {GOLD}{} {GRAY}| {cmds}", m.name));
        }
    }
    if !all && pages > 1 {
        tell(sender, &format!("{GRAY}Page {GOLD}{page}{GRAY}/{GOLD}{pages} {GRAY}- Use /dm list <page> to view more menus"));
    }
}

fn reload_cmd(sender: &CommandSender, a: &[String]) {
    if !sender.has_permission("deluxemenus.reload") {
        return tell(sender, NO_PERMISSION);
    }
    let sender = sender.clone();
    if let Some(name) = a.first() {
        let Some(menu) = menu_named(name) else { return tell(&sender, &invalid_menu(name)) };
        close_all(Some(&menu.name));
        let key = menu.name.clone();
        load(Some(key.clone()), move |r| match r {
            Ok(_) if menu_named(&key).is_some() => tell(&sender, &format!("{GOLD}{key} {GREEN}menu successfully reloaded!")),
            _ => tell(&sender, &format!("{GOLD}{key} {RED}menu could not be reloaded!")),
        });
        return;
    }
    close_all(None);
    horizon_kit::hooks::refresh();
    load(None, move |r| match r {
        Ok(n) => {
            tell(&sender, &format!("{} {GREEN}successfully reloaded!", title()));
            tell(&sender, &format!("{YELLOW}{n} {}", if n == 1 { "menu loaded..." } else { "menus loaded..." }));
        }
        Err(e) => {
            warn(&e);
            tell(&sender, &format!("{RED}Errors detected in config.yml. Failed to reload."));
        }
    });
}

fn execute_cmd(sender: &CommandSender, a: &[String]) {
    if !sender.has_permission("deluxemenus.admin") {
        return tell(sender, NO_PERMISSION);
    }
    if a.len() < 2 {
        return tell(sender, &wrong_usage("/dm execute <player> <action>"));
    }
    let Some(target) = exact(&a[0]) else { return tell(sender, &not_online(&a[0])) };
    let Some(act) = menu::Action::parse(&a[1..].join(" ")) else { return tell(sender, &format!("{RED}Action type specified does not exist!")) };
    let h = session_holder(&target.get_unique_id()).unwrap_or(Holder { viewer: target.clone(), placeholder: None, args: Arc::default(), parse_in: true, parse_after: true });
    match action::run_one(&act, &h) {
        None => tell(sender, &format!("{RED}The chance for this action determined the action should not execute!")),
        Some(0) => tell(sender, &format!("{GREEN}Action executed for player: {}", target.get_name())),
        Some(d) => tell(sender, &format!("{GREEN}Action set to be executed in {d} ticks.")),
    }
}

fn meta_cmd(sender: &CommandSender, a: &[String]) {
    if !sender.has_permission("deluxemenus.admin") {
        return tell(sender, NO_PERMISSION);
    }
    if a.len() < 2 {
        return tell(sender, &wrong_usage("/dm meta <player> <set/remove/add/subtract/switch/list/show>"));
    }
    let Some(uuid) = exact(&a[0]).map(|p| p.get_unique_id()) else { return tell(sender, &not_online(&a[0])) };
    let op = a[1].to_ascii_lowercase();
    let need = |n: usize, usage: &str| -> bool {
        if a.len() < n {
            tell(sender, &wrong_usage(usage));
            false
        } else {
            true
        }
    };
    match op.as_str() {
        "list" => {
            if !need(3, "/dm meta <player> list <type> [page]") {
                return;
            }
            let kind = a[2].to_ascii_uppercase();
            if !meta::TYPES.contains(&kind.as_str()) {
                return tell(sender, &format!("{RED}Unsupported meta type {GOLD}{kind}{RED}! Supported values are: {GOLD}{}", meta::TYPES.join(", ")));
            }
            let values = meta::list(&uuid, &kind);
            if values.is_empty() {
                return tell(sender, &format!("{RED}Could not find any meta values with type {GOLD}{kind}{RED}!"));
            }
            for (k, v) in values {
                tell(sender, &format!("{GRAY}- {GOLD}{k}{GRAY}: {YELLOW}{v}"));
            }
        }
        "show" => {
            if !need(4, "/dm meta <player> show <key> <type>") {
                return;
            }
            match meta::get(&uuid, &a[2], &a[3]) {
                Some(v) => tell(sender, &format!("{GRAY}Meta value with key {GOLD}{} {GRAY}and type {GOLD}{} {GRAY}is: {YELLOW}{v}", a[2], a[3].to_ascii_uppercase())),
                None => tell(sender, &format!("{RED}Could not find a meta value with key {GOLD}{} {RED}and type {GOLD}{}{RED}!", a[2], a[3].to_ascii_uppercase())),
            }
        }
        "switch" | "remove" | "set" | "add" | "subtract" => {
            let (usage, n) = match op.as_str() {
                "switch" => ("/dm meta <player> switch <key>", 3),
                "remove" => ("/dm meta <player> remove <key> <type>", 4),
                "set" => ("/dm meta <player> set <key> <type> <value>", 5),
                "add" => ("/dm meta <player> add <key> <type> <value>", 5),
                _ => ("/dm meta <player> subtract <key> <type> <value>", 5),
            };
            if !need(n, usage) {
                return;
            }
            let value = (a.len() > 4).then(|| a[4..].join(" "));
            let outcome = meta::apply(&uuid, &op, &a[2], a.get(3).map(String::as_str), value.as_deref());
            let key = &a[2];
            let done = match op.as_str() {
                "switch" => "switched",
                "remove" => "removed",
                "set" => "set",
                "add" => "increased",
                _ => "decreased",
            };
            match outcome {
                meta::Outcome::Success => tell(sender, &format!("{GRAY}Meta value with key {GOLD}{key} {GRAY}was {GREEN}{done}{GRAY}!")),
                meta::Outcome::InvalidSyntax => tell(sender, &format!("{RED}An invalid meta key was provided: '{GOLD}{key}{RED}'!")),
                meta::Outcome::InvalidType => tell(sender, &format!("{RED}Only NUMBERS can be added or subtracted!")),
                meta::Outcome::NewValueIsDifferentType => tell(sender, &format!("{RED}Given value does not match the given type!")),
                meta::Outcome::ExistentValueIsDifferentType => tell(sender, &format!("{RED}Given key '{GOLD}{key}{RED}' stores a value with a different type!")),
                meta::Outcome::ValueNotFound => tell(sender, &format!("{RED}Could not find a meta value with key {GOLD}{key}{RED}!")),
            }
        }
        _ => tell(sender, &wrong_usage("/dm meta <player> <set/remove/add/subtract/switch/list/show>")),
    }
}

fn complete(sender: &CommandSender, args: &[&str]) -> Vec<String> {
    let starts = |list: Vec<String>, typed: &str| -> Vec<String> {
        let t = typed.to_ascii_lowercase();
        list.into_iter().filter(|x| x.to_ascii_lowercase().starts_with(&t)).collect()
    };
    let names = || horizon_bukkit::get_online_players().into_iter().map(|p| p.get_name().to_owned()).collect::<Vec<_>>();
    let menus = || all_menus().iter().map(|m| m.name.clone()).collect::<Vec<_>>();
    match args {
        [] => Vec::new(),
        [first] => {
            let mut subs = vec!["help".to_owned()];
            for (s, perm) in [("open", "deluxemenus.open"), ("list", "deluxemenus.list"), ("reload", "deluxemenus.reload"), ("execute", "deluxemenus.admin"), ("meta", "deluxemenus.admin"), ("dump", "deluxemenus.admin")] {
                if sender.has_permission(perm) {
                    subs.push(s.to_owned());
                }
            }
            starts(subs, first)
        }
        [sub, rest @ ..] => match (sub.to_ascii_lowercase().as_str(), rest) {
            ("open" | "reload" | "dump", [m]) => starts(menus(), m),
            ("open", [_, p]) => {
                if p.starts_with('-') {
                    starts(names().into_iter().map(|n| format!("-p:{n}")).collect(), p)
                } else {
                    let mut v = names();
                    v.push("-p:".into());
                    starts(v, p)
                }
            }
            ("open", [_, x, p]) if x.starts_with("-p:") => starts(names(), p),
            ("execute" | "meta", [p]) => starts(names(), p),
            ("execute", [_, t]) => starts(menu::ACTIONS.iter().map(|(t, _)| (*t).to_owned()).collect(), t),
            ("meta", [_, op]) => starts(["set", "remove", "add", "subtract", "switch", "list", "show"].map(String::from).to_vec(), op),
            ("list", [p]) => starts(vec!["all".into()], p),
            _ => Vec::new(),
        },
    }
}

/// `%deluxemenus_..%` (the Expansion).
fn placeholder(uuid: &str, params: &str) -> Option<String> {
    if uuid.is_empty() {
        return None;
    }
    let yes = |b: bool| if b { "yes".to_owned() } else { "no".to_owned() };
    let lower = params.to_ascii_lowercase();
    match lower.as_str() {
        "is_in_menu" => return Some(yes(sessions(|s| s.contains_key(uuid)))),
        "opened_menu" => return Some(sessions(|s| s.get(uuid).map(|x| x.menu.name.clone())).unwrap_or_default()),
        "last_menu" => return Some(LAST.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(uuid).cloned()).unwrap_or_default()),
        _ => {}
    }
    if lower.starts_with("meta_has_value_") {
        let rest = &params[15..];
        let mut parts = rest.splitn(2, '_');
        let key = parts.next().unwrap_or_default();
        if !meta::valid_key(&key.to_ascii_lowercase()) {
            return Some(yes(false));
        }
        return Some(yes(meta::has(uuid, key, parts.next())));
    }
    if !lower.starts_with("meta_") {
        return None;
    }
    let parts: Vec<&str> = params[5..].splitn(3, '_').collect();
    if parts.len() < 2 {
        return None;
    }
    if !meta::valid_key(&parts[0].to_ascii_lowercase()) || !meta::TYPES.iter().any(|t| t.eq_ignore_ascii_case(parts[1])) {
        return Some(yes(false));
    }
    Some(meta::get(uuid, parts[0], parts[1]).unwrap_or_else(|| parts.get(2).map(|d| (*d).to_owned()).unwrap_or_default()))
}

fn on_enable(p: Plugin) {
    let started = Instant::now();
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    let folder = p.get_data_folder();
    std::thread::spawn(move || meta::start(folder.join("meta.json")));
    p.set_executor("deluxemenus", |s, _, a| dm(s, a));
    p.set_tab_completer("deluxemenus", |s, _, a| complete(s, a));
    p.register_event_with("LOW", false, on_click);
    p.register_event_with("LOW", false, on_drag);
    p.register_event_with("NORMAL", false, on_close);
    p.register_event_with("NORMAL", false, on_open);
    p.register_event_with("NORMAL", true, on_command);
    p.register_event(|e: &mut PlayerQuitEvent| {
        let uuid = e.get_player().get_unique_id();
        if let Some(x) = sessions(|s| s.remove(&uuid)) {
            remember_last(&uuid, &x.menu.name);
        }
        if let Some(cd) = COOLDOWN.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            cd.remove(&uuid);
        }
    });
    horizon_kit::hooks::provide_placeholders(&p, "deluxemenus", |uuid, _, params| placeholder(uuid, params));
    p.run_task_timer(1, 1, drain_main);
    p.run_task_timer(20, 20, update_tick);
    load(None, move |r| match r {
        Ok(n) => p.info(&format!("{n} GUI menus loaded! Enabled in {} ms (Rust).", started.elapsed().as_millis())),
        Err(e) => p.warning(&format!("Could not load the menus: {e}")),
    });
}

fn on_disable(_p: Plugin) {
    for player in horizon_bukkit::get_online_players() {
        if sessions(|s| s.remove(&player.get_unique_id())).is_some() {
            player.close_inventory();
        }
    }
    meta::stop();
    MAIN.lock().unwrap_or_else(|e| e.into_inner()).clear();
    *SESSIONS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *MENUS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
