//! The inventories: zMenu's format as zAuctionHouse ships it (`inventories/*.yml`, `patterns/*.yml`): a title,
//! a size, patterns, and buttons with a `type`, `slot`/`slots`, an `item` and an `else`. The zAuctionHouse button
//! types are drawn here from the auction data; a click runs its action on the next tick.

use crate::conf::Conf;
use crate::store::{self, Listing, Sale};
use crate::text;
use crate::yaml::{self, at};
use base64::Engine;
use horizon_bukkit::fam_ev_player::InventoryClickEvent;
use horizon_bukkit::{Inventory, ItemStack, Material, Player};
use serde_json::json;
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Button {
    pub kind: String,
    pub slots: Vec<usize>,
    pub item: Option<Yaml>,
    pub else_: Option<Box<Button>>,
    pub opts: Yaml,
}

#[derive(Clone, Debug)]
pub struct InvDef {
    pub title: String,
    pub size: usize,
    pub buttons: Vec<Button>,
}

fn slots(v: &Yaml) -> Vec<usize> {
    let mut out = Vec::new();
    match at(v, "slots") {
        Some(Yaml::Sequence(items)) => {
            for s in items.iter().filter_map(yaml::scalar) {
                match s.split_once('-') {
                    Some((a, b)) => {
                        if let (Ok(a), Ok(b)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>()) {
                            out.extend(a..=b);
                        }
                    }
                    None => out.extend(s.trim().parse::<usize>().ok()),
                }
            }
        }
        _ => {
            if yaml::contains(v, "slot") {
                out.push(yaml::int(v, "slot", 0).max(0) as usize);
            }
        }
    }
    out
}

fn button(v: &Yaml, inherit: &[usize]) -> Button {
    let mut s = slots(v);
    if s.is_empty() {
        s = inherit.to_vec();
    }
    Button {
        kind: yaml::string(v, "type").unwrap_or_default().to_ascii_uppercase(),
        else_: at(v, "else").filter(|e| e.is_mapping()).map(|e| Box::new(button(e, &s))),
        item: at(v, "item").cloned(),
        slots: s,
        opts: v.clone(),
    }
}

fn buttons_of(doc: &Yaml) -> Vec<Button> {
    yaml::keys(doc, "items").into_iter().filter_map(|k| at(doc, &format!("items.{k}")).map(|v| button(v, &[]))).collect()
}

/// Every inventory by its file name (`auction`, `purchase-confirm`), patterns merged in.
pub fn load_all(folder: &Path) -> HashMap<String, Arc<InvDef>> {
    let mut patterns: HashMap<String, Vec<Button>> = HashMap::new();
    for entry in walk(&folder.join("patterns")) {
        if let Some(doc) = read(&entry) {
            let name = yaml::string(&doc, "name").unwrap_or_else(|| stem(&entry));
            patterns.insert(name, buttons_of(&doc));
        }
    }
    let mut out = HashMap::new();
    for entry in walk(&folder.join("inventories")) {
        let Some(doc) = read(&entry) else { continue };
        let mut buttons = Vec::new();
        for p in yaml::list(&doc, "patterns") {
            if let Some(b) = patterns.get(&p) {
                buttons.extend(b.iter().cloned());
            }
        }
        buttons.extend(buttons_of(&doc));
        let size = (yaml::int(&doc, "size", 54).clamp(9, 54) as usize) / 9 * 9;
        out.insert(stem(&entry), Arc::new(InvDef { title: yaml::string(&doc, "name").unwrap_or_default(), size, buttons }));
    }
    out
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn read(p: &Path) -> Option<Yaml> {
    horizon_kit::yaml::parse(&std::fs::read_to_string(p).ok()?).ok()
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else if p.extension().is_some_and(|x| x == "yml") {
            out.push(p);
        }
    }
    out
}

/// zAuctionHouse's inventory names (`SELLING_ITEMS`) to their files.
pub fn file_of(name: &str) -> String {
    match name.to_ascii_uppercase().as_str() {
        "AUCTION" => "auction".into(),
        "CATEGORIES" => "categories".into(),
        "EXPIRED_ITEMS" => "expired-items".into(),
        "PURCHASED_ITEMS" => "purchased-items".into(),
        "SELLING_ITEMS" => "selling-items".into(),
        "HISTORY" => "history".into(),
        "PURCHASE_CONFIRM" => "purchase-confirm".into(),
        "REMOVE_CONFIRM" => "remove-confirm".into(),
        _ => name.to_ascii_lowercase(),
    }
}

