//! LevelChunk.postProcessGeneration: positions that worldgen marked for
//! postprocessing (aquifer fluids next to open air, features' shape-dependent
//! blocks) are updated once the chunk is live, which starts cave waterfalls
//! and lava falls, raises bubble columns over magma and soul sand and fixes
//! block shapes against their neighbours.
use super::Game;
use horizon_client::{Result, blocks::Fluid};
use horizon_core::position::BlockPos;
use std::sync::Arc;

/// Block.UPDATE_INVISIBLE | UPDATE_KNOWN_SHAPE | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS.
const POSTPROCESS_FLAGS: i32 = 276;

impl Game<'_> {
    pub(super) fn postprocess_chunks(&mut self) -> Result<()> {
        // HorizonsX: the chunks that came in with marks (`World::postprocess_keys`), not every loaded chunk
        // each tick; once every 100 ticks every chunk again, for one that came in another way.
        if self.world.game_time.rem_euclid(100) == 0 {
            let missed: Vec<(i32, i32)> = self
                .world
                .chunks
                .iter()
                .filter(|(_, c)| !c.postprocess.is_empty())
                .map(|(k, _)| *k)
                .collect();
            self.world.postprocess_keys.extend(missed);
        }
        if self.world.postprocess_keys.is_empty() {
            return Ok(());
        }
        let chunks = &self.world.chunks;
        // A chunk gone, or with no marks left, leaves the set.
        self.world
            .postprocess_keys
            .retain(|k| chunks.get(k).is_some_and(|c| !c.postprocess.is_empty()));
        let mut ready: Vec<(i32, i32)> = self
            .world
            .postprocess_keys
            .iter()
            .copied()
            .filter(|k| chunks.get(k).is_some_and(|c| c.lit))
            .filter(|&(x, z)| (-1..=1).all(|dz| (-1..=1).all(|dx| chunks.contains_key(&(x + dx, z + dz)))))
            .collect();
        // A fixed order (the whole-map walk's was the hash map's own).
        ready.sort_unstable();
        if ready.is_empty() {
            return Ok(());
        }
        // The ticks the marks schedule go to the chunks' containers, which a
        // chunk that arrived since the last tick doesn't have yet
        // (`LevelChunk.registerTickContainerInLevel` comes before
        // `postProcessGeneration`).
        self.sync_tick_chunks();
        for key in ready {
            let Some(chunk) = self.world.chunks.get_mut(&key) else {
                continue;
            };
            let marks = std::mem::take(&mut Arc::make_mut(chunk).postprocess);
            self.with_level(|b, level, ticks| {
                for [x, y, z] in marks {
                    let pos = BlockPos::new(x, y, z);
                    let state = level.world.block(x, y, z);
                    let facts = level.facts.get(state);
                    let (fluid, liquid_block) = (
                        facts.fluid != Fluid::None,
                        matches!(&*facts.name, "minecraft:water" | "minecraft:lava"),
                    );
                    if fluid {
                        b.scheduled_fluid_tick(level, ticks, pos);
                    }
                    if liquid_block {
                        // LiquidBlock.tick: bubble columns over soul sand and magma.
                        b.scheduled_block_tick(level, ticks, pos);
                    } else {
                        let updated = b.update_from_neighbour_shapes(level, ticks, pos, state);
                        if updated != state {
                            b.set_block(level, ticks, pos, updated, POSTPROCESS_FLAGS);
                        }
                    }
                }
            });
        }
        self.process_level_events()
    }
}
