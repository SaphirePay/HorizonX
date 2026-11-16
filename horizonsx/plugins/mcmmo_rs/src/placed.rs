//! Placed-block tracking (mcMMO's UserBlockTracker): a block a player placed gives no XP or bonus drops when
//! broken. Kept in memory per chunk as a sorted list of packed in-chunk positions (a few bytes per block), saved
//! per 32x32-chunk region in `plugins/mcMMO/placed/<world>/r.<x>.<z>.bin` from the saver thread, all read at start
//! on that thread.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ChunkKey {
    world: u16,
    x: i32,
    z: i32,
}

#[derive(Default)]
struct Tracker {
    worlds: Vec<String>,
    chunks: HashMap<ChunkKey, Vec<u32>>,
    dirty_regions: HashSet<(u16, i32, i32)>,
}

impl Tracker {
    fn world(&mut self, name: &str) -> u16 {
        if let Some(i) = self.worlds.iter().position(|w| w == name) {
            return i as u16;
        }
        self.worlds.push(name.to_owned());
        (self.worlds.len() - 1) as u16
    }
    fn world_of(&self, name: &str) -> Option<u16> {
        self.worlds.iter().position(|w| w == name).map(|i| i as u16)
    }
}

static TRACKER: LazyLock<Mutex<Tracker>> = LazyLock::new(|| Mutex::new(Tracker::default()));
static FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);

fn pack(x: i32, y: i32, z: i32) -> u32 {
    (((y + 2048) as u32 & 0xfff) << 8) | (((x & 15) as u32) << 4) | ((z & 15) as u32)
}

fn key(world: u16, x: i32, z: i32) -> ChunkKey {
    ChunkKey { world, x: x >> 4, z: z >> 4 }
}

fn tracker() -> std::sync::MutexGuard<'static, Tracker> {
    TRACKER.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether a player placed the block there (one hash lookup and a binary search).
pub fn is_placed(world: &str, x: i32, y: i32, z: i32) -> bool {
    let t = tracker();
    let Some(w) = t.world_of(world) else { return false };
    t.chunks.get(&key(w, x, z)).is_some_and(|v| v.binary_search(&pack(x, y, z)).is_ok())
}

pub fn set(world: &str, x: i32, y: i32, z: i32, placed: bool) {
    let mut t = tracker();
    let w = if placed {
        t.world(world)
    } else {
        match t.world_of(world) {
            Some(w) => w,
            None => return,
        }
    };
    let k = key(w, x, z);
    let p = pack(x, y, z);
    let changed = if placed {
        let v = t.chunks.entry(k).or_default();
        match v.binary_search(&p) {
            Ok(_) => false,
            Err(i) => {
                v.insert(i, p);
                true
            }
        }
    } else {
        match t.chunks.get_mut(&k) {
            Some(v) => match v.binary_search(&p) {
                Ok(i) => {
                    v.remove(i);
                    if v.is_empty() {
                        t.chunks.remove(&k);
                    }
                    true
                }
                Err(_) => false,
            },
            None => false,
        }
    };
    if changed {
        t.dirty_regions.insert((w, k.x >> 5, k.z >> 5));
    }
}

/// Reads every region file (start, on the data thread).
pub fn load(folder: PathBuf) {
    let root = folder.join("placed");
    let _ = std::fs::create_dir_all(&root);
    *FOLDER.lock().unwrap_or_else(|e| e.into_inner()) = Some(root.clone());
    let Ok(worlds) = std::fs::read_dir(&root) else { return };
    let mut loaded: Vec<(String, Vec<(i32, i32, Vec<u32>)>)> = Vec::new();
    for w in worlds.flatten() {
        let world = decode_world(&w.file_name().to_string_lossy());
        let mut chunks = Vec::new();
        let Ok(files) = std::fs::read_dir(w.path()) else { continue };
        for f in files.flatten() {
            let Ok(bytes) = std::fs::read(f.path()) else { continue };
            let mut i = 0;
            let rd = |i: &mut usize| -> Option<u32> {
                let b = bytes.get(*i..*i + 4)?;
                *i += 4;
                Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            };
            while let (Some(cx), Some(cz), Some(n)) = (rd(&mut i), rd(&mut i), rd(&mut i)) {
                let mut v = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    match rd(&mut i) {
                        Some(p) => v.push(p),
                        None => break,
                    }
                }
                v.sort_unstable();
                v.dedup();
                chunks.push((cx as i32, cz as i32, v));
            }
        }
        loaded.push((world, chunks));
    }
    let mut t = tracker();
    for (world, chunks) in loaded {
        let w = t.world(&world);
        for (x, z, mut v) in chunks {
            let e = t.chunks.entry(ChunkKey { world: w, x, z }).or_default();
            // A block placed while the files were read is kept too.
            e.append(&mut v);
            e.sort_unstable();
            e.dedup();
        }
    }
}

fn encode_world(name: &str) -> String {
    name.replace(':', "+")
}

fn decode_world(name: &str) -> String {
    name.replace('+', ":")
}

/// Writes the regions that changed (the data thread; the lock only while the bytes are made).
pub fn save_dirty() {
    let Some(root) = FOLDER.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
    let files: Vec<(PathBuf, Vec<u8>)> = {
        let mut t = tracker();
        if t.dirty_regions.is_empty() {
            return;
        }
        let dirty: HashSet<(u16, i32, i32)> = t.dirty_regions.drain().collect();
        let mut regions: HashMap<(u16, i32, i32), Vec<u8>> = dirty.iter().map(|r| (*r, Vec::new())).collect();
        for (k, v) in t.chunks.iter() {
            let Some(bytes) = regions.get_mut(&(k.world, k.x >> 5, k.z >> 5)) else { continue };
            bytes.extend_from_slice(&(k.x as u32).to_le_bytes());
            bytes.extend_from_slice(&(k.z as u32).to_le_bytes());
            bytes.extend_from_slice(&(v.len() as u32).to_le_bytes());
            for p in v {
                bytes.extend_from_slice(&p.to_le_bytes());
            }
        }
        regions.into_iter().map(|((w, rx, rz), bytes)| (root.join(encode_world(&t.worlds[w as usize])).join(format!("r.{rx}.{rz}.bin")), bytes)).collect()
    };
    for (path, bytes) in files {
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if bytes.is_empty() {
            let _ = std::fs::remove_file(&path);
        } else {
            let _ = std::fs::write(&path, bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tracks() {
        super::set("w", 17, -60, -3, true);
        assert!(super::is_placed("w", 17, -60, -3));
        assert!(!super::is_placed("w", 17, -59, -3));
        super::set("w", 17, -60, -3, false);
        assert!(!super::is_placed("w", 17, -60, -3));
    }
}
