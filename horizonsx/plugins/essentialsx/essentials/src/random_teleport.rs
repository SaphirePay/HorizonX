//! `RandomTeleport`. Java's `CompletableFuture<Location>` is a callback here. A chunk that isn't
//! loaded is forced for a moment with a ticket and read when it's in: nothing here loads one and
//! says when it's done.
use crate::prelude::*;
use crate::config::Config;
use crate::config_entities::LazyLocation;
use crate::utils::location::is_block_unsafe;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};

/// `getHighestBlockYAt` is the block itself on every game since 1.15.
const HIGHEST_BLOCK_Y_OFFSET: f64 = 1.0;
/// How long a chunk gets to load, in ticks, and how often it's looked at.
const CHUNK_WAIT: u32 = 200;
const CHUNK_POLL: u64 = 5;

pub struct RandomTeleport {
    config: RwLock<Config>,
    cached: Mutex<HashMap<String, VecDeque<Location>>>,
}

fn location_key(name: &str, key: &str) -> String {
    format!("locations.{name}.{key}")
}

/// `new Random()`: nothing here has to repeat.
fn next_double() -> f64 {
    use std::hash::{BuildHasher, Hasher};
    let bits = std::collections::hash_map::RandomState::new().build_hasher().finish();
    (bits >> 11) as f64 / (1u64 << 53) as f64
}

impl RandomTeleport {
    pub fn new(file: std::path::PathBuf) -> Self {
        let mut config = Config::with_template(file, include_str!("../resources/tpr.yml"));
        config.load();
        Self { config: RwLock::new(config), cached: Mutex::new(HashMap::new()) }
    }
    fn read(&self) -> std::sync::RwLockReadGuard<'_, Config> {
        self.config.read().unwrap_or_else(|e| e.into_inner())
    }
    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Config> {
        self.config.write().unwrap_or_else(|e| e.into_inner())
    }
    fn clear_cache(&self, name: &str) {
        self.cached.lock().unwrap_or_else(|e| e.into_inner()).remove(name);
    }
    /// `reloadConfig`.
    pub fn reload_config(&self) {
        self.write().load();
        self.cached.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    /// `getFile`.
    pub fn get_file(&self) -> std::path::PathBuf {
        self.read().get_file().to_path_buf()
    }
    /// `hasLocation`.
    pub fn has_location(&self, name: &str) -> bool {
        self.read().has_property(&format!("locations.{name}"))
    }
    /// `getCenter`.
    pub fn get_center(&self, name: &str) -> Location {
        if let Some(center) = self.read().get_location(Some(&location_key(name, "center"))).and_then(|mut center| center.location()) {
            return center;
        }
        let world = get_worlds().into_iter().next().unwrap_or_else(|| World::of("world"));
        let mut center = world.get_world_border().get_center();
        center.y = f64::from(world.get_highest_block_y_at(center.get_block_x(), center.get_block_z())) + HIGHEST_BLOCK_Y_OFFSET;
        self.set_center(name, &center);
        center
    }
    /// `setCenter`.
    pub fn set_center(&self, name: &str, center: &Location) {
        let mut config = self.write();
        config.set_location(Some(&location_key(name, "center")), &LazyLocation::from_location(center));
        config.save();
        drop(config);
        self.clear_cache(name);
    }
    /// `getMinRange`.
    pub fn get_min_range(&self, name: &str) -> f64 {
        self.read().get_double(&location_key(name, "min-range"), 0.0)
    }
    /// `setMinRange`.
    pub fn set_min_range(&self, name: &str, range: f64) {
        self.set_range(name, "min-range", range);
    }
    fn set_range(&self, name: &str, key: &str, range: f64) {
        let mut config = self.write();
        config.set(&location_key(name, key), serde_yaml::Value::from(range));
        config.save();
        drop(config);
        self.clear_cache(name);
    }
    /// `getMaxRange`.
    pub fn get_max_range(&self, name: &str) -> f64 {
        let fallback = self.get_center(name).world.get_world_border().get_size() / 2.0;
        self.read().get_double(&location_key(name, "max-range"), fallback)
    }
    /// `setMaxRange`.
    pub fn set_max_range(&self, name: &str, range: f64) {
        self.set_range(name, "max-range", range);
    }
    /// `getDefaultLocation`.
    pub fn get_default_location(&self) -> String {
        self.read().string_or("default-location", "{world}")
    }
    /// `isPerLocationPermission`.
    pub fn is_per_location_permission(&self) -> bool {
        self.read().get_boolean("per-location-permission", false)
    }
    fn excluded_biomes(&self) -> Vec<String> {
        self.read().get_string_list("excluded-biomes").iter().map(|key| key.to_lowercase()).collect()
    }
    fn find_attempts(&self) -> i32 {
        self.read().get_int("find-attempts", 10)
    }
    fn cache_threshold(&self) -> usize {
        self.read().get_int("cache-threshold", 10).max(0) as usize
    }
    /// `listLocations`.
    pub fn list_locations(&self) -> Vec<String> {
        self.read().section_keys("locations").unwrap_or_default()
    }
    /// `getRandomLocation(name)`.
    pub fn get_random_location(&'static self, name: &str, then: impl FnOnce(Location) + Send + 'static) {
        let mut cached_count = 0;
        let found = {
            let mut cached = self.cached.lock().unwrap_or_else(|e| e.into_inner());
            let queue = cached.entry(name.to_owned()).or_default();
            cached_count = cached_count.max(queue.len());
            queue.pop_front()
        };
        if cached_count < self.cache_threshold() {
            self.cache_random_locations(name.to_owned());
        }
        match found {
            Some(location) => then(location),
            None => {
                let center = self.get_center(name);
                let (min, max) = (self.get_min_range(name), self.get_max_range(name));
                self.attempt(self.find_attempts(), center, min, max, Box::new(then));
            }
        }
    }
    /// `cacheRandomLocations`.
    fn cache_random_locations(&'static self, name: String) {
        let mut task = Some(move || {
            let center = self.get_center(&name);
            let (min, max) = (self.get_min_range(&name), self.get_max_range(&name));
            for _ in 0..self.find_attempts() {
                let name = name.clone();
                self.calculate(&center, min, max, Box::new(move |location| {
                    if self.is_valid(&location) {
                        self.cached.lock().unwrap_or_else(|e| e.into_inner()).entry(name).or_default().push_back(location);
                    }
                }));
            }
        });
        ess().schedule_sync_delayed_task(move || {
            if let Some(task) = task.take() {
                task();
            }
        });
    }
    /// `attemptRandomLocation`.
    fn attempt(&'static self, attempts: i32, center: Location, min: f64, max: f64, then: Box<dyn FnOnce(Location) + Send>) {
        if attempts <= 0 {
            then(center);
            return;
        }
        let from = center.clone();
        self.calculate(&from, min, max, Box::new(move |location| {
            if self.is_valid(&location) {
                then(location);
            } else {
                self.attempt(attempts - 1, center, min, max, then);
            }
        }));
    }
    /// `calculateRandomLocation`.
    fn calculate(&'static self, center: &Location, min: f64, max: f64, then: Box<dyn FnOnce(Location) + Send>) {
        let rect_x = next_double() * (max - min) + min;
        let rect_z = next_double() * (max + min) - min;
        let (offset_x, offset_z) = match (next_double() * 4.0) as u8 {
            0 => (rect_x, rect_z),
            1 => (-rect_z, rect_x),
            2 => (-rect_x, -rect_z),
            _ => (rect_z, -rect_x),
        };
        let world = center.world.clone();
        let mut location = Location::new(world.clone(), center.x + offset_x, f64::from(world.get_max_height()), center.z + offset_z, 360.0 * next_double() as f32 - 180.0, 0.0);
        let (chunk_x, chunk_z) = (location.get_block_x() >> 4, location.get_block_z() >> 4);
        with_chunk(world.clone(), chunk_x, chunk_z, Box::new(move |loaded| {
            if loaded {
                location.y = if world.get_environment() == "NETHER" { nether_y_at(&location) } else { f64::from(world.get_highest_block_y_at(location.get_block_x(), location.get_block_z())) + HIGHEST_BLOCK_Y_OFFSET };
            }
            then(location);
        }));
    }
    /// `isValidRandomLocation`.
    fn is_valid(&self, location: &Location) -> bool {
        location.get_block_y() > location.world.get_min_height() && !self.is_excluded_biome(location)
    }
    fn is_excluded_biome(&self, location: &Location) -> bool {
        let excluded = self.excluded_biomes();
        location.get_block().get_biome().is_some_and(|biome| {
            let key = biome.to_lowercase();
            excluded.iter().any(|known| *known == key || format!("minecraft:{known}") == key || *known == key.trim_start_matches("minecraft:"))
        })
    }
}

