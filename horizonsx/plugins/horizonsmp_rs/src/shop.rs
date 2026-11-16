//! The server shop (shop.yml): categories of items to buy and sell for the server's money, a trade screen with
//! amounts, `/sell hand|all`, a sell chest (`/sell`) that pays for what's put in it, and `/worth`.

use crate::{data, ui};
use horizon_bukkit::prelude::*;
use horizon_kit::{Config, app, gui, hooks::economy, yaml};
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

const SECTION: &str = "Shop";
const DEFAULT: &str = include_str!("../data/shop.yml");

#[derive(Clone, Debug)]
pub struct Entry {
    pub material: String,
    pub buy: f64,
    pub sell: f64,
}

#[derive(Clone, Debug)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub slot: usize,
    pub items: Vec<Entry>,
}

#[derive(Default)]
struct Shop {
    categories: Vec<Category>,
    /// Sell price by material.
    worth: HashMap<String, f64>,
}

static SHOP: RwLock<Option<Arc<Shop>>> = RwLock::new(None);

fn parse_entry(line: &str) -> Option<Entry> {
    let mut w = line.split_whitespace();
    let material = w.next()?.to_ascii_uppercase();
    let buy = w.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let sell = w.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    Material::get_material(&material)?;
    Some(Entry { material, buy, sell })
}

/// How many categories the shop has (for the start-up line).
pub fn category_count() -> usize {
    shop().categories.len()
}

pub fn reload() {
    let c = Config::load(&app::folder(), "shop.yml", DEFAULT);
    let mut categories = Vec::new();
    if let Some(Yaml::Mapping(m)) = c.at("categories") {
        for (i, (k, v)) in m.iter().enumerate() {
            let Some(id) = yaml::key_str(k) else { continue };
            let items: Vec<Entry> = yaml::list(v, "items").iter().filter_map(|l| parse_entry(l)).collect();
            categories.push(Category {
                id: id.clone(),
                name: yaml::string(v, "name").unwrap_or_else(|| ui::pretty(&id)),
                icon: yaml::string(v, "icon").unwrap_or_else(|| "CHEST".into()),
                slot: v.get("slot").and_then(Yaml::as_u64).map_or(19 + i, |s| s as usize),
                items,
            });
        }
    }
    let mut worth = HashMap::new();
    for c in &categories {
        for e in &c.items {
            if e.sell > 0.0 {
                worth.entry(e.material.clone()).or_insert(e.sell);
            }
        }
    }
    *SHOP.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(Shop { categories, worth }));
}

fn shop() -> Arc<Shop> {
    if let Some(s) = SHOP.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return s.clone();
    }
    reload();
    SHOP.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

/// What one of this item sells for (plain items only: renamed or enchanted ones aren't bought back).
pub fn worth_of(stack: &ItemStack) -> Option<f64> {
    if stack.has_display_name() || stack.has_lore() || stack.has_enchants() {
        return None;
    }
    shop().worth.get(&stack.get_type().name().to_ascii_uppercase()).copied()
}

// ---- Selling from the inventory ----

/// Sells every sellable item in the 36 storage slots that `pick` accepts; returns (count, money).
fn sell_where(p: &Player, pick: impl Fn(usize, &ItemStack) -> bool) -> (i64, f64) {
    let inv = p.get_inventory();
    let contents = inv.get_storage_contents();
    let (mut count, mut money) = (0i64, 0.0);
    for (slot, s) in contents.iter().enumerate() {
        let Some(s) = s else { continue };
        if !pick(slot, s) {
            continue;
        }
        let Some(w) = worth_of(s) else { continue };
        count += i64::from(s.get_amount());
        money += w * f64::from(s.get_amount());
        inv.clear_slot(slot as i32);
    }
    (count, money)
}

fn paid(p: &Player, count: i64, money: f64, what: &str) {
    if count == 0 {
        ui::say(p, SECTION, &format!("<#f87171>Nothing to sell{what}."));
        ui::bad(p);
        return;
    }
    let money = (money * 100.0).round() / 100.0;
    economy::deposit(p.get_name(), money);
    data::edit(&p.get_unique_id(), |d| d.money_earned_shop += money);
    ui::say(p, SECTION, &format!("Sold <white>{}</white> item(s){what} for <#4ade80><bold>{}</bold></#4ade80>.", ui::num(count), ui::money(money)));
    ui::bar(p, &format!("<#4ade80>+{}", ui::money(money)));
    ui::sound(p, "ENTITY_EXPERIENCE_ORB_PICKUP:0.8:1.0");
}

