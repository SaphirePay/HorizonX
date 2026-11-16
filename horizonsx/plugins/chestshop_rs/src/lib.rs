//! ChestShop 3.12 rewritten in Rust for Horizon: shop signs (`Name` / `64` / `B 10:5 S` / `Diamond`), admin
//! shops, item codes and aliases, partial transactions, taxes and fees, the built-in protection of shop signs
//! and containers, the same config.yml and language files, `/iteminfo`, `/shopinfo`, `/csgive`, `/csversion`,
//! `/cstoggle`, `/csaccess`, `/csmetrics`. Money goes through Vault (EssentialsX). Shop blocks are kept in an
//! index so hopper and explosion checks never look at the world; files are written on a background thread.
//! The Java it follows is in java/.

pub mod conf;
pub mod io;
pub mod items;
pub mod shop;
pub mod yaml;

use conf::Settings;
use horizon_bukkit::fam_ev_entity::EntityExplodeEvent;
use horizon_bukkit::fam_ev_player::{InventoryMoveItemEvent, PlayerInteractEvent};
use horizon_bukkit::fam_ev_world::{BlockBurnEvent, BlockExplodeEvent, BlockPistonEvent, BlockPistonExtendEvent, SignChangeEvent};
use horizon_bukkit::prelude::*;
use horizon_bukkit::{Block, CommandSender, Component, Player, Plugin, bukkit_plugin};
use horizon_kit::hooks::economy;
use items::ItemDb;
use serde_json::{Value, json};
use shop::ShopInv;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static SETTINGS: Mutex<Option<Arc<Settings>>> = Mutex::new(None);

#[derive(Clone, Debug)]
struct Account {
    uuid: String,
    name: String,
    short: String,
}

#[derive(Default)]
struct Data {
    accounts: Vec<Account>,
    items: ItemDb,
    /// Blocks of known shops (signs and their containers): `world;x;y;z`.
    index: BTreeSet<String>,
    /// `/cstoggle`: owners not told about their shops.
    quiet: BTreeSet<String>,
    /// `/csaccess`: players who trade at shops they have access to.
    access: BTreeSet<String>,
    dirty_accounts: bool,
    dirty_index: bool,
    transactions: (u64, u64, i64, i64),
}

static DATA: Mutex<Option<Data>> = Mutex::new(None);
static CLICKS: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
static NOTIFIED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

fn data<R>(f: impl FnOnce(&mut Data) -> R) -> R {
    f(DATA.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Data::default))
}

fn settings() -> Arc<Settings> {
    SETTINGS.lock().unwrap_or_else(|e| e.into_inner()).clone().expect("ChestShop settings")
}

fn ready() -> bool {
    SETTINGS.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

fn folder() -> PathBuf {
    plugin().map(|p| p.get_data_folder()).unwrap_or_default()
}

// ---- Messages ----

/// MineDown's `[text](hover)` into a component; `&` colours; `%key` values.
fn render(text: &str) -> Value {
    let colorize = horizon_kit::text::colorize;
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        let Some(mid) = rest[start..].find("](").map(|m| start + m) else { break };
        let Some(end) = rest[mid..].find(')').map(|e| mid + e) else { break };
        if start > 0 {
            parts.push(horizon_kit::text::component(&colorize(&rest[..start])));
        }
        let mut shown = horizon_kit::text::component(&colorize(&rest[start + 1..mid]));
        let hover = horizon_kit::text::component(&colorize(&rest[mid + 2..end].replace("\\n", "\n")));
        if let Value::Object(m) = &mut shown {
            m.insert("hover_event".into(), json!({ "action": "show_text", "value": hover }));
        }
        parts.push(shown);
        rest = &rest[end + 1..];
    }
    if !rest.is_empty() {
        parts.push(horizon_kit::text::component(&colorize(rest)));
    }
    json!({ "text": "", "extra": parts })
}

fn fill(text: &str, pairs: &[(&str, String)]) -> String {
    let mut sorted: Vec<&(&str, String)> = pairs.iter().collect();
    sorted.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
    let mut out = text.to_owned();
    for (k, v) in sorted {
        out = out.replace(&format!("%{k}"), v);
    }
    out
}

/// A language message (`sendWithPrefix` when `prefix`).
fn say(s: &CommandSender, key: &str, prefix: bool, pairs: &[(&str, String)]) {
    let set = settings();
    let raw = set.lang.get(key);
    if raw.is_empty() {
        return;
    }
    for line in raw.split('\n') {
        let text = fill(&if prefix { format!("{}{line}", set.lang.prefix) } else { line.to_owned() }, pairs);
        match s.as_player() {
            Some(p) => p.send_component(&Component(render(&text))),
            None => s.send_message(&horizon_kit::text::colorize(&text.replace(['[', ']'], ""))),
        }
    }
}

fn tell(p: &Player, key: &str, pairs: &[(&str, String)]) {
    say(&CommandSender::Player(p.clone()), key, true, pairs);
}

fn log(text: &str) {
    let set = settings();
    if set.p.log_to_console
        && let Some(p) = plugin()
    {
        p.info(text);
    }
    if set.p.log_to_file {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        io::append(folder().join("ChestShop.log"), format!("[{t}] {text}\n"));
    }
}

fn money(v: f64) -> String {
    economy::format(v)
}

// ---- Accounts and names ----

fn account_of_player(p: &Player) -> Account {
    let uuid = p.get_unique_id();
    let name = p.get_name().to_owned();
    data(|d| {
        if let Some(a) = d.accounts.iter_mut().find(|a| a.uuid == uuid) {
            if a.name != name {
                a.name = name.clone();
                d.dirty_accounts = true;
            }
            return a.clone();
        }
        let short = if name.chars().count() <= 15 {
            name.clone()
        } else {
            let id = items::base62(d.accounts.len() as u64 + 1);
            let head: String = name.chars().take(14 - id.len()).collect();
            format!("{head}:{id}")
        };
        let a = Account { uuid, name, short };
        d.accounts.push(a.clone());
        d.dirty_accounts = true;
        a
    })
}

/// `AccountQueryEvent`: the account a sign's name line stands for.
fn account_by_short(short: &str) -> Option<Account> {
    if let Some(a) = data(|d| d.accounts.iter().find(|a| a.short.eq_ignore_ascii_case(short)).cloned()) {
        return Some(a);
    }
    // A shop made before this plugin kept accounts: the player by that name.
    if let Some(p) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(short)) {
        return Some(account_of_player(&p));
    }
    let off = horizon_bukkit::get_offline_player(short);
    off.get_unique_id().map(|u| Account { uuid: u.to_owned(), name: off.get_name().to_owned(), short: short.to_owned() })
}

