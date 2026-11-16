//! What plugins ask of each other, without Vault or PlaceholderAPI jars:
//!
//! - [`economy`]: money, through Vault's `net.milkbowl.vault.economy.Economy` service (EssentialsX provides it).
//! - [`chat`]: rank prefix/suffix/group, through Vault's `net.milkbowl.vault.chat.Chat` (LuckPerms-rs provides it,
//!   from a cache it keeps: a call is one engine round trip and a map lookup).
//! - [`placeholders`]: `%player_name%`, `%vault_prefix%`, `%server_online%`... built in, and any other
//!   `%id_rest%` asked of the plugin that provides `horizon.placeholder.<id>` ([`provide_placeholders`]), so
//!   mcMMO's levels can show in TAB, Jobs' in a leaderboard, and so on.
//!
//! All of these are game queries: call them on the server thread (or read on it and hand values to a worker).

use horizon_bukkit::Player;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Who has which service, looked up once per 10 s per name (a `service_list` query each time is a waste).
static PROVIDERS: Mutex<Option<HashMap<String, (Instant, Option<horizon_bukkit::persistence::ServiceProvider>)>>> = Mutex::new(None);

/// The provider of `name`, cached briefly. `None` when no plugin provides it.
pub fn provider(name: &str) -> Option<horizon_bukkit::persistence::ServiceProvider> {
    let mut held = PROVIDERS.lock().unwrap_or_else(|e| e.into_inner());
    let map = held.get_or_insert_with(HashMap::new);
    if let Some((at, p)) = map.get(name)
        && at.elapsed() < Duration::from_secs(10)
    {
        return p.clone();
    }
    drop(held);
    let found = horizon_bukkit::persistence::service_registration(name);
    PROVIDERS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(name.to_owned(), (Instant::now(), found.clone()));
    found
}

