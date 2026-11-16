//! One random teleport, as the Java plugin runs it: the checks (already rtp'ing, world permission, disabled world,
//! cooldown, price, hunger), the delay (cancelled on move/damage), then attempts: a random spot, its chunk loaded
//! asynchronously (`get_chunks_at_async`: the server thread only polls whether it has arrived), a short safety
//! check, and the next attempt when it isn't safe. Never a long scan on the server thread: the overworld check is
//! a few block reads; the nether one reads one chunk snapshot and scans it in memory.

use crate::db;
use crate::settings::{RtpWorld, Settings, Shape, WorldType};
use crate::{send, settings, text};
use horizon_bukkit::{CommandSender, Location, Player, World};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RtpType {
    Command,
    Forced,
    Join,
    Respawn,
}

#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub delay: bool,
    pub cooldown: bool,
    pub check_cooldown: bool,
    pub money: bool,
    pub hunger: bool,
}

impl Info {
    pub const ALL: Info = Info { delay: true, cooldown: true, check_cooldown: true, money: true, hunger: true };
    pub fn new(delay: bool, cooldown: bool, check_cooldown: bool) -> Info {
        Info { delay, cooldown, check_cooldown, money: true, hunger: true }
    }
}

/// Where a player rtps and with which settings (WorldPlayer).
#[derive(Clone)]
pub struct Target {
    pub world: String,
    pub s: RtpWorld,
    pub kind: WorldType,
}

static RTPING: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Players in their delay: uuid -> the timer.
static DELAYS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);
pub static ANY_DELAY: AtomicBool = AtomicBool::new(false);
/// Players who can't be hurt until the instant.
static INVINCIBLE: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
pub static ANY_INVINCIBLE: AtomicBool = AtomicBool::new(false);

pub fn is_rtping(uuid: &str) -> bool {
    RTPING.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|s| s.contains(uuid))
}
fn set_rtping(uuid: &str, on: bool) {
    let mut held = RTPING.lock().unwrap_or_else(|e| e.into_inner());
    let s = held.get_or_insert_with(HashSet::new);
    if on {
        s.insert(uuid.to_owned());
    } else {
        s.remove(uuid);
    }
}

pub fn has(sender: &CommandSender, node: &str) -> bool {
    sender.has_permission(node) || sender.has_permission("betterrtp.*")
}

pub fn has_p(p: &Player, node: &str) -> bool {
    p.has_permission(node) || p.has_permission("betterrtp.*")
}

