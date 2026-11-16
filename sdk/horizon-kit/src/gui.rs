//! Chest menus: build, open, route clicks, page through lists. One `install` per plugin registers the
//! listeners once; every menu after that is a builder.
//!
//! ```ignore
//! horizon_kit::gui::install(plugin);                       // on_enable, once
//! let mut m = gui::Menu::new(6, "&6Shop");
//! m.fill_border(gui::filler("GRAY_STAINED_GLASS_PANE"));
//! m.button(22, diamond, |c| { c.player.send_message("bought"); c.close(); });
//! m.open(&player);
//! ```
//!
//! Clicks in a kit menu are always cancelled (nothing moves in or out, drags too) unless the menu allows it
//! (`Menu::allow_take`). A button runs on the server thread on the next tick, after the click event is over, so
//! it may open another menu, close this one or run commands safely. [`Pages`] lays a list over content slots
//! with previous/next buttons.

use horizon_bukkit::fam_ev_player::{InventoryClickEvent, InventoryCloseEvent, InventoryDragEvent};
use horizon_bukkit::event::PlayerQuitEvent;
use horizon_bukkit::{Inventory, ItemStack, Material, Player, Plugin};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// How a slot was clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Left,
    Right,
    ShiftLeft,
    ShiftRight,
    Middle,
    Drop,
    /// Hotbar key 1-9 (0-based).
    Number(i32),
    Other,
}

impl Click {
    fn of(event: &InventoryClickEvent) -> Click {
        match event.get_click() {
            "LEFT" => Click::Left,
            "RIGHT" => Click::Right,
            "SHIFT_LEFT" => Click::ShiftLeft,
            "SHIFT_RIGHT" => Click::ShiftRight,
            "MIDDLE" => Click::Middle,
            "DROP" | "CONTROL_DROP" => Click::Drop,
            "NUMBER_KEY" => Click::Number(event.get_hotbar_button()),
            _ => Click::Other,
        }
    }
    pub fn is_left(self) -> bool {
        matches!(self, Click::Left | Click::ShiftLeft)
    }
    pub fn is_right(self) -> bool {
        matches!(self, Click::Right | Click::ShiftRight)
    }
    pub fn is_shift(self) -> bool {
        matches!(self, Click::ShiftLeft | Click::ShiftRight)
    }
}

/// What a button gets: who, where, how.
pub struct Ctx {
    pub player: Player,
    pub slot: usize,
    pub click: Click,
}

impl Ctx {
    pub fn close(&self) {
        self.player.close_inventory();
    }
    /// Change one slot of the menu the player has open (it updates on their screen).
    pub fn set(&self, slot: usize, item: Option<ItemStack>) {
        set_slot(&self.player, slot, item);
    }
}

type Action = Arc<dyn Fn(&Ctx) + Send + Sync>;
type CloseFn = Arc<dyn Fn(&Player) + Send + Sync>;
type CloseItemsFn = Arc<dyn Fn(&Player, Vec<Option<ItemStack>>) + Send + Sync>;

/// A menu being built (and, once open, its slots' actions).
pub struct Menu {
    inv: Inventory,
    actions: HashMap<usize, Action>,
    on_close: Option<CloseFn>,
    on_close_items: Option<CloseItemsFn>,
    allow_take: bool,
    /// A key a plugin may use to tell its menus apart (`"auction:page:2"`).
    pub tag: String,
}