pub fn sell_hand(p: &Player) {
    if !crate::allowed(p, "horizonsmp.sell") {
        return;
    }
    let held = p.get_inventory().get_held_item_slot().max(0) as usize;
    let Some(item) = p.get_inventory().get_item_in_main_hand() else {
        return ui::say(p, SECTION, "<#f87171>Hold the item you want to sell.");
    };
    if worth_of(&item).is_none() {
        return ui::say(p, SECTION, &format!("<#f87171>The server doesn't buy {}.", ui::pretty(&item.get_type().name())));
    }
    let (count, money) = sell_where(p, |slot, _| slot == held);
    paid(p, count, money, "");
}

/// Every stack of the held item's kind.
pub fn sell_hand_all(p: &Player) {
    if !crate::allowed(p, "horizonsmp.sell") {
        return;
    }
    let Some(item) = p.get_inventory().get_item_in_main_hand() else {
        return ui::say(p, SECTION, "<#f87171>Hold the item you want to sell.");
    };
    let kind = item.get_type().name();
    let (count, money) = sell_where(p, |_, s| s.get_type().name() == kind);
    paid(p, count, money, &format!(" ({})", ui::pretty(&kind)));
}

pub fn sell_all(p: &Player) {
    if !crate::allowed(p, "horizonsmp.sell") {
        return;
    }
    // The hotbar holds tools and blocks a player is using: /sell all keeps it unless what's there is sellable
    // junk (they asked for all).
    let (count, money) = sell_where(p, |_, _| true);
    paid(p, count, money, "");
}

fn sell_material(p: &Player, material: &str, max: i64) -> (i64, f64) {
    if !crate::allowed(p, "horizonsmp.shop.sell") {
        return (0, 0.0);
    }
    let (n, stacks) = horizon_kit::inv::take(p, |s| s.get_type().name() == material && worth_of(s).is_some(), max);
    let money: f64 = stacks.iter().map(|s| worth_of(s).unwrap_or(0.0) * f64::from(s.get_amount())).sum();
    (n, money)
}

fn held_count(p: &Player, material: &str) -> i64 {
    horizon_kit::inv::count(p, |s| s.get_type().name() == material && worth_of(s).is_some())
}

/// How many of `material` fit in the player's storage slots.
fn room_for(p: &Player, material: &str) -> i64 {
    horizon_kit::inv::room_for(p, material)
}

fn buy(p: &Player, e: &Entry, n: i64) {
    if !crate::allowed(p, "horizonsmp.shop.buy") {
        return;
    }
    if e.buy <= 0.0 {
        return ui::deny(p, "The shop doesn't sell that.");
    }
    let n = n.min(room_for(p, &e.material));
    if n <= 0 {
        return ui::deny(p, "Your inventory is full.");
    }
    let cost = (e.buy * n as f64 * 100.0).round() / 100.0;
    if !economy::withdraw(p.get_name(), cost) {
        ui::deny(p, &format!("You need {} (you have {}).", ui::money(cost), ui::money(economy::balance(p.get_name()))));
        return;
    }
    if !horizon_kit::inv::give_many(p, &e.material, n) {
        ui::bar(p, "<#facc15>Your inventory is full: the rest dropped at your feet.");
    }
    ui::bar(p, &format!("<#4ade80>Bought {}× {} <#71717a>for <white>{}", n, ui::pretty(&e.material), ui::money(cost)));
    ui::sound(p, "ENTITY_ITEM_PICKUP:0.7:1.2");
}

// ---- Menus ----

