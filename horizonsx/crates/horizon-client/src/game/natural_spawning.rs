//! The game's side of `NaturalSpawner` (`mobs::spawn_selection`): the level
//! it reads, the spawning chunks and caps of `ServerChunkCache.tickChunks`,
//! and the mobs it adds (finalized with the game's `finalizeSpawn`).
use super::{Dim, Game};
use horizon_client::vanillagen::VanillaTerrain;
use horizon_client::{
    biome::{BiomeInfo, SpawnEntry},
    blocks::BlockFacts,
    collisions::{SceneCollisions, no_collision_empty},
    mobs::{
        Mob, MobWorld, Species,
        data::{self, MobData},
        seen_grid::SeenGrid,
        spawn_rules::{Dimension, SpawnLevel},
        spawn_selection::{
            self, Category, Counted, Heightmap, NaturalLevel, SpawnCost, SpawnPlayer, SpawnRequest,
            SpawnState, Spawned, StructureAt, StructureSpawns,
        },
    },
};
use horizon_core::{
    geometry::{Aabb, Vec3},
    random::{LegacyRandom, RandomSource, WorldgenRandom, XoroshiroRandom},
};
use horizon_world::{
    nbt::{Compound, Tag},
    registry::BlockStateId,
};
use std::collections::HashMap;

/// Reused buffers for one pass over the spawning chunks: the candidate chunks
/// and the set that keeps each one once. The buffers keep their capacity
/// between ticks.
#[derive(Default)]
pub(super) struct SpawnScratch {
    pub(super) candidates: Vec<(i32, i32)>,
    pub(super) seen: horizon_client::fast_hash::FxHashSet<(i32, i32)>,
}

/// A player in the level in place, as spawning and despawning see it.
#[derive(Clone)]
pub(super) struct LevelPlayer {
    pub(super) position: Vec3,
    pub(super) bounds: Aabb,
    pub(super) alive: bool,
    pub(super) creative: bool,
    pub(super) spectator: bool,
}
impl LevelPlayer {
    fn distance_sqr(&self, x: f64, y: f64, z: f64) -> f64 {
        (self.position.x - x).powi(2)
            + (self.position.y - y).powi(2)
            + (self.position.z - z).powi(2)
    }
    /// `Level.hasNearbyAlivePlayer`'s test of one player (any distance for a
    /// negative range).
    fn alive_near(&self, x: f64, y: f64, z: f64, range: f64) -> bool {
        self.alive && !self.spectator && (range < 0.0 || self.distance_sqr(x, y, z) < range * range)
    }
}
/// `ChunkMap.euclideanDistanceSquared` below 128 blocks
/// (`playerIsCloseEnoughForSpawning`).
fn close_enough_for_spawning(p: &LevelPlayer, (cx, cz): (i32, i32)) -> bool {
    let (x, z) = (
        f64::from(cx * 16 + 8) - p.position.x,
        f64::from(cz * 16 + 8) - p.position.z,
    );
    !p.spectator && x * x + z * z < 16384.0
}
/// `EntityGetter.getNearestPlayer(x, y, z, -1, false)`: the nearest
/// non-spectator, alive or not.
pub(super) fn nearest_player(
    players: &[LevelPlayer],
    x: f64,
    y: f64,
    z: f64,
) -> Option<&LevelPlayer> {
    let mut best: Option<(&LevelPlayer, f64)> = None;
    for p in players.iter().filter(|p| !p.spectator) {
        let d = p.distance_sqr(x, y, z);
        if best.is_none_or(|(_, b)| d < b) {
            best = Some((p, d));
        }
    }
    best.map(|(p, _)| p)
}

/// Spawn candidate chunks at which their checks run on every core (`spawning_chunks`).
const SPAWN_CANDIDATES_PARALLEL: usize = 2048;

