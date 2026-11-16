//! `signs`: `EssentialsSign`, its `ISign`s, and the list in `Signs`. Sign API events aren't fired:
//! nothing here can listen.
pub mod kinds;
pub mod listeners;
pub mod protection;
pub mod trade_sign;

use crate::fail_if;
use crate::adventure::{mini_to_legacy, strip_color};
use crate::big_decimal::BigDecimal;
use crate::error::{EssError, EssResult};
use crate::ess::{ess, item_db, settings};
use crate::i18n::tl_literal;
use crate::items::meta::MetaItemStack;
use crate::trade::Trade;
use crate::user::User;
use crate::utils::format::strip_format;
use crate::utils::material::{is_sign_post, is_wall_hanging_sign, is_wall_sign};
use crate::utils::number::{sanitize_currency_string, short_currency};
use horizon_bukkit::blocks::Sign;
use horizon_bukkit::fam_ev_world::SignChangeEvent;
use horizon_bukkit::{Block, ItemStack, Material, Player};
use regex::Regex;
use std::sync::LazyLock;

/// `MINTRANSACTION`.
pub fn min_transaction() -> BigDecimal {
    BigDecimal::parse("0.01").unwrap_or_else(BigDecimal::zero)
}

static MONEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^0-9-.]?[.0-9]+[^0-9-.]?$").expect("regex"));
static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ :]+").expect("regex"));

/// `ISign`.
pub trait ISign {
    fn get_line(&self, index: usize) -> String;
    fn set_line(&mut self, index: usize, text: &str);
    fn get_block(&self) -> &Block;
    fn update_sign(&mut self) {}
}

/// Java takes the private-use marks out of every line it reads.
fn without_marks(text: &str) -> String {
    text.chars().filter(|mark| !('\u{f700}'..='\u{f747}').contains(mark)).collect()
}

/// `EventSign`: lines being written, not yet on the block.
pub struct EventSign<'a> {
    event: &'a mut SignChangeEvent,
    block: Block,
}

impl ISign for EventSign<'_> {
    fn get_line(&self, index: usize) -> String {
        without_marks(self.event.get_line(index).unwrap_or_default())
    }
    fn set_line(&mut self, index: usize, text: &str) {
        self.event.set_line(index, text);
    }
    fn get_block(&self) -> &Block {
        &self.block
    }
}

/// `BlockSign`.
pub struct BlockSign {
    block: Block,
}

impl BlockSign {
    pub fn new(block: &Block) -> Self {
        Self { block: block.clone() }
    }
}

impl ISign for BlockSign {
    fn get_line(&self, index: usize) -> String {
        without_marks(&Sign(self.block.tile()).get_line(index).unwrap_or_default())
    }
    fn set_line(&mut self, index: usize, text: &str) {
        let mut sign = Sign(self.block.tile());
        sign.set_line(index, text);
        sign.update();
    }
    fn get_block(&self) -> &Block {
        &self.block
    }
}

/// The start of `isValidSign`: `§1[anything]`.
fn looks_valid(line: &str) -> bool {
    line.strip_prefix("\u{a7}1[").is_some_and(|rest| rest.ends_with(']'))
}

/// `isValidSign(ISign)`.
pub fn is_any_sign(sign: &dyn ISign) -> bool {
    looks_valid(&sign.get_line(0))
}

/// `isValidSign(ess, ISign)`.
pub fn is_valid_sign(sign: &dyn ISign) -> bool {
    let line = sign.get_line(0);
    if !looks_valid(&line) {
        return false;
    }
    let name: String = strip_color(&line).chars().filter(char::is_ascii_alphabetic).collect();
    enabled_signs().iter().any(|sign| sign.name().eq_ignore_ascii_case(&name))
}

/// `checkIfBlockBreaksSigns`.
pub fn check_if_block_breaks_signs(block: &Block) -> bool {
    let up = block.get_relative(0, 1, 0);
    if is_sign_post(&up.get_type()) && is_any_sign(&BlockSign::new(&up)) {
        return true;
    }
    for (x, z, face) in [(0, -1, "north"), (1, 0, "east"), (0, 1, "south"), (-1, 0, "west")] {
        let beside = block.get_relative(x, 0, z);
        let material = beside.get_type();
        if (is_wall_sign(&material) || is_wall_hanging_sign(&material)) && beside.get_property("facing").as_deref() == Some(face) && is_any_sign(&BlockSign::new(&beside)) {
            return true;
        }
    }
    false
}

