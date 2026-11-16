//! `Worth`: `worth.yml`.
use crate::prelude::*;
use crate::config::Config;
use serde_yaml::Value;
use std::sync::Mutex;

pub struct Worth {
    config: Mutex<Config>,
}

fn item_key(stack: &ItemStack) -> String {
    stack.get_type().name().to_lowercase().replace('_', "")
}

impl Worth {
    pub fn new() -> Self {
        let mut config = Config::with_template(crate::data_folder().join("worth.yml"), include_str!("../resources/worth.yml"));
        config.load();
        Self { config: Mutex::new(config) }
    }
    fn config(&self) -> std::sync::MutexGuard<'_, Config> {
        self.config.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `getPrice`.
    pub fn get_price(&self, stack: &ItemStack) -> Option<BigDecimal> {
        let config = self.config();
        let name = item_key(stack);
        let none = BigDecimal::from_i64(-1);
        let mut result = none.clone();
        if config.section_keys(&format!("worth.{name}")).is_some_and(|keys| keys.len() == 1) {
            result = config.get_big_decimal(&format!("worth.{name}.0"), none.clone());
        }
        if result.signum() < 0 {
            result = config.get_big_decimal(&format!("worth.{name}.*"), none.clone());
        }
        if result.signum() < 0 {
            result = config.get_big_decimal(&format!("worth.{name}"), none);
        }
        (result.signum() >= 0).then_some(result)
    }
    /// `getAmount`.
    pub fn get_amount(&self, user: &User, stack: Option<&ItemStack>, args: &[&str], is_bulk_sell: bool) -> EssResult<i32> {
        let stack = stack.filter(|stack| !stack.get_type().is_air()).ok_or_else(|| EssError::tl("itemSellAir", &[]))?;
        let mut amount = 0;
        if let Some(text) = args.get(1) {
            let digits: String = text.chars().filter(char::is_ascii_digit).collect();
            amount = digits.parse::<i32>().map_err(|_| EssError::not_enough())?;
            if text.starts_with('-') {
                amount = -amount;
            }
        }
        let in_stacks = args.get(1).is_some_and(|text| text.ends_with('s'));
        let require_stack = settings().is_trade_in_stacks(stack.get_type());
        fail_if!(require_stack && !in_stacks, EssError::tl("itemMustBeStacked", &[]));
        let mut max = 0;
        if let Some(player) = user.player() {
            for held in inventories::get_inventory(&player, false).into_iter().flatten() {
                if held.is_similar(stack) {
                    max += held.get_amount();
                }
            }
        }
        let stack_size = stack.get_type().get_max_stack_size();
        if in_stacks {
            amount *= stack_size;
        }
        if amount < 1 {
            amount += max;
        }
        if require_stack {
            amount -= amount % stack_size;
        }
        if (amount > max || amount < 1) && !is_bulk_sell {
            user.send_tl("itemNotEnough2", &[]);
            user.send_tl("itemNotEnough3", &[]);
            return Err(EssError::tl("itemNotEnough1", &[]));
        }
        Ok(amount)
    }
    /// `setPrice`.
    pub fn set_price(&self, stack: &ItemStack, price: f64) {
        let mut config = self.config();
        config.set(&format!("worth.{}", item_key(stack)), Value::from(price));
        config.save();
    }
    pub fn get_file(&self) -> std::path::PathBuf {
        self.config().get_file().to_owned()
    }
    /// `reloadConfig`.
    pub fn reload_config(&self) {
        self.config().load();
    }
}