/// `betterrtp.world.*` or `betterrtp.world.<world>`: (passed, the node checked).
pub fn world_permission(sender: &CommandSender, world: &str) -> (bool, String) {
    if has(sender, "betterrtp.world.*") {
        return (true, "betterrtp.world.*".into());
    }
    let node = format!("betterrtp.world.{world}");
    (sender.has_permission(&node), node)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// ---- Settings for one request ----

/// Locations this sender may use in `world` (all worlds when `None` or UseLocationsInSameWorld is off).
pub fn locations_for(s: &Settings, sender: &CommandSender, world: Option<&str>) -> Vec<(String, RtpWorld)> {
    s.locations
        .iter()
        .filter(|(name, _)| !s.locations_require_permission || sender.has_permission(&format!("betterrtp.location.{name}")) || has(sender, "betterrtp.location.*"))
        .filter(|(_, l)| match (world, s.locations_same_world) {
            (Some(w), true) => l.world.as_deref() == Some(w),
            _ => true,
        })
        .cloned()
        .collect()
}

pub fn target_for(s: &Settings, player: Option<&Player>, sender: &CommandSender, world: &str, biomes: Option<Vec<String>>, location: Option<&RtpWorld>) -> Target {
    let mut world = world.to_owned();
    let mut chosen: Option<RtpWorld> = location.cloned();
    if chosen.is_none() && s.locations_enabled && s.use_location_if_available {
        let list = locations_for(s, sender, Some(&world));
        if !list.is_empty() {
            chosen = Some(list[fastrand::usize(..list.len())].1.clone());
        }
    }
    if let Some(l) = &chosen
        && let Some(w) = &l.world
    {
        world = w.clone();
    }
    if let Some(to) = s.overrides.get(&world)
        && horizon_bukkit::get_world(to).is_some()
    {
        world = to.clone();
    }
    let mut rw = match chosen {
        Some(l) => l,
        None => {
            let group = if s.groups_enabled {
                player.and_then(|p| {
                    s.groups
                        .iter()
                        .filter(|(g, _)| has_p(p, &format!("betterrtp.group.{g}")))
                        .flat_map(|(_, ws)| ws.iter())
                        .filter(|w| w.world.as_deref() == Some(world.as_str()))
                        .min_by_key(|w| w.priority)
                        .cloned()
                })
            } else {
                None
            };
            match group {
                Some(g) => g,
                None => match s.custom.get(&world) {
                    Some(c) => c.clone(),
                    None => {
                        let mut d = s.default.clone();
                        d.price = s.default_price(&world);
                        d
                    }
                },
            }
        }
    };
    if let Some(b) = biomes {
        rw.biomes = b;
    }
    let bukkit = horizon_bukkit::get_world(&world);
    if rw.use_world_border
        && let Some(w) = &bukkit
    {
        let border = w.get_world_border();
        let half = (border.get_size() / 2.0) as i32;
        if rw.max_radius > half {
            rw.max_radius = half;
        }
        let c = border.get_center();
        rw.center_x = c.x.floor() as i32;
        rw.center_z = c.z.floor() as i32;
    }
    if rw.max_radius <= rw.min_radius {
        rw.min_radius = s.default.min_radius;
        if rw.max_radius <= rw.min_radius {
            rw.min_radius = 0;
        }
    }
    let env = bukkit.as_ref().map_or("NORMAL", |w| w.get_environment());
    Target { kind: s.world_type(&world, env), world, s: rw }
}

// ---- Cooldowns ----

pub fn apply_cooldown(p: &Player) -> bool {
    settings().cooldown_enabled && !has_p(p, "betterrtp.bypass.cooldown")
}

/// Milliseconds left (-1 = locked for good).
pub fn cooldown_left(p: &Player, t: &Target) -> i64 {
    let s = settings();
    let Some(data) = db::get(&p.get_unique_id()) else { return 0 };
    let time = if s.per_world { data.worlds.get(&t.world).copied().unwrap_or(0) } else { data.global };
    if time == 0 {
        return 0;
    }
    if locked(&data, &s) {
        return -1;
    }
    (time / 1000 + t.s.cooldown - now_ms() / 1000) * 1000
}

fn locked(d: &db::PlayerData, s: &Settings) -> bool {
    s.lock_after > 0 && d.count >= s.lock_after
}

pub fn is_cooling_down(p: &Player, t: &Target) -> bool {
    if !apply_cooldown(p) {
        return false;
    }
    let s = settings();
    cooldown_left(p, t) > 0 || db::get(&p.get_unique_id()).is_some_and(|d| locked(&d, &s))
}

fn add_cooldown(p: &Player, world: &str) {
    let s = settings();
    if !s.cooldown_enabled {
        return;
    }
    let uuid = p.get_unique_id();
    let now = now_ms();
    let per_world = s.per_world;
    let d = db::update(&uuid, |d| {
        d.count += 1;
        d.global = now;
        if per_world {
            d.worlds.insert(world.to_owned(), now);
        }
    });
    if per_world {
        db::send(db::Job::SetCooldown(world.to_owned(), uuid.clone(), now));
    }
    db::send(db::Job::SetPlayer(uuid, d.count, d.global));
}

fn remove_cooldown(p: &Player, world: &str) {
    let s = settings();
    if !s.cooldown_enabled {
        return;
    }
    let uuid = p.get_unique_id();
    let per_world = s.per_world;
    let d = db::update(&uuid, |d| {
        if per_world {
            d.worlds.remove(world);
        } else {
            d.global = 0;
        }
    });
    if per_world {
        db::send(db::Job::RemoveCooldown(world.to_owned(), uuid.clone()));
    }
    db::send(db::Job::SetPlayer(uuid, d.count, d.global));
}

// ---- Economy / hunger ----

fn survivalish(p: &Player) -> bool {
    matches!(p.get_game_mode(), "SURVIVAL" | "ADVENTURE")
}

pub fn has_balance(p: &Player, t: &Target) -> bool {
    let s = settings();
    if !s.economy_enabled || t.s.price == 0 || has_p(p, "betterrtp.bypass.economy") || !horizon_kit::hooks::economy::enabled() {
        return true;
    }
    horizon_kit::hooks::economy::balance(p.get_name()) >= t.s.price as f64
}

pub fn has_hunger(p: &Player) -> bool {
    let h = settings().hunger;
    h == 0 || has_p(p, "betterrtp.bypass.hunger") || !survivalish(p) || p.get_food_level() >= h
}

/// Takes the hunger and the price: false (and nothing taken) when the player can't pay.
fn charge(sender: &CommandSender, p: &Player, t: &Target, info: Info) -> bool {
    let s = settings();
    let mut took = 0;
    if s.hunger != 0 && info.hunger && !has_p(p, "betterrtp.bypass.hunger") && survivalish(p) {
        if p.get_food_level() < s.hunger {
            send(sender, "Failed.Hunger", &[]);
            return false;
        }
        p.set_food_level(p.get_food_level() - s.hunger);
        took = s.hunger;
    }
    if s.economy_enabled && t.s.price != 0 && info.money && !has_p(p, "betterrtp.bypass.economy") && horizon_kit::hooks::economy::enabled() && !horizon_kit::hooks::economy::withdraw(p.get_name(), t.s.price as f64) {
        send(sender, "Failed.Price", &[("price", &t.s.price.to_string())]);
        if took > 0 {
            p.set_food_level(p.get_food_level() + took);
        }
        return false;
    }
    true
}

// ---- The request ----

/// HelperRTP.tp: checks, then the delay or the attempts.
pub fn tp(player: &Player, sender: &CommandSender, world: Option<String>, biomes: Option<Vec<String>>, kind: RtpType, location: Option<RtpWorld>, info: Info) {
    let s = settings();
    let world = world.unwrap_or_else(|| player.get_world().get_name().to_owned());
    let t = target_for(&s, Some(player), sender, &world, biomes, location.as_ref());
    let me = CommandSender::Player(player.clone());
    let same = sender.get_name() == player.get_name();
    let fail = |key: &str, pairs: &[(&str, &str)]| {
        send(&me, key, pairs);
        if !same {
            send(sender, key, pairs);
        }
    };
    let uuid = player.get_unique_id();
    if is_rtping(&uuid) {
        return fail("Already", &[]);
    }
    if same {
        let (ok, node) = world_permission(sender, &t.world);
        if !ok {
            return fail("NoPermission.World", &[("world", &t.world), ("permission", &node)]);
        }
    }
    if s.disabled.iter().any(|d| d == &t.world) {
        return fail("DisabledWorld", &[("world", &t.world)]);
    }
    if info.check_cooldown && is_cooling_down(player, &t) {
        let left = cooldown_left(player, &t);
        if left < 0 || db::get(&uuid).is_some_and(|d| locked(&d, &s)) {
            return fail("Locked", &[]);
        }
        let time = text::duration(left);
        return fail("Cooldown", &[("time", &time), ("cooldown", &time)]);
    }
    if info.money && !has_balance(player, &t) {
        return fail("Failed.Price", &[("price", &t.s.price.to_string())]);
    }
    if info.hunger && !has_hunger(player) {
        return fail("Failed.Hunger", &[]);
    }
    if horizon_bukkit::get_world(&t.world).is_none() {
        return fail("NotExist", &[("world", &t.world)]);
    }
    set_rtping(&uuid, true);
    let job = Job { uuid: uuid.clone(), sender: sender.clone(), t, attempts: 0, kind, info };
    let delay = s.delay;
    if info.delay && s.delay_enabled && delay > 0 && !has_p(player, "betterrtp.bypass.delay") {
        crate::effects::sound(player, &s.effects.sound_delay);
        let shown = crate::effects::title(player, "Delay", &player.get_location(), 0, delay);
        if shown {
            send(&me, "Delay", &[("time", &delay.to_string())]);
        }
        let id = horizon_bukkit::scheduler::run_task_later(delay as u64 * 20, {
            let mut job = Some(job);
            move || {
                let Some(job) = job.take() else { return };
                let had = DELAYS.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|m| m.remove(&job.uuid)).is_some();
                refresh_any_delay();
                if had && is_rtping(&job.uuid) {
                    next(job);
                }
            }
        });
        DELAYS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(uuid, id);
        ANY_DELAY.store(true, Ordering::Release);
    } else {
        crate::effects::sound(player, &s.effects.sound_delay);
        if crate::effects::title(player, "NoDelay", &player.get_location(), 0, 0) {
            send(sender, "Success.Teleport", &[]);
        }
        next(job);
    }
}

