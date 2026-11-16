//! The signs of `Signs` that are a few lines each.
use crate::prelude::*;
use super::protection::SignProtection;
use super::trade_sign::SignTrade;
use super::{EssentialsSign, ISign, get_goods, get_item_meta, get_item_stack, get_price, sign_text, validate_goods, validate_integer, validate_price};
use crate::commands::repair::{repair_all, repair_hand};
use crate::items::{enchantment_by_name, enchantment_max_level, enchantment_real_name};
use crate::kits::Kit;
use crate::mail::get_mail_line;
use crate::spawn_mob::{mob_data, mob_parts, spawnmob};
use crate::textreader::{KeywordReplacer, TextPager, text_input};
use crate::trade::OverflowType;
use crate::utils::tick_format;
use horizon_bukkit::Inventory;

/// `Signs.values()`, in order.
pub const ALL: &[&dyn EssentialsSign] = &[
    &Menu("Anvil", |player| player.open_anvil(None, true)),
    &SignBalance,
    &SignBuy,
    &Menu("Cartography", |player| player.open_cartography_table(None, true)),
    &SignDisposal,
    &SignEnchant,
    &SignFree,
    &SignGameMode,
    &Menu("Grindstone", |player| player.open_grindstone(None, true)),
    &SignHeal,
    &SignInfo,
    &SignKit,
    &Menu("Loom", |player| player.open_loom(None, true)),
    &SignMail,
    &SignProtection,
    &SignRepair,
    &SignSell,
    &Menu("Smithing", |player| player.open_smithing_table(None, true)),
    &SignSpawnmob,
    &SignTime,
    &SignTrade,
    &SignWarp,
    &SignWeather,
    &Menu("Workbench", |player| player.open_workbench(None, true)),
    &SignRandomTeleport,
];

fn at(sign: &dyn ISign) -> horizon_bukkit::Location {
    sign.get_block().get_location()
}

fn log(subtype: &str, sign: &dyn ISign, username: &str, charge: &Trade, user: &User) {
    Trade::log("Sign", subtype, "Interact", Some(username), None, Some(username), Some(charge), Some(&at(sign)), Some(&user.get_money()));
}

/// `SignAnvil` and its five kin: a menu of the game's.
struct Menu(&'static str, fn(&Player) -> bool);

impl EssentialsSign for Menu {
    fn name(&self) -> &'static str {
        self.0
    }
    fn on_interact(&self, _sign: &mut dyn ISign, user: &User, _username: &str) -> EssResult<bool> {
        if let Some(player) = user.player() {
            let open = self.1;
            crate::inventories::after_turn(move || {
                open(&player);
            });
        }
        Ok(true)
    }
}

struct SignBalance;

impl EssentialsSign for SignBalance {
    fn name(&self) -> &'static str {
        "Balance"
    }
    fn on_interact(&self, _sign: &mut dyn ISign, user: &User, _username: &str) -> EssResult<bool> {
        user.send_tl("balance", crate::args![parsed(display_currency(&user.get_money()))]);
        Ok(true)
    }
}

/// `chargeAmount.divide(new BigDecimal(count)).multiply(new BigDecimal(held))`: Java's `divide`
/// throws on a price that doesn't divide exactly.
fn bulk_price(price: &BigDecimal, count: i32, held: i32) -> EssResult<BigDecimal> {
    let count = BigDecimal::from_i64(i64::from(count));
    let each = price.divide_128(&count).filter(|each| each.multiply(&count) == *price);
    let each = each.ok_or_else(|| EssError::Message("Non-terminating decimal expansion; no exact representable decimal result.".to_owned()))?;
    Ok(each.multiply(&BigDecimal::from_i64(i64::from(held))))
}

/// `(items, price)`, scaled to what the player holds when they sneak.
fn bulk(user: &User, items: Trade, price: Trade) -> EssResult<(Trade, Trade)> {
    let sneaking = user.player().is_some_and(|player| player.is_sneaking());
    if settings().is_allow_bulk_buy_sell()
        && sneaking
        && let (Some(stack), Some(held), Some(money)) = (items.get_item_stack(), user.get_item_in_hand(), price.get_money())
        && stack.is_similar(&held)
    {
        let scaled = bulk_price(money, stack.get_amount(), held.get_amount())?;
        let mut stack = stack.clone();
        stack.set_amount(held.get_amount());
        return Ok((Trade::of_item(stack), Trade::of_money(scaled)));
    }
    Ok((items, price))
}