/// `EssentialsSign`: what each sign has of its own. The `false`s are Java's refusals.
pub trait EssentialsSign: Send + Sync {
    fn name(&self) -> &'static str;
    fn on_create(&self, _sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        Ok(true)
    }
    fn on_interact(&self, _sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        Ok(true)
    }
    fn on_break(&self, _sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        Ok(true)
    }
    fn are_heavy_event_required(&self) -> bool {
        false
    }
    fn guards(&self, _material: &Material) -> bool {
        false
    }
    fn on_block_place(&self, _block: &Block, _user: &User, _username: &str) -> EssResult<bool> {
        Ok(true)
    }
    fn on_block_interact(&self, _block: &Block, _user: &User, _username: &str) -> EssResult<bool> {
        Ok(true)
    }
    fn on_block_break_by(&self, _block: &Block, _user: &User, _username: &str) -> EssResult<bool> {
        Ok(true)
    }
    /// `onBlockBreak`, `onBlockExplode`, `onBlockBurn`, `onBlockIgnite` and `onBlockPush`: one answer.
    fn on_block_harm(&self, _block: &Block) -> bool {
        true
    }
}

/// `getSuccessName`.
pub fn success_name(sign: &dyn EssentialsSign) -> Option<String> {
    let name = mini_to_legacy(&tl_literal("signFormatSuccess", crate::args![sign.name()]));
    if name.is_empty() || !name.contains(sign.name()) {
        crate::severe("signFormatSuccess message must use the {0} argument.");
        return None;
    }
    Some(name)
}

/// `getTemplateName`.
pub fn template_name(sign: &dyn EssentialsSign) -> String {
    mini_to_legacy(&tl_literal("signFormatTemplate", crate::args![sign.name()]))
}

/// `getUsername`.
pub fn get_username(user: &User) -> String {
    user.get_name().chars().take(13).collect()
}

fn show_error(user: &User, error: &EssError, sign: &dyn EssentialsSign) {
    ess().show_error(&user.get_source(), error, &format!("\\ sign: {}", sign.name()));
}

fn allowed(user: &User, name: &str, action: &str) -> bool {
    let lower = name.to_lowercase();
    user.is_authorized(&format!("essentials.signs.{lower}.{action}")) || user.is_authorized(&format!("essentials.signs.{action}.{lower}"))
}

/// `onSignCreate(event, ess)`.
pub fn sign_create(sign: &dyn EssentialsSign, event: &mut SignChangeEvent, user: &User) -> bool {
    let Some(block) = event.get_block() else { return true };
    if !allowed(user, sign.name(), "create") {
        return true;
    }
    let mut lines = EventSign { event, block };
    lines.set_line(0, &mini_to_legacy(&tl_literal("signFormatFail", crate::args![sign.name()])));
    match sign.on_create(&mut lines, user, &get_username(user)) {
        Ok(done) => {
            if done && let Some(name) = success_name(sign) {
                lines.set_line(0, &name);
            }
            done
        }
        Err(error) => {
            show_error(user, &error, sign);
            true
        }
    }
}

/// `onSignInteract(block, player, ess)`.
pub fn sign_interact(sign: &dyn EssentialsSign, block: &Block, player: &Player) -> bool {
    let Some(user) = ess().get_user(player) else { return false };
    if user.check_sign_throttle() || player.is_dead() || !allowed(&user, sign.name(), "use") {
        return false;
    }
    let mut lines = BlockSign::new(block);
    sign.on_interact(&mut lines, &user, &get_username(&user)).unwrap_or_else(|error| {
        show_error(&user, &error, sign);
        false
    })
}

/// `onSignBreak(block, player, ess)`.
pub fn sign_break(sign: &dyn EssentialsSign, block: &Block, user: &User) -> EssResult<bool> {
    if !allowed(user, sign.name(), "break") {
        return Ok(false);
    }
    let mut lines = BlockSign::new(block);
    match sign.on_break(&mut lines, user, &get_username(user)) {
        Err(EssError::MaxMoney) => Err(EssError::MaxMoney),
        Err(error) => {
            show_error(user, &error, sign);
            Ok(false)
        }
        Ok(done) => Ok(done),
    }
}

/// `onBlockPlace` and `onBlockInteract`, whose errors end in `false`.
pub fn guarded(sign: &dyn EssentialsSign, user: &User, step: EssResult<bool>) -> bool {
    step.unwrap_or_else(|error| {
        show_error(user, &error, sign);
        false
    })
}

