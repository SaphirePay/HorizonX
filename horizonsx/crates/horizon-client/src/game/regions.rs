//! HorizonsX: Folia's region threads.
//!
//! Folia splits a level into regions (groups of nearby loaded chunks, kept apart by a merge radius) and
//! ticks each region on its own thread, with its own data (`RegionizedWorldData`: its chunks, entities,
//! block and fluid ticks, block entities and players). Only the global parts (time, weather, the player
//! list, console commands) stay on one thread, its "global region".
//!
//! Here a region's data is lent into a region game: a `Game` of its own on a worker thread, built from the
//! same resources and world settings as the server's game ([`Game::region_worker`]). The server's game keeps
//! the global parts, runs the region games at once, and takes their data back after
//! ([`Game::lend_region`], [`Game::take_region_back`]).
use super::{Game, Mob};
use horizon_client::Result;
use horizon_core::position::ChunkPos;
use std::collections::HashMap;

/// Chunks per side of a region section (Folia's grid: 2^4 = 16; HorizonsX's block-tick regions found 4×4
/// sections split players 512 blocks apart, where Folia's made one region).
pub(super) const SECTION_SHIFT: i32 = 2;
/// Active sections this many sections apart or closer are one region (Folia's merge radius).
pub(super) const MERGE_RADIUS: i32 = 2;
/// Entity ids a region game takes from the shared counter at a time, and how few it may have left before a
/// tick before it takes more (no tick hands out anywhere near that many).
const ID_BLOCK: u32 = 1 << 20;
const ID_REFILL: u32 = 1 << 15;

/// Which region each loaded chunk belongs to, Folia's way: sections of `2^SECTION_SHIFT` chunks; a section
/// with a ticking chunk (or a player) is active; active sections `MERGE_RADIUS` apart or closer are one
/// region; a region owns its active sections and the sections around them, so two regions' chunks never
/// meet and a region's ticks find the chunks next to theirs (a block tick reaching past them finds the chunk
/// unloaded, Folia's rule for chunks a region doesn't own).
pub(super) struct Regions {
    /// The region of each owned section.
    owned: horizon_client::fast_hash::FxHashMap<(i32, i32), usize>,
    pub count: usize,
    /// Each region's first section (its smallest): what names a region from one tick to the next.
    pub firsts: Vec<(i32, i32)>,
    /// `owned` as a flat table over the sections' bounds, when that is small enough: every split of the
    /// level asks for each chunk's and entity's region (tens of thousands a tick), one array read each.
    grid: Option<Grid>,
}

/// The owned sections' regions in one array over their bounding box (`u16::MAX`: no region).
struct Grid {
    x0: i32,
    z0: i32,
    w: i32,
    h: i32,
    cells: Vec<u16>,
}

impl Grid {
    /// Sections in the table at most (4 M cells, 8 MB): players spread wider than that keep the map.
    const MAX_CELLS: i64 = 1 << 22;
    fn of(owned: &horizon_client::fast_hash::FxHashMap<(i32, i32), usize>) -> Option<Self> {
        if owned.is_empty() {
            return None;
        }
        let (mut x0, mut z0, mut x1, mut z1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for &(x, z) in owned.keys() {
            x0 = x0.min(x);
            z0 = z0.min(z);
            x1 = x1.max(x);
            z1 = z1.max(z);
        }
        let (w, h) = (i64::from(x1) - i64::from(x0) + 1, i64::from(z1) - i64::from(z0) + 1);
        if w * h > Self::MAX_CELLS || owned.values().any(|&r| r >= usize::from(u16::MAX)) {
            return None;
        }
        let (w, h) = (w as i32, h as i32);
        let mut cells = vec![u16::MAX; (w * h) as usize];
        for (&(x, z), &r) in owned {
            cells[((z - z0) * w + (x - x0)) as usize] = r as u16;
        }
        Some(Self { x0, z0, w, h, cells })
    }
    #[inline]
    fn get(&self, x: i32, z: i32) -> Option<usize> {
        let (dx, dz) = (x.wrapping_sub(self.x0), z.wrapping_sub(self.z0));
        if dx < 0 || dz < 0 || dx >= self.w || dz >= self.h {
            return None;
        }
        match self.cells[(dz * self.w + dx) as usize] {
            u16::MAX => None,
            r => Some(usize::from(r)),
        }
    }
}

/// HorizonsX: the region games a split makes at most (`HORIZON_REGION_BUCKETS`, e.g. the region threads; 0 or unset:
/// no limit). Regions past that share games: each game ticks several regions one after another on its thread.
/// Measured 11-09 with 600 spread players: one game a thread 5.74-5.97 GB (from 6.54-6.67) but 18.0-19.5 TPS, two
/// 5.95-6.10 GB at 17.3-19.1 TPS (a late region holds up the others of its game): off by default.
fn region_buckets() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("HORIZON_REGION_BUCKETS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .filter(|&n: &usize| n > 0)
            // One clock (`shared_clock`): a game for each core at most (the regions past that grouped), so the RAM
            // stays that of a few games whatever the player count.
            .unwrap_or(if shared_clock() { shared_clock_games() } else { usize::MAX })
    })
}

impl Regions {
    /// HorizonsX: at most `k` regions, the regions past that put together (largest first, each to the least loaded,
    /// by owned sections). Regions own disjoint sections with a ring between them, so a group of them is a region
    /// as well: one game ticks it, its parts far apart. Its first section is its members' smallest.
    pub(super) fn bucketed(self, k: usize, players: &[(i32, i32)]) -> Self {
        if self.count <= k {
            return self;
        }
        // HorizonsX: a region's work is half its ticking land (blocks, mobs) and half its players (their chunks,
        // tracking, spawning around them: 300 spread players, ~0.3 ms each): weighed by both, as shares of the
        // level's, so a few players on a big area and many on a small one fill the games evenly.
        let mut sections = vec![0usize; self.count];
        for &r in self.owned.values() {
            sections[r] += 1;
        }
        let mut people = vec![0usize; self.count];
        for &c in players {
            if let Some(r) = self.of_chunk(c) {
                people[r] += 1;
            }
        }
        let (all_sections, all_people) = (sections.iter().sum::<usize>().max(1), people.iter().sum::<usize>().max(1));
        let weight: Vec<usize> = (0..self.count).map(|r| sections[r] * all_people + people[r] * all_sections).collect();
        let mut order: Vec<usize> = (0..self.count).collect();
        order.sort_by_key(|&r| (std::cmp::Reverse(weight[r]), self.firsts[r]));
        let mut load = vec![0usize; k];
        let mut bucket = vec![0usize; self.count];
        for r in order {
            let b = (0..k).min_by_key(|&b| (load[b], b)).unwrap_or(0);
            bucket[r] = b;
            load[b] += weight[r].max(1);
        }
        let mut firsts: Vec<Option<(i32, i32)>> = vec![None; k];
        for (r, &f) in self.firsts.iter().enumerate() {
            let slot = &mut firsts[bucket[r]];
            *slot = Some(slot.map_or(f, |g| g.min(f)));
        }
        // Buckets in the order of their first sections, as regions are numbered.
        let mut used: Vec<usize> = (0..k).filter(|&b| firsts[b].is_some()).collect();
        used.sort_by_key(|&b| firsts[b]);
        let mut renumber = vec![usize::MAX; k];
        for (i, &b) in used.iter().enumerate() {
            renumber[b] = i;
        }
        let owned: horizon_client::fast_hash::FxHashMap<(i32, i32), usize> =
            self.owned.into_iter().map(|(key, r)| (key, renumber[bucket[r]])).collect();
        let firsts = used.iter().map(|&b| firsts[b].expect("used")).collect::<Vec<_>>();
        let grid = Grid::of(&owned);
        Self { owned, count: firsts.len(), firsts, grid }
    }
    pub(super) fn of(active_chunks: impl IntoIterator<Item = (i32, i32)>) -> Self {
        Self::with_players(active_chunks, &[], 0)
    }
    /// Folia's regions own every chunk their players see: each player's sections out to `view` chunks
    /// (its view distance, and the ring its sent chunks' light reads) are active, so a region's players never
    /// see into another region, and two players whose views meet are one region.
    pub(super) fn with_players(active_chunks: impl IntoIterator<Item = (i32, i32)>, players: &[(i32, i32)], view: i32) -> Self {
        use horizon_client::fast_hash::FxHashMap;
        // HorizonsX: two kinds of sections. A ticking one (a chunk in it ticks, or a player stands in it without
        // views) needs Folia's buffer: its ring of one section is read by its ticks, so ticking sections
        // `MERGE_RADIUS` apart or closer are one region, and a ticking section one section from another region's
        // view section is one region with it. A view section (in a player's view, `view` chunks, no ticking chunk)
        // is only read (its chunks sent, its entities tracked): it is owned, without a ring, and two regions' views
        // merge where they touch.
        const TICKING: u8 = 1;
        const VIEW: u8 = 2;
        let mut kind: FxHashMap<(i32, i32), u8> = FxHashMap::default();
        for (x, z) in active_chunks {
            kind.insert((x >> SECTION_SHIFT, z >> SECTION_SHIFT), TICKING);
        }
        for &(x, z) in players {
            if view <= 0 {
                kind.insert((x >> SECTION_SHIFT, z >> SECTION_SHIFT), TICKING);
                continue;
            }
            // Every section the player's view square (out to `view` chunks) touches.
            for sx in (x - view) >> SECTION_SHIFT..=(x + view) >> SECTION_SHIFT {
                for sz in (z - view) >> SECTION_SHIFT..=(z + view) >> SECTION_SHIFT {
                    kind.entry((sx, sz)).or_insert(VIEW);
                }
            }
        }
        let mut region: FxHashMap<(i32, i32), usize> = FxHashMap::default();
        let mut keys: Vec<(i32, i32)> = kind.keys().copied().collect();
        // The same numbering every tick for the same sections: regions in the order of their first section.
        keys.sort_unstable();
        let mut count = 0;
        let mut firsts = Vec::new();
        for start in keys {
            if region.contains_key(&start) {
                continue;
            }
            firsts.push(start);
            region.insert(start, count);
            let mut stack = vec![start];
            while let Some((x, z)) = stack.pop() {
                let mine = kind[&(x, z)];
                let reach = if mine == TICKING { MERGE_RADIUS } else { 1 };
                for dx in -reach..=reach {
                    for dz in -reach..=reach {
                        let next = (x + dx, z + dz);
                        let Some(&theirs) = kind.get(&next) else {
                            continue;
                        };
                        if region.contains_key(&next) {
                            continue;
                        }
                        let apart = dx.abs().max(dz.abs());
                        let joins = match (mine, theirs) {
                            (TICKING, TICKING) => apart <= MERGE_RADIUS,
                            // A player's view is one area; two views join where they touch.
                            (VIEW, VIEW) => apart <= 1,
                            _ => apart <= 1,
                        };
                        if joins {
                            region.insert(next, count);
                            stack.push(next);
                        }
                    }
                }
            }
            count += 1;
        }
        // HorizonsX: a region no player stands in (ticking chunks left at the edge of resident regions' areas, forced
        // chunks) gets no game of its own: its chunks tick on the server's thread with the rest no region owns. Each
        // was a whole game (with its chunk workers) for a few chunks: at 600 spread players about 100 more games
        // than regions with players, made and dropped as the residents came and went.
        if !players.is_empty() {
            let mut with_players = vec![false; count];
            for &(x, z) in players {
                if let Some(&r) = region.get(&(x >> SECTION_SHIFT, z >> SECTION_SHIFT)) {
                    with_players[r] = true;
                }
            }
            if with_players.iter().any(|w| !w) {
                let mut renumber = vec![usize::MAX; count];
                let mut kept = 0;
                for (r, &w) in with_players.iter().enumerate() {
                    if w {
                        renumber[r] = kept;
                        kept += 1;
                    }
                }
                region.retain(|_, r| renumber[*r] != usize::MAX);
                for r in region.values_mut() {
                    *r = renumber[*r];
                }
                firsts = firsts.into_iter().enumerate().filter(|(r, _)| with_players[*r]).map(|(_, f)| f).collect();
                count = kept;
            }
        }
        let mut owned = FxHashMap::default();
        for (&(x, z), &r) in &region {
            if kind[&(x, z)] == TICKING {
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        owned.insert((x + dx, z + dz), r);
                    }
                }
            }
        }
        // A view section is its region's (its own, where a ring around a ticking one reached it).
        for (&key, &r) in &region {
            owned.insert(key, r);
        }
        let grid = Grid::of(&owned);
        Self { owned, count, firsts, grid }
    }
    /// The region that owns chunk `(x, z)`.
    #[inline]
    pub(super) fn of_chunk(&self, (x, z): (i32, i32)) -> Option<usize> {
        let (sx, sz) = (x >> SECTION_SHIFT, z >> SECTION_SHIFT);
        match &self.grid {
            Some(g) => g.get(sx, sz),
            None => self.owned.get(&(sx, sz)).copied(),
        }
    }
    /// The region that owns the block at `x, z`.
    pub(super) fn of_block(&self, x: f64, z: f64) -> Option<usize> {
        self.of_chunk(((x.floor() as i32) >> 4, (z.floor() as i32) >> 4))
    }
}

/// `v`'s items whose place a region owns, moved into that region's list; the rest stay.
fn split<T>(v: &mut Vec<T>, regions: &Regions, at: impl Fn(&T) -> (f64, f64)) -> Vec<Vec<T>> {
    if v.is_empty() {
        return (0..regions.count).map(|_| Vec::new()).collect();
    }
    // HorizonsX: each list at its size first, so every item moves once (a mob is 3 kB: thousands of them, moved
    // again each time a region's list grew).
    let places: Vec<Option<usize>> = v.iter().map(|item| {
        let (x, z) = at(item);
        regions.of_block(x, z)
    }).collect();
    let mut sizes = vec![0usize; regions.count];
    let mut left = 0usize;
    for place in &places {
        match place {
            Some(r) => sizes[*r] += 1,
            None => left += 1,
        }
    }
    let mut out: Vec<Vec<T>> = sizes.iter().map(|&n| Vec::with_capacity(n)).collect();
    let mut kept = Vec::with_capacity(left);
    for (item, place) in v.drain(..).zip(places) {
        match place {
            Some(r) => out[r].push(item),
            None => kept.push(item),
        }
    }
    *v = kept;
    out
}

/// A map's entries whose block a region owns, moved into that region's map.
fn split_map<K: std::hash::Hash + Eq + Copy, V>(
    m: &mut HashMap<K, V>,
    regions: &Regions,
    at: impl Fn(&K) -> (i32, i32),
) -> Vec<HashMap<K, V>> {
    let mut out: Vec<HashMap<K, V>> = (0..regions.count).map(|_| HashMap::new()).collect();
    let keys: Vec<K> = m
        .keys()
        .copied()
        .filter(|k| regions.of_chunk(at(k)).is_some())
        .collect();
    for k in keys {
        let r = regions.of_chunk(at(&k)).expect("filtered");
        let v = m.remove(&k).expect("listed");
        out[r].insert(k, v);
    }
    out
}

fn fx_split<K: std::hash::Hash + Eq + Copy, V>(
    m: &mut horizon_client::fast_hash::FxHashMap<K, V>,
    regions: &Regions,
    at: impl Fn(&K) -> (i32, i32),
) -> Vec<horizon_client::fast_hash::FxHashMap<K, V>> {
    let mut out: Vec<horizon_client::fast_hash::FxHashMap<K, V>> =
        (0..regions.count).map(|_| Default::default()).collect();
    for (k, v) in m.extract_if(|k, _| regions.of_chunk(at(k)).is_some()) {
        let r = regions.of_chunk(at(&k)).expect("filtered");
        out[r].insert(k, v);
    }
    out
}

fn block_chunk(p: (i32, i32, i32)) -> (i32, i32) {
    (p.0 >> 4, p.2 >> 4)
}
fn array_chunk(p: &[i32; 3]) -> (i32, i32) {
    (p[0] >> 4, p[2] >> 4)
}

