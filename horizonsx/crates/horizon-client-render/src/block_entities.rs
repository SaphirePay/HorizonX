//! The world's block entities and the order they tick in: vanilla's
//! `Level.blockEntityTickers`, kept beside the block entity map so a tick
//! visits only the block entities that tick, in vanilla's order, instead of
//! scanning the whole map once per kind.
//!
//! The list follows `LevelChunk.updateBlockEntityTicker` and
//! `Level.tickBlockEntities`:
//! - a block entity whose block state has a ticker gets a slot at the end of
//!   the list (through `pendingBlockEntityTickers`, so a slot made during a
//!   tick first ticks on the next one);
//! - replacing the block entity at a position (`setBlockEntity`), or changing
//!   its block state to another state with a ticker, keeps its slot
//!   (`RebindableTickingBlockEntityWrapper.rebind`);
//! - removing it (`removeBlockEntity`), unloading its chunk
//!   (`clearAllBlockEntities`) or changing its state to one without a ticker
//!   (a jukebox losing its record) frees the slot; a later ticker there is
//!   appended again;
//! - a loaded chunk's block entities are added in the iteration order of its
//!   `Object2ObjectOpenHashMap<BlockPos, BlockEntity>`
//!   (`registerAllBlockEntitiesAfterLevelLoad`), filled in the order the chunk
//!   listed them.
//!
//! Which states tick is a table by state id, built by the game from the block
//! registry; whether a slot is made is decided when the slot is promoted at
//! the start of the next tick, from the block state then.
use crate::chunk::block_entity::{BlockEntity, ModContainer, ModFluidTanks};
use crate::fast_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

pub type Pos = (i32, i32, i32);

/// What a block state's block entity does each tick (`BlockState.getTicker`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ticking {
    /// No ticker, and the block has no block entity that could have one.
    #[default]
    No,
    /// A ticker.
    Yes,
    /// The block's block entity, but this state has no ticker (a jukebox
    /// without a record): a state change to it frees the slot.
    Idle,
}

#[derive(Default)]
pub struct BlockEntities {
    map: FxHashMap<Pos, BlockEntity>,
    /// `Level.blockEntityTickers`: positions and the slot each was given.
    tickers: Arc<Vec<(Pos, u32)>>,
    /// `LevelChunk.tickersInLevel`: the live slot of each ticking position.
    live: FxHashMap<Pos, u32>,
    /// Positions that may need a slot, in the order vanilla would have made
    /// it (`pendingBlockEntityTickers`), each with a token; decided at the
    /// next promotion from the block state then.
    added: Vec<(Pos, u32)>,
    /// The token of each position's pending entry: an entry whose token is
    /// not here any more was cancelled.
    pending: FxHashMap<Pos, u32>,
    next_slot: u32,
    /// Whether a slot was freed since the list was last swept: otherwise
    /// every slot in it is still live, unless its block entity is `out`.
    freed: bool,
    /// Positions whose block entity `take` took and `put` has not brought
    /// back.
    out: FxHashSet<Pos>,
    /// Positions a loaded chunk gave a new block entity over an old one: at
    /// the next tick their slot stays only if their block state ticks
    /// (`updateBlockEntityTicker`: rebind, or remove without a ticker).
    replaced: FxHashSet<Pos>,
    /// `Ticking` by block state id; empty until the game builds it.
    states: Arc<[Ticking]>,
    /// The containers of mod blocks' block entities, by block state id;
    /// built with `states`.
    mod_containers: FxHashMap<u32, Arc<ModContainer>>,
    /// The fluid tanks of mod blocks' block entities, by block state id;
    /// built with `states`, like the containers.
    mod_fluids: FxHashMap<u32, Arc<ModFluidTanks>>,
    /// HorizonsX: a region's lent set records the slots it makes (true) and frees, for `take_back`.
    journal: Option<Vec<(Pos, bool)>>,
}