fn has(p: &Player, node: &str) -> bool {
    p.has_permission(node) || p.has_permission(&node.to_ascii_lowercase())
}

/// `Permission.otherName`.
fn other_name(p: &Player, base: &str, name: &str) -> bool {
    let has_base = base != "ChestShop.name" && other_name(p, "ChestShop.name", name);
    if !has_base && !has(p, &format!("{base}.*")) {
        has(p, &format!("{base}.{name}")) || has(p, &format!("{base}.{}", name.to_lowercase()))
    } else {
        true
    }
}

/// `NameManager.canUseName`.
fn can_use_name(p: &Player, base: &str, name: &str) -> bool {
    let set = settings();
    if shop::is_admin_name(name, &set.p.admin_shop_name) {
        return has(p, "ChestShop.adminshop");
    }
    if other_name(p, base, name) {
        return true;
    }
    match account_by_short(name) {
        Some(a) => (!a.name.eq_ignore_ascii_case(name) && other_name(p, base, &a.name)) || a.uuid == p.get_unique_id(),
        None => p.get_name().eq_ignore_ascii_case(name),
    }
}

fn is_admin_shop(owner: &str) -> bool {
    shop::is_admin_name(owner, &settings().p.admin_shop_name)
}

// ---- Shop creation ----

fn break_later(b: Block) {
    if let Some(p) = plugin() {
        let mut once = Some(b);
        p.run_task(move || {
            if let Some(b) = once.take() {
                b.break_naturally();
            }
        });
    }
}

/// `PriceChecker`: the price line in its standard form (`B 10:5 S`), or `None` when it isn't a price.
fn normalize_price(line: &str, precision: i64) -> Option<String> {
    let mut line = line.to_ascii_uppercase();
    let re = |p: &str| regex::Regex::new(p).ok();
    if precision <= 0 {
        line = re(r"\.\d*")?.replace_all(&line, "").into_owned();
    } else {
        line = re(&format!(r"(\.\d{{0,{precision}}})\d*"))?.replace_all(&line, "${1}").into_owned();
    }
    line = re(r"(\.\d*[1-9])0+")?.replace_all(&line, "${1}").into_owned();
    line = re(r"(\d)\.0+(\D|$)")?.replace_all(&line, "${1}${2}").into_owned();
    let invalid = |part: &str| ['B', 'S'].iter().find(|c| part.contains(**c)).is_some_and(|c| shop::price(part, c.to_ascii_lowercase()).is_none());
    let mut parts: Vec<String> = line.split(':').map(str::to_owned).collect();
    if parts.len() > 1 && (invalid(&parts[0]) ^ invalid(&parts[1])) {
        line = line.replace(':', " ");
        parts = vec![line.clone()];
    }
    if parts[0].split(' ').count() > 2 || line.find('B') != line.rfind('B') || line.find('S') != line.rfind('S') {
        return None;
    }
    if shop::is_price(&parts[0]) {
        line = format!("B {line}");
    }
    if parts.len() > 1 && shop::is_price(&parts[1]) {
        line = format!("{line} S");
    }
    if line.chars().count() > 15 {
        line = line.replace(' ', "");
    }
    if line.chars().count() > 15 {
        return None;
    }
    (shop::price(&line, 'b').is_some() || shop::price(&line, 's').is_some()).then_some(line)
}

fn limit(set: &Settings, path: &str) -> Option<f64> {
    yaml::at(&set.limits, path).and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)))
}