impl Menu {
    /// `rows` 1-6; the title takes `&` colours and hex.
    pub fn new(rows: usize, title: &str) -> Menu {
        let rows = rows.clamp(1, 6);
        Menu { inv: Inventory::create(rows * 9, &crate::text::colorize(title)), actions: HashMap::new(), on_close: None, on_close_items: None, allow_take: false, tag: String::new() }
    }
    /// A menu whose title is already a text component as JSON (from [`crate::ui::Title::json`]): taken as it is,
    /// without the colour codes [`Menu::new`] turns into text (which would break `#rrggbb` colours in it).
    pub fn raw(rows: usize, title_json: &str) -> Menu {
        let rows = rows.clamp(1, 6);
        Menu { inv: Inventory::create(rows * 9, title_json), actions: HashMap::new(), on_close: None, on_close_items: None, allow_take: false, tag: String::new() }
    }
    pub fn size(&self) -> usize {
        self.inv.get_size()
    }
    pub fn item(&mut self, slot: usize, item: ItemStack) -> &mut Self {
        if slot < self.size() {
            self.inv.set_item(slot, Some(item));
        }
        self
    }
    /// An item that does something when clicked.
    pub fn button(&mut self, slot: usize, item: ItemStack, on_click: impl Fn(&Ctx) + Send + Sync + 'static) -> &mut Self {
        if slot < self.size() {
            self.inv.set_item(slot, Some(item));
            self.actions.insert(slot, Arc::new(on_click));
        }
        self
    }
    /// A click action on a slot without changing its item.
    pub fn on(&mut self, slot: usize, on_click: impl Fn(&Ctx) + Send + Sync + 'static) -> &mut Self {
        self.actions.insert(slot, Arc::new(on_click));
        self
    }
    /// Every empty slot.
    pub fn fill(&mut self, item: ItemStack) -> &mut Self {
        for s in 0..self.size() {
            if self.inv.get_item(s).is_none() {
                self.inv.set_item(s, Some(item.clone()));
            }
        }
        self
    }
    /// The outer ring of slots.
    pub fn fill_border(&mut self, item: ItemStack) -> &mut Self {
        let rows = self.size() / 9;
        for s in 0..self.size() {
            let (r, c) = (s / 9, s % 9);
            if (r == 0 || r == rows - 1 || c == 0 || c == 8) && self.inv.get_item(s).is_none() {
                self.inv.set_item(s, Some(item.clone()));
            }
        }
        self
    }
    pub fn on_close(&mut self, f: impl Fn(&Player) + Send + Sync + 'static) -> &mut Self {
        self.on_close = Some(Arc::new(f));
        self
    }
    /// A storage menu's contents as the player left them, when it closes (a sell chest, a trade window).
    /// Runs on the next tick, so it may give items back or open another menu.
    pub fn on_close_items(&mut self, f: impl Fn(&Player, Vec<Option<ItemStack>>) + Send + Sync + 'static) -> &mut Self {
        self.on_close_items = Some(Arc::new(f));
        self
    }
    /// Let players move items in and out (a storage menu); buttons still run.
    pub fn allow_take(&mut self, allow: bool) -> &mut Self {
        self.allow_take = allow;
        self
    }
    pub fn tag(&mut self, tag: &str) -> &mut Self {
        self.tag = tag.to_owned();
        self
    }
    /// The inventory (to read back a storage menu's contents on close).
    pub fn inventory(&self) -> &Inventory {
        &self.inv
    }
    /// Opens it for the player (replacing any kit menu they had open).
    pub fn open(self, player: &Player) {
        player.open_inventory(&self.inv);
        open_menus().lock().unwrap_or_else(|e| e.into_inner()).insert(player.get_unique_id(), self);
    }
}

/// A plain item with no name: a filler pane.
pub fn filler(material: &str) -> ItemStack {
    let mut s = ItemStack::new(Material::of(material), 1);
    crate::item::name(&mut s, " ");
    s
}

/// An item with a coloured name and lore, the most common button.
pub fn icon(material: &str, name: &str, lore: &[&str]) -> ItemStack {
    let mut s = ItemStack::new(Material::get_material(&material.to_ascii_uppercase()).unwrap_or_else(|| Material::of("STONE")), 1);
    crate::item::name(&mut s, name);
    if !lore.is_empty() {
        let lines: Vec<String> = lore.iter().map(|l| (*l).to_owned()).collect();
        crate::item::lore(&mut s, &lines);
    }
    s
}

static PLUGIN: OnceLock<Mutex<Option<Plugin>>> = OnceLock::new();
static OPEN: OnceLock<Mutex<HashMap<String, Menu>>> = OnceLock::new();

fn open_menus() -> &'static Mutex<HashMap<String, Menu>> {
    OPEN.get_or_init(|| Mutex::new(HashMap::new()))
}

fn plugin() -> Option<Plugin> {
    *PLUGIN.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|e| e.into_inner())
}

/// The tag of the kit menu the player has open, if any.
pub fn open_tag(player: &Player) -> Option<String> {
    open_menus().lock().unwrap_or_else(|e| e.into_inner()).get(&player.get_unique_id()).map(|m| m.tag.clone())
}

/// Changes one slot of the menu the player has open.
pub fn set_slot(player: &Player, slot: usize, item: Option<ItemStack>) {
    if let Some(m) = open_menus().lock().unwrap_or_else(|e| e.into_inner()).get_mut(&player.get_unique_id())
        && slot < m.inv.get_size()
    {
        m.inv.set_item(slot, item);
    }
}

/// The contents of the menu the player has open (storage menus).
pub fn contents(player: &Player) -> Option<Vec<Option<ItemStack>>> {
    open_menus().lock().unwrap_or_else(|e| e.into_inner()).get(&player.get_unique_id()).map(|m| m.inv.get_contents().to_vec())
}