fn refresh_any_delay() {
    let empty = DELAYS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_none_or(HashMap::is_empty);
    ANY_DELAY.store(!empty, Ordering::Release);
}

/// The delay ended early (moved, hurt, left).
pub fn cancel_delay(p: &Player, notify: bool) {
    let uuid = p.get_unique_id();
    let Some(id) = DELAYS.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|m| m.remove(&uuid)) else { return };
    refresh_any_delay();
    horizon_bukkit::scheduler::cancel_task(id);
    if notify && crate::effects::title(p, "Cancelled", &p.get_location(), 0, 0) {
        send(&CommandSender::Player(p.clone()), "Moved", &[]);
    }
    remove_cooldown(p, p.get_world().get_name());
    set_rtping(&uuid, false);
}

pub fn in_delay(uuid: &str) -> bool {
    DELAYS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|m| m.contains_key(uuid))
}

pub fn forget(uuid: &str) {
    set_rtping(uuid, false);
    if let Some(id) = DELAYS.lock().unwrap_or_else(|e| e.into_inner()).as_mut().and_then(|m| m.remove(uuid)) {
        horizon_bukkit::scheduler::cancel_task(id);
    }
    refresh_any_delay();
}

pub fn is_invincible(uuid: &str) -> bool {
    let mut held = INVINCIBLE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(map) = held.as_mut() else { return false };
    let now = Instant::now();
    map.retain(|_, until| *until > now);
    ANY_INVINCIBLE.store(!map.is_empty(), Ordering::Release);
    map.contains_key(uuid)
}

