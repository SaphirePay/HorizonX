//! Destinations (`/mv tp`): a world (`w:` or a bare name), an exact place (`e:world:x,y,z[:pitch:yaw]`), a player
//! (`pl:`), an anchor (`a:`) and a bed (`b:`), the entry checks a world asks of a player (access, player limit,
//! blacklist, entry fee) and the safe-spot teleport.

use crate::util::{df, m};
use crate::worlds::{self, MvWorld};
use horizon_bukkit::{Location, Player};
use std::sync::Mutex;

pub enum Dest {
    World(String, Option<f32>),
    Exact(Location),
    Player(String),
    Anchor(String),
    Bed(Option<String>),
}

pub const IDS: &[&str] = &["w", "e", "pl", "a", "b"];

pub fn parse(input: &str) -> Result<Dest, String> {
    let (id, rest) = match input.split_once(':') {
        Some((id, rest)) if IDS.contains(&id.to_ascii_lowercase().as_str()) || id.eq_ignore_ascii_case("ca") => (id.to_ascii_lowercase(), rest),
        _ => ("w".to_owned(), input),
    };
    match id.as_str() {
        "w" => {
            let (name, dir) = rest.split_once(':').map_or((rest, None), |(n, d)| (n, Some(d)));
            let w = worlds::find(name).ok_or_else(|| m("destination.shared.failurereason.worldnotfound", &[("world", name)]))?;
            Ok(Dest::World(w.name, dir.map(yaw_of)))
        }
        "e" => {
            let parts: Vec<&str> = rest.split(':').collect();
            let bad = || m("destination.exact.failurereason.invalidformat", &[]);
            if parts.len() < 2 {
                return Err(bad());
            }
            let w = worlds::find(parts[0]).filter(MvWorld::is_loaded).ok_or_else(|| m("destination.shared.failurereason.worldnotfound", &[("world", parts[0])]))?;
            let xyz: Vec<f64> = parts[1].split(',').map(|s| s.trim().parse::<f64>()).collect::<Result<_, _>>().map_err(|_| bad())?;
            if xyz.len() != 3 {
                return Err(bad());
            }
            let pitch = parts.get(2).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
            let yaw = parts.get(3).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
            Ok(Dest::Exact(Location::new(w.world().ok_or_else(bad)?, xyz[0], xyz[1], xyz[2], yaw, pitch)))
        }
        "pl" => Ok(Dest::Player(rest.to_owned())),
        "a" => {
            anchor(rest).ok_or_else(|| m("destination.anchor.failurereason.anchornotfound", &[("anchor", rest)]))?;
            Ok(Dest::Anchor(rest.to_owned()))
        }
        "b" => Ok(Dest::Bed((!rest.is_empty() && rest != "playerbed").then(|| rest.to_owned()))),
        _ => Err(m("destination.parse.failurereason.invaliddestinationid", &[("id", &id), ("ids", &IDS.join(", "))])),
    }
}

/// A compass word or a number as a yaw.
pub fn yaw_of(dir: &str) -> f32 {
    match dir.to_ascii_lowercase().as_str() {
        "n" | "north" => 180.0,
        "ne" | "northeast" => -135.0,
        "e" | "east" => -90.0,
        "se" | "southeast" => -45.0,
        "s" | "south" => 0.0,
        "sw" | "southwest" => 45.0,
        "w" | "west" => 90.0,
        "nw" | "northwest" => 135.0,
        other => other.parse().unwrap_or(0.0),
    }
}

