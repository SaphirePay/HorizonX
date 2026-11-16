//! What players do: sell, buy, take back, claim; the windows' clicks; the expiry sweep and the money waiting
//! for sellers. All on the server thread (money, inventories and permissions are game queries); the data
//! changes are in memory and saved by the store's thread.

use crate::conf::{Conf, ItemFacts};
use crate::gui::{self, Act, Nav, PState};
use crate::store::{self, Listing, Sale};
use crate::text;
use horizon_bukkit::{CommandSender, Component, ItemStack, Player};
use horizon_kit::hooks::economy;
use std::collections::HashMap;

// ---- Messages ----

fn lines(conf: &Conf, key: &str, pairs: &[(&str, String)]) -> Vec<String> {
    conf.lines(key).iter().filter(|l| !l.is_empty()).map(|l| {
        let l = text::fill(&l.replace("%prefix%", &conf.prefix), pairs);
        if l.starts_with(&conf.prefix) || conf.prefix.is_empty() { l } else { format!("{}{l}", conf.prefix) }
    }).collect()
}

/// A messages.yml message to a player (MiniMessage, the prefix before it).
pub fn msg(p: &Player, key: &str, pairs: &[(&str, String)]) {
    let conf = crate::conf();
    for l in lines(&conf, key, pairs) {
        p.send_component(&Component(text::component(&l, &conf.colors)));
    }
}

pub fn msg_to(s: &CommandSender, key: &str, pairs: &[(&str, String)]) {
    match s.as_player() {
        Some(p) => msg(p, key, pairs),
        None => {
            let conf = crate::conf();
            for l in lines(&conf, key, pairs) {
                s.send_message(&text::legacy(&l, &conf.colors));
            }
        }
    }
}

fn broadcast(key: &str, pairs: &[(&str, String)], except: Option<&str>, option: &str) {
    let conf = crate::conf();
    let opts = store::with(|d| d.options.clone());
    let comps: Vec<Component> = lines(&conf, key, pairs).iter().map(|l| Component(text::component(l, &conf.colors))).collect();
    for p in horizon_bukkit::get_online_players() {
        let uuid = p.get_unique_id();
        if except == Some(uuid.as_str()) || opts.get(&uuid).and_then(|o| o.get(option)).is_some_and(|v| !v) {
            continue;
        }
        for c in &comps {
            p.send_component(c);
        }
    }
}

/// `%items%`: `x2 Diamond Sword` (its own name) or the game's translated name.
pub fn display(conf: &Conf, item: &serde_json::Value) -> String {
    let amount = store::item_amount(item).to_string();
    match item["components"].get("minecraft:custom_name") {
        Some(name) => text::fill(&conf.display_name, &[("%amount%", amount), ("%item-name%", horizon_bukkit::Component(name.clone()).to_legacy())]),
        None => {
            let key = store::item_from_json(item).map(|s| s.get_type().translation_key()).unwrap_or_default();
            text::fill(&conf.display_lang, &[("%amount%", amount), ("%item-translation-key%", key)])
        }
    }
}

// ---- Items ----

pub fn facts(stack: &ItemStack) -> ItemFacts {
    let item = store::item_to_json(stack);
    ItemFacts {
        material: store::item_material(&item),
        is_block: stack.get_type().is_block(),
        name: gui::item_name(&item),
        lore: gui::item_lore(&item),
        model_data: stack.get_custom_model_data().map(i64::from),
        item_model: item["components"]["minecraft:item_model"].as_str().map(str::to_owned),
    }
}

/// The categories an item is in (`misc` when none of the others).
pub fn categorize(conf: &Conf, item: &serde_json::Value) -> Vec<String> {
    let Some(stack) = store::item_from_json(item) else { return Vec::new() };
    let f = facts(&stack);
    let mut out: Vec<String> = conf.categories.iter().filter(|c| !c.rules.is_empty() && c.rules.iter().any(|r| r.matches(&f))).map(|c| c.id.clone()).collect();
    if out.is_empty() {
        out.push("misc".into());
    }
    out
}

