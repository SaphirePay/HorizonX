//! `Kits` and `Kit`: `kits.yml`, plus any file in `kits/`.
use crate::prelude::*;
use crate::config::{Config, entries, scalar_f64, scalar_string};
use crate::textreader::{KeywordReplacer, SimpleTextInput};
use crate::trade::OverflowType;
use crate::utils::base64::decode_lines;
use crate::utils::number::sanitize_currency_string;
use crate::utils::string::capital_case;
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

fn kit_key(name: &str) -> String {
    name.replace(['.', '/'], "_")
}

/// Where a kit lives: `kits.yml`, or a file in `kits/`.
enum Home {
    Root,
    File(PathBuf),
}

struct KitsState {
    root: Config,
    kits: BTreeMap<String, (Value, Home)>,
    /// The keys, as they were found.
    order: Vec<String>,
}

pub struct Kits {
    state: Mutex<KitsState>,
}

fn parse_kits(state: &mut KitsState) {
    state.kits.clear();
    state.order.clear();
    for (name, section) in entries(state.root.get_section("kits")) {
        if section.is_mapping() {
            state.order.push(name.to_lowercase());
            state.kits.insert(name.to_lowercase(), (section, Home::Root));
        }
    }
    let folder = crate::data_folder().join("kits");
    let Ok(files) = std::fs::read_dir(folder) else { return };
    for file in files.flatten() {
        let path = file.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("yml") {
            continue;
        }
        let mut config = Config::new(path.clone());
        config.load();
        for (name, section) in entries(config.get_section("kits")) {
            if section.is_mapping() {
                state.order.push(name.to_lowercase());
                state.kits.insert(name.to_lowercase(), (section, Home::File(path.clone())));
            }
        }
    }
}

impl Kits {
    pub fn new() -> Self {
        let root = Config::with_template(crate::data_folder().join("kits.yml"), include_str!("../resources/kits.yml"));
        let kits = Self { state: Mutex::new(KitsState { root, kits: BTreeMap::new(), order: Vec::new() }) };
        kits.reload_config();
        kits
    }
    fn state(&self) -> std::sync::MutexGuard<'_, KitsState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `reloadConfig`.
    pub fn reload_config(&self) {
        let mut state = self.state();
        state.root.load();
        parse_kits(&mut state);
    }
    pub fn get_file(&self) -> PathBuf {
        self.state().root.get_file().to_owned()
    }
    /// `getKitKeys`.
    pub fn get_kit_keys(&self) -> Vec<String> {
        // `kitMap` is a `HashMap`, and `/kit` prints it as it comes.
        crate::utils::string::java_hash_map_order(self.state().order.clone())
    }
    /// `getKit`.
    pub fn get_kit(&self, name: &str) -> Option<Value> {
        self.state().kits.get(&kit_key(name)).map(|(kit, _)| kit.clone())
    }
    /// `matchKit`.
    pub fn match_kit(&self, name: &str) -> Option<String> {
        self.state().kits.keys().find(|kit| kit.eq_ignore_ascii_case(name)).cloned()
    }
    /// `addKit`.
    pub fn add_kit(&self, name: &str, lines: &[String], delay: i64) {
        let name = kit_key(name).to_lowercase();
        let mut state = self.state();
        state.root.set(&format!("kits.{name}.delay"), Value::from(delay));
        state.root.set(&format!("kits.{name}.items"), Value::Sequence(lines.iter().cloned().map(Value::from).collect()));
        parse_kits(&mut state);
        state.root.save();
    }
    /// `removeKit`.
    pub fn remove_kit(&self, name: &str) {
        let name = kit_key(name).to_lowercase();
        let mut state = self.state();
        match state.kits.get(&name).map(|(_, home)| home) {
            None => return,
            Some(Home::Root) => {
                state.root.remove(&format!("kits.{name}"));
                state.root.blocking_save();
            }
            Some(Home::File(path)) => {
                let mut config = Config::new(path.clone());
                config.load();
                config.remove(&format!("kits.{name}"));
                config.blocking_save();
            }
        }
        parse_kits(&mut state);
    }
    /// `listKits`.
    pub fn list_kits(&self, user: Option<&User>) -> EssResult<String> {
        let mut list = String::new();
        for kit_item in self.get_kit_keys() {
            let Some(user) = user else {
                list.push(' ');
                list.push_str(&capital_case(&kit_item));
                continue;
            };
            if !user.is_authorized(&format!("essentials.kits.{}", kit_item.to_lowercase())) {
                continue;
            }
            let mut cost = String::new();
            let mut name = capital_case(&kit_item);
            let price = Trade::of_command(&format!("kit-{}", kit_item.to_lowercase())).get_command_cost(user);
            if price.signum() > 0 {
                cost = user.player_tl("kitCost", crate::args![display_currency(&price)]);
            }
            let next_use = Kit::new(&kit_item).map_err(|_| EssError::tl("kitError", &[]))?.get_next_use(user).map_err(|_| EssError::tl("kitError", &[]))?;
            if next_use == -1 && settings().is_skipping_used_one_time_kits_from_kit_list() {
                continue;
            } else if next_use != 0 {
                name = user.player_tl("kitDelay", crate::args![name]);
            }
            list.push(' ');
            list.push_str(&name);
            list.push_str(&cost);
        }
        Ok(list.trim().to_owned())
    }
}