impl Dest {
    pub fn id(&self) -> &'static str {
        match self {
            Dest::World(..) => "w",
            Dest::Exact(_) => "e",
            Dest::Player(_) => "pl",
            Dest::Anchor(_) => "a",
            Dest::Bed(_) => "b",
        }
    }
    /// What the finer permission ends with (`multiverse.teleport.self.w.<world>`).
    pub fn target(&self) -> String {
        match self {
            Dest::World(w, _) => w.clone(),
            Dest::Exact(l) => l.world.get_name().to_owned(),
            Dest::Player(p) => p.clone(),
            Dest::Anchor(a) => a.clone(),
            Dest::Bed(p) => p.clone().unwrap_or_else(|| "playerbed".into()),
        }
    }
    pub fn display(&self) -> String {
        match self {
            Dest::World(w, _) => worlds::find(w).map_or_else(|| w.clone(), |w| w.alias_or_name()),
            Dest::Exact(l) => format!("{}:{},{},{}", l.world.get_name(), df(l.x), df(l.y), df(l.z)),
            Dest::Player(p) => p.clone(),
            Dest::Anchor(a) => m("destination.anchor.display", &[("anchor", a)]),
            Dest::Bed(None) => m("destination.bed.display.own", &[]),
            Dest::Bed(Some(p)) => m("destination.bed.display.other", &[("player", p)]),
        }
    }
    /// Whether the place is checked (and moved) for safety before a teleport.
    pub fn checks_safety(&self) -> bool {
        matches!(self, Dest::World(..) | Dest::Anchor(_))
    }
    pub fn location(&self, who: &Player) -> Result<Location, String> {
        match self {
            Dest::World(name, yaw) => {
                let w = worlds::find(name).ok_or_else(|| m("teleportfailurereason.null.world", &[]))?;
                let mut at = w.spawn().ok_or_else(|| m("teleportfailurereason.null.world", &[]))?;
                if let Some(y) = yaw {
                    at.yaw = *y;
                }
                Ok(at)
            }
            Dest::Exact(l) => Ok(l.clone()),
            Dest::Player(p) => horizon_bukkit::get_online_players().into_iter().find(|o| o.get_name().eq_ignore_ascii_case(p)).map(|o| o.get_location()).ok_or_else(|| m("destination.player.failurereason.playernotfound", &[("player", p)])),
            Dest::Anchor(a) => anchor(a).ok_or_else(|| m("destination.anchor.failurereason.anchornotfound", &[("anchor", a)])),
            Dest::Bed(p) => {
                let target = match p {
                    Some(name) => horizon_bukkit::get_online_players().into_iter().find(|o| o.get_name().eq_ignore_ascii_case(name)).ok_or_else(|| m("destination.bed.failurereason.playernotfound", &[("player", name)]))?,
                    None => who.clone(),
                };
                target.get_respawn_location().ok_or_else(|| m("teleportfailurereason.null.location", &[]))
            }
        }
    }
}

// ---- Anchors (anchors.yml: `anchors: <name>: world:x,y,z:yaw:pitch`) ----

static ANCHORS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

pub fn load_anchors(folder: &std::path::Path) {
    let text = std::fs::read_to_string(folder.join("anchors.yml")).unwrap_or_default();
    let doc: serde_yaml::Value = horizon_kit::yaml::parse(&text).unwrap_or(serde_yaml::Value::Null);
    let mut list = Vec::new();
    if let Some(serde_yaml::Value::Mapping(m)) = doc.get("anchors") {
        for (k, v) in m {
            if let (Some(k), Some(v)) = (k.as_str(), v.as_str()) {
                list.push((k.to_owned(), v.to_owned()));
            }
        }
    }
    *ANCHORS.lock().unwrap_or_else(|e| e.into_inner()) = list;
}

pub fn save_anchors() {
    let Some(folder) = crate::folder() else { return };
    let list = ANCHORS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut map = serde_yaml::Mapping::new();
    for (k, v) in list {
        map.insert(k.into(), v.into());
    }
    let mut top = serde_yaml::Mapping::new();
    top.insert("anchors".into(), serde_yaml::Value::Mapping(map));
    if let Ok(text) = serde_yaml::to_string(&top) {
        crate::util::write_file(folder.join("anchors.yml"), text);
    }
}