/// Gives a stack to a player; what didn't fit comes back.
fn give(p: &Player, stack: &ItemStack) -> Option<ItemStack> {
    p.get_inventory().add_item(stack)
}

fn has_space(p: &Player) -> bool {
    p.get_inventory().first_empty() >= 0
}

fn online(uuid: &str) -> Option<Player> {
    horizon_bukkit::get_online_players().into_iter().find(|p| p.get_unique_id() == uuid)
}

// ---- Selling ----

pub fn limit(p: &Player, conf: &Conf) -> i64 {
    conf.limits.iter().filter(|(perm, _)| p.has_permission(perm)).map(|(_, l)| *l).max().unwrap_or(0)
}

fn expiration(p: &Player, conf: &Conf) -> i64 {
    conf.expiration_perms.iter().filter(|(perm, _)| p.has_permission(perm)).map(|(_, e)| *e).max().unwrap_or(conf.expiration)
}

fn tax_messages(p: &Player, conf: &Conf, eco: &str, tr: &crate::conf::TaxResult, key: &str) {
    if tr.bypassed {
        return msg(p, "tax-exempt", &[]);
    }
    if tr.tax <= 0.0 {
        return;
    }
    if let Some(r) = tr.reduced {
        msg(p, "tax-reduced", &[("%percentage%", format!("{:.1}", 100.0 - r))]);
    }
    msg(p, key, &[("%tax%", conf.format_price(eco, tr.tax)), ("%percentage%", format!("{:.1}", tr.percentage))]);
}

/// `/ah sell <price> [amount] [economy]`.
pub fn sell(p: &Player, args: &[String]) {
    let conf = crate::conf();
    let syntax = || msg(p, "command-syntax-error", &[("%syntax%", "/ah sell <price> [<amount>] [<economy>]".into())]);
    let Some(price_text) = args.first() else { return syntax() };
    let inv = p.get_inventory();
    let slot = inv.get_held_item_slot();
    let Some(held) = inv.get_item_in_main_hand().filter(|s| !s.is_empty()) else { return msg(p, "sell-error-air", &[]) };
    let mut amount = args.get(1).and_then(|a| a.parse::<i32>().ok()).unwrap_or(held.get_amount());
    amount = if amount > held.get_amount() { held.get_amount() } else if amount <= 0 { 1 } else { amount };
    let Some(default_eco) = conf.economy(&conf.default_economy).or_else(|| conf.economies.first()) else { return msg(p, "sell-error-default-economy", &[]) };
    let eco_name = args.get(2).cloned().unwrap_or_else(|| default_eco.name.clone());
    let Some(eco) = conf.economy(&eco_name).cloned() else { return msg(p, "sell-error-economy", &[("%name%", eco_name)]) };
    let Some(price) = conf.parse_price(price_text) else { return syntax() };
    if price > eco.max {
        return msg(p, "price-too-high", &[("%max-price%", conf.format_price(&eco.name, eco.max))]);
    }
    if price < eco.min_auction {
        return msg(p, "price-too-low", &[("%min-price%", conf.format_price(&eco.name, eco.min_auction))]);
    }
    if !conf.allow_decimals && price.fract() != 0.0 {
        return msg(p, "price-decimal-not-allowed", &[]);
    }
    let uuid = p.get_unique_id();
    let listed = store::with(|d| d.listed.iter().filter(|l| l.seller == uuid).count() as i64);
    let max = limit(p, &conf);
    if listed >= max {
        return msg(p, "listed-items-limit", &[("%max-items%", max.to_string())]);
    }
    if conf.banned_worlds.iter().any(|w| w == p.get_world().get_name()) {
        return msg(p, "world-banned", &[]);
    }
    let f = facts(&held);
    if let Some(rules) = &conf.blacklist
        && rules.iter().any(|r| r.matches(&f))
    {
        return msg(p, "item-blacklisted", &[]);
    }
    if let Some(rules) = &conf.whitelist
        && !rules.iter().any(|r| r.matches(&f))
    {
        return msg(p, "item-whitelisted", &[]);
    }
    let tax = eco.tax.compute(&|perm| p.has_permission(perm), price, true);
    if let Some(tr) = &tax
        && !tr.bypassed
        && tr.tax > 0.0
        && !economy::withdraw(p.get_name(), tr.tax)
    {
        return msg(p, "tax-insufficient-funds", &[("%tax%", conf.format_price(&eco.name, tr.tax))]);
    }
    // The hand again, now: the same stack must still be there.
    let now = inv.get_item(slot);
    if !now.as_ref().is_some_and(|s| s.is_similar(&held) && s.get_amount() >= amount) {
        return msg(p, "sell-error-change", &[]);
    }
    let left = held.get_amount() - amount;
    if left > 0 {
        inv.set_item(slot, &held.as_quantity(left));
    } else {
        inv.clear_slot(slot);
    }
    let item = store::item_to_json(&held.as_quantity(amount));
    let exp = expiration(p, &conf);
    let created = text::now_ms();
    let categories = categorize(&conf, &item);
    let listing = store::with(|d| {
        let l = Listing { id: d.next(), seller: uuid.clone(), seller_name: p.get_name().to_owned(), item, price, economy: eco.name.clone(), created, expire: if exp > 0 { created + exp * 1000 } else { 0 }, buyer: None, buyer_name: None, categories };
        d.listed.push(l.clone());
        l
    });
    store::changed();
    if let Some(tr) = &tax {
        tax_messages(p, &conf, &eco.name, tr, "tax-sell-applied");
    }
    let items = display(&conf, &listing.item);
    let price_text = conf.format_price(&eco.name, price);
    msg(p, "item-sold", &[("%price%", price_text.clone()), ("%items%", items.clone())]);
    if conf.broadcast_sell {
        let except = conf.broadcast_sell_exclude.then_some(uuid.as_str());
        broadcast("broadcast-sell", &[("%seller%", p.get_name().to_owned()), ("%items%", items), ("%price%", price_text)], except, "broadcast_sell");
    }
    crate::refresh_viewers();
}

