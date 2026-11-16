//! config.yml, economies.yml, categories.yml, rules.yml and messages.yml, read the way zAuctionHouse reads them
//! (defaults written on first start). Pure data: loaded on a background thread.

use crate::yaml::{self, at};
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::path::Path;

pub const DEFAULTS: [(&str, &str); 22] = [
    ("config.yml", include_str!("../data/config.yml")),
    ("messages.yml", include_str!("../data/messages.yml")),
    ("economies.yml", include_str!("../data/economies.yml")),
    ("categories.yml", include_str!("../data/categories.yml")),
    ("rules.yml", include_str!("../data/rules.yml")),
    ("inventories/auction.yml", include_str!("../data/inventories/auction.yml")),
    ("inventories/categories.yml", include_str!("../data/inventories/categories.yml")),
    ("inventories/expired-items.yml", include_str!("../data/inventories/expired-items.yml")),
    ("inventories/purchased-items.yml", include_str!("../data/inventories/purchased-items.yml")),
    ("inventories/selling-items.yml", include_str!("../data/inventories/selling-items.yml")),
    ("inventories/history.yml", include_str!("../data/inventories/history.yml")),
    ("inventories/combined-items.yml", include_str!("../data/inventories/combined-items.yml")),
    ("inventories/options.yml", include_str!("../data/inventories/options.yml")),
    ("inventories/sell-inventory.yml", include_str!("../data/inventories/sell-inventory.yml")),
    ("inventories/shulker-content.yml", include_str!("../data/inventories/shulker-content.yml")),
    ("inventories/confirms/purchase-confirm.yml", include_str!("../data/inventories/confirms/purchase-confirm.yml")),
    ("inventories/confirms/remove-confirm.yml", include_str!("../data/inventories/confirms/remove-confirm.yml")),
    ("inventories/confirms/purchase-inventory-confirm.yml", include_str!("../data/inventories/confirms/purchase-inventory-confirm.yml")),
    ("inventories/confirms/remove-inventory-confirm.yml", include_str!("../data/inventories/confirms/remove-inventory-confirm.yml")),
    ("patterns/back.yml", include_str!("../data/patterns/back.yml")),
    ("patterns/decoration.yml", include_str!("../data/patterns/decoration.yml")),
    ("patterns/pagination.yml", include_str!("../data/patterns/pagination.yml")),
];

#[derive(Clone, Debug, Default)]
pub struct Tax {
    pub enabled: bool,
    pub kind: String,
    pub percentage: bool,
    pub amount: f64,
    pub bypass: String,
    pub reductions: Vec<(String, f64)>,
}

/// What a tax works out to for one sale.
#[derive(Clone, Debug, Default)]
pub struct TaxResult {
    pub tax: f64,
    pub percentage: f64,
    pub bypassed: bool,
    pub reduced: Option<f64>,
    pub kind: String,
}

impl Tax {
    /// `calculateTax` for SELL (`selling`) or PURCHASE/CAPITALISM.
    pub fn compute(&self, has: &dyn Fn(&str) -> bool, price: f64, selling: bool) -> Option<TaxResult> {
        if !self.enabled {
            return None;
        }
        let applies = if selling { matches!(self.kind.as_str(), "SELL" | "BOTH") } else { matches!(self.kind.as_str(), "PURCHASE" | "BOTH" | "CAPITALISM") };
        if !applies {
            return None;
        }
        if !self.bypass.is_empty() && has(&self.bypass) {
            return Some(TaxResult { bypassed: true, kind: self.kind.clone(), ..Default::default() });
        }
        let reduction = self.reductions.iter().find(|(p, _)| has(p)).map(|(_, r)| *r);
        let base = if self.percentage { price * self.amount / 100.0 } else { self.amount };
        let tax = base * (1.0 - reduction.unwrap_or(0.0) / 100.0);
        let percentage = if price > 0.0 { tax / price * 100.0 } else { 0.0 };
        Some(TaxResult { tax, percentage, bypassed: false, reduced: reduction, kind: self.kind.clone() })
    }
}

#[derive(Clone, Debug, Default)]
pub struct Economy {
    pub name: String,
    pub display: String,
    pub format: String,
    pub price_format: String,
    pub min_auction: f64,
    pub max: f64,
    pub tax: Tax,
}

