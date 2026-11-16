//! AuctionGui (guis/main.yml: the bid slots) and PreviewGui (guis/preview.yml: the item before bidding opens).
//! A menu is opened once; each second (and on every bid) only the slots whose item changed are sent again, so
//! the clock ticks down and bids show up without the window reopening.

use crate::auction::{AUCTIONS, Auction, Bidding, State};
use crate::{later, lock, settings};
use horizon_bukkit::{ItemStack, Player};
use horizon_kit::{Config, gui, item};
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bidding,
    Preview,
}

struct Open {
    auction: String,
    kind: Kind,
    token: u64,
    /// What each slot shows now (SNBT), to send only changes.
    shown: HashMap<usize, String>,
}

static OPEN: Mutex<Option<HashMap<String, Open>>> = Mutex::new(None);

fn next_token() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// GuiFrame.buildItem: the route's item (or its `.item`), `%..%` filled, a player head showing `head` when it
/// names no texture of its own.
fn build(file: &Config, route: &str, reps: &[(&str, &str)], head: Option<&str>) -> Option<ItemStack> {
    let mut section = file.at(route)?;
    if horizon_kit::yaml::string(section, "material").is_none() && horizon_kit::yaml::string(section, "type").is_none()
        && let Some(inner) = file.at(&format!("{route}.item"))
    {
        section = inner;
    }
    let spec = item::Spec::from_yaml(section);
    let mut stack = spec.build(&|t| crate::texts::fill(t, reps))?;
    if spec.head.is_none()
        && stack.get_type().key() == "minecraft:player_head"
        && let Some(name) = head
    {
        stack.set_component("minecraft:profile", serde_json::json!({ "name": name }));
    }
    Some(stack)
}

/// The reward's display item (`ItemBuilder.create(display)`).
pub fn reward_item(display: &Yaml) -> Option<ItemStack> {
    item::Spec::from_yaml(display).build(&|t| t.to_owned())
}

/// Every slot's item for a viewer, as the Java open() lays them: the file's plain items, the clock, the item on
/// sale, then (bidding) each bid slot.
fn layout(a: &Auction, file: &Config, viewer: &Player, kind: Kind) -> HashMap<usize, ItemStack> {
    let mut out = HashMap::new();
    let left = a.left_seconds();
    for route in file.keys("") {
        let has_item = file.at(&format!("{route}.material")).is_some() || file.at(&format!("{route}.type")).is_some();
        if !has_item || route == "clock" {
            continue;
        }
        if let Some(stack) = build(file, &route, &[], Some(viewer.get_name())) {
            for s in crate::auction::route_slots(file, &route) {
                out.insert(s, stack.clone());
            }
        }
    }
    let (t, tf) = (left.to_string(), crate::format_time(left * 1000));
    if let Some(mut clock) = build(file, "clock", &[("%time%", &t), ("%time_formatted%", &tf)], None) {
        clock.set_amount(left.clamp(1, 64) as i32);
        for s in crate::auction::route_slots(file, "clock") {
            out.insert(s, clock.clone());
        }
    }
    if let Some(reward) = &a.selected
        && let Some(stack) = reward_item(&reward.display)
    {
        for s in crate::auction::route_slots(file, "item") {
            out.insert(s, stack.clone());
        }
    }
    if kind == Kind::Bidding
        && let (Some(reward), Some(page)) = (&a.selected, a.biddings.get(&a.page))
    {
        for slot in &a.bid_slots {
            let Some(b) = page.get(slot) else { continue };
            let amount = crate::format_money(&reward.currency, b.value);
            let holder = a.top_bids.values().find(|(_, v)| *v == b.value);
            let stack = match holder {
                None if a.bid > b.value => build(file, "bidding.already-higher", &[("%amount%", &amount)], None),
                Some((name, _)) => build(file, "bidding.current-bid", &[("%player%", name), ("%amount%", &amount)], Some(name)),
                None => bid_item(file, b, &amount),
            };
            if let Some(stack) = stack {
                out.insert(*slot, stack);
            }
        }
    }
    out
}