// ---- Buying, taking back ----

/// Buys a listing: false when it couldn't (money, gone).
pub fn purchase(p: &Player, id: u64) -> bool {
    let conf = crate::conf();
    let Some(l) = store::with(|d| d.listed.iter().find(|l| l.id == id).cloned()) else {
        msg(p, "item-no-longer-available", &[]);
        return false;
    };
    let uuid = p.get_unique_id();
    if l.seller == uuid {
        return remove_listed(p, id);
    }
    if l.expire > 0 && l.expire <= text::now_ms() {
        msg(p, "item-no-longer-available", &[]);
        return false;
    }
    let eco = conf.economy(&l.economy).cloned().unwrap_or_default();
    let tax = eco.tax.compute(&|perm| p.has_permission(perm), l.price, false);
    let (pays, gets) = match &tax {
        Some(t) if !t.bypassed && t.kind == "CAPITALISM" => (l.price + t.tax, l.price),
        Some(t) if !t.bypassed => (l.price, (l.price - t.tax).max(0.0)),
        _ => (l.price, l.price),
    };
    if !economy::has(p.get_name(), pays) || !economy::withdraw(p.get_name(), pays) {
        if conf.money_message {
            msg(p, "not-enough-money", &[]);
        }
        if let Some((sound, v, pitch)) = &conf.money_sound {
            p.play_sound(&p.get_location(), sound, *v, *pitch);
        }
        return false;
    }
    // Taken off the market (it may have gone meanwhile: then the money goes back).
    let taken = store::with(|d| d.listed.iter().position(|x| x.id == id).map(|i| d.listed.remove(i)));
    let Some(l) = taken else {
        economy::deposit(p.get_name(), pays);
        msg(p, "item-no-longer-available", &[]);
        return false;
    };
    let items = display(&conf, &l.item);
    let price_text = conf.format_price(&l.economy, l.price);
    let seller = online(&l.seller);
    if let Some(s) = &seller {
        economy::deposit(s.get_name(), gets);
        msg(s, "item-bought-seller", &[("%buyer%", p.get_name().to_owned()), ("%items%", items.clone()), ("%price%", price_text.clone())]);
        if let Some(t) = &tax
            && t.kind != "CAPITALISM"
        {
            tax_messages(s, &conf, &l.economy, t, "tax-purchase-applied");
        }
    }
    let now = text::now_ms();
    let mut stack = store::item_from_json(&l.item);
    if conf.purchased_give
        && let Some(s) = &stack
        && (!conf.purchased_free_space || has_space(p))
    {
        stack = give(p, s);
    } else if conf.purchased_give {
        msg(p, "purchase-inventory-full", &[]);
    }
    store::with(|d| {
        if seller.is_none() {
            *d.pending.entry(l.seller.clone()).or_default().entry(l.economy.clone()).or_default() += gets;
        }
        let sale = Sale { id: d.next(), seller: l.seller.clone(), buyer: uuid.clone(), buyer_name: p.get_name().to_owned(), item: l.item.clone(), price: l.price, economy: l.economy.clone(), date: now, notified: seller.is_some() };
        d.history.push(sale);
        let mine: Vec<usize> = d.history.iter().enumerate().filter(|(_, s)| s.seller == l.seller).map(|(i, _)| i).collect();
        if mine.len() > conf.history_max {
            let drop = mine[0];
            d.history.remove(drop);
        }
        if let Some(rest) = stack {
            let mut kept = l.clone();
            kept.item = store::item_to_json(&rest);
            kept.buyer = Some(uuid.clone());
            kept.buyer_name = Some(p.get_name().to_owned());
            kept.expire = if conf.purchase_keep > 0 { now + conf.purchase_keep * 1000 } else { 0 };
            d.purchased.push(kept);
        }
    });
    store::changed();
    msg(p, "item-bought-buyer", &[("%items%", items.clone()), ("%price%", price_text.clone())]);
    if let Some(t) = &tax
        && t.kind == "CAPITALISM"
    {
        tax_messages(p, &conf, &l.economy, t, "tax-capitalism-info");
    }
    if conf.broadcast_purchase {
        let except = conf.broadcast_purchase_exclude.then_some(uuid.as_str());
        broadcast("broadcast-purchase", &[("%buyer%", p.get_name().to_owned()), ("%items%", items), ("%price%", price_text)], except, "broadcast_purchase");
    }
    crate::refresh_viewers();
    true
}

