//! ZenithBackpacks rewritten in Rust for Horizon (from the ZenithCore `backpacks` module, the version the server
//! ran last). Two things:
//!
//! - **The backpack**: an item (CHEST by default) that opens the player's own storage, 1-6 rows, bought one row
//!   at a time (money / permission / playtime). Shift + right-click opens the upgrade. Contents are per player.
//! - **Vault chests**: a chest item with its own id; placed, right-click opens up to 3 pages of 45 slots with
//!   prev / tidy / search / expand / next controls. Contents are per chest, the item keeps them when broken or
//!   blown up, hoppers can't reach them, a copied item can't be placed twice.
//!
//! Same files as the Java (players/, chests/, placed-chests.yml, config/gui/messages.yml), so a server's data
//! moves over. The Java it follows is in java/.

pub mod conf;
pub mod files;
pub mod items;
pub mod sort;

use conf::{Req, Settings, Step};
use files::{Backpack, Chest, Spot};
use horizon_bukkit::event::{PlayerJoinEvent, PlayerQuitEvent};
use horizon_bukkit::fam_ev_entity::EntityExplodeEvent;
use horizon_bukkit::fam_ev_player::{AsyncChatEvent, InventoryClickEvent, InventoryCloseEvent, InventoryDragEvent, InventoryMoveItemEvent, PlayerInteractEvent};
use horizon_bukkit::fam_ev_world::{BlockBreakEvent, BlockExplodeEvent, BlockPlaceEvent};
use horizon_bukkit::{Block, CommandSender, Component, Inventory, ItemStack, Location, Material, Player, Plugin, bukkit_plugin};
use horizon_kit::{gui, hooks, mini, sound, sync};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ---- State ----

#[derive(Default)]
struct State {
    settings: Arc<Settings>,
    folder: PathBuf,
    /// Backpacks read so far, by player uuid.
    backpacks: HashMap<String, Backpack>,
    /// Vault chests read so far, by chest id.
    chests: HashMap<String, Chest>,
    /// Where each chest is placed, and the reverse.
    placed: HashMap<String, Spot>,
    at: HashMap<Spot, String>,
    /// chest id -> who has it open (one at a time, so two windows can't both write it back).
    chest_open: HashMap<String, String>,
    /// player uuid -> chest id waiting for a search word in chat.
    prompts: HashMap<String, String>,
}

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Never held across a call that can fire an event (opening or closing a window).
fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(STATE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(State::default))
}

fn settings() -> Arc<Settings> {
    st(|s| s.settings.clone())
}

fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

fn next_tick(f: impl FnOnce() + Send + 'static) {
    if let Some(p) = plugin() {
        let mut f = Some(f);
        p.run_task(move || {
            if let Some(f) = f.take() {
                f();
            }
        });
    }
}

// ---- Messages ----

fn send(to: &CommandSender, v: Option<Value>) {
    let Some(v) = v else { return };
    match to.as_player() {
        Some(p) => p.send_component(&Component(v)),
        None => to.send_message(&mini::to_legacy(&v)),
    }
}

fn tell(p: &Player, key: &str, pairs: &[(&str, String)]) {
    if let Some(v) = settings().msg(key, pairs) {
        p.send_component(&Component(v));
    }
}

fn tell_s(s: &CommandSender, key: &str, pairs: &[(&str, String)]) {
    send(s, settings().msg(key, pairs));
}

fn play(p: &Player, name: &Option<String>, pitch: f32) {
    if let Some(n) = name {
        sound::play(p, &format!("{n}:1:{pitch}"));
    }
}

fn title(text: &str, pairs: &[(&str, String)]) -> String {
    mini::to_legacy(&conf::mm(text, pairs))
}

// ---- Requirements ----

fn money(v: f64) -> String {
    format!("{v:.2}")
}

fn minutes_played(p: &Player) -> i64 {
    horizon_bukkit::get_offline_player(p.get_name()).get_statistic("minecraft:custom", "minecraft:play_time") / 1200
}

