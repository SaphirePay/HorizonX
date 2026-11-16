//! A plugin's own `ChunkGenerator` (`class VoidGen extends ChunkGenerator`, handed to `WorldCreator.generator`):
//! the SDK's generator trait, answered by the plugin's object. What the class doesn't override keeps Bukkit's
//! default. The `ChunkData` it writes is the game's own for the length of the call, as on Paper.
use horizon_bukkit::block_data::BlockData;
use horizon_bukkit::worlds::{BiomeGrid, ChunkData, ChunkGenerator, GeneratingWorld, JavaRandom};
use jrt::{R, V};
use std::cell::Cell;
use std::sync::Arc;

pub struct JavaGenerator(V);

/// The chunk being written on this thread, and which call it belongs to: a handle kept past it throws.
struct Current {
    data: *mut ChunkData,
    call: u64,
}

thread_local! {
    static CURRENT: Cell<Current> = const { Cell::new(Current { data: std::ptr::null_mut(), call: 0 }) };
    static CALLS: Cell<u64> = const { Cell::new(0) };
}

/// `ChunkGenerator.ChunkData` as the plugin holds it.
pub struct DataH {
    call: u64,
}

/// The plugin's generator object as the SDK's generator; `None` for a value that isn't one.
pub fn of(value: &V) -> Option<Arc<dyn ChunkGenerator>> {
    matches!(value, V::Obj(_)).then(|| Arc::new(JavaGenerator(value.clone())) as Arc<dyn ChunkGenerator>)
}

pub fn is_generator(value: &V) -> bool {
    matches!(value, V::Obj(_)) && (jrt::instance_of(value, "ChunkGenerator") || jrt::defines(value, "generateChunkData", 5) || jrt::defines(value, "generateNoise", 5) || jrt::defines(value, "generateSurface", 5))
}

impl JavaGenerator {
    fn flag(&self, name: &str) -> bool {
        jrt::defines(&self.0, name, 0) && jrt::report(name, jrt::call(&self.0, name, &[])).truth()
    }
    fn step(&self, name: &str, world: &GeneratingWorld, x: i32, z: i32, data: &mut ChunkData, old: bool) {
        if !jrt::defines(&self.0, name, if old { 5 } else { 5 }) {
            return;
        }
        let call = CALLS.get() + 1;
        CALLS.set(call);
        let before = CURRENT.replace(Current { data: data as *mut ChunkData, call });
        let random = jrt::new_builtin("Random", &[]).and_then(Result::ok).unwrap_or(V::Null);
        let handle = jrt::native(DataH { call });
        let last = if old { jrt::new_builtin("JStub", &[]).and_then(Result::ok).unwrap_or(V::Null) } else { handle };
        let args = [crate::wrap(world.clone()), random, V::Int(x), V::Int(z), last];
        let _ = jrt::report(&format!("ChunkGenerator.{name}"), jrt::call(&self.0, name, &args));
        CURRENT.set(before);
    }
}

impl ChunkGenerator for JavaGenerator {
    fn generate_chunk_data(&self, world: &GeneratingWorld, _random: &mut JavaRandom, chunk_x: i32, chunk_z: i32, _biomes: &mut BiomeGrid, data: &mut ChunkData) {
        self.step("generateChunkData", world, chunk_x, chunk_z, data, true);
    }
    fn generate_noise(&self, world: &GeneratingWorld, _random: &mut JavaRandom, chunk_x: i32, chunk_z: i32, data: &mut ChunkData) {
        self.step("generateNoise", world, chunk_x, chunk_z, data, false);
    }
    fn generate_surface(&self, world: &GeneratingWorld, _random: &mut JavaRandom, chunk_x: i32, chunk_z: i32, data: &mut ChunkData) {
        self.step("generateSurface", world, chunk_x, chunk_z, data, false);
    }
    fn generate_bedrock(&self, world: &GeneratingWorld, _random: &mut JavaRandom, chunk_x: i32, chunk_z: i32, data: &mut ChunkData) {
        self.step("generateBedrock", world, chunk_x, chunk_z, data, false);
    }
    fn generate_caves(&self, world: &GeneratingWorld, _random: &mut JavaRandom, chunk_x: i32, chunk_z: i32, data: &mut ChunkData) {
        self.step("generateCaves", world, chunk_x, chunk_z, data, false);
    }
    fn is_parallel_capable(&self) -> bool {
        self.flag("isParallelCapable")
    }
    fn should_generate_noise(&self) -> bool {
        self.flag("shouldGenerateNoise")
    }
    fn should_generate_surface(&self) -> bool {
        self.flag("shouldGenerateSurface")
    }
    fn should_generate_bedrock(&self) -> bool {
        self.flag("shouldGenerateBedrock")
    }
    fn should_generate_caves(&self) -> bool {
        self.flag("shouldGenerateCaves")
    }
    fn should_generate_decorations(&self) -> bool {
        self.flag("shouldGenerateDecorations")
    }
    fn should_generate_mobs(&self) -> bool {
        self.flag("shouldGenerateMobs")
    }
    fn should_generate_structures(&self) -> bool {
        self.flag("shouldGenerateStructures")
    }
}

