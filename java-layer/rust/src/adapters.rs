//! SDK traits a Java plugin implements with its own object (a mob `Goal`): each call goes to that object.

use horizon_bukkit::fam_entity::{Goal, GoalKey};
use horizon_bukkit::persistence::NamespacedKey;
use jrt::V;

/// A Java `com.destroystokyo.paper.entity.ai.Goal`: its key and types are read once (Paper reads them as the
/// goal is added too), the rest is asked of the Java object every tick.
pub struct JavaGoal {
    java: V,
    key: GoalKey,
    types: Vec<&'static str>,
}

impl JavaGoal {
    pub fn new(java: V) -> Self {
        // [key, entity class name, [type names]] from Bridge.invokeRef's "$goal".
        let about = crate::natives::layer_call(&java, "$goal", &[]).and_then(|r| r.ok()).unwrap_or(V::Null);
        let rows = about.items().unwrap_or_default();
        let text = |i: usize| rows.get(i).map(|v| v.text()).unwrap_or_default();
        let key = NamespacedKey::from_string(&text(0)).unwrap_or_else(|| NamespacedKey::minecraft("java_goal"));
        let types = rows.get(2).and_then(|v| v.items().ok()).unwrap_or_default().iter()
            .map(|t| match t.text().as_str() {
                "MOVE" => "MOVE",
                "LOOK" => "LOOK",
                "JUMP" => "JUMP",
                "TARGET" => "TARGET",
                _ => "UNKNOWN_BEHAVIOR",
            })
            .collect();
        Self { java, key: GoalKey::of(&text(1), key), types }
    }

    fn ask(&self, name: &str) -> bool {
        match crate::natives::layer_call(&self.java, name, &[]) {
            Some(Ok(v)) => v.truth(),
            _ => false,
        }
    }

    fn run(&self, name: &str) {
        if let Some(Err(e)) = crate::natives::layer_call(&self.java, name, &[]) {
            eprintln!("[JavaLayer] goal {:?} threw in {name}: {}", self.key.get_namespaced_key(), e.text());
        }
    }
}

impl Goal for JavaGoal {
    fn should_activate(&mut self) -> bool {
        self.ask("shouldActivate")
    }
    fn should_stay_active(&mut self) -> bool {
        self.ask("shouldStayActive")
    }
    fn start(&mut self) {
        self.run("start")
    }
    fn stop(&mut self) {
        self.run("stop")
    }
    fn tick(&mut self) {
        self.run("tick")
    }
    fn get_key(&self) -> GoalKey {
        self.key.clone()
    }
    fn get_types(&self) -> Vec<&'static str> {
        self.types.clone()
    }
}

// ---- World generation: a Java ChunkGenerator, BiomeProvider or BlockPopulator ----

use horizon_bukkit::worlds::{BiomeGrid, BiomeParameterPoint, BiomeProvider, BlockPopulator, ChunkData, ChunkGenerator, GeneratingWorld, JavaRandom, LimitedRegion};
use horizon_bukkit::World;
use std::sync::{Arc, Mutex};

/// The Java objects behind adapters, by the adapter's address: what World.getGenerator and the like give back.
static JAVA_OF: Mutex<Vec<(usize, V)>> = Mutex::new(Vec::new());

fn remember<T: ?Sized>(adapter: &Arc<T>, java: &V) {
    let mut all = JAVA_OF.lock().unwrap_or_else(|e| e.into_inner());
    all.push((Arc::as_ptr(adapter) as *const () as usize, java.clone()));
}

/// The Java object an SDK generator, provider or populator stands for, if one does.
pub fn java_of<T: ?Sized>(adapter: &Arc<T>) -> V {
    let at = Arc::as_ptr(adapter) as *const () as usize;
    JAVA_OF.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(p, _)| *p == at).map(|(_, v)| v.clone()).unwrap_or(V::Null)
}

/// "$gen" on the Java object: Bridge.invokeRef hands it to Generators.call, which converts for Java.
fn java_gen(java: &V, op: &str, mut args: Vec<V>) -> V {
    args.insert(0, jrt::s(op));
    match crate::natives::layer_call(java, "$gen", &args) {
        Some(Ok(v)) => v,
        Some(Err(e)) => {
            eprintln!("[JavaLayer] world generator threw in {op}: {}", e.text());
            V::Null
        }
        None => V::Null,
    }
}

fn flag(java: &V, name: &str) -> bool {
    matches!(crate::natives::layer_call(java, name, &[]), Some(Ok(v)) if v.truth())
}

/// Data lent to Java (as a handle it writes through) and taken back after.
fn lend<T: Default + Send + 'static>(value: &mut T, run: impl FnOnce(V) -> V) -> V {
    let held = jbridge::wrap(std::mem::take(value));
    let out = run(held.clone());
    if let Some(cell) = held.native::<Mutex<T>>() {
        *value = std::mem::take(&mut *cell.lock().unwrap_or_else(|e| e.into_inner()));
    }
    out
}