/// The seller takes a listing off the market: into their inventory or their expired items.
pub fn remove_listed(p: &Player, id: u64) -> bool {
    let conf = crate::conf();
    let uuid = p.get_unique_id();
    let Some(mut l) = store::with(|d| d.listed.iter().position(|x| x.id == id && x.seller == uuid).map(|i| d.listed.remove(i))) else {
        msg(p, "item-no-longer-available", &[]);
        return false;
    };
    let items = display(&conf, &l.item);
    let mut rest = None;
    if conf.remove_listed_give {
        rest = store::item_from_json(&l.item).and_then(|s| give(p, &s));
    } else {
        rest = rest.or_else(|| store::item_from_json(&l.item));
    }
    if let Some(r) = rest {
        l.item = store::item_to_json(&r);
        l.expire = if conf.expire_keep > 0 { text::now_ms() + conf.expire_keep * 1000 } else { 0 };
        store::with(|d| d.expired.push(l));
    }
    store::changed();
    msg(p, "item-remove-listed", &[("%items%", items)]);
    crate::refresh_viewers();
    true
}

/// Takes an expired / purchased / own listed item back into the inventory.
pub fn claim(p: &Player, kind: &str, id: u64, free_space: bool, key: &str) -> bool {
    let conf = crate::conf();
    let uuid = p.get_unique_id();
    if free_space && !has_space(p) {
        msg(p, "not-enough-space", &[]);
        return false;
    }
    let taken = store::with(|d| {
        let list = match kind {
            "expired" => &mut d.expired,
            "purchased" => &mut d.purchased,
            _ => &mut d.listed,
        };
        let owner = |l: &Listing| if kind == "purchased" { l.buyer.as_deref() == Some(uuid.as_str()) } else { l.seller == uuid };
        list.iter().position(|l| l.id == id && owner(l)).map(|i| list.remove(i))
    });
    let Some(l) = taken else {
        msg(p, "item-no-longer-available", &[]);
        return false;
    };
    let items = display(&conf, &l.item);
    if let Some(stack) = store::item_from_json(&l.item)
        && let Some(rest) = give(p, &stack)
    {
        let mut back = l.clone();
        back.item = store::item_to_json(&rest);
        store::with(|d| match kind {
            "purchased" => d.purchased.push(back),
            _ => d.expired.push(back),
        });
    }
    store::changed();
    msg(p, key, &[("%items%", items)]);
    if kind == "listed" {
        crate::refresh_viewers();
    }
    true
}

