//! `/buy`: the shop menus (`BuyGUI`, `ListingGui`), from the listing already fetched. Opening a menu is no
//! request; buying one asks for a checkout link on the `ask` HTTP thread.

use crate::api::{Category, Package};
use crate::{state, util};
use horizon_bukkit::fam_ev_player::InventoryClickEvent;
use horizon_bukkit::{Inventory, ItemStack, Material, Player};
use serde_json::json;
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Home,
    Category(usize),
    Sub(usize, usize),
    Buy(i64),
}

/// A menu someone has open: its window and what each slot does.
pub struct Menu {
    inv: Inventory,
    actions: HashMap<usize, Action>,
}

struct Builder {
    rows: usize,
    items: Vec<Option<ItemStack>>,
    actions: HashMap<usize, Action>,
}

impl Builder {
    fn new(rows: usize) -> Self {
        let rows = rows.clamp(1, 6);
        Self { rows, items: vec![None; rows * 9], actions: HashMap::new() }
    }
    /// `ListingGui.addItem`: the first free slot.
    fn add(&mut self, item: ItemStack, action: Action) {
        if let Some(slot) = (0..self.items.len()).find(|s| !self.actions.contains_key(s)) {
            self.put(slot, item, action);
        }
    }
    fn put(&mut self, slot: usize, item: ItemStack, action: Action) {
        if slot < self.items.len() {
            self.items[slot] = Some(item);
            self.actions.insert(slot, action);
        }
    }
    fn open(self, player: &Player, title: &str) {
        let mut inv = Inventory::create(self.rows * 9, &remap(title));
        for (slot, item) in self.items.into_iter().enumerate() {
            if item.is_some() {
                inv.set_item(slot, item);
            }
        }
        player.open_inventory(&inv);
        let menu = Menu { inv, actions: self.actions };
        state(|s| s.menus.insert(player.get_unique_id(), menu));
    }
}

/// `&` colour codes as `§` (`remapLegacyFormatSeparator`).
fn remap(text: &str) -> String {
    text.replace('&', "§")
}

fn italic(text: &str) -> String {
    format!("§o{text}§r")
}

fn gui_cfg() -> Yaml {
    state(|s| s.cfg.yaml["gui"].clone())
}

fn material(name: &str) -> Option<Material> {
    util::material_name(name).filter(|n| !n.is_empty()).and_then(|n| Material::get_material(&n))
}

/// An item from a `gui.item.<kind>` section: the store's own item first, the section's, else a book.
fn item(section: &Yaml, own: &str, fill: &dyn Fn(&str) -> String, glint: bool) -> ItemStack {
    let mat = material(own).or_else(|| section["material"].as_str().and_then(material)).unwrap_or_else(|| Material::of("BOOK"));
    let mut stack = ItemStack::new(mat, 1);
    let name = section["name"].as_str().map_or_else(|| fill("%name%"), |n| remap(&italic(&fill(n))));
    stack.set_display_name(&name);
    let lore: Vec<String> = section["lore"].as_sequence().into_iter().flatten().filter_map(Yaml::as_str).map(|l| remap(&italic(&fill(l)))).collect();
    stack.set_lore(lore);
    // HIDE_ATTRIBUTES, HIDE_ENCHANTS, HIDE_UNBREAKABLE; a sale glows (Java adds a hidden enchantment for it).
    stack.set_component("minecraft:tooltip_display", json!({ "hidden_components": ["minecraft:attribute_modifiers", "minecraft:enchantments", "minecraft:unbreakable"] }));
    if glint {
        stack.set_component("minecraft:enchantment_glint_override", json!(true));
    }
    stack
}

fn category_item(gui: &Yaml, name: &str, own: &str) -> ItemStack {
    let fill = |t: &str| if t == "%name%" { name.to_owned() } else { t.replace("%category%", name) };
    item(&gui["item"]["category"], own, &fill, false)
}

fn package_item(gui: &Yaml, p: &Package, iso: &str, symbol: &str) -> ItemStack {
    let section = if p.sale.is_some() { &gui["item"]["package-sale"] } else { &gui["item"]["package"] };
    let fill = |t: &str| {
        if t == "%name%" {
            return p.name.clone();
        }
        let mut t = t
            .replace("%package_name%", &p.name)
            .replace("%package_price%", &util::price(p.price))
            .replace("%package_currency_name%", iso)
            .replace("%package_currency%", symbol);
        if let Some(discount) = p.sale {
            t = t.replace("%package_discount%", &util::price(discount)).replace("%package_sale_price%", &util::price(p.price - discount));
        }
        t
    };
    item(section, &p.item, &fill, p.sale.is_some())
}