struct SignBuy;

impl EssentialsSign for SignBuy {
    fn name(&self) -> &'static str {
        "Buy"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        validate_goods(sign, 1, 2)?;
        validate_price(sign, self.name(), 3)?;
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let items = get_goods(sign, 1, 2)?;
        let charge = get_price(sign, self.name(), 3, 1)?;
        let (items, charge) = bulk(user, items, charge)?;
        charge.is_affordable_for(user)?;
        fail_if!(!items.pay(user)?, EssError::tl("inventoryFull", &[]));
        charge.charge(user)?;
        Trade::log("Sign", "Buy", "Interact", Some(username), Some(&charge), Some(username), Some(&items), Some(&at(sign)), Some(&user.get_money()));
        Ok(true)
    }
}

struct SignSell;

impl EssentialsSign for SignSell {
    fn name(&self) -> &'static str {
        "Sell"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        validate_goods(sign, 1, 2)?;
        validate_price(sign, self.name(), 3)?;
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let charge = get_goods(sign, 1, 2)?;
        let money = get_price(sign, self.name(), 3, 1)?;
        let sneaking = user.player().is_some_and(|player| player.is_sneaking());
        let bulked = settings().is_allow_bulk_buy_sell() && sneaking && charge.get_item_stack().zip(user.get_item_in_hand()).is_some_and(|(stack, held)| stack.is_similar(&held));
        let (charge, money) = bulk(user, charge, money)?;
        let money = match money.get_money().filter(|_| bulked) {
            Some(price) => Trade::of_money(price.multiply(&settings().get_multiplier(user))),
            None => money,
        };
        charge.is_affordable_for(user)?;
        money.pay_with(user, OverflowType::Drop)?;
        charge.charge(user)?;
        Trade::log("Sign", "Sell", "Interact", Some(username), Some(&charge), Some(username), Some(&money), Some(&at(sign)), Some(&user.get_money()));
        Ok(true)
    }
}

struct SignDisposal;

impl EssentialsSign for SignDisposal {
    fn name(&self) -> &'static str {
        "Disposal"
    }
    fn on_create(&self, sign: &mut dyn ISign, user: &User, _username: &str) -> EssResult<bool> {
        if !user.is_authorized("essentials.signs.disposal.name") {
            for line in 1..4 {
                sign.set_line(line, "");
            }
        }
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, _username: &str) -> EssResult<bool> {
        let mut title = format!("{} {} {}", sign.get_line(1), sign.get_line(2), sign.get_line(3)).trim().to_owned();
        if title.is_empty() {
            title = user.player_tl("disposal", &[]);
        }
        if let Some(player) = user.player() {
            player.open_inventory(&Inventory::create(36, &title));
        }
        Ok(true)
    }
}

struct SignEnchant;

fn is_any_item(name: &str) -> bool {
    name == "*" || name.eq_ignore_ascii_case("any")
}

/// `MaterialTagProvider`: the game's item tags.
fn tag_values(name: &str) -> Vec<String> {
    horizon_bukkit::registry::Tag::get("item", &name.to_lowercase()).get_values()
}