fn on_sign_change(e: &mut SignChangeEvent) {
    if !ready() {
        return;
    }
    let Some(p) = e.get_player() else { return };
    let Some(block) = e.get_block() else { return };
    let set = settings();
    let re = set.name_re.as_ref();
    let mut l = shop::clean(e.get_lines());
    if shop::valid(&l, &set.p.admin_shop_name, re) && !can_use_name(&p, "ChestShop.othername.destroy", &l[0]) {
        e.set_cancelled(true);
        return;
    }
    if !shop::valid_prepared(&l, &set.p.admin_shop_name, re) {
        return;
    }
    let fail = |e: &mut SignChangeEvent, key: &str, brk: bool| {
        tell(&p, key, &[]);
        if brk {
            e.set_cancelled(true);
            break_later(block.clone());
        }
    };
    // ItemChecker (and the `?` autofill).
    let container = shop::container_of(&block, &set.p.containers);
    let parsed = data(|d| items::parse_any(&d.items, &set.aliases, &l[3]));
    let item = match parsed {
        Some(i) => i,
        None if set.p.auto_fill && l[3] == "?" => {
            match container.as_ref().and_then(|c| ShopInv::of(c).contents().into_iter().flatten().find(|s| !s.is_empty())) {
                Some(i) => i,
                None => {
                    e.set_line(3, "§l?");
                    return fail(e, "CLICK_TO_AUTOFILL_ITEM", false);
                }
            }
        }
        None => return fail(e, "INCORRECT_ITEM_ID", true),
    };
    let mut code = data(|d| items::name(&mut d.items, &item, items::max_sign_width()));
    if let Some(alias) = set.aliases.alias_of(&code) {
        code = alias;
    }
    if items::width(&code) > items::max_sign_width() {
        return fail(e, "INCORRECT_ITEM_ID", true);
    }
    l[3] = code;
    // PriceChecker, QuantityChecker.
    let Some(price_line) = normalize_price(&l[2], set.p.price_precision) else { return fail(e, "INVALID_SHOP_PRICE", true) };
    l[2] = price_line;
    let amount = shop::quantity(&l[1]).unwrap_or(-1);
    if amount < 1 || amount > set.p.max_amount {
        return fail(e, "INVALID_SHOP_QUANTITY", true);
    }
    // ChestChecker, NameChecker.
    let admin = is_admin_shop(&l[0]);
    if container.is_none() && !admin {
        return fail(e, "NO_CHEST_DETECTED", true);
    }
    if let Some(c) = &container
        && !has(&p, "ChestShop.admin")
        && let Some((_, other)) = shop::sign_of(c, &set.p.containers, &set.p.admin_shop_name, re)
        && !can_use_name(&p, "ChestShop.othername.access", &other[0])
    {
        return fail(e, "CANNOT_ACCESS_THE_CHEST", true);
    }
    if admin {
        l[0] = set.p.admin_shop_name.clone();
        if !has(&p, "ChestShop.adminshop") {
            return fail(e, "NO_PERMISSION", true);
        }
    } else if !l[0].is_empty() && can_use_name(&p, "ChestShop.othername.create", &l[0]) && !l[0].eq_ignore_ascii_case(p.get_name()) {
        match account_by_short(&l[0]) {
            Some(a) => l[0] = a.short,
            None => return fail(e, "PLAYER_NOT_FOUND", true),
        }
    } else {
        l[0] = account_of_player(&p).short;
    }
    // TerrainChecker: one shop per container unless allowed.
    if !set.p.allow_multiple
        && let Some(c) = &container
        && let Some((other_sign, other)) = shop::sign_of(c, &set.p.containers, &set.p.admin_shop_name, re)
        && other_sign != block
        && !can_use_name(&p, "ChestShop.othername.create", &other[0])
    {
        return fail(e, "CANNOT_CREATE_SHOP_HERE", true);
    }
    // PermissionChecker.
    let mat = item.get_type().name().to_ascii_lowercase();
    let buy = shop::price(&l[2], 'b');
    let sell = shop::price(&l[2], 's');
    let allowed = |kind: &str, base: &str| has(&p, &format!("ChestShop.shop.create.{kind}.{mat}")) || has(&p, "ChestShop.shop.create") || (has(&p, &format!("ChestShop.shop.create.{mat}")) && has(&p, base));
    if (buy.is_some() && !allowed("buy", "ChestShop.shop.create.buy")) || (buy.is_none() && sell.is_some() && !allowed("sell", "ChestShop.shop.create.sell")) {
        return fail(e, "NO_PERMISSION", true);
    }
    // PriceRatioChecker, PriceRestrictionModule.
    if set.p.block_sell_above_buy
        && let (Some(b), Some(s)) = (buy, sell)
        && s > b
    {
        return fail(e, "SELL_PRICE_HIGHER_THAN_BUY_PRICE", true);
    }
    let n = amount as f64;
    for (price, kind, buy_side) in [(buy, "buy_price", true), (sell, "sell_price", false)] {
        let Some(price) = price else { continue };
        if let Some(min) = limit(&set, &format!("min.{kind}.{mat}")).map(|m| m * n)
            && price < min
        {
            let key = if buy_side { "BUY_PRICE_BELOW_MIN" } else { "SELL_PRICE_BELOW_MIN" };
            tell(&p, key, &[("price", price.to_string()), ("minprice", min.to_string())]);
            e.set_cancelled(true);
            break_later(block.clone());
            return;
        }
        if let Some(max) = limit(&set, &format!("max.{kind}.{mat}")).map(|m| m * n)
            && price > max
        {
            let key = if buy_side { "BUY_PRICE_ABOVE_MAX" } else { "SELL_PRICE_ABOVE_MAX" };
            tell(&p, key, &[("price", price.to_string()), ("maxprice", max.to_string())]);
            e.set_cancelled(true);
            break_later(block.clone());
            return;
        }
    }
    // MoneyChecker + CreationFeeGetter.
    let fee = set.p.creation_price;
    if fee != 0.0 && !admin && !has(&p, "ChestShop.nofee") {
        if !economy::withdraw(p.get_name(), fee) {
            for i in 0..4 {
                e.set_line(i, "");
            }
            return fail(e, "NOT_ENOUGH_MONEY", true);
        }
        if !set.p.server_account.is_empty() {
            economy::deposit(&set.p.server_account, fee);
        }
        tell(&p, "SHOP_FEE_PAID", &[("amount", money(fee))]);
    }
    for (i, line) in l.iter().enumerate() {
        e.set_line(i, line);
    }
    data(|d| {
        d.index.insert(shop::key(&block));
        if let Some(c) = &container {
            d.index.insert(shop::key(c));
            if let Some(dc) = horizon_bukkit::fam_items::DoubleChest::of(c) {
                d.index.insert(shop::key(dc.get_left_side().get_block()));
                d.index.insert(shop::key(dc.get_right_side().get_block()));
            }
        }
        d.dirty_index = true;
    });
    tell(&p, "SHOP_CREATED", &[]);
    log(&format!("{} created {} shop - {} - {} - at {}", p.get_name(), if buy.is_some() && sell.is_some() { "a buy/sell" } else if buy.is_some() { "a buy" } else { "a sell" }, l[1], l[3], shop::key(&block)));
}

// ---- Transactions ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Buy,
    Sell,
}

fn notify_owner(owner: &Account, key: &str, pairs: &[(&str, String)]) {
    let Some(p) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_unique_id() == owner.uuid) else { return };
    let cooldown = settings().p.notification_cooldown;
    if cooldown > 0 {
        let k = format!("{}|{key}|{}", owner.uuid, pairs.iter().map(|(a, b)| format!("{a}={b}")).collect::<Vec<_>>().join(","));
        let mut held = NOTIFIED.lock().unwrap_or_else(|e| e.into_inner());
        let map = held.get_or_insert_with(HashMap::new);
        if map.get(&k).is_some_and(|t| t.elapsed().as_secs() < cooldown) {
            return;
        }
        map.insert(k, Instant::now());
    }
    tell(&p, key, pairs);
}

fn round(v: f64, precision: i64) -> f64 {
    let f = 10f64.powi(precision.max(0) as i32);
    (v * f).round() / f
}