#[derive(Clone, Debug)]
pub enum Rule {
    Material(Vec<String>),
    Suffix(Vec<String>),
    Prefix(Vec<String>),
    Contains(Vec<String>),
    Tag(Vec<String>),
    Name { mode: String, values: Vec<String>, ignore: bool },
    Lore { mode: String, values: Vec<String>, ignore: bool },
    ModelData(Vec<i64>),
    ItemModel(Vec<String>),
    And(Vec<Rule>),
}

/// What a rule looks at, read once from the item.
pub struct ItemFacts {
    pub material: String,
    pub is_block: bool,
    pub name: Option<String>,
    pub lore: Vec<String>,
    pub model_data: Option<i64>,
    pub item_model: Option<String>,
}

fn text_rule(mode: &str, have: &str, want: &str, ignore: bool) -> bool {
    let (h, w) = if ignore { (have.to_lowercase(), want.to_lowercase()) } else { (have.to_owned(), want.to_owned()) };
    if mode.eq_ignore_ascii_case("EQUALS") { h == w } else { h.contains(&w) }
}

impl Rule {
    pub fn matches(&self, f: &ItemFacts) -> bool {
        let m = f.material.as_str();
        match self {
            Rule::Material(list) => list.iter().any(|x| x.eq_ignore_ascii_case(m)),
            Rule::Suffix(list) => list.iter().any(|x| m.ends_with(&x.to_ascii_uppercase())),
            Rule::Prefix(list) => list.iter().any(|x| m.starts_with(&x.to_ascii_uppercase())),
            Rule::Contains(list) => list.iter().any(|x| m.contains(&x.to_ascii_uppercase())),
            Rule::Tag(list) => list.iter().any(|t| match t.to_ascii_lowercase().as_str() {
                "blocks" | "block" => f.is_block,
                "items" | "item" => !f.is_block,
                _ => false,
            }),
            Rule::Name { mode, values, ignore } => f.name.as_ref().is_some_and(|n| values.iter().any(|v| text_rule(mode, n, v, *ignore))),
            Rule::Lore { mode, values, ignore } => f.lore.iter().any(|l| values.iter().any(|v| text_rule(mode, l, v, *ignore))),
            Rule::ModelData(values) => f.model_data.is_some_and(|d| values.contains(&d)),
            Rule::ItemModel(values) => f.item_model.as_ref().is_some_and(|d| values.iter().any(|v| d.ends_with(v.as_str()))),
            Rule::And(rules) => !rules.is_empty() && rules.iter().all(|r| r.matches(f)),
        }
    }
}

fn rules(v: &Yaml, path: &str) -> Vec<Rule> {
    let Some(Yaml::Sequence(list)) = at(v, path) else { return Vec::new() };
    list.iter().filter_map(rule).collect()
}

fn rule(r: &Yaml) -> Option<Rule> {
    let kind = yaml::string(r, "type")?.to_ascii_lowercase();
    let l = |k: &str| yaml::list(r, k);
    let ignore = yaml::boolean(r, "ignore-case", false);
    let mode = yaml::string(r, "mode").unwrap_or_else(|| "CONTAINS".into()).to_ascii_uppercase();
    Some(match kind.as_str() {
        "material" => Rule::Material(l("materials")),
        "material-suffix" => Rule::Suffix(l("suffixes")),
        "material-prefix" => Rule::Prefix(l("prefixes")),
        "material-contains" => Rule::Contains(l("patterns")),
        "tag" | "material-tag" => {
            let mut t = l("tags");
            if t.is_empty() {
                t.extend(yaml::string(r, "tag"));
            }
            Rule::Tag(t)
        }
        "name" => Rule::Name { mode, values: l("values"), ignore },
        "lore" => Rule::Lore { mode, values: l("values"), ignore },
        "custom-model-data" => Rule::ModelData(l("values").iter().filter_map(|x| x.parse().ok()).collect()),
        "item-model" => {
            let mut m = l("models");
            m.extend(yaml::string(r, "model"));
            Rule::ItemModel(m)
        }
        "and" => Rule::And(rules(r, "rules")),
        _ => return None,
    })
}

#[derive(Clone, Debug)]
pub struct Category {
    pub id: String,
    pub display: String,
    pub rules: Vec<Rule>,
}

/// A command's names: the main word first.
#[derive(Clone, Debug, Default)]
pub struct InvCommand {
    pub aliases: Vec<String>,
    pub permission: String,
    pub inventory: String,
}