/// The chunk of the call running on this thread, if `call` is it.
fn with_data<T>(call: u64, work: impl FnOnce(&mut ChunkData) -> T) -> Result<T, V> {
    let current = CURRENT.replace(Current { data: std::ptr::null_mut(), call: 0 });
    let (data, at) = (current.data, current.call);
    CURRENT.set(current);
    if data.is_null() || at != call {
        return Err(jrt::exc("IllegalStateException", "ChunkData used outside its generator call"));
    }
    // SAFETY: the pointer is the `&mut ChunkData` of the generator call still running on this thread (checked
    // above by its call number), and nothing else touches it while the plugin's code runs.
    Ok(work(unsafe { &mut *data }))
}

/// `this.createChunkData(world)` inside a generator, and the methods of the data it hands over.
pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let V::Obj(_) = this {
        // The chunk being generated: what the plugin writes into it lands in the game's chunk.
        if name == "createChunkData" && a.len() == 1 {
            let current = CURRENT.replace(Current { data: std::ptr::null_mut(), call: 0 });
            let call = current.call;
            let live = !current.data.is_null();
            CURRENT.set(current);
            return live.then(|| Ok(jrt::native(DataH { call })));
        }
        return None;
    }
    let handle = this.native::<DataH>()?;
    let int = |at: usize| a.get(at).map_or(0, V::int);
    let block = |value: &V| -> Option<Result<Material, BlockData>> {
        if let Some(data) = crate::held::<BlockData>(value) {
            return Some(Err(data.clone()));
        }
        crate::material(value).map(Ok)
    };
    type Material = horizon_bukkit::Material;
    Some(match (name, a.len()) {
        ("getMinHeight", 0) => with_data(handle.call, |d| V::Int(d.get_min_height())),
        ("getMaxHeight", 0) => with_data(handle.call, |d| V::Int(d.get_max_height())),
        ("setBlock", 4) => match block(&a[3]) {
            Some(Ok(material)) => with_data(handle.call, |d| { d.set_block(int(0), int(1), int(2), &material); V::Null }),
            Some(Err(data)) => with_data(handle.call, |d| { d.set_block_data(int(0), int(1), int(2), &data); V::Null }),
            None => Err(jrt::exc("IllegalArgumentException", "not a block")),
        },
        ("setRegion", 7) => {
            let (from, to) = ([int(0), int(1), int(2)], [int(3), int(4), int(5)]);
            match block(&a[6]) {
                Some(Ok(material)) => with_data(handle.call, |d| { d.set_region(from, to, &material); V::Null }),
                Some(Err(data)) => with_data(handle.call, |d| { d.set_region_data(from, to, &data); V::Null }),
                None => Err(jrt::exc("IllegalArgumentException", "not a block")),
            }
        }
        ("getType", 3) => with_data(handle.call, |d| crate::wrap(d.get_type(int(0), int(1), int(2)))),
        ("getBlockData", 3) => with_data(handle.call, |d| crate::wrap(d.get_block_data(int(0), int(1), int(2)))),
        ("getBiome", 3) => with_data(handle.call, |d| jrt::s(&d.get_biome(int(0), int(1), int(2)))),
        ("getData", 3) => with_data(handle.call, |d| V::Int(i32::from(d.get_data(int(0), int(1), int(2))))),
        ("getHeight", 3) => with_data(handle.call, |d| V::Int(d.get_height(&a[0].text(), int(1), int(2)))),
        ("hashCode", 0) => Ok(V::Int(handle.call as i32)),
        ("toString", 0) => Ok(jrt::s("ChunkData")),
        _ => return None,
    })
}