fn on_click(event: &mut InventoryClickEvent) {
    let Some(player) = event.get_who_clicked() else { return };
    let uuid = player.get_unique_id();
    let found = {
        let menus = open_menus().lock().unwrap_or_else(|e| e.into_inner());
        let Some(m) = menus.get(&uuid) else { return };
        // A click in their own inventory below a kit menu: shift-clicks would move items into it.
        let in_menu = event.get_inventory().gui().is_some_and(|g| m.inv.same_as(g));
        if !in_menu {
            return;
        }
        let raw = usize::try_from(event.get_raw_slot()).ok();
        let top = raw.filter(|r| *r < m.inv.get_size());
        (m.allow_take, top.and_then(|s| m.actions.get(&s).cloned().map(|a| (s, a))), top.is_some())
    };
    let (allow, action, in_top) = found;
    // A locked menu takes nothing (clicks below it too: a shift-click would move items in); a storage menu
    // only stops clicks on its buttons.
    if !allow || (in_top && action.is_some()) {
        event.set_cancelled(true);
    }
    if let Some((slot, action)) = action {
        let click = Click::of(event);
        let ctx = Ctx { player, slot, click };
        match plugin() {
            Some(p) => {
                p.run_task(move || action(&ctx));
            }
            None => action(&ctx),
        }
    }
}

fn on_drag(event: &mut InventoryDragEvent) {
    let Some(player) = event.get_who_clicked() else { return };
    let menus = open_menus().lock().unwrap_or_else(|e| e.into_inner());
    let Some(m) = menus.get(&player.get_unique_id()) else { return };
    if m.allow_take {
        return;
    }
    let size = m.inv.get_size() as i32;
    if event.get_raw_slots().iter().any(|s| *s < size) {
        event.set_cancelled(true);
    }
}

fn closed(uuid: &str, player: Option<&Player>, items: Option<Vec<Option<ItemStack>>>) {
    let menu = open_menus().lock().unwrap_or_else(|e| e.into_inner()).remove(uuid);
    let (Some(m), Some(p)) = (menu, player) else { return };
    if let Some(f) = m.on_close_items {
        let items = items.unwrap_or_else(|| m.inv.get_contents().to_vec());
        let p2 = p.clone();
        match plugin() {
            Some(pl) => {
                pl.run_task(move || f(&p2, items.clone()));
            }
            None => f(&p2, items),
        }
    }
    if let Some(f) = m.on_close {
        f(p);
    }
}

/// Registers the menu listeners (once per plugin, in `on_enable`).
pub fn install(plugin: Plugin) {
    *PLUGIN.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    plugin.register_event_with("LOWEST", false, on_click);
    plugin.register_event_with("LOWEST", false, on_drag);
    plugin.register_event_with("MONITOR", false, |e: &mut InventoryCloseEvent| {
        if let Some(p) = e.get_player() {
            // Only when the closed window is the kit menu (opening a new menu closes the old one first).
            let ours = { open_menus().lock().unwrap_or_else(|e| e.into_inner()).get(&p.get_unique_id()).is_some_and(|m| e.get_inventory().gui().is_some_and(|g| m.inv.same_as(g))) };
            if ours {
                let items = e.get_inventory().get_contents().to_vec();
                closed(&p.get_unique_id(), Some(&p), Some(items));
            }
        }
    });
    plugin.register_event_with("MONITOR", false, |e: &mut PlayerQuitEvent| {
        let p = e.get_player().clone();
        closed(&p.get_unique_id(), Some(&p), None);
    });
}

/// Every kit menu closed (reload, disable).
pub fn close_all() {
    let all: Vec<String> = open_menus().lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect();
    for uuid in all {
        if let Some(p) = horizon_bukkit::fam_ess::get_player_by_id(&uuid) {
            p.close_inventory();
        }
    }
    open_menus().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Columns (1-7, inside a border) for `n` buttons in one row, centred and mirror-even (an even count leaves the
/// middle free): `row * 9 + col` is the slot. A menu that hides buttons for missing plugins stays tidy.
pub fn centered(n: usize) -> Vec<usize> {
    match n.min(7) {
        0 => vec![],
        1 => vec![4],
        2 => vec![3, 5],
        3 => vec![3, 4, 5],
        4 => vec![2, 3, 5, 6],
        5 => vec![2, 3, 4, 5, 6],
        6 => vec![1, 2, 3, 5, 6, 7],
        _ => vec![1, 2, 3, 4, 5, 6, 7],
    }
}

/// A yes/no menu (3 rows): `yes` runs when they confirm; "no" just closes it.
pub fn confirm(player: &Player, question: &str, yes: impl Fn(&Player) + Send + Sync + 'static) {
    let mut m = Menu::new(3, &format!("&8{question}"));
    m.fill_border(filler("BLACK_STAINED_GLASS_PANE"));
    m.button(11, icon("LIME_CONCRETE", "<#4ade80><bold>Yes, do it", &[]), move |c| {
        c.close();
        yes(&c.player);
    });
    m.item(13, icon("PAPER", &format!("<white><bold>{question}"), &["<#71717a>This can't be undone."]));
    m.button(15, icon("RED_CONCRETE", "<#f87171><bold>No, go back", &[]), |c| c.close());
    m.open(player);
}

/// `9-17`, `0,8,45`, `10-16,19-25` -> slot numbers (zMenu/DeluxeMenus slot lists).
pub fn slots(spec: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let p = part.trim();
        if let Some((a, b)) = p.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>()) {
                out.extend(a.min(b)..=a.max(b));
            }
        } else if let Ok(n) = p.parse() {
            out.push(n);
        }
    }
    out
}