impl<'a> Game<'a> {
    /// A game for a region thread: the same resources, options and world settings as this one, and no
    /// level, players or connections of its own (a region's are lent in for its tick).
    /// A region game moves to its thread and back: its own handle on the resources, made once and kept for the
    /// server's life.
    fn static_resources(&mut self) -> &'static std::sync::Arc<super::Resources> {
        match self.static_res {
            Some(r) => r,
            None => {
                let r: &'static _ = Box::leak(Box::new(std::sync::Arc::clone(self.res_ref)));
                self.static_res = Some(r);
                r
            }
        }
    }

    /// HorizonsX: `n` more region games made on threads of their own (making one reads the server's resource
    /// files and builds a whole game: some 120 ms on the server's thread, a spike when many regions appeared at
    /// once), each taken in by [`Game::collect_region_games`] once made.
    fn request_region_games(&mut self, n: usize) {
        let res = self.static_resources();
        for _ in 0..n {
            let (tx, rx) = std::sync::mpsc::channel();
            let (pack, mut options, settings) = (self.pack.clone(), self.options.clone(), self.settings.clone());
            options.server_thread = false;
            let spawned = std::thread::Builder::new().name("HorizonsX region game maker".into()).spawn(move || {
                let made = super::with_lean_game(|| super::integrated::server_game(res, pack, options, settings))
                    .map(Box::new)
                    .map_err(|e| e.to_string());
                let _ = tx.send(made);
            });
            if spawned.is_ok() {
                REGION_GAMES_MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                *self.region_games_live.entry(self.dim).or_insert(0) += 1;
                self.region_game_factory.push(rx);
            }
        }
    }

    /// The region games made since ([`Game::request_region_games`]), set up for the level in place and kept.
    fn collect_region_games(&mut self) {
        let mut i = 0;
        while i < self.region_game_factory.len() {
            match self.region_game_factory[i].try_recv() {
                Ok(made) => {
                    self.region_game_factory.remove(i);
                    match made {
                        Ok(mut w) => {
                            w.copy_world_settings_from(self);
                            w.region_worker = true;
                            w.next_entity = 0;
                            w.entity_id_end = 0;
                            self.region_games.entry(self.dim).or_default().push(w);
                        }
                        Err(e) => {
                            log::warn!("HorizonsX: a region game could not be made: {e}");
                            self.region_games_live.entry(self.dim).and_modify(|n| *n = n.saturating_sub(1));
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => i += 1,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.region_games_live.entry(self.dim).and_modify(|n| *n = n.saturating_sub(1));
                    self.region_game_factory.remove(i);
                }
            }
        }
    }

    pub(super) fn region_worker(&mut self) -> Result<Game<'static>> {
        let mut options = self.options.clone();
        options.server_thread = false;
        let res = self.static_resources();
        let mut w = super::with_lean_game(|| super::integrated::server_game(res, self.pack.clone(), options, self.settings.clone()))?;
        w.copy_world_settings_from(self);
        w.region_worker = true;
        // No ids of its own: the first tick draws a block from the server's counter (`refill_entity_ids`).
        w.next_entity = 0;
        w.entity_id_end = 0;
        Ok(w)
    }

    /// What every region game shares with the server's: the world's settings and tables, as they are now
    /// (copied again when they change: `/gamerule`, `/difficulty`, `/reload`).
    pub(super) fn copy_world_settings_from(&mut self, from: &Game<'_>) {
        self.level = from.level.clone();
        self.level_shapes = from.level_shapes;
        self.world_root = from.world_root.clone();
        self.world_biomes = from.world_biomes.clone();
        self.end.set_locator(from.end.locator());
        self.dim = from.dim;
        self.world_flat = from.world_flat;
        self.world_debug = from.world_debug;
        // The server's tables, shared: building them read every item and recipe file from disk on the server's
        // thread (~1 s a new region game: the tick spike after a far /rtp).
        self.world_tables = from.world_tables.clone();
        self.share_spawn_conditions();
        self.copy_tick_settings_from(from);
    }

    /// What can change from tick to tick that a region reads but never writes: the rules, the difficulty,
    /// the clock, the border, the ticket levels.
    pub(super) fn copy_tick_settings_from(&mut self, from: &Game<'_>) {
        self.ui.rules = from.ui.rules.clone();
        self.ui.difficulty = from.ui.difficulty;
        self.clock = from.clock.clone();
        self.border = from.border.clone();
        self.tickets = from.tickets.clone();
        self.level.difficulty_locked = from.level.difficulty_locked;
    }

    /// The next entity ids this region game hands out: a block of its own from `pool` (the server's shared
    /// counter) when the one it has runs low, so no two threads ever give out the same id.
    pub(super) fn refill_entity_ids(&mut self, pool: &std::sync::atomic::AtomicU32) {
        if self.entity_id_end.saturating_sub(self.next_entity) < ID_REFILL {
            let start = pool.fetch_add(ID_BLOCK, std::sync::atomic::Ordering::Relaxed);
            self.next_entity = start;
            self.entity_id_end = start + ID_BLOCK;
        }
    }

    /// Region `r`'s part of the level in place, moved into region game `w` (Folia's region data): its
    /// chunks with their block entities, block and fluid ticks and random-tick caches, every entity that
    /// stands in it, and what is kept by block or by entity in it. `worlds`, `ticks`, `sections` and the
    /// entity lists come from one split of the level ([`LevelSplit`]).
    pub(super) fn lend_region(&mut self, w: &mut Game<'_>, mut part: RegionPart) {
        w.world = part.world;
        w.block_ticks = part.ticks;
        w.random_tick_sections = part.sections;
        w.rand_sequences = part.sequences;
        w.mobs = part.mobs;
        w.creaking.lend_scanned(part.scanned);
        w.dropped = part.dropped;
        w.orbs = part.orbs;
        w.creatures.projectiles = part.projectiles;
        w.falling = part.falling;
        w.paintings = part.paintings;
        w.item_frames = part.item_frames;
        w.armor_stands = part.armor_stands;
        w.cushions = part.cushions;
        w.others = part.others;
        w.boats = part.boats;
        w.carts = part.carts;
        w.bolts = part.bolts;
        w.crystals = part.crystals;
        w.dragon_fireballs = part.dragon_fireballs;
        w.breath_clouds = part.breath_clouds;
        w.tridents = part.tridents;
        w.rockets = part.rockets;
        w.ominous_spawners = part.ominous_spawners;
        w.tnts = part.tnts;
        w.fangs.list = part.fangs;
        w.hoppers = part.hoppers;
        w.jukebox_sounds = part.jukebox_sounds;
        w.lid_openers = part.lid_openers;
        w.knot_ids = part.knot_ids;
        w.lectern_pulses = part.lectern_pulses;
        let a = &mut w.be_anims;
        a.chests = std::mem::take(&mut part.anims.chests);
        a.books = std::mem::take(&mut part.anims.books);
        a.shulkers = std::mem::take(&mut part.anims.shulkers);
        a.bells = std::mem::take(&mut part.anims.bells);
        a.conduits = std::mem::take(&mut part.anims.conduits);
        a.skulls = std::mem::take(&mut part.anims.skulls);
        a.pots = std::mem::take(&mut part.anims.pots);
        a.gateway_servers = std::mem::take(&mut part.anims.gateway_servers);
        a.conduit_servers = std::mem::take(&mut part.anims.conduit_servers);
        // The ids of the region's entities: their remembered UUIDs and compounds go with them.
        let mut ids: Vec<u32> = w.mobs.iter().map(|m| m.id).collect();
        ids.extend(w.dropped.iter().map(|d| d.id));
        ids.extend(w.boats.iter().map(|b| b.id));
        ids.extend(w.carts.iter().map(|c| c.id));
        ids.extend(w.armor_stands.iter().map(|s| s.id));
        // HorizonsX: every kind a region's tracker names by UUID (`lan_entity_uuids`), so none is given a new one.
        ids.extend(w.orbs.iter().map(|o| o.id as u32));
        ids.extend(w.creatures.projectiles.iter().map(|p| p.id as u32));
        ids.extend(w.falling.iter().map(|f| f.id as u32));
        ids.extend(w.tnts.iter().map(|t| t.id as u32));
        ids.extend(w.tridents.iter().map(|t| t.id as u32));
        ids.extend(w.rockets.iter().map(|r| r.id as u32));
        ids.extend(w.crystals.iter().map(|c| c.id as u32));
        ids.extend(w.bolts.iter().map(|b| b.id as u32));
        ids.extend(w.ominous_spawners.iter().map(|o| o.id as u32));
        ids.extend(w.breath_clouds.iter().map(|b| b.id as u32));
        ids.extend(w.dragon_fireballs.iter().map(|f| f.id as u32));
        ids.extend(w.fangs.list.iter().map(|f| f.id as u32));
        ids.extend(w.paintings.iter().map(|p| p.id as u32));
        ids.extend(w.item_frames.iter().map(|f| f.id as u32));
        ids.extend(w.cushions.iter().map(|c| c.id as u32));
        ids.extend(w.others.iter().map(|o| o.id as u32));
        // Maps that are empty (most of them, most ticks) are passed over: five lookups an entity added up.
        let (raw, died, love, hurt) = (
            !self.persist.raw.is_empty(),
            !self.died.is_empty(),
            !self.love_cause.is_empty(),
            !self.last_hurt_by_player.is_empty(),
        );
        w.persist.uuids.reserve(ids.len());
        for id in ids {
            if let Some(u) = self.persist.uuids.remove(&id) {
                w.persist.uuids.insert(id, u);
            }
            if raw && let Some(t) = self.persist.raw.remove(&id) {
                w.persist.raw.insert(id, t);
            }
            if died && self.died.remove(&id) {
                w.died.insert(id);
            }
            if love && let Some(v) = self.love_cause.remove(&id) {
                w.love_cause.insert(id, v);
            }
            if hurt && let Some(v) = self.last_hurt_by_player.remove(&id) {
                w.last_hurt_by_player.insert(id, v);
            }
        }
        w.world.game_time = self.world.game_time;
        w.world.day_time = self.world.day_time;
        w.world.weather = self.world.weather;
        w.world.health_rules = self.world.health_rules.clone();
        w.world.watched = self.world.watched.clone();
    }

    /// Region game `w`'s part of the level back in place (after the level's own data, in region order),
    /// with what its tick left for the server: level events, spills, sounds and particles for the network.
    pub(super) fn take_region_back(&mut self, w: &mut Game<'_>, chunks: &[ChunkPos]) -> horizon_client::random_sequences::RandomSequences {
        // HorizonsX: the players' damage records come back with them ([`Game::tick_world_regions_rest`] lends them).
        self.guest_combat.extend(w.guest_combat.drain());
        // HorizonsX: the world's part comes back on a helper while the scheduled ticks, sections and entity lists
        // come back here (each touches its own fields: one after another they were the landing's whole time).
        let stand_in = self.world.stand_in();
        let region_world = std::mem::replace(&mut w.world, stand_in);
        let region_ticks = std::mem::take(&mut w.block_ticks);
        let region_sections = std::mem::take(&mut w.random_tick_sections);
        let world = &mut self.world;
        let world_back = move || horizon_client::spans::time("rl_world", || world.take_back(region_world));
        let rest_back = || {
            horizon_client::spans::time("rl_ticks", || self.block_ticks.take_back(region_ticks, chunks));
            horizon_client::spans::time("rl_sections", || self.random_tick_sections.take_back(region_sections));
            let lists_started = horizon_client::spans::enabled().then(std::time::Instant::now);
            self.mobs.append(&mut w.mobs);
            self.creaking.take_scanned_back(&mut w.creaking);
            self.dropped.append(&mut w.dropped);
            self.orbs.append(&mut w.orbs);
            self.creatures.projectiles.append(&mut w.creatures.projectiles);
            self.falling.append(&mut w.falling);
            self.paintings.append(&mut w.paintings);
            self.item_frames.append(&mut w.item_frames);
            self.armor_stands.append(&mut w.armor_stands);
            self.cushions.append(&mut w.cushions);
            self.others.append(&mut w.others);
            self.boats.append(&mut w.boats);
            self.carts.append(&mut w.carts);
            self.bolts.append(&mut w.bolts);
            self.crystals.append(&mut w.crystals);
            self.dragon_fireballs.append(&mut w.dragon_fireballs);
            self.breath_clouds.append(&mut w.breath_clouds);
            self.tridents.append(&mut w.tridents);
            self.rockets.append(&mut w.rockets);
            self.ominous_spawners.append(&mut w.ominous_spawners);
            self.tnts.append(&mut w.tnts);
            self.fangs.list.append(&mut w.fangs.list);
            self.hoppers.extend(w.hoppers.drain());
            self.jukebox_sounds.extend(w.jukebox_sounds.drain());
            self.lid_openers.extend(w.lid_openers.drain());
            self.knot_ids.extend(w.knot_ids.drain());
            self.lectern_pulses.append(&mut w.lectern_pulses);
            let (a, b) = (&mut self.be_anims, &mut w.be_anims);
            a.chests.extend(b.chests.drain());
            a.books.extend(b.books.drain());
            a.shulkers.extend(b.shulkers.drain());
            a.bells.extend(b.bells.drain());
            a.conduits.extend(b.conduits.drain());
            a.skulls.extend(b.skulls.drain());
            a.pots.extend(b.pots.drain());
            a.gateway_servers.extend(b.gateway_servers.drain());
            a.conduit_servers.extend(b.conduit_servers.drain());
            self.persist.uuids.extend(w.persist.uuids.drain());
            self.persist.raw.extend(w.persist.raw.drain());
            self.died.extend(w.died.drain());
            self.love_cause.extend(w.love_cause.drain());
            self.last_hurt_by_player.extend(w.last_hurt_by_player.drain());
            // What the region's tick left for the server's game, after the server's own.
            self.level_events.append(&mut w.level_events);
            self.pending.absorb(std::mem::take(&mut w.pending));
            self.spill.absorb(std::mem::take(&mut w.spill));
            self.creatures.lan_sounds.append(&mut w.creatures.lan_sounds);
            self.creatures.lan_particles.append(&mut w.creatures.lan_particles);
            self.lan_events.append(&mut w.lan_events);
            self.lan_block_events.append(&mut w.lan_block_events);
            self.lan_entity_events.append(&mut w.lan_entity_events);
            self.lan_critical_hits.append(&mut w.lan_critical_hits);
            self.lan_cushion_breaks.append(&mut w.lan_cushion_breaks);
            self.lan_songs.append(&mut w.lan_songs);
            self.lan_kinetic_hits.append(&mut w.lan_kinetic_hits);
            self.lan_global_events.append(&mut w.lan_global_events);
            if let Some(t) = lists_started {
                horizon_client::spans::record("rl_lists", t.elapsed());
            }
        };
        horizon_client::v2::pool().join(world_back, rest_back);
        std::mem::take(&mut w.rand_sequences)
    }
}

/// How long the server's tick waits for its regions (`HORIZON_REGION_DEADLINE_MS`, 50 by default): a region
/// still running then goes on by itself and comes back on a later tick (Folia's lag isolation).
fn region_deadline() -> std::time::Duration {
    static MS: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    std::time::Duration::from_millis(*MS.get_or_init(|| {
        std::env::var("HORIZON_REGION_DEADLINE_MS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            // A whole tick's budget: a region that still keeps 20 TPS is never cut loose (measured 11-05 at
            // 100 players: 30 ms cut busy-but-fine regions loose, their players were held and came back in
            // bursts, 67 ms ticks in a bad run); a lag machine's region (80 ms) still is.
            // One clock (`shared_clock`): every region waited for, every tick (no region runs on alone).
            .unwrap_or(if shared_clock() { 3_600_000 } else { 50 })
    }))
}

/// The server tick's own clock for the regions' wait: when the tick started, and what the last tick spent after
/// its last regions came back (the network turn, block entities: work the tick still has to do once the wait is
/// over). In nanoseconds from `clock_base`; 0 = not known.
static TICK_STARTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static REGIONS_LANDED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TICK_TAIL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn clock_base() -> std::time::Instant {
    static BASE: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *BASE.get_or_init(std::time::Instant::now)
}
fn clock_ns() -> u64 {
    clock_base().elapsed().as_nanos() as u64 + 1
}

/// A dedicated server's tick starts (`tick_server`).
pub(super) fn server_tick_begins() {
    TICK_STARTED.store(clock_ns(), std::sync::atomic::Ordering::Relaxed);
}

/// The server's tick is over (before the loop sleeps to the next one): its time after its last regions came back
/// is noted for the next tick's wait. A tick with no regions out leaves the last one's.
pub(super) fn server_tick_ends() {
    use std::sync::atomic::Ordering::Relaxed;
    let landed = REGIONS_LANDED.swap(0, Relaxed);
    if landed != 0 {
        TICK_TAIL.store(clock_ns().saturating_sub(landed), Relaxed);
    }
    super::chunk_cost::tick_ends();
}

/// Whether the regions' wait ends with the server tick's time (`region_wait_end`): on, unless
/// `HORIZON_REGION_TICK_DEADLINE=0` (then a flat `region_deadline` after the regions go out, as before).
fn tick_deadline_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_REGION_TICK_DEADLINE").map_or(!shared_clock(), |v| v.trim() != "0"))
}