#[derive(Clone, Debug, Default)]
pub struct Conf {
    pub colors: Vec<(String, String)>,
    pub date_format: String,
    pub sell_aliases: Vec<String>,
    pub claim_aliases: Vec<String>,
    pub search_aliases: Vec<String>,
    pub page_aliases: Vec<String>,
    pub inventory_commands: Vec<InvCommand>,
    pub multipliers: Vec<(String, f64)>,
    pub allow_decimals: bool,
    pub expiration: i64,
    pub expiration_perms: Vec<(String, i64)>,
    pub expire_keep: i64,
    pub purchase_keep: i64,
    pub sweep_seconds: u64,
    pub limits: Vec<(String, i64)>,
    pub banned_worlds: Vec<String>,
    pub lore_listed: Vec<String>,
    pub lore_expired: Vec<String>,
    pub lore_purchased: Vec<String>,
    pub lore_selling: Vec<String>,
    pub lore_history: Vec<String>,
    pub status_seller: String,
    pub status_buyer: String,
    pub display_lang: String,
    pub display_name: String,
    pub time: [String; 4],
    pub remove_listed_give: bool,
    pub remove_listed_open: bool,
    pub remove_listed_confirm: bool,
    pub expired_open: bool,
    pub expired_free_space: bool,
    pub purchased_give: bool,
    pub purchased_free_space: bool,
    pub purchased_open: bool,
    pub money_message: bool,
    pub money_sound: Option<(String, f32, f32)>,
    pub selling_open: bool,
    pub selling_free_space: bool,
    pub auto_claim: bool,
    pub auto_claim_delay: u64,
    pub notify_pending: bool,
    pub sales_notification: bool,
    pub sales_notification_delay: u64,
    pub broadcast_sell: bool,
    pub broadcast_sell_exclude: bool,
    pub broadcast_purchase: bool,
    pub broadcast_purchase_exclude: bool,
    pub default_sort: String,
    pub sort_names: HashMap<String, String>,
    pub filters: Vec<(String, String)>,
    pub history_max: usize,
    pub economies: Vec<Economy>,
    pub default_economy: String,
    pub decimal_format: String,
    pub reductions: Vec<(String, f64, Option<String>)>,
    pub categories: Vec<Category>,
    pub all_category_name: String,
    pub blacklist: Option<Vec<Rule>>,
    pub whitelist: Option<Vec<Rule>>,
    pub messages: HashMap<String, Vec<String>>,
    pub prefix: String,
}

impl Conf {
    pub fn economy(&self, name: &str) -> Option<&Economy> {
        self.economies.iter().find(|e| e.name.eq_ignore_ascii_case(name))
    }

    /// `parseNumber`: `1.5k`, `2M`... with the configured multipliers.
    pub fn parse_price(&self, text: &str) -> Option<f64> {
        let t = text.trim().replace(',', ".");
        if let Ok(v) = t.parse::<f64>() {
            return (v.is_finite() && v >= 0.0).then_some(v);
        }
        let mut best: Option<(&str, f64)> = None;
        for (suffix, mult) in &self.multipliers {
            if t.to_ascii_uppercase().ends_with(&suffix.to_ascii_uppercase()) && best.is_none_or(|(s, _)| suffix.len() > s.len()) {
                best = Some((suffix, *mult));
            }
        }
        let (suffix, mult) = best?;
        let n: f64 = t[..t.len() - suffix.len()].trim().parse().ok()?;
        let v = n * mult;
        (v.is_finite() && v >= 0.0).then_some(v)
    }

    /// A price as its economy writes it.
    pub fn format_price(&self, economy: &str, price: f64) -> String {
        let eco = self.economy(economy);
        let format = eco.map_or("%price%", |e| e.format.as_str());
        let mode = eco.map_or("PRICE_WITH_DECIMAL_FORMAT", |e| e.price_format.as_str());
        let number = match mode {
            "PRICE_WITH_REDUCTION" => {
                let mut divisor = 1.0;
                let mut out = None;
                for (fmt, max, display) in &self.reductions {
                    if price < *max {
                        let text = crate::text::printf_float(fmt, price / divisor);
                        out = Some(display.as_ref().map_or(text.clone(), |d| d.replace("%amount%", &text)));
                        break;
                    }
                    divisor = *max;
                }
                out.unwrap_or_else(|| crate::text::decimal(price, &self.decimal_format))
            }
            "PRICE_WITHOUT_DECIMAL" | "PRICE_RAW" => format!("{}", price.round() as i64),
            _ => crate::text::decimal(price, &self.decimal_format),
        };
        format.replace("%price%", &number)
    }