/// `ZAUCTIONHOUSE_REMOVE_ALL_*`.
fn remove_all(p: &Player, kind: &str) {
    let uuid = p.get_unique_id();
    let ids: Vec<u64> = store::with(|d| match kind {
        "expired" => d.expired.iter().filter(|l| l.seller == uuid).map(|l| l.id).collect(),
        "purchased" => d.purchased.iter().filter(|l| l.buyer.as_deref() == Some(uuid.as_str())).map(|l| l.id).collect(),
        _ => d.listed.iter().filter(|l| l.seller == uuid).map(|l| l.id).collect(),
    });
    let mut done = 0;
    let mut failed = 0;
    for id in ids {
        if !has_space(p) {
            failed += 1;
            continue;
        }
        let k = if kind == "selling" { "listed" } else { kind };
        let taken = store::with(|d| {
            let list = match k {
                "expired" => &mut d.expired,
                "purchased" => &mut d.purchased,
                _ => &mut d.listed,
            };
            list.iter().position(|l| l.id == id).map(|i| list.remove(i))
        });
        if let Some(l) = taken {
            if let Some(stack) = store::item_from_json(&l.item)
                && let Some(rest) = give(p, &stack)
            {
                let mut back = l.clone();
                back.item = store::item_to_json(&rest);
                store::with(|d| if k == "purchased" { d.purchased.push(back) } else { d.expired.push(back) });
                failed += 1;
                continue;
            }
            done += 1;
        }
    }
    store::changed();
    msg(p, "remove-all-items", &[("%amount%", done.to_string())]);
    if failed > 0 {
        msg(p, "remove-all-items-partial", &[("%amount%", failed.to_string())]);
    }
    if kind == "selling" {
        crate::refresh_viewers();
    }
}

/// An admin takes an item out of someone's lists (it goes to the admin).
fn admin_remove(admin: &Player, kind: &str, id: u64, target: &(String, String)) {
    let conf = crate::conf();
    let taken = store::with(|d| {
        let list = match kind {
            "expired" => &mut d.expired,
            "purchased" => &mut d.purchased,
            _ => &mut d.listed,
        };
        list.iter().position(|l| l.id == id).map(|i| list.remove(i))
    });
    let Some(l) = taken else {
        msg(admin, "admin-item-not-available", &[("%items%", "?".into())]);
        return;
    };
    store::changed();
    let items = display(&conf, &l.item);
    if let Some(stack) = store::item_from_json(&l.item)
        && let Some(rest) = give(admin, &stack)
    {
        admin.get_world().drop_item_naturally(&admin.get_location(), &rest);
    }
    msg(admin, "admin-item-removed", &[("%items%", items.clone()), ("%target%", target.1.clone())]);
    if kind == "listed"
        && let Some(s) = online(&l.seller)
    {
        msg(&s, "admin-listing-removed", &[("%items%", items)]);
    }
    crate::refresh_viewers();
}

// ---- Money waiting for sellers ----

