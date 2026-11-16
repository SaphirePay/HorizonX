//! `org.bukkit.entity.Entity` for what isn't a player, `org.bukkit.Chunk`, and what sits beside them.
use crate::{Location, World};
use crate::run_command;
use horizon_mod::{Value, query};
use serde_json::json;

/// The last answers about each entity (its small fields only, no saved tree), for when it is gone.
static LAST: std::sync::Mutex<Option<std::collections::HashMap<(u64, &'static str), Value>>> = std::sync::Mutex::new(None);

fn remember(id: u64, what: &'static str, state: &Value) {
    let Value::Object(fields) = state else {
        return;
    };
    let mut small: serde_json::Map<String, Value> = fields.iter().filter(|(key, _)| key.as_str() != "data").map(|(k, v)| (k.clone(), v.clone())).collect();
    // Of the saved tree only its health, which getHealth falls back to.
    if let Some(health) = fields.get("data").and_then(|data| data.get("Health")) {
        small.insert("data".to_owned(), json!({ "Health": health }));
    }
    let mut all = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let all = all.get_or_insert_with(Default::default);
    // Bounded: a long-running server sees millions of entities come and go.
    if all.len() > 50_000 {
        all.clear();
    }
    all.insert((id, what), Value::Object(small));
}

fn last(id: u64, what: &'static str) -> Value {
    LAST.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|all| all.get(&(id, what)).cloned()).unwrap_or(Value::Null)
}

/// What every class of entity is: `entity.cast::<Zombie>()` is Java's `(Zombie) entity`.
pub trait EntityClass {
    fn of(entity: Entity) -> Self;
}

/// bukkit: org/bukkit/entity/Entity, org/bukkit/entity/LivingEntity, org/bukkit/entity/Mob, org/bukkit/entity/Damageable
///
/// Anything in a level but a player, by its id; a mob's calls are here too.
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    id: u64,
    dimension: String,
    uuid: String,
}