fn price_lore(e: &Entry) -> Vec<String> {
    let mut l = vec![String::new()];
    if e.buy > 0.0 {
        l.push(format!("<#a1a1aa>Buy    <#4ade80>{}</#4ade80> <#52525b>• 64 for {}", ui::money(e.buy), ui::money(e.buy * 64.0)));
    } else {
        l.push("<#a1a1aa>Buy    <#52525b>not sold".into());
    }
    if e.sell > 0.0 {
        l.push(format!("<#a1a1aa>Sell    <#facc15>{}</#facc15> <#52525b>• 64 for {}", ui::money(e.sell), ui::money(e.sell * 64.0)));
    } else {
        l.push("<#a1a1aa>Sell    <#52525b>not bought".into());
    }
    l.push(String::new());
    if e.buy > 0.0 {
        l.push("<#22d3ee>▸ Left-click to buy".into());
        l.push("<#22d3ee>▸ Shift-left to buy 64".into());
    }
    if e.sell > 0.0 {
        l.push("<#facc15>▸ Right-click to sell all you have".into());
    }
    l
}

pub fn open(p: &Player) {
    if !crate::allowed(p, "horizonsmp.shop") {
        return;
    }
    let shop = shop();
    let mut m = ui::frame(6, "☰ Server Shop");
    let bal = economy::balance(p.get_name());
    let mut head = ui::head(p.get_name(), &ui::grad("<bold>Server Shop"), &[
        String::new(),
        format!("<#a1a1aa>Your balance  <#4ade80><bold>{}", ui::money(bal)),
        String::new(),
        "<#71717a>Buy what you need, sell what you farm.".into(),
    ]);
    ui::glint(&mut head);
    m.item(4, head);
    for c in &shop.categories {
        let id = c.id.clone();
        let lore = vec![String::new(), format!("<#a1a1aa>{} items", c.items.len()), String::new(), "<#22d3ee>▸ Click to browse".into()];
        m.button(c.slot, ui::icon(&c.icon, &format!("<#a855f7><bold>{}", c.name), &lore), move |cx| {
            ui::click(&cx.player);
            open_category(&cx.player, &id, 0);
        });
    }
    m.button(47, ui::icon("HOPPER", "<#facc15><bold>Sell items", &ui::lines(&["", "<#a1a1aa>Put items in a chest and", "<#a1a1aa>get paid when you close it.", "", "<#22d3ee>▸ Click to open"])), |c| {
        ui::click(&c.player);
        open_sell(&c.player);
    });
    m.button(51, ui::icon("GOLD_INGOT", "<#facc15><bold>Sell everything", &ui::lines(&["", "<#a1a1aa>Sells every item in your", "<#a1a1aa>inventory the shop buys.", "", "<#22d3ee>▸ Click to sell"])), |c| {
        crate::claims::confirm(&c.player, "Sell everything sellable?", |p| sell_all(p));
    });
    ui::back_button(&mut m, "the server menu", crate::menu::open);
    ui::close_button(&mut m);
    m.open(p);
}

fn open_category(p: &Player, id: &str, page: usize) {
    let shop = shop();
    let Some(cat) = shop.categories.iter().find(|c| c.id == id).cloned() else { return open(p) };
    let mut m = ui::frame(6, &format!("☰ Shop • {}", cat.name));
    m.item(4, ui::icon(&cat.icon, &format!("<#a855f7><bold>{}", cat.name), &[String::new(), format!("<#a1a1aa>Balance <#4ade80>{}", ui::money(economy::balance(p.get_name())))]));
    let id2 = cat.id.clone();
    let id3 = cat.id.clone();
    gui::Pages::standard().draw(
        &mut m,
        page,
        &cat.items,
        |e| ui::icon(&e.material, &format!("<white><bold>{}", ui::pretty(&e.material)), &price_lore(e)),
        move |c, e| {
            if c.click.is_right() {
                if e.sell <= 0.0 {
                    return ui::deny(&c.player, "The shop doesn't buy that.");
                }
                let (n, money) = sell_material(&c.player, &e.material, i64::MAX);
                paid(&c.player, n, money, &format!(" ({})", ui::pretty(&e.material)));
            } else if c.click == gui::Click::ShiftLeft {
                buy(&c.player, e, 64);
            } else if e.buy > 0.0 {
                ui::click(&c.player);
                open_trade(&c.player, &id2, e, 1);
            } else {
                ui::deny(&c.player, "The shop doesn't sell that.");
            }
        },
        move |pl, pg| open_category(pl, &id3, pg),
    );
    m.button(45, ui::icon("ARROW", "<#a1a1aa>← <white>Back", &ui::lines(&["<#71717a>To the shop"])), |c| {
        ui::click(&c.player);
        open(&c.player);
    });
    ui::close_button(&mut m);
    m.open(p);
}