impl EssentialsSign for SignEnchant {
    fn name(&self) -> &'static str {
        "Enchant"
    }
    fn on_create(&self, sign: &mut dyn ISign, user: &User, _username: &str) -> EssResult<bool> {
        let item_name = sign.get_line(1);
        let stack = if is_any_item(&item_name) || !tag_values(&item_name).is_empty() {
            None
        } else {
            match get_item_stack(&item_name, 1) {
                Ok(stack) => Some(stack),
                Err(error) => {
                    sign.set_line(1, "\u{a7}c<item|any>");
                    return Err(error);
                }
            }
        };
        let line = sign.get_line(2);
        let parts = java_split(&line, ':');
        let Some(enchantment) = parts.first().and_then(|name| enchantment_by_name(name)) else {
            sign.set_line(2, "\u{a7}c<enchant>");
            return Err(EssError::tl("enchantmentNotFound", &[]));
        };
        let mut level = 1;
        if let Some(text) = parts.get(1) {
            level = text.parse().map_err(|_| {
                sign.set_line(2, "\u{a7}c<enchant>");
                EssError::tl("errorWithMessage", crate::args![format!("For input string: \"{text}\"")])
            })?;
        }
        let allow_unsafe = settings().allow_unsafe_enchantments() && user.is_authorized("essentials.enchantments.allowunsafe") && user.is_authorized("essentials.signs.enchant.allowunsafe");
        let max = enchantment_max_level(&enchantment);
        if level < 0 || (!allow_unsafe && level > max) {
            level = max;
            sign.set_line(2, &format!("{}:{level}", parts[0]));
        }
        if let Some(stack) = stack {
            let mut meta = crate::items::meta::MetaItemStack::new(stack);
            meta.add_enchantment(allow_unsafe, &enchantment, level).map_err(|error| EssError::tl("errorWithMessage", crate::args![error.get_message()]))?;
        }
        get_price(sign, self.name(), 3, 1)?;
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let hand = user.get_item_in_hand().unwrap_or_else(inventories::air);
        let item_name = sign.get_line(1);
        let tagged = {
            let id = hand.get_type().key();
            tag_values(&item_name).iter().any(|value| *value == id)
        };
        let search = if is_any_item(&item_name) || tagged { None } else { Some(get_item_stack(&item_name, 1)?) };
        let charge = get_price(sign, self.name(), 3, 1)?;
        charge.is_affordable_for(user)?;
        let line = sign.get_line(2);
        let parts = java_split(&line, ':');
        let enchantment = parts.first().and_then(|name| enchantment_by_name(name)).ok_or_else(|| EssError::tl("enchantmentNotFound", &[]))?;
        let mut level = 1;
        if let Some(text) = parts.get(1) {
            level = text.parse().map_err(|_| EssError::tl("errorWithMessage", crate::args![format!("For input string: \"{text}\"")]))?;
        }
        fail_if!(hand.get_amount() != 1 || (level > 0 && hand.get_enchant_level(&enchantment) == level as u32), EssError::tl("missingItems", crate::args![1, item_name]));
        if let Some(search) = search.filter(|search| search.get_type() != hand.get_type()) {
            return Err(EssError::tl("missingItems", crate::args![1, search.get_type().name().to_lowercase().replace('_', " ")]));
        }
        let allow_unsafe = settings().allow_unsafe_enchantments() && user.is_authorized("essentials.signs.enchant.allowunsafe");
        let mut meta = crate::items::meta::MetaItemStack::new(hand);
        meta.add_enchantment(allow_unsafe, &enchantment, level).map_err(|error| EssError::tl("errorWithMessage", crate::args![error.get_message()]))?;
        if let Some(player) = user.player() {
            inventories::set_item_in_hand(&player, meta.get_item_stack());
        }
        let name = enchantment_real_name(&enchantment).replace('_', " ");
        user.send_tl(if level == 0 { "enchantmentRemoved" } else { "enchantmentApplied" }, crate::args![name]);
        charge.charge(user)?;
        Trade::log("Sign", "Enchant", "Interact", Some(username), Some(&charge), Some(username), Some(&charge), Some(&at(sign)), Some(&user.get_money()));
        Ok(true)
    }
}

struct SignFree;

impl EssentialsSign for SignFree {
    fn name(&self) -> &'static str {
        "Free"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        let checked = get_item_stack(&sign.get_line(1), 1).and_then(|item| get_item_meta(None, item, &sign.get_line(2))).and_then(|item| get_item_meta(None, item, &sign.get_line(3)));
        if let Err(error) = checked {
            sign.set_line(1, "\u{a7}c<item>");
            return Err(EssError::tl("errorWithMessage", crate::args![error.get_message()]));
        }
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let source = user.get_source();
        let stack = get_item_stack(&sign.get_line(1), 1)?;
        let stack = get_item_meta(Some(&source), stack, &sign.get_line(2))?;
        let mut item: ItemStack = get_item_meta(Some(&source), stack, &sign.get_line(3))?;
        fail_if!(item.get_type().is_air(), EssError::tl("cantSpawnItem", crate::args!["Air"]));
        item.set_amount(item.get_type().get_max_stack_size());
        let title = item.get_display_name().unwrap_or_else(|| item.get_type().name());
        let mut free = Inventory::create(36, &title);
        for slot in 0..36 {
            free.set_item(slot, Some(item.clone()));
        }
        if let Some(player) = user.player() {
            player.open_inventory(&free);
        }
        Trade::log("Sign", "Free", "Interact", Some(username), None, Some(username), Some(&Trade::of_item(item)), Some(&at(sign)), Some(&user.get_money()));
        Ok(true)
    }
}