/// `/ah claim` and auto-claim: what's pending paid in.
pub fn claim_money(p: &Player, quiet_when_none: bool) {
    let conf = crate::conf();
    let uuid = p.get_unique_id();
    let pending = store::with(|d| d.pending.remove(&uuid)).unwrap_or_default();
    let pending: Vec<(String, f64)> = pending.into_iter().filter(|(_, v)| *v > 0.0).collect();
    if pending.is_empty() {
        if !quiet_when_none {
            msg(p, "claim-no-pending", &[]);
        }
        return;
    }
    store::changed();
    let mut failed = HashMap::new();
    for (eco, amount) in pending {
        if economy::deposit(p.get_name(), amount) {
            let display = conf.economy(&eco).map_or(eco.clone(), |e| e.display.clone());
            msg(p, "claim-economy-success", &[("%amount%", conf.format_price(&eco, amount)), ("%economy%", display)]);
        } else {
            failed.insert(eco, amount);
        }
    }
    if !failed.is_empty() {
        store::with(|d| d.pending.insert(uuid, failed));
        store::changed();
    } else {
        msg(p, "claim-success", &[]);
    }
}

pub fn pending_total(uuid: &str) -> f64 {
    store::with(|d| d.pending.get(uuid).map(|m| m.values().sum()).unwrap_or(0.0))
}

/// After a join: pending money (auto-claimed or announced) and the sales made while away.
pub fn on_join(p: &Player) {
    let conf = crate::conf();
    let uuid = p.get_unique_id();
    if pending_total(&uuid) > 0.0 {
        if conf.auto_claim {
            claim_money(p, true);
        } else if conf.notify_pending {
            msg(p, "claim-pending-notify", &[("%amount%", conf.format_price(&conf.default_economy, pending_total(&uuid)))]);
        }
    }
    if conf.sales_notification {
        let (count, total) = store::with(|d| {
            let mut count = 0;
            let mut total = 0.0;
            for s in d.history.iter_mut().filter(|s| s.seller == uuid && !s.notified) {
                s.notified = true;
                count += 1;
                total += s.price;
            }
            (count, total)
        });
        if count > 0 {
            store::changed();
            msg(p, "sales-notification", &[("%count%", count.to_string()), ("%total%", conf.format_price(&conf.default_economy, total))]);
        }
    }
}

/// Listings past their time go to their seller's expired items; expired and purchased items past theirs go away.
pub fn sweep() {
    let conf = crate::conf();
    let now = text::now_ms();
    let moved = store::with(|d| {
        let mut moved = 0;
        let mut i = 0;
        while i < d.listed.len() {
            if d.listed[i].expire > 0 && d.listed[i].expire <= now {
                let mut l = d.listed.remove(i);
                l.expire = if conf.expire_keep > 0 { now + conf.expire_keep * 1000 } else { 0 };
                d.expired.push(l);
                moved += 1;
            } else {
                i += 1;
            }
        }
        let before = d.expired.len() + d.purchased.len();
        d.expired.retain(|l| l.expire <= 0 || l.expire > now);
        d.purchased.retain(|l| l.expire <= 0 || l.expire > now);
        moved + before - (d.expired.len() + d.purchased.len())
    });
    if moved > 0 {
        store::changed();
        crate::refresh_viewers();
    }
}

// ---- Windows ----

fn fresh_state(conf: &Conf) -> PState {
    PState { category: "all".into(), sort: conf.default_sort.clone(), history_sort: "DATE_DESC".into(), ..PState::default() }
}

pub fn category_counts(conf: &Conf) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    store::with(|d| {
        counts.insert("all".into(), d.listed.len());
        for l in &d.listed {
            for c in &l.categories {
                *counts.entry(c.clone()).or_default() += 1;
            }
        }
    });
    for c in &conf.categories {
        counts.entry(c.id.clone()).or_default();
    }
    counts
}