fn open_trade(p: &Player, cat: &str, e: &Entry, amount: i64) {
    let amount = amount.clamp(1, 2304);
    let mut m = ui::frame(5, &format!("☰ {}", ui::pretty(&e.material)));
    let mut show = ui::icon(&e.material, &format!("<white><bold>{}× {}", amount, ui::pretty(&e.material)), &[
        String::new(),
        format!("<#a1a1aa>Buy {amount}   <#4ade80>{}", ui::money(e.buy * amount as f64)),
        if e.sell > 0.0 { format!("<#a1a1aa>Sell {amount}   <#facc15>{}", ui::money(e.sell * amount as f64)) } else { "<#a1a1aa>Sell     <#52525b>not bought".into() },
        String::new(),
        format!("<#a1a1aa>Balance <white>{}", ui::money(economy::balance(p.get_name()))),
    ]);
    show.set_amount(amount.clamp(1, 64) as i32);
    m.item(13, show);
    for (slot, delta) in [(19usize, -64i64), (20, -10), (21, -1), (23, 1), (24, 10), (25, 64)] {
        let (mat, name) = if delta < 0 { ("RED_STAINED_GLASS_PANE", format!("<#f87171><bold>{delta}")) } else { ("LIME_STAINED_GLASS_PANE", format!("<#4ade80><bold>+{delta}")) };
        let (cat, e) = (cat.to_owned(), e.clone());
        m.button(slot, ui::amount(ui::icon(mat, &name, &[]), delta.unsigned_abs() as i32), move |c| {
            ui::click(&c.player);
            open_trade(&c.player, &cat, &e, (amount + delta).max(1));
        });
    }
    {
        let (cat, e) = (cat.to_owned(), e.clone());
        m.button(30, ui::icon("LIME_CONCRETE", &format!("<#4ade80><bold>Buy {amount}"), &[String::new(), format!("<#a1a1aa>for <white>{}", ui::money(e.buy * amount as f64)), String::new(), "<#22d3ee>▸ Click to buy".into()]), move |c| {
            buy(&c.player, &e, amount);
            open_trade(&c.player, &cat, &e, amount);
        });
    }
    if e.sell > 0.0 {
        let have = held_count(p, &e.material);
        let (cat2, e2) = (cat.to_owned(), e.clone());
        m.button(32, ui::icon("ORANGE_CONCRETE", &format!("<#facc15><bold>Sell {amount}"), &[String::new(), format!("<#a1a1aa>for <white>{}", ui::money(e.sell * amount as f64)), format!("<#71717a>You have {have}"), String::new(), "<#22d3ee>▸ Click to sell".into()]), move |c| {
            let (n, money) = sell_material(&c.player, &e2.material, amount);
            paid(&c.player, n, money, "");
            open_trade(&c.player, &cat2, &e2, amount);
        });
        let (cat3, e3) = (cat.to_owned(), e.clone());
        m.button(31, ui::icon("HOPPER", &format!("<#facc15><bold>Sell all ({have})"), &[String::new(), format!("<#a1a1aa>for <white>{}", ui::money(e.sell * have as f64)), String::new(), "<#22d3ee>▸ Click to sell".into()]), move |c| {
            let (n, money) = sell_material(&c.player, &e3.material, i64::MAX);
            paid(&c.player, n, money, "");
            open_trade(&c.player, &cat3, &e3, amount);
        });
    }
    let cat4 = cat.to_owned();
    m.button(36, ui::icon("ARROW", "<#a1a1aa>← <white>Back", &ui::lines(&["<#71717a>To the category"])), move |c| {
        ui::click(&c.player);
        open_category(&c.player, &cat4, 0);
    });
    ui::close_button(&mut m);
    m.open(p);
}