/// The least the server tick waits for its regions (`HORIZON_REGION_MIN_WAIT_MS`, 10 by default): a tick whose
/// own work before the regions ran long still gives regions that keep time a chance to come back with it.
fn region_min_wait() -> std::time::Duration {
    static MS: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    std::time::Duration::from_millis(*MS.get_or_init(|| {
        std::env::var("HORIZON_REGION_MIN_WAIT_MS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(10)
    }))
}

/// When the server tick stops waiting for the regions it sent out at `lent`: `region_deadline` later, but no later
/// than the tick's own 50 ms less what the last tick spent after its regions came back (so a late region no
/// longer takes the whole server past 50 ms: the tick waited a flat 50 ms after the lend, on top of its own
/// work before and after), and never sooner than `region_min_wait` after the lend.
fn region_wait_end(lent: std::time::Instant) -> std::time::Instant {
    use std::sync::atomic::Ordering::Relaxed;
    let flat = lent + region_deadline();
    let started = TICK_STARTED.load(Relaxed);
    if !tick_deadline_on() || started == 0 {
        return flat;
    }
    let base = clock_base();
    let started = base + std::time::Duration::from_nanos(started);
    // A tick that started long ago is no server tick (a world ticked another way): the flat wait.
    if lent.saturating_duration_since(started) > std::time::Duration::from_secs(2) {
        return flat;
    }
    let tail = std::time::Duration::from_nanos(TICK_TAIL.load(Relaxed)).min(std::time::Duration::from_millis(40));
    let tick_end = (started + std::time::Duration::from_millis(50)).checked_sub(tail).unwrap_or(started);
    // HorizonsX: a tick already past its 50 ms when its regions go out waits for them the flat deadline. Cut
    // down to the minimum wait, regions of a millisecond or two missed it behind the others on the region
    // threads (1,905 late in a minute with 300 spread players, regions at 1.2 ms on average): their players
    // left the server's lists and came back every tick (the locator bar's connections made again for all,
    // entities untracked and tracked), which made the overloaded tick slower still.
    if tick_end <= lent + region_min_wait() {
        return flat;
    }
    flat.min(tick_end)
}

/// A test of lag isolation: `HORIZON_REGION_LAG_TEST_MS` makes the first region that long slower each tick.
fn lag_test() -> std::time::Duration {
    static MS: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    std::time::Duration::from_millis(*MS.get_or_init(|| {
        std::env::var("HORIZON_REGION_LAG_TEST_MS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0)
    }))
}

/// HorizonsX: the slowest region tick since the last region log line: ms, chunks, players, slowest phases.
static SLOWEST_REGION: std::sync::Mutex<(f32, usize, usize, Vec<(&'static str, f32)>)> = std::sync::Mutex::new((0.0, 0, 0, Vec::new()));

/// What a region thread sends back: its game, its tick's result, the ms it took and its spans.
type Back = (Box<Game<'static>>, std::result::Result<(), String>, f32, Vec<horizon_client::spans::Span>);

/// A region game ticking on a region thread (Folia's `TickRegionScheduler`'s tick of one region), and what
/// its part of the level needs to come back. The game itself is the thread's until it sends it back.
pub(super) struct Flight {
    dim: super::Dim,
    game: Option<Box<Game<'static>>>,
    chunks: Vec<ChunkPos>,
    conns: Vec<u32>,
    /// The region's first section (`Regions::firsts`).
    first: (i32, i32),
    /// Where its players stood when it was lent: their areas stay loaded while it is away.
    places: Vec<(i32, i32)>,
    done: std::sync::mpsc::Receiver<Back>,
    result: Option<std::result::Result<(), String>>,
    /// How long its tick took on its thread (ms).
    took: f32,
    /// HorizonsX's resident regions: it stays on its thread between ticks (its part of the level isn't taken
    /// back), ticked again each tick, back when one asks for it or every `resident_period` ticks.
    resident: bool,
    /// The level's regions when it was lent and its own number: what its players' views must stay inside.
    layout: Option<(std::sync::Arc<Regions>, usize)>,
    /// It ran its players' whole turn (it owns all they see, and nobody else sees into it): it may stay resident.
    whole: bool,
    /// Its players gathered on the server's thread while it is parked (`Game::gather_residents`).
    visiting: Vec<u32>,
}

/// The regions' last ticks, for the console's report (Folia's per-region `/tps`): count, slowest, average.
static REGION_TIMES: std::sync::Mutex<(usize, f32, f32, u64)> = std::sync::Mutex::new((0, 0.0, 0.0, 0));

/// What the last region ticks took: regions in the tick, the slowest one's ms, their average ms, regions that
/// ran past a tick since the server started.
pub(super) fn region_report() -> (usize, f32, f32, u64) {
    *REGION_TIMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// HorizonsX's region watchdog (Folia's `RegionizedWatchdog`): every region tick under way, with the phase it is
/// in. A thread checks them once a second and logs each one that has run 5 s or more, again every 5 s, with
/// its level, place, players, chunks and phase (a stuck region doesn't hold the server: lag isolation).
mod watchdog {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    pub(super) struct Running {
        pub level: &'static str,
        pub at: (i32, i32),
        pub players: usize,
        pub chunks: usize,
        pub started: Instant,
        pub printed: Option<Instant>,
        pub phase: Arc<Mutex<&'static str>>,
    }

    static RUNNING: Mutex<Option<HashMap<u64, Running>>> = Mutex::new(None);
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

    thread_local! {
        /// The phase slot of the region this thread ticks, if any.
        static PHASE: std::cell::RefCell<Option<Arc<Mutex<&'static str>>>> = const { std::cell::RefCell::new(None) };
    }

    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A region tick starting: its id, for `begin` on its thread and `end`.
    pub(super) fn register(mut r: Running) -> (u64, Arc<Mutex<&'static str>>) {
        static STARTED: std::sync::Once = std::sync::Once::new();
        STARTED.call_once(|| {
            let _ = std::thread::Builder::new().name("Region Watchdog".into()).spawn(watch);
        });
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        r.started = Instant::now();
        let phase = r.phase.clone();
        lock(&RUNNING).get_or_insert_with(HashMap::new).insert(id, r);
        (id, phase)
    }
    /// On the region thread, for the length of its tick.
    pub(super) fn begin(phase: Arc<Mutex<&'static str>>) {
        PHASE.with(|p| *p.borrow_mut() = Some(phase));
    }
    pub(super) fn end(id: u64) {
        PHASE.with(|p| *p.borrow_mut() = None);
        if let Some(m) = lock(&RUNNING).as_mut() {
            m.remove(&id);
        }
    }
    /// The phase this thread's region is in now.
    #[inline]
    pub(super) fn phase(name: &'static str) {
        PHASE.with(|p| {
            if let Some(slot) = &*p.borrow() {
                *lock(slot) = name;
            }
        });
    }
    fn watch() {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = Instant::now();
            let mut lines = Vec::new();
            if let Some(m) = lock(&RUNNING).as_mut() {
                for r in m.values_mut() {
                    let ran = now.duration_since(r.started);
                    if ran >= Duration::from_secs(5) && r.printed.is_none_or(|p| now.duration_since(p) >= Duration::from_secs(5)) {
                        r.printed = Some(now);
                        lines.push(format!(
                            "HorizonsX region watchdog: a region of {} near block {}, {} ({} players, {} chunks) has not finished its tick in {:.1} s; it is in {}",
                            r.level,
                            r.at.0,
                            r.at.1,
                            r.players,
                            r.chunks,
                            ran.as_secs_f32(),
                            *lock(&r.phase)
                        ));
                    }
                }
            }
            for line in lines {
                log::error!("{line}");
            }
        }
    }
}

/// A region phase: named for the watchdog, timed for the spans.
#[inline]
fn phase<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
    watchdog::phase(name);
    horizon_client::spans::time(name, f)
}

impl Flight {
    fn launch(
        dim: super::Dim,
        mut game: Box<Game<'static>>,
        chunks: Vec<ChunkPos>,
        conns: Vec<u32>,
        lag: std::time::Duration,
        first: (i32, i32),
    ) -> Self {
        let (tx, done) = std::sync::mpsc::sync_channel(1);
        let level = dim.id();
        let (id, slot) = watchdog::register(watchdog::Running {
            level,
            at: (first.0 << (SECTION_SHIFT + 4), first.1 << (SECTION_SHIFT + 4)),
            players: conns.len(),
            chunks: chunks.len(),
            started: std::time::Instant::now(),
            printed: None,
            phase: std::sync::Arc::new(std::sync::Mutex::new("starting")),
        });
        let pool = if horizon_client::v2::region_pool_on() {
            horizon_client::v2::region_pool()
        } else {
            horizon_client::v2::pool()
        };
        pool.spawn(move || {
            let started = std::time::Instant::now();
            watchdog::begin(slot);
            if !lag.is_zero() {
                std::thread::sleep(lag);
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                horizon_client::v2::as_region(|| {
                    horizon_client::block_contact::in_level(Some(level), || game.tick_region_phases())
                })
                .map_err(|e| e.to_string())
            }))
            .unwrap_or_else(|_| Err("a region thread panicked".to_owned()));
            watchdog::end(id);
            // Its phases' times, for the server's reports (`region:NAME`): its thread's spans go back with it.
            let spans = horizon_client::spans::take();
            let _ = tx.send((game, result, started.elapsed().as_secs_f32() * 1000.0, spans));
        });
        Self { dim, game: None, chunks, conns, first, places: Vec::new(), done, result: None, took: 0.0, resident: false, layout: None, whole: false, visiting: Vec::new() }
    }
    fn arrived(&mut self, (game, result, took, spans): Back) {
        // HorizonsX: the slowest region since the last region log line, with its slowest phases (while spans are on).
        if !spans.is_empty()
            && let Ok(mut slowest) = SLOWEST_REGION.lock()
            && took > slowest.0
        {
            let mut top: Vec<(&'static str, f32)> = spans.iter().map(|s| (s.name, s.ms)).collect();
            top.sort_by(|a, b| b.1.total_cmp(&a.1));
            top.truncate(8);
            *slowest = (took, self.chunks.len(), self.conns.len(), top);
        }
        horizon_client::spans::absorb_region(spans);
        self.game = Some(game);
        self.result = Some(result);
        self.took = took;
    }
    /// Done by `deadline`.
    fn wait_until(&mut self, deadline: std::time::Instant) -> bool {
        if self.result.is_some() {
            return true;
        }
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match self.done.recv_timeout(left) {
            Ok(back) => {
                self.arrived(back);
                true
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => false,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                self.result = Some(Err("a region thread stopped".to_owned()));
                true
            }
        }
    }
    /// Done now.
    fn finished(&mut self) -> bool {
        if self.result.is_some() {
            return true;
        }
        match self.done.try_recv() {
            Ok(back) => {
                self.arrived(back);
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.result = Some(Err("a region thread stopped".to_owned()));
                true
            }
        }
    }
    /// Done, waiting for it.
    fn wait(&mut self) -> bool {
        if self.result.is_none() {
            match self.done.recv() {
                Ok(back) => self.arrived(back),
                Err(_) => self.result = Some(Err("a region thread stopped".to_owned())),
            }
        }
        true
    }
}

/// Whether regions tick on their threads with plugins listening: on, unless `HORIZON_REGION_PLUGINS=0`.
fn region_plugins_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_REGION_PLUGINS").map_or(true, |v| v.trim() != "0"))
}

/// The server.properties key that turns region threads on or off (`true` by default). Off, every level
/// ticks on the one server thread, as vanilla and Paper do: less RAM, fewer cores, fewer players at 20 TPS.
pub(super) const REGION_THREADS_PROPERTY: &str = "horizonsx-region-threads";

/// The owner's choice from server.properties (`REGION_THREADS_PROPERTY`), read before the first tick.
static REGION_THREADS_SETTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Sets the server.properties choice; called once at start, before any level ticks.
pub(super) fn set_region_threads(on: bool) {
    REGION_THREADS_SETTING.store(on, std::sync::atomic::Ordering::Relaxed);
    // One clock: a region's phases on its own thread while other regions tick (the shared helpers were waited for
    // behind every other region's: 300 spread players, block ticks 98 ms waited for 24 ms of work).
    if !on {
        horizon_client::v2::set_region_nested(false);
        // A few games, not one a region: each encoder may keep more (12 games: 192 MB at most).
        horizon_client::lan::chunks::set_encoder_cache(16 << 20, 1 << 16);
        // Every player's chunk backlog at once at a landing: a few ticks instead (`set_chunk_send_cap`).
        horizon_client::lan::hub::set_chunk_send_cap(8.0);
    }
}

/// HorizonsX: `horizonsx-region-threads=false`: one clock for the whole server, as on Paper. No region ticks on by
/// itself or falls behind (no region has a TPS of its own): every tick waits for all of them. The level's work still
/// goes over the cores, Pumpkin's way (its entity islands are our regions): a few region games (`shared_clock_games`),
/// resident regions waited for every tick. `HORIZON_REGION_THREADS=0` is the one-thread server instead.
pub(super) fn shared_clock() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("HORIZON_REGION_THREADS").is_err() && !REGION_THREADS_SETTING.load(std::sync::atomic::Ordering::Relaxed)
    })
}

/// The region games one clock keeps (`HORIZON_REGION_BUCKETS` overrides): three quarters of the hardware threads, as
/// Pumpkin's simulation pool takes them all (400 spread players: 8 games 55 ms a tick, 12 games 42.7 ms, +0.1 GB; the
/// rest stay for the network writers, chunk generation and the tick's own thread).
fn shared_clock_games() -> usize {
    (std::thread::available_parallelism().map_or(4, |n| n.get()) * 3 / 4).max(2)
}

/// Whether the level's world phases run on region threads (Folia's regionized ticking): on, under one clock or
/// with lag isolation (`shared_clock`); `HORIZON_REGION_THREADS=0` turns them off (one thread).
/// A level whose players make one region ticks as one either way.
pub(super) fn region_threads_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_REGION_THREADS").map_or(true, |v| v.trim() != "0"))
}

/// Whether a region holds its players' whole view (Folia's rule, `HORIZON_REGION_VIEWS=1`), or only the
/// chunks that tick and a ring around them (the default: far fewer merges, so more regions at once).
fn region_views_on() -> bool {
    // Off unless `HORIZON_REGION_VIEWS=1`: with views, homes 512 blocks apart at a view of 10 are one region (their
    // views touch, as Folia's do: 300 spread players ended in regions of 135). With them on, each region's
    // players' chunks and entities go out on its own thread (`region_chunks_on`, `region_track_on`).
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_REGION_VIEWS").is_ok_and(|v| v.trim() == "1"))
}

/// HorizonsX's resident regions (Folia's: a region keeps its chunks, entities and players): a region that owns all
/// its players see stays on its thread between ticks, ticked again each tick with no split and no landing of the
/// level, its players' whole turn its own. Every `resident_period` ticks (and whenever something needs the whole
/// level: a command, a save, a player leaving, a packet that reaches past the region) they all come back for one
/// tick as before. On with `HORIZON_RESIDENT=1`.
pub(super) fn resident_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    // One clock (`shared_clock`): on, waited for every tick like every other region.
    *ON.get_or_init(|| std::env::var("HORIZON_RESIDENT").map_or(shared_clock(), |v| v.trim() == "1"))
}

/// Whether the resident regions run on through the rest of the server's tick (`HORIZON_RESIDENT_PIPELINE`, on
/// unless 0): the tick waits for them when it next launches them, not at the end of its region phase. Every
/// region still ticks once a server tick; what changes is that the server thread's own work after its regions
/// (streaming, its mobs, the network turn: 16 ms with 650 spread players) runs while the residents tick instead
/// of after, with the region threads idle. A tick then costs the longer of the two, not their sum.
fn resident_pipeline() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_RESIDENT_PIPELINE").is_ok_and(|v| v.trim() == "1"))
}

/// Whether the resident regions' players are gathered on the server's thread while their regions are parked
/// (`Game::gather_residents`): on with resident regions (unless `HORIZON_RESIDENT_GATHER=0`) when they aren't
/// pipelined. Chat, commands, plugin menus and the plugins' tick then see every player, and none of them brings
/// the regions back (a server with hundreds of players chats every tick).
fn gather_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        resident_on() && !resident_pipeline() && std::env::var("HORIZON_RESIDENT_GATHER").map_or(true, |v| v.trim() != "0")
    })
}

/// `HORIZON_REGION_DIAG=1`: big regions are logged (`tick_world_regions_rest`).
fn region_diag_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_REGION_DIAG").is_ok_and(|v| v.trim() == "1"))
}