struct Job {
    uuid: String,
    sender: CommandSender,
    t: Target,
    attempts: u32,
    kind: RtpType,
    info: Info,
}

fn online(uuid: &str) -> Option<Player> {
    horizon_bukkit::get_online_players().into_iter().find(|p| p.get_unique_id() == uuid)
}

/// A random spot in the area (RandomLocation.generateLocation).
fn random_spot(w: &RtpWorld) -> (i32, i32) {
    let min = w.min_radius.max(0);
    let max = (w.max_radius - min).max(1);
    let (x, z) = match w.shape {
        Shape::Square => {
            let a = fastrand::i32(0..max) + min;
            let b = fastrand::i32(0..max) + min;
            match fastrand::u8(0..4) {
                0 => (a, b),
                1 => (-a, -b),
                2 => (-a, b),
                _ => (a, -b),
            }
        }
        Shape::Circle => {
            let (minf, maxf) = (min as f64, max as f64);
            let area = std::f64::consts::PI * (maxf - minf) * (maxf + minf);
            let sub = area * fastrand::f64();
            let r = (sub / std::f64::consts::PI + minf * minf).sqrt();
            let theta = (r - r.floor()) * 2.0 * std::f64::consts::PI;
            ((r * theta.cos()) as i32, (r * theta.sin()) as i32)
        }
    };
    (x + w.center_x, z + w.center_z)
}

/// The next attempt: a spot, its chunk loaded off the tick, then checked.
fn next(mut job: Job) {
    let s = settings();
    if job.attempts >= s.max_attempts {
        return failed(job);
    }
    let Some(world) = horizon_bukkit::get_world(&job.t.world) else { return failed(job) };
    let (x, z) = random_spot(&job.t.s);
    job.attempts += 1;
    let (cx, cz) = (x >> 4, z >> 4);
    let job = Arc::new(Mutex::new(Some(job)));
    world.get_chunks_at_async(cx, cz, cx, cz, false, move || {
        if let Some(job) = job.lock().unwrap_or_else(|e| e.into_inner()).take() {
            attempt(job, x, z);
        }
    });
}

fn bad_block(s: &Settings, name: &str) -> bool {
    s.blacklist.iter().any(|b| b.eq_ignore_ascii_case(name))
}

fn biome_ok(world: &World, x: i32, y: i32, z: i32, biomes: &[String]) -> bool {
    if biomes.is_empty() {
        return true;
    }
    let here = world.get_block_at(x, y, z).get_biome().unwrap_or_default().to_ascii_uppercase();
    let here = here.trim_start_matches("MINECRAFT:");
    biomes.iter().any(|b| here.contains(&b.to_ascii_uppercase()))
}

