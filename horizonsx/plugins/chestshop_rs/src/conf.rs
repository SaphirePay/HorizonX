//! config.yml (`Properties`, upper-case keys), the language file, itemAliases.yml and priceLimits.yml.

use crate::items::Aliases;
use crate::yaml;
use serde_yaml::Value as Yaml;
use std::collections::HashMap;
use std::path::Path;

pub const DEFAULT_CONFIG: &str = include_str!("../data/config.yml");
pub const DEFAULT_LANG: &str = include_str!("../data/languages/lang.en.yml");

#[derive(Clone, Debug)]
pub struct Props {
    pub debug: bool,
    pub default_language: String,
    pub containers: Vec<String>,
    pub interaction_interval: u64,
    pub ignore_creative: bool,
    pub ignore_access_perms: bool,
    pub reverse_buttons: bool,
    pub shift_stacks: bool,
    pub shift_everything: bool,
    pub shift_allows: String,
    pub sign_dying: bool,
    pub allow_left_click_destroying: bool,
    pub remove_empty_shops: bool,
    pub remove_empty_chests: bool,
    pub remove_empty_worlds: Vec<String>,
    pub admin_shop_name: String,
    pub force_unlimited_admin: bool,
    pub server_account: String,
    pub tax: f64,
    pub server_tax: f64,
    pub creation_price: f64,
    pub refund_price: f64,
    pub price_precision: i64,
    pub valid_name: String,
    pub block_sell_above_buy: bool,
    pub max_amount: i64,
    pub allow_multiple: bool,
    pub allow_partial: bool,
    pub auto_fill: bool,
    pub show_out_of_stock: bool,
    pub show_full_shop: bool,
    pub notification_cooldown: u64,
    pub toggle_out_of_stock: bool,
    pub toggle_full_shop: bool,
    pub show_client: bool,
    pub show_owner: bool,
    pub log_to_file: bool,
    pub log_to_console: bool,
    pub log_removals: bool,
    pub stack_to_64: bool,
    pub built_in_protection: bool,
    pub turn_off_sign_protection: bool,
    pub turn_off_hopper_protection: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Lang {
    pub prefix: String,
    pub messages: HashMap<String, String>,
}

impl Lang {
    pub fn get(&self, key: &str) -> String {
        self.messages.get(key).cloned().unwrap_or_default()
    }
}

#[derive(Clone)]
pub struct Settings {
    pub p: Props,
    pub lang: Lang,
    pub aliases: Aliases,
    pub limits: Yaml,
    pub name_re: Option<regex::Regex>,
}

fn load_lang(text: &str) -> Lang {
    let doc: Yaml = horizon_kit::yaml::parse(text).unwrap_or(Yaml::Null);
    let mut messages = HashMap::new();
    if let Yaml::Mapping(m) = &doc {
        for (k, v) in m {
            if let (Some(k), Some(v)) = (yaml::key_str(k), yaml::scalar(v)) {
                messages.insert(k, v);
            }
        }
    }
    Lang { prefix: messages.get("prefix").cloned().unwrap_or_default(), messages }
}

/// Every file read (defaults written first). Disk only: a background thread runs it.
pub fn load(folder: &Path) -> Settings {
    let _ = std::fs::create_dir_all(folder.join("languages"));
    let cpath = folder.join("config.yml");
    if !cpath.exists() {
        let _ = std::fs::write(&cpath, DEFAULT_CONFIG);
    }
    let mut c: Yaml = std::fs::read_to_string(&cpath).ok().and_then(|t| horizon_kit::yaml::parse(&t).ok()).unwrap_or(Yaml::Null);
    // Keys the file lacks come from the defaults (and are added to it, as Java's config writer does).
    let defaults: Yaml = horizon_kit::yaml::parse(DEFAULT_CONFIG).unwrap_or(Yaml::Null);
    let mut missing = Vec::new();
    if let (Yaml::Mapping(mine), Yaml::Mapping(theirs)) = (&mut c, &defaults) {
        for (k, v) in theirs {
            if !mine.contains_key(k) {
                missing.push((k.clone(), v.clone()));
                mine.insert(k.clone(), v.clone());
            }
        }
    }
    if !missing.is_empty() && let Ok(mut text) = std::fs::read_to_string(&cpath) {
        for (k, v) in &missing {
            if let (Some(k), Ok(v)) = (yaml::key_str(k), serde_yaml::to_string(v)) {
                text.push_str(&format!("{k}: {}", v));
            }
        }
        let _ = std::fs::write(&cpath, text);
    }
    let b = |k: &str| yaml::boolean(&c, k, yaml::boolean(&defaults, k, false));
    let s = |k: &str| yaml::string(&c, k).or_else(|| yaml::string(&defaults, k)).unwrap_or_default();
    let n = |k: &str| yaml::num(&c, k, yaml::num(&defaults, k, 0.0));
    let l = |k: &str| yaml::list_or_one(&c, k);
    let p = Props {
        debug: b("DEBUG"),
        default_language: s("DEFAULT_LANGUAGE"),
        containers: l("SHOP_CONTAINERS").into_iter().map(|m| m.to_ascii_uppercase()).collect(),
        interaction_interval: n("SHOP_INTERACTION_INTERVAL").max(0.0) as u64,
        ignore_creative: b("IGNORE_CREATIVE_MODE"),
        ignore_access_perms: b("IGNORE_ACCESS_PERMS"),
        reverse_buttons: b("REVERSE_BUTTONS"),
        shift_stacks: b("SHIFT_SELLS_IN_STACKS"),
        shift_everything: b("SHIFT_SELLS_EVERYTHING"),
        shift_allows: s("SHIFT_ALLOWS"),
        sign_dying: b("SIGN_DYING"),
        allow_left_click_destroying: b("ALLOW_LEFT_CLICK_DESTROYING"),
        remove_empty_shops: b("REMOVE_EMPTY_SHOPS"),
        remove_empty_chests: b("REMOVE_EMPTY_CHESTS"),
        remove_empty_worlds: l("REMOVE_EMPTY_WORLDS"),
        admin_shop_name: s("ADMIN_SHOP_NAME"),
        force_unlimited_admin: b("FORCE_UNLIMITED_ADMIN_SHOP"),
        server_account: s("SERVER_ECONOMY_ACCOUNT"),
        tax: n("TAX_AMOUNT"),
        server_tax: n("SERVER_TAX_AMOUNT"),
        creation_price: n("SHOP_CREATION_PRICE"),
        refund_price: n("SHOP_REFUND_PRICE"),
        price_precision: n("PRICE_PRECISION") as i64,
        valid_name: s("VALID_PLAYERNAME_REGEXP"),
        block_sell_above_buy: b("BLOCK_SHOPS_WITH_SELL_PRICE_HIGHER_THAN_BUY_PRICE"),
        max_amount: n("MAX_SHOP_AMOUNT") as i64,
        allow_multiple: b("ALLOW_MULTIPLE_SHOPS_AT_ONE_BLOCK"),
        allow_partial: b("ALLOW_PARTIAL_TRANSACTIONS"),
        auto_fill: b("ALLOW_AUTO_ITEM_FILL"),
        show_out_of_stock: b("SHOW_MESSAGE_OUT_OF_STOCK"),
        show_full_shop: b("SHOW_MESSAGE_FULL_SHOP"),
        notification_cooldown: n("NOTIFICATION_MESSAGE_COOLDOWN").max(0.0) as u64,
        toggle_out_of_stock: b("CSTOGGLE_TOGGLES_OUT_OF_STOCK"),
        toggle_full_shop: b("CSTOGGLE_TOGGLES_FULL_SHOP"),
        show_client: b("SHOW_TRANSACTION_INFORMATION_CLIENT"),
        show_owner: b("SHOW_TRANSACTION_INFORMATION_OWNER"),
        log_to_file: b("LOG_TO_FILE"),
        log_to_console: b("LOG_TO_CONSOLE"),
        log_removals: b("LOG_ALL_SHOP_REMOVALS"),
        stack_to_64: b("STACK_TO_64"),
        built_in_protection: b("USE_BUILT_IN_PROTECTION"),
        turn_off_sign_protection: b("TURN_OFF_SIGN_PROTECTION"),
        turn_off_hopper_protection: b("TURN_OFF_HOPPER_PROTECTION"),
    };
    // The language: languages/lang.<code>.yml, English for what it lacks.
    let en_path = folder.join("languages").join("lang.en.yml");
    if !en_path.exists() {
        let _ = std::fs::write(&en_path, DEFAULT_LANG);
    }
    let mut lang = load_lang(DEFAULT_LANG);
    if let Ok(text) = std::fs::read_to_string(folder.join("languages").join(format!("lang.{}.yml", p.default_language))) {
        let mine = load_lang(&text);
        lang.messages.extend(mine.messages);
        lang.prefix = lang.messages.get("prefix").cloned().unwrap_or_default();
    }
    let apath = folder.join("itemAliases.yml");
    if !apath.exists() {
        let _ = std::fs::write(&apath, "# Item aliases: item code: alias shown on signs\n# Example:\n# Diamond Sword#1: Excalibur\n");
    }
    let adoc: Yaml = std::fs::read_to_string(&apath).ok().and_then(|t| horizon_kit::yaml::parse(&t).ok()).unwrap_or(Yaml::Null);
    let aliases = Aliases(yaml::keys(&adoc, "").into_iter().filter_map(|k| Some((k.clone(), yaml::string(&adoc, &k)?))).collect());
    let lpath = folder.join("priceLimits.yml");
    if !lpath.exists() {
        let _ = std::fs::write(&lpath, "uses_materials: true\nmax:\n  buy_price:\n    # item_type: maximum price per item\n  sell_price:\nmin:\n  buy_price:\n  sell_price:\n");
    }
    let limits: Yaml = std::fs::read_to_string(&lpath).ok().and_then(|t| horizon_kit::yaml::parse(&t).ok()).unwrap_or(Yaml::Null);
    let name_re = regex::Regex::new(&p.valid_name).ok();
    Settings { p, lang, aliases, limits, name_re }
}
