//! `Trade`: a price in money, items, experience or a command's cost.
use crate::prelude::*;
use crate::inventories::Leftover;
use crate::{args, set_exp_fix};
use std::fs::File;
use std::io::Write;
use std::sync::Mutex;

static LOG: Mutex<Option<File>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeType {
    Money,
    Exp,
    Item,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverflowType {
    Abort,
    Drop,
    Return,
}

#[derive(Clone, Debug, Default)]
pub struct Trade {
    command: Option<String>,
    fallback: Option<Box<Trade>>,
    money: Option<BigDecimal>,
    item: Option<ItemStack>,
    exp: Option<i32>,
}

impl Trade {
    /// `new Trade(String, ess)`.
    pub fn of_command(command: &str) -> Self {
        Self { command: Some(command.to_owned()), ..Self::default() }
    }
    /// `new Trade(String, Trade, ess)`.
    pub fn of_command_with_fallback(command: &str, fallback: Self) -> Self {
        Self { command: Some(command.to_owned()), fallback: Some(Box::new(fallback)), ..Self::default() }
    }
    /// `new Trade(BigDecimal, ess)`.
    pub fn of_money(money: BigDecimal) -> Self {
        Self { money: Some(money), ..Self::default() }
    }
    /// `new Trade(ItemStack, ess)`.
    pub fn of_item(item: ItemStack) -> Self {
        Self { item: Some(item), ..Self::default() }
    }
    /// `new Trade(int, ess)`.
    pub fn of_exp(exp: i32) -> Self {
        Self { exp: Some(exp), ..Self::default() }
    }
    pub fn get_money(&self) -> Option<&BigDecimal> {
        self.money.as_ref()
    }
    pub fn get_item_stack(&self) -> Option<&ItemStack> {
        self.item.as_ref()
    }
    pub fn get_experience(&self) -> Option<i32> {
        self.exp
    }
    /// `getType`.
    pub fn get_type(&self) -> TradeType {
        if self.exp.is_some() {
            TradeType::Exp
        } else if self.item.is_some() {
            TradeType::Item
        } else {
            TradeType::Money
        }
    }
    /// `isAffordableFor(user)`.
    pub fn is_affordable_for(&self, user: &User) -> EssResult {
        let future = EssFuture::new();
        self.is_affordable_for_future(user, &future);
        future.failure().map_or(Ok(()), Err)
    }
    /// `isAffordableFor(user, future)`.
    pub fn is_affordable_for_future(&self, user: &User, future: &EssFuture) {
        if settings().is_debug() {
            crate::info(&format!("checking if {} can afford charge.", user.get_name()));
        }
        if let Some(money) = &self.money
            && money.signum() > 0
            && !user.can_afford(money)
        {
            future.complete_exceptionally(EssError::charge("notEnoughMoney", args![parsed(display_currency(money))]));
            return;
        }
        if let Some(item) = &self.item
            && !user.player().is_some_and(|player| inventories::contains_at_least(&player, item, item.get_amount()))
        {
            future.complete_exceptionally(EssError::charge("missingItems", args![item.get_amount(), item_db().name(item).unwrap_or_default()]));
            return;
        }
        if self.command.as_deref().is_some_and(|command| !command.is_empty()) {
            let cost = self.get_command_cost(user);
            if cost.signum() > 0 && !user.can_afford(&cost) {
                future.complete_exceptionally(EssError::charge("notEnoughMoney", args![parsed(display_currency(&cost))]));
                return;
            }
        }
        if let Some(exp) = self.exp
            && exp > 0
            && user.player().is_none_or(|player| set_exp_fix::get_total_experience(&player) < exp)
        {
            future.complete_exceptionally(EssError::charge("notEnoughExperience", &[]));
        }
    }
    /// `pay(user)`: true if nothing was left over.
    pub fn pay(&self, user: &User) -> EssResult<bool> {
        Ok(self.pay_with(user, OverflowType::Abort)?.is_none())
    }
    /// `pay(user, type)`: what didn't fit, if anything.
    pub fn pay_with(&self, user: &User, overflow: OverflowType) -> EssResult<Option<Leftover>> {
        if let Some(money) = self.money.as_ref().filter(|money| money.signum() > 0) {
            if settings().is_debug() {
                crate::info(&format!("paying user {} via trade {}", user.get_name(), money.to_plain_string()));
            }
            user.give_money(money, None)?;
        }
        if let (Some(item), Some(player)) = (&self.item, user.player()) {
            if overflow == OverflowType::Abort && !inventories::has_space(&player, 0, false, std::slice::from_ref(item)) {
                let mut left = Leftover::new();
                left.insert(0, item.clone());
                return Ok(Some(left));
            }
            let left = inventories::add_item(&player, 0, false, std::slice::from_ref(item));
            if !left.is_empty() {
                if overflow == OverflowType::Return {
                    return Ok(Some(left));
                }
                for mut stack in left.into_values() {
                    let mut spill = stack.get_amount();
                    while spill > 0 {
                        stack.set_amount(spill.min(stack.get_max_stack_size()));
                        player.get_world().drop_item_naturally(&player.get_location(), &stack);
                        spill -= stack.get_amount();
                    }
                }
            }
        }
        if let (Some(exp), Some(player)) = (self.exp, user.player()) {
            set_exp_fix::set_total_experience(&player, set_exp_fix::get_total_experience(&player) + exp);
        }
        Ok(None)
    }
    /// `charge(user)`.
    pub fn charge(&self, user: &User) -> EssResult {
        let future = EssFuture::new();
        self.charge_future(user, &future);
        future.failure().map_or(Ok(()), Err)
    }
    /// `charge(user, future)`.
    pub fn charge_future(&self, user: &User, future: &EssFuture) {
        if settings().is_debug() {
            crate::info(&format!("attempting to charge user {}", user.get_name()));
        }
        if let Some(money) = &self.money {
            if !user.can_afford(money) && money.signum() > 0 {
                future.complete_exceptionally(EssError::charge("notEnoughMoney", args![parsed(display_currency(money))]));
                return;
            }
            user.take_money(money, None);
        }
        if let Some(item) = &self.item {
            let Some(player) = user.player() else { return };
            if !inventories::contains_at_least(&player, item, item.get_amount()) {
                future.complete_exceptionally(EssError::charge("missingItems", args![item.get_amount(), item.get_type().name().to_lowercase().replace('_', " ")]));
                return;
            }
            inventories::remove_item_amount(&player, item, item.get_amount());
        }
        if self.command.is_some() {
            let cost = self.get_command_cost(user);
            if !user.can_afford(&cost) && cost.signum() > 0 {
                future.complete_exceptionally(EssError::charge("notEnoughMoney", args![parsed(display_currency(&cost))]));
                return;
            }
            user.take_money(&cost, None);
        }
        if let (Some(exp), Some(player)) = (self.exp, user.player()) {
            let experience = set_exp_fix::get_total_experience(&player);
            if experience < exp && exp > 0 {
                future.complete_exceptionally(EssError::charge("notEnoughExperience", &[]));
                return;
            }
            set_exp_fix::set_total_experience(&player, experience - exp);
        }
        if settings().is_debug() {
            crate::info(&format!("charge user {} completed", user.get_name()));
        }
    }
    /// `getCommandCost`.
    pub fn get_command_cost(&self, user: &User) -> BigDecimal {
        let mut cost = BigDecimal::zero();
        let Some(command) = self.command.as_deref().filter(|command| !command.is_empty()) else { return cost };
        cost = settings().get_command_cost(command.strip_prefix('/').unwrap_or(command));
        if cost.signum() == 0
            && let Some(fallback) = &self.fallback
        {
            cost = fallback.get_command_cost(user);
        }
        if settings().is_debug() {
            crate::info(&format!("calculated command ({command}) cost for {} as {cost}", user.get_name()));
        }
        if cost.signum() != 0 && (user.is_authorized("essentials.nocommandcost.all") || user.is_authorized(&format!("essentials.nocommandcost.{command}"))) {
            return BigDecimal::zero();
        }
        cost
    }
    /// `log`, to `trade.log`.
    #[allow(clippy::too_many_arguments)]
    pub fn log(kind: &str, subtype: &str, event: &str, sender: Option<&str>, charge: Option<&Self>, receiver: Option<&str>, pay: Option<&Self>, location: Option<&Location>, end_balance: Option<&BigDecimal>) {
        if (location.is_none() && !settings().is_eco_log_update_enabled()) || (location.is_some() && !settings().is_eco_log_enabled()) {
            return;
        }
        let identify = |name: &str| {
            if settings().is_eco_log_uuid_enabled() {
                ess().users().get_name_cache().get(name).cloned().unwrap_or_else(|| name.to_owned())
            } else {
                name.to_owned()
            }
        };
        let side = |trade: Option<&Self>| match trade {
            None => "\"\",\"\",\"\"".to_owned(),
            Some(trade) => {
                let mut text = String::new();
                if let Some(item) = &trade.item {
                    text.push_str(&format!("{},{},", item.get_amount(), item.get_type().name()));
                }
                if let Some(money) = &trade.money {
                    text.push_str(&format!("{money},money,{}", settings().get_currency_symbol()));
                }
                if let Some(exp) = trade.exp {
                    text.push_str(&format!("{exp},exp,\"\""));
                }
                text
            }
        };
        let mut line = format!("{kind},{subtype},{event},\"{}\",\"", chrono::Local::now().format("%A, %B %-d, %Y at %-I:%M:%S %p %Z"));
        if let Some(sender) = sender {
            line.push_str(&identify(sender));
        }
        line.push_str("\",");
        line.push_str(&side(charge));
        line.push_str(",\"");
        if let Some(receiver) = receiver {
            line.push_str(&identify(receiver));
        }
        line.push_str("\",");
        line.push_str(&side(pay));
        match location {
            None => line.push_str(",\"\",\"\",\"\",\"\""),
            Some(at) => line.push_str(&format!(",\"{}\",{},{},{},", at.get_world().get_name(), at.get_block_x(), at.get_block_y(), at.get_block_z())),
        }
        if let Some(balance) = end_balance {
            line.push_str(&balance.to_string());
        }
        line.push_str(",\n");
        let mut file = LOG.lock().unwrap_or_else(|e| e.into_inner());
        if file.is_none() {
            match std::fs::OpenOptions::new().create(true).append(true).open(crate::data_folder().join("trade.log")) {
                Ok(opened) => *file = Some(opened),
                Err(error) => crate::severe(&error.to_string()),
            }
        }
        if let Some(file) = file.as_mut()
            && let Err(error) = file.write_all(line.as_bytes())
        {
            crate::severe(&error.to_string());
        }
    }
    /// `closeLog`.
    pub fn close_log() {
        *LOG.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}