/// HorizonsX: whether a gathered player's command (`Guest::visiting`) needs the world around it, so it waits for
/// the regions to be back: vanilla's commands (but its chat ones) and the world edits of the plugins (WorldEdit,
/// CoreProtect's rollbacks). The plugins' other commands (homes, money, menus, jobs, auctions...) run while the
/// regions stay out.
pub(super) fn command_waits_for_world(host: &super::lan::LanHost, line: &str) -> bool {
    let root = line.trim_start_matches('/').split(' ').next().unwrap_or_default().to_ascii_lowercase();
    let root = root.rsplit(':').next().unwrap_or_default();
    // Chat-like vanilla commands touch no world.
    const CHAT: &[&str] = &["msg", "tell", "w", "me", "list", "help", "teammsg", "tm", "trigger", "say"];
    // World edits and lookups of the plugins (WorldEdit's commands come with their slashes trimmed).
    const WORLD: &[&str] = &[
        "we", "worldedit", "set", "replace", "stack", "move", "copy", "cut", "paste", "undo", "redo", "walls",
        "faces", "outline", "overlay", "naturalize", "smooth", "regen", "brush", "br", "sphere", "hsphere", "cyl",
        "hcyl", "pyramid", "hpyramid", "generate", "gen", "deform", "hollow", "line", "curve", "center", "drain",
        "fixwater", "fixlava", "snow", "thaw", "green", "ex", "extinguish", "butcher", "remove", "schem",
        "schematic", "rotate", "flip", "forest", "flora", "wand", "pos1", "pos2", "hpos1", "hpos2", "sel",
        "expand", "contract", "shift", "outset", "inset", "chunk", "size", "count", "distr", "fill", "fillr",
        "removeabove", "removebelow", "removenear", "replacenear", "setbiome", "tool", "superpickaxe", "sp",
        "toggleplace", "gmask", "mask", "material", "range", "craftscript", "cs", "lrbuild", "biomeinfo", "co",
        "coreprotect", "core", "rollback", "restore", "lookup", "inspect", "near", "jumpto", "ascend", "descend",
        "ceil", "thru", "up", "unstuck", "!",
    ];
    if CHAT.contains(&root) {
        return false;
    }
    if WORLD.contains(&root) {
        return true;
    }
    super::Game::native_command_level(host, root).is_none()
}

/// Whether residency asks only that no player see where another region ticks (`HORIZON_RESIDENT_EDGE`, on
/// unless 0): two players of different regions conflict when one's view (and its ring) reaches the other's
/// simulation distance (and a chunk more for what its ticks touch). Without, any chunk of the other region's
/// sections counted, and sections are 4 chunks: lone players 384 blocks apart at view 10 and simulation 7 never
/// stayed out (their views reach the ring around each other's ticking chunks, which no tick changes). The view's
/// far edge in another region's ring is sent at the next full tick (the server's world then holds it).
fn resident_edge_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_RESIDENT_EDGE").map_or(true, |v| v.trim() != "0"))
}

/// The pairs of owners (a region's number, or none for the server's own players) of `players` closer than
/// `reach` chunks to each other on both axes (`resident_edge_on`'s rule), each pair once, the lower first.
fn close_owners<T: Copy + PartialEq>(players: &[((i32, i32), T)], reach: i32) -> Vec<(T, T)> {
    use horizon_client::fast_hash::FxHashMap;
    let cell = reach.max(1);
    let mut grid: FxHashMap<(i32, i32), Vec<usize>> = FxHashMap::default();
    for (i, &((x, z), _)) in players.iter().enumerate() {
        grid.entry((x.div_euclid(cell), z.div_euclid(cell))).or_default().push(i);
    }
    let mut out = Vec::new();
    for (i, &((x, z), a)) in players.iter().enumerate() {
        let (gx, gz) = (x.div_euclid(cell), z.div_euclid(cell));
        for dx in -1..=1 {
            for dz in -1..=1 {
                for &j in grid.get(&(gx + dx, gz + dz)).map_or(&[][..], Vec::as_slice) {
                    if j <= i {
                        continue;
                    }
                    let ((ox, oz), b) = players[j];
                    if a != b && (x - ox).abs() <= reach && (z - oz).abs() <= reach {
                        out.push((a, b));
                    }
                }
            }
        }
    }
    out
}

/// Region games asked for in one tick at most (`tick_world_regions_rest`), each made on a thread of its own.
const NEW_GAMES_A_TICK: usize = 8;
/// Region games kept past the most a level needed lately (`land_flight`).
const REGION_GAMES_SPARE: usize = 2;
/// How long the most region games a level needed is remembered: 30 s.
const REGION_GAMES_NEED_TICKS: u64 = 600;
/// Region games dropped past the need so far (the region log line).
static REGION_GAMES_DROPPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static REGION_GAMES_MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Why the resident regions came back, counted for the region log line: the period, a request (`resident_land`:
/// a player left), a region that wants the server (`wants_land`), players seeing past their region, a late one.
static RESIDENT_LANDS: [std::sync::atomic::AtomicU64; 7] = [const { std::sync::atomic::AtomicU64::new(0) }; 7];
/// Fast ticks run.
static RESIDENT_TICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn count_land(reason: usize) {
    RESIDENT_LANDS[reason].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Ticks between the resident regions' returns (`HORIZON_RESIDENT_TICKS`; 20, once a second, or 100 under one clock).
fn resident_period() -> u32 {
    static N: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("HORIZON_RESIDENT_TICKS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            // One clock: every 5 s (400 spread players: a landing re-splits the level, one tick of ~140 ms; every
            // 40 ticks that put 2.7% of the ticks there, P99 140 ms; every 100: P99 61 ms, the average as fast).
            .unwrap_or(if shared_clock() { 100 } else { 20 })
            .max(1)
    })
}

/// Whether finished flight `f` may stay on its side as a resident region (it ran its players' whole turn, its tick
/// went well, and it has nothing for the server's thread).
fn may_stay(f: &Flight, blocked: bool) -> bool {
    resident_on() && !blocked && f.whole && f.result.as_ref().is_some_and(|r| r.is_ok()) && f.game.as_ref().is_some_and(|w| !wants_land(w))
}

/// Whether region game `w` has something for the server's thread (and so must come back): its players' events,
/// commands, menus, a packet it left in an inbox (one reaching past the region), a player dead or leaving.
fn wants_land(w: &Game<'_>) -> bool {
    land_reason(w).is_some()
}

/// [`wants_land`]'s reason, as `RESIDENT_LANDS` counts it: 2 events, commands or menus; 5 a packet waiting in an
/// inbox; 6 a player dead or leaving.
fn land_reason(w: &Game<'_>) -> Option<usize> {
    if !w.region_host_events.is_empty() || !w.level_teleports.is_empty() {
        return Some(2);
    }
    let h = w.lan.as_ref()?;
    if !h.commands.is_empty() || !h.quiet.is_empty() || !h.hub.native_menu_events.is_empty() {
        return Some(2);
    }
    if h.hub.guests.iter().any(|g| g.dead || g.health <= 0.0 || !g.left_reason.is_empty()) {
        return Some(6);
    }
    // Gathered, their players' packets for the server's thread are handled there while the region is parked.
    if h.hub.waits_for_server() && !gather_on() {
        return Some(5);
    }
    None
}

impl Game<'_> {
    /// Whether every resident region of the level in place is still seen only by its own players, and they see
    /// only it: no player of another region (or of none) has one of its chunks in view (and its ring).
    fn residents_seen_by_own(&self, view: i32) -> bool {
        use horizon_client::fast_hash::FxHashMap;
        let dim = self.dim;
        // Each resident region's sections (residents kept at different ticks come from different splits).
        let residents: Vec<(usize, &Flight)> = self
            .region_flights
            .iter()
            .filter(|f| f.resident && f.dim == dim && f.game.is_some())
            .enumerate()
            .collect();
        if residents.is_empty() {
            return true;
        }
        if resident_edge_on() {
            // `resident_edge_on`'s rule: no two players of different regions (a resident's, or the server's) so
            // close that one sees where the other's region ticks.
            let sim = self.settings.simulation_distance.max(2) as i32;
            let reach = view + 1 + sim + 1;
            let mut players: Vec<((i32, i32), Option<usize>)> = Vec::new();
            for &(k, f) in &residents {
                if let Some(host) = f.game.as_ref().and_then(|w| w.lan.as_ref()) {
                    players.extend(host.hub.guests.iter().map(|g| (g.chunk(), Some(k))));
                }
            }
            if let Some(h) = self.lan.as_ref() {
                players.extend(h.hub.guests.iter().filter(|g| !g.visiting && g.dimension == dim.id()).map(|g| (g.chunk(), None)));
            }
            return close_owners(&players, reach).iter().all(|(a, b)| a.is_none() && b.is_none());
        }
        let mut owner: FxHashMap<(i32, i32), usize> = FxHashMap::default();
        for &(k, f) in &residents {
            if let Some((regions, r)) = f.layout.as_ref() {
                for (&section, &of) in &regions.owned {
                    if of == *r {
                        owner.entry(section).or_insert(k);
                    }
                }
            }
        }
        let reach = view + 1;
        // (chunk, own resident) of every player: the residents' own, and the server's here.
        let mut players: Vec<((i32, i32), Option<usize>)> = Vec::new();
        for &(k, f) in &residents {
            if let Some(host) = f.game.as_ref().and_then(|w| w.lan.as_ref()) {
                players.extend(host.hub.guests.iter().map(|g| (g.chunk(), Some(k))));
            }
        }
        if let Some(h) = self.lan.as_ref() {
            players.extend(h.hub.guests.iter().filter(|g| !g.visiting && g.dimension == dim.id()).map(|g| (g.chunk(), None)));
        }
        players.iter().all(|&((cx, cz), own)| {
            let (x0, x1) = ((cx - reach) >> SECTION_SHIFT, (cx + reach) >> SECTION_SHIFT);
            let (z0, z1) = ((cz - reach) >> SECTION_SHIFT, (cz + reach) >> SECTION_SHIFT);
            // A player sees no other resident region than its own (chunks no resident owns are fine: they are
            // the server's, or nobody's and don't tick).
            (x0..=x1).all(|sx| (z0..=z1).all(|sz| owner.get(&(sx, sz)).is_none_or(|o| Some(*o) == own)))
        })
    }
}

/// Where resident flight `f`'s players stand now (their areas stay loaded and ticking while it is away).
fn resident_places(f: &Flight) -> Vec<(i32, i32)> {
    f.game
        .as_ref()
        .and_then(|w| w.lan.as_ref())
        .map(|h| h.hub.guests.iter().map(|g| g.chunk()).collect())
        .unwrap_or_default()
}

/// Whether each region tracks its own entities for its own players on its thread, after their chunks (Folia's), in a
/// tick when every region owns all its players see: on, with the region chunks, unless `HORIZON_REGION_TRACK=0`.
pub(super) fn region_track_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| region_chunks_on() && std::env::var("HORIZON_REGION_TRACK").map_or(true, |v| v.trim() != "0"))
}

/// Whether each region sends its own players' chunks on its thread after its tick, when it owns every chunk they
/// see (always with `region_views_on`; without, when the simulation distance's chunks and their ring reach past the
/// view, as at 7 and 10): off unless `HORIZON_REGION_CHUNKS=1` (300 spread players: 8 ms off the server thread, as
/// much more on the regions; no faster yet).
pub(super) fn region_chunks_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| resident_on() || std::env::var("HORIZON_REGION_CHUNKS").is_ok_and(|v| v.trim() == "1"))
}

/// Whether a player not in region `f` has one of its chunks in view (`view` chunks around them).
fn seen_by_others(f: &Flight, guests: &[(u32, (i32, i32))], view: i32) -> bool {
    let Some(first) = f.chunks.first() else {
        return false;
    };
    let (mut x0, mut z0, mut x1, mut z1) = (first.x, first.z, first.x, first.z);
    for c in &f.chunks {
        x0 = x0.min(c.x);
        z0 = z0.min(c.z);
        x1 = x1.max(c.x);
        z1 = z1.max(c.z);
    }
    let near = |&(cx, cz): &(i32, i32)| cx >= x0 - view && cx <= x1 + view && cz >= z0 - view && cz <= z1 + view;
    let mut owned: Option<horizon_client::fast_hash::FxHashSet<(i32, i32)>> = None;
    guests.iter().filter(|g| !f.conns.contains(&g.0) && near(&g.1)).any(|&(_, (cx, cz))| {
        let owned = owned.get_or_insert_with(|| f.chunks.iter().map(|c| (c.x, c.z)).collect());
        (cx - view..=cx + view).any(|x| (cz - view..=cz + view).any(|z| owned.contains(&(x, z))))
    })
}

/// For the region log line: ticks the level ticked as one region (fewer than two), and the chunks and mobs
/// the last split left on the server thread.
static LEFTOVER: std::sync::Mutex<(u64, usize, usize)> = std::sync::Mutex::new((0, 0, 0));

/// Whether region threads handle their own players' packets (Folia's): on, unless `HORIZON_REGION_PACKETS=0`
/// (every packet on the server's thread before the regions, as before).
pub(super) fn region_packets_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_REGION_PACKETS").map_or(true, |v| v.trim() != "0"))
}

/// Whether the End ticks by region once its dragon fight is over (Folia regionizes every level): on, unless
/// `HORIZON_END_REGIONS=0`.
fn end_regions_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_END_REGIONS").map_or(true, |v| v.trim() != "0"))
}

impl<'a> Game<'a> {
    /// Whether this level's world phases may tick by region this tick (the network is then read before the
    /// regions are lent, and each region handles its own players' packets).
    pub(super) fn regions_may_run(&self) -> bool {
        // Plugins listening on what the world phases do hear it from the region thread that did it, with that
        // region's game lent to them, one thread at a time in their code (the plugin runtime's turns): Folia's
        // way. `HORIZON_REGION_PLUGINS=0` keeps such a level on one thread instead.
        let plugins = !region_plugins_on()
            && (super::v2_plugin_block_hooks()
                || super::creatures::plugins_hear_mobs()
                || super::natural_spawning::plugins_hear_spawns());
        region_threads_on()
            && !plugins
            && self.lan.is_some()
            && !self.region_worker
            && (self.dim != super::Dim::End || end_regions_on())
    }

    /// Whether this tick reads the network first and leaves the packets to the region threads.
    pub(super) fn packets_by_region(&self) -> bool {
        region_packets_on() && self.regions_may_run()
    }

    /// The world phases of a tick by region, Folia's way: the level split into its regions, each region's
    /// part lent to a region game, every region ticked at once (each on whichever thread is free: blocks,
    /// entities, mobs, items, experience, falling blocks, TNT, block entities), and the parts taken back in
    /// region order. False when it did nothing (one region, no players, the End's dragon fight): the
    /// level's own phases run then.
    pub(super) fn tick_world_regions(&mut self) -> Result<bool> {
        // Gathered players back to their regions before anything decides whether they go out again.
        self.return_residents();
        // Pipelined: the residents sent out last tick ran through its end; this tick waits for them now.
        if resident_on() && resident_pipeline() {
            self.resident_wait();
        }
        // HorizonsX's resident regions: ticked again where they are (unless they all have to come back), while the
        // rest of the level ticks by region as usual; back on their side once done.
        let launched = if resident_on()
            && self.regions_may_run()
            && self.region_flights.iter().any(|f| f.resident && f.dim == self.dim)
        {
            self.resident_launch()?
        } else {
            None
        };
        self.residents_out = launched.is_some();
        self.resident_players = match &launched {
            Some((flights, _)) => flights.iter().flat_map(|f| f.places.iter().copied()).collect(),
            None => Vec::new(),
        };
        let result = self.tick_world_regions_rest();
        if let Some((flights, deadline)) = launched {
            // The level's world update while the residents finish (`world_update_due`).
            if std::mem::take(&mut self.world_update_due) {
                horizon_client::spans::time("rr_world_update", || self.update_world());
            }
            if resident_pipeline() {
                // They tick on while this tick does the rest of its work (`resident_wait` next tick).
                self.region_flights.extend(flights);
            } else {
                self.resident_collect(flights, deadline);
            }
        }
        self.residents_out = false;
        // Parked until the next tick: their players gathered here meanwhile.
        horizon_client::spans::time("rr_gather", || self.gather_residents());
        // Not run meanwhile (the residents came back this tick): now.
        if std::mem::take(&mut self.world_update_due) {
            self.update_world();
        }
        result
    }

    /// Whether the server's thread has players' packets or commands waiting, which may read or change any place of
    /// the level (a respawn's spawn point, a teleport, a command's blocks): the resident regions come back first.
    fn server_work_waits(&self) -> bool {
        self.lan.as_ref().is_some_and(|h| {
            h.hub.any_for_server() || !h.commands.is_empty() || !h.quiet.is_empty() || !h.hub.native_menu_events.is_empty()
        }) || !self.level_teleports.is_empty()
    }

    /// Whether the level in place has resident regions to tick this tick (its world update then waits for them).
    pub(super) fn residents_waiting(&self) -> bool {
        resident_on() && self.region_flights.iter().any(|f| f.resident && f.dim == self.dim)
    }