impl Entity {
    pub(crate) fn read(value: &Value) -> Option<Self> {
        Some(Self { id: value["id"].as_u64()?, dimension: value["dimension"].as_str().unwrap_or("minecraft:overworld").to_owned(), uuid: value["uuid"].as_str().unwrap_or_default().to_owned() })
    }
    /// An entity by the id an event gave.
    pub fn of(id: u64, world: &World) -> Self {
        let mut uuid = query(json!({ "op": "entity", "id": id, "data": false }))["uuid"].as_str().unwrap_or_default().to_owned();
        // A player isn't one of the level's entities: their id is a player's (a Player seen as a LivingEntity).
        if uuid.is_empty()
            && let Some(player) = crate::get_online_players().into_iter().find(|player| player.get_entity_id() as u64 == id)
        {
            uuid = player.uuid().to_owned();
        }
        Self { id, dimension: world.dimension().to_owned(), uuid }
    }
    /// Another view of the same entity, as a Java cast.
    pub fn cast<C: EntityClass>(&self) -> C {
        C::of(self.clone())
    }
    fn state(&self) -> Value {
        let got = Some(query(json!({ "op": "entity", "id": self.id, "data": true }))).filter(|state| !state.is_null()).unwrap_or_else(|| query(json!({ "op": "entity_kept", "id": self.id, "dimension": self.dimension })));
        remember(self.id, "entity", &got);
        got
    }
    fn mob_state(&self) -> Value {
        let got = query(json!({ "op": "mob", "dimension": self.dimension, "id": self.id }));
        remember(self.id, "mob", &got);
        got
    }
    /// `state`, or what it last was once the entity is gone (removed, or its chunk unloaded): a CraftEntity keeps
    /// its position, type and health then, so getLocation/getType/getHealth still answer on Paper.
    fn known(&self) -> Value {
        let got = self.state();
        if got.is_null() { last(self.id, "entity") } else { got }
    }
    fn known_mob(&self) -> Value {
        let got = self.mob_state();
        if got.is_null() { last(self.id, "mob") } else { got }
    }
    /// The tree the game saves for this entity.
    pub fn get_data(&self) -> Value {
        self.state()["data"].take()
    }
    /// One path of `get_data`.
    pub fn data_at(&self, path: &str) -> Value {
        crate::data::at(&self.get_data(), path).clone()
    }
    /// `/data merge entity`: at once, unlike a command in a player's event.
    pub fn merge(&self, snbt: &str) -> bool {
        query(json!({ "op": "entity_merge", "id": self.id, "snbt": snbt })).as_bool().unwrap_or(false) || query(json!({ "op": "entity_merge_kept", "id": self.id, "dimension": self.dimension, "snbt": snbt })).as_bool().unwrap_or(false)
    }
    pub(crate) fn location_of(&self, value: &Value) -> Option<Location> {
        let at = |i: usize| value[i].as_f64();
        Some(Location::new(self.get_world(), at(0)?, at(1)?, at(2)?, 0.0, 0.0))
    }
    fn act(&self, action: &str, value: Value) -> bool {
        query(json!({ "op": "mob_action", "action": action, "dimension": self.dimension, "id": self.id, "value": value })).as_bool().unwrap_or(false)
    }
    /// bukkit: org/bukkit/entity/Entity.getEntityId
    pub fn get_entity_id(&self) -> u64 {
        self.id
    }
    /// bukkit: org/bukkit/entity/Entity.getUniqueId
    pub fn get_unique_id(&self) -> String {
        crate::dashed(&self.uuid)
    }
    /// bukkit: org/bukkit/entity/Entity.getNearbyEntities
    ///
    /// In a box `x y z` each way around it, itself left out, as Bukkit's.
    pub fn get_nearby_entities_around(&self, x: f64, y: f64, z: f64) -> Vec<Entity> {
        let Some(at) = self.get_location() else { return Vec::new() };
        let me = self.get_unique_id();
        at.get_nearby_entities(x, y, z).into_iter().filter(|other| other.get_unique_id() != me).collect()
    }
    /// bukkit: org/bukkit/entity/Entity.getType
    ///
    /// The game's id: `minecraft:zombie`.
    pub fn get_type(&self) -> String {
        self.known()["kind"].as_str().unwrap_or_default().to_owned()
    }
    /// bukkit: org/bukkit/entity/Entity.isValid
    pub fn is_valid(&self) -> bool {
        // CraftEntity.isValid: in the world and alive (a living entity at no health is not).
        !self.is_dead()
    }
    /// bukkit: org/bukkit/entity/Entity.isDead, org/bukkit/entity/LivingEntity.isDead
    pub fn is_dead(&self) -> bool {
        // Entity.isAlive: gone, or a living one at no health (dying, before it's removed).
        let state = self.state();
        if state.is_null() {
            return true;
        }
        // Only what has health (a living entity's saved `Health`): an item or an arrow has none.
        let health = self.mob_state()["health"].as_f64().or_else(|| crate::data::at(&state["data"], "Health").as_f64());
        health.is_some_and(|health| health <= 0.0)
    }
    /// bukkit: org/bukkit/entity/Entity.getWorld
    pub fn get_world(&self) -> World {
        World::of(&self.dimension)
    }
    /// bukkit: org/bukkit/entity/Entity.getLocation
    pub fn get_location(&self) -> Option<Location> {
        let state = self.known();
        let at = |i: usize| state["pos"][i].as_f64();
        Some(Location::new(self.get_world(), at(0)?, at(1)?, at(2)?, state["yaw"].as_f64().unwrap_or(0.0) as f32, state["pitch"].as_f64().unwrap_or(0.0) as f32))
    }
    /// bukkit: org/bukkit/entity/Entity.getX
    pub fn get_x(&self) -> f64 {
        self.get_location().map_or(0.0, |at| at.x)
    }
    /// bukkit: org/bukkit/entity/Entity.getY
    pub fn get_y(&self) -> f64 {
        self.get_location().map_or(0.0, |at| at.y)
    }
    /// bukkit: org/bukkit/entity/Entity.getZ
    pub fn get_z(&self) -> f64 {
        self.get_location().map_or(0.0, |at| at.z)
    }
    /// bukkit: org/bukkit/entity/Entity.teleport, org/bukkit/entity/Entity.teleportAsync
    pub fn teleport(&self, to: &Location) -> bool {
        if self.act("teleport", json!([to.x, to.y, to.z])) {
            return true;
        }
        self.merge(&format!("{{Pos:[{:?}d,{:?}d,{:?}d],Rotation:[{:?}f,{:?}f]}}", to.x, to.y, to.z, to.yaw, to.pitch))
    }
    /// bukkit: org/bukkit/entity/Entity.setVelocity
    pub fn set_velocity(&self, x: f64, y: f64, z: f64) -> bool {
        self.act("push", json!([x, y, z])) || self.merge(&format!("{{Motion:[{x:?}d,{y:?}d,{z:?}d]}}"))
    }
    /// bukkit: org/bukkit/entity/Entity.getVelocity
    pub fn get_velocity(&self) -> [f64; 3] {
        let motion = self.data_at("Motion");
        [0, 1, 2].map(|i| motion[i].as_f64().unwrap_or(0.0))
    }
    /// bukkit: org/bukkit/entity/Entity.isOnGround
    pub fn is_on_ground(&self) -> bool {
        crate::data::flag(&self.data_at("OnGround"))
    }
    /// bukkit: org/bukkit/entity/Entity.getScoreboardTags
    pub fn get_scoreboard_tags(&self) -> Vec<String> {
        self.data_at("Tags").as_array().into_iter().flatten().filter_map(|tag| tag.as_str().map(str::to_owned)).collect()
    }
    fn write_tags(&self, tags: &[String]) -> bool {
        self.merge(&format!("{{Tags:[{}]}}", tags.iter().map(|tag| crate::data::quoted(tag)).collect::<Vec<_>>().join(",")))
    }
    /// bukkit: org/bukkit/entity/Entity.addScoreboardTag
    pub fn add_scoreboard_tag(&self, tag: &str) -> bool {
        let mut tags = self.get_scoreboard_tags();
        if tags.iter().any(|known| known == tag) {
            return false;
        }
        tags.push(tag.to_owned());
        self.write_tags(&tags)
    }
    /// bukkit: org/bukkit/entity/Entity.removeScoreboardTag
    pub fn remove_scoreboard_tag(&self, tag: &str) -> bool {
        let mut tags = self.get_scoreboard_tags();
        let before = tags.len();
        tags.retain(|known| known != tag);
        tags.len() != before && self.write_tags(&tags)
    }
    /// bukkit: org/bukkit/entity/Entity.customName, org/bukkit/entity/Nameable.customName
    pub fn custom_name(&self) -> Option<crate::Component> {
        let name = self.data_at("CustomName");
        (!name.is_null()).then_some(crate::Component(name))
    }
    /// bukkit: org/bukkit/entity/Entity.customName, org/bukkit/Nameable.customName
    pub fn set_custom_name_component(&self, name: &crate::Component) -> bool {
        self.merge(&format!("{{CustomName:{}}}", crate::data::snbt(&name.0)))
    }
    /// bukkit: org/bukkit/Nameable.getCustomName
    pub fn get_custom_name(&self) -> Option<String> {
        self.custom_name().map(|name| name.to_legacy())
    }
    /// bukkit: org/bukkit/Nameable.setCustomName
    pub fn set_custom_name(&self, name: &str) -> bool {
        self.set_custom_name_component(&crate::adventure::legacy(name))
    }
    /// bukkit: org/bukkit/entity/LivingEntity.getHealth, org/bukkit/entity/Damageable.getHealth
    pub fn get_health(&self) -> f64 {
        self.known_mob()["health"].as_f64().or(self.data_at("Health").as_f64()).or_else(|| self.known()["data"]["Health"].as_f64()).unwrap_or(0.0)
    }
    /// bukkit: org/bukkit/entity/LivingEntity.getMaxHealth, org/bukkit/entity/Damageable.getMaxHealth
    pub fn get_max_health(&self) -> f64 {
        self.known_mob()["max_health"].as_f64().unwrap_or(0.0)
    }
    /// bukkit: org/bukkit/entity/LivingEntity.setHealth, org/bukkit/entity/Damageable.setHealth
    pub fn set_health(&self, health: f64) -> bool {
        // CraftLivingEntity.setHealth(0): the entity dies then (its death event, its drops) by die(generic), with no
        // damage event first. A server without the `die` action (or a player) falls back to the kill.
        if health <= 0.0 {
            return self.act("die", Value::Null) || self.kill();
        }
        self.act("health", json!(health))
    }
    /// bukkit: org/bukkit/entity/Entity.remove
    pub fn remove(&self) -> bool {
        self.act("remove", Value::Null)
    }
    /// bukkit: org/bukkit/entity/Damageable.kill, org/bukkit/entity/LivingEntity.kill
    pub fn kill(&self) -> bool {
        self.act("kill", Value::Null)
    }
    /// bukkit~: org/bukkit/entity/Damageable.damage | a command, so it lands at the end of the tick inside a player's event
    pub fn damage(&self, amount: f64) {
        run_command(&format!("damage {} {amount}", crate::dashed(&self.uuid)));
    }
    /// bukkit: org/bukkit/entity/Damageable.heal
    pub fn heal(&self, amount: f64) -> bool {
        self.set_health((self.get_health() + amount).min(self.get_max_health()))
    }
}

