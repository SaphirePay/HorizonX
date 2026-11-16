//! `HorizonChunks`: whole chunks in and out of the game for plugins that edit many blocks at once
//! (FastAsyncWorldEdit's Horizon adapter). A chunk is read with one engine query, its sections are filled into a
//! plugin's own `char[]` in Rust, and blocks are written from Rust, so a big edit doesn't cost one Java call
//! per block.
//!
//! From Java:
//!   Object chunk = HorizonChunks.read(world, chunkX, chunkZ);   // null when it can't be loaded
//!   String[] palette = HorizonChunks.palette(chunk);            // block states as the game writes them
//!   int bottom = HorizonChunks.bottom(chunk);                    // the world's lowest y
//!   HorizonChunks.fill(chunk, sectionY, ordinals, data);         // data[i] = ordinals[palette index of block i]
//!   int written = HorizonChunks.write(world, xyz, states, codes); // states[codes[i]] at (xyz[3i], xyz[3i+1], xyz[3i+2])
use crate::held;
use horizon_bukkit::World;
use jrt::{R, V};

/// A chunk as one engine query gave it.
pub struct RawChunk {
    bottom: i32,
    palette: Vec<String>,
    ids: Vec<u32>,
}

fn world_of(value: &V) -> Option<World> {
    held::<World>(value).map(|w| w.clone())
}

/// The engine's dimension id of a Bukkit world name.
fn dimension(world: &World) -> String {
    match world.get_name() {
        "world_nether" => "minecraft:the_nether".into(),
        "world_the_end" => "minecraft:the_end".into(),
        "world" | "" => "minecraft:overworld".into(),
        other if other.contains(':') => other.into(),
        other => format!("minecraft:{other}"),
    }
}

/// A block state's text with its properties sorted: the engine's and FAWE's texts of one state can list them in
/// another order.
fn state_key(text: &str) -> String {
    match text.split_once('[') {
        Some((name, rest)) => {
            let mut properties: Vec<&str> = rest.trim_end_matches(']').split(',').filter(|p| !p.is_empty()).collect();
            properties.sort_unstable();
            format!("{}[{}]", name.trim_start_matches("minecraft:"), properties.join(","))
        }
        None => text.trim_start_matches("minecraft:").to_owned(),
    }
}

/// Every block state of the engine by id, and the id of each state's text (`state_key`), asked once. Empty on an
/// engine without the bulk ops (then reads and writes go the old way, one query per chunk / per block).
fn states() -> &'static (Vec<String>, std::collections::HashMap<String, u32>) {
    static STATES: std::sync::OnceLock<(Vec<String>, std::collections::HashMap<String, u32>)> = std::sync::OnceLock::new();
    STATES.get_or_init(|| {
        let names: Vec<String> = horizon_bukkit::horizon_mod::query(serde_json::json!({ "op": "state_names" }))
            .as_array()
            .map(|all| all.iter().map(|n| n.as_str().unwrap_or_default().to_owned()).collect())
            .unwrap_or_default();
        let ids = names.iter().enumerate().filter(|(_, n)| !n.is_empty()).map(|(i, n)| (state_key(n), i as u32)).collect();
        (names, ids)
    })
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