/// The game level as natural spawning reads it.
pub(super) struct GameLevel<'g, 'a> {
    pub(super) game: &'g Game<'a>,
    pub(super) w: MobWorld<'g>,
    /// Every player in the level ([`Game::level_players`]).
    pub(super) players: std::sync::Arc<[LevelPlayer]>,
    /// Where the non-spectators among them are (`getNearestPlayer`).
    pub(super) nearest: Vec<[f64; 3]>,
    /// Boxes of the mobs this pass created, not yet in the game's list.
    pub(super) pending: std::cell::RefCell<Vec<Aabb>>,
    /// The types and boxes of what a spawner created this tick.
    pub(super) pending_ids: std::cell::RefCell<Vec<(String, Aabb)>>,
    /// The game's mobs by box, built once a pass has asked more than
    /// [`SCANNED_QUERIES`] box queries (the mobs do not change while it
    /// holds the game); the first ones scan, as the build costs a few scans.
    pub(super) boxes: std::cell::OnceCell<MobBoxes>,
    pub(super) box_queries: std::cell::Cell<u32>,
}
const SCANNED_QUERIES: u32 = 8;
/// A mob not removed: its index in `Game::mobs`, box, alive, and whether
/// it is a living shulker.
type MobBox = (usize, Aabb, bool, bool);
fn mob_box(i: usize, m: &Mob) -> MobBox {
    let alive = m.alive();
    (
        i,
        m.bounds(),
        alive,
        alive && m.species() == Species::Shulker,
    )
}
/// The boxes of the game's mobs not removed, and a grid over them.
pub(super) struct MobBoxes {
    entries: Vec<MobBox>,
    grid: SeenGrid,
}
impl MobBoxes {
    fn new(mobs: &[Mob]) -> Self {
        let entries: Vec<_> = mobs
            .iter()
            .enumerate()
            .filter(|(_, m)| !m.removed)
            .map(|(i, m)| mob_box(i, m))
            .collect();
        let grid = SeenGrid::new(entries.iter().map(|e| e.1));
        Self { entries, grid }
    }
    /// The entries whose boxes meet `area` (and possibly a few more).
    fn near(&self, area: Aabb) -> impl Iterator<Item = MobBox> + '_ {
        let mut near = Vec::new();
        let indexed = self.grid.candidates(area, &mut near);
        let all = (!indexed).then(|| self.entries.iter());
        near.into_iter()
            .map(|i| self.entries[i as usize])
            .chain(all.into_iter().flatten().copied())
    }
}
impl GameLevel<'_, '_> {
    /// The mobs not removed whose boxes may meet `area` (callers test them).
    fn mobs_near(&self, area: Aabb) -> Box<dyn Iterator<Item = MobBox> + '_> {
        let asked = self.box_queries.get() + 1;
        self.box_queries.set(asked);
        if asked <= SCANNED_QUERIES && self.boxes.get().is_none() {
            return Box::new(
                self.game
                    .mobs
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| !m.removed)
                    .map(|(i, m)| mob_box(i, m)),
            );
        }
        Box::new(
            self.boxes
                .get_or_init(|| MobBoxes::new(&self.game.mobs))
                .near(area),
        )
    }
    fn biome(&self, x: i32, y: i32, z: i32) -> Option<&BiomeInfo> {
        let chunk = self.game.world.chunks.get(&(x >> 4, z >> 4))?;
        Some(world_biomes!(self.game).get(chunk.biome(x, y, z)))
    }
    /// The block whose noise cell `BiomeManager.getBiome` reads.
    fn zoomed(&self, x: i32, y: i32, z: i32) -> [i32; 3] {
        let zoom = self.game.streaming.biome_zoom;
        let [qx, qy, qz] = zoom.quart_at_block(horizon_core::position::BlockPos::new(x, y, z));
        [qx << 2, qy << 2, qz << 2]
    }
    fn zoomed_biome(&self, x: i32, y: i32, z: i32) -> Option<&BiomeInfo> {
        let [x, y, z] = self.zoomed(x, y, z);
        self.biome(x, y, z)
    }
}
impl SpawnLevel for GameLevel<'_, '_> {
    fn state(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        self.w.state(x, y, z)
    }
    fn facts(&self) -> &BlockFacts {
        self.w.facts
    }
    fn mob_data(&self) -> &MobData {
        self.w.data
    }
    fn block_tag(&self, tag: &str, block: &str) -> bool {
        self.w.block_tag(tag, block)
    }
    // Level.getBiome: the zoomed biome.
    fn biome_tag(&self, tag: &str, x: i32, y: i32, z: i32) -> bool {
        let [x, y, z] = self.zoomed(x, y, z);
        self.w.biome_tag(tag, x, y, z)
    }
    fn sky_light(&self, x: i32, y: i32, z: i32) -> i32 {
        self.w.sky_light(x, y, z)
    }
    fn block_light(&self, x: i32, y: i32, z: i32) -> i32 {
        self.w.block_light(x, y, z)
    }
    fn sky_darken(&self) -> i32 {
        SpawnLevel::sky_darken(&self.w)
    }
    fn thundering(&self) -> bool {
        self.w.thundering
    }
    fn sea_level(&self) -> i32 {
        self.w.sea_level()
    }
    fn world_surface(&self, x: i32, z: i32) -> i32 {
        self.w.world_surface(x, z)
    }
    fn peaceful(&self) -> bool {
        self.w.peaceful()
    }
    fn dimension(&self) -> Dimension {
        self.w.dimension()
    }
    fn surface_slime_chance(&self, x: i32, y: i32, z: i32) -> f32 {
        self.w.surface_slime_chance(x, y, z)
    }
    fn world_seed(&self) -> i64 {
        self.w.world_seed
    }
    /// `getNearestPlayer(x, y, z, distance, true)`, over every player.
    fn player_near(&self, x: f64, y: f64, z: f64, distance: f64) -> bool {
        self.players.iter().any(|p| {
            p.alive && !p.creative && !p.spectator && p.distance_sqr(x, y, z) < distance * distance
        })
    }
}
impl NaturalLevel for GameLevel<'_, '_> {
    fn min_y(&self) -> i32 {
        self.w.world.bottom_y()
    }
    fn max_y(&self) -> i32 {
        self.w.world.top_y() - 1
    }
    fn ambient_light(&self) -> f32 {
        match self.game.dim {
            Dim::Nether => 0.1,
            Dim::End => 0.25,
            _ => 0.0,
        }
    }
    fn has_ceiling(&self) -> bool {
        self.game.dim == Dim::Nether
    }
    fn mob_type(&self, entity: &str) -> Option<(Species, f32, f32, Category)> {
        let t = self.game.res.mob_types().get(entity)?;
        Some((
            t.species,
            t.width as f32,
            t.height as f32,
            Category::from_name(&t.category),
        ))
    }
    // The `natural_mob_spawns` attribute reads the zoomed biome
    // (fullResolutionBiomes), the spawn probability the noise cell.
    fn biome_spawns(&self, x: i32, y: i32, z: i32) -> Option<&horizon_client::fast_hash::FxHashMap<String, Vec<SpawnEntry>>> {
        self.zoomed_biome(x, y, z).map(|b| &b.spawns)
    }
    fn spawn_cost(&self, entity: &str, x: i32, y: i32, z: i32) -> Option<SpawnCost> {
        self.zoomed_biome(x, y, z)?.spawn_costs.get(entity).copied()
    }
    fn creature_probability(&self, x: i32, y: i32, z: i32) -> f32 {
        self.biome(x, y, z).map_or(0.1, |b| b.creature_probability)
    }
    fn structures_at(
        &self,
        x: i32,
        y: i32,
        z: i32,
        wanted: &dyn Fn(&str) -> bool,
    ) -> Vec<StructureAt> {
        self.game
            .structures_reaching_where([x, y, z], wanted)
            .into_iter()
            .map(|(id, in_piece)| StructureAt {
                id,
                in_piece,
                in_bounds: true,
            })
            .collect()
    }
    fn structure_spawns(&self) -> &StructureSpawns {
        StructureSpawns::shared(&self.game.options.resources)
    }
    fn nearest_player_positions(&self) -> &[[f64; 3]] {
        &self.nearest
    }
    fn respawn(&self) -> Option<[i32; 3]> {
        // The world spawn point lives in the overworld.
        if self.game.dim != Dim::Overworld {
            return None;
        }
        self.game.level.spawn.map(|s| {
            [
                s[0].floor() as i32,
                s[1].floor() as i32,
                s[2].floor() as i32,
            ]
        })
    }
    fn can_spawn_in_chunk(&self, cx: i32, cz: i32) -> bool {
        self.game.world.chunks.contains_key(&(cx, cz))
    }
    fn no_collision(&self, bounds: Aabb) -> bool {
        let mut scene = SceneCollisions {
            world: &self.game.world,
            binding: self.w.collision,
            subject: None,
        };
        // getEntityCollisions(null, box): living shulkers are solid.
        let grown = bounds.inflate(Vec3::new(1e-7, 1e-7, 1e-7));
        no_collision_empty(&mut scene, bounds)
            && !self
                .mobs_near(grown)
                .any(|(_, b, _, shulker)| shulker && b.intersects(grown))
            && !self
                .pending_ids
                .borrow()
                .iter()
                .any(|(id, b)| id == "minecraft:shulker" && b.intersects(grown))
    }
    fn unobstructed(&self, bounds: Aabb) -> bool {
        let player = self
            .players
            .iter()
            .any(|p| p.alive && !p.spectator && p.bounds.intersects(bounds));
        !player
            && !self.pending.borrow().iter().any(|b| b.intersects(bounds))
            && !self
                .mobs_near(bounds)
                .any(|(_, b, alive, _)| alive && b.intersects(bounds))
    }
    fn spawn_mobs(&self) -> bool {
        self.game.ui.rules.bool("spawn_mobs")
    }
    /// WORLD_SURFACE (every spawn attempt's) is the chunk's kept heightmap.
    /// The others are scanned down the column: sections of air alone are
    /// skipped (air counts for no heightmap). An unloaded column reads as air.
    fn heightmap(&self, kind: Heightmap, x: i32, z: i32) -> i32 {
        let world = self.w.world;
        if kind == Heightmap::WorldSurface {
            return self.w.world_surface(x, z);
        }
        world
            .chunks
            .get(&(x >> 4, z >> 4))
            .and_then(|chunk| {
                chunk.highest(world.air.0, |y| {
                    spawn_selection::heightmap_blocks(self, kind, x, y, z)
                })
            })
            .map_or(self.w.world.bottom_y(), |y| y + 1)
    }
    fn pathfindable_land(&self, x: i32, y: i32, z: i32) -> bool {
        self.w.data.state(self.state(x, y, z)).flags & data::LAND != 0
    }
}
impl horizon_client::mobs::spawner::SpawnerLevel for GameLevel<'_, '_> {
    /// `Level.hasNearbyAlivePlayer`, over every player.
    fn alive_player_near(&self, x: f64, y: f64, z: f64, range: f64) -> bool {
        self.players.iter().any(|p| p.alive_near(x, y, z, range))
    }
    fn spawner_blocks_work(&self) -> bool {
        self.game.ui.rules.bool("spawner_blocks_work")
    }
    fn entity_exists(&self, id: &str) -> bool {
        self.game.res.mob_types().contains_key(id) || self.game.entity_type_exists(id)
    }
    fn count_entities(&self, id: &str, bounds: Aabb) -> usize {
        let mobs = self
            .mobs_near(bounds)
            .filter(|&(i, b, ..)| self.game.mobs[i].kind.id == id && b.intersects(bounds))
            .count();
        let pending = self
            .pending_ids
            .borrow()
            .iter()
            .filter(|(k, b)| k == id && b.intersects(bounds))
            .count();
        mobs + pending
    }
}
impl<'a> Game<'a> {
    pub(super) fn natural_level(&self) -> GameLevel<'_, 'a> {
        let players = self.level_players();
        let nearest = players
            .iter()
            .filter(|p| !p.spectator)
            .map(|p| [p.position.x, p.position.y, p.position.z])
            .collect();
        GameLevel {
            game: self,
            w: self.mob_world_base(&[]),
            players,
            nearest,
            pending: Default::default(),
            pending_ids: Default::default(),
            boxes: Default::default(),
            box_queries: Default::default(),
        }
    }
    /// Creates a mob the spawner accepted, finalized the game's way; the
    /// caller adds it with [`Game::add_spawned`].
    fn finalize_spawned(&self, r: &SpawnRequest, pending: &mut Vec<Mob>) -> Option<Spawned> {
        let kind = self.res.mob_types().get(r.entity)?.clone();
        let [x, y, z] = r.position;
        let mut mob = Mob::new(0, kind, Vec3::new(x, y, z), r.yaw);
        let facts = self.biome_facts(x.floor() as i32, y.floor() as i32, z.floor() as i32);
        let difficulty = self.local_difficulty(r.position);
        horizon_client::mobs::spawning::finalize(&mut mob, &facts, &difficulty);
        horizon_client::mobs::raider::finalize(
            &mut mob,
            horizon_client::mobs::raider::Reason::Natural,
        );
        let bounds = mob.bounds();
        pending.push(mob);
        Some(Spawned {
            bounds,
            school: true,
        })
    }
    /// Adds the mobs a spawn pass created: ids, enchanted gear and jockeys.
    fn add_spawned(&mut self, pending: Vec<Mob>, persistent: bool) {
        for mut mob in pending {
            self.next_entity += 1;
            mob.id = self.next_entity;
            mob.persistent = persistent;
            let id = mob.id;
            self.mobs.push(mob);
            self.native_mobs_joined(self.mobs.len() - 1, true);
            self.enchant_spawn_equipment(id);
            self.halloween_spawn_head(id);
            if !persistent {
                self.spawn_jockeys(id, true);
            }
        }
    }
    /// Every player in the level in place: the local player, and the LAN
    /// guests or server players in it.
    /// HorizonsX: shared, not copied: natural spawning asks for them once a chunk (a copy of 500 players for
    /// each of a thousand chunks a tick).
    pub(super) fn level_players(&self) -> std::sync::Arc<[LevelPlayer]> {
        if !self.players_cached {
            return self.level_players_iter().collect();
        }
        if let Some(players) = self.players_cache.borrow().as_ref() {
            return std::sync::Arc::clone(players);
        }
        let players: std::sync::Arc<[LevelPlayer]> = self.level_players_iter().collect();
        *self.players_cache.borrow_mut() = Some(std::sync::Arc::clone(&players));
        players
    }
    /// HorizonsX: keep the level's players through the world phase starting (`on`), or stop and forget them.
    pub(super) fn cache_players(&mut self, on: bool) {
        self.players_cached = on;
        *self.players_cache.get_mut() = None;
    }
    /// `level_players` one at a time, without collecting them.
    pub(super) fn level_players_iter(&self) -> impl Iterator<Item = LevelPlayer> + '_ {
        let local = self.player_view().map(|p| LevelPlayer {
            position: p.position,
            bounds: p.bounds,
            alive: p.alive,
            creative: p.creative,
            spectator: p.spectator,
        });
        let guests = self.lan.iter().flat_map(|h| {
            h.hub
                .guests
                .iter()
                .filter(|g| h.hub.here(g))
                .map(|g| LevelPlayer {
                    position: Vec3::new(g.position[0], g.position[1], g.position[2]),
                    bounds: super::lan_mobs::guest_bounds(g),
                    alive: !g.dead,
                    creative: g.creative(),
                    spectator: g.spectator(),
                })
        });
        local.into_iter().chain(guests)
    }
    /// `Level.hasNearbyAlivePlayer` before a level is built for the spawn
    /// code: what a spawner asks first, every tick.
    pub(super) fn alive_player_near(&self, x: f64, y: f64, z: f64, range: f64) -> bool {
        self.level_players_iter()
            .any(|p| p.alive_near(x, y, z, range))
    }
    /// `ChunkMap.collectSpawningChunks`: the natural spawn chunk tracker's
    /// chunks (within 8 chunks of a player, `getSpawnCandidateChunks`) that
    /// are ticking and within 128 blocks of a non-spectator. Also returns how
    /// many chunks the tracker holds (`getNaturalSpawnChunkCount`).
    pub(super) fn spawning_chunks(
        &self,
        players: &[LevelPlayer],
        scratch: &mut SpawnScratch,
    ) -> i32 {
        let candidates = &mut scratch.candidates;
        candidates.clear();
        candidates.reserve(289 * players.len().min(64));
        let seen = &mut scratch.seen;
        seen.clear();
        seen.reserve(289 * players.len().min(64));
        // HorizonsX: each player chunk once, in the order its first player has it: a later player in a
        // chunk already gone through adds only chunks already seen (the same list, in the same order; with
        // 600 players in a few chunks this was 173k set inserts a tick).
        let mut player_chunks: Vec<(i32, i32)> = Vec::new();
        let mut chunk_seen = horizon_client::fast_hash::FxHashSet::default();
        for p in players {
            let key = (p.position.x.floor() as i32 >> 4, p.position.z.floor() as i32 >> 4);
            if chunk_seen.insert(key) {
                player_chunks.push(key);
            }
        }
        for (pcx, pcz) in player_chunks {
            for dx in -8..=8 {
                for dz in -8..=8 {
                    let key = (pcx + dx, pcz + dz);
                    if seen.insert(key) {
                        candidates.push(key);
                    }
                }
            }
        }
        let count = candidates.len() as i32;
        let levels = self.tickets.levels();
        // Only the players in the cells around a chunk can be close enough to it.
        let cells = spawn_selection::PlayerCells::new(
            players.iter().map(|p| (p.position.x, p.position.z)),
        );
        // HorizonsX: each candidate's checks (all reads) on every core, the kept ones in their order.
        let world = &self.world;
        let keep = |&(cx, cz): &(i32, i32)| {
            levels.loaded_ticking(cx, cz)
                && super::ticking_chunk(world, cx, cz)
                && cells.any_near(f64::from(cx * 16 + 8), f64::from(cz * 16 + 8), |i| {
                    close_enough_for_spawning(&players[i], (cx, cz))
                })
        };
        if candidates.len() >= SPAWN_CANDIDATES_PARALLEL {
            let all = std::mem::take(candidates);
            *candidates = horizon_client::v2::map_batches(all, 256, |key| keep(&key).then_some(key))
                .into_iter()
                .flatten()
                .collect();
        } else {
            candidates.retain(keep);
        }
        count
    }
    /// `ServerChunkCache.tickChunks`: the spawn state over the loaded mobs,
    /// the categories under their global caps (creatures every 400 ticks),
    /// then `tickSpawningChunk` in the shuffled spawning chunks of every
    /// player: inhabited time, then `spawnForChunk`.
    pub(super) fn spawn_natural(&mut self) {
        // `tickChunks` does nothing in a Debug world.
        if self.world_debug {
            return;
        }
        let players = self.level_players();
        if players.is_empty() {
            return;
        }
        let mut scratch = std::mem::take(&mut self.spawn_scratch);
        let started = horizon_client::spans::enabled().then(std::time::Instant::now);
        let spawnable = self.spawning_chunks(&players, &mut scratch);
        for &key in &scratch.candidates {
            *self.world.inhabited.entry(key).or_insert(0) += 1;
        }
        // `tickSpawningChunk`: every spawning chunk ages, but `spawnForChunk` runs only where
        // `ServerLevel.canSpawnEntitiesInChunk` holds, within the simulation distance (entity ticking). The 8
        // chunks around a player reach past a simulation distance below 8: a spawn there was tried every tick
        // (28% more chunks at 7) and its mob stood in a chunk that never ticks it.
        let levels = self.tickets.levels();
        scratch.candidates.retain(|&(x, z)| levels.entity_ticking(x, z));
        if let Some(t) = started {
            horizon_client::spans::record("sn_chunks", t.elapsed());
        }
        let spawn_mobs = self.ui.rules.bool("spawn_mobs");
        let spawn_enemies = self.ui.rules.bool("spawn_monsters");
        // HorizonsX: players far apart spawn on their own cores (one group: vanilla's pass),
        // when that is measured faster on this machine now (horizon_client::v2::Run).
        let mut run = horizon_client::v2::Run::start(horizon_client::v2::Phase::Spawn);
        run.units(scratch.candidates.len());
        if run.parallel()
            && horizon_client::spans::time("sn_groups", || self.spawn_natural_regions(&players, &scratch, spawn_mobs, spawn_enemies))
        {
            scratch.candidates.clear();
            self.spawn_scratch = scratch;
            return;
        }
        run.fell_back();
        let mut random = std::mem::replace(&mut self.world.random, LegacyRandom::new(0));
        let mut pending = Vec::new();
        let last_state;
        {
            let level = self.natural_level();
            let counted: Vec<Counted> = self
                .mobs
                .iter()
                .map(|m| Counted {
                    entity: &m.kind.id,
                    category: Category::from_name(&m.kind.category),
                    block: [
                        m.position.x.floor() as i32,
                        m.position.y.floor() as i32,
                        m.position.z.floor() as i32,
                    ],
                    is_mob: true,
                    persistent: m.persistent
                        || horizon_client::mobs::ai::requires_custom_persistence(m),
                })
                .collect();
            let players = players
                .iter()
                .map(|p| SpawnPlayer {
                    position: [p.position.x, p.position.y, p.position.z],
                    spectator: p.spectator,
                })
                .collect();
            let mut state = SpawnState::new(&level, spawnable, players, &counted, |cx, cz| {
                self.world.chunks.contains_key(&(cx, cz))
            });
            last_state = state.counts();
            let categories = if spawn_mobs {
                state.filtered_categories_at(spawn_enemies, self.world.game_time)
            } else {
                Vec::new()
            };
            // Util.shuffle.
            for i in (1..scratch.candidates.len()).rev() {
                let j = random.next_int_bound(i as i32 + 1).unwrap_or(0) as usize;
                scratch.candidates.swap(i, j);
            }
            if !categories.is_empty() {
                for chunk in scratch.candidates.drain(..) {
                    let mut spawn = |l: &GameLevel, r: &SpawnRequest| {
                        let spawned = l.game.finalize_spawned(r, &mut pending);
                        if let Some(s) = &spawned {
                            l.pending.borrow_mut().push(s.bounds);
                        }
                        spawned
                    };
                    spawn_selection::spawn_for_chunk(
                        &level,
                        &mut random,
                        &mut state,
                        &categories,
                        chunk,
                        &mut spawn,
                        &mut |_| {},
                    );
                }
            }
        }
        self.world.random = random;
        // ServerChunkCache.lastSpawnState, for the debug overlay.
        self.spawn_counts.insert(self.dim, last_state);
        self.add_spawned(pending, false);
        self.spawn_scratch = scratch;
    }
    /// `NoiseBasedChunkGenerator.spawnOriginalMobs`: creature packs for a
    /// freshly generated chunk, from the biome at its corner column's top,
    /// with the chunk's decoration-seeded random, finalized with the
    /// region's random as generation adds them (mobs are not generated in
    /// the End).
    pub(super) fn spawn_generation_creatures(&mut self, cx: i32, cz: i32) {
        if !self.world.chunks.contains_key(&(cx, cz)) || self.dim == Dim::End {
            return;
        }
        let Some(terrain) = self.end.locator() else {
            return;
        };
        // `FlatLevelSource.spawnOriginalMobs` is empty: a superflat world places no packs.
        if terrain.is_flat() {
            return;
        }
        let mut region = terrain.region_random(cx, cz);
        let any = region.as_any();
        let added = if let Some(r) = any.downcast_mut::<LegacyRandom>() {
            self.generation_creatures(cx, cz, &terrain, r)
        } else if let Some(r) = any.downcast_mut::<XoroshiroRandom>() {
            self.generation_creatures(cx, cz, &terrain, r)
        } else {
            return;
        };
        for nbt in added {
            self.load_mob(Tag::Compound(nbt), None);
        }
    }
    fn generation_creatures<Q: RandomSource + 'static>(
        &self,
        cx: i32,
        cz: i32,
        terrain: &VanillaTerrain,
        region: &mut Q,
    ) -> Vec<Compound> {
        let mut random = WorldgenRandom::new(LegacyRandom::new(0));
        random.set_decoration_seed(self.level.seed, cx * 16, cz * 16);
        let mut added = Vec::new();
        let mut group = None;
        let level = self.natural_level();
        let source = [cx * 16, level.max_y(), cz * 16];
        let mut spawn = |l: &GameLevel, r: &SpawnRequest, region: &mut Q| {
            // Each pack starts without group data.
            if r.group_index == 0 {
                group = None;
            }
            let [x, y, z] = r.position;
            let (bx, by, bz) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
            let biome = l.zoomed_biome(bx, by, bz).map_or("", |b| b.name.as_str());
            let at = Vec3::new(x, y, z);
            match terrain.spawn_generated(r.entity, at, r.yaw, biome, region, &mut group) {
                Some(nbt) => {
                    added.push(nbt);
                    true
                }
                None => false,
            }
        };
        spawn_selection::spawn_mobs_for_chunk_generation(
            &level,
            source,
            (cx, cz),
            &mut random,
            region,
            &mut spawn,
            &mut |_| {},
        );
        added
    }
}