/// Opens (or redraws) an inventory for a player; `push` keeps where they were for BACK.
pub fn open(p: &Player, nav: Nav, push: bool) {
    let conf = crate::conf();
    let Some(def) = crate::inventory(&nav.inv) else {
        return msg(p, "inventory-not-found", &[("%inventory-name%", nav.inv.clone())]);
    };
    let counts = category_counts(&conf);
    let uuid = p.get_unique_id();
    let mut ps = crate::states(|s| s.remove(&uuid)).unwrap_or_else(|| fresh_state(&conf));
    if push && let Some(v) = &ps.view {
        ps.stack.push(v.nav.clone());
    }
    gui::render(p, &conf, &def, nav, &mut ps, &counts);
    crate::states(|s| s.insert(uuid, ps));
}

/// `/ah`: the auction house, category and search reset as configured.
pub fn open_main(p: &Player, page: usize) {
    let conf = crate::conf();
    let uuid = p.get_unique_id();
    crate::states(|s| {
        let ps = s.entry(uuid).or_insert_with(|| fresh_state(&conf));
        ps.category = "all".into();
        ps.search = None;
        ps.stack.clear();
        ps.view = None;
    });
    open(p, Nav { inv: "auction".into(), page, ..Nav::default() }, false);
}

/// The view a player has open now.
fn current(p: &Player) -> Option<Nav> {
    crate::states(|s| s.get(&p.get_unique_id()).and_then(|ps| ps.view.as_ref().map(|v| v.nav.clone())))
}

fn redraw(p: &Player) {
    if let Some(nav) = current(p) {
        open(p, nav, false);
    }
}

fn back(p: &Player) {
    let prev = crate::states(|s| s.get_mut(&p.get_unique_id()).and_then(|ps| ps.stack.pop()));
    match prev {
        Some(nav) => open(p, nav, false),
        None => {
            crate::states(|s| {
                if let Some(ps) = s.get_mut(&p.get_unique_id()) {
                    ps.view = None;
                }
            });
            p.close_inventory();
        }
    }
}

/// A button's action.
pub fn act(p: &Player, act: Act, right: bool) {
    let conf = crate::conf();
    let Some(nav) = current(p) else { return };
    match act {
        Act::Open(inv) => open(p, Nav { inv, page: 1, target: None, admin: nav.admin.clone() }, true),
        Act::Back => back(p),
        Act::Prev => open(p, Nav { page: nav.page.saturating_sub(1).max(1), ..nav }, false),
        Act::Next => open(p, Nav { page: nav.page + 1, ..nav }, false),
        Act::Category(id) => {
            crate::states(|s| {
                if let Some(ps) = s.get_mut(&p.get_unique_id()) {
                    ps.category = id;
                }
            });
            open(p, Nav { inv: "auction".into(), page: 1, ..Nav::default() }, true);
        }
        Act::Refresh => redraw(p),
        Act::Sort(sorts) | Act::HistorySort(sorts) if !sorts.is_empty() => {
            let history = nav.inv == "history";
            crate::states(|s| {
                if let Some(ps) = s.get_mut(&p.get_unique_id()) {
                    let cur = if history { &mut ps.history_sort } else { &mut ps.sort };
                    let i = sorts.iter().position(|x| x == cur).map_or(0, |i| (i + if right { sorts.len() - 1 } else { 1 }) % sorts.len());
                    *cur = sorts[i].clone();
                }
            });
            redraw(p);
        }
        Act::Sort(_) | Act::HistorySort(_) => {}
        Act::Search => {
            if right {
                crate::states(|s| {
                    if let Some(ps) = s.get_mut(&p.get_unique_id()) {
                        ps.search = None;
                    }
                });
                msg(p, "search-cleared", &[]);
                open(p, Nav { page: 1, ..nav }, false);
            } else {
                crate::AWAITING.lock().unwrap_or_else(|e| e.into_inner()).insert(p.get_unique_id());
                crate::states(|s| {
                    if let Some(ps) = s.get_mut(&p.get_unique_id()) {
                        ps.view = None;
                    }
                });
                p.close_inventory();
                msg(p, "search-start", &[]);
            }
        }
        Act::Buy(id) => {
            if store::with(|d| d.listed.iter().any(|l| l.id == id)) {
                open(p, Nav { inv: "purchase-confirm".into(), page: 1, target: Some(id), admin: None }, true);
            } else {
                msg(p, "item-no-longer-available", &[]);
                redraw(p);
            }
        }
        Act::RemoveOwn(id) => {
            if conf.remove_listed_confirm {
                open(p, Nav { inv: "remove-confirm".into(), page: 1, target: Some(id), admin: None }, true);
            } else {
                remove_listed(p, id);
                if conf.remove_listed_open { redraw(p) } else { p.close_inventory() }
            }
        }
        Act::ConfirmPurchase => {
            let Some(id) = nav.target else { return };
            let done = purchase(p, id);
            if done && !conf.purchased_open {
                p.close_inventory();
            } else {
                back(p);
            }
        }
        Act::ConfirmRemove => {
            let Some(id) = nav.target else { return };
            remove_listed(p, id);
            if conf.remove_listed_open { back(p) } else { p.close_inventory() }
        }
        Act::ClaimExpired(id) => {
            claim(p, "expired", id, conf.expired_free_space, "item-remove-expired");
            if conf.expired_open { redraw(p) } else { p.close_inventory() }
        }
        Act::ClaimPurchased(id) => {
            claim(p, "purchased", id, conf.purchased_free_space, "item-remove-purchased");
            if conf.purchased_open { redraw(p) } else { p.close_inventory() }
        }
        Act::RemoveSelling(id) => {
            claim(p, "listed", id, conf.selling_free_space, "item-remove-selling");
            if conf.selling_open { redraw(p) } else { p.close_inventory() }
        }
        Act::RemoveAll(kind) => {
            remove_all(p, kind);
            redraw(p);
        }
        Act::AdminRemove(kind, id) => {
            if let Some(target) = nav.admin.clone() {
                admin_remove(p, kind, id, &target);
                redraw(p);
            }
        }
    }
}