/// What a call gave back: [the Random's state after, a value]; the state goes on to the next step.
fn after(random: &mut JavaRandom, out: &V) -> V {
    let rows = out.items().unwrap_or_default();
    if let Some(state) = rows.first().filter(|s| !s.is_null()) {
        random.set_state(state.long());
    }
    rows.get(1).cloned().unwrap_or(V::Null)
}

pub struct JavaChunkGenerator {
    java: V,
}

impl JavaChunkGenerator {
    pub fn new(java: V) -> Arc<dyn ChunkGenerator> {
        let made: Arc<dyn ChunkGenerator> = Arc::new(Self { java: java.clone() });
        remember(&made, &java);
        made
    }

    fn step(&self, op: &str, world: &GeneratingWorld, random: &mut JavaRandom, x: i32, z: i32, data: &mut ChunkData) {
        let java = &self.java;
        let state = random.state();
        let out = lend(data, |held| java_gen(java, op, vec![jbridge::wrap(world.clone()), V::Long(state), V::Int(x), V::Int(z), held]));
        after(random, &out);
    }
}

impl ChunkGenerator for JavaChunkGenerator {
    fn generate_chunk_data(&self, world: &GeneratingWorld, random: &mut JavaRandom, x: i32, z: i32, biomes: &mut BiomeGrid, data: &mut ChunkData) {
        let java = &self.java;
        let state = random.state();
        // The old way returns a new ChunkData (made with createChunkData): its blocks go over this one's.
        let out = lend(biomes, |grid| lend(data, |held| java_gen(java, "chunk", vec![jbridge::wrap(world.clone()), V::Long(state), V::Int(x), V::Int(z), grid, held])));
        let made = after(random, &out);
        if let Some(cell) = made.native::<Mutex<ChunkData>>() {
            let made = cell.lock().unwrap_or_else(|e| e.into_inner());
            data.overlay(&made);
        }
    }
    fn generate_noise(&self, world: &GeneratingWorld, random: &mut JavaRandom, x: i32, z: i32, data: &mut ChunkData) {
        self.step("noise", world, random, x, z, data)
    }
    fn generate_surface(&self, world: &GeneratingWorld, random: &mut JavaRandom, x: i32, z: i32, data: &mut ChunkData) {
        self.step("surface", world, random, x, z, data)
    }
    fn generate_bedrock(&self, world: &GeneratingWorld, random: &mut JavaRandom, x: i32, z: i32, data: &mut ChunkData) {
        self.step("bedrock", world, random, x, z, data)
    }
    fn generate_caves(&self, world: &GeneratingWorld, random: &mut JavaRandom, x: i32, z: i32, data: &mut ChunkData) {
        self.step("caves", world, random, x, z, data)
    }
    fn get_default_biome_provider(&self) -> Option<Arc<dyn BiomeProvider>> {
        let java = java_gen(&self.java, "biome_provider", vec![]);
        (!java.is_null()).then(|| JavaBiomeProvider::new(java))
    }
    fn get_base_height(&self, world: &GeneratingWorld, random: &mut JavaRandom, x: i32, z: i32, height_map: &str) -> i32 {
        let out = java_gen(&self.java, "base_height", vec![jbridge::wrap(world.clone()), V::Long(random.state()), V::Int(x), V::Int(z), jrt::s(height_map)]);
        after(random, &out).long() as i32
    }
    fn can_spawn(&self, world: &World, x: i32, z: i32) -> bool {
        java_gen(&self.java, "can_spawn", vec![jbridge::wrap(world.clone()), V::Int(x), V::Int(z)]).truth()
    }
    fn get_default_populators(&self, world: &World) -> Vec<Arc<dyn BlockPopulator>> {
        let list = java_gen(&self.java, "populators", vec![jbridge::wrap(world.clone())]);
        list.items().unwrap_or_default().into_iter().filter(|p| !p.is_null()).map(JavaBlockPopulator::new).collect()
    }
    fn get_fixed_spawn_location(&self, world: &World, random: &mut JavaRandom) -> Option<horizon_bukkit::Location> {
        let out = java_gen(&self.java, "spawn", vec![jbridge::wrap(world.clone()), V::Long(random.state())]);
        let at = after(random, &out).items().ok()?;
        let n = |i: usize| at.get(i).map(|v| v.double()).unwrap_or(0.0);
        (at.len() >= 5).then(|| horizon_bukkit::Location::new(world.clone(), n(0), n(1), n(2), n(3) as f32, n(4) as f32))
    }
    fn is_parallel_capable(&self) -> bool {
        flag(&self.java, "isParallelCapable")
    }
    fn should_generate_noise(&self) -> bool {
        flag(&self.java, "shouldGenerateNoise")
    }
    fn should_generate_surface(&self) -> bool {
        flag(&self.java, "shouldGenerateSurface")
    }
    fn should_generate_bedrock(&self) -> bool {
        flag(&self.java, "shouldGenerateBedrock")
    }
    fn should_generate_caves(&self) -> bool {
        flag(&self.java, "shouldGenerateCaves")
    }
    fn should_generate_decorations(&self) -> bool {
        flag(&self.java, "shouldGenerateDecorations")
    }
    fn should_generate_mobs(&self) -> bool {
        flag(&self.java, "shouldGenerateMobs")
    }
    fn should_generate_structures(&self) -> bool {
        flag(&self.java, "shouldGenerateStructures")
    }
}