/// `Kit`.
pub struct Kit {
    name: String,
    kit: Value,
    charge: Trade,
}

impl Kit {
    /// `new Kit(name, ess)`.
    pub fn new(name: &str) -> EssResult<Self> {
        let kit = ess().kits().get_kit(name).ok_or_else(|| EssError::tl("kitNotFound", &[]))?;
        Ok(Self { name: name.to_owned(), kit, charge: Trade::of_command_with_fallback(&format!("kit-{name}"), Trade::of_command("kit-kit")) })
    }
    pub fn get_name(&self) -> &str {
        &self.name
    }
    /// `checkPerms`.
    pub fn check_perms(&self, user: &User) -> EssResult {
        let node = format!("essentials.kits.{}", self.name);
        if user.is_authorized(&node) { Ok(()) } else { Err(EssError::tl("noKitPermission", crate::args![node])) }
    }
    /// `checkDelay`.
    pub fn check_delay(&self, user: &User) -> EssResult {
        let next_use = self.get_next_use(user)?;
        if next_use < 0 {
            user.send_tl("kitOnce", &[]);
            return Err(EssError::NoCharge);
        }
        if next_use > 0 {
            user.send_tl("kitTimed", crate::args![format_date_diff(next_use)]);
            return Err(EssError::NoCharge);
        }
        Ok(())
    }
    /// `checkAffordable`.
    pub fn check_affordable(&self, user: &User) -> EssResult {
        self.charge.is_affordable_for(user)
    }
    /// `setTime`.
    pub fn set_time(&self, user: &User) {
        user.set_kit_timestamp(&self.name, now_ms());
    }
    /// `resetTime`.
    pub fn reset_time(&self, user: &User) {
        user.set_kit_timestamp(&self.name, 0);
    }
    /// `chargeUser`.
    pub fn charge_user(&self, user: &User) -> EssResult {
        self.charge.charge(user)
    }
    /// `getNextUse`.
    pub fn get_next_use(&self, user: &User) -> EssResult<i64> {
        if user.is_authorized("essentials.kit.exemptdelay") {
            return Ok(0);
        }
        let now = now_ms();
        let delay = match self.kit.as_mapping().and_then(|kit| kit.get("delay")) {
            None => 0.0,
            Some(Value::Number(_)) => self.kit.as_mapping().and_then(|kit| kit.get("delay")).map_or(0.0, scalar_f64),
            Some(_) => return Err(EssError::tl("kitError2", &[])),
        };
        let last = user.get_kit_timestamp(&self.name);
        let until = last + i64::from(delay as i32) * 1000 + i64::from(((delay * 1000.0) % 1000.0) as i32);
        Ok(if last == 0 || last > now {
            0
        } else if delay < 0.0 {
            -1
        } else if until < now {
            0
        } else {
            until
        })
    }
    /// `getItems`.
    pub fn get_items(&self) -> EssResult<Vec<String>> {
        let bad = |why: &str| {
            crate::warning(&format!("Error parsing kit {}: {why}", self.name));
            EssError::tl("kitError2", &[])
        };
        let Some(Value::Sequence(items)) = self.kit.as_mapping().and_then(|kit| kit.get("items")) else {
            return Err(bad("Invalid item list"));
        };
        items.iter().map(|item| if item.is_string() { scalar_string(item).ok_or_else(|| bad("Invalid kit item")) } else { Err(bad("Invalid kit item")) }).collect()
    }
    /// `expandItems`.
    pub fn expand_items(&self, user: &User) -> EssResult<bool> {
        // `getItems` first: its error comes before anything is given.
        let items = self.get_items()?;
        self.expand_items_list(user, &items)
    }
    /// `expandItems(user, items)`.
    pub fn expand_items_list(&self, user: &User, items: &[String]) -> EssResult<bool> {
        let result = self.expand(user, items);
        if let Err(error) = &result {
            crate::warning(&error.get_message());
            return Err(EssError::tl("kitError2", &[]));
        }
        result
    }
    fn expand(&self, user: &User, items: &[String]) -> EssResult<bool> {
        let Some(player) = user.player() else { return Ok(false) };
        let output = KeywordReplacer::new(SimpleTextInput::from_lines(items.to_vec()), &user.get_source(), true, true);
        let allow_unsafe = settings().allow_unsafe_enchantments();
        let auto_equip = settings().is_kit_auto_equip();
        let mut item_list: Vec<ItemStack> = Vec::new();
        let mut with_slot: BTreeMap<i32, ItemStack> = BTreeMap::new();
        let mut commands = Vec::new();
        let mut moneys = Vec::new();
        let symbol = settings().get_currency_symbol();
        let symbol = if symbol.is_empty() { "$".to_owned() } else { symbol };
        for line in output.get_lines() {
            let mut kit_item = line.clone();
            if kit_item.starts_with('$') || kit_item.starts_with(&symbol) {
                moneys.push(sanitize_currency_string(&kit_item));
                continue;
            }
            if let Some(command) = kit_item.strip_prefix('/') {
                commands.push(command.replace("{player}", &user.get_name()));
                continue;
            }
            let mut slot = -1;
            if let Some(rest) = kit_item.strip_prefix("slot:")
                && let Some(space) = rest.find(' ')
            {
                let number = &rest[..space];
                slot = if is_int(number) { number.parse().unwrap_or(-1) } else { -1 };
                kit_item = rest[space + 1..].to_owned();
            }
            if let Some(encoded) = kit_item.strip_prefix('@') {
                if let Some(stack) = decode_lines(encoded).and_then(|bytes| ItemStack::deserialize_bytes(&bytes)) {
                    if slot == -1 || with_slot.contains_key(&slot) {
                        item_list.push(stack);
                    } else {
                        with_slot.insert(slot, stack);
                    }
                    continue;
                }
                crate::warning(&crate::adventure::mini_to_legacy(&tl_literal("kitError3", crate::args![self.name.clone(), user.get_name()])));
                continue;
            }
            let parts: Vec<String> = kit_item.split_whitespace().map(str::to_owned).collect();
            let Some(first) = parts.first() else { continue };
            let amount = match parts.get(1) {
                Some(text) => text.parse::<i32>().map_err(|_| EssError::Message(format!("For input string: \"{text}\"")))?,
                None => 1,
            };
            let parsed = item_db().get_amount(first, amount)?;
            if parsed.get_type().is_air() {
                continue;
            }
            let mut meta = MetaItemStack::new(parsed);
            if parts.len() > 2 {
                meta.parse_string_meta(None, allow_unsafe, &parts, 2)?;
            }
            let stack = meta.into_item_stack();
            if slot == -1 || with_slot.contains_key(&slot) {
                item_list.push(stack);
            } else {
                with_slot.insert(slot, stack);
            }
        }
        let max_stack = if user.is_authorized("essentials.oversizedstacks") { settings().get_oversized_stack_size() } else { 0 };
        let drop_if_full = settings().is_drop_items_if_full();
        let mut total = item_list.clone();
        total.extend(with_slot.values().cloned());
        if !drop_if_full && !inventories::has_space(&player, max_stack, auto_equip, &total) {
            user.send_tl("kitInvFullNoDrop", &[]);
            return Ok(false);
        }
        for (slot, stack) in with_slot {
            if let Some(left) = inventories::add_item_at(&player, max_stack, stack, slot) {
                item_list.push(left);
            }
        }
        let left = inventories::add_item(&player, max_stack, auto_equip, &item_list);
        fail_if!(!drop_if_full && !left.is_empty(), EssError::Message("Something has gone terribly wrong while adding items to the user's inventory. Please report this to the EssentialsX developers.".to_owned()));
        let mut spew = false;
        for mut stack in left.into_values() {
            let mut spill = stack.get_amount();
            while spill > 0 {
                if max_stack != 0 {
                    stack.set_amount(spill.min(stack.get_max_stack_size()));
                }
                player.get_world().drop_item_naturally(&player.get_location(), &stack);
                spill -= stack.get_amount();
            }
            spew = true;
        }
        for value in moneys {
            let amount = BigDecimal::parse(value.trim()).ok_or_else(|| EssError::Message(format!("Character array is missing \"exponent\" mark: {value}")))?;
            Trade::of_money(amount).pay_with(user, OverflowType::Drop)?;
        }
        for command in commands {
            horizon_bukkit::dispatch_command(&command);
        }
        if spew {
            user.send_tl("kitInvFull", &[]);
        }
        Ok(true)
    }
}