struct SignGameMode;

impl EssentialsSign for SignGameMode {
    fn name(&self) -> &'static str {
        "GameMode"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        if sign.get_line(1).is_empty() {
            sign.set_line(1, "Survival");
        }
        validate_price(sign, self.name(), 2)?;
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let charge = get_price(sign, self.name(), 2, 1)?;
        let mode = sign_text(sign, 1).to_lowercase();
        fail_if!(mode.is_empty(), EssError::tl("invalidSignLine", crate::args![2]));
        charge.is_affordable_for(user)?;
        let chosen = if mode.contains("survi") || mode == "0" {
            "SURVIVAL"
        } else if mode.contains("creat") || mode == "1" {
            "CREATIVE"
        } else if mode.contains("advent") || mode == "2" {
            "ADVENTURE"
        } else if mode.contains("spec") || mode == "3" {
            "SPECTATOR"
        } else {
            return Err(EssError::tl("invalidSignLine", crate::args![2]));
        };
        if let Some(player) = user.player() {
            player.set_game_mode(chosen);
        }
        user.send_tl("gameMode", crate::args![user.player_tl(&chosen.to_lowercase(), &[]), user.get_display_name()]);
        Trade::log("Sign", "gameMode", "Interact", Some(username), None, Some(username), Some(&charge), Some(&at(sign)), Some(&user.get_money()));
        charge.charge(user)?;
        Ok(true)
    }
}

struct SignHeal;

impl EssentialsSign for SignHeal {
    fn name(&self) -> &'static str {
        "Heal"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        validate_price(sign, self.name(), 1)?;
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let Some(player) = user.player() else { return Ok(false) };
        fail_if!(player.get_health() == 0.0, EssError::tl("healDead", &[]));
        let charge = get_price(sign, self.name(), 1, 1)?;
        charge.is_affordable_for(user)?;
        player.set_health(player.get_max_health());
        player.set_food_level(20);
        player.set_fire_ticks(0);
        user.send_tl("youAreHealed", &[]);
        charge.charge(user)?;
        log("Heal", sign, username, &charge, user);
        Ok(true)
    }
}

struct SignInfo;

impl EssentialsSign for SignInfo {
    fn name(&self) -> &'static str {
        "Info"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        validate_price(sign, self.name(), 3)?;
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let charge = get_price(sign, self.name(), 3, 1)?;
        charge.is_affordable_for(user)?;
        let (chapter, page) = (sign.get_line(1), sign.get_line(2));
        let source = user.get_source();
        user.set_display_nick();
        let input = text_input(&source, "info", true).map_err(|error| EssError::tl("errorWithMessage", crate::args![error]))?;
        let (chapters, bookmarks) = (input.chapters.clone(), input.bookmarks.clone());
        let output = KeywordReplacer::new(input, &source, true, false);
        TextPager { lines: output.get_lines(), chapters: &chapters, bookmarks: &bookmarks, one_page: false }.show_page(Some(&chapter), Some(&page), None, &source);
        charge.charge(user)?;
        log("Info", sign, username, &charge, user);
        Ok(true)
    }
}

struct SignKit;

impl EssentialsSign for SignKit {
    fn name(&self) -> &'static str {
        "Kit"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        validate_price(sign, self.name(), 3)?;
        let kit = sign.get_line(1).to_lowercase().trim().to_owned();
        if kit.is_empty() {
            sign.set_line(1, "\u{a7}dKit name!");
            return Ok(false);
        }
        let group = sign.get_line(2);
        if group.eq_ignore_ascii_case("Everyone") || group.eq_ignore_ascii_case("Everybody") {
            sign.set_line(2, "\u{a7}2Everyone");
        }
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let name = sign.get_line(1).to_lowercase().trim().to_owned();
        let group = sign_text(sign, 2);
        let allowed = if group.is_empty() { user.is_authorized(&format!("essentials.kits.{name}")) } else { group == "\u{a7}2Everyone" || user.in_group(&group) };
        fail_if!(!allowed, if group.is_empty() { EssError::tl("noKitPermission", crate::args![format!("essentials.kits.{name}")]) } else { EssError::tl("noKitGroup", crate::args![group]) });
        let charge = get_price(sign, self.name(), 3, 1)?;
        charge.is_affordable_for(user)?;
        let run = || -> EssResult {
            let kit = Kit::new(&name)?;
            kit.check_delay(user)?;
            kit.set_time(user);
            kit.expand_items(user)?;
            charge.charge(user)
        };
        match run() {
            Ok(()) => {
                log("Kit", sign, username, &charge, user);
                Ok(true)
            }
            Err(EssError::NoCharge) => Ok(false),
            Err(error) => Err(EssError::tl("errorWithMessage", crate::args![error.get_message()])),
        }
    }
}