fn back_item(gui: &Yaml) -> ItemStack {
    let section = &gui["item"]["back"];
    let fill = |t: &str| if t == "%name%" { "Back".to_owned() } else { t.to_owned() };
    item(section, "", &fill, false)
}

fn rows_of(gui: &Yaml, menu: &str) -> usize {
    gui["menu"][menu]["rows"].as_i64().unwrap_or(0).max(0) as usize
}

fn title_of(gui: &Yaml, menu: &str, default: &str) -> String {
    gui["menu"][menu]["title"].as_str().unwrap_or(default).to_owned()
}

fn listing(player: &Player) -> Option<Arc<Vec<Category>>> {
    let list = state(|s| s.categories.clone());
    if list.is_none() {
        player.send_message("§cFailed to get listing. Please contact an administrator.");
    }
    list
}

fn currency() -> (String, String) {
    state(|s| s.info.as_ref().map(|i| (i.store.currency_iso.clone(), i.store.currency_symbol.clone())).unwrap_or_default())
}

/// The categories.
pub fn open_home(player: &Player) {
    let Some(cats) = listing(player) else { return };
    let gui = gui_cfg();
    let needed = cats.len() / 9 + 1;
    let mut b = Builder::new(rows_of(&gui, "home").max(needed));
    for (i, c) in cats.iter().enumerate() {
        b.add(category_item(&gui, &c.name, &c.item), Action::Category(i));
    }
    b.open(player, &title_of(&gui, "home", "Server Shop"));
}

/// A category: its sub-categories, a back button, its packages.
fn open_category(player: &Player, ci: usize) {
    let Some(cats) = listing(player) else { return };
    let Some(c) = cats.get(ci) else { return open_home(player) };
    let gui = gui_cfg();
    let (iso, symbol) = currency();
    let mut b = Builder::new(rows_of(&gui, "category").max(c.packages.len() / 9 + 1));
    for (si, sub) in c.subs.iter().enumerate() {
        b.add(category_item(&gui, &sub.name, &sub.item), Action::Sub(ci, si));
    }
    let back = b.rows * 9 - 5;
    b.put(back, back_item(&gui), Action::Home);
    for p in &c.packages {
        b.add(package_item(&gui, p, &iso, &symbol), Action::Buy(p.id));
    }
    b.open(player, &title_of(&gui, "category", "Viewing %category%").replace("%category%", &c.name));
}

fn open_sub(player: &Player, ci: usize, si: usize) {
    let Some(cats) = listing(player) else { return };
    let Some((c, sub)) = cats.get(ci).and_then(|c| c.subs.get(si).map(|s| (c, s))) else { return open_home(player) };
    let gui = gui_cfg();
    let (iso, symbol) = currency();
    let mut b = Builder::new(rows_of(&gui, "sub-category").max(sub.packages.len() / 9 + 1));
    let back = b.rows * 9 - 5;
    b.put(back, back_item(&gui), Action::Category(ci));
    for p in &sub.packages {
        b.add(package_item(&gui, p, &iso, &symbol), Action::Buy(p.id));
    }
    let title = title_of(&gui, "sub-category", "Viewing %sub_category% (%category%)").replace("%category%", &c.name).replace("%sub_category%", &sub.name);
    b.open(player, &title);
}

/// A click in one of the shop's menus: cancelled, and its slot's action run next tick (as Java's `runTask`).
pub fn on_click(event: &mut InventoryClickEvent) {
    let Some(gui) = event.get_inventory().gui() else { return };
    let Some(player) = event.get_who_clicked() else { return };
    let uuid = player.get_unique_id();
    let slot = usize::try_from(event.get_raw_slot()).ok();
    let found = state(|s| {
        let menu = s.menus.get(&uuid)?;
        menu.inv.same_as(gui).then(|| slot.and_then(|slot| menu.actions.get(&slot).copied()))
    });
    let Some(action) = found else { return };
    event.set_cancelled(true);
    let Some(action) = action else { return };
    crate::on_main(move || match action {
        Action::Home => open_home(&player),
        Action::Category(ci) => open_category(&player, ci),
        Action::Sub(ci, si) => open_sub(&player, ci, si),
        Action::Buy(package) => buy(&player, package),
    });
}

fn buy(player: &Player, package: i64) {
    player.close_inventory();
    let key = crate::secret();
    let player = player.clone();
    crate::ASK.run(move |c| {
        let line = match c.checkout(&key, package, player.get_name()) {
            Ok(url) => crate::checkout_line(&url),
            Err(_) => "§cFailed to create checkout URL. Please contact an administrator.".to_owned(),
        };
        crate::on_main(move || player.send_message(&line));
    });
}