pub struct JavaBiomeProvider {
    java: V,
}

impl JavaBiomeProvider {
    pub fn new(java: V) -> Arc<dyn BiomeProvider> {
        let made: Arc<dyn BiomeProvider> = Arc::new(Self { java: java.clone() });
        remember(&made, &java);
        made
    }
}

fn biome_name(out: V) -> String {
    if out.is_null() { "minecraft:plains".to_owned() } else { out.text() }
}

impl BiomeProvider for JavaBiomeProvider {
    fn get_biome(&self, world: &GeneratingWorld, x: i32, y: i32, z: i32) -> String {
        biome_name(java_gen(&self.java, "biome", vec![jbridge::wrap(world.clone()), V::Int(x), V::Int(y), V::Int(z)]))
    }
    fn get_biome_at(&self, world: &GeneratingWorld, x: i32, y: i32, z: i32, point: &BiomeParameterPoint) -> String {
        let climate = [point.get_temperature(), point.get_humidity(), point.get_continentalness(), point.get_erosion(), point.get_depth(), point.get_weirdness()];
        let climate = jrt::list(climate.iter().map(|c| V::Double(*c)).collect());
        biome_name(java_gen(&self.java, "biome_at", vec![jbridge::wrap(world.clone()), V::Int(x), V::Int(y), V::Int(z), climate]))
    }
    fn get_biomes(&self, world: &GeneratingWorld) -> Vec<String> {
        java_gen(&self.java, "biomes", vec![jbridge::wrap(world.clone())]).items().unwrap_or_default().iter().map(|b| b.text()).collect()
    }
}

pub struct JavaBlockPopulator {
    java: V,
}

impl JavaBlockPopulator {
    pub fn new(java: V) -> Arc<dyn BlockPopulator> {
        let made: Arc<dyn BlockPopulator> = Arc::new(Self { java: java.clone() });
        remember(&made, &java);
        made
    }
}

impl BlockPopulator for JavaBlockPopulator {
    fn populate(&self, world: &World, random: &mut JavaRandom, chunk_x: i32, chunk_z: i32, region: &LimitedRegion) {
        let args = vec![jbridge::wrap(world.clone()), V::Long(random.state()), V::Int(chunk_x), V::Int(chunk_z), jbridge::wrap(region.clone())];
        let out = java_gen(&self.java, "populate", args);
        after(random, &out);
    }
}

// ---- AsyncStructureGenerateEvent: a Java BlockTransformer or EntityTransformer ----

use horizon_bukkit::block_data::BlockData;
use horizon_bukkit::fam_core::{BlockTransformer, EntityTransformer, TransformationState};

pub struct JavaBlockTransformer {
    java: V,
}

impl JavaBlockTransformer {
    pub fn new(java: V) -> Self {
        Self { java }
    }
}

impl BlockTransformer for JavaBlockTransformer {
    fn transform(&self, region: &LimitedRegion, x: i32, y: i32, z: i32, current: BlockData, state: &TransformationState) -> BlockData {
        let args = vec![jbridge::wrap(region.clone()), V::Int(x), V::Int(y), V::Int(z), jbridge::wrap(current.clone()), jbridge::wrap(state.get_original().clone()), jbridge::wrap(state.get_world().clone())];
        let out = java_gen(&self.java, "transform_block", args);
        out.native::<Mutex<BlockData>>().map(|d| d.lock().unwrap_or_else(|e| e.into_inner()).clone()).unwrap_or(current)
    }
}

pub struct JavaEntityTransformer {
    java: V,
}

impl JavaEntityTransformer {
    pub fn new(java: V) -> Self {
        Self { java }
    }
}

impl EntityTransformer for JavaEntityTransformer {
    fn transform(&self, region: &LimitedRegion, x: i32, y: i32, z: i32, entity: &mut horizon_bukkit::entities3::EntitySnapshot, allowed_to_spawn: bool) -> bool {
        let args = vec![jbridge::wrap(region.clone()), V::Int(x), V::Int(y), V::Int(z), jrt::s(&entity.0), V::Bool(allowed_to_spawn)];
        let out = java_gen(&self.java, "transform_entity", args);
        // [allowed, the entity's data after] (a plugin may change it through the entity it was given).
        let rows = out.items().unwrap_or_default();
        if let Some(snbt) = rows.get(1).filter(|v| !v.is_null()) {
            entity.0 = snbt.text();
        }
        rows.first().map_or(allowed_to_spawn, |v| v.truth())
    }
}

