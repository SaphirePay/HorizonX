//! The orders chunks are made in (`iterator/*`): each a little state machine, so a resumed task walks past what
//! was done without a list in memory (a 50k-block radius is 39 million chunks).

use crate::Selection;
use std::path::Path;

pub trait ChunkIter: Send {
    fn next(&mut self) -> Option<(i32, i32)>;
    fn total(&self) -> u64;
}

pub fn create(sel: &Selection, region_dir: &Path, data_dir: &Path) -> Box<dyn ChunkIter> {
    if sel.pattern == "world" {
        return Box::new(WorldIter::new(region_dir));
    }
    if sel.uses_z() {
        return Box::new(Loop::new(sel));
    }
    match sel.pattern.as_str() {
        "loop" => Box::new(Loop::new(sel)),
        "spiral" => Box::new(Spiral::new(sel)),
        "concentric" => Box::new(Concentric::new(sel)),
        "csv" => Box::new(Csv::new(&data_dir.join(format!("{}.csv", sel.pattern_value.clone().unwrap_or_default())))),
        _ => Box::new(Region::new(sel)),
    }
}

/// Rows of the rectangle: x outer, z inner (`Loop2ChunkIterator`).
pub struct Loop {
    min: (i32, i32),
    max: (i32, i32),
    x: i32,
    z: i32,
}

impl Loop {
    fn new(sel: &Selection) -> Self {
        let (cx, cz) = sel.center_chunk();
        let (rx, rz) = sel.radius_chunks();
        Self { min: (cx - rx, cz - rz), max: (cx + rx, cz + rz), x: cx - rx, z: cz - rz }
    }
}

impl ChunkIter for Loop {
    fn next(&mut self) -> Option<(i32, i32)> {
        if self.x > self.max.0 {
            return None;
        }
        let out = (self.x, self.z);
        self.z += 1;
        if self.z > self.max.1 {
            self.z = self.min.1;
            self.x += 1;
        }
        Some(out)
    }
    fn total(&self) -> u64 {
        (self.max.0 - self.min.0 + 1) as u64 * (self.max.1 - self.min.1 + 1) as u64
    }
}

/// Rings outward from the centre, each starting at its (+x, +z) corner (`ConcentricChunkIterator`).
pub struct Concentric {
    c: (i32, i32),
    r: i32,
    ring: i32,
    /// Position within the ring: 0..8*ring (ring 0 has one cell).
    pos: i32,
}

impl Concentric {
    fn new(sel: &Selection) -> Self {
        Self { c: sel.center_chunk(), r: sel.radius_chunks().0, ring: 0, pos: 0 }
    }
}

/// The `pos`-th cell of ring `k` around (0,0), walked as Chunky walks it: from (k, k) down, left, up, right.
fn ring_cell(k: i32, pos: i32) -> (i32, i32) {
    if k == 0 {
        return (0, 0);
    }
    let span = 2 * k;
    match pos {
        0 => (k, k),
        p if p <= span => (k, k - p),
        p if p <= 2 * span => (k - (p - span), -k),
        p if p <= 3 * span => (-k, -k + (p - 2 * span)),
        p => (-k + (p - 3 * span), k),
    }
}

impl ChunkIter for Concentric {
    fn next(&mut self) -> Option<(i32, i32)> {
        if self.ring > self.r {
            return None;
        }
        let (dx, dz) = ring_cell(self.ring, self.pos);
        self.pos += 1;
        let cells = if self.ring == 0 { 1 } else { 8 * self.ring };
        if self.pos >= cells {
            self.ring += 1;
            self.pos = 0;
        }
        Some((self.c.0 + dx, self.c.1 + dz))
    }
    fn total(&self) -> u64 {
        let d = (2 * self.r + 1) as u64;
        d * d
    }
}

/// A square spiral from the centre (`SpiralChunkIterator`): for an odd side, its first side² steps fill it.
pub struct Spiral {
    c: (i32, i32),
    total: u64,
    emitted: u64,
    x: i32,
    z: i32,
    span: i32,
    span_count: i32,
    progress: i32,
    dir: u8,
}

impl Spiral {
    fn new(sel: &Selection) -> Self {
        let d = (2 * sel.radius_chunks().0 + 1) as u64;
        Self { c: sel.center_chunk(), total: d * d, emitted: 0, x: 0, z: 0, span: 1, span_count: 0, progress: 0, dir: 0 }
    }
}

impl ChunkIter for Spiral {
    fn next(&mut self) -> Option<(i32, i32)> {
        if self.emitted >= self.total {
            return None;
        }
        let out = (self.c.0 + self.x, self.c.1 + self.z);
        self.emitted += 1;
        match self.dir {
            0 => self.x += 1,
            1 => self.z -= 1,
            2 => self.x -= 1,
            _ => self.z += 1,
        }
        self.progress += 1;
        if self.progress == self.span {
            self.progress = 0;
            self.dir = (self.dir + 1) % 4;
            self.span_count += 1;
            if self.span_count == 2 {
                self.span_count = 0;
                self.span += 1;
            }
        }
        Some(out)
    }
    fn total(&self) -> u64 {
        self.total
    }
}