struct SignMail;

impl EssentialsSign for SignMail {
    fn name(&self) -> &'static str {
        "Mail"
    }
    fn on_interact(&self, _sign: &mut dyn ISign, user: &User, _username: &str) -> EssResult<bool> {
        let mut kept = Vec::new();
        for mut mail in user.get_mail_messages() {
            if mail.is_expired() {
                continue;
            }
            user.send_message(&get_mail_line(&mail));
            mail.read = true;
            kept.push(mail);
        }
        if kept.is_empty() {
            user.send_tl("noNewMail", &[]);
            return Ok(false);
        }
        user.set_mail_list(kept);
        user.send_tl("markMailAsRead", &[]);
        Ok(true)
    }
}

struct SignRandomTeleport;

impl EssentialsSign for SignRandomTeleport {
    fn name(&self) -> &'static str {
        "RandomTeleport"
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, _username: &str) -> EssResult<bool> {
        let target = user.clone();
        ess().random_teleport().get_random_location(&sign.get_line(1), move |location| {
            let future = EssFuture::new();
            let told = target.clone();
            future.then_accept(move |success| {
                if success {
                    told.send_tl("tprSuccess", &[]);
                }
            });
            target.get_async_teleport().now(&location, false, "COMMAND", future);
        });
        Ok(true)
    }
}

struct SignRepair;

impl EssentialsSign for SignRepair {
    fn name(&self) -> &'static str {
        "Repair"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        let target = sign.get_line(1);
        if target.is_empty() {
            sign.set_line(1, "Hand");
        } else if !target.eq_ignore_ascii_case("all") && !target.eq_ignore_ascii_case("hand") {
            sign.set_line(1, "\u{a7}c<hand|all>");
            return Err(EssError::tl("invalidSignLine", crate::args![2]));
        }
        validate_price(sign, self.name(), 2)?;
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let charge = get_price(sign, self.name(), 2, 1)?;
        charge.is_affordable_for(user)?;
        let target = sign.get_line(1);
        if target.eq_ignore_ascii_case("hand") {
            repair_hand(user)?;
        } else if target.eq_ignore_ascii_case("all") {
            repair_all(user)?;
        } else {
            return Err(EssError::tl("errorWithMessage", crate::args![""]));
        }
        charge.charge(user)?;
        log("Repair", sign, username, &charge, user);
        Ok(true)
    }
}

struct SignSpawnmob;

impl EssentialsSign for SignSpawnmob {
    fn name(&self) -> &'static str {
        "Spawnmob"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        validate_integer(sign, 1)?;
        validate_price(sign, self.name(), 3)?;
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let charge = get_price(sign, self.name(), 3, 1)?;
        charge.is_affordable_for(user)?;
        let mobs = sign.get_line(2);
        let count = sign.get_line(1).parse().map_err(|_| EssError::tl("errorWithMessage", crate::args![format!("For input string: \"{}\"", sign.get_line(1))]))?;
        spawnmob(&user.get_source(), user, mob_parts(&mobs), &mob_data(&mobs), count).map_err(|error| EssError::tl("errorWithMessage", crate::args![error.get_message()]))?;
        charge.charge(user)?;
        log("Spawnmob", sign, username, &charge, user);
        Ok(true)
    }
}

fn lines_empty(sign: &dyn ISign) -> bool {
    (1..4).all(|line| sign.get_line(line).is_empty())
}

struct SignTime;