    /// [`Game::tick_world_regions`] for what the server's game holds (all of the level but its resident regions).
    fn tick_world_regions_rest(&mut self) -> Result<bool> {
        if !self.regions_may_run() || (self.dim == super::Dim::End && self.end_fight_active()) {
            self.land_region_flights(false)?;
            return Ok(false);
        }
        // Packets or commands for the server's thread: every region back by the end of this tick.
        self.keep_blocked = self.server_work_waits();
        // The chunks that tick blocks, and the players' own: what makes a section active (Folia's regions
        // are their players' areas). A region owns a section's ring around its active ones, which covers the
        // entity-ticking chunks further out; anything left outside every region ticks on this game, at the
        // same time as the regions.
        let levels = horizon_client::spans::time("rg_levels", || self.tickets.levels());
        let mut active: Vec<(i32, i32)> = self
            .world
            .chunks
            .keys()
            .copied()
            .filter(|&(x, z)| levels.random_ticking(x, z))
            .collect();
        let guests: Vec<(u32, (i32, i32))> = self
            .lan
            .as_ref()
            .map(|h| {
                h.hub
                    .guests
                    .iter()
                    .filter(|g| !g.visiting && g.dimension == self.dim.id())
                    .map(|g| (g.conn, ((g.position[0].floor() as i32) >> 4, (g.position[2].floor() as i32) >> 4)))
                    .collect()
            })
            .unwrap_or_default();
        // Folia's regions hold the chunks their players load (their whole view), so players whose views touch
        // are one region: walking players a few hundred blocks apart end as one region for the whole level,
        // ticked on one thread. What a tick needs is less: the chunks that tick (the active sections above) and
        // a ring to read around them. With `region_views_on` off (the default) regions are made of those, and a
        // player's view may reach into another region's chunks: a region that runs late while other players
        // see its chunks is waited for (`seen_by_others`), so nobody's view ever has a hole.
        let view_distance = self.lan.as_ref().map_or(10, |h| h.hub.config.view_distance);
        // The view and the ring of chunks around it a sent chunk's light and readiness read.
        let view = if region_views_on() { view_distance + 1 } else { 0 };
        let places: Vec<(i32, i32)> = guests.iter().map(|g| g.1).collect();
        // `HORIZON_REGION_DIAG=1`: a region with many players is logged (a few a minute): its players' and ticking
        // chunks' extents, to see what joined them.
        let diag_active: Option<Vec<(i32, i32)>> = region_diag_on().then(|| active.clone());
        let regions = std::sync::Arc::new(horizon_client::spans::time("rg_plan", || {
            Regions::with_players(active, &places, view).bucketed(region_buckets(), &places)
        }));
        if let Some(active) = diag_active {
            static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
            let due = LAST.lock().is_ok_and(|mut l| {
                let due = l.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(5));
                if due {
                    *l = Some(std::time::Instant::now());
                }
                due
            });
            if due {
                let mut players: Vec<Vec<(i32, i32)>> = vec![Vec::new(); regions.count];
                for &p in &places {
                    if let Some(r) = regions.of_chunk(p) {
                        players[r].push(p);
                    }
                }
                for (r, list) in players.iter().enumerate() {
                    if list.len() <= 25 {
                        continue;
                    }
                    let mine: Vec<(i32, i32)> = active.iter().copied().filter(|&c| regions.of_chunk(c) == Some(r)).collect();
                    let bbox = |v: &[(i32, i32)]| {
                        v.iter().fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |b, &(x, z)| {
                            (b.0.min(x), b.1.min(z), b.2.max(x), b.3.max(z))
                        })
                    };
                    let mut homes: Vec<(i32, i32)> = list.iter().map(|&(x, z)| (x.div_euclid(32), z.div_euclid(32))).collect();
                    homes.sort_unstable();
                    homes.dedup();
                    // Players more than 6 chunks from a multiple of 32 (the ramp's homes are 512 blocks apart).
                    let off = |v: i32| (v - (v + 16).div_euclid(32) * 32).abs();
                    let strays: Vec<(i32, i32)> =
                        list.iter().copied().filter(|&(x, z)| off(x) > 6 || off(z) > 6).take(40).collect();
                    let conns: Vec<String> = guests
                        .iter()
                        .filter(|g| strays.contains(&g.1))
                        .take(10)
                        .map(|g| format!("{}@{:?}", g.0, g.1))
                        .collect();
                    log::info!("HorizonsX region diag: strays {strays:?} conns {conns:?}");
                    log::info!(
                        "HorizonsX region diag: region {r} of {}: {} players in {} 512-block cells, players' chunks {:?}, {} ticking chunks {:?}; level players {}, active {}",
                        regions.count,
                        list.len(),
                        homes.len(),
                        bbox(list),
                        mine.len(),
                        bbox(&mine),
                        places.len(),
                        active.len()
                    );
                }
            }
        }
        {
            static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            if CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 200 == 0 {
                let (_, slowest, average, late) = region_report();
                let (one, kept_chunks, kept_mobs) = LEFTOVER.lock().map_or((0, 0, 0), |l| *l);
                let (games, need) = self.region_games_kept();
                log::info!(
                    "HorizonsX region threads: {} regions, {} players; last tick slowest {slowest:.1} ms, average {average:.1} ms; {late} late so far; {one} ticks as one region; left on the server thread last tick: {kept_chunks} chunks, {kept_mobs} mobs; region games kept {games}, needed lately {need}, dropped {}; made {}, flights out {}",
                    regions.count,
                    guests.len(),
                    REGION_GAMES_DROPPED.load(std::sync::atomic::Ordering::Relaxed),
                    REGION_GAMES_MADE.load(std::sync::atomic::Ordering::Relaxed),
                    self.region_flights.len()
                );
                if resident_on() {
                    let n = |i: usize| RESIDENT_LANDS[i].load(std::sync::atomic::Ordering::Relaxed);
                    log::info!(
                        "HorizonsX resident regions: {} fast ticks; back for the period {}, a request {}, events {}, inbox {}, dead or leaving {}, views {}, late {}",
                        RESIDENT_TICKS.load(std::sync::atomic::Ordering::Relaxed),
                        n(0),
                        n(1),
                        n(2),
                        n(5),
                        n(6),
                        n(3),
                        n(4)
                    );
                }
                if let Ok(mut worst) = SLOWEST_REGION.lock()
                    && worst.0 > 0.0
                {
                    let phases: Vec<String> = worst.3.iter().map(|(n, ms)| format!("{n} {ms:.1}")).collect();
                    log::info!(
                        "HorizonsX slowest region since: {:.1} ms, {} chunks, {} players: {}",
                        worst.0, worst.1, worst.2, phases.join(", ")
                    );
                    *worst = (0.0, 0, 0, Vec::new());
                }
            }
        }
        if regions.count < 2 {
            if let Ok(mut l) = LEFTOVER.lock() {
                l.0 += 1;
            }
            super::health::record_regions(self.dim.id(), Vec::new());
            self.land_region_flights(false)?;
            return Ok(false);
        }
        // The level's own chunk management first (as `tick_blocks` starts): region games skip it.
        horizon_client::spans::time("rg_chunks", || {
            self.tickets.tick_portal();
            self.sync_tick_chunks();
            self.start_ticking_chunks();
        });
        // A region game for each region (made once for each level, kept). HorizonsX: a few more a tick at most
        // (one takes a while to make; resident regions hold theirs): short of them, this tick's level ticks here.
        horizon_client::spans::time("rg_new_games", || self.collect_region_games());
        // What this level needs at once: this split's regions and the resident ones out with theirs. Noted every
        // split, also one short of games: noted only once they were made, the need lagged the players joining,
        // and the games landing were dropped just before more were made.
        {
            let dim = self.dim;
            let out = self.region_flights.iter().filter(|f| f.dim == dim).count();
            let need = regions.count + out;
            let ticks = self.ticks;
            let seen = self.region_games_need.entry(dim).or_insert((0, 0));
            if need >= seen.0 || ticks.saturating_sub(seen.1) > REGION_GAMES_NEED_TICKS {
                *seen = (need, ticks);
            }
        }
        let have = self.region_games.get(&self.dim).map_or(0, Vec::len);
        if have < regions.count {
            // Short of games: they are made on their own threads meanwhile, and this tick's level ticks here.
            let coming = self.region_game_factory.len();
            // HorizonsX: never more games than the level needed lately: the games still out with regions come back
            // (residents landing for their period are split again before theirs are kept), and made anew for them
            // the level held 580 games for 125 regions (each with its chunk workers).
            // Every game in being counts (kept, out with a region, being made): made less dropped. Counted where
            // they are, the regions waited on in this tick's split were in none of those places.
            let dim = self.dim;
            let live = self.region_games_live.get(&dim).copied().unwrap_or(0);
            let cap = self.region_games_need.get(&dim).map_or(regions.count, |n| n.0.max(regions.count)) + REGION_GAMES_SPARE;
            let room = cap.saturating_sub(live);
            let wanted = (regions.count - have).saturating_sub(coming).min(NEW_GAMES_A_TICK).min(room);
            if wanted > 0 {
                self.request_region_games(wanted);
            }
            self.land_region_flights(false)?;
            return Ok(false);
        }
        // Entity ids: one shared counter, a block of it for each game (this one's too).
        let pool = self
            .entity_pool
            .get_or_insert_with(|| std::sync::Arc::new(std::sync::atomic::AtomicU32::new(self.next_entity + 1)))
            .clone();
        if self.entity_id_end == u32::MAX {
            self.entity_id_end = self.next_entity;
        }
        self.refill_entity_ids(&pool);
        // The regions' worlds log their block and light changes for their players: the server's hub stops logging
        // its own while no player of the level is here (all of them in resident regions), and a region's world
        // logs as the level's did when it was split (without, its players' digs were never sent to them).
        if !guests.is_empty() {
            if self.world.block_log.is_none() {
                self.world.block_log = Some(Vec::new());
            }
            if self.world.light_log.is_none() {
                self.world.light_log = Some(Default::default());
            }
        }
        let LevelSplit { parts, chunks } = horizon_client::spans::time("rg_split", || self.split_level(&regions));
        if let Ok(mut l) = LEFTOVER.lock() {
            (l.1, l.2) = (self.world.chunks.len(), self.mobs.len());
        }
        // HorizonsX: room for every lent mob before they come back (appended region by region, the list grew
        // again and again: a mob is 3 kB).
        let lent_mobs: usize = parts.iter().map(|p| p.mobs.len()).sum();
        self.mobs.reserve(lent_mobs);
        let mut games: Vec<Box<Game<'static>>> = {
            let kept = self.region_games.get_mut(&self.dim).expect("made above");
            let from = kept.len() - regions.count;
            kept.drain(from..).collect()
        };
        let mut host = self.lan.take().expect("checked above");
        // The proto chunks the regions' chunk senders ask about, copied once for all of them.
        let protos = (region_chunks_on() && host.hub.far.is_none())
            .then(|| horizon_client::spans::time("rd_protos", || std::sync::Arc::new(self.streaming.proto_snapshot())));
        // The tracked entities' states, each to the region its entity was last seen in (a dedicated server's: an
        // integrated one's host player is tracked on this game).
        // Which regions own every chunk their players see (the view, and the ring a sent chunk's light reads):
        // those send their players' chunks themselves. When all of them do (and no player is in none), their
        // block and light changes and entities go from them too: nobody else sees their chunks.
        // HorizonsX: and which regions nobody else sees into: those send their block and light changes and track
        // their entities themselves too (`full`), and may stay resident; the others' players are this thread's.
        let mut sees_own = vec![protos.is_some(); regions.count];
        let mut seen_by_other = vec![false; regions.count];
        let edge = resident_edge_on() && protos.is_some();
        if edge {
            // `resident_edge_on`: a player conflicts with a region whose player's simulation distance its view reaches.
            let sim = self.settings.simulation_distance.max(2) as i32;
            let reach = view_distance + 1 + sim + 1;
            // This split's players (owner: their region) and the resident regions' (owner: none of this split's,
            // marked apart).
            let mut players: Vec<((i32, i32), (Option<usize>, bool))> =
                guests.iter().map(|&(_, c)| (c, (regions.of_chunk(c), false))).collect();
            players.extend(self.resident_players.iter().map(|&c| (c, (None, true))));
            for (a, b) in close_owners(&players, reach) {
                for ((mine, _), (theirs, theirs_resident)) in [(a, b), (b, a)] {
                    // `mine` sees where `theirs` ticks.
                    if let Some(m) = mine {
                        sees_own[m] = false;
                    }
                    if let Some(o) = theirs {
                        seen_by_other[o] = true;
                    } else if theirs_resident && let Some(m) = mine {
                        // A resident region's player close by: this one may not stay out beside it.
                        seen_by_other[m] = true;
                    }
                }
            }
        }
        if protos.is_some() && !edge {
            let reach = view_distance + 1;
            for (_, (cx, cz)) in &guests {
                let mine = regions.of_chunk((*cx, *cz));
                let (x0, x1) = ((cx - reach) >> SECTION_SHIFT, (cx + reach) >> SECTION_SHIFT);
                let (z0, z1) = ((cz - reach) >> SECTION_SHIFT, (cz + reach) >> SECTION_SHIFT);
                for sx in x0..=x1 {
                    for sz in z0..=z1 {
                        let owner = regions.of_chunk((sx << SECTION_SHIFT, sz << SECTION_SHIFT));
                        // Chunks no region owns don't tick: nothing there changes while the regions are out (their
                        // columns go out from this thread's world, `update_chunks`).
                        if let Some(o) = owner
                            && Some(o) != mine
                        {
                            if let Some(m) = mine {
                                sees_own[m] = false;
                            }
                            seen_by_other[o] = true;
                        }
                    }
                }
            }
        }
        // The resident regions' players (out this tick, not among this split's) see into no region that stays.
        if protos.is_some() && !edge {
            let reach = view_distance + 1;
            for &(cx, cz) in &self.resident_players {
                let (x0, x1) = ((cx - reach) >> SECTION_SHIFT, (cx + reach) >> SECTION_SHIFT);
                let (z0, z1) = ((cz - reach) >> SECTION_SHIFT, (cz + reach) >> SECTION_SHIFT);
                for sx in x0..=x1 {
                    for sz in z0..=z1 {
                        if let Some(o) = regions.of_chunk((sx << SECTION_SHIFT, sz << SECTION_SHIFT)) {
                            seen_by_other[o] = true;
                        }
                    }
                }
            }
        }
        let full: Vec<bool> = (0..regions.count).map(|r| sees_own[r] && !seen_by_other[r]).collect();
        let all_own = protos.is_some() && full.iter().all(|f| *f);
        let mut trackers = (full.iter().any(|f| *f) && region_track_on() && host.hub.config.dedicated)
            .then(|| horizon_client::spans::time("rd_trackers", || host.hub.lend_trackers(regions.count, |c| regions.of_chunk(c))));
        let trackers_lent = trackers.is_some();
        {
            static SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            if SEEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 200 == 0 && protos.is_some() {
                let own = sees_own.iter().filter(|o| **o).count();
                let whole = full.iter().filter(|o| **o).count();
                log::info!("HorizonsX region network turns: {own} of {} regions send their own chunks, {whole} their whole turn (all: {all_own})", regions.count);
            }
        }
        let mut conns_of: Vec<Vec<u32>> = Vec::with_capacity(regions.count);
        // Each guest's region, and every region's guests split off in one pass.
        let region_of_conn: horizon_client::fast_hash::FxHashMap<u32, usize> =
            guests.iter().filter_map(|g| Some((g.0, regions.of_chunk(g.1)?))).collect();
        let mut split_guests = horizon_client::spans::time("rd_guests", || {
            host.hub.split_guests(regions.count, |conn| region_of_conn.get(&conn).copied())
        });
        let lend_started = horizon_client::spans::enabled().then(std::time::Instant::now);
        for (r, (part, w)) in parts.into_iter().zip(games.iter_mut()).enumerate() {
            let mob_ids: Vec<u32> = part.mobs.iter().map(|m| m.id).collect();
            horizon_client::spans::time("rd_game", || self.lend_region(w, part));
            w.copy_tick_settings_from(self);
            w.region_protos = if sees_own[r] { protos.clone() } else { None };
            w.region_full_turn = full[r] && trackers.is_some();
            // One that may stay resident runs its players' own turn there too (the server's thread would else
            // run it for every region kept: 190 ms with 600 players).
            w.resident_turn = resident_on() && w.region_full_turn;
            w.player_turn_done = false;
            w.refill_entity_ids(&pool);
            let lent_guests = std::mem::take(&mut split_guests[r]);
            let conns: Vec<u32> = lent_guests.iter().map(|g| g.conn).collect();
            let mut shell = w.lan.take().unwrap_or_else(|| host.region_shell());
            horizon_client::spans::time("rd_host", || host.lend_region(&mut shell, &conns, &mob_ids, self.dim, Some(lent_guests)));
            // HorizonsX: the players' damage records go with them (`leave_guest` keeps each in the game it left;
            // kept in the region games, every game ended up holding a record for each player that passed through:
            // 94 MB at 600 spread players).
            for conn in &conns {
                if let Some(t) = self.guest_combat.remove(conn) {
                    w.guest_combat.insert(*conn, t);
                }
            }
            shell.tracked = None;
            if let Some(trackers) = trackers.as_mut() {
                shell.hub.set_tracker(std::mem::take(&mut trackers[r]));
            }
            // Last tick's tracked entities standing in the region: what its players' moves and places
            // collide with while its thread handles their packets.
            // Shared, not copied: nothing changes them until the regions are back.
            if !conns.is_empty() && region_packets_on() {
                shell.states = std::sync::Arc::clone(&host.states);
            }
            w.lan = Some(shell);
            conns_of.push(conns);
        }
        if let Some(t) = lend_started {
            horizon_client::spans::record("rg_lend", t.elapsed());
        }
        // Every region at once, each on a region thread of its own (Folia's: it ticks on whichever thread is
        // free), and what no region owns here meanwhile (with the players no region holds).
        let deadline = region_wait_end(std::time::Instant::now());
        let mut flights: Vec<Flight> = games
            .into_iter()
            .zip(chunks)
            .zip(conns_of)
            .enumerate()
            .map(|(r, ((w, chunks), conns))| {
                // The test's slow region: the one at the world's origin (none while that one is away).
                let slow = regions.of_chunk((0, 0)) == Some(r);
                let lag = if slow { lag_test() } else { std::time::Duration::ZERO };
                let mut f = Flight::launch(self.dim, w, chunks, conns, lag, regions.firsts[r]);
                f.layout = Some((std::sync::Arc::clone(&regions), r));
                f.whole = full[r] && trackers_lent;
                f
            })
            .collect();
        let dim = self.dim.id();
        self.lan = Some(host);
        // The packets of the players no region holds, here, before this game's block ticks.
        self.tick_lan_packets();
        let leftover = horizon_client::spans::time("rg_leftover", || {
            horizon_client::block_contact::in_level(Some(dim), || self.tick_region_phases())
        });
        // Back, in region order, every region that finished by the deadline. A slower one keeps ticking on its
        // own (Folia's lag isolation): its players and chunks wait for it, the rest of the level goes on, and
        // it comes back when it is done (`land_region_flights`), having missed the ticks it was late for.
        let mut errors = Vec::new();
        let (mut slowest, mut total, mut late) = (0.0f32, 0.0f32, 0u64);
        let count = flights.len();
        let mut stats = Vec::with_capacity(count);
        for mut f in flights.drain(..) {
            // A region that ran past its last tick isn't waited for (Folia: nothing waits for a slow region):
            // it comes back when it is done, and is waited for again once it keeps time.
            let slow = self.slow_regions.contains(&(self.dim, f.first))
                || f.conns.iter().any(|c| self.slow_players.contains(c));
            let mut done = f.wait_until(if slow { std::time::Instant::now() } else { deadline });
            // A late region whose chunks other players see is waited for: lag isolation would take chunks out
            // of their views while it is away.
            if !done && !region_views_on() && seen_by_others(&f, &guests, view_distance) {
                done = f.wait();
            }
            stats.push(super::health::RegionStat {
                level: dim.to_owned(),
                at: (f.first.0 << (SECTION_SHIFT + 4), f.first.1 << (SECTION_SHIFT + 4)),
                chunks: f.chunks.len(),
                players: f.conns.len(),
                ms: if done { f.took } else { region_deadline().as_secs_f32() * 1000.0 },
                late: !done,
            });
            slowest = slowest.max(if done { f.took } else { region_deadline().as_secs_f32() * 1000.0 });
            total += f.took;
            late += u64::from(!done);
            if done {
                if slow {
                    self.slow_regions.remove(&(self.dim, f.first));
                    for c in &f.conns {
                        self.slow_players.remove(c);
                    }
                }
                // HorizonsX's resident regions: one that owns all its players see stays where it is.
                if may_stay(&f, self.keep_blocked) {
                    self.keep_resident(f);
                    continue;
                }
                if let Some(e) = horizon_client::spans::time("rg_land", || self.land_flight(f)) {
                    errors.push(e);
                }
            } else {
                self.slow_regions.insert((self.dim, f.first));
                self.slow_players.extend(f.conns.iter().copied());
                if let Some(h) = self.lan.as_mut() {
                    h.hub.away.extend(f.conns.iter().copied());
                }
                self.streaming.away.extend(f.chunks.iter().map(|c| (c.x, c.z)));
                f.places = guests.iter().filter(|g| f.conns.contains(&g.0)).map(|g| g.1).collect();
                log::warn!(
                    "HorizonsX: a region of {} ({} chunks, {} players) runs past the tick; the rest of the server goes on",
                    dim,
                    f.chunks.len(),
                    f.conns.len()
                );
                self.region_flights.push(f);
            }
        }
        {
            let mut times = REGION_TIMES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            *times = (count, slowest, total / count.max(1) as f32, times.3 + late);
        }
        if let Some(h) = self.lan.as_mut() {
            horizon_client::spans::time("rl_order", || h.hub.restore_guest_order());
        }
        super::health::record_regions(dim, stats);
        REGIONS_LANDED.store(clock_ns(), std::sync::atomic::Ordering::Relaxed);
        // Regions that ran past an earlier tick and are done now come back after this tick's world phases, so
        // their players get this tick's network turn before their region goes out again.
        let landed = self.land_region_flights(false);
        self.flush_spill_after_level();
        // What the region threads left in their players' inboxes (chat, commands, respawns...) is handled by
        // this tick's network turn (`tick_lan`), each player's turn going on where its region stopped.
        if region_packets_on() {
            self.level_pass.packets_drained = false;
        }
        leftover?;
        landed?;
        if let Some(e) = errors.into_iter().next() {
            return Err(e.into());
        }
        Ok(true)
    }

    /// A finished region game's part of the level back in place, with its players; the game is kept for the
    /// next tick. Its error, if its tick failed.
    fn land_flight(&mut self, mut f: Flight) -> Option<String> {
        let result = f.result.take().unwrap_or(Ok(()));
        // A thread that never answered took the region's part with it: nothing comes back.
        let Some(mut w) = f.game.take() else {
            // Its game went with it.
            self.region_games_live.entry(f.dim).and_modify(|n| *n = n.saturating_sub(1));
            for conn in &f.conns {
                if let Some(host) = self.lan.as_mut() {
                    host.hub.away.remove(conn);
                }
            }
            return result.err();
        };
        // The region's own random sequences are dropped: the level's continue (`RandomSequences::join`).
        let _ = self.take_region_back(&mut w, &f.chunks);
        if !self.streaming.away.is_empty() {
            horizon_client::spans::time("rl_away", || {
                for c in &f.chunks {
                    self.streaming.away.remove(&(c.x, c.z));
                }
            });
        }
        if let Some(mut shell) = w.lan.take() {
            if let Some(host) = self.lan.as_mut() {
                for conn in &f.conns {
                    host.hub.away.remove(conn);
                }
                horizon_client::spans::time("rl_host", || host.take_region_back(&mut shell));
            }
            // The shared states let go, so the server's tick changes its own in place.
            shell.states = Default::default();
            w.lan = Some(shell);
        }
        // What its players' packets left for the server (an anticheat ban), now that they are back.
        if !w.region_host_events.is_empty()
            && let Some(host) = self.lan.take()
        {
            let events = std::mem::take(&mut w.region_host_events);
            let spawn = self.spawn.map_or([0.5, 80.0, 0.5], |s| [s.x, s.y, s.z]);
            if let Some(host) = self.lan_process_events(host, events, None, &[], spawn) {
                self.lan = Some(host);
            }
        }
        // HorizonsX: kept only while the level may need it. Resident regions hold their games, so a region that
        // appeared while they were out was given a new one; kept for good, the games outgrew the regions (747 games
        // for 93 regions at 400 players, each with its tables and chunk caches: about 2 GB).
        let dim = f.dim;
        let kept = self.region_games.get(&dim).map_or(0, Vec::len);
        let out = self.region_flights.iter().filter(|g| g.dim == dim).count();
        let need = self.region_games_need.get(&dim).map_or(usize::MAX, |n| n.0);
        if kept + out >= need.saturating_add(REGION_GAMES_SPARE) {
            REGION_GAMES_DROPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.region_games_live.entry(dim).and_modify(|n| *n = n.saturating_sub(1));
            // Freed on a thread of its own: a game's tables are many allocations.
            let _ = std::thread::Builder::new().name("HorizonsX region game drop".into()).spawn(move || drop(w));
        } else {
            // What its busiest tick grew it to goes before it waits in the pool (`HORIZON_TRIM_REGION_GAMES=1`: measured
            // only with the shared games above, so off by default).
            static TRIM: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            if *TRIM.get_or_init(|| std::env::var("HORIZON_TRIM_REGION_GAMES").is_ok_and(|v| v.trim() == "1")) {
                w.mobs.shrink_to_fit();
                if let Some(shell) = w.lan.as_mut() {
                    shell.trim();
                }
            }
            self.region_games.entry(dim).or_default().push(w);
        }
        result.err()
    }

    /// The region games kept for a level and the most it needed lately (the region log line).
    fn region_games_kept(&self) -> (usize, usize) {
        (
            self.region_games.get(&self.dim).map_or(0, Vec::len),
            self.region_games_need.get(&self.dim).map_or(0, |n| n.0),
        )
    }

    /// HorizonsX's resident regions: a finished region kept on its side (its part of the level and players stay
    /// in its game), its players and chunks away from the server's game as a late region's are.
    fn keep_resident(&mut self, mut f: Flight) {
        f.resident = true;
        // This tick's turn of its players, here: from now on its own thread runs it.
        if let Some(w) = f.game.as_mut()
            && !std::mem::take(&mut w.player_turn_done)
        {
            horizon_client::spans::time("rr_first_turn", || w.resident_player_turn());
        }
        if let Some(h) = self.lan.as_mut() {
            h.hub.away.extend(f.conns.iter().copied());
        }
        self.streaming.away.extend(f.chunks.iter().map(|c| (c.x, c.z)));
        f.places = resident_places(&f);
        self.region_flights.push(f);
    }

    /// The level's resident regions launched for one more tick where they are (Folia's region ticks: no split, no
    /// landing): their players' packets given to them and the tick's settings, each on its thread with its players'
    /// whole turn. `None` when they came back instead (every `resident_period` ticks, when one asked, one ran
    /// late, or someone sees into one): the usual tick then takes the whole level.
    fn resident_launch(&mut self) -> Result<Option<(Vec<Flight>, std::time::Instant)>> {
        let dim = self.dim;
        let view_distance = self.lan.as_ref().map_or(10, |h| h.hub.config.view_distance);
        self.resident_ticks += 1;
        let mut land = None;
        if self.resident_ticks >= resident_period() {
            land = Some(0);
        } else if self.resident_land || self.server_work_waits() {
            land = Some(1);
        }
        let mut late = 0usize;
        for f in self.region_flights.iter_mut().filter(|f| f.resident && f.dim == dim) {
            // A late one keeps going (Folia: nothing waits for a slow region), and is ticked again once done.
            if !f.finished() {
                late += 1;
                continue;
            }
            if land.is_none() {
                if !f.result.as_ref().is_some_and(|r| r.is_ok()) {
                    land = Some(2);
                } else if let Some(reason) = f.game.as_ref().map_or(Some(2), |w| land_reason(w)) {
                    land = Some(reason);
                }
            }
        }
        if late > 0 {
            count_land(4);
        }
        if land.is_none() && !horizon_client::spans::time("rr_check", || self.residents_seen_by_own(view_distance)) {
            land = Some(3);
        }
        if let Some(reason) = land {
            count_land(reason);
            self.resident_ticks = 0;
            self.resident_land = false;
            let mut errors = Vec::new();
            let started = horizon_client::spans::enabled().then(std::time::Instant::now);
            let mut i = 0;
            while i < self.region_flights.len() {
                if self.region_flights[i].resident && self.region_flights[i].dim == dim {
                    // A late one isn't waited for: it comes back once done (`land_region_flights`).
                    if !self.region_flights[i].finished() {
                        self.region_flights[i].resident = false;
                        self.region_flights[i].whole = false;
                        i += 1;
                        continue;
                    }
                    let mut f = self.region_flights.remove(i);
                    f.resident = false;
                    // Back before this tick's turn: its players have theirs on the server's thread.
                    if let Some(h) = f.game.as_mut().and_then(|w| w.lan.as_mut()) {
                        for g in &mut h.hub.guests {
                            g.turn_done = false;
                        }
                    }
                    if let Some(e) = self.land_flight(f) {
                        errors.push(e);
                    }
                } else {
                    i += 1;
                }
            }
            if let Some(h) = self.lan.as_mut() {
                h.hub.restore_guest_order();
            }
            self.flush_spill_after_level();
            if let Some(t) = started {
                horizon_client::spans::record("rr_land", t.elapsed());
            }
            if let Some(e) = errors.into_iter().next() {
                return Err(e.into());
            }
            return Ok(None);
        }
        // Entity ids, the proto chunks the chunk senders ask about, the server's hub for the settings.
        let pool = self
            .entity_pool
            .get_or_insert_with(|| std::sync::Arc::new(std::sync::atomic::AtomicU32::new(self.next_entity + 1)))
            .clone();
        self.refill_entity_ids(&pool);
        let protos = horizon_client::spans::time("rd_protos", || std::sync::Arc::new(self.streaming.proto_snapshot()));
        let Some(host) = self.lan.take() else {
            return Ok(None);
        };
        let mut host = host;
        let started = horizon_client::spans::enabled().then(std::time::Instant::now);
        let mut flights = Vec::new();
        let mut i = 0;
        while i < self.region_flights.len() {
            if !(self.region_flights[i].resident && self.region_flights[i].dim == dim) || !self.region_flights[i].finished() {
                i += 1;
                continue;
            }
            let f = self.region_flights.remove(i);
            let Flight { game, chunks, conns, first, layout, whole, places, .. } = f;
            let Some(mut w) = game else {
                continue;
            };
            w.copy_tick_settings_from(self);
            w.refill_entity_ids(&pool);
            w.world.game_time = self.world.game_time;
            w.world.day_time = self.world.day_time;
            w.world.weather = self.world.weather;
            w.region_protos = Some(std::sync::Arc::clone(&protos));
            w.region_full_turn = true;
            w.resident_turn = true;
            if let Some(shell) = w.lan.as_mut() {
                shell.refresh_from(&host);
                // A new tick's turn for each player (`Hub::poll` starts it for the players on the server's hub; these
                // never pass through it while their region stays out): without, a resident player's packets were
                // handled only in the tick after each landing, their budget spent for good until the next.
                // (Gathered players went through it this tick already.)
                if !gather_on() {
                    for g in &mut shell.hub.guests {
                        g.drained = false;
                    }
                }
                if !horizon_client::spans::time("rr_give", || host.hub.give_held(&mut shell.hub)) {
                    self.resident_land = true;
                }
            }
            let mut f = Flight::launch(dim, w, chunks, conns, std::time::Duration::ZERO, first);
            f.layout = layout;
            f.whole = whole;
            f.places = places;
            f.resident = true;
            flights.push(f);
        }
        if let Some(t) = started {
            horizon_client::spans::record("rr_launch", t.elapsed());
        }
        self.lan = Some(host);
        RESIDENT_TICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(Some((flights, region_wait_end(std::time::Instant::now()))))
    }

    /// HorizonsX: the players of the resident regions parked on this thread gathered into the server's hub
    /// ([`gather_on`], `Guest::visiting`), with their turns' state and models, and what their regions left for the
    /// server's thread (commands, plugin menu events): until `return_residents`, chat reaches them, their chat and
    /// commands are handled here, and the plugins see them, without their regions coming back.
    pub(super) fn gather_residents(&mut self) {
        if !gather_on() {
            return;
        }
        let dim = self.dim;
        let Some(mut host) = self.lan.take() else {
            return;
        };
        let mut gathered = false;
        for f in self.region_flights.iter_mut().filter(|f| f.resident && f.dim == dim && f.visiting.is_empty()) {
            // A late one keeps its players until it is done.
            if !f.finished() {
                continue;
            }
            let Some(shell) = f.game.as_mut().and_then(|w| w.lan.as_mut()) else {
                continue;
            };
            if shell.hub.guests.is_empty() {
                continue;
            }
            let mut guests = std::mem::take(&mut shell.hub.guests);
            for g in &mut guests {
                g.visiting = true;
                f.visiting.push(g.conn);
                host.hub.away.remove(&g.conn);
            }
            for conn in &f.visiting {
                if let Some(p) = shell.play.remove(conn) {
                    host.play.insert(*conn, p);
                }
                if let Some(m) = shell.models.remove(conn) {
                    host.models.insert(*conn, m);
                }
            }
            host.commands.append(&mut shell.commands);
            host.quiet.extend(shell.quiet.drain(..));
            host.hub.native_menu_events.append(&mut shell.hub.native_menu_events);
            host.hub.guests.append(&mut guests);
            gathered = true;
        }
        if gathered {
            host.hub.restore_guest_order();
        }
        self.lan = Some(host);
    }

    /// HorizonsX: a plugin's question (`{"op": ..., ...}`, answered by `answer`) put to the game that holds what
    /// it names: a `pos` or `chunk` in a resident region parked on this thread is that region's to answer; an
    /// entity `id` this game doesn't know is asked of the parked regions too. Everything else here.
    pub(super) fn answer_where_held(&mut self, request: &str, answer: for<'x> fn(&mut Game<'x>, &str) -> String) -> String {
        if !gather_on() || self.region_worker || !self.region_flights.iter().any(|f| f.resident && f.game.is_some()) {
            return answer(self, request);
        }
        let Ok(query) = serde_json::from_str::<serde_json::Value>(request) else {
            return answer(self, request);
        };
        if query["dimension"].as_str().is_some_and(|d| d != self.dim.id()) {
            return answer(self, request);
        }
        let int = |v: &serde_json::Value| v.as_f64().map(|n| n.floor() as i32);
        let chunk = match (&query["pos"], &query["chunk"]) {
            (serde_json::Value::Array(p), _) if p.len() == 3 => int(&p[0]).zip(int(&p[2])).map(|(x, z)| (x >> 4, z >> 4)),
            (_, serde_json::Value::Array(c)) if c.len() == 2 => int(&c[0]).zip(int(&c[1])),
            _ => None,
        };
        let dim = self.dim;
        if let Some(chunk) = chunk
            && !self.world.chunks.contains_key(&chunk)
        {
            for f in self.region_flights.iter_mut().filter(|f| f.resident && f.dim == dim) {
                if let Some(w) = f.game.as_mut()
                    && w.world.chunks.contains_key(&chunk)
                {
                    return answer(w, request);
                }
            }
        }
        let here = answer(self, request);
        if !query["id"].is_null() && here == "null" {
            for f in self.region_flights.iter_mut().filter(|f| f.resident && f.dim == dim) {
                if let Some(w) = f.game.as_mut() {
                    let there = answer(w, request);
                    if there != "null" {
                        return there;
                    }
                }
            }
        }
        here
    }

    /// The gathered players ([`Game::gather_residents`]) back to their parked regions, where they stand now (a
    /// command may have moved one: its region's views check sends it back to the server if so). A player who left
    /// meanwhile is gone from both.
    pub(super) fn return_residents(&mut self) {
        if !self.region_flights.iter().any(|f| !f.visiting.is_empty()) {
            return;
        }
        let Some(mut host) = self.lan.take() else {
            return;
        };
        // Each gathered player's flight, then one pass over the hub's guests (a pass per region was 140 x 700
        // players moved a tick).
        let mut owner: horizon_client::fast_hash::FxHashMap<u32, usize> = Default::default();
        for (k, f) in self.region_flights.iter_mut().enumerate() {
            for conn in std::mem::take(&mut f.visiting) {
                owner.insert(conn, k);
            }
        }
        let mut back: Vec<Vec<horizon_client::lan::guest::Guest>> = (0..self.region_flights.len()).map(|_| Vec::new()).collect();
        let mut kept = Vec::with_capacity(host.hub.guests.len());
        for g in std::mem::take(&mut host.hub.guests) {
            match owner.get(&g.conn) {
                Some(&k) if g.visiting => back[k].push(g),
                _ => kept.push(g),
            }
        }
        host.hub.guests = kept;
        let mut moved_level = false;
        for (k, guests) in back.into_iter().enumerate() {
            let f = &mut self.region_flights[k];
            if !owner.values().any(|&o| o == k) {
                continue;
            }
            let level = f.dim.id();
            let Some(shell) = f.game.as_mut().and_then(|w| w.lan.as_mut()) else {
                for mut g in guests {
                    g.visiting = false;
                    host.hub.guests.push(g);
                }
                continue;
            };
            for mut g in guests {
                g.visiting = false;
                // Sent to another level meanwhile: the region comes back at its launch.
                if g.dimension != level {
                    moved_level = true;
                }
                if let Some(p) = host.play.remove(&g.conn) {
                    shell.play.insert(g.conn, p);
                }
                if let Some(m) = host.models.remove(&g.conn) {
                    shell.models.insert(g.conn, m);
                }
                host.hub.away.insert(g.conn);
                shell.hub.guests.push(g);
            }
            shell.hub.restore_guest_order();
            let present: horizon_client::fast_hash::FxHashSet<u32> = shell.hub.guests.iter().map(|g| g.conn).collect();
            f.conns.retain(|c| present.contains(c));
            f.places = resident_places(f);
        }
        host.hub.restore_guest_order();
        self.lan = Some(host);
        if moved_level {
            self.resident_land = true;
        }
    }

    /// Pipelined residents ([`resident_pipeline`]): the ones of the level in place still out, waited for until the
    /// tick's region deadline (a late one keeps going, and comes back once done), their players' places noted.
    fn resident_wait(&mut self) {
        let dim = self.dim;
        if !self.region_flights.iter().any(|f| f.resident && f.dim == dim) {
            return;
        }
        let started = horizon_client::spans::enabled().then(std::time::Instant::now);
        let deadline = region_wait_end(std::time::Instant::now());
        for f in self.region_flights.iter_mut().filter(|f| f.resident && f.dim == dim) {
            if f.wait_until(deadline) {
                f.places = resident_places(f);
            }
        }
        if let Some(t) = started {
            horizon_client::spans::record("rr_wait", t.elapsed());
        }
    }

    /// The resident regions launched this tick back on their side ([`Game::resident_launch`]), each waited for
    /// until `deadline` (a late one keeps going, and comes back once done).
    fn resident_collect(&mut self, flights: Vec<Flight>, deadline: std::time::Instant) {
        let started = horizon_client::spans::enabled().then(std::time::Instant::now);
        for mut f in flights {
            if f.wait_until(deadline) {
                f.places = resident_places(&f);
            }
            self.region_flights.push(f);
        }
        if let Some(t) = started {
            horizon_client::spans::record("rr_wait", t.elapsed());
        }
    }

    /// The regions of the level in place that ran past an earlier tick, back once they are done (all of them,
    /// waiting, when `wait`: before a save or a stop).
    pub(super) fn land_region_flights(&mut self, wait: bool) -> Result<()> {
        if self.region_flights.is_empty() {
            return Ok(());
        }
        if wait {
            self.return_residents();
        }
        let mut errors = Vec::new();
        let mut landed = false;
        let mut i = 0;
        while i < self.region_flights.len() {
            // A resident region comes back only when settling (or from `resident_tick`).
            let here = self.region_flights[i].dim == self.dim && (wait || !self.region_flights[i].resident);
            let done = here && {
                let f = &mut self.region_flights[i];
                if wait { f.wait() } else { f.finished() }
            };
            if done {
                let f = self.region_flights.remove(i);
                // A late region that owns all its players see stays on its side once done.
                if !wait && may_stay(&f, self.keep_blocked) {
                    self.slow_regions.remove(&(f.dim, f.first));
                    for c in &f.conns {
                        self.slow_players.remove(c);
                    }
                    self.keep_resident(f);
                    continue;
                }
                landed = true;
                // Back within a tick's wait: it keeps time again.
                if f.took <= region_deadline().as_secs_f32() * 1000.0 {
                    self.slow_regions.remove(&(f.dim, f.first));
                    for c in &f.conns {
                        self.slow_players.remove(c);
                    }
                }
                if let Some(e) = self.land_flight(f) {
                    errors.push(e);
                }
            } else {
                i += 1;
            }
        }
        if landed {
            if let Some(h) = self.lan.as_mut() {
                h.hub.restore_guest_order();
            }
            self.flush_spill_after_level();
        }
        match errors.into_iter().next() {
            Some(e) => Err(e.into()),
            None => Ok(()),
        }
    }

    /// Every region still running, in every level, waited for and back (before a save or a stop).
    pub(super) fn settle_region_flights(&mut self) -> Result<()> {
        if self.region_flights.is_empty() {
            return Ok(());
        }
        self.return_residents();
        let back = self.dim;
        let mut dims: Vec<super::Dim> = self.region_flights.iter().map(|f| f.dim).collect();
        dims.dedup();
        let mut result = Ok(());
        for dim in dims {
            if let Err(e) = self.enter_level(None, dim) {
                result = Err(e);
                break;
            }
            if let Err(e) = self.land_region_flights(true) {
                result = Err(e);
            }
        }
        self.enter_level(None, back)?;
        result
    }

    /// The areas of the players in the level in place's regions still running (`lan_update_streaming`).
    pub(super) fn region_flight_areas(&self, radius: i32) -> Vec<((i32, i32), i32)> {
        self.region_flights
            .iter()
            .filter(|f| f.dim == self.dim)
            .flat_map(|f| f.places.iter().map(move |&c| (c, radius)))
            .collect()
    }

    /// The players of the regions still on their threads (resident or running late), in every level.
    pub(super) fn away_players(&self) -> usize {
        // Gathered players are in the server's hub meanwhile (counted there).
        self.region_flights.iter().map(|f| f.conns.len().saturating_sub(f.visiting.len())).sum()
    }

    /// The levels with a region still running (they tick until it is back).
    pub(super) fn region_flight_levels(&self) -> impl Iterator<Item = super::Dim> + '_ {
        self.region_flights.iter().map(|f| f.dim)
    }

    /// The rest of `tick_world` once the regions ticked the world phases: what reaches past a region or
    /// belongs to the whole server (Folia's global region): players' maps and compasses, portals, natural
    /// spawning and the custom spawners, raids, the scoreboard, advancements and boss bars, the network,
    /// sleep, the End, sculk, the block entities' client animations and game tests. In `tick_world`'s order.
    pub(super) fn tick_world_after_regions(&mut self) -> Result<()> {
        horizon_client::spans::time("cool_light", || self.cool_light());
        horizon_client::spans::time("tick_maps", || self.tick_maps());
        self.tick_lodestone_compasses();
        horizon_client::spans::time("entity_portals", || {
            self.handle_entity_portals();
            self.handle_end_gateway_portals();
        });
        horizon_client::spans::time("spawn_loaded_entities", || self.spawn_loaded_entities());
        self.restore_root_vehicle();
        horizon_client::spans::time("tick_mobs", || -> Result<()> {
            self.attack_ticks = self.attack_ticks.saturating_add(1);
            self.tick_leads();
            if self.ui.rules.bool("spawn_mobs") {
                self.tick_wandering_trader();
                self.tick_patrol_spawner();
                self.tick_cat_spawner();
                self.tick_village_siege();
            }
            self.tick_raids();
            self.position_rider();
            self.tick_stats();
            self.tick_scoreboard();
            self.load_bossbars();
            self.tick_worldborder();
            self.tick_subtitles();
            self.spawn_phantoms();
            self.tick_advancements();
            self.tick_player_model();
            Ok(())
        })?;
        horizon_client::spans::time("entity_portals", || {
            self.find_entity_portals();
            self.resolve_arrivals();
        });
        horizon_client::spans::time("tick_lan", || self.tick_lan(false));
        self.tick_friends();
        horizon_client::spans::time("tick_hand", || self.tick_hand());
        horizon_client::spans::time("tick_sleep", || self.tick_sleep());
        self.announce_sleep_status();
        self.free_left_beds();
        horizon_client::spans::time("tick_portal", || self.tick_nether_portal())?;
        horizon_client::spans::time("tick_end", || self.tick_end())?;
        horizon_client::spans::time("end_gateways", || {
            self.find_end_gateway_portals();
            self.resolve_end_gateway_arrivals()
        })?;
        self.keep_gateway_destinations();
        horizon_client::spans::time("tick_sculk_entities", || self.tick_sculk_entities());
        horizon_client::spans::time("tick_sculk_blocks", || self.tick_sculk_blocks())?;
        horizon_client::spans::time("tick_block_entity_anims", || self.tick_block_entity_anims());
        horizon_client::spans::time("tick_particles", || self.tick_particles());
        self.tick_client_ambient();
        self.tick_game_tests();
        self.publish_boss_bars();
        Ok(())
    }

    /// What a region game ticks of the level lent to it ([`Game::tick_world_regions`]): its blocks, its
    /// entities, its mobs' own ticks, its items and orbs, its falling blocks and TNT, its block entities.
    /// Its natural spawning too. Raids, the custom spawners, portals, sculk and the network run on the server's
    /// game after the regions.
    pub(super) fn tick_region_phases(&mut self) -> Result<()> {
        // A region game's players' packets first, on its own thread (before its block ticks, as vanilla).
        if self.region_worker && region_packets_on() {
            phase("region_packets", || self.drain_region_packets());
        }
        self.cache_players(true);
        let result = self.tick_region_phases_cached();
        self.cache_players(false);
        // Its players' network turn, from its own chunks and entities, before it goes back (Folia's).
        if self.region_worker && self.region_protos.is_some() {
            let resident = std::mem::take(&mut self.resident_turn);
            self.region_network_turn(resident);
            // A resident region's players' own turn too: the server's thread doesn't see them.
            if resident {
                phase("resident_players", || self.resident_player_turn());
            }
        }
        // A resident region holds its players' whole view: most of its chunks tick nowhere.
        if self.region_worker {
            phase("cool_light", || self.cool_light());
        }
        result
    }

    /// A region game's players' network turn, after its tick, in the server's order for them: the block events,
    /// their chunks ([`Hub::region_chunks`](horizon_client::lan::hub::Hub::region_chunks)), the block and light
    /// changes, then the entities (their states built here, the events of those gone first, the tracker given
    /// to this region at the split).
    fn region_network_turn(&mut self, resident: bool) {
        let Some(protos) = self.region_protos.take() else {
            return;
        };
        let Some(mut host) = self.lan.take() else {
            return;
        };
        if host.hub.guests.is_empty() {
            self.lan = Some(host);
            return;
        }
        let holding = horizon_client::lan::Holding::start();
        let full = std::mem::take(&mut self.region_full_turn);
        // `runBlockEvents`' broadcast precedes the tick's block changes.
        if full {
            for (pos, b0, b1, state) in std::mem::take(&mut self.lan_block_events) {
                host.hub.block_event(pos, b0, b1, state);
            }
        }
        phase("region_chunks", || host.hub.region_chunks(&self.world, &protos));
        if full {
            phase("region_changes", || host.hub.region_changes(&mut self.world));
            phase("region_track", || self.region_track(&mut host, resident));
        }
        drop(holding);
        self.lan = Some(host);
    }

    /// The region's entities tracked for its players ([`Hub::region_track`](horizon_client::lan::hub::Hub::region_track)):
    /// the server's `tick_lan` steps for them, on this region's own entities.
    fn region_track(&mut self, host: &mut super::lan::LanHost, resident: bool) {
        self.lan_assign_ids();
        let guests: Vec<(u32, i32)> = host.hub.guests.iter().filter_map(|g| Some((g.vehicle?, g.entity_id))).collect();
        self.lan_riding(None, &guests, &mut host.hub.riding);
        // A resident region's own states of last tick (its players' packets read them), else the ones kept here.
        let mut states = if resident {
            std::sync::Arc::try_unwrap(std::mem::take(&mut host.states)).unwrap_or_default()
        } else {
            host.tracked.take().unwrap_or_default()
        };
        self.lan_entities(None, &host.mob_players, &mut host.hub, &mut states);
        self.lan_hook_states(None, &host.play, &host.hub, &mut states);
        self.lan_entity_uuids(&mut states);
        for m in &mut self.mobs {
            m.needs_sync = false;
        }
        for p in &mut self.creatures.projectiles {
            p.needs_sync = false;
        }
        for d in &mut self.dropped {
            d.needs_sync = false;
        }
        // An entity that left since the last tick has its event sent before the tracker drops it.
        if !self.lan_entity_events.is_empty() {
            let ids: horizon_client::fast_hash::FxHashSet<i32> = states.as_slice().iter().map(|e| e.id).collect();
            let (gone, later): (Vec<_>, Vec<_>) =
                std::mem::take(&mut self.lan_entity_events).into_iter().partition(|(id, ..)| !ids.contains(id));
            self.lan_entity_events = later;
            for (id, event, at) in gone {
                host.hub.entity_event(id, event, at);
            }
        }
        let spawn = self.spawn.map_or([0.5, 80.0, 0.5], |s| [s.x, s.y, s.z]);
        let clock_rate = self.network_clock_rate();
        let mut ctx = horizon_client::lan::hub::TickCtx {
            world: &mut self.world,
            host: None,
            entities: states.as_slice(),
            next_entity: &mut self.next_entity,
            spawn,
            difficulty: i32::from(self.ui.difficulty),
            difficulty_locked: self.level.difficulty_locked,
            clock_rate,
            tick_rate: self.clock.rate(),
            tick_frozen: self.clock.frozen(),
            tick_steps: self.clock.step_state(),
            collision: Some(self.collision),
            border: &self.border,
            streaming: None,
        };
        host.hub.region_track(&mut ctx);
        // Staying out, its players' packets read these next tick; going back, the server's tick takes them.
        if resident {
            host.states = std::sync::Arc::new(states);
        } else {
            host.tracked = Some(states);
        }
    }

    fn tick_region_phases_cached(&mut self) -> Result<()> {
        phase("tick_blocks", || self.tick_blocks())?;
        phase("tick_paintings", || self.tick_paintings());
        phase("tick_item_frames", || self.tick_item_frames());
        phase("tick_armor_stands", || self.tick_armor_stands());
        phase("tick_cushions", || self.tick_cushions());
        phase("tick_boats", || self.tick_boats());
        phase("tick_carts", || self.tick_carts());
        phase("tick_thunder", || self.tick_thunder());
        phase("tick_lecterns", || self.tick_lecterns());
        phase("tick_bolts", || self.tick_bolts());
        phase("tick_fishing", || self.tick_fishing());
        phase("tick_tridents", || self.tick_tridents());
        phase("tick_rockets", || self.tick_rockets());
        phase("tick_ominous_item_spawners", || self.tick_ominous_item_spawners());
        let before = self.dropped_ids();
        phase("tick_items", || self.tick_items());
        self.native_items_left_since(&before);
        phase("touch_arrows", || self.touch_arrows());
        // `ServerChunkCache.tickChunks`' natural spawning in the region's chunks, around its players, against
        // caps from its own spawnable chunks and mobs (Folia: spawning is a region's; the caps scale with the
        // chunks, so the regions' add up to the level's).
        phase("spawn_natural", || self.spawn_natural());
        phase("tick_creature_entities", || self.tick_creature_entities())?;
        phase("recheck_golem_openers", || self.recheck_golem_openers());
        let before = self.orb_ids();
        phase("tick_experience", || self.tick_experience());
        self.native_orbs_left_since(&before);
        phase("tick_falling", || self.tick_falling())?;
        phase("tick_tnt", || self.tick_tnt())?;
        phase("tick_block_entities", || self.tick_block_entities())?;
        Ok(())
    }
}