    /// A message's lines (the key's text, or its list), the prefix in front of each.
    pub fn lines(&self, key: &str) -> Vec<String> {
        self.messages.get(key).cloned().unwrap_or_default()
    }
}

fn messages(doc: &Yaml) -> HashMap<String, Vec<String>> {
    let mut out = HashMap::new();
    if let Yaml::Mapping(m) = doc {
        for (k, v) in m {
            let Some(k) = yaml::key_str(k) else { continue };
            let lines = match v {
                Yaml::Sequence(items) => items.iter().filter_map(yaml::scalar).collect(),
                Yaml::Mapping(_) => {
                    let mut l = yaml::list_or_one(v, "messages");
                    if l.is_empty() {
                        l = yaml::list_or_one(v, "message");
                    }
                    l
                }
                other => yaml::scalar(other).into_iter().collect(),
            };
            out.insert(k, lines);
        }
    }
    out
}

/// Built-in messages for keys a server's older messages.yml lacks.
fn default_messages() -> HashMap<String, Vec<String>> {
    horizon_kit::yaml::parse(include_str!("../data/messages.yml")).map(|d| messages(&d)).unwrap_or_default()
}

pub fn read(folder: &Path, rel: &str) -> Yaml {
    let path = folder.join(rel);
    if !path.exists()
        && let Some((_, text)) = DEFAULTS.iter().find(|(name, _)| *name == rel)
    {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, text);
    }
    std::fs::read_to_string(&path).ok().and_then(|t| horizon_kit::yaml::parse(&t).ok()).unwrap_or(Yaml::Null)
}

/// Writes every default file that isn't there yet.
pub fn write_defaults(folder: &Path) {
    for (rel, text) in DEFAULTS {
        let path = folder.join(rel);
        if !path.exists() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, text);
        }
    }
}

fn aliases(c: &Yaml, path: &str, fallback: &[&str]) -> Vec<String> {
    let l = yaml::list(c, path);
    if l.is_empty() { fallback.iter().map(|s| (*s).to_owned()).collect() } else { l.into_iter().map(|s| s.to_ascii_lowercase()).collect() }
}

fn perm_list(c: &Yaml, path: &str, value: &str) -> Vec<(String, i64)> {
    match at(c, path) {
        Some(Yaml::Sequence(items)) => items.iter().filter_map(|i| Some((yaml::string(i, "permission")?, yaml::int(i, value, 0)))).collect(),
        _ => Vec::new(),
    }
}