/// A chest the player fills; on close, what the shop buys is paid for and the rest goes back.
pub fn open_sell(p: &Player) {
    if !crate::allowed(p, "horizonsmp.sell") {
        return;
    }
    let mut m = gui::Menu::new(6, "&8☰ Sell items &7(close to sell)");
    m.allow_take(true);
    for s in 45..54 {
        m.item(s, gui::filler("BLACK_STAINED_GLASS_PANE"));
        m.on(s, |_| {});
    }
    m.button(49, ui::icon("EMERALD", "<#4ade80><bold>Sell and close", &ui::lines(&["", "<#a1a1aa>Put items in the slots above.", "<#a1a1aa>What the shop doesn't buy", "<#a1a1aa>comes back to you.", "", "<#22d3ee>▸ Click (or just close)"])), |c| c.close());
    m.on_close_items(|p, items| sold_chest(p, items));
    m.open(p);
}

/// What was in the sell chest when it closed: paid for, the rest given back.
fn sold_chest(p: &Player, items: Vec<Option<ItemStack>>) {
    let (mut count, mut money) = (0i64, 0.0);
    let mut back = Vec::new();
    for s in items.into_iter().take(45).flatten() {
        match worth_of(&s) {
            Some(w) => {
                count += i64::from(s.get_amount());
                money += w * f64::from(s.get_amount());
            }
            None => back.push(s),
        }
    }
    for s in &back {
        crate::give_item(p, s);
    }
    if count > 0 {
        paid(p, count, money, "");
    }
    if !back.is_empty() {
        ui::say(p, SECTION, &format!("{} stack(s) the shop doesn't buy came back to you.", back.len()));
    }
}

// ---- Commands ----

pub fn shop_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    let Some(p) = sender.as_player() else { return true };
    let p = p.clone();
    match args.first() {
        Some(a) if a.eq_ignore_ascii_case("reload") && p.has_permission("horizonsmp.shop.admin") => {
            reload();
            ui::say(&p, SECTION, "Shop reloaded.");
        }
        Some(cat) => {
            let id = cat.to_ascii_lowercase();
            if shop().categories.iter().any(|c| c.id == id) {
                open_category(&p, &id, 0);
            } else {
                open(&p);
            }
        }
        None => open(&p),
    }
    true
}

pub fn sell_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    let Some(p) = sender.as_player() else { return true };
    let p = p.clone();
    match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("hand" | "h") => sell_hand(&p),
        Some("handall" | "same") => sell_hand_all(&p),
        Some("all" | "inventory" | "inv") => sell_all(&p),
        _ => open_sell(&p),
    }
    true
}

pub fn worth_cmd(sender: &CommandSender, _args: &[&str]) -> bool {
    let Some(p) = sender.as_player() else { return true };
    let p = p.clone();
    let Some(item) = p.get_inventory().get_item_in_main_hand() else {
        ui::say(&p, SECTION, "Hold an item to see what it sells for.");
        return true;
    };
    let name = ui::pretty(&item.get_type().name());
    match worth_of(&item) {
        Some(w) => ui::say(&p, SECTION, &format!("<white>{name}</white> sells for <#facc15>{}</#facc15> each, <#facc15>{}</#facc15> for this stack of {}.", ui::money(w), ui::money(w * f64::from(item.get_amount())), item.get_amount())),
        None => ui::say(&p, SECTION, &format!("The shop doesn't buy <white>{name}</white>{}.", if item.has_display_name() || item.has_enchants() { " (renamed or enchanted items aren't bought)" } else { "" })),
    }
    true
}

pub fn complete(args: &[&str]) -> Vec<String> {
    match args {
        [one] => shop().categories.iter().map(|c| c.id.clone()).filter(|c| c.starts_with(&one.to_ascii_lowercase())).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_shop_parses() {
        let v: serde_yaml::Value = serde_yaml::from_str(super::DEFAULT).unwrap();
        let cats = v["categories"].as_mapping().unwrap();
        assert!(cats.len() >= 8);
        for (_, c) in cats {
            for line in c["items"].as_sequence().unwrap() {
                let parts: Vec<&str> = line.as_str().unwrap().split_whitespace().collect();
                assert_eq!(parts.len(), 3, "{line:?}");
                assert!(parts[1].parse::<f64>().is_ok() && parts[2].parse::<f64>().is_ok());
            }
        }
    }
}