/// One region's part of a level, split out of the server's game ([`Game::split_level`]).
pub(super) struct RegionPart {
    /// The chunks of the part already scanned for creaking hearts.
    scanned: horizon_client::fast_hash::FxHashSet<(i32, i32)>,
    world: horizon_client::world::World,
    ticks: horizon_world::level::Ticks,
    sections: horizon_client::random_tick_sections::RandomTickSections,
    sequences: horizon_client::random_sequences::RandomSequences,
    mobs: Vec<Mob>,
    dropped: Vec<super::ItemEntity>,
    orbs: Vec<super::ExperienceOrb>,
    projectiles: Vec<horizon_client::projectiles::Projectile>,
    falling: Vec<super::FallingBlock>,
    paintings: Vec<horizon_client::entities::Painting>,
    item_frames: Vec<horizon_client::entities::ItemFrame>,
    armor_stands: Vec<horizon_client::entities::ArmorStand>,
    cushions: Vec<horizon_client::entities::Cushion>,
    others: Vec<super::commands::OtherEntity>,
    boats: Vec<super::boats::Boat>,
    carts: Vec<super::minecarts::Minecart>,
    bolts: Vec<super::lightning::Bolt>,
    crystals: Vec<super::dragon::Crystal>,
    dragon_fireballs: Vec<super::dragon::DragonFireball>,
    breath_clouds: Vec<super::dragon::Breath>,
    tridents: Vec<super::trident::ThrownTrident>,
    rockets: Vec<super::fireworks::Rocket>,
    ominous_spawners: Vec<super::ominous_item_spawner::OminousItemSpawner>,
    tnts: Vec<super::tnt::PrimedTnt>,
    fangs: Vec<super::fangs::Fangs>,
    hoppers: HashMap<(i32, i32, i32), super::machines::HopperState>,
    jukebox_sounds: HashMap<(i32, i32, i32), u64>,
    lid_openers: HashMap<(i32, i32, i32), i32>,
    knot_ids: HashMap<[i32; 3], u32>,
    lectern_pulses: Vec<((i32, i32, i32), i32)>,
    anims: horizon_client::block_entity_anim::BlockEntityAnims,
}