impl EssentialsSign for SignTime {
    fn name(&self) -> &'static str {
        "Time"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        if lines_empty(sign) {
            return Ok(true);
        }
        validate_price(sign, self.name(), 2)?;
        let word = sign.get_line(1);
        for (name, marked) in [("Day", "\u{a7}2Day"), ("Night", "\u{a7}2Night")] {
            if word.eq_ignore_ascii_case(name) {
                sign.set_line(1, marked);
                return Ok(true);
            }
        }
        Err(EssError::tl("onlyDayNight", &[]))
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let Some(world) = user.get_world() else { return Ok(false) };
        if lines_empty(sign) {
            user.send_tl("timeWorldCurrentSign", crate::args![tick_format::format(world.get_time())]);
            return Ok(true);
        }
        let charge = get_price(sign, self.name(), 2, 1)?;
        charge.is_affordable_for(user)?;
        let word = sign.get_line(1);
        let mut time = world.get_time();
        time -= time % 24_000;
        for (marked, offset, subtype) in [("\u{a7}2Day", 24_000, "TimeDay"), ("\u{a7}2Night", 37_700, "TimeNight")] {
            if word.eq_ignore_ascii_case(marked) {
                world.set_time(time + offset);
                charge.charge(user)?;
                log(subtype, sign, username, &charge, user);
                return Ok(true);
            }
        }
        Err(EssError::tl("onlyDayNight", &[]))
    }
}

struct SignWarp;

impl EssentialsSign for SignWarp {
    fn name(&self) -> &'static str {
        "Warp"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        validate_price(sign, self.name(), 3)?;
        let warp = sign.get_line(1);
        if warp.is_empty() {
            sign.set_line(1, "\u{a7}c<Warp name>");
            return Err(EssError::tl("invalidSignLine", crate::args![1]));
        }
        ess().warps().get_warp(&warp)?;
        let group = sign.get_line(2);
        if group.eq_ignore_ascii_case("Everyone") || group.eq_ignore_ascii_case("Everybody") {
            sign.set_line(2, "\u{a7}2Everyone");
        }
        Ok(true)
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let (warp, group) = (sign.get_line(1), sign.get_line(2));
        if !group.is_empty() {
            fail_if!(group != "\u{a7}2Everyone" && !user.in_group(&group), EssError::tl("warpUsePermission", &[]));
        } else if settings().get_per_warp_permission() && !user.is_authorized(&format!("essentials.warps.{warp}")) {
            return Err(EssError::tl("warpUsePermission", &[]));
        }
        let charge = get_price(sign, self.name(), 3, 1)?;
        let future = EssFuture::new();
        let (logged, name, buyer, place) = (charge.clone(), username.to_owned(), user.clone(), at(sign));
        future.then_accept(move |success| {
            if success {
                Trade::log("Sign", "Warp", "Interact", Some(&name), None, Some(&name), Some(&logged), Some(&place), Some(&buyer.get_money()));
            }
        });
        let source = user.get_source();
        future.exceptionally(move |error| ess().show_error(&source, error, "\\ sign: Warp"));
        user.get_async_teleport().warp(user, &warp, Some(&charge), "PLUGIN", future);
        Ok(true)
    }
}

struct SignWeather;

impl EssentialsSign for SignWeather {
    fn name(&self) -> &'static str {
        "Weather"
    }
    fn on_create(&self, sign: &mut dyn ISign, _user: &User, _username: &str) -> EssResult<bool> {
        if lines_empty(sign) {
            return Ok(true);
        }
        validate_price(sign, self.name(), 2)?;
        let word = sign.get_line(1);
        for (name, marked) in [("Sun", "\u{a7}2Sun"), ("Storm", "\u{a7}2Storm")] {
            if word.eq_ignore_ascii_case(name) {
                sign.set_line(1, marked);
                return Ok(true);
            }
        }
        sign.set_line(1, "\u{a7}c<sun|storm>");
        Err(EssError::tl("onlySunStorm", &[]))
    }
    fn on_interact(&self, sign: &mut dyn ISign, user: &User, username: &str) -> EssResult<bool> {
        let Some(world) = user.get_world() else { return Ok(false) };
        if lines_empty(sign) {
            user.send_tl(if world.has_storm() { "weatherSignStorm" } else { "weatherSignSun" }, &[]);
            return Ok(true);
        }
        let charge = get_price(sign, self.name(), 2, 1)?;
        charge.is_affordable_for(user)?;
        let word = sign.get_line(1);
        for (marked, storm, subtype) in [("\u{a7}2Sun", false, "WeatherSun"), ("\u{a7}2Storm", true, "WeatherStorm")] {
            if word.eq_ignore_ascii_case(marked) {
                world.set_storm(storm);
                charge.charge(user)?;
                log(subtype, sign, username, &charge, user);
                return Ok(true);
            }
        }
        Err(EssError::tl("onlySunStorm", &[]))
    }
}