fn base64_decode(text: &str) -> Vec<u8> {
    let mut table = [255u8; 256];
    for (i, c) in B64.iter().enumerate() {
        table[*c as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = table[c as usize];
        if v == 255 {
            continue;
        }
        buffer = (buffer << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

/// A whole chunk's state ids in one engine query (`blocks_read`, packed and base64): no JSON value per block.
fn read_bulk(world: &World, x: i32, z: i32) -> Option<RawChunk> {
    let (names, _) = states();
    if names.is_empty() {
        return None;
    }
    let answer = horizon_bukkit::horizon_mod::query(serde_json::json!({ "op": "blocks_read", "dimension": dimension(world), "chunk": [x, z] }));
    let (bottom, height) = (answer["bottom"].as_i64()? as i32, answer["height"].as_i64()? as usize);
    let bytes = base64_decode(answer["data"].as_str()?);
    let mut buffer: Vec<u32> = bytes.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    buffer.resize(height * 256, 0);
    // The chunk's own palette: the states it holds, in the order met.
    let mut local: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut palette = Vec::new();
    for id in buffer.iter_mut() {
        let next = local.len() as u32;
        let index = *local.entry(*id).or_insert_with(|| {
            palette.push(names.get(*id as usize).cloned().unwrap_or_else(|| "minecraft:air".into()));
            next
        });
        *id = index;
    }
    Some(RawChunk { bottom, palette, ids: buffer })
}

fn read(a: &[V]) -> R {
    let (Some(world), Some(x), Some(z)) = (a.first().and_then(world_of), a.get(1).map(V::int), a.get(2).map(V::int)) else {
        return Err(jrt::exc("IllegalArgumentException", "HorizonChunks.read(world, chunkX, chunkZ)"));
    };
    if !world.is_chunk_loaded(x, z) {
        world.load_chunk(x, z);
    }
    if let Some(chunk) = read_bulk(&world, x, z) {
        return Ok(jrt::native(chunk));
    }
    let Some(snapshot) = world.get_chunk_at(x, z).get_chunk_snapshot() else {
        return Ok(V::Null);
    };
    let (bottom, palette, ids) = snapshot.raw();
    Ok(jrt::native(RawChunk { bottom, palette: palette.to_vec(), ids: ids.to_vec() }))
}

fn fill(a: &[V]) -> R {
    let Some(chunk) = a.first().and_then(|c| c.native::<RawChunk>()) else {
        return Err(jrt::exc("IllegalArgumentException", "HorizonChunks.fill: not a chunk from read"));
    };
    let (V::Arr(ordinals), V::Arr(data)) = (&a[2], &a[3]) else {
        return Err(jrt::exc("IllegalArgumentException", "HorizonChunks.fill(chunk, sectionY, char[] ordinals, char[] data)"));
    };
    let section_y = a[1].int();
    let ordinals: Vec<V> = jrt::lock(ordinals).clone();
    let start = ((section_y << 4) - chunk.bottom).max(0) as usize * 256;
    let mut data = jrt::lock(data);
    let air = ordinals.first().cloned().unwrap_or(V::Char('\u{1}'));
    for (i, slot) in data.iter_mut().enumerate().take(4096) {
        *slot = chunk.ids.get(start + i).and_then(|id| ordinals.get(*id as usize)).cloned().unwrap_or_else(|| air.clone());
    }
    Ok(V::Null)
}

fn write(a: &[V]) -> R {
    let Some(world) = a.first().and_then(world_of) else {
        return Err(jrt::exc("IllegalArgumentException", "HorizonChunks.write(world, int[] xyz, String[] states, int[] codes)"));
    };
    let (V::Arr(xyz), V::Arr(states), V::Arr(codes)) = (&a[1], &a[2], &a[3]) else {
        return Err(jrt::exc("IllegalArgumentException", "HorizonChunks.write(world, int[] xyz, String[] states, int[] codes)"));
    };
    let xyz: Vec<i32> = jrt::lock(xyz).iter().map(V::int).collect();
    let states: Vec<String> = jrt::lock(states).iter().map(V::text).collect();
    let codes: Vec<usize> = jrt::lock(codes).iter().map(|c| c.int().max(0) as usize).collect();
    debug_box(&format!("write: {} blocks, states {:?} (thread {:?})", codes.len(), states, std::thread::current().id()));
    // The engine's `blocks_write`: every block of the edit in one call, by state id.
    let (names, ids) = self::states();
    if !names.is_empty() {
        let wanted: Vec<Option<u32>> = states.iter().map(|s| ids.get(&state_key(s)).copied()).collect();
        let mut records: Vec<u8> = Vec::with_capacity(codes.len() * 16);
        for (i, code) in codes.iter().enumerate() {
            if let (Some(Some(id)), Some(p)) = (wanted.get(*code), xyz.get(i * 3..i * 3 + 3)) {
                for word in [p[0] as u32, p[1] as u32, p[2] as u32, *id] {
                    records.extend_from_slice(&word.to_le_bytes());
                }
            }
        }
        let answer = horizon_bukkit::horizon_mod::query(serde_json::json!({ "op": "blocks_write", "dimension": dimension(&world), "data": base64_encode(&records) }));
        if let Some(changed) = answer.as_u64() {
            return Ok(V::Int(changed as i32));
        }
    }
    let mut written = 0;
    for (i, code) in codes.iter().enumerate() {
        let (Some(state), Some(&[x, y, z])) = (states.get(*code), xyz.get(i * 3..i * 3 + 3).map(|p| <&[i32; 3]>::try_from(p).ok()).flatten()) else {
            continue;
        };
        if world.get_block_at(x, y, z).set_block_data(state) {
            written += 1;
        }
    }
    Ok(V::Int(written))
}

/// `HorizonChunks.properties("minecraft:oak_stairs")`: each property of the block and its values in the game's own
/// order, as `[name, String[] values, name, String[] values, ...]`, from every state the block has (one engine query).
/// WorldEdit takes block properties from its version adapter; its bundled data has none.
fn properties(a: &[V]) -> R {
    let block = a.first().map(V::text).unwrap_or_default();
    let states = horizon_bukkit::horizon_mod::query(serde_json::json!({ "op": "items_block_states", "block": block }));
    let mut keys: Vec<String> = Vec::new();
    let mut values: Vec<Vec<String>> = Vec::new();
    for state in states.as_array().into_iter().flatten().filter_map(serde_json::Value::as_str) {
        let Some((_, rest)) = state.split_once('[') else { continue };
        for pair in rest.trim_end_matches(']').split(',') {
            let Some((key, value)) = pair.split_once('=') else { continue };
            let at = match keys.iter().position(|k| k == key) {
                Some(at) => at,
                None => {
                    keys.push(key.to_owned());
                    values.push(Vec::new());
                    keys.len() - 1
                }
            };
            if !values[at].iter().any(|v| v == value) {
                values[at].push(value.to_owned());
            }
        }
    }
    let mut out = Vec::new();
    for (key, list) in keys.into_iter().zip(values) {
        out.push(jrt::s(&key));
        out.push(jrt::arr(list.iter().map(|v| jrt::s(v)).collect()));
    }
    Ok(jrt::arr(out))
}

/// `Bukkit.getTag(registry, key, Material.class)` / `Bukkit.getTags(registry, Material.class)`: the game's block and
/// item tags, their values as Materials (WorldEdit's block categories: `##logs` patterns, what counts as a sign).
pub struct TagH(horizon_bukkit::registry::Tag, String);

fn tag_kind(registry: &str) -> &'static str {
    match registry {
        "items" => "item",
        "fluids" => "fluid",
        "entity_types" => "entity_type",
        "game_events" => "game_event",
        _ => "block",
    }
}

pub fn tag_static(class: &str, name: &str, a: &[V]) -> Option<R> {
    if !matches!(class.rsplit(['.', '$']).next(), Some("Bukkit" | "Server")) {
        return None;
    }
    match (name, a.len()) {
        ("getTag", 3) => {
            let kind = tag_kind(&a[0].text());
            let key = a[1].text();
            let key = key.trim_start_matches("NamespacedKey{").trim_end_matches('}').to_owned();
            Some(Ok(jrt::native(TagH(horizon_bukkit::registry::Tag::get(kind, &key), key))))
        }
        ("getTags", 2) => {
            let kind = tag_kind(&a[0].text());
            let all = horizon_bukkit::registry::Tag::all(kind);
            Some(Ok(jrt::list(all.into_iter().map(|tag| {
                let key = tag.get_key().map_or_else(String::new, |k| format!("{}:{}", k.get_namespace(), k.get_key()));
                jrt::native(TagH(tag, key))
            }).collect())))
        }
        _ => None,
    }
}

pub fn tag_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    let tag = this.native::<TagH>()?;
    Some(Ok(match (name, a.len()) {
        ("getValues", 0) => {
            let set = match jrt::new_other("LinkedHashSet", &[]) {
                Ok(set) => set,
                Err(e) => return Some(Err(e)),
            };
            for id in tag.0.get_values() {
                let _ = jrt::call(&set, "add", &[crate::wrap(horizon_bukkit::Material::of(&id))]);
            }
            set
        }
        ("isTagged", 1) => V::Bool(tag.0.is_tagged(&crate::text_arg(&a[0]))),
        ("getKey", 0) | ("toString", 0) => jrt::s(&tag.1),
        ("hashCode", 0) => V::Int(tag.1.len() as i32),
        ("equals", 1) => V::Bool(a[0].native::<TagH>().is_some_and(|o| o.1 == tag.1)),
        _ => return None,
    }))
}


// ---- Native edits (FAWE's //set, //replace, //undo, //redo of a box): the whole box in one engine query, the
// blocks as they were kept here (4 bytes a block) for undo. FAWE's converted Java only decides it's a plain box
// with a plain block; nothing per block runs in Java.

/// A request's own token: the engine answers a repeat of it with the first answer (a request from a plugin thread
/// can reach the game twice).
fn token() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    static RUN: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    let run = RUN.get_or_init(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    format!("{run:x}-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// The engine's ids of a state text (`minecraft:stone`, `minecraft:oak_stairs[facing=east,...]`), or of every state
/// of a block for `type:minecraft:oak_stairs`.
fn engine_ids(text: &str, out: &mut Vec<u32>) {
    let (names, ids) = states();
    if let Some(block) = text.strip_prefix("type:") {
        let block = block.trim_start_matches("minecraft:");
        static BY_BLOCK: std::sync::OnceLock<std::collections::HashMap<String, Vec<u32>>> = std::sync::OnceLock::new();
        let by_block = BY_BLOCK.get_or_init(|| {
            let mut map: std::collections::HashMap<String, Vec<u32>> = std::collections::HashMap::new();
            for (id, name) in names.iter().enumerate() {
                let block = name.split('[').next().unwrap_or_default().trim_start_matches("minecraft:").to_owned();
                map.entry(block).or_default().push(id as u32);
            }
            map
        });
        out.extend(by_block.get(block).into_iter().flatten().copied());
    } else if let Some(id) = ids.get(&state_key(text)) {
        out.push(*id);
    }
}

/// A line in `jrt-box.txt` when `JRT_TRACE` names "box" (jrt-env.txt): what each native edit asked and got.
pub fn debug_line(line: &str) {
    debug_box(line);
}

fn debug_box(line: &str) {
    if std::env::var("JRT_TRACE").is_ok_and(|w| w.contains("box")) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("jrt-box.txt") {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// `HorizonChunks.trueIndices(boolean[])`: where the array is true (FAWE's BlockMask holds its states as a flag per
/// ordinal: scanned here, not in converted Java).
fn true_indices(a: &[V]) -> R {
    let Some(V::Arr(flags)) = a.first() else { return Ok(jrt::arr(Vec::new())) };
    let found: Vec<V> = jrt::lock(flags).iter().enumerate().filter(|(_, f)| f.truth()).map(|(i, _)| V::Int(i as i32)).collect();
    Ok(jrt::arr(found))
}

/// `HorizonChunks.boxFill(world, x1, y1, z1, x2, y2, z2, String state, String[] only, boolean except)`: `long[]
/// {edit, changed}`, or null when the engine doesn't know the state (FAWE then does it its own way). `only` (null:
/// every block) holds state texts or `type:<block>`. One engine query: the box written chunk by chunk in the game,
/// the blocks as they were kept there for undo.
fn box_fill(a: &[V]) -> R {
    let Some(world) = a.first().and_then(world_of) else { return Ok(V::Null) };
    let n = |i: usize| a.get(i).map_or(0, V::int);
    let mut target = Vec::new();
    engine_ids(&a.get(7).map(V::text).unwrap_or_default(), &mut target);
    let Some(&state) = target.first() else {
        debug_box(&format!("boxFill: no engine state for {:?} ({} engine states known)", a.get(7).map(V::text), states().0.len()));
        return Ok(V::Null);
    };
    let mut query = serde_json::json!({
        "op": "box_fill", "dimension": dimension(&world), "min": [n(1), n(2), n(3)], "max": [n(4), n(5), n(6)], "state": state,
        "keep": true, "token": token(),
    });
    if let Some(list @ V::Arr(_)) = a.get(8) {
        let mut ids = Vec::new();
        for text in list.items().unwrap_or_default() {
            engine_ids(&text.text(), &mut ids);
        }
        ids.sort_unstable();
        ids.dedup();
        query["only"] = serde_json::json!(ids);
        query["except"] = serde_json::json!(a.get(9).is_some_and(V::truth));
    }
    let started = std::time::Instant::now();
    let answer = horizon_bukkit::horizon_mod::query(query);
    debug_box(&format!("boxFill {:?}: {answer} in {:?}", a.get(7).map(V::text), started.elapsed()));
    let (Some(edit), Some(changed)) = (answer["edit"].as_u64(), answer["changed"].as_u64()) else { return Ok(V::Null) };
    Ok(jrt::arr(vec![V::Long(edit as i64), V::Long(changed as i64)]))
}

/// A box's state ids as the engine has them ((y, z, x) order, FAWE's clipboard order), or None when the box isn't
/// all inside the world's height or the engine can't read it.
fn read_box(dimension: &str, min: [i32; 3], max: [i32; 3]) -> Option<Vec<u32>> {
    let answer = horizon_bukkit::horizon_mod::query(serde_json::json!({ "op": "box_read", "dimension": dimension, "min": min, "max": max }));
    if answer["bottom"].as_i64() != Some(i64::from(min[1])) || answer["top"].as_i64() != Some(i64::from(max[1])) {
        debug_box(&format!("box_read {min:?}..{max:?}: {}", answer.get("error").unwrap_or(&answer["bottom"])));
        return None;
    }
    let bytes = base64_decode(answer["data"].as_str()?);
    Some(bytes.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

/// `HorizonChunks.boxCopy(world, x1, y1, z1, x2, y2, z2, char[] states)`: the box read in one engine query into a
/// clipboard's array (FAWE's CPUOptimizedClipboard: x + z * width + y * area) as indices into the palette it answers
/// (state texts); `remap` then turns them into FAWE's ordinals. Null when the box can't be read that way.
fn box_copy(a: &[V]) -> R {
    let Some(world) = a.first().and_then(world_of) else { return Ok(V::Null) };
    let n = |i: usize| a.get(i).map_or(0, V::int);
    let Some(V::Arr(cells)) = a.get(7) else { return Ok(V::Null) };
    let started = std::time::Instant::now();
    let Some(ids) = read_box(&dimension(&world), [n(1), n(2), n(3)], [n(4), n(5), n(6)]) else { return Ok(V::Null) };
    let mut out = jrt::lock(cells);
    if ids.len() != out.len() {
        debug_box(&format!("boxCopy: {} blocks read for a clipboard of {}", ids.len(), out.len()));
        return Ok(V::Null);
    }
    let names = &states().0;
    let mut palette: Vec<V> = Vec::new();
    let mut index: std::collections::HashMap<u32, char> = std::collections::HashMap::new();
    for (slot, id) in out.iter_mut().zip(&ids) {
        let at = *index.entry(*id).or_insert_with(|| {
            palette.push(jrt::s(names.get(*id as usize).map_or("minecraft:air", String::as_str)));
            char::from_u32(palette.len() as u32 - 1).unwrap_or('\0')
        });
        *slot = V::Char(at);
    }
    debug_box(&format!("boxCopy {} blocks, {} states in {:?}", ids.len(), palette.len(), started.elapsed()));
    Ok(jrt::arr(palette))
}

/// `HorizonChunks.remap(char[] states, char[] to)`: each entry `i` of `states` made `to[i]`.
fn remap(a: &[V]) -> R {
    let (Some(V::Arr(states)), Some(V::Arr(to))) = (a.first(), a.get(1)) else { return Ok(V::Null) };
    let to: Vec<V> = jrt::lock(to).clone();
    for slot in jrt::lock(states).iter_mut() {
        if let Some(new) = to.get(slot.int() as usize) {
            *slot = new.clone();
        }
    }
    Ok(V::Null)
}

/// `HorizonChunks.distinct(char[] states)`: the values in the array, once each, smallest first.
fn distinct(a: &[V]) -> R {
    let Some(V::Arr(states)) = a.first() else { return Ok(jrt::arr(Vec::new())) };
    let mut seen = vec![false; 65536];
    for v in jrt::lock(states).iter() {
        seen[(v.int() as usize) & 0xFFFF] = true;
    }
    Ok(jrt::arr(seen.iter().enumerate().filter(|(_, s)| **s).map(|(i, _)| V::Int(i as i32)).collect()))
}

/// A paste done natively: the box before and after, for undo and redo. Kept here (the engine keeps //set's).
struct Paste {
    dimension: String,
    min: [i32; 3],
    max: [i32; 3],
    before: Vec<u32>,
    after: Vec<u32>,
}

/// Paste edit ids start here, apart from the engine's own edit ids.
const PASTE_IDS: u64 = 1 << 40;
static PASTES: std::sync::Mutex<Option<std::collections::HashMap<u64, Paste>>> = std::sync::Mutex::new(None);
static NEXT_PASTE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(PASTE_IDS);

fn write_ids(dimension: &str, min: [i32; 3], max: [i32; 3], ids: &[u32]) -> Option<u64> {
    let bytes: Vec<u8> = ids.iter().flat_map(|id| id.to_le_bytes()).collect();
    let answer = horizon_bukkit::horizon_mod::query(serde_json::json!({
        "op": "box_write", "dimension": dimension, "min": min, "max": max, "data": base64_encode(&bytes), "token": token(),
    }));
    answer["changed"].as_u64()
}

/// `HorizonChunks.boxPaste(world, x, y, z, width, height, length, char[] states, int[] ordinals, String[] texts,
/// boolean pasteAir)`: a clipboard's array written at (x, y, z) up in one engine query, `texts[i]` the state of
/// ordinal `ordinals[i]` (FAWE's); air left alone unless `pasteAir`, a state the engine doesn't know always. `long[]
/// {edit, changed}` (undo with boxUndo), or null when it can't be done this way.
fn box_paste(a: &[V]) -> R {
    let Some(world) = a.first().and_then(world_of) else { return Ok(V::Null) };
    let n = |i: usize| a.get(i).map_or(0, V::int);
    let (Some(V::Arr(cells)), Some(ordinals), Some(texts)) = (a.get(7), a.get(8), a.get(9)) else { return Ok(V::Null) };
    let paste_air = a.get(10).is_some_and(V::truth);
    let (w, h, l) = (n(4), n(5), n(6));
    if w <= 0 || h <= 0 || l <= 0 {
        return Ok(V::Null);
    }
    let started = std::time::Instant::now();
    let names = &states().0;
    let mut table = vec![u32::MAX; 65536];
    let (ordinals, texts) = (ordinals.items().unwrap_or_default(), texts.items().unwrap_or_default());
    for (ordinal, text) in ordinals.iter().zip(&texts) {
        let mut ids = Vec::new();
        engine_ids(&text.text(), &mut ids);
        let Some(&id) = ids.first() else { continue };
        let air = names.get(id as usize).is_some_and(|name| {
            matches!(name.split('[').next().unwrap_or_default().trim_start_matches("minecraft:"), "air" | "cave_air" | "void_air")
        });
        if paste_air || !air {
            table[(ordinal.int() as usize) & 0xFFFF] = id;
        }
    }
    let after: Vec<u32> = jrt::lock(cells).iter().map(|v| table[(v.int() as usize) & 0xFFFF]).collect();
    if after.len() != (w as usize) * (h as usize) * (l as usize) {
        return Ok(V::Null);
    }
    let dimension = dimension(&world);
    let (min, max) = ([n(1), n(2), n(3)], [n(1) + w - 1, n(2) + h - 1, n(3) + l - 1]);
    let Some(before) = read_box(&dimension, min, max) else { return Ok(V::Null) };
    let Some(changed) = write_ids(&dimension, min, max, &after) else { return Ok(V::Null) };
    let id = NEXT_PASTE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    jrt::lock(&PASTES).get_or_insert_with(Default::default).insert(id, Paste { dimension, min, max, before, after });
    debug_box(&format!("boxPaste {} blocks, {changed} changed in {:?}", w * h * l, started.elapsed()));
    Ok(jrt::arr(vec![V::Long(id as i64), V::Long(changed as i64)]))
}

/// `HorizonChunks.boxUndo(edit)` / `boxRedo(edit)`: the box back as it was, or the edit again. Blocks changed.
fn box_replay(a: &[V], undo: bool) -> R {
    let edit = a.first().map_or(0, V::long);
    let started = std::time::Instant::now();
    if edit as u64 >= PASTE_IDS {
        let found = jrt::lock(&PASTES).as_ref().and_then(|p| p.get(&(edit as u64))).map(|paste| {
            (paste.dimension.clone(), paste.min, paste.max, if undo { paste.before.clone() } else { paste.after.clone() })
        });
        let Some((dimension, min, max, ids)) = found else { return Ok(V::Int(0)) };
        let changed = write_ids(&dimension, min, max, &ids).unwrap_or(0);
        debug_box(&format!("paste {} {edit}: {changed} in {:?}", if undo { "undo" } else { "redo" }, started.elapsed()));
        return Ok(V::Int(changed as i32));
    }
    let answer = horizon_bukkit::horizon_mod::query(serde_json::json!({ "op": if undo { "box_undo" } else { "box_redo" }, "edit": edit, "token": token() }));
    debug_box(&format!("{} {edit}: {answer} in {:?}", if undo { "boxUndo" } else { "boxRedo" }, started.elapsed()));
    Ok(V::Int(answer["changed"].as_u64().unwrap_or(0) as i32))
}

fn box_forget(a: &[V]) -> R {
    let edit = a.first().map_or(0, V::long);
    if edit as u64 >= PASTE_IDS {
        jrt::lock(&PASTES).as_mut().map(|p| p.remove(&(edit as u64)));
        return Ok(V::Null);
    }
    let _ = horizon_bukkit::horizon_mod::query(serde_json::json!({ "op": "box_forget", "edit": edit }));
    Ok(V::Null)
}

pub fn static_method(class: &str, name: &str, a: &[V]) -> Option<R> {
    if let Some(found) = tag_static(class, name, a) {
        return Some(found);
    }
    if class.rsplit(['.', '$']).next() != Some("HorizonChunks") {
        return None;
    }
    Some(match (name, a.len()) {
        ("read", 3) => read(a),
        ("trueIndices", 1) => true_indices(a),
        ("boxFill", 10) => box_fill(a),
        ("boxUndo", 1) => box_replay(a, true),
        ("boxRedo", 1) => box_replay(a, false),
        ("boxForget", 1) => box_forget(a),
        ("boxCopy", 8) => box_copy(a),
        ("remap", 2) => remap(a),
        ("distinct", 1) => distinct(a),
        ("boxPaste", 11) => box_paste(a),
        ("palette", 1) => match a[0].native::<RawChunk>() {
            Some(chunk) => Ok(jrt::arr(chunk.palette.iter().map(|s| jrt::s(s)).collect())),
            None => Ok(V::Null),
        },
        ("bottom", 1) => Ok(V::Int(a[0].native::<RawChunk>().map_or(0, |c| c.bottom))),
        ("fill", 4) => fill(a),
        ("write", 4) => write(a),
        ("properties", 1) => properties(a),
        // The block's default state with every property (`minecraft:oak_stairs[facing=north,half=bottom,...]`):
        // WorldEdit reads each block's properties' defaults from it.
        ("defaultState", 1) => {
            let block = a[0].text();
            let facts = horizon_bukkit::horizon_mod::query(serde_json::json!({ "op": "items_block", "block": block }));
            Ok(jrt::s(facts["state"].as_str().filter(|s| !s.is_empty()).unwrap_or(&block)))
        }
        _ => Err(jrt::exc("NoSuchMethodError", format!("HorizonChunks.{name} with {} arguments", a.len()))),
    })
}