// ---- HorizonsX: natural spawning by regions ---------------------------------------------------

/// Whether natural spawning runs by regions: on unless `HORIZON_REGIONS=0`.
pub(super) fn regions_on() -> bool {
    horizon_client::v2::regions_on()
}

/// Whether a plugin hears what a spawn attempt does (the plugin layer fills this in): then the
/// attempts run on the game thread, where plugin events run.
fn v2_plugin_spawn_hooks() -> bool {
    false
}

/// HorizonsX's region threads ask this (`plugin_spawn_hooks`, kept as the plugin layer fills it in).
pub(super) fn plugins_hear_spawns() -> bool {
    v2_plugin_spawn_hooks()
}

/// Players whose spawn areas (the 17 by 17 chunks around each) touch: within 17 chunks.
const REGION_REACH: i32 = 17;

/// What every region's spawn pass reads. Nothing in it changes while the regions run, and the
/// compiler checks that threads may share it.
struct SpawnShared<'g> {
    parts: horizon_client::mobs::MobWorldParts<'g>,
    zoom: horizon_core::biome::BiomeZoom,
    ambient_light: f32,
    has_ceiling: bool,
    mob_types: &'g HashMap<String, std::sync::Arc<horizon_client::mobs::MobType>>,
    locator: Option<std::sync::Arc<VanillaTerrain>>,
    structure_spawns: &'static StructureSpawns,
    respawn: Option<[i32; 3]>,
    spawn_mobs: bool,
    boxes: MobBoxes,
}