/// A click on a shop sign: `PreTransactionEvent` checks, then `TransactionEvent` (money, items, messages).
fn trade(p: &Player, sign: &Block, l: &[String; 4], kind: Kind) {
    let set = settings();
    let pp = &set.p;
    let admin = is_admin_shop(&l[0]);
    let owner = if admin { None } else { account_by_short(&l[0]) };
    if !admin && owner.is_none() {
        return tell(p, "PLAYER_NOT_FOUND", &[]);
    }
    let Some(price_total) = shop::price(&l[2], if kind == Kind::Buy { 'b' } else { 's' }) else {
        return tell(p, if kind == Kind::Buy { "NO_BUYING_HERE" } else { "NO_SELLING_HERE" }, &[]);
    };
    let Some(item) = data(|d| items::parse_any(&d.items, &set.aliases, &l[3])) else { return tell(p, "INVALID_SHOP_DETECTED", &[]) };
    let Some(mut amount) = shop::quantity(&l[1]).filter(|a| *a >= 1 && *a <= pp.max_amount) else { return tell(p, "INVALID_SHOP_PRICE", &[]) };
    let container = shop::container_of(sign, &pp.containers);
    let unlimited = admin && (container.is_none() || pp.force_unlimited_admin);
    let inv = if unlimited { None } else { container.as_ref().map(ShopInv::of) };
    if !unlimited && inv.is_none() {
        return tell(p, "NO_CHEST_DETECTED", &[]);
    }
    let mut price = price_total;
    // Shift: a stack, or everything.
    let shift_ok = pp.shift_allows.eq_ignore_ascii_case("ALL") || pp.shift_allows.eq_ignore_ascii_case(if kind == Kind::Buy { "BUY" } else { "SELL" });
    if (pp.shift_stacks || pp.shift_everything) && shift_ok && p.is_sneaking() {
        let max = i64::from(item.get_max_stack_size());
        let available = match kind {
            Kind::Buy => inv.as_ref().map_or(max, |i| i.count(&item)),
            Kind::Sell => shop::count_in(&shop::storage(p), &item),
        };
        let new = if pp.shift_stacks { if unlimited { max } else { available.min(max).max(if available >= max { max } else { available }) } } else { available };
        if new > 0 {
            price = round(price / amount as f64 * new as f64, pp.price_precision);
            amount = new;
        }
    }
    // Permissions, creative mode, names.
    let mat = item.get_type().name().to_ascii_lowercase();
    let perm = match kind {
        Kind::Buy => has(p, "ChestShop.shop.buy") || has(p, &format!("ChestShop.shop.buy.{mat}")),
        Kind::Sell => has(p, "ChestShop.shop.sell") || has(p, &format!("ChestShop.shop.sell.{mat}")),
    };
    if !perm {
        return tell(p, "NO_PERMISSION", &[]);
    }
    if pp.ignore_creative && p.get_game_mode().eq_ignore_ascii_case("CREATIVE") {
        return tell(p, "TRADE_DENIED_CREATIVE_MODE", &[]);
    }
    if set.name_re.as_ref().is_some_and(|re| !re.is_match(p.get_name())) {
        return tell(p, "INVALID_CLIENT_NAME", &[]);
    }
    let per_item = price / amount as f64;
    let loc_pairs = || -> Vec<(&'static str, String)> { vec![("world", sign.get_world().get_name().to_owned()), ("x", sign.get_x().to_string()), ("y", sign.get_y().to_string()), ("z", sign.get_z().to_string())] };
    let item_list = |n: i64| -> String { format!("{n} {}", data(|d| items::name(&mut d.items, &item, 0))) };
    let partial = pp.allow_partial;
    let scale = |n: i64, price: &mut f64, amount: &mut i64| {
        *price = round(per_item * n as f64, pp.price_precision);
        *amount = n;
    };
    let owner_name = owner.as_ref().map_or_else(|| pp.server_account.clone(), |a| a.name.clone());
    match kind {
        Kind::Buy => {
            // Money, stock, space (partial: as much as possible).
            if !economy::has(p.get_name(), price) {
                let affordable = if per_item > 0.0 { (economy::balance(p.get_name()) / per_item).floor() as i64 } else { amount };
                if !partial || affordable < 1 {
                    return tell(p, "NOT_ENOUGH_MONEY", &[]);
                }
                scale(affordable.min(amount), &mut price, &mut amount);
            }
            if let Some(inv) = &inv {
                let stock = inv.count(&item);
                if stock < amount {
                    if !partial || stock < 1 {
                        if pp.show_out_of_stock && (!pp.toggle_out_of_stock || !owner.as_ref().is_some_and(|o| data(|d| d.quiet.contains(&o.uuid)))) {
                            if let Some(o) = &owner {
                                let mut pairs = loc_pairs();
                                pairs.extend([("price", money(price)), ("buyer", p.get_name().to_owned()), ("item", item_list(amount)), ("material", item_list(amount))]);
                                notify_owner(o, "NOT_ENOUGH_STOCK_IN_YOUR_SHOP", &pairs);
                            }
                        }
                        return tell(p, "NOT_ENOUGH_STOCK", &[]);
                    }
                    scale(stock, &mut price, &mut amount);
                }
            }
            let room = shop::room_in(&shop::storage(p), &item);
            if room < amount {
                if !partial || room < 1 {
                    return tell(p, "NOT_ENOUGH_SPACE_IN_INVENTORY", &[]);
                }
                scale(room, &mut price, &mut amount);
            }
            if per_item > 0.0 && price <= 0.0 {
                return tell(p, "NOT_ENOUGH_MONEY", &[]);
            }
            // Money: the client pays, the owner (or the server account) receives, less tax.
            if price > 0.0 && !economy::withdraw(p.get_name(), price) {
                return tell(p, "NOT_ENOUGH_MONEY", &[]);
            }
            let tax_rate = if admin { pp.server_tax } else { pp.tax };
            let tax = if tax_rate != 0.0 && !has(p, "ChestShop.notax.buy") { round(price * tax_rate / 100.0, pp.price_precision) } else { 0.0 };
            let received = price - tax;
            let receiver = if admin { (!pp.server_account.is_empty()).then(|| pp.server_account.clone()) } else { Some(owner_name.clone()) };
            if let Some(r) = &receiver
                && received > 0.0
                && !economy::deposit(r, received)
            {
                economy::deposit(p.get_name(), price);
                return tell(p, "SHOP_DEPOSIT_FAILED", &[]);
            }
            if tax > 0.0 && !pp.server_account.is_empty() && receiver.as_deref() != Some(pp.server_account.as_str()) {
                economy::deposit(&pp.server_account, tax);
            }
            // Items.
            if let Some(inv) = &inv {
                inv.remove(&item.as_quantity(1).as_quantity(amount.min(i64::from(i32::MAX)) as i32));
            }
            let left = shop::give_to_player(p, &item, amount);
            if left > 0 {
                p.get_world().drop_item_naturally(&p.get_location(), &item.as_quantity(left as i32));
            }
            data(|d| {
                d.transactions.0 += 1;
                d.transactions.2 += amount;
            });
            let list = item_list(amount);
            if pp.show_client {
                let mut pairs = loc_pairs();
                pairs.extend([("price", money(price)), ("owner", if admin { pp.admin_shop_name.clone() } else { owner_name.clone() }), ("item", list.clone()), ("material", list.clone())]);
                tell(p, "YOU_BOUGHT_FROM_SHOP", &pairs);
            }
            if pp.show_owner
                && let Some(o) = &owner
                && !data(|d| d.quiet.contains(&o.uuid))
            {
                let mut pairs = loc_pairs();
                pairs.extend([("price", money(price)), ("buyer", p.get_name().to_owned()), ("item", list.clone()), ("material", list.clone())]);
                if let Some(op) = horizon_bukkit::get_online_players().into_iter().find(|x| x.get_unique_id() == o.uuid) {
                    tell(&op, "SOMEBODY_BOUGHT_FROM_YOUR_SHOP", &pairs);
                }
            }
            log(&format!("{} bought {list} for {price} from {} at {}", p.get_name(), if admin { pp.admin_shop_name.as_str() } else { owner_name.as_str() }, shop::key(sign)));
            // EmptyShopDeleter.
            if pp.remove_empty_shops
                && !admin
                && let (Some(inv), Some(c)) = (&inv, &container)
                && inv.count(&item) < 1
                && (pp.remove_empty_worlds.is_empty() || pp.remove_empty_worlds.iter().any(|w| w == sign.get_world().get_name()))
            {
                data(|d| {
                    d.index.remove(&shop::key(sign));
                    d.dirty_index = true;
                });
                sign.break_naturally();
                if pp.remove_empty_chests {
                    c.break_naturally();
                }
            }
        }
        Kind::Sell => {
            let pays = if admin { (!pp.server_account.is_empty()).then(|| pp.server_account.clone()) } else { Some(owner_name.clone()) };
            if let Some(payer) = &pays
                && !economy::has(payer, price)
            {
                let affordable = if per_item > 0.0 { (economy::balance(payer) / per_item).floor() as i64 } else { amount };
                if !partial || affordable < 1 {
                    return tell(p, "NOT_ENOUGH_MONEY_SHOP", &[]);
                }
                scale(affordable.min(amount), &mut price, &mut amount);
            }
            let have = shop::count_in(&shop::storage(p), &item);
            if have < amount {
                if !partial || have < 1 {
                    return tell(p, "NOT_ENOUGH_ITEMS_TO_SELL", &[]);
                }
                scale(have, &mut price, &mut amount);
            }
            if let Some(inv) = &inv {
                let room = inv.room_for(&item);
                if room < amount {
                    if !partial || room < 1 {
                        if pp.show_full_shop && (!pp.toggle_full_shop || !owner.as_ref().is_some_and(|o| data(|d| d.quiet.contains(&o.uuid)))) {
                            if let Some(o) = &owner {
                                let mut pairs = loc_pairs();
                                pairs.extend([("price", money(price)), ("seller", p.get_name().to_owned()), ("item", item_list(amount)), ("material", item_list(amount))]);
                                notify_owner(o, "NOT_ENOUGH_SPACE_IN_YOUR_SHOP", &pairs);
                            }
                        }
                        return tell(p, "NOT_ENOUGH_SPACE_IN_CHEST", &[]);
                    }
                    scale(room, &mut price, &mut amount);
                }
            }
            if per_item > 0.0 && price <= 0.0 {
                return tell(p, "NOT_ENOUGH_MONEY_SHOP", &[]);
            }
            if let Some(payer) = &pays
                && price > 0.0
                && !economy::withdraw(payer, price)
            {
                return tell(p, "NOT_ENOUGH_MONEY_SHOP", &[]);
            }
            let tax_rate = if admin { pp.server_tax } else { pp.tax };
            let tax = if tax_rate != 0.0 && !has(p, "ChestShop.notax.sell") { round(price * tax_rate / 100.0, pp.price_precision) } else { 0.0 };
            if price - tax > 0.0 && !economy::deposit(p.get_name(), price - tax) {
                if let Some(payer) = &pays {
                    economy::deposit(payer, price);
                }
                return tell(p, "CLIENT_DEPOSIT_FAILED", &[]);
            }
            if tax > 0.0 && !pp.server_account.is_empty() {
                economy::deposit(&pp.server_account, tax);
            }
            let missing = shop::take_from_player(p, &item, amount);
            let moved = amount - missing;
            if let Some(inv) = &inv {
                let left = inv.add(&item.as_quantity(moved.min(i64::from(i32::MAX)) as i32));
                if left > 0 {
                    shop::give_to_player(p, &item, left);
                }
            }
            data(|d| {
                d.transactions.1 += 1;
                d.transactions.3 += moved;
            });
            let list = item_list(moved);
            if pp.show_client {
                let mut pairs = loc_pairs();
                pairs.extend([("price", money(price)), ("buyer", if admin { pp.admin_shop_name.clone() } else { owner_name.clone() }), ("item", list.clone()), ("material", list.clone())]);
                tell(p, "YOU_SOLD_TO_SHOP", &pairs);
            }
            if pp.show_owner
                && let Some(o) = &owner
                && !data(|d| d.quiet.contains(&o.uuid))
                && let Some(op) = horizon_bukkit::get_online_players().into_iter().find(|x| x.get_unique_id() == o.uuid)
            {
                let mut pairs = loc_pairs();
                pairs.extend([("price", money(price)), ("seller", p.get_name().to_owned()), ("item", list.clone()), ("material", list.clone())]);
                tell(&op, "SOMEBODY_SOLD_TO_YOUR_SHOP", &pairs);
            }
            log(&format!("{} sold {list} for {price} to {} at {}", p.get_name(), if admin { pp.admin_shop_name.as_str() } else { owner_name.as_str() }, shop::key(sign)));
        }
    }
}

fn on_interact(e: &mut PlayerInteractEvent) {
    if !ready() {
        return;
    }
    let Some(block) = e.get_clicked_block().cloned() else { return };
    let action = e.get_action().to_owned();
    if action != "RIGHT_CLICK_BLOCK" && action != "LEFT_CLICK_BLOCK" {
        return;
    }
    let p = e.get_player().clone();
    let set = settings();
    let pp = &set.p;
    let re = set.name_re.as_ref();
    let key = block.get_type().key().to_owned();
    // A shop's container: only those who may use its name open it.
    if pp.built_in_protection && shop::is_container(&block, &pp.containers) {
        if action == "RIGHT_CLICK_BLOCK"
            && let Some((sign, l)) = shop::sign_of(&block, &pp.containers, &pp.admin_shop_name, re)
        {
            data(|d| {
                if d.index.insert(shop::key(&block)) | d.index.insert(shop::key(&sign)) {
                    d.dirty_index = true;
                }
            });
            if !can_use_name(&p, "ChestShop.othername.access", &l[0]) && !has(&p, "ChestShop.admin") {
                e.set_cancelled(true);
                if has(&p, "ChestShop.shopinfo") {
                    shop_info(&p, &sign, &l);
                } else {
                    tell(&p, "ACCESS_DENIED", &[]);
                }
            }
        }
        return;
    }
    if !shop::is_sign_type(&key) {
        return;
    }
    let hand = p.get_inventory().get_item_in_main_hand();
    let hand_name = hand.as_ref().map(|h| h.get_type().name()).unwrap_or_default();
    if hand_name.contains("SIGN") {
        return;
    }
    let l = shop::clean(&shop::lines(&block));
    if !shop::valid(&l, &pp.admin_shop_name, re) {
        return;
    }
    data(|d| {
        if d.index.insert(shop::key(&block)) {
            d.dirty_index = true;
        }
    });
    // `?`: the item in hand fills the sign.
    if pp.auto_fill && l[3] == "?" {
        if !can_use_name(&p, "ChestShop.othername.create", &l[0]) {
            return tell(&p, "ACCESS_DENIED", &[]);
        }
        let Some(item) = hand.filter(|h| !h.is_empty()) else { return tell(&p, "NO_ITEM_IN_HAND", &[]) };
        let mut code = data(|d| items::name(&mut d.items, &item, items::max_sign_width()));
        if let Some(a) = set.aliases.alias_of(&code) {
            code = a;
        }
        let mut sign = horizon_bukkit::blocks::Sign(block.tile());
        sign.set_line(3, &code);
        sign.0.update();
        e.set_cancelled(true);
        return;
    }
    let ignoring = data(|d| d.access.contains(&p.get_unique_id()));
    let owner_like = can_use_name(&p, "ChestShop.othername.access", &l[0]);
    if !ignoring && owner_like && !is_admin_shop(&l[0]) && (pp.ignore_access_perms || account_by_short(&l[0]).is_some_and(|a| a.uuid == p.get_unique_id())) {
        if (hand_name.ends_with("DYE") || hand_name.ends_with("INK_SAC")) && action == "RIGHT_CLICK_BLOCK" {
            if pp.sign_dying {
                return;
            }
            e.set_cancelled(true);
        }
        if action == "LEFT_CLICK_BLOCK" && pp.allow_left_click_destroying {
            return;
        }
        return tell(&p, "TRADE_DENIED_ACCESS_PERMS", &[]);
    }
    if action == "RIGHT_CLICK_BLOCK" {
        e.set_cancelled(true);
    } else if !pp.turn_off_sign_protection && !owner_like {
        e.set_cancelled(true);
    }
    // SpamClickProtector.
    {
        let mut held = CLICKS.lock().unwrap_or_else(|e| e.into_inner());
        let map = held.get_or_insert_with(HashMap::new);
        let uuid = p.get_unique_id();
        if map.get(&uuid).is_some_and(|t| (t.elapsed().as_millis() as u64) < pp.interaction_interval) {
            return;
        }
        map.insert(uuid, Instant::now());
    }
    let buy_action = if pp.reverse_buttons { "LEFT_CLICK_BLOCK" } else { "RIGHT_CLICK_BLOCK" };
    trade(&p, &block, &l, if action == buy_action { Kind::Buy } else { Kind::Sell });
}

// ---- Protection ----

/// May `breaker` (or nothing, an explosion) break this block?
fn can_break(block: &Block, breaker: Option<&Player>) -> bool {
    let set = settings();
    let pp = &set.p;
    let re = set.name_re.as_ref();
    let key = block.get_type().key().to_owned();
    if shop::is_sign_type(&key) {
        let l = shop::clean(&shop::lines(block));
        if !shop::valid(&l, &pp.admin_shop_name, re) || pp.turn_off_sign_protection {
            return true;
        }
        return breaker.is_some_and(|p| can_use_name(p, "ChestShop.othername.destroy", &l[0]) || has(p, "ChestShop.admin"));
    }
    if pp.built_in_protection && shop::is_container(block, &pp.containers) {
        return match shop::sign_of(block, &pp.containers, &pp.admin_shop_name, re) {
            None => true,
            Some((_, l)) => breaker.is_some_and(|p| can_use_name(p, "ChestShop.othername.destroy", &l[0]) || has(p, "ChestShop.admin")),
        };
    }
    // Something a shop sign hangs on.
    for (x, y, z) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)] {
        let b = block.get_relative(x, y, z);
        if !data(|d| d.index.contains(&shop::key(&b))) || !shop::is_sign_type(b.get_type().key()) {
            continue;
        }
        let l = shop::clean(&shop::lines(&b));
        if shop::valid(&l, &pp.admin_shop_name, re) && !pp.turn_off_sign_protection && !breaker.is_some_and(|p| can_use_name(p, "ChestShop.othername.destroy", &l[0])) {
            return false;
        }
    }
    true
}

