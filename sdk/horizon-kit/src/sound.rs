//! Sounds by any name a config uses: Bukkit's enum names (`ENTITY_PLAYER_LEVELUP`), keys
//! (`entity.player.levelup`, `minecraft:entity.player.levelup`), old names with `.` or `-`.
//! [`key`] gives the resource key `Player.play_sound` wants; [`play`] parses `NAME[:volume[:pitch]]` too.

use horizon_bukkit::Player;
use std::collections::HashMap;
use std::sync::LazyLock;

static BY_NAME: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| include_str!("sounds.tsv").lines().filter_map(|l| l.split_once('\t')).collect());

/// `minecraft:entity.player.levelup` for `ENTITY_PLAYER_LEVELUP`, `entity.player.levelup` or the key itself.
pub fn key(name: &str) -> Option<String> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    if n.contains(':') {
        return Some(n.to_ascii_lowercase());
    }
    let upper = n.to_ascii_uppercase().replace(['.', '-', ' '], "_");
    if let Some(k) = BY_NAME.get(upper.as_str()) {
        return Some(format!("minecraft:{k}"));
    }
    // Already a key (lower case with dots).
    (n.contains('.') && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_'))).then(|| format!("minecraft:{n}"))
}

/// `NAME`, `NAME:volume`, `NAME:volume:pitch` or `NAME volume pitch` (the forms menu plugins write) -> key,
/// volume, pitch.
pub fn parse(spec: &str) -> Option<(String, f32, f32)> {
    let s = spec.trim();
    let parts: Vec<&str> = if s.contains(' ') { s.split_whitespace().collect() } else { s.split(':').collect() };
    // `minecraft:x` keys hold a colon themselves.
    let (name, rest): (String, &[&str]) = if parts.len() >= 2 && parts[0].eq_ignore_ascii_case("minecraft") { (format!("minecraft:{}", parts[1]), &parts[2..]) } else { (parts.first()?.to_string(), &parts[1..]) };
    let volume = rest.first().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let pitch = rest.get(1).and_then(|v| v.parse().ok()).unwrap_or(1.0);
    Some((key(&name)?, volume, pitch))
}

/// Plays a sound (any name form, with optional `:volume:pitch`) to one player where they stand.
pub fn play(player: &Player, spec: &str) -> bool {
    match parse(spec) {
        Some((k, v, p)) => {
            player.play_sound(&player.get_location(), &k, v, p);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert_eq!(key("ENTITY_PLAYER_LEVELUP").as_deref(), Some("minecraft:entity.player.levelup"));
        assert_eq!(key("entity.player.levelup").as_deref(), Some("minecraft:entity.player.levelup"));
        assert_eq!(key("minecraft:block.note_block.pling").as_deref(), Some("minecraft:block.note_block.pling"));
        assert_eq!(parse("ENTITY_PLAYER_LEVELUP:0.5:2").map(|p| (p.1, p.2)), Some((0.5, 2.0)));
        assert_eq!(parse("minecraft:ui.button.click 1 1").map(|p| p.0).as_deref(), Some("minecraft:ui.button.click"));
    }
}