/// The first requirement not met, as its message (`RequirementEvaluator.checkList`).
fn check(p: &Player, reqs: &[Req], bypass: bool) -> Option<Option<Value>> {
    if bypass {
        return None;
    }
    let s = settings();
    for r in reqs {
        match r {
            Req::Permission(perm) if !p.has_permission(perm) => return Some(s.msg("requirements.permission", &[("perm", perm.clone())])),
            Req::Money(need) => {
                if !hooks::economy::enabled() {
                    return Some(s.msg("general.economy-required", &[]));
                }
                let have = hooks::economy::balance(p.get_name());
                if have < *need {
                    return Some(s.msg("requirements.money", &[("amount", money(*need)), ("have", money(have))]));
                }
            }
            Req::Playtime(need) => {
                let have = minutes_played(p);
                if have < *need {
                    return Some(s.msg("requirements.playtime", &[("need", need.to_string()), ("have", have.to_string())]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Takes the step's money (all at once).
fn pay(p: &Player, step: &Step, bypass: bool) -> bool {
    let total = step.money();
    if bypass || total <= 0.0 {
        return true;
    }
    hooks::economy::enabled() && hooks::economy::balance(p.get_name()) >= total && hooks::economy::withdraw(p.get_name(), total)
}

// ---- Open windows ----

#[derive(Clone)]
enum Kind {
    Backpack,
    Pages { chest: String, page: i32 },
    Search { chest: String, map: Vec<usize> },
}

struct Session {
    inv: Inventory,
    kind: Kind,
}

static SESSIONS: Mutex<Option<HashMap<String, Session>>> = Mutex::new(None);

fn sessions<R>(f: impl FnOnce(&mut HashMap<String, Session>) -> R) -> R {
    f(SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new))
}

/// The session whose window this is.
fn session_of(p: &Player, gui: Option<&Inventory>) -> Option<Kind> {
    let gui = gui?;
    let me = p.get_unique_id();
    sessions(|m| m.get(&me).filter(|s| s.inv.same_as(gui)).map(|s| s.kind.clone()))
}

fn show(p: &Player, inv: Inventory, kind: Kind) {
    // A window opened over ours closes ours first: its close (if it comes) no longer matches.
    let me = p.get_unique_id();
    sessions(|m| m.remove(&me));
    p.open_inventory(&inv);
    sessions(|m| m.insert(me, Session { inv, kind }));
}

// ---- Backpack ----

fn backpack_rows(uuid: &str) -> i32 {
    st(|s| {
        let (folder, start, max) = (s.folder.clone(), s.settings.starting_rows, s.settings.max_rows);
        s.backpacks.entry(uuid.to_owned()).or_insert_with(|| files::load_backpack(&folder, uuid, start, max)).rows
    })
}

fn save_backpack(uuid: &str) {
    let job = st(|s| {
        let max = s.settings.max_rows;
        s.backpacks.get(uuid).map(|b| (files::backpack_path(&s.folder, uuid), files::backpack_text(b, max)))
    });
    if let Some((path, text)) = job {
        files::save(path, text);
    }
}

/// Highest `zenithbackpacks.rows.N` held, or the config max when none.
fn row_ceiling(p: &Player, max: i32) -> i32 {
    (1..=max).rev().find(|n| p.has_permission(&format!("zenithbackpacks.rows.{n}"))).unwrap_or(max)
}

fn next_row_step(p: &Player, rows: i32, s: &Settings) -> Option<Step> {
    let cap = row_ceiling(p, s.max_rows);
    if rows >= s.max_rows || rows >= cap {
        return None;
    }
    let step = s.upgrades.iter().find(|st| st.size > rows)?;
    (step.size <= s.max_rows && step.size <= cap).then(|| step.clone())
}

fn open_backpack(p: &Player) {
    let s = settings();
    let uuid = p.get_unique_id();
    let rows = backpack_rows(&uuid);
    let eff = row_ceiling(p, s.max_rows).min(s.max_rows).min(rows).max(1);
    let size = eff as usize * 9;
    let maxed = rows >= s.max_rows;
    let t = title(&s.gui(if maxed { "backpack.title-maxed" } else { "backpack.title-progress" }), &[("rows", rows.to_string()), ("max", s.max_rows.to_string()), ("eff", eff.to_string())]);
    let mut inv = Inventory::create(size, &t);
    let items: Vec<Option<ItemStack>> = st(|st| st.backpacks.get(&uuid).map(|b| b.items[..size].to_vec())).unwrap_or_default();
    for (i, it) in items.into_iter().enumerate() {
        if it.is_some() {
            inv.set_item(i, it);
        }
    }
    show(p, inv, Kind::Backpack);
}

fn backpack_closed(p: &Player, contents: &[Option<ItemStack>]) {
    let uuid = p.get_unique_id();
    st(|s| {
        if let Some(b) = s.backpacks.get_mut(&uuid) {
            for (i, it) in contents.iter().enumerate().take(conf::BACKPACK_CAP) {
                b.items[i] = it.clone().filter(|s| !s.is_empty());
            }
        }
    });
    save_backpack(&uuid);
    sync_held(p);
}

/// The held backpack redrawn for the saved rows.
fn sync_held(p: &Player) {
    let inv = p.get_inventory();
    let Some(mut hand) = inv.get_item_in_main_hand() else { return };
    if !items::is_backpack(Some(&hand)) {
        return;
    }
    let rows = backpack_rows(&p.get_unique_id());
    let before = hand.clone();
    items::backpack_look(&settings(), &mut hand, rows);
    if hand != before {
        inv.set_item_in_main_hand(&hand);
    }
}

/// The 9 slot confirm window (cancel at 3, confirm at 5) shared by both ladders.
fn confirm_menu(title_key: &str, confirm_key: &str, now: String, next: String, cost: f64, on_confirm: impl Fn(&Player) + Send + Sync + 'static) -> gui::Menu {
    let s = settings();
    let mut m = gui::Menu::new(1, &title(&s.gui(title_key), &[]));
    m.tag("zbp:upgrade");
    let cancel_lore = s.gui_list("upgrade.cancel-lore");
    let cancel_lore = if cancel_lore.is_empty() {
        vec![conf::mm("<dark_gray>❙</dark_gray> <gradient:#F4D03F:#F39C12>ʟᴇꜰᴛ‑ᴄʟɪᴄᴋ</gradient> <dark_gray>·</dark_gray> <red>ʀᴇᴛᴜʀɴ ᴡɪᴛʜᴏᴜᴛ ᴜᴘɢʀᴀᴅᴇ</red>", &[])]
    } else {
        conf::lore_lines(&cancel_lore, &[])
    };
    m.button(3, items::button(&s.gui("upgrade.cancel-material"), "ARROW", conf::mm(&s.gui("upgrade.cancel-name"), &[]), cancel_lore), |c| c.close());
    let field = |label: &str, value: &str| conf::mm(&format!("<light_purple>|</light_purple> <gray>{}:</gray> {value}", s.gui(label)), &[]);
    let cost_text = if cost > 0.0 { format!("<#c9a227>${}</#c9a227>", money(cost)) } else { "<gray>—</gray>".to_owned() };
    let lore = vec![
        conf::mm(&s.gui("upgrade.banner"), &[]),
        Value::String(String::new()),
        conf::mm("<dark_gray><strikethrough>                              </strikethrough></dark_gray>", &[]),
        field("upgrade.labels.current", &now),
        field("upgrade.labels.new-tier", &next),
        field("upgrade.labels.cost", &cost_text),
        Value::String(String::new()),
        conf::mm(&s.gui("upgrade.confirm-hint"), &[]),
    ];
    m.button(5, items::button(&s.gui("upgrade.confirm-material"), "SMITHING_TABLE", conf::mm(&s.gui(confirm_key), &[]), lore), move |c| on_confirm(&c.player));
    m.fill(items::filler(&s));
    m
}

fn tier_text(rows: i32, colour: &str) -> String {
    format!("<{colour}>ᴛɪᴇʀ</{colour}> <white>{rows}</white> <dark_gray>·</dark_gray> <gray>{} ꜱʟᴏᴛꜱ</gray>", rows * 9)
}

fn pages_text(pages: i32, colour: &str) -> String {
    format!("<{colour}>ᴘᴀɢᴇꜱ</{colour}> <white>{pages}</white> <dark_gray>·</dark_gray> <gray>{} ꜱʟᴏᴛꜱ</gray>", conf::capacity(pages))
}

fn open_backpack_upgrade(p: &Player, step: &Step, rows: i32) {
    let m = confirm_menu("upgrade.title", "upgrade.confirm-name", tier_text(rows, "aqua"), tier_text(step.size, "green"), step.money(), |p| try_backpack_upgrade(p));
    m.open(p);
}

fn try_backpack_upgrade(p: &Player) {
    let s = settings();
    let uuid = p.get_unique_id();
    let rows = backpack_rows(&uuid);
    let Some(step) = next_row_step(p, rows, &s) else {
        tell(p, "upgrade.none", &[]);
        p.close_inventory();
        return;
    };
    let bypass = p.has_permission("zenithbackpacks.bypass");
    if let Some(fail) = check(p, &step.reqs, bypass) {
        if let Some(v) = fail {
            p.send_component(&Component(v));
        }
        return;
    }
    if !pay(p, &step, bypass) {
        tell(p, "general.economy-required", &[]);
        return;
    }
    st(|st| {
        if let Some(b) = st.backpacks.get_mut(&uuid) {
            b.rows = step.size;
        }
    });
    save_backpack(&uuid);
    play(p, &s.sound_upgrade, 1.2);
    sync_held(p);
    p.close_inventory();
    open_backpack(p);
    tell(p, "upgrade.success", &[]);
}

/// Right-click with the backpack in the main hand.
fn on_backpack_use(e: &mut PlayerInteractEvent) {
    if e.get_hand() != Some("HAND") || !matches!(e.get_action(), "RIGHT_CLICK_AIR" | "RIGHT_CLICK_BLOCK") {
        return;
    }
    if !items::is_backpack(e.get_item().as_ref()) {
        return;
    }
    // A head would be worn, a chest placed: the item does only this.
    e.set_cancelled(true);
    e.set_use_item_in_hand("DENY");
    e.set_use_interacted_block("DENY");
    let p = e.get_player().clone();
    let sneaking = p.is_sneaking();
    // Opened a tick later, outside HorizonsX's per-player swap (as the commands).
    next_tick(move || backpack_use(&p, sneaking));
}

fn backpack_use(p: &Player, sneaking: bool) {
    let p = p.clone();
    let s = settings();
    if !p.has_permission("zenithbackpacks.use") {
        return tell(&p, "use.no-permission", &[]);
    }
    if s.world_blacklist.iter().any(|w| w == p.get_world().get_name()) {
        return tell(&p, "use.world-blocked", &[]);
    }
    if s.block_in_creative && p.get_game_mode() == "CREATIVE" {
        return tell(&p, "use.creative-blocked", &[]);
    }
    let rows = backpack_rows(&p.get_unique_id());
    if sneaking && p.has_permission("zenithbackpacks.upgrade") {
        if rows >= s.max_rows {
            return tell(&p, "upgrade.max-tier", &[]);
        }
        match next_row_step(&p, rows, &s) {
            Some(step) => open_backpack_upgrade(&p, &step, rows),
            None => tell(&p, "upgrade.none", &[]),
        }
        return;
    }
    play(&p, &s.sound_open, 1.0);
    open_backpack(&p);
}

// ---- Vault chests ----

fn chest_loaded(id: &str, owner: &str) {
    st(|s| {
        let folder = s.folder.clone();
        s.chests.entry(id.to_owned()).or_insert_with(|| files::load_chest(&folder, id, owner));
    });
}

fn chest_pages(id: &str) -> i32 {
    st(|s| s.chests.get(id).map_or(1, |c| c.pages))
}

fn save_chest(id: &str) {
    let job = st(|s| s.chests.get(id).map(|c| (files::chest_path(&s.folder, id), files::chest_text(c))));
    if let Some((path, text)) = job {
        files::save(path, text);
    }
}

fn unload_chest(id: &str) {
    save_chest(id);
    st(|s| {
        if !s.chest_open.contains_key(id) && !s.prompts.values().any(|c| c == id) {
            s.chests.remove(id);
        }
    });
}

fn save_placed() {
    let job = st(|s| (s.folder.join("placed-chests.yml"), files::placed_text(&s.placed)));
    files::save(job.0, job.1);
}

fn bind(id: &str, spot: Spot) {
    st(|s| {
        if let Some(old) = s.placed.insert(id.to_owned(), spot.clone()) {
            s.at.remove(&old);
        }
        s.at.insert(spot, id.to_owned());
    });
    save_placed();
}

fn unbind(id: &str) {
    st(|s| {
        if let Some(old) = s.placed.remove(id) {
            s.at.remove(&old);
        }
    });
    save_placed();
}

fn is_chest_block(b: &Block) -> bool {
    b.get_type().key() == "minecraft:chest"
}

fn chest_key(name: &str) -> horizon_bukkit::persistence::NamespacedKey {
    horizon_bukkit::persistence::NamespacedKey::from_string(&format!("zenithbackpacks:{name}")).expect("valid key")
}

/// The vault chest at a block: the index, else the block's own tag (a chest placed before the index knew it).
fn chest_at(b: &Block) -> Option<(String, String)> {
    if !is_chest_block(b) {
        return None;
    }
    let spot = Spot::of(b);
    if let Some(id) = st(|s| s.at.get(&spot).cloned()) {
        let owner = st(|s| s.chests.get(&id).map(|c| c.owner.clone()));
        let owner = owner.or_else(|| tile_uuid(b, "vault_chest_owner")).unwrap_or_default();
        return Some((id, owner));
    }
    let id = tile_uuid(b, "vault_chest_id")?;
    let owner = tile_uuid(b, "vault_chest_owner").unwrap_or_default();
    bind(&id, spot);
    Some((id, owner))
}

fn tile_uuid(b: &Block, name: &str) -> Option<String> {
    let mut tile = b.tile();
    let pdc = tile.get_persistent_data_container();
    ["zenithbackpacks", "zenithcore"].iter().find_map(|ns| {
        let key = horizon_bukkit::persistence::NamespacedKey::from_string(&format!("{ns}:{name}"))?;
        pdc.get(&key).and_then(|v| v.as_str().map(|s| s.trim_matches('"').to_owned())).filter(|s| s.len() == 36)
    })
}

/// The block's own tag, so the chest is known by its block too (as the Java kept it).
fn tag_block(b: &Block, id: &str, owner: &str) {
    let mut tile = b.tile();
    {
        let mut pdc = tile.get_persistent_data_container();
        pdc.set(&chest_key("vault_chest_id"), Value::String(id.to_owned()));
        pdc.set(&chest_key("vault_chest_owner"), Value::String(owner.to_owned()));
    }
    tile.update();
}

/// `facing`'s clockwise turn (north -> east).
fn clockwise(f: &str) -> &'static str {
    match f {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        _ => "north",
    }
}

fn counter_clockwise(f: &str) -> &'static str {
    match f {
        "north" => "west",
        "west" => "south",
        "south" => "east",
        _ => "north",
    }
}

fn prop<'a>(data: &'a str, key: &str) -> Option<&'a str> {
    let inner = data.split_once('[')?.1.trim_end_matches(']');
    inner.split(',').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
}

/// The chest this one is joined to, if it's half of a double chest.
fn partner(b: &Block) -> Option<Block> {
    let data = b.get_block_data();
    let facing = prop(&data, "facing")?;
    let dir = match prop(&data, "type")? {
        "left" => clockwise(facing),
        "right" => counter_clockwise(facing),
        _ => return None,
    };
    Some(b.get_relative_face(&dir.to_ascii_uppercase()))
}

fn make_single(b: &Block) {
    let data = b.get_block_data();
    if let Some(t) = prop(&data, "type").filter(|t| *t != "single") {
        b.set_block_data(&data.replace(&format!("type={t}"), "type=single"));
    }
}

/// A vault chest stays single: two ids can't share one window.
fn keep_single(b: &Block) {
    if !is_chest_block(b) {
        return;
    }
    let Some(other) = partner(b) else { return };
    let vault = |x: &Block| st(|s| s.at.contains_key(&Spot::of(x)));
    if vault(b) || vault(&other) {
        make_single(b);
        if is_chest_block(&other) {
            make_single(&other);
        }
    }
}

fn drop_vault_item(b: &Block, id: &str, owner: &str) {
    chest_loaded(id, owner);
    let s = settings();
    let pages = chest_pages(id);
    let owner = st(|st| st.chests.get(id).map(|c| c.owner.clone())).unwrap_or_else(|| owner.to_owned());
    let item = items::chest(&s, &owner, id, pages);
    let mut at = b.get_location();
    at.add(0.5, 0.5, 0.5);
    b.get_world().drop_item_naturally(&at, &item);
    unload_chest(id);
}

fn give(p: &Player, item: &ItemStack) {
    if let Some(rest) = p.get_inventory().add_item(item) {
        p.get_world().drop_item_naturally(&p.get_location(), &rest);
    }
}

/// The page window: 45 slots of the page, the control row below.
fn open_pages(p: &Player, id: &str, page: i32, query: &str) {
    let s = settings();
    let me = p.get_unique_id();
    let busy = st(|st| st.chest_open.get(id).filter(|v| **v != me).cloned());
    if let Some(other) = busy {
        let still = sessions(|m| m.get(&other).is_some_and(|x| matches!(&x.kind, Kind::Pages { chest, .. } | Kind::Search { chest, .. } if chest == id)));
        if still {
            return tell(p, "chest.sort-busy", &[]);
        }
    }
    let (pages, slots) = st(|st| {
        st.chest_open.insert(id.to_owned(), me.clone());
        let c = st.chests.get_mut(id)?;
        c.last_opened = horizon_kit::time::now_ms() as i64;
        let page = page.clamp(1, c.pages);
        let first = conf::first_slot(page);
        Some((c.pages, c.items[first..first + conf::SLOTS_PER_PAGE].to_vec()))
    })
    .unwrap_or((1, vec![None; conf::SLOTS_PER_PAGE]));
    let page = page.clamp(1, pages);
    let t = title(&s.gui("chest.pages.title"), &[("tier", s.tier_name(pages)), ("page", page.to_string()), ("max", pages.to_string())]);
    let mut inv = Inventory::create(conf::WINDOW_SIZE, &t);
    for (i, it) in slots.into_iter().enumerate() {
        if it.is_some() {
            inv.set_item(i, it);
        }
    }
    controls(&s, &mut inv, id, page, false, query);
    show(p, inv, Kind::Pages { chest: id.to_owned(), page });
}

fn open_results(p: &Player, id: &str, query: &str, map: Vec<usize>) {
    let s = settings();
    let me = p.get_unique_id();
    let slots: Vec<Option<ItemStack>> = st(|st| {
        st.chest_open.insert(id.to_owned(), me);
        st.chests.get(id).map(|c| map.iter().map(|i| c.items[*i].clone()).collect())
    })
    .unwrap_or_default();
    let t = title(&s.gui("chest.pages.search-title"), &[("query", query.to_owned()), ("count", map.len().to_string())]);
    let mut inv = Inventory::create(conf::WINDOW_SIZE, &t);
    for (i, it) in slots.into_iter().enumerate().take(conf::SLOTS_PER_PAGE) {
        if it.is_some() {
            inv.set_item(i, it);
        }
    }
    controls(&s, &mut inv, id, 1, true, query);
    show(p, inv, Kind::Search { chest: id.to_owned(), map });
}

const SLOT_PREV: usize = 45;
const SLOT_SORT: usize = 47;
const SLOT_SEARCH: usize = 49;
const SLOT_UPGRADE: usize = 51;
const SLOT_NEXT: usize = 53;

fn lines(s: &Settings, key: &str, pairs: &[(&str, String)]) -> Vec<Value> {
    conf::lore_lines(&s.gui_list(key), pairs)
}

fn controls(s: &Settings, inv: &mut Inventory, id: &str, page: i32, searching: bool, query: &str) {
    let (pages, used) = st(|st| st.chests.get(id).map(|c| (c.pages, c.items[..conf::capacity(c.pages)].iter().filter(|x| x.is_some()).count()))).unwrap_or((1, 0));
    let filler = items::filler(s);
    for slot in conf::SLOTS_PER_PAGE..conf::WINDOW_SIZE {
        inv.set_item(slot, Some(filler.clone()));
    }
    let nav = |which: &str, enabled: bool| {
        let name = conf::mm(&s.gui(&format!("chest.pages.{which}{}", if enabled { "" } else { "-disabled" })), &[]);
        let lore_key = if searching {
            "chest.pages.nav-searching-lore"
        } else if enabled {
            "chest.pages.nav-lore"
        } else {
            "chest.pages.nav-end-lore"
        };
        items::button(if enabled { "ARROW" } else { "GRAY_DYE" }, "ARROW", name, lines(s, lore_key, &[("page", page.to_string()), ("max", pages.to_string())]))
    };
    inv.set_item(SLOT_PREV, Some(nav("prev", !searching && page > 1)));
    inv.set_item(SLOT_NEXT, Some(nav("next", !searching && page < pages)));
    inv.set_item(SLOT_SORT, Some(items::button("HOPPER", "HOPPER", conf::mm(&s.gui("chest.pages.sort"), &[]), lines(s, "chest.pages.sort-lore", &[]))));
    let q = [("query", if query.trim().is_empty() { "-".to_owned() } else { query.to_owned() })];
    let (mat, name, lore) = if searching {
        ("BARRIER", "chest.pages.search-clear", "chest.pages.search-clear-lore")
    } else {
        ("SPYGLASS", "chest.pages.search", "chest.pages.search-lore")
    };
    inv.set_item(SLOT_SEARCH, Some(items::button(mat, mat, conf::mm(&s.gui(name), &q), lines(s, lore, &q))));
    let next = s.next_chest_step(pages);
    let maxed = next.is_none() || pages >= s.chest_max_pages;
    let next_pages = next.map_or(pages, |n| n.size);
    let cost = next.map_or(0.0, Step::money);
    let pairs = [
        ("tier", s.tier_name(next_pages)),
        ("pages", pages.to_string()),
        ("max", s.chest_max_pages.to_string()),
        ("next", next_pages.to_string()),
        ("nextslots", conf::capacity(next_pages).to_string()),
        ("used", used.to_string()),
        ("slots", conf::capacity(pages).to_string()),
        ("cost", thousands(cost)),
    ];
    let (mat, name, lore) = if maxed {
        ("NETHER_STAR", "chest.pages.upgrade-maxed", "chest.pages.upgrade-maxed-lore")
    } else {
        ("ANVIL", "chest.pages.upgrade", "chest.pages.upgrade-lore")
    };
    inv.set_item(SLOT_UPGRADE, Some(items::button(mat, mat, conf::mm(&s.gui(name), &pairs), lines(s, lore, &pairs))));
}

/// `%,.0f`: 25000 -> "25,000".
fn thousands(v: f64) -> String {
    let n = v.round() as i64;
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// The window's storage slots back into the chest (a search view by its map). A stack in a result slot with no
/// storage behind it (the click rules should stop that, but nothing may be lost) goes to the chest's first free
/// slot, or back to the player.
fn write_back(p: &Player, kind: &Kind, contents: &[Option<ItemStack>]) {
    let clean = |i: usize| contents.get(i).cloned().flatten().filter(|s| !s.is_empty());
    let strays: Vec<ItemStack> = st(|s| match kind {
        Kind::Pages { chest, page } => {
            if let Some(c) = s.chests.get_mut(chest) {
                let first = conf::first_slot(*page);
                for i in 0..conf::SLOTS_PER_PAGE {
                    c.items[first + i] = clean(i);
                }
            }
            Vec::new()
        }
        Kind::Search { chest, map } => {
            let mut strays: Vec<ItemStack> = (map.len()..conf::SLOTS_PER_PAGE).filter_map(clean).collect();
            if let Some(c) = s.chests.get_mut(chest) {
                for (i, at) in map.iter().enumerate().take(conf::SLOTS_PER_PAGE) {
                    c.items[*at] = clean(i);
                }
                let cap = conf::capacity(c.pages);
                strays.retain(|stack| match c.items[..cap].iter().position(Option::is_none) {
                    Some(free) => {
                        c.items[free] = Some(stack.clone());
                        false
                    }
                    None => true,
                });
            }
            strays
        }
        Kind::Backpack => Vec::new(),
    });
    for stack in strays {
        give(p, &stack);
    }
}

fn chest_of(kind: &Kind) -> Option<String> {
    match kind {
        Kind::Pages { chest, .. } | Kind::Search { chest, .. } => Some(chest.clone()),
        Kind::Backpack => None,
    }
}

/// A control row click, a tick later, with what the window held when clicked.
fn control(p: &Player, kind: Kind, slot: usize, contents: Vec<Option<ItemStack>>) {
    let me = p.get_unique_id();
    // Still the window that was clicked?
    if !sessions(|m| m.get(&me).is_some_and(|s| chest_of(&s.kind) == chest_of(&kind))) {
        return;
    }
    let Some(id) = chest_of(&kind) else { return };
    let searching = matches!(kind, Kind::Search { .. });
    let page = if let Kind::Pages { page, .. } = kind { page } else { 1 };
    match slot {
        SLOT_PREV | SLOT_NEXT => {
            if searching {
                return;
            }
            let target = page + if slot == SLOT_NEXT { 1 } else { -1 };
            if target < 1 || target > chest_pages(&id) {
                return;
            }
            write_back(p, &kind, &contents);
            open_pages(p, &id, target, "");
        }
        SLOT_UPGRADE => {
            let s = settings();
            let pages = chest_pages(&id);
            let Some(step) = s.next_chest_step(pages).cloned() else {
                return tell(p, "chest.upgrade-max", &[]);
            };
            write_back(p, &kind, &contents);
            leave_chest(&me, &id);
            save_chest(&id);
            let chest = id.clone();
            let m = confirm_menu("chest.upgrade.title", "chest.upgrade.confirm-name", pages_text(pages, "aqua"), pages_text(step.size, "green"), step.money(), move |p| try_chest_upgrade(p, &chest));
            sessions(|m| m.remove(&me));
            m.open(p);
        }
        SLOT_SORT => {
            write_back(p, &kind, &contents);
            st(|s| {
                if let Some(c) = s.chests.get_mut(&id) {
                    let cap = conf::capacity(c.pages);
                    sort::sort(&mut c.items, cap);
                }
            });
            save_chest(&id);
            open_pages(p, &id, page, "");
        }
        SLOT_SEARCH => {
            write_back(p, &kind, &contents);
            save_chest(&id);
            if searching {
                return open_pages(p, &id, 1, "");
            }
            st(|s| s.prompts.insert(me.clone(), id.clone()));
            sessions(|m| m.remove(&me));
            leave_chest(&me, &id);
            p.close_inventory();
            tell(p, "chest.search-prompt", &[]);
        }
        _ => {}
    }
}

fn leave_chest(me: &str, id: &str) {
    st(|s| {
        if s.chest_open.get(id).is_some_and(|v| v == me) {
            s.chest_open.remove(id);
        }
    });
}

/// The typed word, on the server thread.
fn search_typed(p: &Player, id: &str, typed: &str) {
    let t = typed.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("cancel") || t.eq_ignore_ascii_case("exit") {
        return open_pages(p, id, 1, "");
    }
    let hits = st(|s| s.chests.get(id).map(|c| sort::find(&c.items, conf::capacity(c.pages), t, conf::SLOTS_PER_PAGE))).unwrap_or_default();
    if hits.is_empty() {
        tell(p, "chest.search-empty", &[]);
        return open_pages(p, id, 1, "");
    }
    open_results(p, id, t, hits);
}

fn try_chest_upgrade(p: &Player, id: &str) {
    let s = settings();
    let pages = chest_pages(id);
    let Some(step) = s.next_chest_step(pages).cloned() else {
        tell(p, "chest.upgrade-max", &[]);
        p.close_inventory();
        return;
    };
    let bypass = p.has_permission("zenithbackpacks.bypass");
    if let Some(fail) = check(p, &step.reqs, bypass) {
        if let Some(v) = fail {
            p.send_component(&Component(v));
        }
        return;
    }
    if !pay(p, &step, bypass) {
        tell(p, "general.economy-required", &[]);
        return;
    }
    st(|st| {
        if let Some(c) = st.chests.get_mut(id) {
            c.pages = conf::clamp_pages(step.size);
        }
    });
    save_chest(id);
    play(p, &s.sound_upgrade, 1.2);
    tell(p, "chest.upgrade-success", &[]);
    let (p2, id2) = (p.clone(), id.to_owned());
    next_tick(move || open_pages(&p2, &id2, step.size, ""));
}

// ---- Events ----

fn on_chest_use(e: &mut PlayerInteractEvent) {
    if e.get_hand() != Some("HAND") || e.get_action() != "RIGHT_CLICK_BLOCK" {
        return;
    }
    let Some(b) = e.get_clicked_block().cloned() else { return };
    let Some((id, owner)) = chest_at(&b) else { return };
    e.set_cancelled(true);
    e.set_use_interacted_block("DENY");
    e.set_use_item_in_hand("DENY");
    let p = e.get_player().clone();
    next_tick(move || {
        let owner = if owner.is_empty() { p.get_unique_id() } else { owner };
        if owner != p.get_unique_id() && !p.has_permission("zenithbackpacks.chest.bypass") {
            return tell(&p, "chest.not-owner", &[]);
        }
        chest_loaded(&id, &owner);
        play(&p, &settings().sound_open, 1.0);
        open_pages(&p, &id, 1, "");
    });
}

fn on_place(e: &mut BlockPlaceEvent) {
    let item = e.get_item_in_hand().cloned();
    if items::is_backpack(item.as_ref()) {
        e.set_cancelled(true);
        return;
    }
    let placed = e.get_block_placed().clone();
    let Some(id) = items::chest_id(item.as_ref()) else {
        if is_chest_block(&placed) && st(|s| !s.at.is_empty()) {
            next_tick(move || keep_single(&placed));
        }
        return;
    };
    let p = e.get_player().clone();
    if !settings().chest_enabled {
        e.set_cancelled(true);
        return;
    }
    // One id in two places means the item was copied.
    if let Some(spot) = st(|s| s.placed.get(&id).cloned()) {
        let world = horizon_bukkit::get_world(&spot.world);
        let still = world.as_ref().is_none_or(|w| {
            // An unloaded chunk is trusted rather than loaded to answer.
            let loc = Location::new(w.clone(), spot.x as f64, spot.y as f64, spot.z as f64, 0.0, 0.0);
            !loc.is_chunk_loaded() || is_chest_block(&w.get_block_at(spot.x, spot.y, spot.z))
        });
        if still {
            e.set_cancelled(true);
            let p2 = p.clone();
            next_tick(move || tell(&p2, "chest.already-placed", &[]));
            if let Some(pl) = plugin() {
                pl.warning(&format!("Refused duplicate vault chest {id} placed by {}; original at {} {} {} {}", p.get_name(), spot.world, spot.x, spot.y, spot.z));
            }
            return;
        }
        unbind(&id);
    }
    let owner = items::chest_owner(item.as_ref()).unwrap_or_else(|| p.get_unique_id());
    bind(&id, Spot::of(&placed));
    chest_loaded(&id, &owner);
    next_tick(move || {
        if is_chest_block(&placed) {
            keep_single(&placed);
            make_single(&placed);
            tag_block(&placed, &id, &owner);
        } else {
            // Another plugin stopped the placement after us.
            unbind(&id);
        }
    });
}

fn on_break(e: &mut BlockBreakEvent) {
    let b = e.get_block().clone();
    if !is_chest_block(&b) {
        return;
    }
    let Some(id) = st(|s| s.at.get(&Spot::of(&b)).cloned()) else { return };
    let owner = st(|s| s.chests.get(&id).map(|c| c.owner.clone())).or_else(|| tile_uuid(&b, "vault_chest_owner")).unwrap_or_default();
    e.set_drop_items(false);
    drop_vault_item(&b, &id, &owner);
    unbind(&id);
}

/// An explosion drops the vault item instead of deleting the chest. The index is asked first (no block query
/// for the hundreds of positions a blast lists), and the item drops a tick later: the blast hurts entities after
/// this event here, so an item dropped now would be blown up with the rest.
fn exploded(blocks: &mut Vec<Block>) {
    if st(|s| s.at.is_empty()) {
        return;
    }
    let mut keep = Vec::with_capacity(blocks.len());
    for b in blocks.drain(..) {
        let id = st(|s| s.at.get(&Spot::of(&b)).cloned()).filter(|_| is_chest_block(&b));
        match id {
            Some(id) => {
                let owner = st(|s| s.chests.get(&id).map(|c| c.owner.clone())).unwrap_or_default();
                unbind(&id);
                b.set_type(&Material::of("AIR"));
                next_tick(move || drop_vault_item(&b, &id, &owner));
            }
            None => keep.push(b),
        }
    }
    *blocks = keep;
}

/// Hoppers can't reach the block's own (unused) inventory.
fn on_hopper(e: &mut InventoryMoveItemEvent) {
    if st(|s| s.at.is_empty()) {
        return;
    }
    let vault = |m: Option<horizon_bukkit::fam_ev_player::MenuInventory>| {
        m.and_then(|m| m.get_location().map(|l| Spot { world: l.get_world().get_name().to_owned(), x: l.get_block_x(), y: l.get_block_y(), z: l.get_block_z() }))
            .is_some_and(|spot| st(|s| s.at.contains_key(&spot)))
    };
    if vault(e.get_destination()) || vault(e.get_source()) {
        e.set_cancelled(true);
    }
}

fn on_click(e: &mut InventoryClickEvent) {
    let Some(p) = e.get_who_clicked() else { return };
    let gui = e.get_inventory().gui().cloned();
    let Some(kind) = session_of(&p, gui.as_ref()) else { return };
    if matches!(kind, Kind::Backpack) {
        return;
    }
    let raw = e.get_raw_slot();
    let in_top = (0..conf::WINDOW_SIZE as i32).contains(&raw);
    if in_top && raw as usize >= conf::SLOTS_PER_PAGE {
        e.set_cancelled(true);
        let contents = e.get_inventory().get_contents();
        let slot = raw as usize;
        let p2 = p.clone();
        next_tick(move || control(&p2, kind, slot, contents));
        return;
    }
    if let Kind::Search { map, .. } = &kind {
        // Results are take-out only: a stack put in has no page of its own. Only a result still there may be
        // clicked (the carried stack isn't always known here, so an empty slot refuses everything).
        let cursor = e.get_cursor().filter(|c| !c.is_empty());
        let keys = matches!(e.get_click(), "NUMBER_KEY" | "SWAP_OFFHAND");
        let there = e.get_current_item().is_some_and(|c| !c.is_empty());
        let refused = in_top && (raw as usize >= map.len() || !there || cursor.is_some() || keys);
        if refused || (!in_top && e.is_shift_click()) {
            e.set_cancelled(true);
        }
    }
}

fn on_drag(e: &mut InventoryDragEvent) {
    let Some(p) = e.get_who_clicked() else { return };
    let gui = e.get_inventory().gui().cloned();
    let Some(kind) = session_of(&p, gui.as_ref()) else { return };
    let hit = |limit: usize, from: usize| e.get_raw_slots().iter().any(|s| *s >= from as i32 && (*s as usize) < limit);
    let stop = match kind {
        Kind::Backpack => false,
        Kind::Pages { .. } => hit(conf::WINDOW_SIZE, conf::SLOTS_PER_PAGE),
        Kind::Search { .. } => hit(conf::WINDOW_SIZE, 0),
    };
    if stop {
        e.set_cancelled(true);
    }
}

fn closed(p: &Player, kind: Kind, contents: &[Option<ItemStack>]) {
    match &kind {
        Kind::Backpack => backpack_closed(p, contents),
        Kind::Pages { chest, .. } | Kind::Search { chest, .. } => {
            write_back(p, &kind, contents);
            leave_chest(&p.get_unique_id(), chest);
            unload_chest(chest);
        }
    }
}

fn on_close(e: &mut InventoryCloseEvent) {
    let Some(p) = e.get_player() else { return };
    let inv = e.get_inventory();
    let gui = inv.gui().cloned();
    let Some(kind) = session_of(&p, gui.as_ref()) else { return };
    sessions(|m| m.remove(&p.get_unique_id()));
    closed(&p, kind, &inv.get_contents());
}

fn on_quit(e: &mut PlayerQuitEvent) {
    let p = e.get_player().clone();
    let me = p.get_unique_id();
    if let Some(s) = sessions(|m| m.remove(&me)) {
        let contents = s.inv.live().get_contents().to_vec();
        closed(&p, s.kind, &contents);
    }
    let prompt = st(|s| s.prompts.remove(&me));
    if let Some(id) = prompt {
        unload_chest(&id);
    }
    save_backpack(&me);
    st(|s| s.backpacks.remove(&me));
}

fn on_chat(e: &mut AsyncChatEvent) {
    let me = e.get_player().get_unique_id();
    let Some(id) = st(|s| s.prompts.remove(&me)) else { return };
    e.set_cancelled(true);
    let typed = e.message().plain().trim().to_owned();
    let p = e.get_player().clone();
    sync::on_main(move || search_typed(&p, &id, &typed));
}

/// The tier icons' pack, sent on top of any other (its own id, so it never replaces the server's).
const PACK_ID: &str = "5a1f0c3e2b7d4c919e4a7b3d2f6a8c10";

fn send_pack(p: &Player) {
    let Some((url, sha1, required, prompt)) = settings().pack.clone() else { return };
    let prompt = (!prompt.trim().is_empty()).then(|| Component(conf::mm(&prompt, &[])));
    p.add_resource_pack(PACK_ID, &url, &sha1, prompt.as_ref(), required);
}

// ---- Commands ----

fn admin(sender: &CommandSender, args: &[&str]) -> bool {
    if !sender.has_permission("zenithbackpacks.admin") {
        tell_s(sender, "admin.no-permission", &[]);
        return true;
    }
    let sub = args.first().map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let online = |name: &str| horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name));
    match sub.as_str() {
        "reload" => {
            reload();
            // A changed icon pack (new sha1) reaches everyone on now, not at their next join.
            for p in horizon_bukkit::get_online_players() {
                send_pack(&p);
            }
            tell_s(sender, "admin.reload-done", &[]);
        }
        "give" | "givechest" => {
            let Some(name) = args.get(1) else {
                tell_s(sender, "admin.give-usage", &[]);
                return true;
            };
            let Some(target) = online(name) else {
                tell_s(sender, "admin.player-offline", &[]);
                return true;
            };
            let s = settings();
            if sub == "give" {
                let rows = backpack_rows(&target.get_unique_id());
                give(&target, &items::backpack(&s, rows));
                tell_s(sender, "admin.gave", &[("player", target.get_name().to_owned())]);
            } else {
                let id = new_uuid();
                give(&target, &items::chest(&s, &target.get_unique_id(), &id, s.chest_starting_pages));
                tell_s(sender, "admin.gave-chest", &[("player", target.get_name().to_owned())]);
            }
        }
        "chestpurge" => {
            let Some(days) = args.get(1).and_then(|d| d.parse::<i64>().ok()) else {
                tell_s(sender, "admin.usage", &[]);
                return true;
            };
            // A dry run unless told otherwise: getting this wrong deletes storage.
            let dry = !args.get(2).is_some_and(|a| a.eq_ignore_ascii_case("--confirm"));
            let cutoff = horizon_kit::time::now_ms() as i64 - days * 86_400_000;
            let n = purge(cutoff, dry);
            tell_s(sender, "chest.purge-result", &[("count", n.to_string()), ("mode", if dry { "would be removed" } else { "removed" }.to_owned())]);
        }
        _ => tell_s(sender, "admin.usage", &[]),
    }
    true
}

/// Chest files nobody opened since `cutoff` (in use ones skipped).
fn purge(cutoff: i64, dry: bool) -> usize {
    let (dir, live): (PathBuf, Vec<String>) = st(|s| (files::chest_dir(&s.folder), s.chests.keys().cloned().collect()));
    let mut n = 0;
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(id) = path.file_name().and_then(|f| f.to_str()).and_then(|f| f.strip_suffix(".yml")) else { continue };
        if id.len() != 36 || live.iter().any(|l| l == id) || files::last_opened(&path) > cutoff {
            continue;
        }
        n += 1;
        if !dry && std::fs::remove_file(&path).is_err()
            && let Some(p) = plugin()
        {
            p.warning(&format!("Could not delete orphaned chest {id}"));
        }
    }
    n
}