/// Region by region, rings of regions outward; inside a region, its chunks in the selection row by row
/// (`RegionChunkIterator`): each region file is finished before the next is opened.
pub struct Region {
    min: (i32, i32),
    max: (i32, i32),
    rc: (i32, i32),
    rr: i32,
    ring: i32,
    pos: i32,
    current: Option<(i32, i32, i32, i32, i32, i32)>,
}

impl Region {
    fn new(sel: &Selection) -> Self {
        let (cx, cz) = sel.center_chunk();
        let r = sel.radius_chunks().0;
        Self { min: (cx - r, cz - r), max: (cx + r, cz + r), rc: (cx >> 5, cz >> 5), rr: ((r as f32) / 32.0).ceil() as i32 + 1, ring: 0, pos: 0, current: None }
    }
    /// The next region that overlaps the selection: its chunk bounds and the cursor.
    fn next_region(&mut self) -> Option<(i32, i32, i32, i32, i32, i32)> {
        loop {
            if self.ring > self.rr {
                return None;
            }
            let (dx, dz) = ring_cell(self.ring, self.pos);
            self.pos += 1;
            let cells = if self.ring == 0 { 1 } else { 8 * self.ring };
            if self.pos >= cells {
                self.ring += 1;
                self.pos = 0;
            }
            let (rx, rz) = (self.rc.0 + dx, self.rc.1 + dz);
            let (x0, z0) = ((rx << 5).max(self.min.0), (rz << 5).max(self.min.1));
            let (x1, z1) = (((rx << 5) + 31).min(self.max.0), ((rz << 5) + 31).min(self.max.1));
            if x0 <= x1 && z0 <= z1 {
                return Some((x0, z0, x1, z1, x0, z0));
            }
        }
    }
}

impl ChunkIter for Region {
    fn next(&mut self) -> Option<(i32, i32)> {
        if self.current.is_none() {
            self.current = self.next_region();
        }
        let (x0, _z0, x1, z1, x, z) = self.current?;
        let out = (x, z);
        let (mut nx, mut nz) = (x + 1, z);
        if nx > x1 {
            nx = x0;
            nz += 1;
        }
        self.current = if nz > z1 { None } else { self.current.map(|c| (c.0, c.1, c.2, c.3, nx, nz)) };
        Some(out)
    }
    fn total(&self) -> u64 {
        (self.max.0 - self.min.0 + 1) as u64 * (self.max.1 - self.min.1 + 1) as u64
    }
}

/// Every chunk of every region file the world has (`WorldChunkIterator`).
pub struct WorldIter {
    regions: Vec<(i32, i32)>,
    at: usize,
    i: i32,
}

impl WorldIter {
    fn new(dir: &Path) -> Self {
        let mut regions: Vec<(i32, i32)> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let mut parts = name.strip_prefix("r.")?.strip_suffix(".mca")?.split('.');
                Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
            })
            .collect();
        regions.sort_by_key(|(x, z)| (x.abs().max(z.abs()), *x, *z));
        Self { regions, at: 0, i: 0 }
    }
}

impl ChunkIter for WorldIter {
    fn next(&mut self) -> Option<(i32, i32)> {
        let (rx, rz) = *self.regions.get(self.at)?;
        let out = ((rx << 5) + (self.i & 31), (rz << 5) + (self.i >> 5));
        self.i += 1;
        if self.i >= 1024 {
            self.i = 0;
            self.at += 1;
        }
        Some(out)
    }
    fn total(&self) -> u64 {
        self.regions.len() as u64 * 1024
    }
}

/// `x,z` chunk lines from `plugins/Chunky/<name>.csv`.
pub struct Csv {
    chunks: Vec<(i32, i32)>,
    at: usize,
}

impl Csv {
    fn new(path: &Path) -> Self {
        let chunks = std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let (x, z) = l.split_once(',')?;
                Some((x.trim().parse().ok()?, z.trim().parse().ok()?))
            })
            .collect();
        Self { chunks, at: 0 }
    }
}

impl ChunkIter for Csv {
    fn next(&mut self) -> Option<(i32, i32)> {
        let c = self.chunks.get(self.at).copied();
        self.at += 1;
        c
    }
    fn total(&self) -> u64 {
        self.chunks.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    fn sel(pattern: &str, r: f64) -> Selection {
        Selection { world: "w".into(), center_x: 40.0, center_z: -40.0, radius_x: r, radius_z: r, pattern: pattern.into(), pattern_value: None, shape: "square".into() }
    }
    #[test]
    fn every_pattern_covers_the_square_once() {
        for p in ["loop", "spiral", "concentric", "region"] {
            let s = sel(p, 600.0);
            let mut it = create(&s, Path::new("."), Path::new("."));
            let total = it.total();
            let mut seen = HashSet::new();
            while let Some(c) = it.next() {
                assert!(seen.insert(c), "{p} repeats {c:?}");
            }
            assert_eq!(seen.len() as u64, total, "{p} covers all");
            let (cx, cz) = s.center_chunk();
            let r = s.radius_chunks().0;
            assert!(seen.iter().all(|(x, z)| (x - cx).abs() <= r && (z - cz).abs() <= r), "{p} stays inside");
        }
    }
}