pub fn anchors() -> Vec<(String, String)> {
    ANCHORS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn set_anchor(name: &str, at: &Location) {
    let text = location_to_string(at);
    let mut list = ANCHORS.lock().unwrap_or_else(|e| e.into_inner());
    list.retain(|(n, _)| n != name);
    list.push((name.to_owned(), text));
    drop(list);
    save_anchors();
}

pub fn delete_anchor(name: &str) -> bool {
    let mut list = ANCHORS.lock().unwrap_or_else(|e| e.into_inner());
    let before = list.len();
    list.retain(|(n, _)| n != name);
    let done = list.len() != before;
    drop(list);
    if done {
        save_anchors();
    }
    done
}

pub fn anchor(name: &str) -> Option<Location> {
    let text = ANCHORS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(n, _)| n == name).map(|(_, v)| v.clone())?;
    string_to_location(&text)
}

/// `world:x,y,z:yaw:pitch` with two decimals, as LocationManipulation writes it.
pub fn location_to_string(l: &Location) -> String {
    format!("{}:{:.2},{:.2},{:.2}:{:.2}:{:.2}", l.world.get_name(), l.x, l.y, l.z, l.yaw, l.pitch)
}

pub fn string_to_location(s: &str) -> Option<Location> {
    let parts: Vec<&str> = s.split(':').collect();
    if !(2..=4).contains(&parts.len()) {
        return None;
    }
    let xyz: Vec<f64> = parts[1].split(',').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    if xyz.len() != 3 {
        return None;
    }
    let world = horizon_bukkit::get_world(&worlds::find(parts[0]).map_or_else(|| parts[0].to_owned(), |w| w.name))?;
    let yaw = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let pitch = parts.get(3).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    Some(Location::new(world, xyz[0], xyz[1], xyz[2], yaw, pitch))
}

/// `X: 1 Y: 2 ...` in colour, as strCoords.
pub fn str_coords(l: &Location) -> String {
    format!("§fX: §b{} §fY: §b{} §fZ: §b{} §fP: §6{} §fY: §6{} ", df(l.x), df(l.y), df(l.z), df(l.pitch as f64), df(l.yaw as f64))
}

// ---- Entry checks ----

fn players_in(world: &str) -> usize {
    horizon_bukkit::get_world(world).map_or(0, |w| w.get_players().len())
}

/// Whether `p` may enter `to` (from `from`): access, player limit, blacklist, entry fee. `Err` = the message.
pub fn can_enter(p: &Player, from: Option<&str>, to: &MvWorld) -> Result<(), String> {
    can_stay(p, to)?;
    if let Some(from) = from
        && to.list("world-blacklist").iter().any(|b| b.eq_ignore_ascii_case(from))
    {
        return Err(m("entrycheck.blacklisted", &[("world", &to.alias_or_name())]));
    }
    if let Some((amount, currency)) = to.entry_fee()
        && amount > 0.0
        && !p.has_permission(&format!("multiverse.exempt.{}", to.name))
    {
        if !currency.eq_ignore_ascii_case("@vault-economy") || !horizon_kit::hooks::economy::enabled() {
            return Err(m("entrycheck.cannotpayentryfee", &[]));
        }
        if !horizon_kit::hooks::economy::has(p.get_name(), amount) {
            return Err(m("entrycheck.notenoughmoney", &[("amount", &horizon_kit::hooks::economy::format(amount))]));
        }
    }
    Ok(())
}

pub fn can_stay(p: &Player, to: &MvWorld) -> Result<(), String> {
    if crate::config::b("world.enforce-access", false) && !p.has_permission(&format!("multiverse.access.{}", to.name)) {
        return Err(m("entrycheck.noworldaccess", &[]));
    }
    let limit = to.player_limit();
    if limit > -1 && !p.has_permission(&format!("mv.bypass.playerlimit.{}", to.name)) && players_in(&to.name) as i64 >= limit {
        return Err(m("entrycheck.exceedplayerlimit", &[]));
    }
    Ok(())
}