/// A random (v4) uuid: the OS-seeded hasher keys, mixed with the time and a counter.
fn new_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    let half = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(nanos ^ salt);
        h.write_u64(COUNT.fetch_add(1, Ordering::Relaxed));
        h.finish()
    };
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&half(0x9e37_79b9_7f4a_7c15).to_be_bytes());
    b[8..].copy_from_slice(&half(0xc2b2_ae3d_27d4_eb4f).to_be_bytes());
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|v| format!("{v:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn admin_complete(sender: &CommandSender, args: &[&str]) -> Vec<String> {
    if !sender.has_permission("zenithbackpacks.admin") {
        return Vec::new();
    }
    match args {
        [a] => ["reload", "give", "givechest", "chestpurge"].iter().filter(|s| s.starts_with(&a.to_ascii_lowercase())).map(|s| (*s).to_owned()).collect(),
        [sub, name] if sub.eq_ignore_ascii_case("give") || sub.eq_ignore_ascii_case("givechest") => {
            // HorizonsX asks this inside the player's own swap, where the list leaves them out.
            let mut names: Vec<String> = horizon_bukkit::get_online_players().into_iter().map(|p| p.get_name().to_owned()).collect();
            if let Some(me) = sender.as_player() {
                names.push(me.get_name().to_owned());
            }
            names.sort();
            names.dedup();
            names.retain(|n| n.to_lowercase().starts_with(&name.to_lowercase()));
            names
        }
        _ => Vec::new(),
    }
}