// ---- Maps: a Java MapRenderer, drawn by the SDK's map loop ----

use horizon_bukkit::fam_core::{MapRenderer, MapView};
use horizon_bukkit::pure2::{MapCanvas, MapCursor, MapCursorCollection};

/// A Java `org.bukkit.map.MapRenderer`: the SDK asks it to draw (Bridge.invokeRef "$map" -> MapRenderers.render) for
/// each player holding the map, each tick. The base and the picture cross as one 16384-char string (a byte each).
pub struct JavaMapRenderer {
    java: V,
    contextual: bool,
}

impl JavaMapRenderer {
    pub fn new(java: V, contextual: bool) -> Self {
        Self { java, contextual }
    }
}

impl MapRenderer for JavaMapRenderer {
    fn render(&self, map: &MapView, canvas: &mut MapCanvas, player: &horizon_bukkit::Player) {
        let mut base = String::with_capacity(128 * 128 * 2);
        for i in 0..128 * 128 {
            base.push(char::from(canvas.get_base_pixel(i % 128, i / 128)));
        }
        let args = [V::Int(map.get_id()), jbridge::wrap(player.clone()), jrt::s(&base)];
        let drawn = match crate::natives::layer_call(&self.java, "$map", &args) {
            Some(Ok(v)) => v,
            Some(Err(e)) => {
                eprintln!("[JavaLayer] map renderer threw in render: {}", e.text());
                return;
            }
            None => return,
        };
        let rows = drawn.items().unwrap_or_default();
        let Some(picture) = rows.first() else {
            return;
        };
        // 255 is Java's -1: nothing drawn there, the base shows (CraftMapView.render skips it too).
        for (i, c) in picture.text().chars().take(128 * 128).enumerate() {
            let color = c as u32;
            if color < 255 {
                canvas.set_pixel(i as i32 % 128, i as i32 / 128, color as u8);
            }
        }
        let mut cursors = MapCursorCollection::default();
        for row in rows.get(1).and_then(|v| v.items().ok()).unwrap_or_default() {
            let f = row.items().unwrap_or_default();
            let int = |i: usize| f.get(i).map(|v| v.long()).unwrap_or(0);
            let kind = f.get(3).map(|v| v.text()).unwrap_or_default();
            let mut cursor = MapCursor::new(int(0) as i8, int(1) as i8, int(2) as u8, horizon_bukkit::fam_core::map_cursor_type_value(&kind).unwrap_or(0), f.get(4).is_none_or(|v| v.truth()));
            if let Some(caption) = f.get(5).filter(|v| !v.is_null()) {
                cursor.set_caption(Some(&caption.text()));
            }
            cursors.add_cursor(cursor);
        }
        canvas.set_cursors(cursors);
    }
    fn is_contextual(&self) -> bool {
        self.contextual
    }
}

/// The adapters added to maps, by (map id, the Java renderer's identity): what removeRenderer takes away.
static MAP_RENDERERS: Mutex<Vec<(i32, i32, Arc<dyn MapRenderer>)>> = Mutex::new(Vec::new());

pub fn add_map_renderer(map: &MapView, java: V, identity: i32, contextual: bool) {
    let arc = map.add_renderer(JavaMapRenderer::new(java, contextual));
    MAP_RENDERERS.lock().unwrap_or_else(|e| e.into_inner()).push((map.get_id(), identity, arc));
}

pub fn remove_map_renderer(map: &MapView, identity: i32) -> bool {
    let found = {
        let mut all = MAP_RENDERERS.lock().unwrap_or_else(|e| e.into_inner());
        all.iter().position(|(id, who, _)| *id == map.get_id() && *who == identity).map(|i| all.remove(i).2)
    };
    found.is_some_and(|arc| map.remove_renderer(&arc))
}

/// MapView.removeRenderer of the game's own (CraftMapRenderer): the SDK's game renderer comes off that map, so only
/// the plugins' drawing shows, as on Paper.
pub fn game_renderer_off(map: &MapView) -> bool {
    let ours: Vec<*const ()> = MAP_RENDERERS.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|(id, ..)| *id == map.get_id()).map(|(.., arc)| Arc::as_ptr(arc).cast::<()>()).collect();
    for renderer in map.get_renderers() {
        if renderer.foreign().is_none() && !ours.contains(&Arc::as_ptr(&renderer).cast::<()>()) {
            return map.remove_renderer(&renderer);
        }
    }
    false
}
