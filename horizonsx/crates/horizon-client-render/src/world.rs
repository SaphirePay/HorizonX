//! Resident client world: streamed chunk columns plus the bookkeeping the game
//! loop needs to relight and remesh after edits.
use crate::{
    chunk::{Chunk, ColumnLayout, LevelShape},
    fast_hash::{FxHashMap, FxHashSet},
    section_mask::SectionMask,
};
use horizon_core::random::LegacyRandom;
use horizon_world::registry::BlockStateId;
use std::sync::Arc;

/// Sections (x, section index, z) waiting to be remeshed, kept by column
/// (a bit per section index), so the mesh scheduler looks at each column once
/// rather than at every section of it.
#[derive(Clone, Debug, Default)]
pub struct DirtySections {
    columns: crate::fast_hash::FxHashMap<(i32, i32), SectionMask>,
}
impl DirtySections {
    pub fn insert(&mut self, (x, sy, z): (i32, i32, i32)) {
        debug_assert!((0..SectionMask::CAPACITY as i32).contains(&sy));
        self.columns.entry((x, z)).or_default().insert(sy as usize);
    }
    pub fn contains(&self, &(x, sy, z): &(i32, i32, i32)) -> bool {
        (0..SectionMask::CAPACITY as i32).contains(&sy)
            && self
                .columns
                .get(&(x, z))
                .is_some_and(|m| m.contains(sy as usize))
    }
    /// The dirty section indexes of column (x, z), removed.
    pub fn take_column(&mut self, x: i32, z: i32) -> SectionMask {
        self.columns.remove(&(x, z)).unwrap_or_default()
    }
    /// Keep the columns `keep` accepts.
    pub fn retain_columns(&mut self, mut keep: impl FnMut(i32, i32) -> bool) {
        self.columns.retain(|&(x, z), _| keep(x, z));
    }
    /// The columns holding dirty sections, with their section indexes.
    pub fn columns(&self) -> impl Iterator<Item = ((i32, i32), SectionMask)> + '_ {
        self.columns.iter().map(|(&c, &m)| (c, m))
    }
    pub fn clear(&mut self) {
        self.columns.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }
    /// How many sections are dirty.
    pub fn len(&self) -> usize {
        self.columns.values().map(SectionMask::len).sum()
    }
}
pub struct World {
    /// The level's build range and skylight (its dimension type); its
    /// chunks hold `shape.column()`.
    pub shape: LevelShape,
    pub fast_lava: bool,
    pub chunks: crate::chunk_map::ChunkMap,
    pub random: LegacyRandom,
    pub revision: u64,
    pub health_rules: HealthRules,
    pub air: BlockStateId,
    /// Sections whose geometry changed and need remeshing.
    pub dirty_sections: DirtySections,
    /// Chunks whose light must be recomputed.
    pub dirty_light: crate::fast_hash::FxHashSet<(i32, i32)>,
    /// Edits on lit chunks (position, old state, new state), relit
    /// incrementally by streaming.
    pub light_edits: Vec<((i32, i32, i32), u32, u32)>,
    /// Saved entities of chunks that just loaded, spawned by the game.
    pub loaded_entities: Vec<((i32, i32), Vec<horizon_world::nbt::Tag>)>,
    /// Chunks that just came into this level from a worker, and whether each
    /// was generated rather than read from the save: the game tells native mods.
    pub loaded_chunks: Vec<(i32, i32, bool)>,
    /// Chunks changed since the last save.
    pub modified: FxHashSet<(i32, i32)>,
    /// Block entities, and the order they tick in.
    pub block_entities: crate::block_entities::BlockEntities,
    /// Each chest's `ContainerOpenersCounter`: opener count and largest
    /// interaction range.
    pub chest_openers: FxHashMap<(i32, i32, i32), (i32, f64)>,
    /// `BaseContainerBlockEntity.name`: custom names of container block entities.
    pub block_names: FxHashMap<(i32, i32, i32), String>,
    /// States (by id) whose writes are recorded in `watched_writes` (sculk
    /// blocks, which need a block entity).
    pub watched: Vec<bool>,
    /// Writes of watched states: position, previous and new state.
    pub watched_writes: Vec<((i32, i32, i32), u32, u32)>,
    /// `ChunkAccess.inhabitedTime` of resident chunks: ticks a player spent
    /// within spawning range (local difficulty).
    pub inhabited: FxHashMap<(i32, i32), i64>,
    pub game_time: i64,
    pub day_time: i64,
    pub weather: Weather,
    /// Every block position written since the log was last drained, kept
    /// only while the world is open to the network (see `lan`).
    pub block_log: Option<Vec<(i32, i32, i32)>>,
    /// Every write since the mobs last ticked (position, old and new state),
    /// for `ServerLevel.sendBlockUpdated`'s path recomputing; kept only while
    /// the mobs of this world tick (`game/creatures.rs`).
    pub path_writes: Option<Vec<((i32, i32, i32), u32, u32)>>,
    /// The sections whose light changed since the log was last drained, as
    /// a bit per section of each column, kept while `block_log` is
    /// (`ChunkHolder.sectionLightChanged`).
    pub light_log: Option<crate::fast_hash::FxHashMap<(i32, i32), SectionMask>>,
    /// The villager POIs of each section (`PoiManager`'s sections).
    pub pois: crate::poi_sections::PoiSections<crate::poi_sections::Poi>,
    /// HorizonsX: the chunks that came in with post-processing marks still to run (postprocess_chunks), so
    /// a tick looks at these instead of every loaded chunk.
    pub postprocess_keys: FxHashSet<(i32, i32)>,
    /// Moving-piston states removed during the last tick; retained until the
    /// final block mesh is visible.
    pub moving_piston_fallback: Vec<([i32; 3], horizon_world::piston_shape::MovingPistonState)>,
    /// The moving-piston block entities of the level, by position, copied
    /// from the block ticks after each tick: collision and rendering read them.
    pub moving_pistons: Vec<([i32; 3], horizon_world::piston_shape::MovingPistonState)>,
    /// The `ShulkerBoxBlockEntity` lid state of the level, by position, copied
    /// from the block entities after each tick: the collision queries read it
    /// (`ShulkerBoxBlockEntity.getBoundingBox`), as `moving_pistons` for pistons.
    pub shulkers: FxHashMap<[i32; 3], crate::block_entity_anim::Shulker>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Weather {
    pub clear_time: i32,
    pub rain_time: i32,
    pub thunder_time: i32,
    pub raining: bool,
    pub thundering: bool,
    pub rain_level: (f32, f32),
    pub thunder_level: (f32, f32),
}
impl Weather {
    /// The timers of `advanceWeatherCycle`, which run only while
    /// `advance_weather` is on.
    pub fn tick_timers(&mut self, random: &mut horizon_core::random::LegacyRandom) {
        use horizon_core::random::RandomSource;
        let mut uniform =
            |min: i32, max: i32| min + random.next_int_bound(max - min + 1).unwrap_or(0);
        if self.clear_time > 0 {
            self.clear_time -= 1;
            self.thunder_time = if self.thundering { 0 } else { 1 };
            self.rain_time = if self.raining { 0 } else { 1 };
            self.thundering = false;
            self.raining = false;
        } else {
            if self.thunder_time > 0 {
                self.thunder_time -= 1;
                if self.thunder_time == 0 {
                    self.thundering = !self.thundering;
                }
            } else if self.thundering {
                self.thunder_time = uniform(3600, 15600);
            } else {
                self.thunder_time = uniform(12000, 180000);
            }
            if self.rain_time > 0 {
                self.rain_time -= 1;
                if self.rain_time == 0 {
                    self.raining = !self.raining;
                }
            } else if self.raining {
                self.rain_time = uniform(12000, 24000);
            } else {
                self.rain_time = uniform(12000, 180000);
            }
        }
    }
    /// The rain and thunder level fade at the end of `advanceWeatherCycle`,
    /// which runs whatever `advance_weather` says, so `/weather` still shows.
    pub fn tick_levels(&mut self) {
        self.thunder_level.0 = self.thunder_level.1;
        self.thunder_level.1 =
            (self.thunder_level.1 + if self.thundering { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
        self.rain_level.0 = self.rain_level.1;
        self.rain_level.1 =
            (self.rain_level.1 + if self.raining { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
    }
    /// Level.getRainLevel / getThunderLevel (thunder scales with rain).
    pub fn levels(&self, partial: f32) -> (f32, f32) {
        let rain = self.rain_level.0 + (self.rain_level.1 - self.rain_level.0) * partial;
        let thunder =
            self.thunder_level.0 + (self.thunder_level.1 - self.thunder_level.0) * partial;
        (rain, thunder * rain)
    }
}
impl World {
    pub fn new(air: BlockStateId, seed: i64) -> Self {
        Self {
            shape: LevelShape::OVERWORLD,
            chunks: Default::default(),
            random: LegacyRandom::new(seed),
            revision: 0,
            health_rules: HealthRules::default(),
            air,
            fast_lava: false,
            dirty_sections: Default::default(),
            dirty_light: Default::default(),
            light_edits: Vec::new(),
            loaded_entities: Vec::new(),
            loaded_chunks: Vec::new(),
            modified: FxHashSet::default(),
            block_entities: Default::default(),
            chest_openers: FxHashMap::default(),
            block_names: FxHashMap::default(),
            watched: Vec::new(),
            watched_writes: Vec::new(),
            inhabited: FxHashMap::default(),
            game_time: 0,
            day_time: 1000,
            weather: Weather::default(),
            block_log: None,
            path_writes: None,
            light_log: None,
            moving_piston_fallback: Vec::new(),
            moving_pistons: Vec::new(),
            shulkers: FxHashMap::default(),
            pois: Default::default(),
            postprocess_keys: FxHashSet::default(),
        }
    }
    /// HorizonsX's regions (Folia's `RegionizedWorldData`): a world for each region, holding the region's
    /// chunks and their block entities, moved out of this one, and copies of the level-wide values, for a
    /// region's block ticks on its own core. What the ticks change comes back with [`World::take_back`].
    /// To a region every chunk not lent to it is not loaded (Folia's rule for chunks a region doesn't own).
    pub fn lend(&mut self, regions: usize, region_of: impl Fn((i32, i32)) -> Option<usize> + Sync, seeds: &[i64]) -> Vec<World> {
        // HorizonsX: the chunks, the block entities and the inhabited times move on threads of their own at once
        // (one after another they were the split's longest part, 9 ms with 300 spread players).
        let (map, block_entities, inhabited_of) = (&mut self.chunks, &mut self.block_entities, &mut self.inhabited);
        let of = &region_of;
        let (chunks, mut entities, mut inhabited) = std::thread::scope(|scope| {
            let entities = scope.spawn(move || {
                horizon_threads::favour_current();
                block_entities.lend(regions, |p| of((p.0 >> 4, p.2 >> 4)))
            });
            let inhabited = scope.spawn(move || {
                horizon_threads::favour_current();
                let mut out: Vec<FxHashMap<(i32, i32), i64>> = (0..regions).map(|_| FxHashMap::default()).collect();
                for (key, t) in inhabited_of.extract_if(|key, _| of(*key).is_some()) {
                    if let Some(r) = of(key) {
                        out[r].insert(key, t);
                    }
                }
                out
            });
            let chunks = map.lend(regions, of);
            match (entities.join(), inhabited.join()) {
                (Ok(e), Ok(i)) => (chunks, e, i),
                (Err(e), _) | (_, Err(e)) => std::panic::resume_unwind(e),
            }
        });
        let mut out: Vec<World> = chunks
            .into_iter()
            .enumerate()
            .map(|(r, chunks)| World {
                shape: self.shape,
                fast_lava: self.fast_lava,
                chunks,
                random: LegacyRandom::new(seeds.get(r).copied().unwrap_or(r as i64)),
                revision: self.revision,
                health_rules: self.health_rules.clone(),
                air: self.air,
                dirty_sections: Default::default(),
                dirty_light: Default::default(),
                light_edits: Vec::new(),
                loaded_entities: Vec::new(),
                loaded_chunks: Vec::new(),
                modified: FxHashSet::default(),
                block_entities: std::mem::take(&mut entities[r]),
                chest_openers: FxHashMap::default(),
                block_names: FxHashMap::default(),
                watched: self.watched.clone(),
                watched_writes: Vec::new(),
                inhabited: std::mem::take(&mut inhabited[r]),
                game_time: self.game_time,
                day_time: self.day_time,
                weather: self.weather,
                block_log: self.block_log.as_ref().map(|_| Vec::new()),
                path_writes: self.path_writes.as_ref().map(|_| Vec::new()),
                light_log: self.light_log.as_ref().map(|_| FxHashMap::default()),
                moving_piston_fallback: Vec::new(),
                moving_pistons: Vec::new(),
                shulkers: FxHashMap::default(),
                pois: Default::default(),
                postprocess_keys: FxHashSet::default(),
            })
            .collect();
        // What a region's ticks read of these, by position: copies of the region's own.
        for (&pos, &v) in &self.chest_openers {
            if let Some(r) = region_of((pos.0 >> 4, pos.2 >> 4)) {
                out[r].chest_openers.insert(pos, v);
            }
        }
        for (pos, name) in &self.block_names {
            if let Some(r) = region_of((pos.0 >> 4, pos.2 >> 4)) {
                out[r].block_names.insert(*pos, name.clone());
            }
        }
        // The block and light changes not sent yet (a command's, made on the server's thread) go with their chunks:
        // a region's thread sends its own players its chunks' changes (else nobody would, its players being away).
        if let Some(log) = self.block_log.as_mut() {
            for p in log.extract_if(.., |p| region_of((p.0 >> 4, p.2 >> 4)).is_some()) {
                if let Some(r) = region_of((p.0 >> 4, p.2 >> 4)) {
                    out[r].block_log.get_or_insert_with(Vec::new).push(p);
                }
            }
        }
        if let Some(log) = self.light_log.as_mut() {
            for (key, mask) in log.extract_if(|key, _| region_of(*key).is_some()) {
                if let Some(r) = region_of(key) {
                    out[r].light_log.get_or_insert_with(Default::default).insert(key, mask);
                }
            }
        }
        // HorizonsX: the inhabited times moved out with their chunks above (and back with `take_back`), not copied.
        out
    }
    /// An empty world with this one's level-wide values (shape, time, weather, rules), to stand in its place
    /// while HorizonsX's largest region ticks on the world itself: whatever reads the level's time or
    /// weather meanwhile reads the same.
    pub fn stand_in(&self) -> World {
        let mut w = World::new(self.air, 0);
        w.shape = self.shape;
        w.fast_lava = self.fast_lava;
        w.health_rules = self.health_rules.clone();
        w.game_time = self.game_time;
        w.day_time = self.day_time;
        w.weather = self.weather;
        w.revision = self.revision;
        w
    }
    /// A region's world from [`World::lend`] back in this one: its chunks and block entities, and what its
    /// ticks logged (sections to remesh, light to redo, writes), after this world's own.
    pub fn take_back(&mut self, region: World) {
        let World {
            chunks,
            revision,
            dirty_sections,
            dirty_light,
            light_edits,
            modified,
            block_entities,
            chest_openers,
            block_names,
            watched_writes,
            inhabited,
            block_log,
            path_writes,
            light_log,
            ..
        } = region;
        // Each region counted its own writes from the same start: the total keeps the count going up.
        self.revision = self.revision.max(revision);
        self.chunks.take_back(chunks);
        self.block_entities.take_back(block_entities);
        for ((x, z), mask) in dirty_sections.columns {
            let entry = self.dirty_sections.columns.entry((x, z)).or_default();
            for s in mask.iter() {
                entry.insert(s);
            }
        }
        self.dirty_light.extend(dirty_light);
        self.light_edits.extend(light_edits);
        self.modified.extend(modified);
        self.chest_openers.extend(chest_openers);
        self.block_names.extend(block_names);
        self.watched_writes.extend(watched_writes);
        // The region's chunks' inhabited time, which its natural spawning counts up (copies of its own were lent).
        self.inhabited.extend(inhabited);
        if let (Some(all), Some(log)) = (self.block_log.as_mut(), block_log) {
            all.extend(log);
        }
        if let (Some(all), Some(log)) = (self.path_writes.as_mut(), path_writes) {
            all.extend(log);
        }
        if let (Some(all), Some(log)) = (self.light_log.as_mut(), light_log) {
            for (key, mask) in log {
                let entry = all.entry(key).or_default();
                for s in mask.iter() {
                    entry.insert(s);
                }
            }
        }
    }
    /// A controlled blockstate scene for the native scene probes: material
    /// layers, a stepped pyramid and the model gallery. Not generated terrain.
    pub fn test_scene(
        assets: &crate::assets::Assets,
        render: &crate::assets::RenderAssets,
        radius: i32,
    ) -> crate::Result<Self> {
        let air = assets
            .registry
            .default_state("minecraft:air")
            .ok_or("missing air state")?;
        let mut world = Self::new(air, 0);
        for cz in -radius..=radius {
            for cx in -radius..=radius {
                world
                    .chunks
                    .insert((cx, cz), Arc::new(Chunk::empty(cx, cz)));
                for z in 0..16 {
                    for x in 0..16 {
                        for y in 0..4 {
                            let layer = match y {
                                0 => 0,
                                1 | 2 => 1,
                                _ if (cx + cz) & 1 == 0 => 2,
                                _ => 3,
                            };
                            world.set(
                                cx * 16 + x,
                                y,
                                cz * 16 + z,
                                assets.material_states[layer],
                            )?;
                        }
                    }
                }
            }
        }
        for y in 4..11 {
            for z in -10_i32..=10 {
                for x in -10_i32..=10 {
                    if x.abs().max(z.abs()) <= 10 - (y - 4) {
                        world.set(
                            x,
                            y,
                            z,
                            assets.material_states[if y & 1 == 0 { 4 } else { 5 }],
                        )?;
                    }
                }
            }
        }
        for (position, state) in &render.model_placements {
            world.set(position.x, position.y, position.z, *state)?;
        }
        world.revision = 0;
        Ok(world)
    }
    /// The column the level's chunks hold.
    pub fn layout(&self) -> ColumnLayout {
        self.shape.column()
    }
    /// The lowest block Y the level's chunks hold.
    pub fn bottom_y(&self) -> i32 {
        self.layout().min_y()
    }
    /// One above the highest block Y the level's chunks hold.
    pub fn top_y(&self) -> i32 {
        self.layout().max_y()
    }
    /// Resident storage write; marks the touched sections (and neighbours on
    /// section borders) for remeshing and the surrounding chunks for relighting.
    /// Neighbour updates, drops and block entities belong to the gameplay layer.
    pub fn set(&mut self, x: i32, y: i32, z: i32, state: BlockStateId) -> crate::Result<()> {
        let Some(chunk) = self.chunks.get_mut(&(x >> 4, z >> 4)) else {
            return Err("write outside resident chunks".into());
        };
        let (min_y, sections) = (chunk.min_y(), chunk.sections.len() as i32);
        if chunk.section_index(y).is_none() {
            return Err("write outside world height".into());
        }
        let previous = Arc::make_mut(chunk).set(x & 15, y, z & 15, state.0);
        if previous != state.0 {
            self.block_entities
                .state_changed((x, y, z), previous, state.0);
            let watches = |id: u32| self.watched.get(id as usize).copied().unwrap_or(false);
            if watches(previous) || watches(state.0) {
                self.watched_writes.push(((x, y, z), previous, state.0));
            }
            self.revision = self.revision.wrapping_add(1);
            if let Some(log) = &mut self.block_log {
                log.push((x, y, z));
            }
            if let Some(writes) = &mut self.path_writes {
                writes.push(((x, y, z), previous, state.0));
            }
            self.modified.insert((x >> 4, z >> 4));
            // The mesher reads a 3x3x3 neighbourhood for AO, so a write marks
            // the sections it can reach: this section, the one above/below
            // only on a section face, and a neighbour column only on a chunk
            // border (the diagonal only where both borders meet).
            let sy = (y - min_y) >> 4;
            let local_y = y & 15;
            let mark = |dirty: &mut DirtySections, bx: i32, bz: i32| {
                dirty.insert((bx >> 4, sy, bz >> 4));
                if local_y == 0 && sy > 0 {
                    dirty.insert((bx >> 4, sy - 1, bz >> 4));
                }
                if local_y == 15 && sy + 1 < sections {
                    dirty.insert((bx >> 4, sy + 1, bz >> 4));
                }
            };
            let (bx, bz) = (x & 15, z & 15);
            mark(&mut self.dirty_sections, x, z);
            if bx == 0 {
                mark(&mut self.dirty_sections, x - 1, z);
            } else if bx == 15 {
                mark(&mut self.dirty_sections, x + 1, z);
            }
            if bz == 0 {
                mark(&mut self.dirty_sections, x, z - 1);
            } else if bz == 15 {
                mark(&mut self.dirty_sections, x, z + 1);
            }
            if (bx == 0 || bx == 15) && (bz == 0 || bz == 15) {
                mark(
                    &mut self.dirty_sections,
                    if bx == 0 { x - 1 } else { x + 1 },
                    if bz == 0 { z - 1 } else { z + 1 },
                );
            }
            if self.chunks.get(&(x >> 4, z >> 4)).is_some_and(|c| c.lit) {
                self.light_edits.push(((x, y, z), previous, state.0));
            } else {
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        self.dirty_light.insert(((x >> 4) + dx, (z >> 4) + dz));
                    }
                }
            }
        }
        Ok(())
    }
    /// `getHeight(Heightmap.Types.WORLD_SURFACE, x, z)`: one above the highest
    /// block that isn't `air`, or the column's minimum Y (also where the chunk
    /// isn't loaded).
    pub fn world_surface(&self, x: i32, z: i32, air: impl Fn(BlockStateId) -> bool) -> i32 {
        let bottom = self.layout().min_y();
        self.chunks.get(&(x >> 4, z >> 4)).map_or(bottom, |chunk| {
            chunk.world_surface(x, z, |v| air(BlockStateId(v)))
        })
    }
    /// `getHeight(Heightmap.Types.MOTION_BLOCKING, x, z)`: one above the
    /// highest block where `blocks` (motion blocking or holding a fluid),
    /// or the column's minimum Y (also where the chunk isn't loaded).
    pub fn motion_blocking(&self, x: i32, z: i32, blocks: impl Fn(BlockStateId) -> bool) -> i32 {
        let bottom = self.layout().min_y();
        self.chunks.get(&(x >> 4, z >> 4)).map_or(bottom, |chunk| {
            chunk.motion_blocking(x & 15, z & 15, |v| blocks(BlockStateId(v)))
        })
    }
    pub fn block(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        self.chunks
            .get(&(x >> 4, z >> 4))
            .map_or(self.air, |chunk| chunk.block(x & 15, y, z & 15))
    }
    /// Whether a block of the box `lo..=hi` may match, from the palettes of
    /// the sections it reaches (a palette of the whole registry may hold
    /// anything): `false` means [`Self::block`] matches in none of its cells,
    /// so a scan over them can stop before reading one.
    pub fn may_hold(
        &self,
        lo: [i32; 3],
        hi: [i32; 3],
        mut matches: impl FnMut(BlockStateId) -> bool,
    ) -> bool {
        let layout = self.layout();
        let (bottom, top) = (layout.min_y(), layout.max_y() - 1);
        // Heights outside the sections read as state 0.
        if (lo[1] < bottom || hi[1] > top) && matches(BlockStateId(0)) {
            return true;
        }
        let sections = (lo[1].max(bottom) - bottom) >> 4..=(hi[1].min(top) - bottom) >> 4;
        for cx in lo[0] >> 4..=hi[0] >> 4 {
            for cz in lo[2] >> 4..=hi[2] >> 4 {
                let Some(chunk) = self.chunks.get(&(cx, cz)) else {
                    if matches(self.air) {
                        return true;
                    }
                    continue;
                };
                for s in sections.clone() {
                    let blocks = &chunk.sections[s as usize].blocks;
                    if blocks.is_global() || blocks.maybe_has(|v| matches(BlockStateId(v))) {
                        return true;
                    }
                }
            }
        }
        false
    }
    /// The block entities to tick this tick, in order (`BlockEntities::tickers`).
    pub fn block_entity_tickers(
        &mut self,
    ) -> std::sync::Arc<Vec<(crate::block_entities::Pos, u32)>> {
        let (chunks, air) = (&self.chunks, self.air);
        self.block_entities.tickers(|(x, y, z)| {
            chunks
                .get(&(x >> 4, z >> 4))
                .map_or(air, |chunk| chunk.block(x & 15, y, z & 15))
                .0
        })
    }
    pub fn light(&self, x: i32, y: i32, z: i32) -> u8 {
        self.chunks
            .get(&(x >> 4, z >> 4))
            .map_or(0xF0, |chunk| chunk.light(x & 15, y, z & 15))
    }
    pub fn is_air(&self, state: BlockStateId) -> bool {
        state == self.air
    }
    pub fn loaded(&self, x: i32, z: i32) -> bool {
        self.chunks.contains_key(&(x >> 4, z >> 4))
    }
    /// `LockCode`: the `lock` a container block entity refuses to open
    /// without, as the tag it was loaded or set with carries it (Horizon's
    /// model holds no field for it).
    pub fn block_lock(&self, key: (i32, i32, i32)) -> Option<&horizon_world::nbt::Tag> {
        self.chunks
            .get(&(key.0 >> 4, key.2 >> 4))?
            .saved
            .as_ref()?
            .block_entity_sources
            .iter()
            .find(|(p, _)| *p == key)?
            .1
            .get("lock")
    }
    /// `LockCode.addToTag` for a tag a command gives a container (`setblock`,
    /// `data merge`): the tag it was set with keeps the lock, so `data get`
    /// shows it, a player's open checks it and the save writes it.
    pub fn set_block_lock(&mut self, key: (i32, i32, i32), tag: &horizon_world::nbt::Tag) {
        use horizon_world::nbt::Tag;
        let lock = tag.get("lock").and_then(|lock| match lock {
            Tag::Compound(_) => Some(lock.clone()),
            _ => None,
        });
        self.with_chunk_extras(key, |extras| {
            let at = extras
                .block_entity_sources
                .iter()
                .position(|(p, _)| *p == key);
            match at {
                Some(i) => {
                    if let Tag::Compound(source) = &mut extras.block_entity_sources[i].1 {
                        match lock {
                            Some(lock) => {
                                source.insert("lock".into(), lock);
                            }
                            None => {
                                source.remove(&"lock".into());
                            }
                        }
                    }
                }
                // A container set at runtime has no source yet: the tag the
                // command gave is the one it was set with, `id` and position
                // included (the save checks the id before merging).
                None if lock.is_some() => extras.block_entity_sources.push((key, tag.clone())),
                None => {}
            }
        });
    }
    /// `LevelChunk.removeBlockEntity`: the tag a block entity was loaded or set
    /// with goes with it, so a container placed there later starts unlocked.
    pub fn forget_block_entity_source(&mut self, key: (i32, i32, i32)) {
        self.with_chunk_extras(key, |extras| {
            extras.block_entity_sources.retain(|(p, _)| *p != key);
        });
    }
    /// Run `f` on the keep-as-loaded data of the chunk holding `key`, put back
    /// for the next save (`block_entity_sources` is where Horizon restores the
    /// block entity fields its model does not hold).
    fn with_chunk_extras(
        &mut self,
        key: (i32, i32, i32),
        f: impl FnOnce(&mut crate::chunk::ChunkExtras),
    ) {
        let chunk_key = (key.0 >> 4, key.2 >> 4);
        let Some(chunk) = self.chunks.get_mut(&chunk_key) else {
            return;
        };
        let chunk = Arc::make_mut(chunk);
        let mut extras = chunk
            .saved
            .take()
            .map(Arc::unwrap_or_clone)
            .unwrap_or_default();
        f(&mut extras);
        chunk.saved = Some(Arc::new(extras));
        self.modified.insert(chunk_key);
    }
}

/// The game rules and difficulty a player's health reads (`Player.isInvulnerableTo`,
/// natural regeneration), kept with the world they belong to.
#[derive(Clone, Copy, Debug)]
pub struct HealthRules {
    pub difficulty: horizon_world::food::Difficulty,
    pub natural_regeneration: bool,
    /// The `drowning_damage`, `fall_damage`, `fire_damage` and
    /// `freeze_damage` rules `Player.isInvulnerableTo` reads.
    pub drowning_damage: bool,
    pub fall_damage: bool,
    pub fire_damage: bool,
    pub freeze_damage: bool,
}
impl Default for HealthRules {
    fn default() -> Self {
        Self {
            difficulty: horizon_world::food::Difficulty::Normal,
            natural_regeneration: true,
            drowning_damage: true,
            fall_damage: true,
            fire_damage: true,
            freeze_damage: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DirtySections, World};
    use crate::chunk::{Chunk, MIN_Y, SECTIONS};
    use horizon_world::registry::BlockStateId;
    use std::sync::Arc;

    /// Every block write marks the same section set the 27-cell loop did, but
    /// only the distinct keys: one in the interior, a neighbour section on a
    /// section face, a neighbour column on a chunk border and the diagonal at
    /// a corner (eight keys at a corner on a section face).
    #[test]
    fn writes_mark_exactly_the_reachable_sections() {
        let mut world = World::new(BlockStateId(0), 0);
        world.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        for (x, y, z) in [
            (3, 70, 3),
            (0, 70, 3),
            (15, 70, 3),
            (3, 70, 0),
            (0, 70, 0),
            (15, 70, 15),
            (0, MIN_Y, 0),
            (0, 80, 0),
            (15, MIN_Y + SECTIONS as i32 * 16 - 1, 15),
        ] {
            world.dirty_sections.clear();
            world.set(x, y, z, BlockStateId(1)).unwrap();
            let mut expected = DirtySections::default();
            for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (bx, by, bz) = (x + dx, y + dy, z + dz);
                        let s = (by - MIN_Y) >> 4;
                        if (0..SECTIONS as i32).contains(&s) {
                            expected.insert((bx >> 4, s, bz >> 4));
                        }
                    }
                }
            }
            let mut got: Vec<_> = world.dirty_sections.columns().collect();
            let mut want: Vec<_> = expected.columns().collect();
            got.sort_unstable_by_key(|&(column, _)| column);
            want.sort_unstable_by_key(|&(column, _)| column);
            assert_eq!(got, want, "write at {x} {y} {z}");
            if (x & 15, z & 15) == (3, 3) {
                assert_eq!(world.dirty_sections.len(), 1);
            }
            if (x & 15, z & 15, y & 15) == (0, 0, 0) && y > MIN_Y {
                // Four columns, each with the section below and this one.
                assert_eq!(world.dirty_sections.len(), 8);
            }
        }
    }

    /// A box misses a state only where no section it reaches lists it, and
    /// unloaded columns or heights past the world count as their air.
    #[test]
    fn may_hold_reads_the_palettes_of_the_reached_sections() {
        let mut world = World::new(BlockStateId(0), 0);
        let mut chunk = Chunk::empty(0, 0);
        chunk.set(3, 70, 3, 9);
        world.chunks.insert((0, 0), Arc::new(chunk));
        let nine = |s: BlockStateId| s.0 == 9;
        // The section of y 64..80 holds 9, the one below does not.
        assert!(world.may_hold([0, 64, 0], [1, 65, 1], nine));
        assert!(world.may_hold([10, 79, 10], [12, 80, 12], nine));
        assert!(!world.may_hold([0, 50, 0], [15, 63, 15], nine));
        // A box over the unloaded column next door sees only air.
        assert!(!world.may_hold([16, 70, 0], [17, 71, 1], nine));
        assert!(world.may_hold([16, 70, 0], [17, 71, 1], |s| s.0 == 0));
        // Past the top, and the palette of air below it.
        assert!(!world.may_hold([0, 400, 0], [0, 401, 0], nine));
        assert!(world.may_hold([0, 400, 0], [0, 401, 0], |s| s.0 == 0));
    }
}