fn user(sender: &CommandSender) -> bool {
    let Some(p) = sender.as_player() else {
        tell_s(sender, "command.players-only", &[]);
        return true;
    };
    if !p.has_permission("zenithbackpacks.use") {
        tell(&p, "use.no-permission", &[]);
        return true;
    }
    for key in ["command.help.line1", "command.help.line2", "command.help.line3"] {
        tell(&p, key, &[]);
    }
    true
}

// ---- Start, reload, stop ----

/// config.yml, gui.yml and messages.yml read again; cached backpacks clamped to the new max.
pub fn reload() {
    let Some(p) = plugin() else { return };
    let folder = p.get_data_folder();
    let s = Settings::load(&folder);
    let max = s.max_rows;
    let changed: Vec<String> = st(|st| {
        st.settings = Arc::new(s);
        st.folder = folder;
        let mut out = Vec::new();
        for (uuid, b) in &mut st.backpacks {
            if b.rows > max {
                b.rows = max;
                for slot in b.items.iter_mut().skip(max as usize * 9) {
                    *slot = None;
                }
                out.push(uuid.clone());
            }
        }
        out
    });
    for uuid in changed {
        save_backpack(&uuid);
    }
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    horizon_kit::app::set(plugin);
    reload();
    files::remember_data_version();
    let placed = files::load_placed(&plugin.get_data_folder());
    st(|s| {
        s.at = placed.iter().map(|(id, spot)| (spot.clone(), id.clone())).collect();
        s.placed = placed;
    });
    gui::install(plugin);
    plugin.run_task_timer(1, 1, sync::drain);
    plugin.register_event_with("LOWEST", false, on_backpack_use);
    plugin.register_event_with("HIGH", true, on_chest_use);
    plugin.register_event_with("HIGH", true, on_place);
    plugin.register_event_with("HIGH", true, on_break);
    plugin.register_event_with("NORMAL", true, |e: &mut EntityExplodeEvent| exploded(e.block_list()));
    plugin.register_event_with("NORMAL", true, |e: &mut BlockExplodeEvent| exploded(e.block_list()));
    plugin.register_event_with("NORMAL", true, on_hopper);
    plugin.register_event_with("NORMAL", true, on_click);
    plugin.register_event_with("NORMAL", true, on_drag);
    plugin.register_event_with("NORMAL", false, on_close);
    plugin.register_event_with("NORMAL", false, on_quit);
    plugin.register_event_with("LOWEST", false, on_chat);
    plugin.register_event_with("MONITOR", false, |e: &mut PlayerJoinEvent| {
        let p = e.get_player().clone();
        next_tick(move || send_pack(&p));
    });
    // Players already on (a hot reload) get it now, not at their next join.
    next_tick(|| {
        for p in horizon_bukkit::get_online_players() {
            send_pack(&p);
        }
    });
    // The work runs a tick later: HorizonsX runs a player's command inside a swap where that player is the
    // server's own, so the online list leaves them out and what they're sent comes back without its colours.
    plugin.set_executor("zenithbackpacks", |s, _, _| {
        let s = s.clone();
        next_tick(move || {
            user(&s);
        });
        true
    });
    plugin.set_tab_completer("zenithbackpacks", |_, _, _| Vec::new());
    plugin.set_executor("zenithbackpacksadmin", |s, _, a| {
        let s = s.clone();
        let args: Vec<String> = a.iter().map(|x| (*x).to_owned()).collect();
        next_tick(move || {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            admin(&s, &refs);
        });
        true
    });
    plugin.set_tab_completer("zenithbackpacksadmin", |s, _, a| admin_complete(s, a));
    plugin.info("ZenithBackpacks enabled.");
}

fn on_disable(_plugin: Plugin) {
    // Open windows written back first (a reload or stop with players inside).
    let open: Vec<(String, Session)> = sessions(|m| m.drain().collect());
    for (uuid, s) in open {
        if let Some(p) = horizon_bukkit::fam_ess::get_player_by_id(&uuid) {
            let contents = s.inv.live().get_contents().to_vec();
            closed(&p, s.kind, &contents);
            p.close_inventory();
        }
    }
    gui::close_all();
    let (chests, packs): (Vec<String>, Vec<String>) = st(|s| (s.chests.keys().cloned().collect(), s.backpacks.keys().cloned().collect()));
    for id in chests {
        save_chest(&id);
    }
    for uuid in packs {
        save_backpack(&uuid);
    }
    save_placed();
    files::flush();
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