impl std::ops::Deref for BlockEntities {
    type Target = FxHashMap<Pos, BlockEntity>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl<'a> IntoIterator for &'a BlockEntities {
    type Item = (&'a Pos, &'a BlockEntity);
    type IntoIter = std::collections::hash_map::Iter<'a, Pos, BlockEntity>;
    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

/// `Vec3i.hashCode`.
fn java_hash((x, y, z): Pos) -> i32 {
    y.wrapping_add(z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(x)
}
/// `HashCommon.mix`.
fn mix(x: i32) -> usize {
    let h = (x as u32).wrapping_mul(0x9E37_79B9);
    (h ^ (h >> 16)) as usize
}

/// The order `values().forEach` visits a `new Object2ObjectOpenHashMap<>()`
/// keyed by these positions, put in this order (repeats keep their first
/// slot): open addressing with linear probing, a rehash to twice the size
/// past three quarters full, and iteration from the last slot down.
pub fn chunk_map_order(positions: impl IntoIterator<Item = Pos>) -> Vec<Pos> {
    // HashCommon.arraySize(16, .75f), and maxFill of each size.
    let mut slots: Vec<Option<Pos>> = vec![None; 32];
    let max_fill = |n: usize| (n * 3).div_ceil(4).min(n - 1);
    let mut size = 0;
    for p in positions {
        let mask = slots.len() - 1;
        let mut at = mix(java_hash(p)) & mask;
        let mut known = false;
        while let Some(k) = slots[at] {
            if k == p {
                known = true;
                break;
            }
            at = (at + 1) & mask;
        }
        if known {
            continue;
        }
        slots[at] = Some(p);
        // `if (size++ >= maxFill) rehash(arraySize(size + 1, f))`, which
        // puts the keys back from the last slot down.
        size += 1;
        if size > max_fill(slots.len()) {
            let n = ((size + 1) * 4).div_ceil(3).next_power_of_two();
            let old = std::mem::replace(&mut slots, vec![None; n]);
            for k in old.into_iter().rev().flatten() {
                let mut at = mix(java_hash(k)) & (n - 1);
                while slots[at].is_some() {
                    at = (at + 1) & (n - 1);
                }
                slots[at] = Some(k);
            }
        }
    }
    slots.into_iter().rev().flatten().collect()
}

impl BlockEntities {
    /// Whether the ticking table has been built.
    pub fn has_states(&self) -> bool {
        !self.states.is_empty()
    }
    pub fn set_states(&mut self, states: Vec<Ticking>) {
        self.states = states.into();
    }
    pub fn ticking(&self, state: u32) -> Ticking {
        self.states.get(state as usize).copied().unwrap_or_default()
    }
    pub fn set_mod_containers(&mut self, containers: FxHashMap<u32, Arc<ModContainer>>) {
        self.mod_containers = containers;
    }
    /// The container of a mod block's block entity in block state `state`.
    pub fn mod_container(&self, state: u32) -> Option<&Arc<ModContainer>> {
        self.mod_containers.get(&state)
    }
    pub fn set_mod_fluids(&mut self, fluids: FxHashMap<u32, Arc<ModFluidTanks>>) {
        self.mod_fluids = fluids;
    }
    /// The fluid tanks of a mod block's block entity in block state `state`.
    pub fn mod_fluids(&self, state: u32) -> Option<&Arc<ModFluidTanks>> {
        self.mod_fluids.get(&state)
    }

    fn next(&mut self) -> u32 {
        let n = self.next_slot;
        self.next_slot = n.wrapping_add(1);
        n
    }
    /// A ticker may be made for `pos` now (`updateBlockEntityTicker`).
    fn add(&mut self, pos: Pos) {
        if let Some(journal) = self.journal.as_mut() {
            journal.push((pos, true));
        }
        let token = self.next();
        self.pending.insert(pos, token);
        self.added.push((pos, token));
    }
    /// `removeBlockEntityTicker`: no slot, made or pending.
    fn cancel(&mut self, pos: &Pos) {
        if let Some(journal) = self.journal.as_mut() {
            journal.push((*pos, false));
        }
        self.freed |= self.live.remove(pos).is_some();
        self.pending.remove(pos);
    }
    /// HorizonsX's regions: the block entities each region's chunks hold, moved out into one set each,
    /// with copies of their slots, for the region's ticks on its own core. The region's set keeps a journal
    /// of the slots it made and freed; [`BlockEntities::take_back`] replays it here, so the tick list
    /// stays this set's.
    pub fn lend(&mut self, regions: usize, region_of: impl Fn(Pos) -> Option<usize>) -> Vec<BlockEntities> {
        let mut out: Vec<BlockEntities> = (0..regions)
            .map(|_| BlockEntities {
                states: self.states.clone(),
                mod_containers: self.mod_containers.clone(),
                mod_fluids: self.mod_fluids.clone(),
                next_slot: self.next_slot,
                journal: Some(Vec::new()),
                ..Default::default()
            })
            .collect();
        // HorizonsX: each region's share of the ticking list, in the list's order. Lent without it, a region's
        // ticks found nothing to tick: the furnaces, hoppers, brewing stands, campfires and spawners of every
        // region stood still whenever the level ran as regions (only those placed during the region's tick ticked).
        let mut tickers: Vec<Vec<(Pos, u32)>> = (0..regions).map(|_| Vec::new()).collect();
        for &(pos, slot) in self.tickers.iter() {
            if let Some(r) = region_of(pos).filter(|&r| r < regions) {
                tickers[r].push((pos, slot));
            }
        }
        for (r, list) in tickers.into_iter().enumerate() {
            out[r].tickers = Arc::new(list);
        }
        for (pos, entity) in self.map.extract_if(|pos, _| region_of(*pos).is_some()) {
            let r = region_of(pos).expect("extracted for its region");
            if let Some(&slot) = self.live.get(&pos) {
                out[r].live.insert(pos, slot);
            }
            if let Some(&token) = self.pending.get(&pos) {
                out[r].pending.insert(pos, token);
            }
            out[r].map.insert(pos, entity);
        }
        out
    }
    /// A region's set from [`BlockEntities::lend`] back: its block entities, and the slots its ticks made
    /// and freed, in the order they did.
    pub fn take_back(&mut self, region: BlockEntities) {
        self.map.reserve(region.map.len());
        self.map.extend(region.map);
        self.out.extend(region.out);
        for (pos, made) in region.journal.unwrap_or_default() {
            if made {
                self.add(pos);
            } else {
                self.cancel(&pos);
            }
        }
    }

    pub fn get_mut(&mut self, pos: &Pos) -> Option<&mut BlockEntity> {
        self.map.get_mut(pos)
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Pos, &mut BlockEntity)> {
        self.map.iter_mut()
    }
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut BlockEntity> {
        self.map.values_mut()
    }
    /// `LevelChunk.setBlockEntity` (+ `addAndRegisterBlockEntity`): a new
    /// block entity may get a slot; one replacing another keeps its slot.
    pub fn insert(&mut self, pos: Pos, entity: BlockEntity) -> Option<BlockEntity> {
        let old = self.map.insert(pos, entity);
        if old.is_none() {
            self.add(pos);
        }
        old
    }
    /// The block entity at `pos`, made by `make` when there is none.
    pub fn get_or_insert_with(
        &mut self,
        pos: Pos,
        make: impl FnOnce() -> BlockEntity,
    ) -> &mut BlockEntity {
        if !self.map.contains_key(&pos) {
            self.add(pos);
        }
        self.map.entry(pos).or_insert_with(make)
    }
    /// `LevelChunk.removeBlockEntity`: the block entity and its slot.
    pub fn remove(&mut self, pos: &Pos) -> Option<BlockEntity> {
        self.cancel(pos);
        self.map.remove(pos)
    }
    /// The block entity at `pos`, out of the map for a while: its slot stays
    /// for `put` to bring it back (a tick working on it).
    pub fn take(&mut self, pos: &Pos) -> Option<BlockEntity> {
        let taken = self.map.remove(pos);
        if taken.is_some() {
            self.out.insert(*pos);
        }
        taken
    }
    /// Brings back what `take` took, in its slot.
    pub fn put(&mut self, pos: Pos, entity: BlockEntity) {
        self.out.remove(&pos);
        if self.map.insert(pos, entity).is_none()
            && !self.live.contains_key(&pos)
            && !self.pending.contains_key(&pos)
        {
            self.add(pos);
        }
    }
    /// Removes the block entities `keep` refuses, and their slots
    /// (`clearAllBlockEntities` of unloaded chunks).
    pub fn retain(&mut self, mut keep: impl FnMut(&Pos, &mut BlockEntity) -> bool) {
        let (live, pending, freed) = (&mut self.live, &mut self.pending, &mut self.freed);
        self.map.retain(|p, e| {
            let kept = keep(p, e);
            if !kept {
                *freed |= live.remove(p).is_some();
                pending.remove(p);
            }
            kept
        });
    }
    pub fn clear(&mut self) {
        self.map.clear();
        self.tickers = Arc::default();
        self.live.clear();
        self.added.clear();
        self.pending.clear();
        self.out.clear();
        self.replaced.clear();
        self.freed = false;
    }
    /// A loaded or generated chunk's block entities, in the order the chunk
    /// lists them; they tick in its block entity map's order
    /// (`registerAllBlockEntitiesAfterLevelLoad`). One replacing an entity
    /// already at its position (`LevelChunk.setBlockEntity`) drops that one;
    /// its slot is decided by the block state at the next tick.
    pub fn load_chunk(&mut self, entities: Vec<(Pos, BlockEntity)>) {
        let order = chunk_map_order(entities.iter().map(|(p, _)| *p));
        for (p, e) in entities {
            if self.map.insert(p, e).is_some() {
                self.replaced.insert(p);
            }
        }
        for p in order {
            self.add(p);
        }
    }
    /// `LevelChunk.setBlockState` on a block entity's position, from state
    /// `old` to `new`: a ticker made, kept or dropped.
    pub fn state_changed(&mut self, pos: Pos, old: u32, new: u32) {
        if self.states.is_empty() || !self.map.contains_key(&pos) {
            return;
        }
        match self.ticking(new) {
            // A new ticker, unless one was made or is pending (rebound).
            Ticking::Yes => {
                if self.ticking(old) != Ticking::Yes && !self.live.contains_key(&pos) {
                    self.add(pos);
                }
            }
            Ticking::Idle => self.cancel(&pos),
            // Another block: the game removes or replaces the block entity.
            Ticking::No => {}
        }
    }
    /// The start of `Level.tickBlockEntities`: pending slots join the list
    /// and freed ones leave it. `state` reads the block state at a position.
    /// Returns the slots to tick, in order; check each with `is_live` before
    /// ticking it, as the ticks before it can free it.
    pub fn tickers(&mut self, state: impl Fn(Pos) -> u32) -> Arc<Vec<(Pos, u32)>> {
        for (pos, token) in std::mem::take(&mut self.added) {
            if self.pending.get(&pos) != Some(&token) {
                continue;
            }
            self.pending.remove(&pos);
            if self.live.contains_key(&pos)
                || !self.map.contains_key(&pos)
                || self.ticking(state(pos)) != Ticking::Yes
            {
                continue;
            }
            let slot = self.next();
            self.live.insert(pos, slot);
            Arc::make_mut(&mut self.tickers).push((pos, slot));
        }
        for pos in std::mem::take(&mut self.replaced) {
            if self.live.contains_key(&pos) && self.ticking(state(pos)) != Ticking::Yes {
                self.cancel(&pos);
            }
        }
        if !std::mem::take(&mut self.freed) && self.out.is_empty() {
            return Arc::clone(&self.tickers);
        }
        self.out.clear();
        let (live, map) = (&mut self.live, &self.map);
        Arc::make_mut(&mut self.tickers).retain(|(p, slot)| {
            let mine = live.get(p) == Some(slot);
            if mine && !map.contains_key(p) {
                live.remove(p);
            }
            mine && map.contains_key(p)
        });
        Arc::clone(&self.tickers)
    }
    /// Whether a slot from `tickers` still ticks.
    pub fn is_live(&self, (pos, slot): (Pos, u32)) -> bool {
        self.live.get(&pos) == Some(&slot) && self.map.contains_key(&pos)
    }
    /// Frees the slot at `pos` (the block entity stays).
    pub fn forget(&mut self, pos: &Pos) {
        self.cancel(pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn hopper() -> BlockEntity {
        BlockEntity::Chest(vec![None; 5])
    }
    /// States: 0 air, 1 ticks, 2 idle.
    fn level() -> BlockEntities {
        let mut b = BlockEntities::default();
        b.set_states(vec![Ticking::No, Ticking::Yes, Ticking::Idle]);
        b
    }
    fn order(b: &mut BlockEntities, states: &HashMap<Pos, u32>) -> Vec<Pos> {
        b.tickers(|p| states.get(&p).copied().unwrap_or(0))
            .iter()
            .map(|(p, _)| *p)
            .collect()
    }

    /// HorizonsX: a region lent block entities ticks those of its own part, in the level's order.
    #[test]
    fn a_lent_region_ticks_its_block_entities() {
        let mut b = level();
        let mut s = HashMap::new();
        for p in [(0, 0, 0), (40, 0, 0), (1, 0, 0), (41, 0, 0), (2, 0, 0)] {
            s.insert(p, 1);
            b.insert(p, hopper());
        }
        assert_eq!(order(&mut b, &s).len(), 5);
        let mut lent = b.lend(2, |(x, _, _)| Some(usize::from(x >= 32)));
        assert_eq!(order(&mut lent[0], &s), [(0, 0, 0), (1, 0, 0), (2, 0, 0)]);
        assert_eq!(order(&mut lent[1], &s), [(40, 0, 0), (41, 0, 0)]);
        for part in lent {
            b.take_back(part);
        }
        assert_eq!(order(&mut b, &s), [(0, 0, 0), (40, 0, 0), (1, 0, 0), (41, 0, 0), (2, 0, 0)]);
    }

    #[test]
    fn slots_follow_vanilla_lifecycle() {
        let mut b = level();
        let mut s = HashMap::new();
        for p in [(0, 0, 0), (1, 0, 0), (2, 0, 0)] {
            s.insert(p, 1);
            b.insert(p, hopper());
        }
        assert_eq!(order(&mut b, &s), [(0, 0, 0), (1, 0, 0), (2, 0, 0)]);
        // Replacing keeps the slot; removing and placing again appends.
        b.insert((0, 0, 0), hopper());
        b.remove(&(1, 0, 0));
        b.insert((1, 0, 0), hopper());
        assert_eq!(order(&mut b, &s), [(0, 0, 0), (2, 0, 0), (1, 0, 0)]);
        // Take and put keep it; an idle state frees it, ticking again appends.
        let e = b.take(&(0, 0, 0)).unwrap();
        b.put((0, 0, 0), e);
        assert_eq!(order(&mut b, &s), [(0, 0, 0), (2, 0, 0), (1, 0, 0)]);
        s.insert((2, 0, 0), 2);
        b.state_changed((2, 0, 0), 1, 2);
        assert_eq!(order(&mut b, &s), [(0, 0, 0), (1, 0, 0)]);
        s.insert((2, 0, 0), 1);
        b.state_changed((2, 0, 0), 2, 1);
        assert_eq!(order(&mut b, &s), [(0, 0, 0), (1, 0, 0), (2, 0, 0)]);
        // A slot freed during a tick is not live any more.
        let slots = b.tickers(|p| s.get(&p).copied().unwrap_or(0));
        b.retain(|p, _| *p != (1, 0, 0));
        assert!(b.is_live(slots[0]) && !b.is_live(slots[1]));
        // A block entity taken and never brought back loses its slot.
        b.take(&(0, 0, 0));
        assert_eq!(order(&mut b, &s), [(2, 0, 0)]);
        assert_eq!(order(&mut b, &s), [(2, 0, 0)]);
    }

    #[test]
    fn chunk_loads_in_open_hash_map_order() {
        // A few keys: their mixed hashes over 32 slots, read from the top.
        let positions = [
            (0, 64, 0),
            (1, 64, 0),
            (0, 64, 1),
            (15, -64, 15),
            (3, 70, 9),
        ];
        let slot = |p: &Pos| mix(java_hash(*p)) & 31;
        let mut expected = positions.to_vec();
        expected.sort_by_key(|p| std::cmp::Reverse(slot(p)));
        let distinct: std::collections::HashSet<usize> = positions.iter().map(slot).collect();
        assert_eq!(distinct.len(), positions.len());
        assert_eq!(chunk_map_order(positions), expected);
        // Past 24 keys the table doubles; every key stays, once.
        let many: Vec<Pos> = (0..100).map(|i| (i % 16, i / 16, 3)).collect();
        let order = chunk_map_order(many.iter().copied().chain(many.iter().copied()));
        assert_eq!(order.len(), 100);
    }

    /// A chunk loaded over a block entity already there: the new one replaces
    /// it, and keeps the old slot only while its block state ticks.
    #[test]
    fn loaded_chunk_replaces_block_entity_by_state() {
        let mut b = level();
        let mut s = HashMap::new();
        let (p, q) = ((0, 0, 0), (1, 0, 0));
        for pos in [p, q] {
            s.insert(pos, 1);
        }
        // Two chunks, so the order is the order they load in.
        b.load_chunk(vec![(p, hopper())]);
        b.load_chunk(vec![(q, hopper())]);
        assert_eq!(order(&mut b, &s), [p, q]);
        // Replaced where the state ticks: the slot stays in its place.
        b.load_chunk(vec![(p, hopper())]);
        assert_eq!(order(&mut b, &s), [p, q]);
        // Replaced where the state has no ticker: the slot is freed, and the
        // new block entity is the one in the map.
        s.insert(p, 2);
        b.load_chunk(vec![(p, BlockEntity::Chest(vec![None; 27]))]);
        assert_eq!(order(&mut b, &s), [q]);
        assert!(matches!(b.get(&p), Some(BlockEntity::Chest(items)) if items.len() == 27));
        // Ticking again later appends it, as any new ticker does.
        s.insert(p, 1);
        b.state_changed(p, 2, 1);
        assert_eq!(order(&mut b, &s), [q, p]);
    }
}
