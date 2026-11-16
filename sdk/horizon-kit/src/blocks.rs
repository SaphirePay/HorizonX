//! Many blocks read at once: a `Snapshot` of a box in one query (`box_read`, ids by run), instead of one query a
//! block. A mod or plugin that scans a tree, a vein or a room reads its box once and looks blocks up locally.
//!
//! ```ignore
//! let snap = blocks::Snapshot::read(&world, [x - 8, y - 2, z - 8], [x + 8, y + 30, z + 8])?;
//! if snap.name(x, y + 1, z) == "minecraft:oak_log" { .. }
//! let distance = snap.property(x, y + 5, z, "distance");
//! ```
use horizon_bukkit::World;
use serde_json::json;
use std::sync::OnceLock;

/// The dimension id of a world: `world` is `minecraft:overworld`, `world_nether` `minecraft:the_nether`, and so on.
pub fn dimension(world: &World) -> &'static str {
    match world.get_name() {
        "world_nether" | "minecraft:the_nether" | "the_nether" => "minecraft:the_nether",
        "world_the_end" | "minecraft:the_end" | "the_end" => "minecraft:the_end",
        _ => "minecraft:overworld",
    }
}

/// Every block state the game has, by state id, written as the game writes it (`minecraft:oak_log[axis=y]`).
pub fn state_names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let answer = horizon_bukkit::query_from_main(json!({ "op": "state_names" }));
        answer.as_array().map(|a| a.iter().map(|v| v.as_str().unwrap_or("minecraft:air").to_owned()).collect()).unwrap_or_default()
    })
}

fn b64(text: &str) -> Vec<u8> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for v in text.bytes().filter_map(value) {
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// A box of blocks as they were when it was read.
pub struct Snapshot {
    min: [i32; 3],
    max: [i32; 3],
    /// The rows the world has (the box clamped to its floor and roof).
    bottom: i32,
    top: i32,
    ids: Vec<u32>,
}

impl Snapshot {
    /// Reads the box from `min` to `max` (both included) of `world`. None when its chunks aren't loaded. Call it on
    /// the server thread (an event, a command, a timer).
    pub fn read(world: &World, min: [i32; 3], max: [i32; 3]) -> Option<Snapshot> {
        let lo = [min[0].min(max[0]), min[1].min(max[1]), min[2].min(max[2])];
        let hi = [min[0].max(max[0]), min[1].max(max[1]), min[2].max(max[2])];
        let answer = horizon_bukkit::query_from_main(json!({ "op": "box_read", "dimension": dimension(world), "min": lo, "max": hi, "format": "runs" }));
        let runs = answer["runs"].as_str()?;
        let (bottom, top) = (answer["bottom"].as_i64()? as i32, answer["top"].as_i64()? as i32);
        let bytes = b64(runs);
        let mut ids = Vec::new();
        for pair in bytes.chunks_exact(8) {
            let id = u32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]);
            let n = u32::from_le_bytes([pair[4], pair[5], pair[6], pair[7]]);
            ids.extend(std::iter::repeat_n(id, n as usize));
        }
        Some(Snapshot { min: lo, max: hi, bottom, top, ids })
    }

    /// Whether a place is inside the box (and the world).
    pub fn contains(&self, x: i32, y: i32, z: i32) -> bool {
        (self.min[0]..=self.max[0]).contains(&x) && (self.min[2]..=self.max[2]).contains(&z) && (self.bottom..=self.top).contains(&y)
    }

    /// The state id at a place; None outside the box.
    pub fn id(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        if !self.contains(x, y, z) {
            return None;
        }
        let (dx, dz) = ((self.max[0] - self.min[0] + 1) as usize, (self.max[2] - self.min[2] + 1) as usize);
        let i = ((y - self.bottom) as usize * dz + (z - self.min[2]) as usize) * dx + (x - self.min[0]) as usize;
        self.ids.get(i).copied()
    }

    /// The block state at a place, as the game writes it (`minecraft:oak_leaves[distance=2,persistent=false,..]`);
    /// `minecraft:air` outside the box.
    pub fn state(&self, x: i32, y: i32, z: i32) -> &'static str {
        self.id(x, y, z).and_then(|id| state_names().get(id as usize)).map_or("minecraft:air", String::as_str)
    }

    /// The block at a place without its properties (`minecraft:oak_leaves`).
    pub fn name(&self, x: i32, y: i32, z: i32) -> &'static str {
        let state = self.state(x, y, z);
        state.split_once('[').map_or(state, |(name, _)| name)
    }

    /// One property of the block at a place (`distance`, `persistent`, `axis`...).
    pub fn property(&self, x: i32, y: i32, z: i32, key: &str) -> Option<&'static str> {
        let props = self.state(x, y, z).split_once('[')?.1.strip_suffix(']')?;
        props.split(',').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
    }

    /// Whether the place is air (any of the three airs).
    pub fn is_air(&self, x: i32, y: i32, z: i32) -> bool {
        matches!(self.name(x, y, z), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_decodes() {
        assert_eq!(super::b64("AQAAAAIAAAA="), vec![1, 0, 0, 0, 2, 0, 0, 0]);
    }
}