type Ready = Arc<Mutex<Option<Box<dyn FnOnce(bool) + Send>>>>;

/// `PaperLib.getChunkAtAsync`: `then` when the chunk is in, at once if it's in already, with whether it
/// came. Nothing else loads one, so it's forced with a ticket for as long as `then` takes.
pub fn with_chunk(world: World, x: i32, z: i32, then: Box<dyn FnOnce(bool) + Send>) {
    let chunk = world.get_chunk_at(x, z);
    let ticket = !chunk.is_loaded() && chunk.add_plugin_chunk_ticket();
    let held = world.clone();
    wait_for_chunk(
        CHUNK_WAIT,
        Box::new(move |loaded| {
            then(loaded);
            if ticket {
                held.get_chunk_at(x, z).remove_plugin_chunk_ticket();
            }
        }),
        world,
        x,
        z,
    );
}

fn wait_for_chunk(left: u32, ready: Box<dyn FnOnce(bool) + Send>, world: World, x: i32, z: i32) {
    if world.get_chunk_at(x, z).is_loaded() {
        ready(true);
        return;
    }
    if left == 0 {
        ready(false);
        return;
    }
    let slot: Ready = Arc::new(Mutex::new(Some(ready)));
    ess().schedule_sync_delayed_task_later(
        move || {
            if let Some(ready) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
                wait_for_chunk(left.saturating_sub(CHUNK_POLL as u32), ready, world, x, z);
            }
        },
        CHUNK_POLL,
    );
}

/// `getNetherYAt`: Java's `Double.MIN_VALUE` is what fails the check on the way out.
fn nether_y_at(location: &Location) -> f64 {
    let world = &location.world;
    let (x, z) = (location.get_block_x(), location.get_block_z());
    for y in 32..world.get_max_height() {
        if world.get_block_at(x, y, z).get_type() == Material::of("BEDROCK") {
            break;
        }
        if !is_block_unsafe(world, x, y, z) {
            return f64::from(y);
        }
    }
    f64::MIN_POSITIVE
}
