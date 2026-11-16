//! `com.earth2me.essentials.api.Economy`: the store's API, what `/eco` and any other Rust plugin call.
//! Java's name and UUID overloads are `by_name` and `by_uuid` in front of the same functions.
use crate::prelude::*;
use crate::utils::string::{name_uuid_from_bytes, safe_string};

/// Vault's `Economy`, for any plugin: `getBalance`, `has`, `withdrawPlayer`, `depositPlayer`, `format`, `hasAccount`, by player name.
pub fn provide_vault(plugin: horizon_bukkit::Plugin) {
    use serde_json::{Value, json};
    let answer = |method: &str, args: &Value| -> Value {
        let (name, amount) = (args[0].as_str().unwrap_or_default(), args[1].as_f64().unwrap_or(0.0));
        let response = |user: &User, done: bool| json!({ "success": done, "balance": user.get_money().to_f64() });
        match method {
            "isEnabled" => json!(true),
            "getName" => json!("EssentialsX Economy"),
            "format" => json!(format(&BigDecimal::from_f64(args[0].as_f64().unwrap_or(0.0)))),
            "hasAccount" => json!(player_exists(name)),
            "getBalance" => by_name(name).map_or(json!(0.0), |user| json!(user.get_money().to_f64())),
            "has" => by_name(name).is_ok_and(|user| has_enough(&user, &BigDecimal::from_f64(amount))).into(),
            "withdrawPlayer" => by_name(name).map_or(json!({ "success": false }), |user| {
                let done = has_enough(&user, &BigDecimal::from_f64(amount)) && subtract(&user, &BigDecimal::from_f64(amount)).is_ok();
                response(&user, done)
            }),
            "depositPlayer" => by_name(name).map_or(json!({ "success": false }), |user| {
                let done = add(&user, &BigDecimal::from_f64(amount)).is_ok();
                response(&user, done)
            }),
            _ => Value::Null,
        }
    };
    horizon_bukkit::persistence::provide_service(&plugin, "net.milkbowl.vault.economy.Economy", "Highest", answer);
}

/// `UserDoesNotExistException`.
fn no_such_user(name: &str) -> EssError {
    EssError::tl("userDoesNotExist", crate::args![name])
}

/// `getUserByName`.
pub fn by_name(name: &str) -> EssResult<User> {
    ess().get_offline_user(name).ok_or_else(|| no_such_user(name))
}

/// `getUserByUUID`.
pub fn by_uuid(uuid: &str) -> EssResult<User> {
    ess().get_user_by_uuid(uuid).ok_or_else(|| no_such_user(uuid))
}

/// `getMoneyExact`.
pub fn get_money_exact(user: &User) -> BigDecimal {
    user.get_money()
}

/// `setMoney`.
pub fn set_money(user: &User, balance: &BigDecimal) -> EssResult {
    // `NoLoanPermittedException`, both times.
    fail_if!(*balance < settings().get_min_money() || (balance.signum() < 0 && !user.is_authorized("essentials.eco.loan")), EssError::tl("negativeBalanceError", &[]));
    user.set_money(balance.clone())?;
    crate::trade::Trade::log("API", "Set", "API", Some(&user.get_name()), Some(&crate::trade::Trade::of_money(balance.clone())), None, None, None, Some(balance));
    Ok(())
}

/// `add`.
pub fn add(user: &User, amount: &BigDecimal) -> EssResult {
    set_money(user, &user.get_money().add(amount))
}

/// `subtract`.
pub fn subtract(user: &User, amount: &BigDecimal) -> EssResult {
    set_money(user, &user.get_money().subtract(amount))
}

/// `divide`.
pub fn divide(user: &User, amount: &BigDecimal) -> EssResult {
    let result = user.get_money().divide_128(amount).ok_or_else(|| EssError::Message("Division by zero".to_owned()))?;
    set_money(user, &result)
}

/// `multiply`.
pub fn multiply(user: &User, amount: &BigDecimal) -> EssResult {
    set_money(user, &user.get_money().multiply(amount))
}

/// `resetBalance`.
pub fn reset_balance(user: &User) -> EssResult {
    set_money(user, &settings().get_starting_balance())
}

/// `hasEnough`.
pub fn has_enough(user: &User, amount: &BigDecimal) -> bool {
    get_money_exact(user) >= *amount
}

/// `hasMore`.
pub fn has_more(user: &User, amount: &BigDecimal) -> bool {
    get_money_exact(user) > *amount
}

/// `hasLess`.
pub fn has_less(user: &User, amount: &BigDecimal) -> bool {
    get_money_exact(user) < *amount
}

/// `isNegative`.
pub fn is_negative(user: &User) -> bool {
    get_money_exact(user).signum() < 0
}

/// `format`.
pub fn format(amount: &BigDecimal) -> String {
    crate::adventure::mini_to_legacy(&display_currency(amount))
}

/// `playerExists(String)`.
pub fn player_exists(name: &str) -> bool {
    ess().get_offline_user(name).is_some()
}

/// `playerExists(UUID)`.
pub fn player_exists_by_uuid(uuid: &str) -> bool {
    ess().get_user_by_uuid(uuid).is_some()
}

/// `isNPC`.
pub fn is_npc(name: &str) -> EssResult<bool> {
    Ok(by_name(name)?.is_npc())
}

/// `createNPCFile` and `createNPC`.
pub fn create_npc(name: &str) -> bool {
    if ess().get_offline_user(name).is_some() {
        return false;
    }
    let npc_name = safe_string(name);
    let uuid = name_uuid_from_bytes(format!("NPC:{npc_name}").as_bytes());
    let user = User::load(&uuid, Some(&npc_name), None);
    user.set_npc(true);
    let _ = user.set_money(settings().get_starting_balance());
    user.holder().last_account_name = Some(npc_name.clone());
    user.save();
    ess().users().add_cached_npc_name(&uuid, &npc_name);
    true
}

/// `removeNPC`.
pub fn remove_npc(name: &str) -> EssResult {
    by_name(name)?.reset();
    Ok(())
}