fn on_break(e: &mut BlockBreakEvent) {
    if !ready() {
        return;
    }
    let block = e.get_block().clone();
    let k = shop::key(&block);
    let indexed = data(|d| d.index.contains(&k));
    let type_key = block.get_type().key().to_owned();
    let set = settings();
    if !indexed && !shop::is_sign_type(&type_key) && !shop::is_container(&block, &set.p.containers) {
        // Not a shop block: only a block a known shop sign hangs on needs a look.
        let near = [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)].iter().any(|(x, y, z)| data(|d| d.index.contains(&shop::key(&block.get_relative(*x, *y, *z)))));
        if !near {
            return;
        }
    }
    let p = e.get_player().clone();
    if !can_break(&block, Some(&p)) {
        e.set_cancelled(true);
        tell(&p, "ACCESS_DENIED", &[]);
        return;
    }
    // A shop sign broken: the shop is gone (refund, log).
    if shop::is_sign_type(&type_key) {
        let l = shop::clean(&shop::lines(&block));
        if shop::valid(&l, &set.p.admin_shop_name, set.name_re.as_ref()) {
            data(|d| {
                d.index.remove(&k);
                d.dirty_index = true;
            });
            let refund = set.p.refund_price;
            if refund != 0.0
                && !is_admin_shop(&l[0])
                && let Some(owner) = account_by_short(&l[0])
                && !has(&p, "ChestShop.nofee")
            {
                economy::deposit(&owner.name, refund);
                if !set.p.server_account.is_empty() {
                    economy::withdraw(&set.p.server_account, refund);
                }
                tell(&p, "SHOP_REFUNDED", &[("amount", money(refund))]);
            }
            if set.p.log_removals {
                log(&format!("{} was removed - {} - {} - at {}", l[0], l[2], l[3], k));
            }
        }
    }
}