// ---- What a player sees ----

#[derive(Clone, Debug)]
pub enum Act {
    Open(String),
    Back,
    Prev,
    Next,
    Category(String),
    Refresh,
    Sort(Vec<String>),
    HistorySort(Vec<String>),
    Search,
    Buy(u64),
    RemoveOwn(u64),
    ClaimExpired(u64),
    ClaimPurchased(u64),
    RemoveSelling(u64),
    ConfirmPurchase,
    ConfirmRemove,
    RemoveAll(&'static str),
    AdminRemove(&'static str, u64),
}

#[derive(Clone, Debug, Default)]
pub struct Nav {
    pub inv: String,
    pub page: usize,
    pub target: Option<u64>,
    /// An admin looking at someone else's items: (uuid, name).
    pub admin: Option<(String, String)>,
}

pub struct View {
    pub nav: Nav,
    pub inv: Inventory,
    pub acts: HashMap<usize, Act>,
}

#[derive(Default)]
pub struct PState {
    pub category: String,
    pub sort: String,
    pub history_sort: String,
    pub search: Option<String>,
    pub stack: Vec<Nav>,
    pub view: Option<View>,
    pub awaiting_search: bool,
}

/// An item from a zMenu `item:` section, its texts filled by `fill`.
pub fn item_of(section: &Yaml, conf: &Conf, fill: &dyn Fn(&str) -> String) -> Option<ItemStack> {
    let mut stack = if let Some(url) = yaml::string(section, "url") {
        let mut s = ItemStack::new(Material::of("PLAYER_HEAD"), 1);
        let value = if url.starts_with("http") {
            base64::engine::general_purpose::STANDARD.encode(format!("{{\"textures\":{{\"SKIN\":{{\"url\":\"{url}\"}}}}}}"))
        } else {
            url
        };
        s.set_component("minecraft:profile", json!({ "properties": [{ "name": "textures", "value": value }] }));
        s
    } else if let Some(head) = yaml::string(section, "player-head") {
        let mut s = ItemStack::new(Material::of("PLAYER_HEAD"), 1);
        s.set_component("minecraft:profile", json!({ "name": fill(&head) }));
        s
    } else {
        let m = fill(&yaml::string(section, "material").unwrap_or_else(|| "STONE".into()));
        let mat = Material::get_material(&m.to_ascii_uppercase()).or_else(|| m.contains(':').then(|| Material::of(&m)))?;
        if mat.is_air() {
            return None;
        }
        ItemStack::new(mat, 1)
    };
    stack.set_amount(yaml::int(section, "amount", 1).clamp(1, 99) as i32);
    if let Some(name) = yaml::string(section, "name").or_else(|| yaml::string(section, "display-name")) {
        text::set_name(&mut stack, &fill(&name), &conf.colors);
    }
    let lore: Vec<String> = yaml::list(section, "lore").iter().map(|l| fill(l)).flat_map(|l| l.split('\n').map(str::to_owned).collect::<Vec<_>>()).collect();
    if !lore.is_empty() {
        text::set_lore(&mut stack, &lore, &conf.colors);
    }
    if yaml::boolean(section, "glow", false) {
        stack.set_component("minecraft:enchantment_glint_override", json!(true));
    }
    for key in ["modelID", "model-id", "custom-model-data", "customModelData"] {
        let n = yaml::int(section, key, 0);
        if n != 0 {
            stack.set_custom_model_data(Some(n as i32));
        }
    }
    stack.set_component("minecraft:tooltip_display", json!({ "hidden_components": ["minecraft:attribute_modifiers", "minecraft:enchantments"] }));
    Some(stack)
}

/// One list entry (a listed, expired, purchased, selling or history item) and its click.
struct Entry {
    stack: ItemStack,
    act: Option<Act>,
}

const LIST_KINDS: [&str; 5] = ["ZAUCTIONHOUSE_LISTED_ITEMS", "ZAUCTIONHOUSE_EXPIRED_ITEMS", "ZAUCTIONHOUSE_PURCHASED_ITEMS", "ZAUCTIONHOUSE_SELLING_ITEMS", "ZAUCTIONHOUSE_HISTORY_ITEMS"];

fn remaining(conf: &Conf, expire: i64) -> String {
    if expire <= 0 {
        return "∞".into();
    }
    let secs = (expire - text::now_ms()) / 1000;
    text::duration(secs, &conf.time[0], &conf.time[1], &conf.time[2], &conf.time[3])
}

/// A listing's stack with the configured lore under it.
pub fn listing_stack(conf: &Conf, l: &Listing, lore: &[String], status: &str, buyer: &str) -> Option<ItemStack> {
    let mut stack = store::item_from_json(&l.item)?;
    let pairs = [
        ("%seller%", l.seller_name.clone()),
        ("%price%", conf.format_price(&l.economy, l.price)),
        ("%time-remaining%", remaining(conf, l.expire)),
        ("%status%", status.to_owned()),
        ("%buyer%", buyer.to_owned()),
        ("%amount%", store::item_amount(&l.item).to_string()),
        ("%economy%", l.economy.clone()),
        ("%date%", text::date(l.created, &conf.date_format)),
    ];
    let lines: Vec<String> = lore.iter().map(|x| text::fill(x, &pairs)).collect();
    text::add_lore(&mut stack, &lines, &conf.colors);
    Some(stack)
}

fn sale_stack(conf: &Conf, s: &Sale) -> Option<ItemStack> {
    let mut stack = store::item_from_json(&s.item)?;
    let pairs = [("%buyer%", s.buyer_name.clone()), ("%price%", conf.format_price(&s.economy, s.price)), ("%date%", text::date(s.date, &conf.date_format)), ("%amount%", store::item_amount(&s.item).to_string())];
    let lines: Vec<String> = conf.lore_history.iter().map(|x| text::fill(x, &pairs)).collect();
    text::add_lore(&mut stack, &lines, &conf.colors);
    Some(stack)
}

/// Whether a listing matches a search (`field op value`, or plain words in its name or material).
pub fn matches_search(conf: &Conf, l: &Listing, query: &str) -> bool {
    let name = item_name(&l.item).unwrap_or_default();
    let material = store::item_material(&l.item);
    let lore = item_lore(&l.item).join("\n");
    let mut ops: Vec<(&str, &str)> = conf.filters.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    ops.sort_by_key(|(_, s)| std::cmp::Reverse(s.len()));
    for (kind, sym) in ops {
        if let Some((field, value)) = query.split_once(&format!(" {sym} ")) {
            let have = match field.trim().to_ascii_lowercase().as_str() {
                "name" => name.clone(),
                "material" => material.clone(),
                "lore" => lore.clone(),
                "seller" => l.seller_name.clone(),
                _ => continue,
            };
            let value = value.trim();
            return match kind {
                "EQUALS" => have == value,
                "CONTAINS_IGNORE_CASE" => have.to_lowercase().contains(&value.to_lowercase()),
                "EQUALS_IGNORE_CASE" => have.eq_ignore_ascii_case(value),
                _ => have.contains(value),
            };
        }
    }
    let q = query.to_lowercase();
    name.to_lowercase().contains(&q) || material.to_lowercase().replace('_', " ").contains(&q.replace('_', " ")) || l.seller_name.to_lowercase() == q
}

/// The custom name of a stored item, as plain text.
pub fn item_name(item: &serde_json::Value) -> Option<String> {
    let c = item["components"].get("minecraft:custom_name").or_else(|| item["components"].get("minecraft:item_name"))?;
    Some(horizon_bukkit::Component(c.clone()).plain())
}

pub fn item_lore(item: &serde_json::Value) -> Vec<String> {
    item["components"]["minecraft:lore"].as_array().into_iter().flatten().map(|c| horizon_bukkit::Component(c.clone()).plain()).collect()
}

fn sort_listings(list: &mut [Listing], sort: &str) {
    match sort {
        "ASCENDING_PRICE" => list.sort_by(|a, b| a.price.total_cmp(&b.price)),
        "DECREASING_PRICE" => list.sort_by(|a, b| b.price.total_cmp(&a.price)),
        "ASCENDING_DATE" => list.sort_by_key(|l| l.created),
        _ => list.sort_by_key(|l| std::cmp::Reverse(l.created)),
    }
}

fn sort_sales(list: &mut [Sale], sort: &str) {
    match sort {
        "DATE_ASC" => list.sort_by_key(|s| s.date),
        "PRICE_DESC" => list.sort_by(|a, b| b.price.total_cmp(&a.price)),
        "PRICE_ASC" => list.sort_by(|a, b| a.price.total_cmp(&b.price)),
        "BUYER_ASC" => list.sort_by(|a, b| a.buyer_name.to_lowercase().cmp(&b.buyer_name.to_lowercase())),
        "BUYER_DESC" => list.sort_by(|a, b| b.buyer_name.to_lowercase().cmp(&a.buyer_name.to_lowercase())),
        _ => list.sort_by_key(|s| std::cmp::Reverse(s.date)),
    }
}

/// What one player's lists hold now (copied out of the store, so drawing holds no lock).
pub struct Snapshot {
    pub listed: Vec<Listing>,
    pub own_selling: Vec<Listing>,
    pub expired: Vec<Listing>,
    pub purchased: Vec<Listing>,
    pub sales: Vec<Sale>,
    pub target: Option<Listing>,
    pub total_listed: usize,
}

pub fn snapshot(owner: &str, nav: &Nav, ps: &PState, conf: &Conf, need_listed: bool) -> Snapshot {
    store::with(|d| {
        let mut listed = Vec::new();
        if need_listed {
            listed = d
                .listed
                .iter()
                .filter(|l| ps.category.is_empty() || ps.category == "all" || l.categories.iter().any(|c| *c == ps.category))
                .filter(|l| ps.search.as_ref().is_none_or(|q| matches_search(conf, l, q)))
                .cloned()
                .collect();
            sort_listings(&mut listed, &ps.sort);
        }
        let mut own_selling: Vec<Listing> = d.listed.iter().filter(|l| l.seller == owner).cloned().collect();
        own_selling.sort_by_key(|l| std::cmp::Reverse(l.created));
        let mut expired: Vec<Listing> = d.expired.iter().filter(|l| l.seller == owner).cloned().collect();
        expired.sort_by_key(|l| std::cmp::Reverse(l.expire));
        let mut purchased: Vec<Listing> = d.purchased.iter().filter(|l| l.buyer.as_deref() == Some(owner)).cloned().collect();
        purchased.sort_by_key(|l| std::cmp::Reverse(l.expire));
        let mut sales: Vec<Sale> = d.history.iter().filter(|s| s.seller == owner).cloned().collect();
        sort_sales(&mut sales, &ps.history_sort);
        let target = nav.target.and_then(|id| d.listed.iter().find(|l| l.id == id).cloned());
        Snapshot { listed, own_selling, expired, purchased, sales, target, total_listed: d.listed.len() }
    })
}

/// Draws `nav` for `player`: the window opened (or its contents replaced) and the view kept in `ps`.
pub fn render(player: &Player, conf: &Conf, def: &InvDef, nav: Nav, ps: &mut PState, counts: &HashMap<String, usize>) {
    let viewer_uuid = player.get_unique_id();
    let owner = nav.admin.as_ref().map_or(viewer_uuid.clone(), |(u, _)| u.clone());
    let list_kind = def.buttons.iter().find(|b| LIST_KINDS.contains(&b.kind.as_str())).map(|b| b.kind.clone());
    let snap = snapshot(&owner, &nav, ps, conf, list_kind.as_deref() == Some("ZAUCTIONHOUSE_LISTED_ITEMS"));
    let admin = nav.admin.is_some();
    // The entries of the list this inventory shows.
    let entries: Vec<Entry> = match list_kind.as_deref() {
        Some("ZAUCTIONHOUSE_LISTED_ITEMS") => snap
            .listed
            .iter()
            .filter_map(|l| {
                let own = l.seller == viewer_uuid;
                let status = if own { &conf.status_seller } else { &conf.status_buyer };
                let stack = listing_stack(conf, l, &conf.lore_listed, status, "")?;
                Some(Entry { stack, act: Some(if own { Act::RemoveOwn(l.id) } else { Act::Buy(l.id) }) })
            })
            .collect(),
        Some("ZAUCTIONHOUSE_SELLING_ITEMS") => snap
            .own_selling
            .iter()
            .filter_map(|l| Some(Entry { stack: listing_stack(conf, l, &conf.lore_selling, "", "")?, act: Some(if admin { Act::AdminRemove("listed", l.id) } else { Act::RemoveSelling(l.id) }) }))
            .collect(),
        Some("ZAUCTIONHOUSE_EXPIRED_ITEMS") => snap
            .expired
            .iter()
            .filter_map(|l| Some(Entry { stack: listing_stack(conf, l, &conf.lore_expired, "", "")?, act: Some(if admin { Act::AdminRemove("expired", l.id) } else { Act::ClaimExpired(l.id) }) }))
            .collect(),
        Some("ZAUCTIONHOUSE_PURCHASED_ITEMS") => snap
            .purchased
            .iter()
            .filter_map(|l| Some(Entry { stack: listing_stack(conf, l, &conf.lore_purchased, "", l.buyer_name.as_deref().unwrap_or(""))?, act: Some(if admin { Act::AdminRemove("purchased", l.id) } else { Act::ClaimPurchased(l.id) }) }))
            .collect(),
        Some("ZAUCTIONHOUSE_HISTORY_ITEMS") => snap.sales.iter().filter_map(|s| Some(Entry { stack: sale_stack(conf, s)?, act: None })).collect(),
        _ => Vec::new(),
    };
    let per_page = def.buttons.iter().find(|b| list_kind.as_deref() == Some(b.kind.as_str())).map_or(1, |b| b.slots.len().max(1));
    let max_page = entries.len().div_ceil(per_page).max(1);
    let page = nav.page.clamp(1, max_page);
    let nav = Nav { page, ..nav };
    let category_name = if ps.category.is_empty() || ps.category == "all" { conf.all_category_name.clone() } else { conf.categories.iter().find(|c| c.id == ps.category).map_or(ps.category.clone(), |c| c.display.clone()) };
    let plural = |n: usize| if n > 1 { "s" } else { "" };
    let n_expired = snap.expired.len();
    let n_purchased = snap.purchased.len();
    let n_selling = snap.own_selling.len();
    let sort_name = conf.sort_names.get(&ps.sort).cloned().unwrap_or_else(|| ps.sort.clone());
    let base_pairs: Vec<(&str, String)> = vec![
        ("%page%", page.to_string()),
        ("%max-page%", max_page.to_string()),
        ("%zauctionhouse_category_name%", category_name.clone()),
        ("%zauctionhouse_listed_items%", snap.total_listed.to_string()),
        ("%zauctionhouse_sorting_name%", sort_name),
        ("%search_query%", ps.search.clone().unwrap_or_else(|| "None".into())),
        ("%target%", nav.admin.as_ref().map_or_else(|| player.get_name().to_owned(), |(_, n)| n.clone())),
        ("%player%", player.get_name().to_owned()),
    ];
    let fill_with = |t: &str, extra: &[(&str, String)]| -> String {
        let mut s = text::fill(t, &base_pairs);
        s = text::fill(&s, extra);
        if s.contains("%zauctionhouse_category_count_") {
            for (id, n) in counts {
                s = s.replace(&format!("%zauctionhouse_category_count_{id}%"), &n.to_string());
            }
        }
        if s.contains('%') { horizon_kit::hooks::placeholders(&s, Some(player)) } else { s }
    };
    let title = text::legacy(&fill_with(&def.title, &[]), &conf.colors);
    let mut inv = Inventory::create(def.size, &title);
    let mut acts: HashMap<usize, Act> = HashMap::new();
    let put = |inv: &mut Inventory, slot: usize, stack: Option<ItemStack>, act: Option<Act>, acts: &mut HashMap<usize, Act>| {
        if slot < def.size {
            inv.set_item(slot, stack);
            match act {
                Some(a) => {
                    acts.insert(slot, a);
                }
                None => {
                    acts.remove(&slot);
                }
            }
        }
    };
    for b in &def.buttons {
        let static_item = |b: &Button, extra: &[(&str, String)]| b.item.as_ref().and_then(|i| item_of(i, conf, &|t| fill_with(t, extra)));
        match b.kind.as_str() {
            k if Some(k) == list_kind.as_deref() => {
                let shown: Vec<&Entry> = entries.iter().skip((page - 1) * per_page).take(per_page).collect();
                if shown.is_empty() {
                    if let Some(e) = &b.else_ {
                        let stack = static_item(e, &[]);
                        for s in &e.slots {
                            put(&mut inv, *s, stack.clone(), None, &mut acts);
                        }
                    } else if yaml::contains(&b.opts, "empty-slot") {
                        put(&mut inv, yaml::int(&b.opts, "empty-slot", 22) as usize, static_item(b, &[]), None, &mut acts);
                    }
                } else {
                    for (slot, e) in b.slots.iter().zip(shown) {
                        put(&mut inv, *slot, Some(e.stack.clone()), e.act.clone(), &mut acts);
                    }
                }
            }
            "PREVIOUS" | "NEXT" => {
                let has = if b.kind == "PREVIOUS" { page > 1 } else { page < max_page };
                let chosen = if has { Some(b) } else { b.else_.as_deref() };
                if let Some(c) = chosen {
                    let stack = static_item(c, &[]);
                    let act = has.then(|| if b.kind == "PREVIOUS" { Act::Prev } else { Act::Next });
                    for s in &c.slots {
                        put(&mut inv, *s, stack.clone(), act.clone(), &mut acts);
                    }
                }
            }
            "ZAUCTIONHOUSE_EXPIRED_INVENTORY" | "ZAUCTIONHOUSE_PURCHASED_INVENTORY" | "ZAUCTIONHOUSE_SELLING_INVENTORY" => {
                let (n, target) = match b.kind.as_str() {
                    "ZAUCTIONHOUSE_EXPIRED_INVENTORY" => (n_expired, "expired-items"),
                    "ZAUCTIONHOUSE_PURCHASED_INVENTORY" => (n_purchased, "purchased-items"),
                    _ => (n_selling, "selling-items"),
                };
                let extra = [("%expired-items%", n_expired.to_string()), ("%purchased-items%", n_purchased.to_string()), ("%selling-items%", n_selling.to_string()), ("%s%", plural(n).to_owned())];
                let chosen = if n > 0 { Some(b) } else { b.else_.as_deref() };
                if let Some(c) = chosen {
                    let opens = n > 0 || yaml::boolean(&c.opts, "open-when-empty", false);
                    let stack = static_item(c, &extra);
                    for s in &c.slots {
                        put(&mut inv, *s, stack.clone(), opens.then(|| Act::Open(target.into())), &mut acts);
                    }
                }
            }
            "ZAUCTIONHOUSE_CATEGORY" => {
                let id = yaml::string(&b.opts, "category").unwrap_or_else(|| "misc".into());
                let stack = static_item(b, &[]);
                for s in &b.slots {
                    put(&mut inv, *s, stack.clone(), Some(Act::Category(id.clone())), &mut acts);
                }
            }
            "ZAUCTIONHOUSE_CHANGE_SORT" | "ZAUCTIONHOUSE_HISTORY_SORT" => {
                let sorts = yaml::list(&b.opts, "sorts").into_iter().map(|s| s.to_ascii_uppercase()).collect::<Vec<_>>();
                let current = if b.kind == "ZAUCTIONHOUSE_CHANGE_SORT" { &ps.sort } else { &ps.history_sort };
                let enable = yaml::string(&b.opts, "enable-text").unwrap_or_default();
                let disable = yaml::string(&b.opts, "disable-text").unwrap_or_default();
                let extra: Vec<(String, String)> = sorts
                    .iter()
                    .map(|s| {
                        let name = yaml::string(&b.opts, &format!("sort-names.{s}")).or_else(|| conf.sort_names.get(s).cloned()).unwrap_or_else(|| s.clone());
                        (format!("%{s}%"), (if s == current { &enable } else { &disable }).replace("%sorting%", &name))
                    })
                    .collect();
                let extra_ref: Vec<(&str, String)> = extra.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
                let stack = static_item(b, &extra_ref);
                let act = if b.kind == "ZAUCTIONHOUSE_CHANGE_SORT" { Act::Sort(sorts.clone()) } else { Act::HistorySort(sorts.clone()) };
                for s in &b.slots {
                    put(&mut inv, *s, stack.clone(), Some(act.clone()), &mut acts);
                }
            }
            "ZAUCTIONHOUSE_SHOW" => {
                let stack = snap.target.as_ref().and_then(|l| listing_stack(conf, l, &conf.lore_listed, "", ""));
                for s in &b.slots {
                    put(&mut inv, *s, stack.clone(), None, &mut acts);
                }
            }
            "ZAUCTIONHOUSE_SHULKER_OPEN" | "ZAUCTIONHOUSE_SHULKER_INFO" | "ZAUCTIONHOUSE_ITEM_CONTENT" => {
                // Container previews are not drawn by the Rust rewrite: the button only shows for shulker boxes.
                let shulker = snap.target.as_ref().is_some_and(|l| store::item_material(&l.item).ends_with("SHULKER_BOX"));
                if shulker {
                    let stack = static_item(b, &[]);
                    for s in &b.slots {
                        put(&mut inv, *s, stack.clone(), None, &mut acts);
                    }
                }
            }
            other => {
                let act = match other {
                    "BACK" => Some(Act::Back),
                    "INVENTORY" => yaml::string(&b.opts, "inventory").map(|i| Act::Open(file_of(&i))),
                    "ZAUCTIONHOUSE_REFRESH" => Some(Act::Refresh),
                    "ZAUCTIONHOUSE_SEARCH" => Some(Act::Search),
                    "ZAUCTIONHOUSE_CONFIRM_PURCHASE" => Some(Act::ConfirmPurchase),
                    "ZAUCTIONHOUSE_CONFIRM_REMOVE_LISTED" => Some(Act::ConfirmRemove),
                    "ZAUCTIONHOUSE_REMOVE_ALL_EXPIRED" => Some(Act::RemoveAll("expired")),
                    "ZAUCTIONHOUSE_REMOVE_ALL_PURCHASED" => Some(Act::RemoveAll("purchased")),
                    "ZAUCTIONHOUSE_REMOVE_ALL_SELLING" => Some(Act::RemoveAll("selling")),
                    "ZAUCTIONHOUSE_EXPIRED_ITEMS_INVENTORY" => Some(Act::Open("expired-items".into())),
                    _ => None,
                };
                let stack = static_item(b, &[]);
                for s in &b.slots {
                    put(&mut inv, *s, stack.clone(), act.clone(), &mut acts);
                }
            }
        }
    }
    let same_window = ps.view.as_ref().is_some_and(|v| v.nav.inv == nav.inv && v.inv.get_size() == inv.get_size() && v.inv.get_title() == inv.get_title());
    if same_window && let Some(v) = ps.view.as_mut() {
        // Same window: the contents swapped in place (no close/open flicker).
        let mut live = v.inv.clone();
        for slot in 0..inv.get_size() {
            let new = inv.get_item(slot).cloned();
            if live.get_item(slot) != new.as_ref() {
                live.set_item(slot, new);
            }
        }
        v.inv = live;
        v.acts = acts;
        v.nav = nav;
        return;
    }
    player.open_inventory(&inv);
    ps.view = Some(View { nav, inv, acts });
}

/// A click in one of these windows: cancelled, its action handed to the next tick.
pub fn on_click(event: &mut InventoryClickEvent) {
    let Some(player) = event.get_who_clicked() else { return };
    let uuid = player.get_unique_id();
    let gui = event.get_inventory().gui().cloned();
    let slot = usize::try_from(event.get_raw_slot()).ok();
    let right = event.is_right_click();
    let found = crate::states(|s| {
        let v = s.get(&uuid)?.view.as_ref()?;
        if !gui.as_ref().is_some_and(|g| g.same_as(&v.inv)) {
            return None;
        }
        Some(slot.and_then(|slot| v.acts.get(&slot).cloned()))
    });
    let Some(act) = found else { return };
    event.set_cancelled(true);
    let Some(act) = act else { return };
    crate::on_main(move || crate::ops::act(&player, act, right));
}