/// One region's work: its chunks (in the tracker's order), players, counted mobs, its share of
/// the spawn tracker's chunks and the seed of its own random.
struct RegionWork<'m> {
    chunks: Vec<(i32, i32)>,
    players: Vec<LevelPlayer>,
    counted: Vec<Counted<'m>>,
    spawnable: i32,
    seed: i64,
    /// Whether this region's pass splits into tiles: when it holds more than its share of the cores'
    /// work (measured 11-04: tiling every region only added rounds of waiting).
    tiles: bool,
}

/// A spawn a region decided: made into a mob, the game's way, back on the game thread.
struct RegionSpawn {
    entity: String,
    species: Species,
    position: [f64; 3],
    yaw: f32,
    group_index: u32,
}

/// The level as one region's spawn pass reads it, on that region's thread.
struct RegionLevel<'s, 'g> {
    s: &'s SpawnShared<'g>,
    w: MobWorld<'g>,
    players: &'s [LevelPlayer],
    nearest: Vec<[f64; 3]>,
    /// Boxes of the mobs this region's pass decided on.
    pending: std::cell::RefCell<Vec<Aabb>>,
}
impl RegionLevel<'_, '_> {
    fn biome(&self, x: i32, y: i32, z: i32) -> Option<&BiomeInfo> {
        let chunk = self.w.world.chunks.get(&(x >> 4, z >> 4))?;
        Some(self.s.parts.biomes.get(chunk.biome(x, y, z)))
    }
    fn zoomed_biome(&self, x: i32, y: i32, z: i32) -> Option<&BiomeInfo> {
        let [qx, qy, qz] = self.s.zoom.quart_at_block(horizon_core::position::BlockPos::new(x, y, z));
        self.biome(qx << 2, qy << 2, qz << 2)
    }
}
impl SpawnLevel for RegionLevel<'_, '_> {
    fn state(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        self.w.state(x, y, z)
    }
    fn facts(&self) -> &BlockFacts {
        self.w.facts
    }
    fn mob_data(&self) -> &MobData {
        self.w.data
    }
    fn block_tag(&self, tag: &str, block: &str) -> bool {
        self.w.block_tag(tag, block)
    }
    fn biome_tag(&self, tag: &str, x: i32, y: i32, z: i32) -> bool {
        let [qx, qy, qz] = self.s.zoom.quart_at_block(horizon_core::position::BlockPos::new(x, y, z));
        self.w.biome_tag(tag, qx << 2, qy << 2, qz << 2)
    }
    fn sky_light(&self, x: i32, y: i32, z: i32) -> i32 {
        self.w.sky_light(x, y, z)
    }
    fn block_light(&self, x: i32, y: i32, z: i32) -> i32 {
        self.w.block_light(x, y, z)
    }
    fn sky_darken(&self) -> i32 {
        SpawnLevel::sky_darken(&self.w)
    }
    fn thundering(&self) -> bool {
        self.w.thundering
    }
    fn sea_level(&self) -> i32 {
        self.w.sea_level()
    }
    fn world_surface(&self, x: i32, z: i32) -> i32 {
        self.w.world_surface(x, z)
    }
    fn peaceful(&self) -> bool {
        self.w.peaceful()
    }
    fn dimension(&self) -> Dimension {
        self.w.dimension()
    }
    fn surface_slime_chance(&self, x: i32, y: i32, z: i32) -> f32 {
        self.w.surface_slime_chance(x, y, z)
    }
    fn world_seed(&self) -> i64 {
        self.w.world_seed
    }
    fn player_near(&self, x: f64, y: f64, z: f64, distance: f64) -> bool {
        self.players.iter().any(|p| {
            p.alive && !p.creative && !p.spectator && p.distance_sqr(x, y, z) < distance * distance
        })
    }
}
impl NaturalLevel for RegionLevel<'_, '_> {
    fn min_y(&self) -> i32 {
        self.w.world.bottom_y()
    }
    fn max_y(&self) -> i32 {
        self.w.world.top_y() - 1
    }
    fn ambient_light(&self) -> f32 {
        self.s.ambient_light
    }
    fn has_ceiling(&self) -> bool {
        self.s.has_ceiling
    }
    fn mob_type(&self, entity: &str) -> Option<(Species, f32, f32, Category)> {
        let t = self.s.mob_types.get(entity)?;
        Some((t.species, t.width as f32, t.height as f32, Category::from_name(&t.category)))
    }
    fn biome_spawns(
        &self,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<&horizon_client::fast_hash::FxHashMap<String, Vec<SpawnEntry>>> {
        self.zoomed_biome(x, y, z).map(|b| &b.spawns)
    }
    fn spawn_cost(&self, entity: &str, x: i32, y: i32, z: i32) -> Option<SpawnCost> {
        self.zoomed_biome(x, y, z)?.spawn_costs.get(entity).copied()
    }
    fn creature_probability(&self, x: i32, y: i32, z: i32) -> f32 {
        self.biome(x, y, z).map_or(0.1, |b| b.creature_probability)
    }
    fn structures_at(&self, x: i32, y: i32, z: i32, wanted: &dyn Fn(&str) -> bool) -> Vec<StructureAt> {
        super::structures::structures_reaching_in(self.w.world, self.s.locator.as_deref(), [x, y, z], wanted)
            .into_iter()
            .map(|(id, in_piece)| StructureAt {
                id,
                in_piece,
                in_bounds: true,
            })
            .collect()
    }
    fn structure_spawns(&self) -> &StructureSpawns {
        self.s.structure_spawns
    }
    fn nearest_player_positions(&self) -> &[[f64; 3]] {
        &self.nearest
    }
    fn respawn(&self) -> Option<[i32; 3]> {
        self.s.respawn
    }
    fn can_spawn_in_chunk(&self, cx: i32, cz: i32) -> bool {
        self.w.world.chunks.contains_key(&(cx, cz))
    }
    fn no_collision(&self, bounds: Aabb) -> bool {
        let mut scene = SceneCollisions {
            world: self.w.world,
            binding: self.w.collision,
            subject: None,
        };
        let grown = bounds.inflate(Vec3::new(1e-7, 1e-7, 1e-7));
        no_collision_empty(&mut scene, bounds)
            && !self
                .s
                .boxes
                .near(grown)
                .any(|(_, b, _, shulker)| shulker && b.intersects(grown))
    }
    fn unobstructed(&self, bounds: Aabb) -> bool {
        let player = self
            .players
            .iter()
            .any(|p| p.alive && !p.spectator && p.bounds.intersects(bounds));
        !player
            && !self.pending.borrow().iter().any(|b| b.intersects(bounds))
            && !self
                .s
                .boxes
                .near(bounds)
                .any(|(_, b, alive, _)| alive && b.intersects(bounds))
    }
    fn spawn_mobs(&self) -> bool {
        self.s.spawn_mobs
    }
    fn heightmap(&self, kind: Heightmap, x: i32, z: i32) -> i32 {
        let world = self.w.world;
        if kind == Heightmap::WorldSurface {
            return self.w.world_surface(x, z);
        }
        world
            .chunks
            .get(&(x >> 4, z >> 4))
            .and_then(|chunk| {
                chunk.highest(world.air.0, |y| spawn_selection::heightmap_blocks(self, kind, x, y, z))
            })
            .map_or(world.bottom_y(), |y| y + 1)
    }
    fn pathfindable_land(&self, x: i32, y: i32, z: i32) -> bool {
        self.w.data.state(self.state(x, y, z)).flags & data::LAND != 0
    }
}

/// One region's spawn pass: vanilla's `tickChunks` spawning over the region's chunks, players
/// and mobs, with the region's own random. What it decides comes back as [`RegionSpawn`]s.
fn spawn_region(
    s: &SpawnShared<'_>,
    work: &RegionWork<'_>,
    spawn_enemies: bool,
    game_time: i64,
) -> (Vec<RegionSpawn>, (i32, [i32; 8])) {
    let level = RegionLevel {
        s,
        w: s.parts.world(),
        players: &work.players,
        nearest: work
            .players
            .iter()
            .filter(|p| !p.spectator)
            .map(|p| [p.position.x, p.position.y, p.position.z])
            .collect(),
        pending: Default::default(),
    };
    let players = work
        .players
        .iter()
        .map(|p| SpawnPlayer {
            position: [p.position.x, p.position.y, p.position.z],
            spectator: p.spectator,
        })
        .collect();
    let mut state = SpawnState::new(&level, work.spawnable, players, &work.counted, |cx, cz| {
        level.w.world.chunks.contains_key(&(cx, cz))
    });
    let counts = state.counts();
    let categories = if s.spawn_mobs {
        state.filtered_categories_at(spawn_enemies, game_time)
    } else {
        Vec::new()
    };
    let mut random = LegacyRandom::new(work.seed);
    let mut chunks = work.chunks.clone();
    // Util.shuffle, with the region's random.
    for i in (1..chunks.len()).rev() {
        let j = random.next_int_bound(i as i32 + 1).unwrap_or(0) as usize;
        chunks.swap(i, j);
    }
    let mut out = Vec::new();
    if work.tiles && !categories.is_empty() && chunks.len() >= TILED_SPAWN_MIN {
        if let Some(tiled) = spawn_tiles(s, work, &level, &mut state, &categories, &chunks) {
            return (tiled, counts);
        }
    }
    if !categories.is_empty() {
        for chunk in chunks {
            let mut spawn = |l: &RegionLevel, r: &SpawnRequest| {
                // The box of the type's size: the mob itself is made on the game thread.
                let t = l.s.mob_types.get(r.entity)?;
                let [x, y, z] = r.position;
                let bounds = spawn_selection::spawn_box(x, y, z, t.width as f32, t.height as f32);
                l.pending.borrow_mut().push(bounds);
                out.push(RegionSpawn {
                    entity: r.entity.to_owned(),
                    species: r.species,
                    position: r.position,
                    yaw: r.yaw,
                    group_index: r.group_index,
                });
                Some(Spawned {
                    bounds,
                    school: true,
                })
            };
            spawn_selection::spawn_for_chunk(
                &level,
                &mut random,
                &mut state,
                &categories,
                chunk,
                &mut spawn,
                &mut |_| {},
            );
        }
    }
    (out, counts)
}

/// HorizonsX: regions with this many spawning chunks or more spawn by tiles on several cores.
const TILED_SPAWN_MIN: usize = 128;
/// The tiles: 8×8 chunks (128 blocks).
const TILE_SHIFT: i32 = 3;

/// HorizonsX: one region's spawn pass on every free core (beyond Folia, whose regions spawn on one).
/// The region's chunks go in tiles of 8×8 chunks, in nine rounds by the tile's place in a 3×3 pattern:
/// tiles of one round are at least 256 blocks apart, so no player is within 128 blocks of two of them
/// and each tile's local mob caps are exact. Each tile runs vanilla's per-chunk pass on its own copy of
/// the spawn state and its own random; after the round, its spawns are counted into the region's state in
/// tile order, each only while its category's global cap still has room (a round's tiles all started
/// from the same counts). None when the chunks make one tile only.
fn spawn_tiles(
    s: &SpawnShared<'_>,
    work: &RegionWork<'_>,
    level: &RegionLevel<'_, '_>,
    state: &mut SpawnState,
    categories: &[Category],
    chunks: &[(i32, i32)],
) -> Option<Vec<RegionSpawn>> {
    use horizon_client::fast_hash::FxHashMap;
    let mut tiles: FxHashMap<(i32, i32), Vec<(i32, i32)>> = FxHashMap::default();
    let mut order: Vec<(i32, i32)> = Vec::new();
    for &c in chunks {
        let key = (c.0 >> TILE_SHIFT, c.1 >> TILE_SHIFT);
        tiles
            .entry(key)
            .or_insert_with(|| {
                order.push(key);
                Vec::new()
            })
            .push(c);
    }
    if tiles.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    for colour in 0..9 {
        let round: Vec<((i32, i32), &Vec<(i32, i32)>)> = order
            .iter()
            .filter(|k| k.0.rem_euclid(3) + 3 * k.1.rem_euclid(3) == colour)
            .map(|k| (*k, &tiles[k]))
            .collect();
        if round.is_empty() {
            continue;
        }
        let base = &*state;
        let (nearest, players, seed) = (&level.nearest, &work.players, work.seed);
        // The boxes of what the rounds before committed: a tile's spawns keep clear of them too.
        let committed: Vec<Aabb> = level.pending.borrow().clone();
        let committed = &committed;
        let made: Vec<Vec<(RegionSpawn, Category)>> = horizon_client::v2::map_here_and_pool(
            round,
            |(_, tile)| tile.len(),
            |(key, tile)| {
                let tile_level = RegionLevel {
                    s,
                    w: s.parts.world(),
                    players,
                    nearest: nearest.clone(),
                    pending: std::cell::RefCell::new(committed.clone()),
                };
                let mut st = base.clone();
                let mut random = LegacyRandom::new(
                    seed ^ (i64::from(key.0)).wrapping_mul(0x9E37_79B9) ^ (i64::from(key.1)).wrapping_mul(0x85EB_CA6B),
                );
                let mut spawns: Vec<RegionSpawn> = Vec::new();
                let mut kinds: Vec<Category> = Vec::new();
                let mut current = None;
                for &chunk in tile.iter() {
                    let mut spawn = |l: &RegionLevel, r: &SpawnRequest| {
                        let t = l.s.mob_types.get(r.entity)?;
                        let [x, y, z] = r.position;
                        let bounds = spawn_selection::spawn_box(x, y, z, t.width as f32, t.height as f32);
                        l.pending.borrow_mut().push(bounds);
                        spawns.push(RegionSpawn {
                            entity: r.entity.to_owned(),
                            species: r.species,
                            position: r.position,
                            yaw: r.yaw,
                            group_index: r.group_index,
                        });
                        Some(Spawned { bounds, school: true })
                    };
                    let mut trace = |e: spawn_selection::Event| match e {
                        spawn_selection::Event::Start(category, _) => current = Some(category),
                        spawn_selection::Event::Spawn(..) => {
                            if let Some(category) = current {
                                kinds.push(category);
                            }
                        }
                        _ => {}
                    };
                    spawn_selection::spawn_for_chunk(
                        &tile_level,
                        &mut random,
                        &mut st,
                        categories,
                        chunk,
                        &mut spawn,
                        &mut trace,
                    );
                }
                spawns.into_iter().zip(kinds).collect()
            },
        );
        for (spawn, category) in made.into_iter().flatten() {
            if !state.can_spawn_global(category) {
                continue;
            }
            let pos = [spawn.position[0].floor() as i32, spawn.position[1].floor() as i32, spawn.position[2].floor() as i32];
            state.record_spawn(level, &spawn.entity, category, pos);
            level.pending.borrow_mut().push(match s.mob_types.get(&spawn.entity) {
                Some(t) => spawn_selection::spawn_box(
                    spawn.position[0],
                    spawn.position[1],
                    spawn.position[2],
                    t.width as f32,
                    t.height as f32,
                ),
                None => continue,
            });
            out.push(spawn);
        }
    }
    Some(out)
}

impl Game<'_> {
    /// HorizonsX: `tickChunks`' natural spawning, by regions. Players whose spawn areas touch
    /// form a region; regions are more than 128 blocks apart, so no attempt of one can see
    /// another's players, local mob caps, mobs or new spawns. Each region runs vanilla's pass over
    /// its own chunks and mobs (its global cap scaled to its chunks), with its own random, on a
    /// core of its own. The rules are vanilla's; which random number an attempt draws is not.
    /// False (nothing done) when the players form one region: vanilla's own pass runs then.
    fn spawn_natural_regions(
        &mut self,
        players: &[LevelPlayer],
        scratch: &SpawnScratch,
        spawn_mobs: bool,
        spawn_enemies: bool,
    ) -> bool {
        // A plugin hears a spawn attempt (`PreCreatureSpawnEvent`): its listeners run on the game
        // thread, so vanilla's single pass runs.
        if v2_plugin_spawn_hooks() {
            return false;
        }
        let chunk_of = |p: &LevelPlayer| {
            (
                (p.position.x.floor() as i32) >> 4,
                (p.position.z.floor() as i32) >> 4,
            )
        };
        // Union-find over players, by cells of REGION_REACH chunks (a pair that close is in
        // neighbouring cells).
        let n = players.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        let mut cells: horizon_client::fast_hash::FxHashMap<(i32, i32), Vec<usize>> = Default::default();
        for (i, p) in players.iter().enumerate() {
            let (cx, cz) = chunk_of(p);
            cells
                .entry((cx.div_euclid(REGION_REACH), cz.div_euclid(REGION_REACH)))
                .or_default()
                .push(i);
        }
        for (i, p) in players.iter().enumerate() {
            let (cx, cz) = chunk_of(p);
            let (gx, gz) = (cx.div_euclid(REGION_REACH), cz.div_euclid(REGION_REACH));
            for a in gx - 1..=gx + 1 {
                for b in gz - 1..=gz + 1 {
                    for &j in cells.get(&(a, b)).map_or(&[][..], Vec::as_slice) {
                        let (jx, jz) = chunk_of(&players[j]);
                        if j > i && (cx - jx).abs().max((cz - jz).abs()) <= REGION_REACH {
                            let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                            if ri != rj {
                                parent[ri.max(rj)] = ri.min(rj);
                            }
                        }
                    }
                }
            }
        }
        // Regions numbered by their first player.
        let mut region_of_root = vec![usize::MAX; n];
        let mut region_of = vec![0usize; n];
        let mut regions = 0;
        for i in 0..n {
            let r = root(&mut parent, i);
            if region_of_root[r] == usize::MAX {
                region_of_root[r] = regions;
                regions += 1;
            }
            region_of[i] = region_of_root[r];
        }
        if regions < 2 {
            return false;
        }
        // Each chunk of a spawn area is its region's; the tracker's chunk count by region.
        let mut area: horizon_client::fast_hash::FxHashMap<(i32, i32), usize> = Default::default();
        let mut spawnable = vec![0i32; regions];
        for (i, p) in players.iter().enumerate() {
            let (pcx, pcz) = chunk_of(p);
            for dx in -8..=8 {
                for dz in -8..=8 {
                    if area.insert((pcx + dx, pcz + dz), region_of[i]).is_none() {
                        spawnable[region_of[i]] += 1;
                    }
                }
            }
        }
        // Seeds from the level's random, region by region.
        let seeds: Vec<i64> = (0..regions).map(|_| self.world.random.next_long()).collect();
        let game_time = self.world.game_time;
        let decided: Vec<(Vec<RegionSpawn>, (i32, [i32; 8]))> = {
            let mut work: Vec<RegionWork> = (0..regions)
                .map(|r| RegionWork {
                    chunks: Vec::new(),
                    players: Vec::new(),
                    counted: Vec::new(),
                    spawnable: spawnable[r],
                    seed: seeds[r],
                    tiles: false,
                })
                .collect();
            for (i, p) in players.iter().enumerate() {
                work[region_of[i]].players.push(p.clone());
            }
            for &chunk in &scratch.candidates {
                if let Some(&r) = area.get(&chunk) {
                    work[r].chunks.push(chunk);
                }
            }
            // A region with more than its share of the cores' work splits into tiles (spawn_tiles): the
            // tick waits for the largest region, so that one must not run on one core alone.
            let total: usize = work.iter().map(|w| w.chunks.len()).sum();
            let share = total / horizon_client::v2::workers(usize::MAX).max(1);
            for w in &mut work {
                w.tiles = w.chunks.len() > share;
            }
            for m in &self.mobs {
                let block = [
                    m.position.x.floor() as i32,
                    m.position.y.floor() as i32,
                    m.position.z.floor() as i32,
                ];
                if let Some(&r) = area.get(&(block[0] >> 4, block[2] >> 4)) {
                    work[r].counted.push(Counted {
                        entity: &m.kind.id,
                        category: Category::from_name(&m.kind.category),
                        block,
                        is_mob: true,
                        persistent: m.persistent
                            || horizon_client::mobs::ai::requires_custom_persistence(m),
                    });
                }
            }
            let base = self.mob_world_base(&[]);
            let shared = SpawnShared {
                parts: base.parts(),
                zoom: self.streaming.biome_zoom,
                ambient_light: match self.dim {
                    Dim::Nether => 0.1,
                    Dim::End => 0.25,
                    _ => 0.0,
                },
                has_ceiling: self.dim == Dim::Nether,
                mob_types: self.res.mob_types(),
                locator: self.end.locator(),
                structure_spawns: StructureSpawns::shared(&self.options.resources),
                respawn: (self.dim == Dim::Overworld)
                    .then_some(self.level.spawn)
                    .flatten()
                    .map(|s| [s[0].floor() as i32, s[1].floor() as i32, s[2].floor() as i32]),
                spawn_mobs,
                boxes: MobBoxes::new(&self.mobs),
            };
            // Every region on the pool's threads (it balances them), results in region order.
            let shared = &shared;
            // The largest region on this thread, the others on the pool (`map_here_and_pool`).
            horizon_client::v2::map_here_and_pool(
                work.iter().collect(),
                |w| w.chunks.len(),
                |w| spawn_region(shared, w, spawn_enemies, game_time),
            )
        };
        // The mobs, made and added on the game thread, region by region in the order decided.
        let mut pending = Vec::new();
        let (mut spawnable_total, mut counts) = (0, [0i32; 8]);
        for (spawns, (chunks, by_category)) in &decided {
            spawnable_total += chunks;
            for (sum, c) in counts.iter_mut().zip(by_category) {
                *sum += c;
            }
            for s in spawns {
                let request = SpawnRequest {
                    entity: &s.entity,
                    species: s.species,
                    position: s.position,
                    yaw: s.yaw,
                    group_index: s.group_index,
                    reason: horizon_client::mobs::spawn_rules::Reason::Natural,
                };
                self.finalize_spawned(&request, &mut pending);
            }
        }
        self.spawn_counts.insert(self.dim, (spawnable_total, counts));
        self.add_spawned(pending, false);
        true
    }
}