/// A search typed in chat.
pub fn search(p: &Player, query: &str) {
    let q = query.trim().to_owned();
    crate::states(|s| {
        if let Some(ps) = s.get_mut(&p.get_unique_id()) {
            ps.search = (!q.is_empty()).then(|| q.clone());
            ps.stack.clear();
            ps.view = None;
        }
    });
    msg(p, "search-searching", &[("%query%", q.clone())]);
    let found = store::with(|d| d.listed.iter().any(|l| gui::matches_search(&crate::conf(), l, &q)));
    if !found {
        msg(p, "search-no-results", &[("%query%", q)]);
    }
    open(p, Nav { inv: "auction".into(), page: 1, ..Nav::default() }, false);
}

/// `/ah admin add <player> <listed|expired|purchased> [price]`: the admin's held item into someone's list.
pub fn admin_add(admin: &Player, target: (String, String), kind: &str, price: f64) {
    let conf = crate::conf();
    let Some(held) = admin.get_inventory().get_item_in_main_hand().filter(|s| !s.is_empty()) else { return msg(admin, "sell-error-air", &[]) };
    let item = store::item_to_json(&held);
    let now = text::now_ms();
    let categories = categorize(&conf, &item);
    let items = display(&conf, &item);
    store::with(|d| {
        let mut l = Listing { id: d.next(), seller: target.0.clone(), seller_name: target.1.clone(), item, price, economy: conf.default_economy.clone(), created: now, expire: 0, buyer: None, buyer_name: None, categories };
        match kind {
            "expired" => {
                l.expire = if conf.expire_keep > 0 { now + conf.expire_keep * 1000 } else { 0 };
                d.expired.push(l);
            }
            "purchased" => {
                l.buyer = Some(target.0.clone());
                l.buyer_name = Some(target.1.clone());
                l.expire = if conf.purchase_keep > 0 { now + conf.purchase_keep * 1000 } else { 0 };
                d.purchased.push(l);
            }
            _ => {
                l.expire = if conf.expiration > 0 { now + conf.expiration * 1000 } else { 0 };
                d.listed.push(l);
            }
        }
    });
    store::changed();
    msg(admin, "admin-item-added", &[("%items%", items), ("%target%", target.1), ("%type%", kind.to_owned())]);
    crate::refresh_viewers();
}