fn protect_list(blocks: &[Block]) -> bool {
    blocks.iter().any(|b| data(|d| d.index.contains(&shop::key(b))))
}

// ---- Commands ----

fn shop_info(p: &Player, sign: &Block, l: &[String; 4]) {
    let set = settings();
    let item = data(|d| items::parse_any(&d.items, &set.aliases, &l[3]));
    let stock = match (&item, shop::container_of(sign, &set.p.containers)) {
        (Some(i), Some(c)) if !is_admin_shop(&l[0]) => ShopInv::of(&c).count(i).to_string(),
        _ => "∞".into(),
    };
    let full = item.as_ref().map(|i| data(|d| items::name(&mut d.items, i, 0))).unwrap_or_else(|| l[3].clone());
    let s = CommandSender::Player(p.clone());
    say(&s, "shopinfo", true, &[("owner", l[0].clone()), ("stock", stock), ("item", full)]);
    let amount = shop::quantity(&l[1]).unwrap_or(0).to_string();
    if let Some(b) = shop::price(&l[2], 'b') {
        say(&s, "shopinfo_buy", false, &[("amount", amount.clone()), ("price", money(b))]);
    }
    if let Some(v) = shop::price(&l[2], 's') {
        say(&s, "shopinfo_sell", false, &[("amount", amount), ("price", money(v))]);
    }
}