/// A level split into its regions' parts, and the chunks each region holds (for the block ticks' return).
pub(super) struct LevelSplit {
    pub parts: Vec<RegionPart>,
    pub chunks: Vec<Vec<ChunkPos>>,
}

impl<'a> Game<'a> {
    /// The level in place split into `regions`' parts (Folia's per-region data): each region's chunks,
    /// block entities, ticks and entities moved out of this game's level; what no region owns stays.
    pub(super) fn split_level(&mut self, regions: &Regions) -> LevelSplit {
        let n = regions.count;
        let of_chunk = |key: (i32, i32)| regions.of_chunk(key);
        let mut chunks: Vec<Vec<ChunkPos>> = vec![Vec::new(); n];
        for &(x, z) in self.world.chunks.keys() {
            if let Some(r) = of_chunk((x, z)) {
                chunks[r].push(ChunkPos::new(x, z));
            }
        }
        let seeds: Vec<i64> = (0..n)
            .map(|_| horizon_core::random::RandomSource::next_long(&mut self.world.random))
            .collect();
        let tick_seeds: Vec<i32> = seeds.iter().map(|s| *s as i32).collect();
        // HorizonsX: the world's chunks, the scheduled ticks and the random-tick sections are lent at once, each on
        // a thread of its own (each touches only its own part of the level; one after another they were most of
        // the split's 11 ms with 300 spread players).
        let (world, block_ticks, sections_of) = (&mut self.world, &mut self.block_ticks, &mut self.random_tick_sections);
        let (chunks_ref, seeds_ref, tick_seeds_ref) = (&chunks, &seeds, &tick_seeds);
        // The entities are sorted below meanwhile, on this thread.
        let parts = std::thread::scope(|scope| {
            let timed = horizon_client::spans::enabled();
            // Timed on their threads, recorded here (the spans are this thread's).
            // Favoured as the tick threads are: resident regions fill every core meanwhile, and a split thread
            // at normal priority waited behind them (817 ms for 100 ms of work).
            let worlds = scope.spawn(move || {
                horizon_threads::favour_current();
                let t = timed.then(std::time::Instant::now);
                (world.lend(n, of_chunk, seeds_ref), t.map(|t| t.elapsed()))
            });
            let ticks = scope.spawn(move || {
                horizon_threads::favour_current();
                let t = timed.then(std::time::Instant::now);
                (block_ticks.lend(chunks_ref, tick_seeds_ref), t.map(|t| t.elapsed()))
            });
            let t = timed.then(std::time::Instant::now);
            let sections = sections_of.lend(n, of_chunk);
            if let Some(t) = t {
                horizon_client::spans::record("rs_sections", t.elapsed());
            }
            let mut sequences = self.rand_sequences.split(n + 1);
            // The level's own sequences stay with the level (the first of the split).
            self.rand_sequences = sequences.remove(0);
            let v3 = |p: &horizon_core::geometry::Vec3| (p.x, p.z);
            let a3 = |p: &[f64; 3]| (p[0], p[2]);
            let b3 = |p: &[i32; 3]| (f64::from(p[0]), f64::from(p[2]));
            let entities_started = horizon_client::spans::enabled().then(std::time::Instant::now);
            let mut mobs = split(&mut self.mobs, regions, |m| v3(&m.position));
            let mut dropped = split(&mut self.dropped, regions, |d| v3(&d.position));
            let mut orbs = split(&mut self.orbs, regions, |o| v3(&o.position));
            let mut projectiles = split(&mut self.creatures.projectiles, regions, |p| v3(&p.position));
            let mut falling = split(&mut self.falling, regions, |f| v3(&f.position));
            let mut paintings = split(&mut self.paintings, regions, |p| b3(&p.pos));
            let mut item_frames = split(&mut self.item_frames, regions, |f| b3(&f.pos));
            let mut armor_stands = split(&mut self.armor_stands, regions, |s| v3(&s.position));
            let mut cushions = split(&mut self.cushions, regions, |c| v3(&c.position));
            let mut others = split(&mut self.others, regions, |o| v3(&o.position));
            let mut boats = split(&mut self.boats, regions, |b| v3(&b.position));
            let mut carts = split(&mut self.carts, regions, |c| v3(&c.position));
            let mut bolts = split(&mut self.bolts, regions, |b| a3(&b.at));
            let mut crystals = split(&mut self.crystals, regions, |c| v3(&c.position));
            let mut dragon_fireballs = split(&mut self.dragon_fireballs, regions, |f| v3(&f.position));
            let mut breath_clouds = split(&mut self.breath_clouds, regions, |b| a3(&b.at));
            let mut tridents = split(&mut self.tridents, regions, |t| v3(&t.position));
            let mut rockets = split(&mut self.rockets, regions, |r| v3(&r.position));
            let mut ominous_spawners = split(&mut self.ominous_spawners, regions, |o| a3(&o.position));
            let mut tnts = split(&mut self.tnts, regions, |t| v3(&t.position));
            let mut fangs = split(&mut self.fangs.list, regions, |f| a3(&f.pos));
            let mut hoppers = split_map(&mut self.hoppers, regions, |p| block_chunk(*p));
            let mut jukebox_sounds = split_map(&mut self.jukebox_sounds, regions, |p| block_chunk(*p));
            let mut lid_openers = split_map(&mut self.lid_openers, regions, |p| block_chunk(*p));
            let mut knot_ids = split_map(&mut self.knot_ids, regions, array_chunk);
            let mut scanned = self.creaking.split_scanned(n, |key| regions.of_chunk(key));
            let mut lectern_pulses: Vec<Vec<((i32, i32, i32), i32)>> = (0..n).map(|_| Vec::new()).collect();
            self.lectern_pulses.retain(|&(p, t)| match regions.of_chunk(block_chunk(p)) {
                Some(r) => {
                    lectern_pulses[r].push((p, t));
                    false
                }
                None => true,
            });
            let a = &mut self.be_anims;
            let mut chests = fx_split(&mut a.chests, regions, array_chunk);
            let mut books = fx_split(&mut a.books, regions, array_chunk);
            let mut shulkers = fx_split(&mut a.shulkers, regions, array_chunk);
            let mut bells = fx_split(&mut a.bells, regions, array_chunk);
            let mut conduits = fx_split(&mut a.conduits, regions, array_chunk);
            let mut skulls = fx_split(&mut a.skulls, regions, array_chunk);
            let mut pots = fx_split(&mut a.pots, regions, array_chunk);
            let mut gateway_servers = fx_split(&mut a.gateway_servers, regions, array_chunk);
            let mut conduit_servers = fx_split(&mut a.conduit_servers, regions, array_chunk);
            if let Some(t) = entities_started {
                horizon_client::spans::record("rs_entities", t.elapsed());
            }
            let ((worlds, world_took), (ticks, ticks_took)) = match (worlds.join(), ticks.join()) {
                (Ok(worlds), Ok(ticks)) => (worlds, ticks),
                (Err(e), _) | (_, Err(e)) => std::panic::resume_unwind(e),
            };
            if let Some(t) = world_took {
                horizon_client::spans::record("rs_world", t);
            }
            if let Some(t) = ticks_took {
                horizon_client::spans::record("rs_ticks", t);
            }
            let mut parts = Vec::with_capacity(n);
            for (r, ((world, ticks), sections)) in worlds.into_iter().zip(ticks).zip(sections).enumerate() {
                let take = |v: &mut Vec<_>| std::mem::take(&mut v[r]);
                parts.push(RegionPart {
                    world,
                    ticks,
                    sections,
                    sequences: sequences.remove(0),
                    mobs: std::mem::take(&mut mobs[r]),
                    dropped: std::mem::take(&mut dropped[r]),
                    orbs: std::mem::take(&mut orbs[r]),
                    projectiles: std::mem::take(&mut projectiles[r]),
                    falling: std::mem::take(&mut falling[r]),
                    paintings: std::mem::take(&mut paintings[r]),
                    item_frames: std::mem::take(&mut item_frames[r]),
                    armor_stands: std::mem::take(&mut armor_stands[r]),
                    cushions: std::mem::take(&mut cushions[r]),
                    others: std::mem::take(&mut others[r]),
                    boats: std::mem::take(&mut boats[r]),
                    carts: std::mem::take(&mut carts[r]),
                    bolts: std::mem::take(&mut bolts[r]),
                    crystals: std::mem::take(&mut crystals[r]),
                    dragon_fireballs: std::mem::take(&mut dragon_fireballs[r]),
                    breath_clouds: std::mem::take(&mut breath_clouds[r]),
                    tridents: std::mem::take(&mut tridents[r]),
                    rockets: std::mem::take(&mut rockets[r]),
                    ominous_spawners: std::mem::take(&mut ominous_spawners[r]),
                    tnts: std::mem::take(&mut tnts[r]),
                    fangs: std::mem::take(&mut fangs[r]),
                    hoppers: std::mem::take(&mut hoppers[r]),
                    jukebox_sounds: std::mem::take(&mut jukebox_sounds[r]),
                    lid_openers: std::mem::take(&mut lid_openers[r]),
                    knot_ids: std::mem::take(&mut knot_ids[r]),
                    lectern_pulses: take(&mut lectern_pulses),
                    scanned: std::mem::take(&mut scanned[r]),
                    anims: horizon_client::block_entity_anim::BlockEntityAnims {
                        chests: std::mem::take(&mut chests[r]),
                        books: std::mem::take(&mut books[r]),
                        shulkers: std::mem::take(&mut shulkers[r]),
                        bells: std::mem::take(&mut bells[r]),
                        conduits: std::mem::take(&mut conduits[r]),
                        skulls: std::mem::take(&mut skulls[r]),
                        pots: std::mem::take(&mut pots[r]),
                        gateway_servers: std::mem::take(&mut gateway_servers[r]),
                        conduit_servers: std::mem::take(&mut conduit_servers[r]),
                        ..Default::default()
                    },
                });
            }
            parts
        });
        LevelSplit { parts, chunks }
    }
}