/// The entry fee taken (or a negative one paid out) after a world change.
pub fn pay_entry(p: &Player, to: &MvWorld) {
    let Some((amount, currency)) = to.entry_fee() else { return };
    if p.has_permission(&format!("multiverse.exempt.{}", to.name)) || !currency.eq_ignore_ascii_case("@vault-economy") {
        return;
    }
    use horizon_kit::hooks::economy;
    if amount > 0.0 {
        if economy::withdraw(p.get_name(), amount) {
            p.send_message(&m("economy.vault.withdraw", &[("price", &economy::format(amount))]));
        }
    } else if economy::deposit(p.get_name(), -amount) {
        p.send_message(&m("economy.vault.deposit", &[("price", &economy::format(-amount))]));
    }
}

// ---- Safe teleports ----

fn safe_at(l: &Location) -> bool {
    let w = &l.world;
    let (x, y, z) = (l.x.floor() as i32, l.y.floor() as i32, l.z.floor() as i32);
    let feet = w.get_block_at(x, y, z);
    let head = w.get_block_at(x, y + 1, z);
    let below = w.get_block_at(x, y - 1, z);
    let lava = |b: &horizon_bukkit::Block| b.get_type().name().contains("LAVA") || b.get_type().name().contains("FIRE");
    feet.is_passable() && head.is_passable() && !lava(&feet) && !lava(&head) && !lava(&below) && (below.is_solid() || below.get_type().name() == "WATER")
}

/// The place, or a safe one near it (the highest block of the column, then a short spiral): `None` when nothing is.
pub fn safe_spot(l: &Location) -> Option<Location> {
    if safe_at(l) {
        return Some(l.clone());
    }
    let w = l.world.clone();
    let (bx, bz) = (l.x.floor() as i32, l.z.floor() as i32);
    for r in 0..=3i32 {
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() != r && dz.abs() != r {
                    continue;
                }
                let (x, z) = (bx + dx, bz + dz);
                let y = w.get_highest_block_y_at(x, z) + 1;
                if y <= w.get_min_height() || y >= w.get_max_height() {
                    continue;
                }
                let at = Location::new(w.clone(), x as f64 + 0.5, y as f64, z as f64 + 0.5, l.yaw, l.pitch);
                if safe_at(&at) {
                    return Some(at);
                }
            }
        }
    }
    None
}

/// Teleports `who` to `dest`: Err = the reason (teleportfailurereason message).
pub fn teleport(who: &Player, dest: &Dest, check_safety: bool) -> Result<(), String> {
    let at = dest.location(who)?;
    teleport_to(who, &at, check_safety && dest.checks_safety())
}

/// The safety check reads blocks, and an unloaded chunk reads as air: when the place's chunks aren't loaded
/// they are loaded first (in the background, `get_chunks_at_async`), then checked and the player sent; a
/// failure then is told to the player.
pub fn teleport_to(who: &Player, at: &Location, check_safety: bool) -> Result<(), String> {
    if !check_safety {
        return if who.teleport(at) { Ok(()) } else { Err(m("teleportfailurereason.teleport.failed", &[])) };
    }
    let (cx, cz) = ((at.x.floor() as i32) >> 4, (at.z.floor() as i32) >> 4);
    let world = at.world.clone();
    if world.is_chunk_loaded(cx, cz) {
        let at = safe_spot(at).ok_or_else(|| m("teleportfailurereason.unsafe.location", &[]))?;
        return if who.teleport(&at) { Ok(()) } else { Err(m("teleportfailurereason.teleport.failed", &[])) };
    }
    let (who, at) = (who.clone(), at.clone());
    world.get_chunks_at_async(cx - 1, cz - 1, cx + 1, cz + 1, true, move || match safe_spot(&at) {
        Some(safe) => {
            if !who.teleport(&safe) {
                who.send_message(&m("teleportfailurereason.teleport.failed", &[]));
            }
        }
        None => who.send_message(&m("teleportfailurereason.unsafe.location", &[])),
    });
    Ok(())
}