fn iteminfo(s: &CommandSender, args: &[&str]) -> bool {
    let set = settings();
    let item = if args.is_empty() {
        let Some(p) = s.as_player() else { return false };
        match p.get_inventory().get_item_in_main_hand().filter(|i| !i.is_empty()) {
            Some(i) => i,
            None => return false,
        }
    } else {
        match data(|d| items::parse_any(&d.items, &set.aliases, &args.join(" "))) {
            Some(i) => i,
            None => {
                say(s, "INCORRECT_ITEM_ID", true, &[]);
                return true;
            }
        }
    };
    let (full, short) = data(|d| (items::name(&mut d.items, &item, 0), items::name(&mut d.items, &item, items::max_sign_width())));
    let short = set.aliases.alias_of(&short).unwrap_or(short);
    say(s, "iteminfo", true, &[]);
    say(s, "iteminfo_fullname", false, &[("item", full)]);
    say(s, "iteminfo_shopname", false, &[("item", short)]);
    let lore = item.get_lore();
    if !lore.is_empty() {
        say(s, "iteminfo_lore", false, &[("lore", lore.join("\n"))]);
    }
    true
}

fn csgive(s: &CommandSender, label: &str, args: &[&str]) -> bool {
    if !s.has_permission("ChestShop.admin") && !s.has_permission("chestshop.admin") {
        say(s, "NO_PERMISSION", true, &[]);
        return true;
    }
    let Some(code) = args.first() else {
        s.send_message(&format!("/{label} <item code> (amount) (player)"));
        return true;
    };
    let set = settings();
    let Some(item) = data(|d| items::parse_any(&d.items, &set.aliases, code)) else {
        say(s, "INCORRECT_ITEM_ID", true, &[]);
        return true;
    };
    let mut amount = 1i64;
    let mut target = s.as_player().cloned();
    for a in &args[1..] {
        match a.parse::<i64>() {
            Ok(n) => amount = n.max(1),
            Err(_) => target = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(a)),
        }
    }
    let Some(t) = target else {
        say(s, "PLAYER_NOT_FOUND", true, &[]);
        return true;
    };
    shop::give_to_player(&t, &item, amount);
    let list = format!("{amount} {}", data(|d| items::name(&mut d.items, &item, 0)));
    say(s, "ITEM_GIVEN", true, &[("item", list), ("player", t.get_name().to_owned())]);
    true
}

fn reload(then: impl FnOnce() + Send + 'static) {
    let dir = folder();
    std::thread::spawn(move || {
        let s = conf::load(&dir);
        horizon_kit::sync::on_main(move || {
            *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(s));
            horizon_kit::hooks::refresh();
            then();
        });
    });
}

/// Accounts, items and the shop index written out when they changed (every few seconds, on the I/O thread).
fn save_changed(force: bool) {
    let dir = folder();
    let (accounts, items, index, toggles) = data(|d| {
        let a = (d.dirty_accounts || force).then(|| {
            d.dirty_accounts = false;
            json!(d.accounts.iter().map(|a| json!({ "uuid": a.uuid, "name": a.name, "short": a.short })).collect::<Vec<_>>())
        });
        let i = (d.items.dirty || force).then(|| {
            d.items.dirty = false;
            d.items.to_json()
        });
        let x = (d.dirty_index || force).then(|| {
            d.dirty_index = false;
            json!(d.index.iter().collect::<Vec<_>>())
        });
        let t = force.then(|| json!({ "quiet": d.quiet, "access": d.access }));
        (a, i, x, t)
    });
    for (name, v) in [("accounts.json", accounts), ("itemdb.json", items), ("shops.json", index), ("toggles.json", toggles)] {
        if let Some(v) = v {
            io::write(dir.join(name), serde_json::to_string(&v).unwrap_or_default());
        }
    }
}