/// HorizonsX: a 64th of the level's chunks a tick looked at by [`Game::cool_light`].
const COOL_SLICES: u64 = 64;

impl Game<'_> {
    /// HorizonsX: the light of chunks nothing ticks is kept compressed ([`horizon_client::chunk::Light::deflated`]),
    /// and thawed once they tick again. Those are most of a server's chunks (the view reaches past the simulation
    /// distance): about a sixth of the bytes for their light. Only a dedicated server's level does it, a slice a tick.
    fn cool_light(&mut self) {
        // Off unless `HORIZON_COOL_CHUNKS=1`: measured 11-09 with 600 spread players, the compressing cost 36-55 ms a
        // tick across the region games and private memory stayed 6.3-6.7 GB.
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*ON.get_or_init(|| std::env::var("HORIZON_COOL_CHUNKS").is_ok_and(|v| v.trim() == "1")) {
            return;
        }
        if self.options.dedicated.is_none() && !self.region_worker {
            return;
        }
        // Every 4th tick: a chunk that started ticking thaws within 4 ticks (a frozen one reads correctly meanwhile),
        // and compressing the chunks players' views reach cost 36 ms a tick across 120 regions done each tick.
        if self.ticks % 4 != 0 {
            return;
        }
        let levels = self.tickets.levels();
        let slice = (self.ticks / 4) % COOL_SLICES;
        let ticking = |x: i32, z: i32| levels.block_ticking(x, z);
        type Swap = (usize, Option<horizon_client::chunk::Light>, Option<horizon_core::paletted::PalettedContainer>);
        let mut changes: Vec<((i32, i32), Vec<Swap>)> = Vec::new();
        for (&(x, z), chunk) in self.world.chunks.iter() {
            let hot = ticking(x, z);
            // Ticking chunks are thawed every tick (a chunk a player came near ticks thawed); the rest are
            // looked at a slice a tick.
            if !hot && ((x.wrapping_mul(31) ^ z.wrapping_mul(17)) as u64) % COOL_SLICES != slice {
                continue;
            }
            if hot
                && !chunk
                    .sections
                    .iter()
                    .any(|s| matches!(s.light, horizon_client::chunk::Light::Deflated(_)) || s.blocks.is_frozen())
            {
                continue;
            }
            let near = hot || ticking(x + 1, z) || ticking(x - 1, z) || ticking(x, z + 1) || ticking(x, z - 1);
            let mut swaps = Vec::new();
            for (i, s) in chunk.sections.iter().enumerate() {
                let (light, blocks) = if hot {
                    (s.light.thawed(), s.blocks.thawed())
                } else if !near {
                    (s.light.deflated(), s.blocks.frozen())
                } else {
                    (None, None)
                };
                if light.is_some() || blocks.is_some() {
                    swaps.push((i, light, blocks));
                }
            }
            if !swaps.is_empty() {
                changes.push(((x, z), swaps));
            }
        }
        for (key, swaps) in changes {
            if let Some(chunk) = self.world.chunks.get_mut(&key) {
                let chunk = std::sync::Arc::make_mut(chunk);
                for (i, light, blocks) in swaps {
                    let section = std::sync::Arc::make_mut(&mut chunk.sections[i]);
                    if let Some(light) = light {
                        section.light = light;
                    }
                    if let Some(blocks) = blocks {
                        section.blocks = blocks;
                    }
                }
            }
        }
    }
}