/// RandomLocation.getSafeLocation.
fn safe_location(s: &Settings, t: &Target, world: &World, x: i32, z: i32) -> Option<Location> {
    match t.kind {
        WorldType::Normal => {
            let mut b = world.get_highest_block_at(x, z);
            let mut name = b.get_type().name();
            if name.ends_with("AIR") {
                b = world.get_block_at(x, b.get_y() - 1, z);
                name = b.get_type().name();
            }
            if !b.get_type().is_solid() && !bad_block(s, &name) {
                b = world.get_block_at(x, b.get_y() - 1, z);
                name = b.get_type().name();
            }
            let y = b.get_y();
            (y >= t.s.min_y && y <= t.s.max_y && !bad_block(s, &name) && biome_ok(world, x, y, z, &t.s.biomes)).then(|| Location::new(world.clone(), x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5, 0.0, 0.0))
        }
        WorldType::Nether => {
            let snap = world.get_chunk_at(x >> 4, z >> 4).get_chunk_snapshot()?;
            let kind = |y: i32| snap.get_block_type(x, y, z).name();
            let air = |n: &str| n.ends_with("AIR");
            for y in (t.s.min_y + 1)..t.s.max_y {
                let here = kind(y);
                let here_air = air(&here);
                if !here_air {
                    // A non-solid block (grass, fire...) that isn't blacklisted can be stood in.
                    if horizon_bukkit::Material::of(&here).is_solid() || bad_block(s, &here) {
                        continue;
                    }
                }
                let below = kind(y - 1);
                if !air(&below) && air(&kind(y + 1)) && !bad_block(s, &below) && biome_ok(world, x, y, z, &t.s.biomes) {
                    return Some(Location::new(world.clone(), x as f64 + 0.5, y as f64, z as f64 + 0.5, 0.0, 0.0));
                }
            }
            None
        }
    }
}

fn attempt(job: Job, x: i32, z: i32) {
    let Some(p) = online(&job.uuid) else {
        set_rtping(&job.uuid, false);
        return;
    };
    let s = settings();
    let Some(world) = horizon_bukkit::get_world(&job.t.world) else { return failed(job) };
    let Some(mut at) = safe_location(&s, &job.t, &world, x, z) else { return next(job) };
    if !charge(&job.sender, &p, &job.t, job.info) {
        if job.info.cooldown {
            remove_cooldown(&p, &job.t.world);
        }
        set_rtping(&job.uuid, false);
        return;
    }
    if job.info.cooldown && apply_cooldown(&p) {
        add_cooldown(&p, &job.t.world);
    }
    let here = p.get_location();
    at.yaw = here.yaw;
    at.pitch = here.pitch;
    // Loading.
    let me = CommandSender::Player(p.clone());
    if crate::effects::title(&p, "Loading", &here, 0, 0) && s.status_messages {
        send(&me, "Success.Loading", &[]);
    }
    let ok = p.teleport(&at);
    set_rtping(&job.uuid, false);
    if !ok {
        return failed(job);
    }
    after(&p, &job, &at);
}

/// RTPTeleport.afterTeleport + the success messages.
fn after(p: &Player, job: &Job, at: &Location) {
    let s = settings();
    crate::effects::sound(p, &s.effects.sound_success);
    crate::effects::particles(p, at);
    crate::effects::potions(p);
    if let Some(secs) = s.effects.invincible
        && secs > 0
    {
        INVINCIBLE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(job.uuid.clone(), Instant::now() + Duration::from_secs(secs));
        ANY_INVINCIBLE.store(true, Ordering::Release);
    }
    let (x, y, z) = (at.x.floor() as i64, at.y.floor() as i64, at.z.floor() as i64);
    let (xs, ys, zs, att) = (x.to_string(), y.to_string(), z.to_string(), job.attempts.to_string());
    let me = CommandSender::Player(p.clone());
    if crate::effects::title(p, "Teleport", at, job.attempts, 0) {
        if job.t.s.price != 0 && s.economy_enabled && !has_p(p, "betterrtp.bypass.economy") && job.info.money {
            send(&me, "Success.Paid", &[("x", &xs), ("y", &ys), ("z", &zs), ("price", &job.t.s.price.to_string()), ("attempts", &att)]);
        } else {
            send(&me, "Success.Bypass", &[("x", &xs), ("y", &ys), ("z", &zs), ("attempts", &att)]);
        }
    }
    if job.sender.get_name() != p.get_name() {
        send(&job.sender, "Other.Success", &[("player", p.get_name()), ("x", &xs), ("y", &ys), ("z", &zs), ("attempts", &att)]);
    }
    if job.kind == RtpType::Join && s.first_join.as_ref().is_some_and(|f| f.1) {
        p.set_respawn_location(at);
    }
    crate::log_rtp(p.get_name(), at, job.attempts);
}

fn failed(job: Job) {
    set_rtping(&job.uuid, false);
    let Some(p) = online(&job.uuid) else { return };
    remove_cooldown(&p, &job.t.world);
    let max = settings().max_attempts.to_string();
    if crate::effects::title(&p, "Failed", &p.get_location(), 0, 0) {
        if job.sender.get_name() == p.get_name() {
            send(&job.sender, "Failed.NotSafe", &[("attempts", &max)]);
        } else {
            send(&job.sender, "Other.NotSafe", &[("attempts", &max), ("player", p.get_name())]);
        }
    }
}