impl World {
    /// bukkit: org/bukkit/World.getEntities
    pub fn get_entities(&self) -> Vec<Entity> {
        let mut entities = self.entities_but_players();
        entities.extend(self.player_entities(None));
        entities
    }
    /// bukkit: org/bukkit/World.getLivingEntities
    ///
    /// What is a `LivingEntity` in Java: mobs, armor stands and players, not items, orbs, projectiles, vehicles,
    /// displays or other objects.
    pub fn get_living_entities(&self) -> Vec<Entity> {
        fn living(kind: &str) -> bool {
            let kind = kind.trim_start_matches("minecraft:");
            !(kind.ends_with("minecart") || kind.ends_with("boat") || kind.ends_with("raft") || kind.ends_with("_display") || kind.ends_with("arrow") || kind.ends_with("fireball") || kind.ends_with("_potion") || kind.ends_with("wind_charge"))
                && !matches!(
                    kind,
                    "item" | "experience_orb" | "trident" | "snowball" | "egg" | "ender_pearl" | "eye_of_ender" | "wither_skull" | "firework_rocket" | "fishing_bobber"
                        | "llama_spit" | "shulker_bullet" | "experience_bottle" | "tnt" | "falling_block" | "area_effect_cloud" | "lightning_bolt" | "marker" | "interaction"
                        | "item_frame" | "glow_item_frame" | "painting" | "leash_knot" | "end_crystal" | "evoker_fangs" | "ominous_item_spawner" | "potion"
                )
        }
        let rows = query(json!({ "op": "entities", "dimension": self.dimension() }));
        let kept = query(json!({ "op": "entity_kept_list", "dimension": self.dimension() }));
        let mut entities: Vec<Entity> = rows.as_array().into_iter().flatten().chain(kept.as_array().into_iter().flatten()).filter(|row| living(row["kind"].as_str().unwrap_or_default())).filter_map(Entity::read).collect();
        entities.extend(self.player_entities(None));
        entities
    }
    fn entities_but_players(&self) -> Vec<Entity> {
        query(json!({ "op": "entities", "dimension": self.dimension() })).as_array().into_iter().flatten().filter_map(Entity::read).chain(query(json!({ "op": "entity_kept_list", "dimension": self.dimension() })).as_array().into_iter().flatten().filter_map(Entity::read)).collect()
    }
    /// bukkit: org/bukkit/World.getNearbyEntities
    pub fn get_nearby_entities(&self, at: &Location, x: f64, y: f64, z: f64) -> Vec<Entity> {
        let mut entities = self.nearby_but_players(at, x, y, z);
        entities.extend(self.player_entities(Some((at, x, y, z))));
        entities
    }
    fn nearby_but_players(&self, at: &Location, x: f64, y: f64, z: f64) -> Vec<Entity> {
        query(json!({ "op": "entities", "dimension": self.dimension() }))
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entity| {
                let near = |i: usize, center: f64, range: f64| entity["pos"][i].as_f64().is_some_and(|value| (value - center).abs() <= range);
                near(0, at.x, x) && near(1, at.y, y) && near(2, at.z, z)
            })
            .filter_map(Entity::read)
            .collect()
    }
    /// bukkit: org/bukkit/World.spawn, org/bukkit/RegionAccessor.spawn, org/bukkit/World.spawnEntity, org/bukkit/RegionAccessor.spawnEntity
    pub fn spawn(&self, at: &Location, kind: &str) -> Option<Entity> {
        let id = if kind.contains(':') { kind.to_owned() } else { format!("minecraft:{}", kind.to_ascii_lowercase()) };
        // A falling block is the game's sand until it's told otherwise (with no BlockState it would be air, and gone).
        let extra = if id == "minecraft:falling_block" { r#",BlockState:{Name:"minecraft:sand"}"# } else { "" };
        let snbt = format!("{{id:{},Pos:[{:?}d,{:?}d,{:?}d],Rotation:[{:?}f,{:?}f]{extra}}}", crate::data::quoted(&id), at.x, at.y, at.z, at.yaw, at.pitch);
        let spawned = Entity::read(&crate::fam_ev_entity::plugin_spawn(json!({ "op": "spawn", "dimension": self.dimension(), "snbt": snbt })))?;
        if id == "minecraft:experience_orb" {
            crate::fam_entity::orb_spawned_by_plugin(&spawned);
        }
        Some(spawned)
    }
    /// bukkit: org/bukkit/World.getChunkAt
    pub fn get_chunk_at(&self, x: i32, z: i32) -> Chunk {
        Chunk { world: self.clone(), x, z }
    }
    /// bukkit: org/bukkit/World.isChunkLoaded
    pub fn is_chunk_loaded(&self, x: i32, z: i32) -> bool {
        self.get_chunk_at(x, z).is_loaded()
    }
}

/// `org.bukkit.Chunk`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    world: World,
    x: i32,
    z: i32,
}

impl Chunk {
    /// bukkit: org/bukkit/Chunk.getX
    pub fn get_x(&self) -> i32 {
        self.x
    }
    /// bukkit: org/bukkit/Chunk.getZ
    pub fn get_z(&self) -> i32 {
        self.z
    }
    /// bukkit: org/bukkit/Chunk.getWorld
    pub fn get_world(&self) -> &World {
        &self.world
    }
    /// bukkit: org/bukkit/Chunk.isLoaded
    pub fn is_loaded(&self) -> bool {
        query(json!({ "op": "chunk_loaded", "dimension": self.world.dimension(), "chunk": [self.x, self.z] })).as_bool().unwrap_or(false)
    }
    /// bukkit: org/bukkit/Chunk.getBlock
    pub fn get_block(&self, x: i32, y: i32, z: i32) -> crate::Block {
        self.world.get_block_at(self.x * 16 + x, y, self.z * 16 + z)
    }
}

impl Location {
    /// bukkit: org/bukkit/Location.getChunk
    pub fn get_chunk(&self) -> Chunk {
        self.world.get_chunk_at(self.get_block_x() >> 4, self.get_block_z() >> 4)
    }
}
