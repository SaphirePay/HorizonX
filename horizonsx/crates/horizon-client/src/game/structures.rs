//! Which structures contain a position (`StructureManager.getStructureAt`
//! and `getStructureWithPieceAt`), from the chunk's structure references and
//! the generator's cached starts.
use super::Game;

impl Game<'_> {
    /// Structures reaching `pos`: (id, inside a piece). `getStructureAt`
    /// tests the start's whole box, `getStructureWithPieceAt` a piece's.
    pub(super) fn structures_reaching(&self, pos: [i32; 3]) -> Vec<(String, bool)> {
        self.structures_reaching_where(pos, &|_| true)
    }
    /// The same, for the structures whose id `wanted` takes: the others'
    /// starts aren't looked up.
    pub(super) fn structures_reaching_where(
        &self,
        pos: [i32; 3],
        wanted: &dyn Fn(&str) -> bool,
    ) -> Vec<(String, bool)> {
        if self
            .world
            .chunks
            .get(&(pos[0] >> 4, pos[2] >> 4))
            .is_none_or(|chunk| chunk.structure_refs.is_empty())
        {
            return Vec::new();
        }
        structures_reaching_in(&self.world, self.end.locator().as_deref(), pos, wanted)
    }
}

/// [`Game::structures_reaching_where`] over a world and the generator's starts: what threads that
/// hold no game (HorizonsX's spawn regions) ask.
pub(super) fn structures_reaching_in(
    world: &horizon_client::world::World,
    locator: Option<&horizon_client::vanillagen::VanillaTerrain>,
    pos: [i32; 3],
    wanted: &dyn Fn(&str) -> bool,
) -> Vec<(String, bool)> {
    {
        let Some(chunk) = world.chunks.get(&(pos[0] >> 4, pos[2] >> 4)) else {
            return Vec::new();
        };
        if chunk.structure_refs.is_empty() {
            return Vec::new();
        }
        let Some(locator) = locator else {
            return Vec::new();
        };
        let mut out: Vec<(String, bool)> = Vec::new();
        for (id, (sx, sz)) in &chunk.structure_refs {
            if !wanted(id) {
                continue;
            }
            for start in locator.starts_at_shared(*sx, *sz).iter() {
                if *start.name != **id || !start.bounding_box.is_inside(pos) {
                    continue;
                }
                let piece = start.pieces.iter().any(|p| p.bounding_box().is_inside(pos));
                match out.iter_mut().find(|(n, _)| *n == start.name) {
                    Some(e) => e.1 |= piece,
                    None => out.push((start.name.clone(), piece)),
                }
            }
        }
        out
    }
}

impl Game<'_> {
    /// The structures with a piece containing `pos`.
    pub(super) fn structures_at(&self, pos: [i32; 3]) -> Vec<String> {
        self.structures_reaching(pos)
            .into_iter()
            .filter(|(_, piece)| *piece)
            .map(|(n, _)| n)
            .collect()
    }
}