/// A list laid over content slots, a page at a time.
pub struct Pages {
    /// Slots the entries go in, in order.
    pub content: Vec<usize>,
    pub prev_slot: usize,
    pub next_slot: usize,
}

impl Pages {
    /// The default: rows 1-4 inside a 6-row border, arrows at 45 and 53.
    pub fn standard() -> Pages {
        Pages { content: (0..54).filter(|s| { let (r, c) = (s / 9, s % 9); (1..5).contains(&r) && (1..8).contains(&c) }).collect(), prev_slot: 45, next_slot: 53 }
    }
    pub fn count(&self, total: usize) -> usize {
        total.div_ceil(self.content.len().max(1)).max(1)
    }
    /// Index range of `page` (0-based) in a list of `total`.
    pub fn range(&self, page: usize, total: usize) -> std::ops::Range<usize> {
        let per = self.content.len().max(1);
        let start = (page * per).min(total);
        start..(start + per).min(total)
    }
    /// Puts page `page` of `entries` in the menu (each entry an item and its action), and the arrows when
    /// there are more pages; `goto(page)` opens another page.
    pub fn draw<T: Clone + Send + Sync + 'static>(
        &self,
        menu: &mut Menu,
        page: usize,
        entries: &[T],
        render: impl Fn(&T) -> ItemStack,
        on_pick: impl Fn(&Ctx, &T) + Send + Sync + Clone + 'static,
        goto: impl Fn(&Player, usize) + Send + Sync + Clone + 'static,
    ) {
        let pages = self.count(entries.len());
        let page = page.min(pages - 1);
        for (slot, e) in self.content.iter().zip(&entries[self.range(page, entries.len())]) {
            let e2 = e.clone();
            let pick = on_pick.clone();
            menu.button(*slot, render(e), move |c| pick(c, &e2));
        }
        if page > 0 {
            let g = goto.clone();
            menu.button(self.prev_slot, icon("ARROW", "&ePrevious page", &[&format!("&7Page {}/{}", page, pages)]), move |c| g(&c.player, page - 1));
        }
        if page + 1 < pages {
            menu.button(self.next_slot, icon("ARROW", "&eNext page", &[&format!("&7Page {}/{}", page + 2, pages)]), move |c| goto(&c.player, page + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slot_lists_and_pages() {
        assert_eq!(slots("0,8, 10-12"), vec![0, 8, 10, 11, 12]);
        let p = Pages::standard();
        assert_eq!(p.content.len(), 28);
        assert_eq!(p.count(29), 2);
        assert_eq!(p.range(1, 29), 28..29);
        assert_eq!(p.count(0), 1);
        for n in 0..=7 {
            let c = centered(n);
            assert_eq!(c.len(), n);
            assert_eq!(c, c.iter().rev().map(|x| 8 - x).collect::<Vec<_>>());
        }
    }
}

/// An invisible item (the pack's `kitui:blank`): a clickable area over a button drawn in the menu's art. Its name
/// and lore still show when hovered.
pub fn hotspot(name: &str, lore: &[&str]) -> ItemStack {
    let mut s = icon("PAPER", name, lore);
    s.set_item_model("kitui:blank");
    s
}

/// Fills every slot of `slots` with a hotspot (one drawn button over several slots).
pub fn hotspots(menu: &mut Menu, slots: &[usize], name: &str, lore: &[&str], on_click: impl Fn(&Ctx) + Send + Sync + Clone + 'static) {
    for &s in slots {
        menu.button(s, hotspot(name, lore), on_click.clone());
    }
}

/// Frames on the menu the player has open: each `(ticks from now, slot, item)` is set then, if the same menu (by
/// tag) is still open. For progress bars, spinning reels, flashes.
pub fn animate(player: &Player, frames: Vec<(u64, usize, Option<ItemStack>)>) {
    let tag = open_tag(player);
    for (at, slot, item) in frames {
        let p = player.clone();
        let tag = tag.clone();
        let mut item = Some(item);
        crate::app::later(at, move || {
            if open_tag(&p) == tag {
                if let Some(it) = item.take() {
                    set_slot(&p, slot, it);
                }
            }
        });
    }
}