/// Forget the cached providers (a plugin was enabled or swapped).
pub fn refresh() {
    *PROVIDERS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn call(service: &str, method: &str, args: Value) -> Value {
    provider(service).map_or(Value::Null, |p| p.call(method, &args))
}

/// Money, by player name (as Vault's `OfflinePlayer` overloads resolve them).
pub mod economy {
    use super::*;
    const SERVICE: &str = "net.milkbowl.vault.economy.Economy";

    /// An economy is there to use.
    pub fn enabled() -> bool {
        super::provider(SERVICE).is_some()
    }
    pub fn balance(name: &str) -> f64 {
        call(SERVICE, "getBalance", json!([name])).as_f64().unwrap_or(0.0)
    }
    pub fn has(name: &str, amount: f64) -> bool {
        call(SERVICE, "has", json!([name, amount])).as_bool().unwrap_or(false)
    }
    /// Takes `amount`; false (nothing taken) when they can't pay or there's no economy.
    pub fn withdraw(name: &str, amount: f64) -> bool {
        if amount <= 0.0 {
            return amount == 0.0;
        }
        call(SERVICE, "withdrawPlayer", json!([name, amount]))["success"].as_bool().unwrap_or(false)
    }
    pub fn deposit(name: &str, amount: f64) -> bool {
        if amount <= 0.0 {
            return amount == 0.0;
        }
        call(SERVICE, "depositPlayer", json!([name, amount]))["success"].as_bool().unwrap_or(false)
    }
    /// The economy's own money text (`$1,234.50`), else two decimals.
    pub fn format(amount: f64) -> String {
        call(SERVICE, "format", json!([amount])).as_str().map_or_else(|| format!("{amount:.2}"), str::to_owned)
    }
}

/// Rank meta, by player name or uuid.
pub mod chat {
    use super::*;
    const SERVICE: &str = "net.milkbowl.vault.chat.Chat";

    pub fn prefix(who: &str) -> String {
        call(SERVICE, "getPlayerPrefix", json!([who])).as_str().unwrap_or_default().to_owned()
    }
    pub fn suffix(who: &str) -> String {
        call(SERVICE, "getPlayerSuffix", json!([who])).as_str().unwrap_or_default().to_owned()
    }
    pub fn primary_group(who: &str) -> String {
        call(SERVICE, "getPrimaryGroup", json!([who])).as_str().unwrap_or("default").to_owned()
    }
    pub fn groups(who: &str) -> Vec<String> {
        strings(call(SERVICE, "getPlayerGroups", json!([who])))
    }
    /// Every prefix, highest weight first (LuckPerms `getPrefixes`).
    pub fn prefixes(who: &str) -> Vec<String> {
        strings(call(SERVICE, "getPlayerPrefixes", json!([who])))
    }
    pub fn suffixes(who: &str) -> Vec<String> {
        strings(call(SERVICE, "getPlayerSuffixes", json!([who])))
    }
    /// A `meta.KEY.VALUE` value (LuckPerms `getMetaValue`, Vault `getPlayerInfoString`).
    /// A whole-number meta value (`lp group vip meta set myplugin.homes 5`), asked of LuckPerms at most every
    /// 30 s per player and key: rank perks read in hot paths cost a map lookup.
    pub fn meta_int(who: &str, key: &str) -> Option<i64> {
        use std::collections::HashMap;
        use std::sync::Mutex;
        type Cache = HashMap<(String, String), (u64, Option<i64>)>;
        static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
        let now = crate::time::now_ms();
        let k = (who.to_owned(), key.to_owned());
        if let Some((at, v)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&k).cloned())
            && now.saturating_sub(at) < 30_000
        {
            return v;
        }
        let v = meta(who, key).and_then(|s| s.trim().parse::<f64>().ok()).map(|f| f as i64);
        CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(k, (now, v));
        v
    }
    pub fn meta(who: &str, key: &str) -> Option<String> {
        call(SERVICE, "getPlayerInfoString", json!([who, key])).as_str().map(str::to_owned)
    }
    /// Everything the chat service knows of a player in one call: prefix, suffix, primary, groups, prefixes,
    /// suffixes, meta (an object). For plugins that need several (chat formats, tab lists).
    pub fn all(who: &str) -> Meta {
        let p = call(SERVICE, "getPlayerPrefixes", json!([who]));
        let s = call(SERVICE, "getPlayerSuffixes", json!([who]));
        let prefixes = strings(p);
        let suffixes = strings(s);
        Meta {
            prefix: prefixes.first().cloned().unwrap_or_default(),
            suffix: suffixes.first().cloned().unwrap_or_default(),
            primary: primary_group(who),
            prefixes,
            suffixes,
            meta: call(SERVICE, "getPlayerMeta", json!([who])),
        }
    }
    /// The track holds the group (LuckPerms tracks).
    pub fn track_contains(track: &str, group: &str) -> bool {
        call(SERVICE, "trackContains", json!([track, group])).as_bool().unwrap_or(false)
    }
    fn strings(v: Value) -> Vec<String> {
        v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
    }

    #[derive(Clone, Debug, Default)]
    pub struct Meta {
        pub prefix: String,
        pub suffix: String,
        pub primary: String,
        pub prefixes: Vec<String>,
        pub suffixes: Vec<String>,
        /// `meta.KEY.VALUE` as an object, keys lower case.
        pub meta: Value,
    }
    impl Meta {
        pub fn value(&self, key: &str) -> Option<&str> {
            self.meta[key.to_ascii_lowercase()].as_str()
        }
    }
}

/// Offers `%<id>_...%` placeholders to every other plugin: `answer(uuid, name, params)` (params = the text after
/// `<id>_`, uuid/name empty when there's no player). `None` = not ours, left as typed.
pub fn provide_placeholders(plugin: &horizon_bukkit::Plugin, id: &str, answer: impl Fn(&str, &str, &str) -> Option<String> + Send + Sync + 'static) {
    let service = format!("horizon.placeholder.{}", id.to_ascii_lowercase());
    horizon_bukkit::persistence::provide_service(plugin, &service, "Normal", move |method, args| {
        if method != "request" {
            return Value::Null;
        }
        let s = |i: usize| args[i].as_str().unwrap_or_default();
        answer(s(0), s(1), s(2)).map_or(Value::Null, Value::String)
    });
}