/// A slot you can bid on: the stage's item, named and described by `bidding.bid`.
fn bid_item(file: &Config, b: &Bidding, amount: &str) -> Option<ItemStack> {
    let mut stack = item::Spec::from_yaml(&b.item).build(&|t| t.to_owned())?;
    stack.set_amount(1);
    let name = file.str("bidding.bid.name", "%amount%").replace("%amount%", amount);
    item::name(&mut stack, &name);
    let lore: Vec<String> = file.list("bidding.bid.lore").iter().map(|l| l.replace("%amount%", amount)).collect();
    item::lore(&mut stack, &lore);
    Some(stack)
}

fn file_of(kind: Kind) -> Option<Config> {
    settings(|s| if kind == Kind::Bidding { s.main_gui.clone() } else { s.preview_gui.clone() })
}

/// Opens the auction's bidding or preview GUI for a player (View, the NPCs, the state changes).
pub fn open(p: &Player, auction: &str, kind: Kind) {
    let Some(file) = file_of(kind) else { return };
    let items = {
        let all = lock(&AUCTIONS);
        let Some(a) = all.get(auction) else { return };
        let fits = match kind {
            Kind::Bidding => a.state == State::Bidding,
            Kind::Preview => a.state == State::Showing,
        };
        if !fits || a.selected.is_none() {
            return;
        }
        layout(a, &file, p, kind)
    };
    let rows = file.int("rows", if kind == Kind::Bidding { 6 } else { 3 }).clamp(1, 6) as usize;
    let title = crate::texts::legacy(&file.str("title", "---").replace("%time%", "---"));
    let mut menu = gui::Menu::new(rows, &title);
    let mut shown = HashMap::new();
    for (slot, stack) in &items {
        shown.insert(*slot, stack.to_snbt());
        menu.item(*slot, stack.clone());
    }
    if kind == Kind::Bidding {
        let slots = crate::auction::with(auction, |a| a.bid_slots.clone()).unwrap_or_default();
        for slot in slots {
            let name = auction.to_owned();
            menu.on(slot, move |c| {
                bid_click(&c.player, &name, c.slot);
                crate::finish();
            });
        }
    }
    let token = next_token();
    let uuid = p.get_unique_id();
    menu.on_close(move |player| {
        let mut open = lock(&OPEN);
        if let Some(m) = open.as_mut()
            && m.get(&player.get_unique_id()).is_some_and(|o| o.token == token)
        {
            m.remove(&player.get_unique_id());
        }
    });
    menu.tag(&format!("axda:{auction}"));
    menu.open(p);
    lock(&OPEN).get_or_insert_with(HashMap::new).insert(uuid, Open { auction: auction.to_owned(), kind, token, shown });
}

/// The auction's open menus, each changed slot sent again (`update(Menu.MAIN/PREVIEW)`).
pub fn refresh(auction: &str) {
    let viewers: Vec<(String, Kind)> = lock(&OPEN).as_ref().map(|m| m.iter().filter(|(_, o)| o.auction == auction).map(|(u, o)| (u.clone(), o.kind)).collect()).unwrap_or_default();
    if viewers.is_empty() {
        return;
    }
    let files = (file_of(Kind::Bidding), file_of(Kind::Preview));
    let mut updates = Vec::new();
    {
        let all = lock(&AUCTIONS);
        let Some(a) = all.get(auction) else { return };
        for (uuid, kind) in viewers {
            let Some(p) = horizon_bukkit::fam_ess::get_player_by_id(&uuid) else { continue };
            let file = if kind == Kind::Bidding { files.0.as_ref() } else { files.1.as_ref() };
            let Some(file) = file else { continue };
            updates.push((p.clone(), layout(a, file, &p, kind)));
        }
    }
    for (p, items) in updates {
        let changed: Vec<(usize, ItemStack, String)> = {
            let open = lock(&OPEN);
            let Some(o) = open.as_ref().and_then(|m| m.get(&p.get_unique_id())) else { continue };
            items.into_iter().filter_map(|(s, st)| {
                let snbt = st.to_snbt();
                (o.shown.get(&s) != Some(&snbt)).then_some((s, st, snbt))
            }).collect()
        };
        for (slot, stack, snbt) in changed {
            gui::set_slot(&p, slot, Some(stack));
            if let Some(o) = lock(&OPEN).as_mut().and_then(|m| m.get_mut(&p.get_unique_id())) {
                o.shown.insert(slot, snbt);
            }
        }
    }
}

