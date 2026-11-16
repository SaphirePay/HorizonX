//! Land claims: a player claims a rectangle of land (all heights) with the claim shovel or `/claim create`, paid
//! for in claim blocks (one a square block) that come with playing. Inside it only the owner, the players they
//! trust and (by default) their team may build, open containers or use doors; explosions, fire, pistons from
//! outside and mobs that move blocks can't touch it. A claim's border shows as glowing blocks only its viewer
//! sees. Everything is managed from `/claim` (menus) or its sub-commands.

use crate::{conf, data, teams, ui};
use horizon_bukkit::entity::Entity;
use horizon_bukkit::fam_ev_world::BlockPistonEvent;
use horizon_bukkit::prelude::*;
use horizon_kit::store::File;
use horizon_kit::app;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const ACCESS: u8 = 1;
pub const CONTAINER: u8 = 2;
pub const BUILD: u8 = 3;
pub const MANAGER: u8 = 4;

const SECTION: &str = "Claims";
/// The trust key that stands for everyone.
const PUBLIC: &str = "*";

pub fn level_name(level: u8) -> &'static str {
    match level {
        ACCESS => "Access",
        CONTAINER => "Containers",
        BUILD => "Build",
        _ => "Manager",
    }
}

fn level_about(level: u8) -> &'static str {
    match level {
        ACCESS => "doors, buttons, levers, beds",
        CONTAINER => "+ chests, furnaces, animals",
        BUILD => "+ break and place blocks",
        _ => "+ trust other players",
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Trust {
    pub name: String,
    pub level: u8,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Claim {
    pub id: u64,
    pub world: String,
    pub x1: i32,
    pub z1: i32,
    pub x2: i32,
    pub z2: i32,
    /// The owner's uuid; empty for a server (admin) claim.
    pub owner: String,
    pub owner_name: String,
    pub name: String,
    pub trusted: BTreeMap<String, Trust>,
    pub pvp: bool,
    pub explosions: bool,
    pub created: u64,
}

impl Claim {
    pub fn admin(&self) -> bool {
        self.owner.is_empty()
    }
    pub fn width(&self) -> i32 {
        self.x2 - self.x1 + 1
    }
    pub fn length(&self) -> i32 {
        self.z2 - self.z1 + 1
    }
    pub fn area(&self) -> i64 {
        i64::from(self.width()) * i64::from(self.length())
    }
    pub fn contains(&self, world: &str, x: i32, z: i32) -> bool {
        self.world == world && x >= self.x1 && x <= self.x2 && z >= self.z1 && z <= self.z2
    }
    fn overlaps(&self, world: &str, x1: i32, z1: i32, x2: i32, z2: i32) -> bool {
        self.world == world && x1 <= self.x2 && x2 >= self.x1 && z1 <= self.z2 && z2 >= self.z1
    }
    pub fn title(&self) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else if self.admin() {
            "Server land".into()
        } else {
            format!("{}'s claim", self.owner_name)
        }
    }
    pub fn center(&self) -> (i32, i32) {
        ((self.x1 + self.x2) / 2, (self.z1 + self.z2) / 2)
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Db {
    next_id: u64,
    claims: Vec<Claim>,
}

/// A corner the player picked with the shovel; `resizing` when it was a corner of their own claim.
#[derive(Clone)]
struct Corner {
    world: String,
    x: i32,
    z: i32,
    resizing: Option<u64>,
}

#[derive(Default)]
struct State {
    db: Db,
    /// (world, chunk x, chunk z) -> claim ids touching that chunk.
    index: HashMap<(String, i32, i32), Vec<u64>>,
    corners: HashMap<String, Corner>,
    /// Blocks shown to a player (where, what is really there) and a number that tells a newer showing apart.
    shown: HashMap<String, (u64, Vec<(Location, String)>)>,
    /// The claim each player stood in at the last look.
    inside: HashMap<String, Option<u64>>,
    bypass: HashSet<String>,
}

impl State {
    fn rebuild(&mut self) {
        self.index.clear();
        for c in &self.db.claims {
            for cx in (c.x1 >> 4)..=(c.x2 >> 4) {
                for cz in (c.z1 >> 4)..=(c.z2 >> 4) {
                    self.index.entry((c.world.clone(), cx, cz)).or_default().push(c.id);
                }
            }
        }
    }
    fn at(&self, world: &str, x: i32, z: i32) -> Option<&Claim> {
        let ids = self.index.get(&(world.to_owned(), x >> 4, z >> 4))?;
        self.db.claims.iter().find(|c| ids.contains(&c.id) && c.contains(world, x, z))
    }
    fn by_id(&mut self, id: u64) -> Option<&mut Claim> {
        self.db.claims.iter_mut().find(|c| c.id == id)
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut held = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(held.get_or_insert_with(State::default))
}

fn file() -> &'static File<Db> {
    static FILE: OnceLock<File<Db>> = OnceLock::new();
    FILE.get_or_init(|| File::open(&app::folder().join("claims.json"), Duration::from_secs(3)))
}

/// After any change: the index made again and the claims handed to the saver.
fn changed(st: &mut State) {
    st.rebuild();
    let copy = st.db.clone();
    file().edit(|d| *d = copy);
}

pub fn save() {
    file().save_now();
}

pub fn load() {
    let db = file().get();
    with(|st| {
        st.db = db;
        st.rebuild();
    });
}

// ---- Who may do what ----

fn world_name(w: &World) -> String {
    w.get_name().to_owned()
}

/// The claim at a block, if any (a copy).
pub fn claim_at(world: &World, x: i32, z: i32) -> Option<Claim> {
    let w = world_name(world);
    with(|st| st.at(&w, x, z).cloned())
}

pub fn claim_at_loc(l: &Location) -> Option<Claim> {
    claim_at(l.get_world(), l.get_block_x(), l.get_block_z())
}

/// The level a player has in a claim (owner and bypass: everything).
pub fn level_of(p: &Player, c: &Claim) -> u8 {
    let uuid = p.get_unique_id();
    if c.owner == uuid || with(|st| st.bypass.contains(&uuid)) {
        return MANAGER + 1;
    }
    if c.admin() && p.has_permission("horizonsmp.claims.admin") {
        return MANAGER + 1;
    }
    let mut level = c.trusted.get(&uuid).map_or(0, |t| t.level);
    level = level.max(c.trusted.get(PUBLIC).map_or(0, |t| t.level));
    if level < BUILD && !c.admin() && conf::get().team_trusted && teams::same_team(&uuid, &c.owner) {
        level = BUILD;
    }
    level
}

pub fn may(p: &Player, c: &Claim, level: u8) -> bool {
    level_of(p, c) >= level
}

/// Whether the player may do `level` things at a block; tells them whose land it is when not.
fn allowed(p: &Player, world: &World, x: i32, z: i32, level: u8) -> bool {
    let Some(c) = claim_at(world, x, z) else { return true };
    if may(p, &c, level) {
        return true;
    }
    let what = match level {
        ACCESS => "use that",
        CONTAINER => "open that",
        _ => "build",
    };
    if c.admin() {
        ui::deny(p, &format!("You can't {what} here: this is protected server land."));
    } else {
        ui::deny(p, &format!("You can't {what} here: this land belongs to {}.", c.owner_name));
    }
    false
}

fn allowed_block(p: &Player, b: &Block, level: u8) -> bool {
    allowed(p, b.get_world(), b.get_x(), b.get_z(), level)
}

// ---- Claim blocks ----

pub fn total_blocks(uuid: &str) -> i64 {
    // Rank perk: `horizonsmp.claim-blocks` meta adds blocks on top.
    conf::get().starting_blocks + data::get(uuid).claim_blocks + conf::meta_int(uuid, "horizonsmp.claim-blocks").unwrap_or(0).max(0)
}

/// How many claims a player may have: config, or the rank's `horizonsmp.max-claims` meta.
pub fn max_claims(uuid: &str) -> usize {
    conf::meta_int(uuid, "horizonsmp.max-claims").map_or(conf::get().max_claims, |n| n.max(1) as usize)
}

pub fn used_blocks(uuid: &str) -> i64 {
    with(|st| st.db.claims.iter().filter(|c| c.owner == uuid).map(Claim::area).sum())
}

pub fn claims_of(uuid: &str) -> Vec<Claim> {
    with(|st| st.db.claims.iter().filter(|c| c.owner == uuid).cloned().collect())
}

pub fn claim_count() -> usize {
    with(|st| st.db.claims.len())
}

/// Paid out every minute online.
pub fn accrue(p: &Player) {
    let s = conf::get();
    let per_minute = (s.blocks_per_hour as f64 / 60.0).round() as i64;
    if per_minute <= 0 {
        return;
    }
    let uuid = p.get_unique_id();
    data::edit(&uuid, |d| {
        if d.claim_blocks < s.max_earned_blocks {
            d.claim_blocks = (d.claim_blocks + per_minute).min(s.max_earned_blocks);
        }
    });
}

// ---- Making, resizing, deleting ----

enum Made {
    Ok(Claim),
    Err(String),
}

fn try_claim(p: &Player, world: &str, ax: i32, az: i32, bx: i32, bz: i32, resizing: Option<u64>, admin: bool) -> Made {
    let s = conf::get();
    if !admin && !p.has_permission("horizonsmp.claims.create") {
        return Made::Err("You don't have permission to claim land (horizonsmp.claims.create).".into());
    }
    if s.claim_disabled_worlds.iter().any(|w| w.eq_ignore_ascii_case(world)) {
        return Made::Err("Claims are turned off in this world.".into());
    }
    let (x1, x2, z1, z2) = (ax.min(bx), ax.max(bx), az.min(bz), az.max(bz));
    let (w, l) = (x2 - x1 + 1, z2 - z1 + 1);
    let area = i64::from(w) * i64::from(l);
    if !admin && (w < s.min_width || l < s.min_width) {
        return Made::Err(format!("A claim must be at least {0}×{0} blocks (this one is {w}×{l}).", s.min_width));
    }
    if !admin && area < s.min_area {
        return Made::Err(format!("A claim must cover at least {} blocks.", s.min_area));
    }
    let uuid = p.get_unique_id();
    if let Some(c) = with(|st| st.db.claims.iter().find(|c| Some(c.id) != resizing && c.overlaps(world, x1, z1, x2, z2)).cloned()) {
        return Made::Err(format!("That overlaps {} at {}, {}.", if c.owner == uuid { "your other claim".to_owned() } else { c.title() }, c.x1, c.z1));
    }
    if !admin {
        let mine = claims_of(&uuid);
        let most = max_claims(&uuid);
        if resizing.is_none() && mine.len() >= most {
            return Made::Err(format!("You already have {most} claims, the most you can have."));
        }
        let old = resizing.and_then(|id| mine.iter().find(|c| c.id == id).map(Claim::area)).unwrap_or(0);
        let free = total_blocks(&uuid) - used_blocks(&uuid) + old;
        if area > free {
            return Made::Err(format!("That needs <white>{}</white> claim blocks and you have <white>{}</white>. Earn more by playing, or <click:run_command:/claim buyblocks><#22d3ee><u>buy some</u></#22d3ee></click>.", ui::num(area), ui::num(free)));
        }
    }
    let made = with(|st| {
        let claim = if let Some(id) = resizing {
            let c = st.by_id(id)?;
            c.x1 = x1;
            c.x2 = x2;
            c.z1 = z1;
            c.z2 = z2;
            c.clone()
        } else {
            st.db.next_id += 1;
            let mut trusted = BTreeMap::new();
            if admin {
                trusted.insert(PUBLIC.to_owned(), Trust { name: "Everyone".into(), level: ACCESS });
            }
            let c = Claim {
                id: st.db.next_id,
                world: world.to_owned(),
                x1,
                z1,
                x2,
                z2,
                owner: if admin { String::new() } else { uuid.clone() },
                owner_name: if admin { "Server".into() } else { p.get_name().to_owned() },
                name: String::new(),
                trusted,
                pvp: if admin { false } else { conf::get().claim_pvp_default },
                explosions: false,
                created: horizon_kit::time::now_ms(),
            };
            st.db.claims.push(c.clone());
            c
        };
        changed(st);
        Some(claim)
    });
    match made {
        Some(c) => Made::Ok(c),
        None => Made::Err("That claim is gone.".into()),
    }
}

/// Server land made without a player (spawn setup): everyone may use doors and buttons there. Overlapping
/// claims stop it.
pub fn server_claim(world: &str, ax: i32, az: i32, bx: i32, bz: i32) -> Result<Claim, String> {
    let (x1, x2, z1, z2) = (ax.min(bx), ax.max(bx), az.min(bz), az.max(bz));
    with(|st| {
        if let Some(c) = st.db.claims.iter().find(|c| c.overlaps(world, x1, z1, x2, z2)) {
            return Err(format!("it overlaps {} at {}, {}", c.title(), c.x1, c.z1));
        }
        st.db.next_id += 1;
        let mut trusted = BTreeMap::new();
        trusted.insert(PUBLIC.to_owned(), Trust { name: "Everyone".into(), level: ACCESS });
        let c = Claim { id: st.db.next_id, world: world.to_owned(), x1, z1, x2, z2, owner: String::new(), owner_name: "Server".into(), name: "Spawn".into(), trusted, pvp: false, explosions: false, created: horizon_kit::time::now_ms() };
        st.db.claims.push(c.clone());
        changed(st);
        Ok(c)
    })
}

fn delete(id: u64) -> Option<Claim> {
    with(|st| {
        let at = st.db.claims.iter().position(|c| c.id == id)?;
        let c = st.db.claims.remove(at);
        changed(st);
        Some(c)
    })
}

fn edit_claim(id: u64, f: impl FnOnce(&mut Claim)) -> Option<Claim> {
    with(|st| {
        let c = st.by_id(id)?;
        f(c);
        let out = c.clone();
        changed(st);
        Some(out)
    })
}

// ---- Showing borders ----

fn ground_y(world: &World, x: i32, z: i32, near: i32) -> i32 {
    let top = world.get_highest_block_y_at(x, z);
    if (top - near).abs() <= 12 { top } else { near - 1 }
}

fn block_key(world: &World, x: i32, y: i32, z: i32) -> String {
    let m = world.get_block_at(x, y, z).get_type();
    format!("minecraft:{}", m.name().to_ascii_lowercase())
}

/// Shows these claims' borders to the player for a while (their own in gold, others' in red, server land in
/// diamond).
pub fn show(p: &Player, claims: &[Claim]) {
    let world = p.get_world();
    let wname = world_name(&world);
    let at = p.get_location();
    let py = at.get_block_y();
    let uuid = p.get_unique_id();
    unshow(p);
    let mut changes: Vec<(Location, String)> = Vec::new();
    let mut real: Vec<(Location, String)> = Vec::new();
    for c in claims.iter().filter(|c| c.world == wname) {
        let (corner, edge) = if c.admin() {
            ("minecraft:diamond_block", "minecraft:sea_lantern")
        } else if c.owner == uuid {
            ("minecraft:gold_block", "minecraft:glowstone")
        } else if may(p, c, BUILD) {
            ("minecraft:emerald_block", "minecraft:lime_concrete")
        } else {
            ("minecraft:redstone_block", "minecraft:red_concrete")
        };
        let mut points: Vec<(i32, i32, &str)> = Vec::new();
        for (x, z) in [(c.x1, c.z1), (c.x1, c.z2), (c.x2, c.z1), (c.x2, c.z2)] {
            points.push((x, z, corner));
        }
        let step = ((c.width().max(c.length()) / 12).max(4)) as usize;
        for x in ((c.x1 + 2)..(c.x2 - 1)).step_by(step) {
            points.push((x, c.z1, edge));
            points.push((x, c.z2, edge));
        }
        for z in ((c.z1 + 2)..(c.z2 - 1)).step_by(step) {
            points.push((c.x1, z, edge));
            points.push((c.x2, z, edge));
        }
        for (x, z, block) in points {
            // Only what is near enough to see (the client ignores far chunks).
            if (x - at.get_block_x()).abs() > 120 || (z - at.get_block_z()).abs() > 120 {
                continue;
            }
            let y = ground_y(&world, x, z, py);
            let l = Location::new(world.clone(), f64::from(x), f64::from(y), f64::from(z), 0.0, 0.0);
            real.push((l.clone(), block_key(&world, x, y, z)));
            changes.push((l, block.to_owned()));
        }
    }
    if changes.is_empty() {
        return;
    }
    p.send_block_changes(&changes);
    let generation = horizon_kit::time::now_ms();
    with(|st| st.shown.insert(uuid.clone(), (generation, real)));
    let secs = conf::get().show_seconds;
    app::later(secs * 20, move || {
        if let Some(p) = horizon_bukkit::fam_ess::get_player_by_id(&uuid) {
            let same = with(|st| st.shown.get(&uuid).is_some_and(|(g, _)| *g == generation));
            if same {
                unshow(&p);
            }
        }
    });
}

/// Puts back what is really there.
pub fn unshow(p: &Player) {
    if let Some((_, real)) = with(|st| st.shown.remove(&p.get_unique_id())) {
        p.send_block_changes(&real);
    }
}

fn nearby(p: &Player, radius: i32) -> Vec<Claim> {
    let l = p.get_location();
    let w = world_name(l.get_world());
    let (x, z) = (l.get_block_x(), l.get_block_z());
    with(|st| st.db.claims.iter().filter(|c| c.world == w && c.x1 - radius <= x && c.x2 + radius >= x && c.z1 - radius <= z && c.z2 + radius >= z).cloned().collect())
}

// ---- The shovel ----

fn shovel_click(p: &Player, b: &Block) {
    let uuid = p.get_unique_id();
    let world = world_name(b.get_world());
    let (x, z) = (b.get_x(), b.get_z());
    let first = with(|st| st.corners.get(&uuid).cloned());
    if first.is_none() && claim_at(b.get_world(), x, z).is_none() && !p.has_permission("horizonsmp.claims.create") {
        ui::deny(p, "You don't have permission to claim land (horizonsmp.claims.create).");
        return;
    }
    match first {
        None => {
            // A corner of their own claim: they mean to resize it.
            let resizing = claim_at(b.get_world(), x, z).filter(|c| (c.owner == uuid || (c.admin() && p.has_permission("horizonsmp.claims.admin"))) && (x == c.x1 || x == c.x2) && (z == c.z1 || z == c.z2));
            if let Some(c) = &resizing {
                ui::say(p, SECTION, &format!("Resizing <white>{}</white>. Right-click where this corner should move to.", ui::esc(&c.title())));
            } else if let Some(c) = claim_at(b.get_world(), x, z) {
                // Inside a claim (not on a corner): show it.
                show(p, std::slice::from_ref(&c));
                if c.owner == uuid {
                    ui::say(p, SECTION, "This is your claim. Right-click one of its <#facc15>gold corners</#facc15> to resize it, or open <click:run_command:/claim><#22d3ee><u>/claim</u></#22d3ee></click> to manage it.");
                } else {
                    ui::say(p, SECTION, &format!("This land belongs to <white>{}</white>.", c.owner_name));
                }
                return;
            } else {
                ui::say(p, SECTION, &format!("First corner set at <white>{x}, {z}</white>. Now right-click the opposite corner."));
            }
            with(|st| st.corners.insert(uuid, Corner { world: world.clone(), x, z, resizing: resizing.map(|c| c.id) }));
            p.send_block_change(&Location::new(b.get_world().clone(), f64::from(x), f64::from(b.get_y()), f64::from(z), 0.0, 0.0), "minecraft:diamond_block");
            ui::sound(p, "BLOCK_NOTE_BLOCK_PLING:0.6:1.4");
        }
        Some(c1) => {
            with(|st| st.corners.remove(&uuid));
            if c1.world != world {
                ui::say(p, SECTION, "<#f87171>Both corners must be in the same world. Start again.");
                return;
            }
            let admin = c1.resizing.is_none() && with(|st| st.bypass.contains(&uuid)) && p.has_permission("horizonsmp.claims.admin") && p.is_sneaking();
            let made = if let Some(id) = c1.resizing {
                let Some(old) = with(|st| st.db.claims.iter().find(|c| c.id == id).cloned()) else { return };
                // The corner picked moves; the opposite one stays.
                let ox = if c1.x == old.x1 { old.x2 } else { old.x1 };
                let oz = if c1.z == old.z1 { old.z2 } else { old.z1 };
                try_claim(p, &world, ox, oz, x, z, Some(id), old.admin())
            } else {
                try_claim(p, &world, c1.x, c1.z, x, z, None, admin)
            };
            match made {
                Made::Ok(c) => {
                    let free = total_blocks(&uuid) - used_blocks(&uuid);
                    let verb = if c1.resizing.is_some() { "resized" } else { "created" };
                    ui::say(p, SECTION, &format!("<#4ade80>Claim {verb}!</#4ade80> {}×{} ({} blocks). You have <white>{}</white> claim blocks left.", c.width(), c.length(), ui::num(c.area()), ui::num(free)));
                    ui::title(p, &ui::grad("<bold>LAND CLAIMED</bold>"), &format!("<#a1a1aa>{}×{} • <white>/claim</white> to manage it", c.width(), c.length()));
                    ui::fanfare(p);
                    show(p, &[c]);
                }
                Made::Err(e) => {
                    ui::say(p, SECTION, &format!("<#f87171>{e}"));
                    ui::bad(p);
                }
            }
        }
    }
}

// ---- Material groups ----

fn is_container(m: &str) -> bool {
    matches!(m, "CHEST" | "TRAPPED_CHEST" | "BARREL" | "FURNACE" | "BLAST_FURNACE" | "SMOKER" | "HOPPER" | "DROPPER" | "DISPENSER" | "BREWING_STAND" | "CHISELED_BOOKSHELF" | "DECORATED_POT" | "CRAFTER" | "JUKEBOX" | "LECTERN" | "BEACON" | "CAMPFIRE" | "SOUL_CAMPFIRE" | "FLOWER_POT" | "COMPOSTER" | "BEEHIVE" | "BEE_NEST" | "RESPAWN_ANCHOR" | "CAKE" | "ANVIL" | "CHIPPED_ANVIL" | "DAMAGED_ANVIL" | "CAULDRON" | "WATER_CAULDRON" | "LAVA_CAULDRON" | "POWDER_SNOW_CAULDRON" | "VAULT" | "SHELF")
        || m.ends_with("SHULKER_BOX")
        || m.starts_with("POTTED_")
        || m.ends_with("_CANDLE_CAKE")
        || m.ends_with("_SHELF")
        || m.ends_with("COPPER_CHEST")
}

fn is_access(m: &str) -> bool {
    m.ends_with("_DOOR") || m.ends_with("_TRAPDOOR") || m.ends_with("_FENCE_GATE") || m.ends_with("_BUTTON") || m.ends_with("_BED") || matches!(m, "LEVER" | "BELL")
}

fn is_free(m: &str) -> bool {
    matches!(m, "CRAFTING_TABLE" | "ENDER_CHEST" | "ENCHANTING_TABLE" | "STONECUTTER" | "GRINDSTONE" | "LOOM" | "CARTOGRAPHY_TABLE" | "SMITHING_TABLE")
}

/// Blocks that change when right-clicked (a repeater's delay, a sign's text, berries picked).
fn is_switch(m: &str) -> bool {
    matches!(m, "REPEATER" | "COMPARATOR" | "NOTE_BLOCK" | "DAYLIGHT_DETECTOR" | "REDSTONE_WIRE" | "DRAGON_EGG" | "SWEET_BERRY_BUSH" | "CAVE_VINES" | "CAVE_VINES_PLANT" | "END_PORTAL_FRAME" | "COMMAND_BLOCK" | "STRUCTURE_BLOCK" | "TNT")
        || m.ends_with("_SIGN")
        || m.ends_with("CANDLE")
        || m.ends_with("_COPPER_BULB")
        || m == "COPPER_BULB"
}

/// Held items that change a block or the world when used on one.
fn item_needs_build(m: &str) -> bool {
    matches!(m, "FLINT_AND_STEEL" | "FIRE_CHARGE" | "BONE_MEAL" | "ARMOR_STAND" | "END_CRYSTAL" | "ITEM_FRAME" | "GLOW_ITEM_FRAME" | "PAINTING" | "LEAD" | "SHEARS" | "HONEYCOMB" | "INK_SAC" | "GLOW_INK_SAC" | "BRUSH" | "WIND_CHARGE")
        || m.ends_with("_SPAWN_EGG")
        || m.ends_with("_BOAT")
        || m.ends_with("_RAFT")
        || m.ends_with("MINECART")
        || m.ends_with("_AXE")
        || m.ends_with("_SHOVEL")
        || m.ends_with("_HOE")
        || m.ends_with("_DYE")
        || m.ends_with("BUCKET")
}

pub fn is_hostile(kind: &str) -> bool {
    let k = kind.trim_start_matches("minecraft:").to_ascii_uppercase();
    matches!(
        k.as_str(),
        "ZOMBIE" | "SKELETON" | "CREEPER" | "SPIDER" | "CAVE_SPIDER" | "ENDERMAN" | "WITCH" | "SLIME" | "MAGMA_CUBE" | "BLAZE" | "GHAST" | "PHANTOM" | "DROWNED" | "HUSK" | "STRAY" | "PILLAGER" | "VINDICATOR" | "EVOKER" | "RAVAGER" | "VEX" | "GUARDIAN" | "ELDER_GUARDIAN" | "SHULKER" | "SILVERFISH" | "ENDERMITE" | "HOGLIN" | "ZOGLIN" | "PIGLIN_BRUTE" | "WITHER_SKELETON" | "ZOMBIE_VILLAGER" | "ZOMBIFIED_PIGLIN" | "PIGLIN" | "WARDEN" | "BREEZE" | "BOGGED" | "CREAKING" | "WITHER" | "ENDER_DRAGON" | "ILLUSIONER" | "GIANT" | "PARCHED" | "SLIME_CUBE"
    )
}

fn kind(e: &Entity) -> String {
    e.get_type().trim_start_matches("minecraft:").to_ascii_uppercase()
}

// ---- Events ----

fn on_break(e: &mut BlockBreakEvent) {
    let p = e.get_player().clone();
    if !allowed_block(&p, e.get_block(), BUILD) {
        e.set_cancelled(true);
    }
}

fn on_place(e: &mut BlockPlaceEvent) {
    let p = e.get_player().clone();
    if !allowed_block(&p, e.get_block_placed(), BUILD) {
        e.set_cancelled(true);
    }
}

fn on_interact(e: &mut PlayerInteractEvent) {
    // A crate (or another plugin) already took the click. Air clicks come cancelled too: the stick still works.
    if e.is_cancelled() && e.get_action() != "RIGHT_CLICK_AIR" {
        return;
    }
    let p = e.get_player().clone();
    let action = e.get_action().to_owned();
    let s = conf::get();
    let held = e.get_item().map(|i| i.get_type().name().to_ascii_uppercase()).unwrap_or_default();
    // The shovel and the stick: off-hand clicks are the same click again.
    if e.get_hand().is_some_and(|h| h.eq_ignore_ascii_case("OFF_HAND")) && (held == s.claim_tool || held == s.inspect_tool) {
        return;
    }
    if s.claims && action == "RIGHT_CLICK_BLOCK" && held == s.claim_tool {
        if let Some(b) = e.get_clicked_block().cloned() {
            e.set_cancelled(true);
            shovel_click(&p, &b);
            return;
        }
    }
    if s.claims && (action == "RIGHT_CLICK_BLOCK" || action == "RIGHT_CLICK_AIR") && held == s.inspect_tool {
        if let Some(b) = e.get_clicked_block().cloned() {
            match claim_at(b.get_world(), b.get_x(), b.get_z()) {
                Some(c) => {
                    show(&p, std::slice::from_ref(&c));
                    ui::bar(&p, &format!("<#a855f7>⛨ <white>{}</white> <#71717a>• {}×{} • your access: <#22d3ee>{}", ui::esc(&c.title()), c.width(), c.length(), if may(&p, &c, MANAGER + 1) { "Owner" } else { match level_of(&p, &c) { 0 => "None", l => level_name(l) } }));
                }
                None => {
                    let near = nearby(&p, 64);
                    if !near.is_empty() {
                        show(&p, &near);
                    }
                    ui::bar(&p, &format!("<#4ade80>☘ <white>Wilderness</white> <#71717a>• free to claim{}", if near.is_empty() { String::new() } else { format!(" • {} claim(s) nearby", near.len()) }));
                }
            }
            e.set_cancelled(true);
            return;
        }
    }
    let Some(b) = e.get_clicked_block().cloned() else { return };
    let m = b.get_type().name().to_ascii_uppercase();
    let level = match action.as_str() {
        "PHYSICAL" => {
            if m == "FARMLAND" || m == "TURTLE_EGG" || m == "SNIFFER_EGG" {
                BUILD
            } else {
                return;
            }
        }
        "RIGHT_CLICK_BLOCK" => {
            if is_free(&m) {
                return;
            } else if is_container(&m) {
                CONTAINER
            } else if is_access(&m) {
                ACCESS
            } else if is_switch(&m) || item_needs_build(&held) {
                BUILD
            } else {
                return;
            }
        }
        "LEFT_CLICK_BLOCK" => {
            // Putting out fire, punching a dragon egg.
            if m == "DRAGON_EGG" || b.get_relative_face("UP").get_type().name().eq_ignore_ascii_case("FIRE") { BUILD } else { return }
        }
        _ => return,
    };
    if !allowed_block(&p, &b, level) {
        e.set_cancelled(true);
    }
}

fn on_bucket_empty(e: &mut PlayerBucketEmptyEvent) {
    let (Some(p), Some(b)) = (e.get_player(), e.get_block()) else { return };
    if !allowed_block(&p, &b, BUILD) {
        e.set_cancelled(true);
    }
}

fn on_bucket_fill(e: &mut PlayerBucketFillEvent) {
    let (Some(p), Some(b)) = (e.get_player(), e.get_block()) else { return };
    if !allowed_block(&p, &b, BUILD) {
        e.set_cancelled(true);
    }
}

fn on_interact_entity(e: &mut PlayerInteractEntityEvent) {
    let p = e.get_player().clone();
    let target = e.get_right_clicked();
    let k = kind(&target);
    if k == "PLAYER" || is_hostile(&k) {
        return;
    }
    let level = if k == "VILLAGER" || k == "WANDERING_TRADER" { ACCESS } else { CONTAINER };
    if let Some(l) = target.get_location()
        && !allowed(&p, l.get_world(), l.get_block_x(), l.get_block_z(), level)
    {
        e.set_cancelled(true);
    }
}

fn on_armor_stand(e: &mut PlayerArmorStandManipulateEvent) {
    let (Some(p), Some(stand)) = (e.get_player(), e.get_right_clicked()) else { return };
    if let Some(l) = stand.get_location()
        && !allowed(&p, l.get_world(), l.get_block_x(), l.get_block_z(), CONTAINER)
    {
        e.set_cancelled(true);
    }
}

/// A player hitting a claimed animal, villager, item frame or armour stand. (Players hitting players: combat.)
pub fn entity_hit(attacker: &Player, victim: &Entity) -> bool {
    let k = kind(victim);
    if is_hostile(&k) {
        return true;
    }
    let Some(l) = victim.get_location() else { return true };
    allowed(attacker, l.get_world(), l.get_block_x(), l.get_block_z(), CONTAINER)
}

fn on_hanging_break(e: &mut HangingBreakByEntityEvent) {
    let Some(remover) = e.get_remover() else { return };
    let Some(p) = horizon_bukkit::fam_ess::get_player_by_id(&remover.get_unique_id()) else { return };
    let Some(l) = e.get_damage_source().get_damage_location().cloned().or_else(|| remover.get_location()) else { return };
    if !allowed(&p, l.get_world(), l.get_block_x(), l.get_block_z(), BUILD) {
        e.set_cancelled(true);
    }
}

fn shielded(b: &Block) -> bool {
    claim_at(b.get_world(), b.get_x(), b.get_z()).is_some_and(|c| !c.explosions)
}

fn on_entity_explode(e: &mut EntityExplodeEvent) {
    e.block_list().retain(|b| !shielded(b));
}

fn on_block_explode(e: &mut BlockExplodeEvent) {
    e.block_list().retain(|b| !shielded(b));
}

fn on_burn(e: &mut BlockBurnEvent) {
    if let Some(b) = e.get_block()
        && claim_at(b.get_world(), b.get_x(), b.get_z()).is_some()
    {
        e.set_cancelled(true);
    }
}

fn on_ignite(e: &mut BlockIgniteEvent) {
    let Some(b) = e.get_block() else { return };
    if let Some(p) = e.get_player() {
        if !allowed_block(&p, &b, BUILD) {
            e.set_cancelled(true);
        }
        return;
    }
    if matches!(e.get_cause(), "SPREAD" | "LAVA" | "FIREBALL" | "LIGHTNING") && claim_at(b.get_world(), b.get_x(), b.get_z()).is_some() {
        e.set_cancelled(true);
    }
}

fn on_change_block(e: &mut EntityChangeBlockEvent) {
    let Some(b) = e.get_block() else { return };
    let Some(ent) = e.get_entity() else { return };
    let k = kind(&ent);
    if k == "PLAYER" || k == "FALLING_BLOCK" || k == "VILLAGER" || k == "SHEEP" {
        return;
    }
    if claim_at(b.get_world(), b.get_x(), b.get_z()).is_some() {
        e.set_cancelled(true);
    }
}

/// Owner of the land a block is on ("" = wilderness), for pistons.
fn owner_at(b: &Block) -> String {
    claim_at(b.get_world(), b.get_x(), b.get_z()).map(|c| format!("#{}", if c.admin() { "server".into() } else { c.owner })).unwrap_or_default()
}

fn piston_ok(piston: Option<Block>, moved: Vec<Block>) -> bool {
    let Some(piston) = piston else { return true };
    let home = owner_at(&piston);
    moved.iter().all(|b| {
        let o = owner_at(b);
        o.is_empty() || o == home
    })
}

fn on_piston_extend(e: &mut BlockPistonExtendEvent) {
    if !piston_ok(e.get_block(), e.get_blocks()) {
        e.set_cancelled(true);
    }
}

fn on_piston_retract(e: &mut BlockPistonRetractEvent) {
    if !piston_ok(e.get_block(), e.get_blocks()) {
        e.set_cancelled(true);
    }
}

/// Every half second: who walked into or out of a claim.
pub fn watch_borders() {
    for p in horizon_bukkit::get_online_players() {
        let l = p.get_location();
        let now = claim_at_loc(&l);
        let id = now.as_ref().map(|c| c.id);
        let uuid = p.get_unique_id();
        let before = with(|st| st.inside.insert(uuid.clone(), id));
        if before.is_none() || before == Some(id) {
            continue;
        }
        match now {
            Some(c) => {
                let pvp = if c.pvp { " <#f87171>⚔ PvP on" } else { "" };
                let who = if c.owner == uuid { "<#facc15>your land".to_owned() } else if c.admin() { "<#22d3ee>protected server land".to_owned() } else { format!("<#a1a1aa>owned by <white>{}", c.owner_name) };
                ui::bar(&p, &format!("<#a855f7>⛨ <white>{}</white> <#52525b>• {who}{pvp}", ui::esc(&c.title())));
            }
            None => ui::bar(&p, "<#4ade80>☘ <white>Wilderness</white> <#52525b>• <#a1a1aa>unclaimed land, PvP on"),
        }
    }
}

pub fn quit(uuid: &str) {
    with(|st| {
        st.inside.remove(uuid);
        st.corners.remove(uuid);
        st.shown.remove(uuid);
    });
}

pub fn register(plugin: Plugin) {
    plugin.register_event_with("HIGH", true, on_break);
    plugin.register_event_with("HIGH", true, on_place);
    plugin.register_event_with("HIGH", false, on_interact);
    plugin.register_event_with("HIGH", true, on_bucket_empty);
    plugin.register_event_with("HIGH", true, on_bucket_fill);
    plugin.register_event_with("HIGH", true, on_interact_entity);
    plugin.register_event_with("HIGH", true, on_armor_stand);
    plugin.register_event_with("HIGH", true, on_hanging_break);
    plugin.register_event_with("HIGH", true, on_entity_explode);
    plugin.register_event_with("HIGH", true, on_block_explode);
    plugin.register_event_with("HIGH", true, on_burn);
    plugin.register_event_with("HIGH", true, on_ignite);
    plugin.register_event_with("HIGH", true, on_change_block);
    plugin.register_event_with("HIGH", true, on_piston_extend);
    plugin.register_event_with("HIGH", true, on_piston_retract);
}

// ---- Menus ----

fn blocks_lore(uuid: &str) -> Vec<String> {
    let s = conf::get();
    let (total, used) = (total_blocks(uuid), used_blocks(uuid));
    vec![
        String::new(),
        format!("<#a1a1aa>Available  <white>{}", ui::num(total - used)),
        format!("<#a1a1aa>In use      <white>{}", ui::num(used)),
        format!("<#a1a1aa>Total        <white>{}", ui::num(total)),
        String::new(),
        format!("<#71717a>You earn <#22d3ee>{}</#22d3ee> an hour online.", ui::num(s.blocks_per_hour)),
        format!("<#71717a>{}", ui::meter(if total > 0 { used as f64 / total as f64 } else { 0.0 }, 20, "#a855f7", "#3f3f46")),
    ]
}

pub fn open_main(p: &Player) {
    if !crate::allowed(p, "horizonsmp.claims") {
        return;
    }
    let s = conf::get();
    let uuid = p.get_unique_id();
    let mut m = ui::frame(6, "⛨ Land Claims");
    let mut book = ui::icon("BOOK", &ui::grad("<bold>Your claim blocks"), &blocks_lore(&uuid));
    ui::glint(&mut book);
    m.item(4, book);
    let tool = ui::pretty(&s.claim_tool);
    m.button(
        20,
        ui::icon(&s.claim_tool, &format!("<#facc15><bold>How to claim"), &[
            String::new(),
            format!("<#a1a1aa>1. Hold a <white>{tool}</white>."),
            "<#a1a1aa>2. <white>Right-click</white> a block at one corner.".into(),
            "<#a1a1aa>3. <white>Right-click</white> the opposite corner.".into(),
            String::new(),
            "<#a1a1aa>Resize: right-click a <#facc15>gold corner</#facc15>".into(),
            "<#a1a1aa>then where it should go.".into(),
            format!("<#a1a1aa>Look: right-click with a <white>{}</white>.", ui::pretty(&s.inspect_tool)),
            String::new(),
            format!("<#22d3ee>▸ Click to get a {tool}"),
        ]),
        |c| {
            give_tool(&c.player);
            c.close();
        },
    );
    let r = s.default_radius;
    let side = r * 2 + 1;
    m.button(
        22,
        ui::icon("GRASS_BLOCK", "<#4ade80><bold>Claim the land around you", &[
            String::new(),
            format!("<#a1a1aa>A <white>{side}×{side}</white> square centred on you."),
            format!("<#a1a1aa>Costs <white>{}</white> claim blocks.", ui::num(i64::from(side) * i64::from(side))),
            String::new(),
            "<#22d3ee>▸ Click to claim".into(),
        ]),
        |c| {
            c.close();
            create_here(&c.player, conf::get().default_radius);
        },
    );
    m.button(
        24,
        ui::icon("EMERALD", "<#4ade80><bold>Buy claim blocks", &[
            String::new(),
            format!("<#a1a1aa>Price: <white>{}</white> a block", ui::money(s.block_price)),
            String::new(),
            "<#22d3ee>▸ Click to choose an amount".into(),
        ]),
        |c| {
            ui::click(&c.player);
            open_buy(&c.player);
        },
    );
    let mine = claims_of(&uuid);
    let slots: Vec<usize> = (28..35).chain(37..44).collect();
    if mine.is_empty() {
        m.item(31, ui::icon("MAP", "<#a1a1aa>No claims yet", &ui::lines(&["<#71717a>Claim some land and it shows here."])));
    }
    for (slot, c) in slots.iter().zip(mine.iter()) {
        let id = c.id;
        let lore = vec![
            format!("<#71717a>{} • {}, {}", c.world, c.center().0, c.center().1),
            String::new(),
            format!("<#a1a1aa>Size     <white>{}×{}", c.width(), c.length()),
            format!("<#a1a1aa>Trusted  <white>{}", c.trusted.keys().filter(|k| *k != PUBLIC).count()),
            format!("<#a1a1aa>PvP       {}", if c.pvp { "<#f87171>on" } else { "<#4ade80>off" }),
            String::new(),
            "<#22d3ee>▸ Click to manage".into(),
        ];
        m.button(*slot, ui::icon("FILLED_MAP", &format!("<#a855f7><bold>{}", ui::esc(&c.title())), &lore), move |c| {
            ui::click(&c.player);
            open_claim(&c.player, id);
        });
    }
    ui::back_button(&mut m, "the server menu", crate::menu::open);
    ui::close_button(&mut m);
    m.open(p);
}

fn give_tool(p: &Player) {
    if !crate::allowed(p, "horizonsmp.claims.tool") {
        return;
    }
    let s = conf::get();
    if p.get_inventory().contains(&Material::of(&s.claim_tool)) {
        ui::say(p, SECTION, &format!("You already have a {}. Right-click two corners with it.", ui::pretty(&s.claim_tool)));
        return;
    }
    static GIVEN: horizon_kit::sync::Cooldowns = horizon_kit::sync::Cooldowns::new();
    if !GIVEN.try_start(&p.get_unique_id(), 300_000) {
        ui::say(p, SECTION, "<#f87171>You got one a moment ago. Check your inventory.");
        return;
    }
    let mut tool = ItemStack::new(Material::of(&s.claim_tool), 1);
    horizon_kit::item::name(&mut tool, "<gradient:#facc15:#fb923c><bold>Claim Shovel</bold></gradient>");
    horizon_kit::item::lore(&mut tool, &ui::lines(&["<#a1a1aa>Right-click two corners to claim land.", "<#a1a1aa>Right-click a gold corner to resize."]));
    crate::give_item(p, &tool);
    ui::say(p, SECTION, "Here's a claim shovel. <white>Right-click</white> one corner, then the opposite corner.");
    ui::good(p);
}

pub fn create_here(p: &Player, radius: i32) {
    let l = p.get_location();
    let (x, z) = (l.get_block_x(), l.get_block_z());
    let r = radius.clamp(2, 200);
    match try_claim(p, l.get_world().get_name(), x - r, z - r, x + r, z + r, None, false) {
        Made::Ok(c) => {
            let uuid = p.get_unique_id();
            ui::say(p, SECTION, &format!("<#4ade80>Claim created!</#4ade80> {}×{} around you. <white>{}</white> claim blocks left. Manage it with <click:run_command:/claim><#22d3ee><u>/claim</u></#22d3ee></click>.", c.width(), c.length(), ui::num(total_blocks(&uuid) - used_blocks(&uuid))));
            ui::title(p, &ui::grad("<bold>LAND CLAIMED</bold>"), &format!("<#a1a1aa>{}×{} • only you and those you trust can build", c.width(), c.length()));
            ui::fanfare(p);
            show(p, &[c]);
        }
        Made::Err(e) => {
            ui::say(p, SECTION, &format!("<#f87171>{e}"));
            ui::bad(p);
        }
    }
}

fn open_buy(p: &Player) {
    let s = conf::get();
    let mut m = ui::frame(3, "⛨ Buy Claim Blocks");
    for (slot, n) in [(10usize, 100i64), (12, 500), (14, 1000), (16, 5000)] {
        let cost = s.block_price * n as f64;
        m.button(slot, ui::amount(ui::icon("GRASS_BLOCK", &format!("<#4ade80><bold>{} blocks", ui::num(n)), &[String::new(), format!("<#a1a1aa>Price <white>{}", ui::money(cost)), String::new(), "<#22d3ee>▸ Click to buy".into()]), (n / 100).clamp(1, 64) as i32), move |c| buy_blocks(&c.player, n));
    }
    ui::back_button(&mut m, "Land Claims", open_main);
    ui::close_button(&mut m);
    m.open(p);
}

fn buy_blocks(p: &Player, n: i64) {
    if !crate::allowed(p, "horizonsmp.claims.buyblocks") {
        return;
    }
    let s = conf::get();
    if s.block_price <= 0.0 {
        ui::say(p, SECTION, "<#f87171>Claim blocks can't be bought on this server.");
        return;
    }
    let cost = s.block_price * n as f64;
    if !horizon_kit::hooks::economy::withdraw(p.get_name(), cost) {
        ui::say(p, SECTION, &format!("<#f87171>You need {} for {} claim blocks.", ui::money(cost), ui::num(n)));
        ui::bad(p);
        return;
    }
    data::edit(&p.get_unique_id(), |d| d.claim_blocks += n);
    ui::say(p, SECTION, &format!("<#4ade80>Bought {} claim blocks</#4ade80> for {}.", ui::num(n), ui::money(cost)));
    ui::good(p);
    open_buy(p);
}

fn find(id: u64) -> Option<Claim> {
    with(|st| st.db.claims.iter().find(|c| c.id == id).cloned())
}

pub fn open_claim(p: &Player, id: u64) {
    let Some(c) = find(id) else { return open_main(p) };
    let manager = may(p, &c, MANAGER);
    let mut m = ui::frame(6, &format!("⛨ {}", c.title()));
    let mut info = ui::icon("FILLED_MAP", &format!("<#a855f7><bold>{}", ui::esc(&c.title())), &[
        String::new(),
        format!("<#a1a1aa>Owner    <white>{}", c.owner_name),
        format!("<#a1a1aa>World    <white>{}", c.world),
        format!("<#a1a1aa>From      <white>{}, {}", c.x1, c.z1),
        format!("<#a1a1aa>To          <white>{}, {}", c.x2, c.z2),
        format!("<#a1a1aa>Size       <white>{}×{} <#71717a>({} blocks)", c.width(), c.length(), ui::num(c.area())),
        String::new(),
        "<#71717a>Rename: /claim name <name>".into(),
    ]);
    ui::glint(&mut info);
    m.item(4, info);
    let trusted = c.trusted.iter().filter(|(k, _)| *k != PUBLIC).count();
    m.button(19, ui::icon("PLAYER_HEAD", "<#22d3ee><bold>Trusted players", &[String::new(), format!("<#a1a1aa><white>{trusted}</white> trusted"), "<#71717a>Add: /trust <player>".into(), String::new(), "<#22d3ee>▸ Click to view".into()]), move |cx| {
        ui::click(&cx.player);
        open_trust(&cx.player, id);
    });
    let toggle = |on: bool| if on { "<#4ade80>● On" } else { "<#f87171>● Off" };
    m.button(21, ui::icon(if c.pvp { "DIAMOND_SWORD" } else { "WOODEN_SWORD" }, "<#f87171><bold>PvP", &[String::new(), format!("<#a1a1aa>Status  {}", toggle(c.pvp)), "<#71717a>Players can fight inside the claim.".into(), String::new(), "<#22d3ee>▸ Click to switch".into()]), move |cx| {
        flip(&cx.player, id, "pvp");
    });
    m.button(23, ui::icon("TNT", "<#fb923c><bold>Explosions", &[String::new(), format!("<#a1a1aa>Status  {}", toggle(c.explosions)), "<#71717a>Creepers and TNT break blocks here.".into(), String::new(), "<#22d3ee>▸ Click to switch".into()]), move |cx| {
        flip(&cx.player, id, "explosions");
    });
    let public = c.trusted.get(PUBLIC).map_or(0, |t| t.level);
    m.button(25, ui::icon("OAK_DOOR", "<#facc15><bold>Public doors", &[String::new(), format!("<#a1a1aa>Status  {}", toggle(public >= ACCESS)), "<#71717a>Anyone may use doors, buttons".into(), "<#71717a>and levers here.".into(), String::new(), "<#22d3ee>▸ Click to switch".into()]), move |cx| {
        flip(&cx.player, id, "public");
    });
    m.button(31, ui::icon("SPYGLASS", "<#22d3ee><bold>Show the border", &ui::lines(&["", "<#a1a1aa>Glowing blocks mark the edges", "<#a1a1aa>for a few seconds.", "", "<#22d3ee>▸ Click to show"])), move |cx| {
        cx.close();
        if let Some(c) = find(id) {
            show(&cx.player, &[c]);
        }
    });
    if manager || c.owner == p.get_unique_id() {
        m.button(40, ui::icon("LAVA_BUCKET", "<#f87171><bold>Delete claim", &ui::lines(&["", "<#a1a1aa>Gives the land up and returns", "<#a1a1aa>its claim blocks to you.", "", "<#f87171>▸ Click to delete"])), move |cx| {
            ui::click(&cx.player);
            confirm(&cx.player, "Delete this claim?", move |p| {
                abandon(p, id);
            });
        });
    }
    ui::back_button(&mut m, "Land Claims", open_main);
    ui::close_button(&mut m);
    m.open(p);
}

fn flip(p: &Player, id: u64, what: &str) {
    let Some(c) = find(id) else { return };
    if !may(p, &c, MANAGER) {
        ui::deny(p, "Only the owner can change that.");
        return;
    }
    edit_claim(id, |c| match what {
        "pvp" => c.pvp = !c.pvp,
        "explosions" => c.explosions = !c.explosions,
        _ => {
            if c.trusted.get(PUBLIC).is_some_and(|t| t.level >= ACCESS) {
                c.trusted.remove(PUBLIC);
            } else {
                c.trusted.insert(PUBLIC.to_owned(), Trust { name: "Everyone".into(), level: ACCESS });
            }
        }
    });
    ui::click(p);
    open_claim(p, id);
}

fn open_trust(p: &Player, id: u64) {
    let Some(c) = find(id) else { return open_main(p) };
    let mut m = ui::frame(6, &format!("⛨ Trusted • {}", c.title()));
    m.item(4, ui::icon("WRITABLE_BOOK", "<#22d3ee><bold>Trust levels", &[
        String::new(),
        format!("<white>Access</white> <#71717a>{}", level_about(ACCESS)),
        format!("<white>Containers</white> <#71717a>{}", level_about(CONTAINER)),
        format!("<white>Build</white> <#71717a>{}", level_about(BUILD)),
        format!("<white>Manager</white> <#71717a>{}", level_about(MANAGER)),
        String::new(),
        "<#71717a>Add someone: /trust <player> [level]".into(),
        if conf::get().team_trusted { "<#71717a>Your team can always build here.".into() } else { String::new() },
    ]));
    let people: Vec<(String, Trust)> = c.trusted.iter().filter(|(k, _)| *k != PUBLIC).map(|(k, v)| (k.clone(), v.clone())).collect();
    if people.is_empty() {
        m.item(22, ui::icon("GRAY_DYE", "<#a1a1aa>Nobody trusted yet", &ui::lines(&["<#71717a>/trust <player> to add someone."])));
    }
    for (slot, (uuid, t)) in ui::inner(6).into_iter().zip(people) {
        let lore = vec![String::new(), format!("<#a1a1aa>Level  <white>{}", level_name(t.level)), format!("<#71717a>{}", level_about(t.level)), String::new(), "<#22d3ee>▸ Left-click: change level".into(), "<#f87171>▸ Right-click: remove".into()];
        let name = t.name.clone();
        m.button(slot, ui::head(&t.name, &format!("<white><bold>{}", t.name), &lore), move |cx| {
            let Some(c) = find(id) else { return };
            if !may(&cx.player, &c, MANAGER) {
                ui::deny(&cx.player, "Only the owner can change that.");
                return;
            }
            let uuid = uuid.clone();
            if cx.click.is_right() {
                edit_claim(id, |c| {
                    c.trusted.remove(&uuid);
                });
                ui::say(&cx.player, SECTION, &format!("<white>{name}</white> is no longer trusted here."));
            } else {
                edit_claim(id, |c| {
                    if let Some(t) = c.trusted.get_mut(&uuid) {
                        t.level = if t.level >= MANAGER { ACCESS } else { t.level + 1 };
                    }
                });
            }
            ui::click(&cx.player);
            open_trust(&cx.player, id);
        });
    }
    m.button(45, ui::icon("ARROW", "<#a1a1aa>← <white>Back", &[format!("<#71717a>To {}", c.title())]), move |cx| {
        ui::click(&cx.player);
        open_claim(&cx.player, id);
    });
    ui::close_button(&mut m);
    m.open(p);
}

/// A yes/no menu (the kit's).
pub fn confirm(p: &Player, question: &str, yes: impl Fn(&Player) + Send + Sync + 'static) {
    horizon_kit::gui::confirm(p, question, yes);
}

fn abandon(p: &Player, id: u64) {
    let Some(c) = find(id) else { return };
    if !(c.owner == p.get_unique_id() || (c.admin() && p.has_permission("horizonsmp.claims.admin")) || with(|st| st.bypass.contains(&p.get_unique_id()))) {
        ui::deny(p, "Only the owner can delete this claim.");
        return;
    }
    unshow(p);
    delete(id);
    ui::say(p, SECTION, &format!("Claim deleted. <white>{}</white> claim blocks are free again.", ui::num(c.area())));
    ui::sound(p, "BLOCK_GLASS_BREAK:0.6:1.0");
}

// ---- Commands ----

fn here(p: &Player) -> Option<Claim> {
    claim_at_loc(&p.get_location())
}

fn level_arg(s: Option<&&str>) -> Option<u8> {
    match s.map(|s| s.to_ascii_lowercase()).as_deref() {
        None | Some("build" | "b") => Some(BUILD),
        Some("access" | "a" | "doors") => Some(ACCESS),
        Some("container" | "containers" | "c" | "chests") => Some(CONTAINER),
        Some("manager" | "manage" | "m" | "permission") => Some(MANAGER),
        _ => None,
    }
}

fn trust_cmd(p: &Player, args: &[&str]) {
    if !crate::allowed(p, "horizonsmp.claims.trust") {
        return;
    }
    let Some(name) = args.first() else {
        ui::say(p, SECTION, "Usage: <white>/trust <player> [access|container|build|manager]");
        return;
    };
    let Some(level) = level_arg(args.get(1)) else {
        ui::say(p, SECTION, "<#f87171>The level is one of: access, container, build, manager.");
        return;
    };
    let public = *name == "public" || *name == "*" || name.eq_ignore_ascii_case("everyone");
    let (uuid, real) = if public {
        (PUBLIC.to_owned(), "Everyone".to_owned())
    } else {
        match data::uuid_of(name) {
            Some(x) => x,
            None => {
                ui::say(p, SECTION, &format!("<#f87171>No player named {} has played here.", ui::esc(name)));
                return;
            }
        }
    };
    if uuid == p.get_unique_id() {
        ui::say(p, SECTION, "<#f87171>You always have every right in your own claims.");
        return;
    }
    let targets: Vec<Claim> = match here(p) {
        Some(c) => {
            if !may(p, &c, MANAGER) {
                ui::deny(p, "You need Manager trust here to trust others.");
                return;
            }
            vec![c]
        }
        None => claims_of(&p.get_unique_id()),
    };
    if targets.is_empty() {
        ui::say(p, SECTION, "<#f87171>Stand in your claim (or own one) to trust someone.");
        return;
    }
    for c in &targets {
        let (u, n) = (uuid.clone(), real.clone());
        edit_claim(c.id, |c| {
            c.trusted.insert(u, Trust { name: n, level });
        });
    }
    let scope = if targets.len() == 1 { format!("in <white>{}</white>", ui::esc(&targets[0].title())) } else { format!("in all <white>{}</white> of your claims", targets.len()) };
    ui::say(p, SECTION, &format!("<#4ade80>{real}</#4ade80> now has <white>{}</white> trust {scope}.", level_name(level)));
    ui::good(p);
    if let Some(t) = horizon_bukkit::fam_ess::get_player_by_id(&uuid) {
        ui::say(&t, SECTION, &format!("<white>{}</white> gave you <white>{}</white> trust {scope}.", p.get_name(), level_name(level)));
    }
}

fn untrust_cmd(p: &Player, args: &[&str]) {
    if !crate::allowed(p, "horizonsmp.claims.trust") {
        return;
    }
    let Some(name) = args.first() else {
        ui::say(p, SECTION, "Usage: <white>/untrust <player>");
        return;
    };
    let public = *name == "public" || *name == "*";
    let uuid = if public { Some(PUBLIC.to_owned()) } else { data::uuid_of(name).map(|x| x.0) };
    let Some(uuid) = uuid else {
        ui::say(p, SECTION, &format!("<#f87171>No player named {}.", ui::esc(name)));
        return;
    };
    let targets: Vec<Claim> = match here(p) {
        Some(c) if may(p, &c, MANAGER) => vec![c],
        Some(_) => {
            ui::deny(p, "You need Manager trust here.");
            return;
        }
        None => claims_of(&p.get_unique_id()),
    };
    let mut n = 0;
    for c in &targets {
        if c.trusted.contains_key(&uuid) {
            edit_claim(c.id, |c| {
                c.trusted.remove(&uuid);
            });
            n += 1;
        }
    }
    if n == 0 {
        ui::say(p, SECTION, &format!("<white>{}</white> wasn't trusted there.", ui::esc(name)));
    } else {
        ui::say(p, SECTION, &format!("<white>{}</white> is no longer trusted in {n} claim(s).", ui::esc(name)));
    }
}

fn info_cmd(p: &Player) {
    match here(p) {
        Some(c) => {
            show(p, std::slice::from_ref(&c));
            let trusted: Vec<String> = c.trusted.values().map(|t| format!("{} <#71717a>({})</#71717a>", t.name, level_name(t.level))).collect();
            ui::raw(p, &format!(
                "\n {}\n <#a1a1aa>Owner <white>{}</white>   <#a1a1aa>Size <white>{}×{}</white>   <#a1a1aa>PvP {}\n <#a1a1aa>From <white>{}, {}</white> to <white>{}, {}</white>\n <#a1a1aa>Trusted <white>{}\n",
                ui::grad(&format!("<bold>{}</bold>", ui::esc(&c.title()))),
                c.owner_name,
                c.width(),
                c.length(),
                if c.pvp { "<#f87171>on" } else { "<#4ade80>off" },
                c.x1,
                c.z1,
                c.x2,
                c.z2,
                if trusted.is_empty() { "nobody".to_owned() } else { trusted.join("<#52525b>, <white>") }
            ));
        }
        None => ui::say(p, SECTION, "You're in the <#4ade80>wilderness</#4ade80>: nobody owns this land. <click:run_command:/claim create><#22d3ee><u>Claim it</u></#22d3ee></click>."),
    }
}

pub fn claim_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    let Some(p) = sender.as_player() else {
        if args.first() == Some(&"giveblocks") {
            give_blocks(sender, &args[1..]);
        } else {
            ui::tell(sender, SECTION, "Only players can use claims. Console: /claim giveblocks <player> <n>");
        }
        return true;
    };
    let p = p.clone();
    if !conf::get().claims {
        ui::say(&p, SECTION, "<#f87171>Claims are turned off.");
        return true;
    }
    let sub = args.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    let rest = if args.is_empty() { &[][..] } else { &args[1..] };
    match sub.as_str() {
        "" | "menu" | "gui" => open_main(&p),
        "create" | "new" | "here" | "auto" => {
            let r = rest.first().and_then(|s| s.parse::<i32>().ok()).unwrap_or(conf::get().default_radius);
            create_here(&p, r);
        }
        "info" | "i" => info_cmd(&p),
        "list" | "ls" => {
            let mine = claims_of(&p.get_unique_id());
            if mine.is_empty() {
                ui::say(&p, SECTION, "You have no claims yet. <click:run_command:/claim create><#22d3ee><u>Claim here</u></#22d3ee></click>.");
            } else {
                ui::say(&p, SECTION, &format!("Your <white>{}</white> claims:", mine.len()));
                for c in &mine {
                    ui::raw(&p, &format!(" <#52525b>•</#52525b> <white>{}</white> <#71717a>{} at {}, {} • {}×{}", ui::esc(&c.title()), c.world, c.center().0, c.center().1, c.width(), c.length()));
                }
            }
        }
        "trust" => trust_cmd(&p, rest),
        "untrust" => untrust_cmd(&p, rest),
        "trustlist" | "trusted" => match here(&p) {
            Some(c) => open_trust(&p, c.id),
            None => ui::say(&p, SECTION, "Stand in a claim to see who is trusted there."),
        },
        "delete" | "abandon" | "remove" | "unclaim" => match here(&p) {
            Some(c) if c.owner == p.get_unique_id() || may(&p, &c, MANAGER + 1) => {
                let id = c.id;
                confirm(&p, "Delete this claim?", move |p| abandon(p, id));
            }
            Some(_) => ui::deny(&p, "This isn't your claim."),
            None => ui::say(&p, SECTION, "Stand in the claim you want to delete."),
        },
        "deleteall" | "abandonall" => {
            let uuid = p.get_unique_id();
            confirm(&p, "Delete ALL your claims?", move |p| {
                for c in claims_of(&uuid) {
                    delete(c.id);
                }
                ui::say(p, SECTION, "All your claims are deleted.");
            });
        }
        "name" | "rename" => match here(&p) {
            Some(c) if may(&p, &c, MANAGER) => {
                let name: String = rest.join(" ").chars().filter(|ch| !ch.is_control()).take(24).collect();
                edit_claim(c.id, |c| c.name = name.clone());
                ui::say(&p, SECTION, &format!("Claim renamed to <white>{}</white>.", ui::esc(if name.is_empty() { "(default)" } else { &name })));
            }
            _ => ui::say(&p, SECTION, "Stand in your claim to rename it."),
        },
        "flag" | "pvp" | "explosions" => {
            let what = if sub == "flag" { rest.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default() } else { sub.clone() };
            match here(&p) {
                Some(c) if may(&p, &c, MANAGER) && (what == "pvp" || what == "explosions" || what == "public") => {
                    flip(&p, c.id, &what);
                }
                Some(_) => ui::say(&p, SECTION, "Usage: <white>/claim flag <pvp|explosions|public></white> in your claim."),
                None => ui::say(&p, SECTION, "Stand in your claim first."),
            }
        }
        "buyblocks" | "buy" => match rest.first().and_then(|s| s.parse::<i64>().ok()).filter(|n| *n > 0) {
            Some(n) => buy_blocks(&p, n),
            None => open_buy(&p),
        },
        "blocks" => ui::raw(&p, &blocks_lore(&p.get_unique_id()).join("\n")),
        "show" | "view" | "border" => {
            let near = nearby(&p, 96);
            if near.is_empty() {
                ui::say(&p, SECTION, "There are no claims near you.");
            } else {
                show(&p, &near);
                ui::say(&p, SECTION, &format!("Showing <white>{}</white> claim(s) near you.", near.len()));
            }
        }
        "tool" | "shovel" => give_tool(&p),
        "bypass" | "ignore" if p.has_permission("horizonsmp.claims.admin") => {
            let uuid = p.get_unique_id();
            let on = with(|st| if st.bypass.remove(&uuid) { false } else { st.bypass.insert(uuid.clone()) });
            ui::say(&p, SECTION, if on { "<#facc15>Bypass on:</#facc15> you can build in every claim. Sneak while placing the second shovel corner to make server land." } else { "Bypass off." });
        }
        "admin" | "server" if p.has_permission("horizonsmp.claims.admin") => {
            let r = rest.first().and_then(|s| s.parse::<i32>().ok()).unwrap_or(40);
            let l = p.get_location();
            let (x, z) = (l.get_block_x(), l.get_block_z());
            match try_claim(&p, l.get_world().get_name(), x - r, z - r, x + r, z + r, None, true) {
                Made::Ok(c) => {
                    ui::say(&p, SECTION, &format!("<#4ade80>Server land made:</#4ade80> {}×{} around you. Everyone may use doors and buttons there.", c.width(), c.length()));
                    show(&p, &[c]);
                }
                Made::Err(e) => ui::say(&p, SECTION, &format!("<#f87171>{e}")),
            }
        }
        "giveblocks" if p.has_permission("horizonsmp.claims.admin") => give_blocks(sender, rest),
        _ => help(&p),
    }
    true
}

fn give_blocks(sender: &CommandSender, args: &[&str]) {
    let (Some(name), Some(n)) = (args.first(), args.get(1).and_then(|s| s.parse::<i64>().ok())) else {
        ui::tell(sender, SECTION, "Usage: /claim giveblocks <player> <amount>");
        return;
    };
    let Some((uuid, real)) = data::uuid_of(name) else {
        ui::tell(sender, SECTION, "No such player.");
        return;
    };
    data::edit(&uuid, |d| d.claim_blocks += n);
    ui::tell(sender, SECTION, &format!("Gave {real} {} claim blocks.", ui::num(n)));
}

fn help(p: &Player) {
    let admin = p.has_permission("horizonsmp.claims.admin");
    let mut l = vec![
        String::new(),
        format!(" {}  <#71717a>protect your builds", ui::grad("<bold>LAND CLAIMS</bold>")),
        " <#22d3ee>/claim</#22d3ee> <#71717a>open the claims menu".to_owned(),
        " <#22d3ee>/claim create [radius]</#22d3ee> <#71717a>claim the land around you".to_owned(),
        " <#22d3ee>/claim info</#22d3ee> <#71717a>who owns the land you stand on".to_owned(),
        " <#22d3ee>/trust <player> [level]</#22d3ee> <#71717a>let someone use your claim".to_owned(),
        " <#22d3ee>/untrust <player></#22d3ee> <#71717a>take it back".to_owned(),
        " <#22d3ee>/claim name <name></#22d3ee> <#71717a>rename the claim you stand in".to_owned(),
        " <#22d3ee>/claim flag <pvp|explosions|public></#22d3ee> <#71717a>switch a setting".to_owned(),
        " <#22d3ee>/claim buyblocks <n></#22d3ee> <#71717a>buy more claim blocks".to_owned(),
        " <#22d3ee>/claim show</#22d3ee> <#71717a>see the claims around you".to_owned(),
        " <#22d3ee>/unclaim</#22d3ee> <#71717a>delete the claim you stand in".to_owned(),
    ];
    if admin {
        l.push(" <#facc15>/claim bypass • /claim admin [radius] • /claim giveblocks".to_owned());
    }
    l.push(String::new());
    ui::raw(p, &l.join("\n"));
}

pub fn complete(args: &[&str]) -> Vec<String> {
    let subs = ["create", "info", "list", "trust", "untrust", "trustlist", "delete", "deleteall", "name", "flag", "buyblocks", "blocks", "show", "tool", "menu", "bypass", "admin", "giveblocks"];
    match args {
        [one] => subs.iter().filter(|s| s.starts_with(&one.to_ascii_lowercase())).map(|s| (*s).to_owned()).collect(),
        [sub, two] if matches!(*sub, "trust" | "untrust" | "giveblocks") => horizon_bukkit::get_online_players().into_iter().map(|p| p.get_name().to_owned()).filter(|n| n.to_ascii_lowercase().starts_with(&two.to_ascii_lowercase())).collect(),
        [sub, _] if *sub == "flag" => vec!["pvp".into(), "explosions".into(), "public".into()],
        [sub, _, _] if *sub == "trust" => vec!["access".into(), "container".into(), "build".into(), "manager".into()],
        _ => Vec::new(),
    }
}

pub fn trust_complete(args: &[&str]) -> Vec<String> {
    match args {
        [one] => horizon_bukkit::get_online_players().into_iter().map(|p| p.get_name().to_owned()).filter(|n| n.to_ascii_lowercase().starts_with(&one.to_ascii_lowercase())).collect(),
        [_, _] => vec!["access".into(), "container".into(), "build".into(), "manager".into()],
        _ => Vec::new(),
    }
}

pub fn trust_entry(sender: &CommandSender, args: &[&str]) -> bool {
    if let Some(p) = sender.as_player() {
        trust_cmd(&p.clone(), args);
    }
    true
}

pub fn untrust_entry(sender: &CommandSender, args: &[&str]) -> bool {
    if let Some(p) = sender.as_player() {
        untrust_cmd(&p.clone(), args);
    }
    true
}

pub fn unclaim_entry(sender: &CommandSender, _args: &[&str]) -> bool {
    claim_cmd(sender, &["delete"])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rectangles() {
        let c = Claim { world: "w".into(), x1: 0, z1: 0, x2: 9, z2: 4, ..Default::default() };
        assert_eq!(c.area(), 50);
        assert!(c.contains("w", 9, 4));
        assert!(!c.contains("w", 10, 4));
        assert!(c.overlaps("w", 9, 4, 20, 20));
        assert!(!c.overlaps("w", 10, 0, 20, 20));
        assert!(is_container("OAK_SHELF") && is_container("RED_SHULKER_BOX") && !is_container("STONE"));
        assert!(is_access("OAK_DOOR") && is_access("STONE_BUTTON"));
        assert!(item_needs_build("COW_SPAWN_EGG") && item_needs_build("WATER_BUCKET") && !item_needs_build("BREAD"));
        assert!(is_hostile("minecraft:zombie") && !is_hostile("COW"));
    }
}