/// `%...%` placeholders in `text` for `player` (none: only the server ones).
pub fn placeholders(text: &str, player: Option<&Player>) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let key = &after[..end];
        if key.is_empty() || key.contains(' ') {
            out.push('%');
            rest = after;
            continue;
        }
        match placeholder(key, player) {
            Some(v) => out.push_str(&v),
            None => {
                out.push('%');
                out.push_str(key);
                out.push('%');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// One placeholder's value (the key without its `%`s).
pub fn placeholder(key: &str, player: Option<&Player>) -> Option<String> {
    let lower = key.to_ascii_lowercase();
    if let Some(v) = server_value(&lower) {
        return Some(v);
    }
    if let Some(p) = player
        && let Some(v) = player_value(&lower, p)
    {
        return Some(v);
    }
    let (id, _) = lower.split_once('_')?;
    let (uuid, name) = player.map(|p| (p.get_unique_id(), p.get_name().to_owned())).unwrap_or_default();
    let params = &key[id.len() + 1..];
    call(&format!("horizon.placeholder.{id}"), "request", json!([uuid, name, params])).as_str().map(str::to_owned)
}

fn server_value(key: &str) -> Option<String> {
    Some(match key {
        "server_online" | "bungee_total" | "online" => horizon_bukkit::get_online_players().len().to_string(),
        "server_max_players" | "max_players" => horizon_bukkit::fam_core::Server.get_max_players().to_string(),
        "server_tps" | "server_tps_1" => format!("{:.1}", horizon_bukkit::server::get_tps()[0].min(20.0)),
        "server_tps_5" => format!("{:.1}", horizon_bukkit::server::get_tps()[1].min(20.0)),
        "server_tps_15" => format!("{:.1}", horizon_bukkit::server::get_tps()[2].min(20.0)),
        "server_name" => "Horizon".to_owned(),
        "server_time" => now_hms(),
        _ => return None,
    })
}

fn now_hms() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{:02}:{:02}:{:02}", secs / 3600 % 24, secs / 60 % 60, secs % 60)
}

fn player_value(key: &str, p: &Player) -> Option<String> {
    Some(match key {
        "player_name" | "player" | "name" => p.get_name().to_owned(),
        "player_displayname" | "displayname" | "player_display_name" | "essentials_nickname" => {
            let d = p.get_display_name();
            if d.is_empty() { p.get_name().to_owned() } else { d.to_owned() }
        }
        "essentials_nickname_stripped" => crate::text::strip(p.get_display_name()),
        "player_online" => "yes".to_owned(),
        "player_max_health" => format!("{:.0}", p.get_max_health()),
        "player_food_level" => p.get_food_level().to_string(),
        "player_exp" => format!("{:.2}", p.get_exp()),
        "player_biome" => p.get_location().get_block().get_biome()?.to_ascii_lowercase(),
        "player_uuid" => p.get_unique_id(),
        "player_world" | "world" => p.get_world().get_name().to_owned(),
        "player_x" => p.get_location().get_block_x().to_string(),
        "player_y" => p.get_location().get_block_y().to_string(),
        "player_z" => p.get_location().get_block_z().to_string(),
        "player_health" => format!("{:.0}", p.get_health()),
        "player_level" => p.get_level().to_string(),
        "player_ping" | "ping" => p.get_ping().to_string(),
        "player_gamemode" => p.get_game_mode().to_ascii_lowercase(),
        "vault_prefix" | "luckperms_prefix" | "prefix" => chat::prefix(&p.get_unique_id()),
        "vault_suffix" | "luckperms_suffix" | "suffix" => chat::suffix(&p.get_unique_id()),
        "vault_rank" | "vault_group" | "luckperms_primary_group_name" | "luckperms_primary_group" | "group" => chat::primary_group(&p.get_unique_id()),
        "vault_eco_balance" | "balance" | "essentials_balance" => format!("{:.2}", economy::balance(p.get_name())),
        "vault_eco_balance_formatted" | "essentials_balance_formatted" => economy::format(economy::balance(p.get_name())),
        "vault_eco_balance_fixed" => format!("{:.0}", economy::balance(p.get_name()).floor()),
        _ => return None,
    })
}

/// The player behind a hit: the one who swung, or who shot the arrow/trident/snowball that struck. None for mobs
/// and the world.
pub fn attacker(e: &horizon_bukkit::fam_ev_entity::EntityDamageByEntityEvent) -> Option<horizon_bukkit::Player> {
    use horizon_bukkit::fam_ev_entity::Actor;
    match e.get_damager()? {
        Actor::Player(p) => Some(p),
        Actor::Entity(ent) => ent.cast::<horizon_bukkit::entities::Projectile>().get_shooter(),
    }
}