/// `getSignText`.
pub fn sign_text(sign: &dyn ISign, line: usize) -> String {
    sign.get_line(line).trim().to_owned()
}

/// `getInteger`.
pub fn get_integer(line: &str) -> EssResult<i32> {
    line.parse().map_err(|_| EssError::tl("invalidSign", &[]))
}

/// `getIntegerPositive`.
pub fn get_integer_positive(line: &str) -> EssResult<i32> {
    let quantity = get_integer(line)?;
    fail_if!(quantity < 1, EssError::tl("moreThanZero", &[]));
    Ok(quantity)
}

/// `validateInteger`.
pub fn validate_integer(sign: &mut dyn ISign, index: usize) -> EssResult {
    let line = sign_text(sign, index);
    fail_if!(line.is_empty(), EssError::tl("emptySignLine", crate::args![index + 1]));
    let quantity = get_integer_positive(&line)?;
    sign.set_line(index, &quantity.to_string());
    Ok(())
}

/// `getItemStack`. `allowId`'s old numeric ids have no table here: see GAPS.md.
pub fn get_item_stack(name: &str, quantity: i32) -> EssResult<ItemStack> {
    let mut item = item_db().get(name).map_err(|error| match error {
        EssError::Tl { .. } => error,
        other => EssError::tl("errorWithMessage", crate::args![other.get_message()]),
    })?;
    item.set_amount(quantity);
    Ok(item)
}

/// `getItemMeta`.
pub fn get_item_meta(source: Option<&crate::command_source::CommandSource>, item: ItemStack, meta: &str) -> EssResult<ItemStack> {
    if meta.is_empty() {
        return Ok(item);
    }
    let mut stack = MetaItemStack::new(item);
    stack.add_string_meta(source, settings().allow_unsafe_enchantments(), meta).map_err(|error| EssError::tl("errorWithMessage", crate::args![error.get_message()]))?;
    Ok(stack.into_item_stack())
}

/// `getMoney`.
pub fn get_money(line: &str) -> EssResult<Option<BigDecimal>> {
    if MONEY.is_match(line) { get_big_decimal_positive(line).map(Some) } else { Ok(None) }
}

/// `getBigDecimalPositive`.
pub fn get_big_decimal_positive(line: &str) -> EssResult<BigDecimal> {
    let quantity = get_big_decimal(line)?;
    fail_if!(quantity < min_transaction(), EssError::tl("moreThanZero", &[]));
    Ok(quantity)
}

/// `getBigDecimal`.
pub fn get_big_decimal(line: &str) -> EssResult<BigDecimal> {
    BigDecimal::parse(&sanitize_currency_string(line)).ok_or_else(|| EssError::tl("errorWithMessage", crate::args!["Character array is missing \"exponent\" mark 'e' or 'E'."]))
}

/// `split("[ :]+")`, without Java's trailing empties.
pub fn split_line(line: &str) -> Vec<String> {
    let mut split: Vec<String> = SPLIT.split(line).map(str::to_owned).collect();
    while split.last().is_some_and(String::is_empty) {
        split.pop();
    }
    split
}

fn sign_trade(name: &str) -> Trade {
    Trade::of_command(&format!("{}sign", name.to_lowercase()))
}

/// `getTrade(sign, index, decrement, ess)`: a price line.
pub fn get_price(sign: &mut dyn ISign, name: &str, index: usize, decrement: i32) -> EssResult<Trade> {
    let line = sign_text(sign, index);
    if line.is_empty() {
        return Ok(sign_trade(name));
    }
    if let Some(money) = get_money(&line)? {
        return Ok(Trade::of_money(money));
    }
    let mut parts = SPLIT.splitn(&line, 2);
    let (Some(first), Some(second)) = (parts.next(), parts.next()) else {
        return Err(EssError::tl("invalidCharge", &[]));
    };
    let quantity = get_integer_positive(first)?;
    let item = second.to_lowercase();
    if item == "times" {
        sign.set_line(index, &format!("{} times", quantity - decrement));
        sign.update_sign();
        return Ok(sign_trade(name));
    }
    if item == "exp" || item == "xp" {
        sign.set_line(index, &format!("{quantity} exp"));
        return Ok(Trade::of_exp(quantity));
    }
    let stack = get_item_stack(&item, quantity)?;
    sign.set_line(index, &format!("{quantity} {item}"));
    Ok(Trade::of_item(stack))
}