/// closeGuis: everyone's window on this auction closes (after the state is let go).
pub fn close_auction(auction: &str) {
    let who: Vec<String> = lock(&OPEN).as_ref().map(|m| m.iter().filter(|(_, o)| o.auction == auction).map(|(u, _)| u.clone()).collect()).unwrap_or_default();
    if who.is_empty() {
        return;
    }
    later(move || {
        for uuid in who {
            if let Some(p) = horizon_bukkit::fam_ess::get_player_by_id(&uuid) {
                p.close_inventory();
            }
        }
    });
}

pub fn forget(p: &Player) {
    if let Some(m) = lock(&OPEN).as_mut() {
        m.remove(&p.get_unique_id());
    }
}

/// A click on a bid slot (AuctionGui's GuiItem action): only a slot that offers a bid does anything.
fn bid_click(p: &Player, auction: &str, slot: usize) {
    let mut all = lock(&AUCTIONS);
    let Some(a) = all.get_mut(auction) else { return };
    if a.state != State::Bidding {
        return;
    }
    let Some(reward) = a.selected.clone() else { return };
    let Some(b) = a.biddings.get(&a.page).and_then(|row| row.get(&slot)).cloned() else { return };
    let held = a.top_bids.values().any(|(_, v)| *v == b.value);
    if held || a.bid > b.value {
        return;
    }
    let shown = crate::format_money(&reward.currency, b.value);
    let sender = horizon_bukkit::CommandSender::Player(p.clone());
    if a.bid >= b.value {
        crate::send_lang(&sender, "errors.already-higher", &[("%amount%", &shown)]);
        return;
    }
    if a.top.as_ref().is_some_and(|t| t.get_unique_id() == p.get_unique_id()) && !crate::config_bool("allow-increasing-own-bid", true) {
        crate::send_lang(&sender, "errors.already-winning", &[]);
        return;
    }
    let Some(currency) = crate::currency(&reward.currency) else { return };
    let balance = currency.balance(p);
    if balance < b.value {
        crate::send_lang(&sender, "errors.not-enough-balance", &[("%amount%", &shown), ("%balance%", &crate::format_money(&reward.currency, balance))]);
        return;
    }
    a.bid = b.value;
    a.top = Some(p.clone());
    a.top_bids.insert(p.get_unique_id(), (p.get_name().to_owned(), b.value));
    if a.extend_bid && a.bid_time - a.time < a.extend_time {
        a.time = a.bid_time - a.extend_time;
    }
    let left = a.bid_time - a.time;
    let item = Auction::item_name(&reward);
    let amount = crate::format_money(&reward.currency, a.bid);
    a.broadcast("bid", &[("%player%", p.get_name()), ("%item%", &item), ("%amount%", &amount), ("%time%", &left.to_string()), ("%time_formatted%", &crate::format_time(left * 1000))]);
    // The page's last slot was bid on: the next page of amounts, a second later.
    let last = a.bid_slots.last().copied() == Some(slot);
    let name = auction.to_owned();
    drop(all);
    refresh(&name);
    if last {
        horizon_kit::app::later(20, move || {
            crate::auction::with(&name, |a| {
                a.page += 1;
                let page = a.page;
                a.make_page(page);
            });
            refresh(&name);
            crate::finish();
        });
    }
}