fn read_json(dir: &std::path::Path, name: &str) -> Value {
    std::fs::read_to_string(dir.join(name)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

fn on_enable(p: Plugin) {
    let started = Instant::now();
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    p.set_executor("iteminfo", |s, _, a| iteminfo(s, a));
    p.set_executor("shopinfo", |s, _, _| {
        let Some(pl) = s.as_player() else {
            say(s, "NO_SHOP_FOUND", true, &[]);
            return true;
        };
        let set = settings();
        match pl.get_target_block_exact(6, "NEVER").filter(|b| shop::is_sign_type(b.get_type().key())) {
            Some(b) => {
                let l = shop::clean(&shop::lines(&b));
                if shop::valid(&l, &set.p.admin_shop_name, set.name_re.as_ref()) {
                    shop_info(pl, &b, &l);
                } else {
                    say(s, "NO_SHOP_FOUND", true, &[]);
                }
            }
            None => say(s, "NO_SHOP_FOUND", true, &[]),
        }
        true
    });
    p.set_executor("csGive", |s, l, a| csgive(s, l, a));
    p.set_executor("csVersion", |s, _, a| {
        if a.first().is_some_and(|x| x.eq_ignore_ascii_case("reload")) {
            if !s.has_permission("ChestShop.admin") && !s.has_permission("chestshop.admin") {
                say(s, "NO_PERMISSION", true, &[]);
                return true;
            }
            let s = s.clone();
            reload(move || s.send_message("§aThe config was reloaded."));
            return true;
        }
        let v = plugin().map(|p| p.get_version().to_owned()).unwrap_or_default();
        s.send_message(&format!("§7ChestShop's version is: §a{v}"));
        true
    });
    p.set_executor("csMetrics", |s, _, _| {
        let (accounts, (buys, sells, bought, sold)) = data(|d| (d.accounts.len(), d.transactions));
        say(s, "METRICS", false, &[
            ("accounts", accounts.to_string()),
            ("totalTransactions", (buys + sells).to_string()),
            ("buyTransactions", buys.to_string()),
            ("sellTransactions", sells.to_string()),
            ("totalItems", (bought + sold).to_string()),
            ("boughtItems", bought.to_string()),
            ("soldItems", sold.to_string()),
        ]);
        true
    });
    p.set_executor("cstoggle", |s, _, _| {
        let Some(pl) = s.as_player() else { return true };
        let uuid = pl.get_unique_id();
        let now_quiet = data(|d| if d.quiet.remove(&uuid) { false } else { d.quiet.insert(uuid.clone()) });
        say(s, if now_quiet { "TOGGLE_MESSAGES_OFF" } else { "TOGGLE_MESSAGES_ON" }, true, &[]);
        save_changed(true);
        true
    });
    p.set_executor("csaccess", |s, _, _| {
        let Some(pl) = s.as_player() else { return true };
        if !has(pl, "ChestShop.accesstoggle") {
            say(s, "NO_PERMISSION", true, &[]);
            return true;
        }
        let uuid = pl.get_unique_id();
        let now = data(|d| if d.access.remove(&uuid) { false } else { d.access.insert(uuid.clone()) });
        say(s, if now { "TOGGLE_ACCESS_OFF" } else { "TOGGLE_ACCESS_ON" }, true, &[]);
        save_changed(true);
        true
    });
    p.register_event_with("NORMAL", true, on_sign_change);
    p.register_event_with("LOW", true, on_interact);
    p.register_event_with("HIGHEST", true, on_break);
    p.register_event_with("HIGHEST", true, |e: &mut EntityExplodeEvent| {
        if ready() && settings().p.built_in_protection && protect_list(e.block_list()) {
            e.set_cancelled(true);
        }
    });
    p.register_event_with("HIGHEST", true, |e: &mut BlockExplodeEvent| {
        if ready() && settings().p.built_in_protection && protect_list(e.block_list()) {
            e.set_cancelled(true);
        }
    });
    p.register_event_with("HIGHEST", true, |e: &mut BlockPistonExtendEvent| {
        if ready() && protect_list(&e.get_blocks()) {
            e.set_cancelled(true);
        }
    });
    p.register_event_with("HIGHEST", true, |e: &mut BlockBurnEvent| {
        if ready() && let Some(b) = e.get_block() && protect_list(&[b]) {
            e.set_cancelled(true);
        }
    });
    p.register_event_with("HIGHEST", true, |e: &mut InventoryMoveItemEvent| {
        // Hopper minecarts (no block) don't take from shops.
        if !ready() || settings().p.turn_off_hopper_protection {
            return;
        }
        let to_entity = e.get_destination().is_some_and(|d| d.get_location().is_none());
        if to_entity && let Some(loc) = e.get_source().and_then(|s| s.get_location().cloned()) && protect_list(&[loc.get_block()]) {
            e.set_cancelled(true);
        }
    });
    p.register_event(|e: &mut PlayerJoinEvent| {
        if ready() {
            account_of_player(e.get_player());
        }
    });
    p.register_event(|e: &mut PlayerQuitEvent| {
        let uuid = e.get_player().get_unique_id();
        if let Some(m) = CLICKS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            m.remove(&uuid);
        }
    });
    p.run_task_timer(1, 1, horizon_kit::sync::drain);
    p.run_task_timer(100, 100, || save_changed(false));
    io::start();
    let dir = p.get_data_folder();
    std::thread::spawn(move || {
        let s = conf::load(&dir);
        let accounts = read_json(&dir, "accounts.json");
        let items = read_json(&dir, "itemdb.json");
        let index = read_json(&dir, "shops.json");
        let toggles = read_json(&dir, "toggles.json");
        horizon_kit::sync::on_main(move || {
            data(|d| {
                d.accounts = accounts.as_array().into_iter().flatten().filter_map(|a| Some(Account { uuid: a["uuid"].as_str()?.to_owned(), name: a["name"].as_str()?.to_owned(), short: a["short"].as_str()?.to_owned() })).collect();
                d.items = ItemDb::from_json(&items);
                d.index = index.as_array().into_iter().flatten().filter_map(|k| k.as_str().map(str::to_owned)).collect();
                d.quiet = toggles["quiet"].as_array().into_iter().flatten().filter_map(|k| k.as_str().map(str::to_owned)).collect();
                d.access = toggles["access"].as_array().into_iter().flatten().filter_map(|k| k.as_str().map(str::to_owned)).collect();
            });
            *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(s));
            for pl in horizon_bukkit::get_online_players() {
                account_of_player(&pl);
            }
            if let Some(p) = plugin() {
                let shops = data(|d| d.index.len());
                p.info(&format!("{shops} shop blocks known. Enabled in {} ms (Rust).", started.elapsed().as_millis()));
            }
        });
    });
}

fn on_disable(_p: Plugin) {
    if ready() {
        save_changed(true);
    }
    io::stop();
    horizon_kit::sync::clear();
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *DATA.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
