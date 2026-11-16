//! `BanLookup`.
use horizon_bukkit::core2::{BanEntry, get_ban_entries};

/// `getBanEntry`.
pub fn get_ban_entry(name: &str) -> Option<BanEntry> {
    get_ban_entries(false).into_iter().find(|entry| entry.get_target() == name)
}

/// `isBanned`.
pub fn is_banned(name: &str) -> bool {
    get_ban_entry(name).is_some()
}

/// `getExpiration().getTime()`.
pub fn expiration(entry: &BanEntry) -> Option<i64> {
    let when = entry.get_expiration()?;
    chrono::DateTime::parse_from_str(when, "%Y-%m-%d %H:%M:%S %z").ok().map(|date| date.timestamp_millis())
}