pub fn load(folder: &Path) -> Conf {
    write_defaults(folder);
    let c = read(folder, "config.yml");
    let e = read(folder, "economies.yml");
    let cats = read(folder, "categories.yml");
    let rules_doc = read(folder, "rules.yml");
    let msgs = read(folder, "messages.yml");
    let mut conf = Conf {
        colors: match at(&c, "message-colors") {
            Some(Yaml::Sequence(items)) => items.iter().filter_map(|i| Some((yaml::string(i, "key")?, yaml::string(i, "color")?))).collect(),
            _ => Vec::new(),
        },
        date_format: yaml::string(&c, "date-format").unwrap_or_else(|| "dd/MM/yyyy HH:mm:ss".into()),
        sell_aliases: aliases(&c, "commands.sell.aliases", &["sell"]),
        claim_aliases: aliases(&c, "commands.claim.aliases", &["claim"]),
        search_aliases: aliases(&c, "commands.search.aliases", &["search"]),
        page_aliases: aliases(&c, "commands.page.aliases", &["page"]),
        inventory_commands: match at(&c, "commands.inventories") {
            Some(Yaml::Sequence(items)) => items
                .iter()
                .filter(|i| yaml::boolean(i, "enable", true))
                .map(|i| InvCommand { aliases: yaml::list(i, "aliases").into_iter().map(|a| a.to_ascii_lowercase()).collect(), permission: yaml::string(i, "permission").unwrap_or_default(), inventory: yaml::string(i, "inventory").unwrap_or_default() })
                .filter(|i| !i.aliases.is_empty())
                .collect(),
            _ => Vec::new(),
        },
        multipliers: if yaml::boolean(&c, "number-sell-multiplication.enable", true) {
            match at(&c, "number-sell-multiplication.formats") {
                Some(Yaml::Sequence(items)) => items.iter().filter_map(|i| Some((yaml::string(i, "format")?, yaml::num(i, "multiplication", 1.0)))).collect(),
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        },
        allow_decimals: yaml::boolean(&c, "allow-decimal-prices", true),
        expiration: yaml::int(&c, "expiration.auction.default-expiration", 172_800),
        expiration_perms: if yaml::boolean(&c, "expiration.auction.permission.enable", false) { perm_list(&c, "expiration.auction.permission.permissions", "expiration") } else { Vec::new() },
        expire_keep: yaml::int(&c, "expiration.expire.default-expiration", 604_800),
        purchase_keep: yaml::int(&c, "expiration.purchase.default-expiration", 604_800),
        sweep_seconds: yaml::int(&c, "maintenance.expiration-sweep-interval-seconds", 60).max(5) as u64,
        limits: perm_list(&c, "permissions.auction", "limit"),
        banned_worlds: yaml::list(&c, "banned-worlds.auction"),
        lore_listed: yaml::list(&c, "item-lore.listed-auction-item"),
        lore_expired: yaml::list(&c, "item-lore.expired-item"),
        lore_purchased: yaml::list(&c, "item-lore.purchased-item"),
        lore_selling: yaml::list(&c, "item-lore.selling-item"),
        lore_history: yaml::list(&c, "item-lore.history-item"),
        status_seller: yaml::string(&c, "item-lore.status.seller").unwrap_or_default(),
        status_buyer: yaml::string(&c, "item-lore.status.buyer").unwrap_or_default(),
        display_lang: yaml::string(&c, "item-display.lang").unwrap_or_else(|| "x%amount% <lang:%item-translation-key%>".into()),
        display_name: yaml::string(&c, "item-display.item-name").unwrap_or_else(|| "x%amount% %item-name%".into()),
        time: [
            yaml::string(&c, "time.time-day").unwrap_or_else(|| "%02dd %02dh %02dm".into()),
            yaml::string(&c, "time.time-hour").unwrap_or_else(|| "%02dh %02dm %02ds".into()),
            yaml::string(&c, "time.time-minute").unwrap_or_else(|| "%02dm %02ds".into()),
            yaml::string(&c, "time.time-second").unwrap_or_else(|| "%02ds".into()),
        ],
        remove_listed_give: yaml::boolean(&c, "action.remove-listed-item.give-item", false),
        remove_listed_open: yaml::boolean(&c, "action.remove-listed-item.open-inventory", true),
        remove_listed_confirm: yaml::boolean(&c, "action.remove-listed-item.open-confirm-inventory", true),
        expired_open: yaml::boolean(&c, "action.remove-expired-item.open-inventory", true),
        expired_free_space: yaml::boolean(&c, "action.remove-expired-item.player-inventory-must-have-free-space", true),
        purchased_give: yaml::boolean(&c, "action.purchased-item.give-item", false),
        purchased_free_space: yaml::boolean(&c, "action.purchased-item.player-inventory-must-have-free-space", true),
        purchased_open: yaml::boolean(&c, "action.purchased-item.open-inventory", true),
        money_message: yaml::boolean(&c, "action.purchased-item.money-message", true),
        money_sound: yaml::boolean(&c, "action.purchased-item.money-sound.enable", false).then(|| {
            (
                yaml::string(&c, "action.purchased-item.money-sound.sound").unwrap_or_else(|| "minecraft:entity.villager.no".into()),
                yaml::num(&c, "action.purchased-item.money-sound.volume", 1.0) as f32,
                yaml::num(&c, "action.purchased-item.money-sound.pitch", 1.0) as f32,
            )
        }),
        selling_open: yaml::boolean(&c, "action.selling-item.open-inventory", true),
        selling_free_space: yaml::boolean(&c, "action.selling-item.player-inventory-must-have-free-space", true),
        auto_claim: yaml::boolean(&c, "auto-claim.enable", true),
        auto_claim_delay: yaml::int(&c, "auto-claim.delay-ticks", 20).max(1) as u64,
        notify_pending: yaml::boolean(&c, "auto-claim.notify-pending", true),
        sales_notification: yaml::boolean(&c, "sales-notification.enable", true),
        sales_notification_delay: yaml::int(&c, "sales-notification.delay-ticks", 60).max(1) as u64,
        broadcast_sell: yaml::boolean(&c, "broadcast.sell.enable", false),
        broadcast_sell_exclude: yaml::boolean(&c, "broadcast.sell.exclude-seller", true),
        broadcast_purchase: yaml::boolean(&c, "broadcast.purchase.enable", false),
        broadcast_purchase_exclude: yaml::boolean(&c, "broadcast.purchase.exclude-buyer", true),
        default_sort: yaml::string(&c, "sort-items.default-sort").unwrap_or_else(|| "DECREASING_DATE".into()).to_ascii_uppercase(),
        sort_names: yaml::keys(&c, "sort-items.translations").into_iter().filter_map(|k| Some((k.clone(), yaml::string(&c, &format!("sort-items.translations.{k}"))?))).collect(),
        filters: yaml::keys(&c, "search-filters").into_iter().filter_map(|k| Some((k.clone(), yaml::string(&c, &format!("search-filters.{k}"))?))).collect(),
        history_max: yaml::int(&c, "history.max-entries", 500).max(1) as usize,
        default_economy: yaml::string(&e, "default-economy.auction").unwrap_or_else(|| "vault".into()),
        decimal_format: yaml::string(&e, "price-decimal-format").unwrap_or_else(|| "#,###.#".into()),
        reductions: match at(&e, "price-reductions") {
            Some(Yaml::Sequence(items)) => items.iter().filter_map(|i| Some((yaml::string(i, "format")?, yaml::num(i, "max-amount", f64::MAX), yaml::string(i, "display")))).collect(),
            _ => Vec::new(),
        },
        all_category_name: yaml::string(&cats, "settings.all-category-name").unwrap_or_else(|| "Auction House".into()),
        blacklist: yaml::boolean(&rules_doc, "blacklist.enabled", false).then(|| rules(&rules_doc, "blacklist.rules")),
        whitelist: yaml::boolean(&rules_doc, "whitelist.enabled", false).then(|| rules(&rules_doc, "whitelist.rules")),
        ..Conf::default()
    };
    if let Some(Yaml::Sequence(list)) = at(&e, "economies") {
        for x in list {
            if !yaml::boolean(x, "is-enable", true) {
                continue;
            }
            let Some(name) = yaml::string(x, "name") else { continue };
            conf.economies.push(Economy {
                display: yaml::string(x, "display-name").unwrap_or_else(|| name.clone()),
                name,
                format: yaml::string(x, "format").unwrap_or_else(|| "%price%".into()),
                price_format: yaml::string(x, "price-format").unwrap_or_else(|| "PRICE_WITH_DECIMAL_FORMAT".into()).to_ascii_uppercase(),
                min_auction: yaml::num(x, "min-prices.auction", 0.0),
                max: yaml::num(x, "max-prices", f64::MAX),
                tax: Tax {
                    enabled: yaml::boolean(x, "tax.enabled", false),
                    kind: yaml::string(x, "tax.type").unwrap_or_else(|| "SELL".into()).to_ascii_uppercase(),
                    percentage: yaml::string(x, "tax.amount-type").unwrap_or_else(|| "PERCENTAGE".into()).eq_ignore_ascii_case("PERCENTAGE"),
                    amount: yaml::num(x, "tax.amount", 0.0),
                    bypass: yaml::string(x, "tax.bypass-permission").unwrap_or_default(),
                    reductions: match at(x, "tax.reductions") {
                        Some(Yaml::Sequence(items)) => items.iter().filter_map(|i| Some((yaml::string(i, "permission")?, yaml::num(i, "percentage", 0.0)))).collect(),
                        _ => Vec::new(),
                    },
                },
            });
        }
    }
    for key in yaml::keys(&cats, "") {
        if key == "settings" {
            continue;
        }
        conf.categories.push(Category { display: yaml::string(&cats, &format!("{key}.display-name")).unwrap_or_else(|| key.clone()), rules: rules(&cats, &format!("{key}.rules")), id: key });
    }
    let mut m = default_messages();
    m.extend(messages(&msgs));
    conf.prefix = m.get("prefix").and_then(|l| l.first().cloned()).unwrap_or_default();
    conf.messages = m;
    conf
}