/// `validateTrade(sign, index, ess)`.
pub fn validate_price(sign: &mut dyn ISign, name: &str, index: usize) -> EssResult {
    if sign_text(sign, index).is_empty() {
        return Ok(());
    }
    if let Some(money) = get_price(sign, name, index, 0)?.get_money().cloned() {
        sign.set_line(index, &short_currency(&money));
    }
    Ok(())
}

/// `getTrade(sign, amountIndex, itemIndex, player, ess)`: an amount and an item.
pub fn get_goods(sign: &dyn ISign, amount_index: usize, item_index: usize) -> EssResult<Trade> {
    let item_type = sign_text(sign, item_index);
    if item_type.eq_ignore_ascii_case("exp") || item_type.eq_ignore_ascii_case("xp") {
        return Ok(Trade::of_exp(get_integer_positive(&sign_text(sign, amount_index))?));
    }
    let mut item = get_item_stack(&item_type, 1)?;
    let amount = get_integer_positive(&sign_text(sign, amount_index))?.min(item.get_type().get_max_stack_size() * 41);
    fail_if!(item.get_type().is_air() || amount < 1, EssError::tl("moreThanZero", &[]));
    item.set_amount(amount);
    Ok(Trade::of_item(item))
}

/// `validateTrade(sign, amountIndex, itemIndex, player, ess)`.
pub fn validate_goods(sign: &mut dyn ISign, amount_index: usize, item_index: usize) -> EssResult {
    let item_type = sign_text(sign, item_index);
    if item_type.eq_ignore_ascii_case("exp") || item_type.eq_ignore_ascii_case("xp") {
        let amount = get_integer_positive(&sign_text(sign, amount_index))?;
        sign.set_line(amount_index, &amount.to_string());
        sign.set_line(item_index, "exp");
        return Ok(());
    }
    let amount = get_goods(sign, amount_index, item_index)?.get_item_stack().map_or(0, ItemStack::get_amount);
    sign.set_line(amount_index, &amount.to_string());
    sign.set_line(item_index, &item_type);
    Ok(())
}

fn owner_key() -> horizon_bukkit::persistence::NamespacedKey {
    horizon_bukkit::persistence::NamespacedKey::new(&ess().plugin(), "sign-owner")
}

/// `setOwnerData`: the owner by id on the sign itself, so that a new name keeps it.
pub fn set_owner_data(user: &User, sign: &dyn ISign) {
    let mut state = sign.get_block().tile();
    state.get_persistent_data_container().set(&owner_key(), serde_json::json!(user.get_uuid()));
    state.update();
}

/// `setOwner`.
pub fn set_owner(user: &User, sign: &mut dyn ISign, name_index: usize, prefix: &str) {
    set_owner_data(user, sign);
    sign.set_line(name_index, &format!("{prefix}{}", get_username(user)));
}

/// The id on the sign, if it has one.
fn owner_data(sign: &dyn ISign) -> Option<String> {
    sign.get_block().tile().get_persistent_data_container().get(&owner_key()).and_then(|owner| owner.as_str().map(str::to_owned))
}

/// `isOwner`: by the id, or by the name on the line for a sign from before there was one, which gets it now.
pub fn is_owner(user: &User, sign: &mut dyn ISign, name_index: usize, prefix: &str) -> bool {
    match owner_data(sign) {
        Some(owner) if owner == user.get_uuid() => {
            sign.set_line(name_index, &format!("{prefix}{}", get_username(user)));
            true
        }
        Some(_) => false,
        None => {
            let legacy = strip_format(&sign.get_line(name_index)).eq_ignore_ascii_case(&get_username(user));
            if legacy {
                set_owner_data(user, sign);
            }
            legacy
        }
    }
}

/// `isOwner`, where the sign is only being looked at: the line stays as it is.
pub fn reads_as_owner(user: &User, sign: &dyn ISign, name_index: usize) -> bool {
    owner_data(sign).map_or_else(|| strip_format(&sign.get_line(name_index)).eq_ignore_ascii_case(&get_username(user)), |owner| owner == user.get_uuid())
}

/// `Signs.values()`.
pub fn all_signs() -> &'static [&'static dyn EssentialsSign] {
    kinds::ALL
}

/// `enabledSigns`.
pub fn enabled_signs() -> Vec<&'static dyn EssentialsSign> {
    let names = settings().enabled_signs();
    all_signs().iter().copied().filter(|sign| names.iter().any(|name| name.eq_ignore_ascii_case(sign.name()))).collect()
}
