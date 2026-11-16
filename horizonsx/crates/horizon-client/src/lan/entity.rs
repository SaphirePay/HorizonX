//! Entity tracking (ChunkMap.TrackedEntity / ServerEntity): the game describes
//! every entity it simulates as an `EntityState` each tick. A level's
//! `Entities` remember what its clients were last told about each entity,
//! once for all of them as vanilla's ServerEntity does, and produce the
//! movement, motion, metadata and equipment packets that bring them up to
//! date at the entity type's update interval (`tracking`); each client's
//! `Tracker` is the set of entities it has been paired with.
use super::tracking;
use super::tracking_config::{self, Group, TrackingConfig};
use horizon_protocol::{
    buf::pack_degrees,
    entity_data::{Value, serializer},
    item::Stack,
    packets::play::{
        self, AddEntity, AttributeSnapshot, EntityPositionSync, MoveEntity, MoveEntityPos,
        MoveEntityPosRot, MoveEntityRot, PositionPath, RotateHead, SetEntityData, SetEntityLink,
        SetEntityMotion, SetEquipment, SetPassengers, UpdateAttributes, VecDelta,
    },
};

/// The attribute instances of `now` that differ from what `sent` held (an
/// instance is never removed: one that disappears is left as it was sent).
pub fn changed_attributes(
    now: &[AttributeSnapshot],
    sent: &[AttributeSnapshot],
) -> Vec<AttributeSnapshot> {
    now.iter()
        .filter(|a| {
            sent.iter()
                .find(|b| b.attribute == a.attribute)
                .is_none_or(|b| b != *a)
        })
        .cloned()
        .collect()
}

/// What the game says about one entity this tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityState {
    pub id: i32,
    pub uuid: u128,
    /// Registry id of the entity type (`minecraft:zombie`).
    pub kind: String,
    pub position: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub head_yaw: f32,
    pub on_ground: bool,
    pub velocity: [f64; 3],
    /// ClientboundAddEntityPacket.data (item frame direction, block state ...).
    pub data: i32,
    /// SynchedEntityData entries that differ from the defaults.
    pub meta: Vec<(u8, i32, Value)>,
    /// The defaults of entries `meta` leaves out while they are at them,
    /// sent when one goes back to its default (a calf grown up, a mob healed
    /// back to 1, wool grown back white) beyond Entity's own below.
    pub defaults: Vec<(u8, i32, Value)>,
    /// EquipmentSlot ordinals (0 main hand, 1 off hand, 2 feet, 3 legs,
    /// 4 chest, 5 head, 6 body, 7 saddle) and their stacks.
    pub equipment: Vec<(u8, Option<Stack>)>,
    /// A living entity's client-syncable attribute instances
    /// (`AttributeMap.getSyncableAttributes`), by registry id.
    pub attributes: Vec<AttributeSnapshot>,
    /// `Entity.getRequiresPrecisePosition` (primed TNT and falling blocks
    /// touching a block): moves are sent as full position syncs.
    pub precise: bool,
    /// `Entity.needsSync`: send its changes this tick, due or not.
    pub needs_sync: bool,
    /// `Leashable.getLeashHolder`: the entity holding its lead.
    pub leash: Option<i32>,
    /// `ServerPlayer.isSpectator`: a player in spectator mode, whom only the
    /// clients of other spectators track (`ServerPlayer.broadcastToPlayer`).
    pub spectator: bool,
    /// `ServerPlayer.getCamera() != this`: a spectator watching through
    /// another entity is shown to nobody (`ServerPlayer.broadcastToPlayer`).
    pub watching: bool,
}

impl EntityState {
    /// The entity `id` of type `kind` as a fresh state has it: at the origin,
    /// at rest, with no data, equipment or attributes. Keeps the allocations
    /// of the strings and vectors it held.
    pub fn reset(&mut self, id: i32, uuid: u128, kind: &str) {
        self.id = id;
        self.uuid = uuid;
        self.kind.clear();
        self.kind.push_str(kind);
        self.position = [0.0; 3];
        (self.yaw, self.pitch, self.head_yaw) = (0.0, 0.0, 0.0);
        self.on_ground = false;
        self.velocity = [0.0; 3];
        self.data = 0;
        self.meta.clear();
        self.defaults.clear();
        self.equipment.clear();
        self.attributes.clear();
        self.precise = false;
        self.needs_sync = false;
        self.leash = None;
        self.spectator = false;
        self.watching = false;
    }
}

/// The states a level describes its entities with, kept from tick to tick:
/// each tick's are written over the last tick's in place, so the strings and
/// vectors in them are reused instead of being made and freed again for every
/// entity on every tick.
#[derive(Clone, Default)]
pub struct States {
    list: Vec<EntityState>,
    len: usize,
}

impl States {
    /// Start a new tick's states.
    pub fn clear(&mut self) {
        self.len = 0;
    }
    /// The next entity's state, reset (`EntityState::reset`).
    pub fn add(&mut self, id: i32, uuid: u128, kind: &str) -> &mut EntityState {
        if self.len == self.list.len() {
            self.list.push(EntityState::default());
        }
        let e = &mut self.list[self.len];
        self.len += 1;
        e.reset(id, uuid, kind);
        e
    }
    /// The next `n` entities' states, to be filled (each `reset` first) on any thread: HorizonsX builds
    /// the mobs' states on every core.
    pub fn add_many(&mut self, n: usize) -> &mut [EntityState] {
        let start = self.len;
        if self.list.len() < start + n {
            self.list.resize_with(start + n, EntityState::default);
        }
        self.len += n;
        &mut self.list[start..start + n]
    }
    pub fn push(&mut self, e: EntityState) {
        match self.list.get_mut(self.len) {
            Some(slot) => *slot = e,
            None => self.list.push(e),
        }
        self.len += 1;
    }
    pub fn as_slice(&self) -> &[EntityState] {
        &self.list[..self.len]
    }
    pub fn as_mut_slice(&mut self) -> &mut [EntityState] {
        &mut self.list[..self.len]
    }
}

impl States {
    /// HorizonsX: `other`'s states moved to the end of these (a region's, back from its thread), `other` left empty.
    pub fn append(&mut self, other: &mut States) {
        for e in &mut other.list[..other.len] {
            self.push(std::mem::take(e));
        }
        other.len = 0;
    }
}

impl Extend<EntityState> for States {
    fn extend<I: IntoIterator<Item = EntityState>>(&mut self, states: I) {
        for e in states {
            self.push(e);
        }
    }
}

/// `AbstractArrow`: every update sends its position and rotation.
fn is_arrow(kind: &str) -> bool {
    matches!(
        kind,
        "minecraft:arrow" | "minecraft:spectral_arrow" | "minecraft:trident"
    )
}

/// `AbstractHurtingProjectile.accelerationPower` (0.1, none for wind
/// charges), sent with each velocity update.
fn acceleration_power(kind: &str) -> Option<f64> {
    match kind {
        "minecraft:fireball"
        | "minecraft:small_fireball"
        | "minecraft:dragon_fireball"
        | "minecraft:wither_skull" => Some(0.1),
        "minecraft:wind_charge" | "minecraft:breeze_wind_charge" => Some(0.0),
        _ => None,
    }
}

/// Who rides what this tick (`Entity.getPassengers`, `isPassenger`).
#[derive(Default)]
pub struct Riding {
    /// Each vehicle's passengers, in order.
    pub passengers: crate::fast_hash::FxHashMap<i32, Vec<i32>>,
    /// Each passenger's vehicle.
    pub vehicle: crate::fast_hash::FxHashMap<i32, i32>,
}

impl Riding {
    pub fn clear(&mut self) {
        self.passengers.clear();
        self.vehicle.clear();
    }
    /// `passenger` rides `vehicle`, after the ones already riding it.
    pub fn add(&mut self, vehicle: i32, passenger: i32) {
        self.passengers.entry(vehicle).or_default().push(passenger);
        self.vehicle.insert(passenger, vehicle);
    }
    pub fn passengers(&self, id: i32) -> &[i32] {
        self.passengers.get(&id).map_or(&[], Vec::as_slice)
    }
    fn set_passengers(&self, vehicle: i32) -> play::Clientbound {
        play::Clientbound::SetPassengers(SetPassengers {
            vehicle,
            passengers: self.passengers(vehicle).to_vec(),
        })
    }
}

/// What a level's clients were last told about one entity (ServerEntity's
/// fields), shared by every client that tracks it.
struct ServerEntity {
    /// The registry id of its type (None: a type the clients don't know,
    /// never paired).
    kind: Option<i32>,
    /// The type's tracking range in chunks, update interval (0: never),
    /// whether velocity is tracked, whether it is an `AbstractArrow` and its
    /// `AbstractHurtingProjectile.accelerationPower`.
    range: i32,
    /// HorizonsX: its group for the server's tracking ranges ([`tracking_config`]).
    group: Group,
    interval: u32,
    track_deltas: bool,
    arrow: bool,
    power: Option<f64>,
    /// VecDeltaCodec's base: the last position sent.
    base: [f64; 3],
    /// lastSentYRot / lastSentXRot / lastSentYHeadRot.
    rotation: (i8, i8),
    head: i8,
    /// lastSentMovement.
    velocity: [f64; 3],
    /// trackedDataValues: the entity data the clients hold.
    meta: Vec<(u8, i32, Value)>,
    equipment: Vec<(u8, Option<Stack>)>,
    /// The attribute instances the clients hold.
    attributes: Vec<AttributeSnapshot>,
    was_on_ground: bool,
    tick: u32,
    teleport_delay: u32,
    /// lastPassengers, and wasRiding: a passenger's position isn't synced,
    /// and the first update after it dismounts is a full position sync.
    passengers: Vec<i32>,
    was_riding: bool,
    /// The lead's holder the clients were told of.
    leash: Option<i32>,
    /// The level tick it was last seen in, its place in that tick's list, and
    /// whether it was added in it (its first update is the next tick's).
    seen: u32,
    index: usize,
    added: bool,
    /// TrackedEntity.lastSectionPos: its pairs are checked again when it
    /// moves to another section.
    section: [i32; 3],
    /// Whether it was a spectator last tick: its pairs are checked again when
    /// that changes, as `ServerPlayer.broadcastToPlayer` answers otherwise.
    spectator: bool,
    /// Likewise for a spectator who watches through a camera.
    watching: bool,
}

/// SectionPos.of(entity).
fn section_of(position: [f64; 3]) -> [i32; 3] {
    position.map(|v| (v.floor() as i32) >> 4)
}

/// VecDeltaCodec.encode.
fn encode(v: f64) -> i64 {
    (v * 4096.0).round() as i64
}

impl ServerEntity {
    /// `new ServerEntity`: the entity as it is when it joins the level.
    fn new(e: &EntityState, kind: Option<i32>, riding: &Riding) -> Self {
        let (range, interval, track_deltas) = tracking::settings(&e.kind);
        Self {
            kind,
            range: i32::from(range),
            group: Group::of(&e.kind),
            interval,
            track_deltas,
            arrow: is_arrow(&e.kind),
            power: acceleration_power(&e.kind),
            base: e.position,
            rotation: (pack_degrees(e.pitch), pack_degrees(e.yaw)),
            head: pack_degrees(e.head_yaw),
            velocity: e.velocity,
            meta: e.meta.clone(),
            equipment: e.equipment.clone(),
            attributes: e.attributes.clone(),
            was_on_ground: e.on_ground,
            tick: 0,
            teleport_delay: 0,
            passengers: riding.passengers(e.id).to_vec(),
            was_riding: false,
            leash: e.leash,
            seen: 0,
            index: 0,
            added: true,
            section: section_of(e.position),
            spectator: e.spectator,
            watching: e.watching,
        }
    }

    /// ServerEntity.sendPairingData: the packets that introduce the entity
    /// to a client that starts tracking it.
    fn pairing(
        &self,
        e: &EntityState,
        kind: i32,
        riding: &Riding,
        out: &mut Vec<play::Clientbound>,
    ) {
        out.push(play::Clientbound::AddEntity(AddEntity {
            id: e.id,
            uuid: e.uuid,
            kind,
            position: e.position,
            velocity: e.velocity,
            pitch: pack_degrees(e.pitch),
            yaw: pack_degrees(e.yaw),
            head_yaw: pack_degrees(e.head_yaw),
            data: e.data,
        }));
        if !self.meta.is_empty() {
            out.push(play::Clientbound::SetEntityData(SetEntityData {
                id: e.id,
                values: self.meta.clone(),
            }));
        }
        // getSyncableAttributes: the instances as they are now.
        if !e.attributes.is_empty() {
            out.push(play::Clientbound::UpdateAttributes(UpdateAttributes {
                id: e.id,
                values: e.attributes.clone(),
            }));
        }
        if !self.equipment.is_empty() {
            out.push(play::Clientbound::SetEquipment(SetEquipment {
                id: e.id,
                slots: self.equipment.clone(),
            }));
        }
        if !riding.passengers(e.id).is_empty() {
            out.push(riding.set_passengers(e.id));
        }
        if let Some(&vehicle) = riding.vehicle.get(&e.id) {
            out.push(riding.set_passengers(vehicle));
        }
        // A leashed entity: who holds its lead.
        if let Some(dest) = e.leash {
            out.push(play::Clientbound::SetEntityLink(SetEntityLink {
                source: e.id,
                dest,
            }));
        }
    }

    /// Changes since the last update (ServerEntity.sendChanges): movement and
    /// rotation every update interval (positions at least every 60 ticks, a
    /// full position sync every 400 or when the entity lands or leaves the
    /// ground), velocity for types that track it, changed data at once.
    fn send_changes(&mut self, e: &EntityState, riding: &Riding, out: &mut Vec<play::Clientbound>) {
        let passengers = riding.passengers(e.id);
        if passengers != self.passengers.as_slice() {
            out.push(riding.set_passengers(e.id));
            self.passengers.clear();
            self.passengers.extend_from_slice(passengers);
        }
        // Leashable.setLeashedTo / dropLeash: sendToTrackingPlayers at once.
        if e.leash != self.leash {
            out.push(play::Clientbound::SetEntityLink(SetEntityLink {
                source: e.id,
                dest: e.leash.unwrap_or(0),
            }));
            self.leash = e.leash;
        }
        // SynchedEntityData.isDirty: once for every client.
        let dirty = e.meta != self.meta;
        let due = self.interval != 0 && self.tick.is_multiple_of(self.interval);
        if due || dirty || e.needs_sync {
            let rotation = (pack_degrees(e.pitch), pack_degrees(e.yaw));
            let turned = |a: i8, b: i8| (i32::from(a) - i32::from(b)).abs() >= 1;
            let rotate = self.arrow
                || turned(rotation.1, self.rotation.1)
                || turned(rotation.0, self.rotation.0);
            if riding.vehicle.contains_key(&e.id) {
                // A passenger: its rotation alone (its vehicle places it).
                if rotate {
                    out.push(play::Clientbound::MoveEntityRot(MoveEntityRot(
                        MoveEntity {
                            id: e.id,
                            delta: None,
                            rotation: Some((rotation.1, rotation.0)),
                            on_ground: e.on_ground,
                        },
                    )));
                    self.rotation = rotation;
                }
                self.base = e.position;
                self.was_riding = true;
                self.send_dirty(e, dirty, out);
                self.send_head(e, out);
                self.tick += 1;
                self.send_equipment(e, out);
                return;
            }
            self.teleport_delay += 1;
            let d: f64 = (0..3).map(|k| (e.position[k] - self.base[k]).powi(2)).sum();
            let send_position = self.arrow || d >= 7.629_394_5e-6 || self.tick.is_multiple_of(60);
            let sync = EntityPositionSync {
                id: e.id,
                path: PositionPath::Linear(e.position),
                yaw: e.yaw,
                pitch: e.pitch,
                on_ground: e.on_ground,
            };
            // ServerEntity.createMovePacket.
            let packet = if e.precise
                || self.teleport_delay > 400
                || self.was_riding
                || self.was_on_ground != e.on_ground
            {
                self.was_on_ground = e.on_ground;
                self.teleport_delay = 0;
                self.was_riding = false;
                Some(play::Clientbound::EntityPositionSync(sync))
            } else if send_position {
                let delta = [0, 1, 2].map(|k| encode(e.position[k]) - encode(self.base[k]));
                if delta.iter().any(|v| i16::try_from(*v).is_err()) {
                    Some(play::Clientbound::EntityPositionSync(sync))
                } else {
                    let m = MoveEntity {
                        id: e.id,
                        delta: Some(VecDelta::Linear(delta.map(|v| v as i16))),
                        rotation: rotate.then_some((rotation.1, rotation.0)),
                        on_ground: e.on_ground,
                    };
                    Some(if rotate {
                        play::Clientbound::MoveEntityPosRot(MoveEntityPosRot(m))
                    } else {
                        play::Clientbound::MoveEntityPos(MoveEntityPos(m))
                    })
                }
            } else if rotate {
                Some(play::Clientbound::MoveEntityRot(MoveEntityRot(
                    MoveEntity {
                        id: e.id,
                        delta: None,
                        rotation: Some((rotation.1, rotation.0)),
                        on_ground: e.on_ground,
                    },
                )))
            } else {
                None
            };
            if self.track_deltas || e.needs_sync {
                let diff: f64 = (0..3)
                    .map(|k| (e.velocity[k] - self.velocity[k]).powi(2))
                    .sum();
                let still = e.velocity.iter().all(|v| *v == 0.0);
                if diff > 1.0e-7 || diff > 0.0 && still {
                    self.velocity = e.velocity;
                    let motion = play::Clientbound::SetEntityMotion(SetEntityMotion {
                        id: e.id,
                        velocity: e.velocity,
                    });
                    match self.power {
                        // A bundle: the motion and the projectile's power.
                        Some(power) => out.extend([
                            play::Clientbound::BundleDelimiter(horizon_protocol::packets::Empty),
                            motion,
                            play::Clientbound::ProjectilePower(play::ProjectilePower {
                                id: e.id,
                                power,
                            }),
                            play::Clientbound::BundleDelimiter(horizon_protocol::packets::Empty),
                        ]),
                        None => out.push(motion),
                    }
                }
            }
            if let Some(packet) = packet {
                let (position, rotated) = match &packet {
                    play::Clientbound::EntityPositionSync(_)
                    | play::Clientbound::MoveEntityPosRot(_) => (true, true),
                    play::Clientbound::MoveEntityPos(_) => (true, false),
                    _ => (false, true),
                };
                out.push(packet);
                if position {
                    self.base = e.position;
                }
                if rotated {
                    self.rotation = rotation;
                }
            }
            self.send_dirty(e, dirty, out);
            self.send_head(e, out);
        }
        self.tick += 1;
        self.send_equipment(e, out);
    }

    /// ServerEntity.sendDirtyEntityData: the changed entity data, then the
    /// attribute instances that changed since they were last sent
    /// (`AttributeMap.getAttributesToSync`).
    fn send_dirty(&mut self, e: &EntityState, dirty: bool, out: &mut Vec<play::Clientbound>) {
        if dirty {
            let mut changed: Vec<_> = e
                .meta
                .iter()
                .filter(|m| !self.meta.contains(m))
                .cloned()
                .collect();
            // A value back at its default is left out of the state, but the
            // clients still hold the old one: Entity's shared flags and pose go
            // back to 0 (a player standing up again, a mob no longer on fire),
            // a sleeper's bed to none, an item frame's item to empty and its
            // rotation to 0 (ItemFrame.DATA_ITEM and DATA_ROTATION).
            let frame = e.kind.ends_with("item_frame");
            for (index, kind, _) in &self.meta {
                let default = match (*index, *kind) {
                    (0, serializer::BYTE) => Value::Byte(0),
                    (6, serializer::POSE) => Value::Id(0),
                    (14, serializer::OPTIONAL_BLOCK_POS) => Value::OptPos(None),
                    (9, serializer::ITEM_STACK) if frame => Value::Item(None),
                    (10, serializer::INT) if frame => Value::Int(0),
                    _ => match e.defaults.iter().find(|d| (d.0, d.1) == (*index, *kind)) {
                        Some(d) => d.2.clone(),
                        None => continue,
                    },
                };
                if !e.meta.iter().any(|m| m.0 == *index) {
                    changed.push((*index, *kind, default));
                }
            }
            if !changed.is_empty() {
                out.push(play::Clientbound::SetEntityData(SetEntityData {
                    id: e.id,
                    values: changed,
                }));
            }
            self.meta.clone_from(&e.meta);
        }
        if e.attributes != self.attributes {
            let changed = changed_attributes(&e.attributes, &self.attributes);
            if !changed.is_empty() {
                out.push(play::Clientbound::UpdateAttributes(UpdateAttributes {
                    id: e.id,
                    values: changed,
                }));
            }
            self.attributes.clone_from(&e.attributes);
        }
    }

    fn send_head(&mut self, e: &EntityState, out: &mut Vec<play::Clientbound>) {
        let head = pack_degrees(e.head_yaw);
        if (i32::from(head) - i32::from(self.head)).abs() >= 1 {
            out.push(play::Clientbound::RotateHead(RotateHead {
                id: e.id,
                head_yaw: head,
            }));
            self.head = head;
        }
    }

    /// LivingEntity.detectEquipmentUpdates: changed slots every tick.
    fn send_equipment(&mut self, e: &EntityState, out: &mut Vec<play::Clientbound>) {
        if e.equipment != self.equipment {
            // Cleared slots are sent as empty stacks.
            let mut slots: Vec<(u8, Option<Stack>)> = e
                .equipment
                .iter()
                .filter(|m| !self.equipment.contains(m))
                .cloned()
                .collect();
            for (slot, _) in &self.equipment {
                if !e.equipment.iter().any(|(k, _)| k == slot) {
                    slots.push((*slot, None));
                }
            }
            if !slots.is_empty() {
                out.push(play::Clientbound::SetEquipment(SetEquipment {
                    id: e.id,
                    slots,
                }));
            }
            self.equipment.clone_from(&e.equipment);
        }
    }
}

/// The entities one client tracks (`TrackedEntity.seenBy`, from the
/// client's side).
#[derive(Default)]
pub struct Tracker {
    seen: crate::fast_hash::FxHashSet<i32>,
    /// HorizonsX: the tracked entities it is sent as far ones ([`TrackingConfig::far_multiplier`]): its game may
    /// hold an older position of them than the others', so coming near again starts with a position sync.
    far: crate::fast_hash::FxHashSet<i32>,
}

impl Tracker {
    pub fn is_tracking(&self, id: i32) -> bool {
        self.seen.contains(&id)
    }
    pub fn ids(&self) -> Vec<i32> {
        self.seen.iter().copied().collect()
    }
    pub fn clear(&mut self) {
        self.seen.clear();
        self.far.clear();
    }
    /// Stop tracking `id` without a packet: the caller removes it from the
    /// client, and the next tick pairs it again if it is still in range.
    pub fn forget(&mut self, id: i32) -> bool {
        self.far.remove(&id);
        self.seen.remove(&id)
    }
    pub fn len(&self) -> usize {
        self.seen.len()
    }
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

/// Who a level's entities are tracked by this tick: a client's position,
/// view distance and chunks.
pub struct Viewer<'a> {
    pub entity_id: i32,
    /// Its player is in spectator mode: it tracks the other spectators too.
    pub spectator: bool,
    /// It accepted a move since the last tick (ChunkMap.move): every pair
    /// of it is checked again.
    pub moved: bool,
    pub position: [f64; 3],
    pub view_distance: i32,
    pub chunks: &'a crate::fast_hash::FxHashSet<(i32, i32)>,
    pub tracker: &'a mut Tracker,
    /// What the tick sends it, in order.
    pub out: &'a mut Vec<Vec<u8>>,
    /// Where the entities it stops tracking were last sent, when asked (the prediction's replay).
    pub gone: Option<&'a mut Vec<[f64; 3]>>,
}

/// One entity as this tick's range checks read it: where it is
/// horizontally, its id, its tracking range in blocks (scaled by
/// `entity-broadcast-range-percentage`) and the chunk it is in (an index
/// into the tick's chunks).
#[derive(Clone, Copy)]
struct Probe {
    x: f64,
    z: f64,
    id: i32,
    range: i32,
    chunk: u32,
    spectator: bool,
    watching: bool,
    /// Primed TNT: a client sees at most [`tnt_view_limits`]' count of them.
    tnt: bool,
    /// HorizonsX: a player (not an NPC): a client sees at most [`TrackingConfig::max_players_seen`] of them.
    player: bool,
    /// HorizonsX: a player or mob alone (no vehicle, no passengers) with an update interval: its updates to
    /// far clients are thinned ([`TrackingConfig::far_multiplier`]).
    thinned: bool,
}

/// What a TNT field shows each player, from `spigot.yml` (read again every 5 s, no restart; 0 turns one off):
/// `max-tnt-seen-per-player` (default 32: the nearest primed TNT it sees; the ones it already sees stay),
/// `max-explosion-effects-per-tick` (default 4: explosion packets, their particles and sound, a player gets in
/// a tick; past the first one they come as a small puff without the broken blocks' smoke) and
/// `explosion-effect-merge-blocks` (default 8: a blast this close to one already shown that tick shows nothing more). A field of thousands lagged the players' game, not only the server.
pub fn tnt_view_limits() -> (usize, usize, f64) {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, (usize, usize, f64))>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((read, limits)) = *cache
        && read.elapsed() < Duration::from_secs(5)
    {
        return limits;
    }
    let text = std::fs::read_to_string("spigot.yml").unwrap_or_default();
    let number = |key: &str, default: f64| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(key))
            .and_then(|value| value.trim().parse::<f64>().ok())
            .map_or(default, |n| n.max(0.0))
    };
    let limits = (
        number("max-tnt-seen-per-player:", 32.0) as usize,
        number("max-explosion-effects-per-tick:", 4.0) as usize,
        number("explosion-effect-merge-blocks:", 8.0),
    );
    *cache = Some((Instant::now(), limits));
    limits
}

/// `ServerPlayer.broadcastToPlayer` (`Entity`'s is `true`): to a spectator,
/// a player is shown only while its camera is itself; to anyone else, a
/// spectator is hidden.
fn broadcast_to_player(viewer_spectator: bool, p: &Probe) -> bool {
    if viewer_spectator {
        !p.watching
    } else {
        !p.spectator
    }
}

/// TrackedEntity.updatePlayer: within the type's range (capped by the view
/// distance) horizontally, shown to the client (`Entity.broadcastToPlayer`:
/// `ServerPlayer`'s hides a spectator from players who are not spectators;
/// a guest's camera is always its own player, so a spectating client sees
/// every player) and in a chunk the client has. `held` remembers for
/// each of the tick's chunks whether the client has it (0: not looked up
/// yet, 1: no, 2: yes), so each chunk is looked up once per client.
fn sees(v: &Viewer, p: &Probe, chunks: &[(i32, i32)], held: &mut [u8]) -> bool {
    let range = f64::from(p.range.min(v.view_distance * 16));
    let (dx, dz) = (v.position[0] - p.x, v.position[2] - p.z);
    dx * dx + dz * dz <= range * range && broadcast_to_player(v.spectator, p) && {
        let held = &mut held[p.chunk as usize];
        if *held == 0 {
            *held = 1 + u8::from(v.chunks.contains(&chunks[p.chunk as usize]));
        }
        *held == 2
    }
}

/// One level's tracked entities (ChunkMap.entityMap).
#[derive(Default)]
pub struct Entities {
    /// `entity-broadcast-range-percentage` (None: 100), which scales every
    /// type's tracking range (MinecraftServer.getScaledTrackingDistance).
    pub range_percent: Option<i32>,
    states: crate::fast_hash::FxHashMap<i32, ServerEntity>,
    /// The level tick, stamped on every entity still in the level.
    tick: u32,
    /// This tick's entities as the range checks read them, the chunks they
    /// are in (and each one's index), whether the client being checked has
    /// each chunk, and what each client makes of each entity ([`SEES`],
    /// [`TRACKS`]), kept between ticks.
    probes: Vec<Probe>,
    chunks: Vec<(i32, i32)>,
    chunk_index: crate::fast_hash::FxHashMap<(i32, i32), u32>,
    held: Vec<u8>,
    flags: Vec<Vec<u8>>,
    /// Whether a client starts tracking each entity this tick.
    pairs: Vec<bool>,
    /// HorizonsX: whether a client gets each entity as a far one, or comes near it again, this tick: its far
    /// packets are made.
    far_needed: Vec<bool>,
    /// The entities that appeared or moved to another section this tick
    /// (their pairs with every client are checked again), in order and as a
    /// flag per entity.
    moved: Vec<usize>,
    is_moved: Vec<bool>,
    /// One entity's packets and their bodies, kept between entities.
    packets: Vec<play::Clientbound>,
    bodies: Vec<Vec<u8>>,
    /// HorizonsX: the server's tracking settings, read at the start of each tick.
    config: TrackingConfig,
    /// HorizonsX: per entity this tick, whether its far clients get their thinned update (a position sync).
    far_due: Vec<bool>,
    /// HorizonsX: the states the regions' trackers sent from last, back from their threads ([`Entities::park`]):
    /// kept apart, so the server's own tick (for players no region holds) never sends changes from them that
    /// the regions' players then would miss. The next split lends them out again.
    parked: crate::fast_hash::FxHashMap<i32, ServerEntity>,
}

/// The client sees the entity this tick, and already tracked it.
const SEES: u8 = 1;
const TRACKS: u8 = 2;
/// HorizonsX: it tracks a thinned entity from far away ([`TrackingConfig::far_multiplier`]), or comes near one
/// it held as far (its game is sent a position sync first).
const FAR: u8 = 4;
const RESYNC: u8 = 8;

fn encode_all(packets: &[play::Clientbound], bodies: &mut Vec<Vec<u8>>) {
    bodies.clear();
    bodies.extend(packets.iter().map(play::Clientbound::encode));
}

/// HorizonsX: what a client that has an entity as a far one is sent of it this tick: its changes but the moves
/// and head turns, then (when due, or when it comes near again) a position sync to where the near clients
/// were last sent it.
struct Far {
    still: Vec<Vec<u8>>,
    sync: Vec<Vec<u8>>,
}

impl Far {
    fn make(s: &ServerEntity, e: &EntityState, packets: &[play::Clientbound]) -> Self {
        let moves = |p: &&play::Clientbound| {
            matches!(
                p,
                play::Clientbound::MoveEntityPos(_)
                    | play::Clientbound::MoveEntityPosRot(_)
                    | play::Clientbound::MoveEntityRot(_)
                    | play::Clientbound::RotateHead(_)
            )
        };
        let sync = [
            play::Clientbound::EntityPositionSync(EntityPositionSync {
                id: e.id,
                path: PositionPath::Linear(s.base),
                yaw: horizon_protocol::buf::unpack_degrees(s.rotation.1),
                pitch: horizon_protocol::buf::unpack_degrees(s.rotation.0),
                on_ground: e.on_ground,
            }),
            play::Clientbound::RotateHead(RotateHead { id: e.id, head_yaw: s.head }),
        ];
        Self {
            still: packets.iter().filter(|p| !moves(p)).map(play::Clientbound::encode).collect(),
            sync: sync.iter().map(play::Clientbound::encode).collect(),
        }
    }
}

/// One tracked entity's packets this tick to one client (`f` its flags): all its changes, or as a far one,
/// or coming near again ([`Far`]).
fn send_tracked(v: &mut Viewer, f: u8, id: i32, changes: Option<&[Vec<u8>]>, far: Option<&Far>, far_due: bool) {
    match far {
        Some(far) if f & (FAR | RESYNC) != 0 => {
            v.out.extend(far.still.iter().cloned());
            if f & RESYNC != 0 {
                v.out.extend(far.sync.iter().cloned());
                v.tracker.far.remove(&id);
            } else {
                if far_due {
                    v.out.extend(far.sync.iter().cloned());
                }
                v.tracker.far.insert(id);
            }
        }
        _ => {
            if let Some(bodies) = changes {
                v.out.extend(bodies.iter().cloned());
            }
        }
    }
}

impl Entities {
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// HorizonsX's regions (Folia's: a region tracks its own entities for its own players): the states of the
    /// entities last seen in each region's chunks (`region_of` a chunk's region) moved out, one tracker each, at
    /// this tick's count. What no region owns stays.
    pub fn lend(&mut self, regions: usize, region_of: impl Fn((i32, i32)) -> Option<usize>) -> Vec<Entities> {
        let mut out: Vec<Entities> = (0..regions)
            .map(|_| Entities { range_percent: self.range_percent, tick: self.tick, ..Default::default() })
            .collect();
        let chunk = |s: &ServerEntity| (s.section[0], s.section[2]);
        for (id, s) in self.parked.drain() {
            match region_of(chunk(&s)) {
                Some(r) => {
                    out[r].states.insert(id, s);
                }
                None => {
                    self.states.insert(id, s);
                }
            }
        }
        for (id, s) in self.states.extract_if(|_, s| region_of(chunk(s)).is_some()) {
            if let Some(r) = region_of(chunk(&s)) {
                out[r].states.entry(id).or_insert(s);
            }
        }
        out
    }

    /// A region's tracker back ([`Entities::lend`]): its states parked, its count kept (it only goes up).
    pub fn park(&mut self, region: Entities) {
        self.tick = self.tick.max(region.tick);
        self.parked.extend(region.states);
    }

    /// The parked states back among this tracker's own, for a tick of the server's that has players to send to
    /// (no region ran, or some players are in none): theirs are the newer.
    pub fn unpark(&mut self) {
        if !self.parked.is_empty() {
            self.states.extend(self.parked.drain());
        }
    }
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// One tick of ChunkMap.tick for the entities in the level now and the
    /// clients in it: each client first loses the entities that left its
    /// range or the level (one ClientboundRemoveEntitiesPacket), then each
    /// entity's changes are made once and go to every client tracking it,
    /// and the clients that start tracking it are paired with it. A client
    /// never tracks its own entity. `kind` names a type's registry id.
    pub fn tick(
        &mut self,
        entities: &[&EntityState],
        riding: &Riding,
        viewers: &mut [Viewer],
        mut kind: impl FnMut(&str) -> Option<i32>,
    ) {
        self.tick = self.tick.wrapping_add(1);
        let now = self.tick;
        self.probes.clear();
        self.chunks.clear();
        self.chunk_index.clear();
        self.moved.clear();
        self.is_moved.clear();
        self.is_moved.resize(entities.len(), false);
        let config = tracking_config::current();
        self.config = config;
        self.far_due.clear();
        // An entity's range in blocks: its group's (or its own), then the server's percentage.
        let percent = self.range_percent;
        let scaled = move |s: &ServerEntity| {
            let r = config.range(s.group, s.range * 16);
            percent.map_or(r, |p| p * r / 100)
        };
        for (index, e) in entities.iter().enumerate() {
            let mut added = false;
            let s = self.states.entry(e.id).or_insert_with(|| {
                added = true;
                ServerEntity::new(e, kind(&e.kind), riding)
            });
            s.seen = now;
            s.index = index;
            // ChunkMap.addEntity, ChunkMap.tick's section change, and a
            // player going in or out of spectator mode (which ChunkMap.move
            // catches at the player's next move, a second later at most):
            // updatePlayers.
            let section = section_of(e.position);
            if added
                || section != s.section
                || e.spectator != s.spectator
                || e.watching != s.watching
            {
                s.section = section;
                s.spectator = e.spectator;
                s.watching = e.watching;
                self.moved.push(index);
                self.is_moved[index] = true;
            }
            let at = (
                (e.position[0].floor() as i32) >> 4,
                (e.position[2].floor() as i32) >> 4,
            );
            let next = self.chunks.len() as u32;
            let chunk = *self.chunk_index.entry(at).or_insert(next);
            if chunk == next {
                self.chunks.push(at);
            }
            let thinned = config.thins()
                && s.group.is_mob_or_player()
                && s.interval != 0
                && !riding.vehicle.contains_key(&e.id)
                && riding.passengers(e.id).is_empty();
            self.far_due.push(thinned && s.tick.is_multiple_of(s.interval * config.far_multiplier));
            self.probes.push(Probe {
                x: e.position[0],
                z: e.position[2],
                id: e.id,
                range: scaled(s),
                chunk,
                spectator: e.spectator,
                watching: e.watching,
                tnt: e.kind == "minecraft:tnt",
                player: e.kind == "minecraft:player",
                thinned,
            });
        }
        // TrackedEntity.getEffectiveRange: a vehicle is tracked as far as its
        // widest indirect passenger, so a rider never leaves before its mount.
        for (&vehicle, passengers) in &riding.passengers {
            let Some(own) = self.states.get(&vehicle).filter(|s| s.seen == now) else {
                continue;
            };
            let (index, mut range) = (own.index, scaled(own));
            let mut pending = passengers.clone();
            while let Some(id) = pending.pop() {
                if let Some(s) = self.states.get(&id).filter(|s| s.seen == now) {
                    range = range.max(scaled(s));
                }
                pending.extend_from_slice(riding.passengers(id));
            }
            self.probes[index].range = range;
        }
        self.flags.resize_with(viewers.len(), Vec::new);
        self.pairs.clear();
        self.pairs.resize(entities.len(), false);
        self.far_needed.clear();
        self.far_needed.resize(entities.len(), false);
        let look = Look {
            states: &self.states,
            probes: &self.probes,
            chunks: &self.chunks,
            moved: &self.moved,
            is_moved: &self.is_moved,
            now,
            max_tnt: tnt_view_limits().0,
            max_players: config.max_players_seen,
            far_squared: config.thins().then_some(config.far_distance * config.far_distance),
        };
        // HorizonsX: each client's pairs on a core of its own when there are many (and that is
        // measured faster: crate::v2::Run); "a client starts tracking it" is merged after, an or.
        let mut run = (viewers.len() >= PARALLEL_VIEWERS && look.probes.len() >= PARALLEL_PROBES)
            .then(|| {
                let mut run = crate::v2::Run::start(crate::v2::Phase::Track);
                run.units(viewers.len());
                run
            });
        if run.as_ref().is_some_and(crate::v2::Run::parallel) {
            let workers = crate::v2::workers(viewers.len());
            let per = viewers.len().div_ceil(workers);
            let (look, entities_len) = (&look, entities.len());
            // Each part claimed by whichever thread is free, the tick's own among them.
            let items: Vec<_> = viewers.chunks_mut(per).zip(self.flags.chunks_mut(per)).collect();
            let parts: Vec<(Vec<bool>, Vec<bool>)> = crate::v2::map_here_and_pool(
                items,
                |(vs, _)| vs.len(),
                |(vs, fs)| {
                    let (mut held, mut pairs) = (Vec::new(), vec![false; entities_len]);
                    let mut far = vec![false; entities_len];
                    for (v, flags) in vs.iter_mut().zip(fs) {
                        viewer_pairs(look, v, flags, &mut held, &mut pairs, &mut far);
                    }
                    (pairs, far)
                },
            );
            for (part, far) in parts {
                for (all, one) in self.pairs.iter_mut().zip(part) {
                    *all |= one;
                }
                for (all, one) in self.far_needed.iter_mut().zip(far) {
                    *all |= one;
                }
            }
        } else {
            if let Some(run) = run.as_mut() {
                run.fell_back();
            }
            for (v, flags) in viewers.iter_mut().zip(&mut self.flags) {
                viewer_pairs(&look, v, flags, &mut self.held, &mut self.pairs, &mut self.far_needed);
            }
        }
        drop(run);
        self.after_pairs(entities, riding, viewers);
    }

    /// The rest of [`Self::tick`], after every client's pairs: each entity's changes made once and
    /// sent to every client tracking it, and the clients that start tracking it paired with it.
    /// HorizonsX: with many clients and entities, in two steps on every core (each entity's packets, then
    /// each client's list in entity order: the same bytes in the same order as one loop over both).
    fn after_pairs(&mut self, entities: &[&EntityState], riding: &Riding, viewers: &mut [Viewer]) {
        #[cfg(test)]
        if SERIAL_AFTER_PAIRS.with(|s| s.get()) {
            return self.after_pairs_serial(entities, riding, viewers);
        }
        // HorizonsX: also with few clients when the entities are many (a field of primed TNT): their packets are
        // the work then, and they are made on every core the same way.
        let many = viewers.len() >= PARALLEL_VIEWERS || (!viewers.is_empty() && entities.len() >= PARALLEL_ENTITIES);
        if !many || entities.len() < PARALLEL_PROBES {
            return self.after_pairs_serial(entities, riding, viewers);
        }
        let now = self.tick;
        let (pairs, far_needed, far_due) = (&self.pairs, &self.far_needed, &self.far_due);
        // Each entity's changes since it was last sent, and its pairing packets when a client starts to
        // track it, encoded once.
        let states: Vec<&mut ServerEntity> = self.states.values_mut().filter(|s| s.seen == now).collect();
        type Bodies = Option<Vec<Vec<u8>>>;
        let made: Vec<(usize, Bodies, Bodies, Option<Far>)> = crate::v2::map_batches(states, 64, |s| {
            let e = entities[s.index];
            let mut packets = Vec::new();
            if !s.added {
                s.send_changes(e, riding, &mut packets);
            }
            s.added = false;
            let changes = (!packets.is_empty()).then(|| packets.iter().map(play::Clientbound::encode).collect());
            let far = far_needed[s.index].then(|| Far::make(s, e, &packets));
            let pairing = match s.kind {
                Some(kind) if pairs[s.index] => {
                    let mut pairing = Vec::new();
                    s.pairing(e, kind, riding, &mut pairing);
                    Some(pairing.iter().map(play::Clientbound::encode).collect())
                }
                _ => None,
            };
            (s.index, changes, pairing, far)
        });
        let mut by_index: Vec<(Bodies, Bodies, Option<Far>)> = (0..entities.len()).map(|_| (None, None, None)).collect();
        for (index, changes, pairing, far) in made {
            by_index[index] = (changes, pairing, far);
        }
        // Each client's packets, entity by entity.
        let by_index = &by_index;
        let items: Vec<(&mut Viewer, &Vec<u8>)> = viewers.iter_mut().zip(self.flags.iter()).collect();
        crate::v2::map_batches(items, 4, |(v, flags)| {
            for (index, (changes, pairing, far)) in by_index.iter().enumerate() {
                let f = flags.get(index).copied().unwrap_or(0);
                if f & TRACKS != 0 {
                    send_tracked(v, f, entities[index].id, changes.as_deref(), far.as_ref(), far_due[index]);
                } else if f & SEES != 0
                    && let Some(bodies) = pairing
                {
                    v.out.extend(bodies.iter().cloned());
                    v.tracker.seen.insert(entities[index].id);
                }
            }
        });
        self.states.retain(|_, s| s.seen == now);
    }

    /// [`Self::after_pairs`] in one loop, for few clients or entities.
    fn after_pairs_serial(&mut self, entities: &[&EntityState], riding: &Riding, viewers: &mut [Viewer]) {
        for (index, e) in entities.iter().enumerate() {
            let Some(s) = self.states.get_mut(&e.id) else {
                continue;
            };
            self.packets.clear();
            if !s.added {
                s.send_changes(e, riding, &mut self.packets);
            }
            s.added = false;
            // Nothing to send: no changes, nobody starts tracking it, and no client has it as a far one.
            if self.packets.is_empty() && !self.pairs[index] && !self.far_needed[index] {
                continue;
            }
            let mut encoded = false;
            let mut pairing: Option<Vec<Vec<u8>>> = None;
            let far = self.far_needed[index].then(|| Far::make(s, e, &self.packets));
            for (v, flags) in viewers.iter_mut().zip(&self.flags) {
                let f = flags[index];
                if f & TRACKS != 0 {
                    if f & (FAR | RESYNC) == 0 && self.packets.is_empty() {
                        continue;
                    }
                    if !encoded && !self.packets.is_empty() {
                        encode_all(&self.packets, &mut self.bodies);
                        encoded = true;
                    }
                    let changes = encoded.then_some(self.bodies.as_slice());
                    send_tracked(v, f, e.id, changes, far.as_ref(), self.far_due[index]);
                } else if f & SEES != 0
                    && let Some(kind) = s.kind
                {
                    let bodies = pairing.get_or_insert_with(|| {
                        let mut packets = Vec::new();
                        s.pairing(e, kind, riding, &mut packets);
                        packets.iter().map(play::Clientbound::encode).collect()
                    });
                    v.out.extend(bodies.iter().cloned());
                    v.tracker.seen.insert(e.id);
                }
            }
        }
        let now = self.tick;
        self.states.retain(|_, s| s.seen == now);
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: `after_pairs` in one loop, to compare the cores' version against.
    static SERIAL_AFTER_PAIRS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Fewer clients or entities than these: their pairs are checked on the game thread.
const PARALLEL_VIEWERS: usize = 16;
const PARALLEL_PROBES: usize = 256;
/// Entities enough for `after_pairs` on every core whatever the clients.
const PARALLEL_ENTITIES: usize = 1024;

/// What every client's pair checks read this tick.
struct Look<'a> {
    states: &'a crate::fast_hash::FxHashMap<i32, ServerEntity>,
    probes: &'a [Probe],
    chunks: &'a [(i32, i32)],
    moved: &'a [usize],
    is_moved: &'a [bool],
    now: u32,
    /// The most primed TNT and players a client is sent (0: no limit).
    max_tnt: usize,
    max_players: usize,
    /// HorizonsX: past this many blocks (squared) a thinned entity is a far one (None: none is).
    far_squared: Option<f64>,
}

/// One client's part of `ChunkMap.tick`: what it makes of each entity ([`SEES`], [`TRACKS`]) into
/// `flags`, the entities it stops tracking removed (one ClientboundRemoveEntitiesPacket), and in
/// `pairs` the entities it starts tracking. `held` is scratch.
fn viewer_pairs(
    look: &Look,
    v: &mut Viewer,
    flags: &mut Vec<u8>,
    held: &mut Vec<u8>,
    pairs: &mut [bool],
    far_needed: &mut [bool],
) {
    let now = look.now;
    {
            flags.clear();
            held.clear();
            held.resize(look.chunks.len(), 0);
            let chunks = look.chunks;
            let mut kept = 0;
            // A client that moved (ChunkMap.move), or whose own entity
            // appeared or changed section (ChunkMap.tick's movedPlayers), has
            // every pair checked again.
            let all = v.moved
                || look
                    .states
                    .get(&v.entity_id)
                    .is_none_or(|s| s.seen != now || look.is_moved[s.index]);
            if all {
                flags.extend(look.probes.iter().zip(pairs.iter_mut()).map(|(p, pairs)| {
                    if p.id == v.entity_id || !sees(v, p, chunks, held) {
                        0
                    } else if v.tracker.seen.contains(&p.id) {
                        kept += 1;
                        SEES | TRACKS
                    } else {
                        *pairs = true;
                        SEES
                    }
                }));
            } else if v.tracker.seen.len() * 4 > look.probes.len() {
                // Otherwise its pairs stay as they were, but for the entities
                // that appeared or changed section; a client tracking much
                // of the level goes through it in order.
                let is_moved = look.is_moved;
                flags.extend(look.probes.iter().zip(pairs.iter_mut()).zip(is_moved).map(
                    |((p, pairs), &moved)| {
                        let tracks = v.tracker.seen.contains(&p.id);
                        if p.id == v.entity_id || (moved && !sees(v, p, chunks, held)) {
                            0
                        } else if tracks {
                            kept += 1;
                            SEES | TRACKS
                        } else if moved {
                            *pairs = true;
                            SEES
                        } else {
                            0
                        }
                    },
                ));
            } else {
                // One tracking little of it, from what it tracks.
                flags.resize(look.probes.len(), 0);
                for id in &v.tracker.seen {
                    if let Some(s) = look.states.get(id).filter(|s| s.seen == now) {
                        flags[s.index] = SEES | TRACKS;
                        kept += 1;
                    }
                }
                for &index in look.moved {
                    let p = &look.probes[index];
                    let tracks = flags[index] & TRACKS != 0;
                    if p.id != v.entity_id && sees(v, p, chunks, held) {
                        if !tracks {
                            flags[index] = SEES;
                            pairs[index] = true;
                        }
                    } else if tracks {
                        flags[index] = 0;
                        kept -= 1;
                    }
                }
            }
            // A TNT field: the client sees at most the cap's count of primed TNT, the ones it already tracks
            // first (no flicker), then the nearest. The rest stay out of its game until a slot frees.
            // HorizonsX: likewise a crowd of players: the nearest, one already sent counting 8 blocks nearer (a
            // crowd's edge doesn't flicker, and a player walking up still takes a far one's place).
            let (max_tnt, max_players) = (look.max_tnt, look.max_players);
            if max_tnt > 0 || max_players > 0 {
                let (mut tnt, mut players): (Vec<(bool, f64, usize)>, Vec<(bool, f64, usize)>) = Default::default();
                for (i, f) in flags.iter().enumerate() {
                    let p = &look.probes[i];
                    if f & SEES == 0 || !(p.tnt && max_tnt > 0 || p.player && max_players > 0) {
                        continue;
                    }
                    let (dx, dz) = (v.position[0] - p.x, v.position[2] - p.z);
                    let one = (f & TRACKS != 0, dx * dx + dz * dz, i);
                    if p.tnt { tnt.push(one) } else { players.push(one) }
                }
                let mut hide = |list: &[(bool, f64, usize)]| {
                    for &(tracked, _, i) in list {
                        if tracked {
                            kept -= 1;
                        }
                        flags[i] = 0;
                    }
                };
                if max_tnt > 0 && tnt.len() > max_tnt {
                    tnt.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
                    hide(&tnt[max_tnt..]);
                }
                if max_players > 0 && players.len() > max_players {
                    let key = |p: &(bool, f64, usize)| p.1.sqrt() - if p.0 { 8.0 } else { 0.0 };
                    players.sort_by(|a, b| key(a).total_cmp(&key(b)));
                    hide(&players[max_players..]);
                }
            }
            // Usually the client still sees all it tracks.
            let mut removed = Vec::new();
            let Tracker { seen, far } = &mut *v.tracker;
            if kept != seen.len() {
                let states = look.states;
                seen.retain(|id| match states.get(id) {
                    Some(s) if s.seen == now && flags[s.index] & TRACKS != 0 => true,
                    other => {
                        if let (Some(gone), Some(s)) = (v.gone.as_deref_mut(), other) {
                            gone.push(s.base);
                        }
                        far.remove(id);
                        removed.push(*id);
                        false
                    }
                });
            }
            // HorizonsX: the players and mobs it is sent as far ones this tick, and the ones it comes near again
            // (or no longer thinned) that it holds an older position of.
            if look.far_squared.is_some() || !far.is_empty() {
                let far_squared = look.far_squared.unwrap_or(f64::INFINITY);
                for id in seen.iter() {
                    let Some(s) = look.states.get(id) else { continue };
                    let (i, p) = (s.index, &look.probes[s.index]);
                    let (dx, dz) = (v.position[0] - p.x, v.position[2] - p.z);
                    if p.thinned && dx * dx + dz * dz > far_squared {
                        flags[i] |= FAR;
                        far_needed[i] = true;
                    } else if far.contains(id) {
                        flags[i] |= RESYNC;
                        far_needed[i] = true;
                    }
                }
            }
            if !removed.is_empty() {
                v.out.push(
                    play::Clientbound::RemoveEntities(play::RemoveEntities { ids: removed })
                        .encode(),
                );
            }
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HorizonsX: the entities' packets and the clients' lists made on every core give each client the
    /// same bytes in the same order as the one loop: twenty clients, three hundred mobs moving, six ticks.
    #[test]
    fn after_pairs_on_every_core_match_the_serial_loop() {
        fn run(serial: bool) -> Vec<Vec<Vec<u8>>> {
            SERIAL_AFTER_PAIRS.with(|s| s.set(serial));
            let chunks: crate::fast_hash::FxHashSet<(i32, i32)> =
                (-8..8).flat_map(|x| (-8..8).map(move |z| (x, z))).collect();
            let mut level = Entities::default();
            let mut trackers: Vec<Tracker> = (0..20).map(|_| Tracker::default()).collect();
            let mut outs: Vec<Vec<Vec<u8>>> = vec![Vec::new(); 20];
            let mut all = Vec::new();
            for tick in 0..6 {
                let mobs: Vec<EntityState> = (0..300)
                    .map(|i| {
                        let mut e = state(f64::from(i % 40) * 3.0 - 60.0 + f64::from(tick) * 0.7 * f64::from(i % 3));
                        e.id = 100 + i;
                        e.position[2] = f64::from(i / 40) * 9.0 - 30.0;
                        e.yaw = (tick * 11 + i) as f32;
                        e
                    })
                    .collect();
                let refs: Vec<&EntityState> = mobs.iter().collect();
                for out in &mut outs {
                    out.clear();
                }
                let mut viewers: Vec<Viewer> = trackers
                    .iter_mut()
                    .zip(outs.iter_mut())
                    .enumerate()
                    .map(|(k, (tracker, out))| Viewer {
                        entity_id: 1 + k as i32,
                        spectator: false,
                        moved: tick == 0,
                        position: [f64::from(k as i32 % 5) * 20.0 - 40.0, 64.0, f64::from(k as i32 / 5) * 20.0 - 40.0],
                        view_distance: 10,
                        chunks: &chunks,
                        tracker,
                        out,
                        gone: None,
                    })
                    .collect();
                level.tick(&refs, &Riding::default(), &mut viewers, |_| Some(100));
                drop(viewers);
                all.extend(outs.iter().cloned());
            }
            SERIAL_AFTER_PAIRS.with(|s| s.set(false));
            all
        }
        let serial = run(true);
        let parallel = run(false);
        assert!(serial.iter().map(Vec::len).sum::<usize>() > 1000, "the clients were sent their mobs");
        assert_eq!(serial, parallel);
        // HorizonsX: the same with far mobs thinned for some clients.
        with_config(TrackingConfig { far_distance: 30.0, far_multiplier: 2, ..TrackingConfig::vanilla() });
        let serial = run(true);
        let parallel = run(false);
        with_config(TrackingConfig::vanilla());
        assert_eq!(serial, parallel);
    }

    fn with_config(c: TrackingConfig) {
        tracking_config::TEST_CONFIG.with(|t| t.set(c));
    }

    fn decoded(out: &[Vec<u8>]) -> Vec<play::Clientbound> {
        out.iter().map(|p| play::Clientbound::decode(i32::from(p[0]), &p[1..]).unwrap()).collect()
    }

    /// HorizonsX: a crowd: each client is sent only its nearest players; one it sees keeps its place until
    /// another is 8 blocks nearer.
    #[test]
    fn a_crowd_shows_each_client_its_nearest_players() {
        with_config(TrackingConfig { max_players_seen: 3, ..TrackingConfig::vanilla() });
        let mut level = Entities::default();
        let mut c = Clients::new();
        c.moved = [true; 2];
        let mut players: Vec<EntityState> = (0..6)
            .map(|i| {
                let mut p = state(f64::from(i) * 4.0 + 4.0);
                (p.id, p.kind) = (10 + i, "minecraft:player".into());
                p
            })
            .collect();
        let mut zombie = state(30.0);
        zombie.id = 50;
        let mut tick = |c: &mut Clients, players: &[EntityState]| {
            let mut refs: Vec<&EntityState> = players.iter().collect();
            refs.push(&zombie);
            c.tick(&mut level, &refs);
            let mut ids = c.trackers[0].ids();
            ids.sort_unstable();
            ids
        };
        // Players at 4, 8, 12, 16, 20 and 24 blocks; the zombie isn't limited.
        assert_eq!(tick(&mut c, &players), [10, 11, 12, 50]);
        // The fourth comes to 10 blocks: not 8 nearer than the third (12), so hidden still.
        players[3].position[0] = 10.0;
        assert_eq!(tick(&mut c, &players), [10, 11, 12, 50]);
        // To 1 block: it takes the third's place.
        players[3].position[0] = 1.0;
        assert_eq!(tick(&mut c, &players), [10, 11, 13, 50]);
        with_config(TrackingConfig::vanilla());
    }

    /// HorizonsX: a client far from a mob is sent no moves of it, only a position sync every other update;
    /// coming near, it is synced to where the near clients were last sent it, then gets their moves.
    #[test]
    fn far_mobs_are_sent_less_often_and_synced_when_near_again() {
        use play::Clientbound as C;
        with_config(TrackingConfig { far_distance: 32.0, far_multiplier: 2, ..TrackingConfig::vanilla() });
        let mut level = Entities::default();
        let mut c = Clients::new();
        c.positions[1] = [50.0, 64.0, 0.0];
        c.moved = [true; 2];
        let mut z = state(50.0);
        let (mut far_moves, mut far_syncs, mut near_moves) = (0, 0, 0);
        let mut last_sent = z.position;
        for t in 0..24 {
            z.position[0] = 50.0 + f64::from(t) * 0.3;
            c.tick(&mut level, &[&z]);
            for p in decoded(&c.out[0]) {
                match p {
                    C::MoveEntityPos(_) | C::MoveEntityPosRot(_) | C::MoveEntityRot(_) => far_moves += 1,
                    C::EntityPositionSync(_) => far_syncs += 1,
                    _ => {}
                }
            }
            for p in decoded(&c.out[1]) {
                if matches!(p, C::MoveEntityPos(_) | C::MoveEntityPosRot(_)) {
                    near_moves += 1;
                    last_sent = z.position;
                }
            }
        }
        assert_eq!((far_moves, near_moves, far_syncs), (0, 8, 4));
        // The first client walks up to it.
        c.positions[0] = [45.0, 64.0, 0.0];
        c.tick(&mut level, &[&z]);
        if decoded(&c.out[1]).iter().any(|p| matches!(p, C::MoveEntityPos(_) | C::MoveEntityPosRot(_))) {
            last_sent = z.position;
        }
        let synced = decoded(&c.out[0]).into_iter().find_map(|p| match p {
            C::EntityPositionSync(s) => Some(s.path.end()),
            _ => None,
        });
        assert_eq!(synced, Some(last_sent));
        assert!(!c.trackers[0].far.contains(&7));
        for t in 0..6 {
            z.position[0] = 60.0 + f64::from(t) * 0.3;
            c.tick(&mut level, &[&z]);
            assert_eq!(c.out[0], c.out[1]);
        }
        with_config(TrackingConfig::vanilla());
    }

    fn state(x: f64) -> EntityState {
        EntityState {
            id: 7,
            uuid: 1,
            kind: "minecraft:zombie".into(),
            position: [x, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            head_yaw: 0.0,
            on_ground: true,
            velocity: [0.0; 3],
            data: 0,
            meta: Vec::new(),
            defaults: Vec::new(),
            equipment: Vec::new(),
            attributes: Vec::new(),
            precise: false,
            needs_sync: false,
            leash: None,
            spectator: false,
            watching: false,
        }
    }

    #[test]
    fn states_are_written_over_last_ticks_in_place() {
        let mut states = States::default();
        let e = states.add(1, 1, "minecraft:zombie");
        e.position = [1.0, 2.0, 3.0];
        e.meta.push((9, serializer::FLOAT, Value::Float(20.0)));
        e.equipment.push((0, None));
        let (kind, meta) = (e.kind.as_ptr(), e.meta.as_ptr());
        states.push(state(2.0));
        states.clear();
        let e = states.add(3, 3, "minecraft:husk");
        // Nothing of the last tick's state is left, but its allocations are.
        let fresh = EntityState {
            id: 3,
            uuid: 3,
            kind: "minecraft:husk".into(),
            ..EntityState::default()
        };
        assert_eq!(*e, fresh);
        assert_eq!((e.kind.as_ptr(), e.meta.as_ptr()), (kind, meta));
        assert_eq!(states.as_slice(), std::slice::from_ref(&fresh));
        states.extend([state(5.0), state(6.0)]);
        assert_eq!(states.as_slice(), [fresh, state(5.0), state(6.0)]);
    }

    /// The entity as added this tick, then its updates (one a tick).
    fn added(e: &EntityState) -> ServerEntity {
        ServerEntity::new(e, Some(100), &Riding::default())
    }
    fn changes(s: &mut ServerEntity, e: &EntityState) -> Vec<play::Clientbound> {
        let mut out = Vec::new();
        s.send_changes(e, &Riding::default(), &mut out);
        out
    }

    #[test]
    fn furnace_cart_sends_unpowered_metadata_when_fuel_runs_out() {
        let mut powered = state(0.0);
        powered.kind = "minecraft:furnace_minecart".into();
        powered
            .defaults
            .push((13, serializer::BOOLEAN, Value::Bool(false)));
        powered
            .meta
            .push((13, serializer::BOOLEAN, Value::Bool(true)));
        let mut tracker = added(&powered);
        let mut empty = powered.clone();
        empty.meta.clear();
        let mut packets = Vec::new();
        tracker.send_dirty(&empty, true, &mut packets);
        assert!(packets.iter().any(|packet| matches!(packet,
            play::Clientbound::SetEntityData(data)
                if data.values.contains(&(13, serializer::BOOLEAN, Value::Bool(false)))
        )));
    }

    #[test]
    fn moves_go_out_every_update_interval() {
        let mut s = added(&state(0.0));
        let mut spawn = Vec::new();
        s.pairing(&state(0.0), 100, &Riding::default(), &mut spawn);
        assert!(matches!(spawn[..], [play::Clientbound::AddEntity(_)]));
        // Tick 0 is due and a multiple of 60: the position goes out unchanged.
        assert!(matches!(
            changes(&mut s, &state(0.0))[..],
            [play::Clientbound::MoveEntityPos(_)]
        ));
        // A zombie updates every third tick.
        assert!(changes(&mut s, &state(0.5)).is_empty());
        assert!(changes(&mut s, &state(0.5)).is_empty());
        assert!(matches!(
            changes(&mut s, &state(0.5))[..],
            [play::Clientbound::MoveEntityPos(_)]
        ));
        for _ in 0..2 {
            changes(&mut s, &state(40.0));
        }
        let jump = changes(&mut s, &state(40.0));
        assert!(matches!(jump[0], play::Clientbound::EntityPositionSync(_)));
    }

    #[test]
    fn a_knocked_back_mob_is_sent_at_once() {
        let mut s = added(&state(0.0));
        changes(&mut s, &state(0.0));
        // Not due (tick 1 of 3), but LivingEntity.knockback set needsSync:
        // its move and its new motion go out now.
        let mut knocked = state(0.3);
        knocked.velocity = [0.2, 0.3, 0.0];
        knocked.needs_sync = true;
        let out = changes(&mut s, &knocked);
        assert!(
            matches!(
                out[..],
                [
                    play::Clientbound::SetEntityMotion(_),
                    play::Clientbound::MoveEntityPos(_)
                ]
            ),
            "{out:?}"
        );
        // Then every third tick again.
        assert!(changes(&mut s, &state(0.5)).is_empty());
        assert!(matches!(
            changes(&mut s, &state(0.6))[..],
            [
                play::Clientbound::SetEntityMotion(_),
                play::Clientbound::MoveEntityPos(_)
            ]
        ));
    }

    #[test]
    fn fireballs_bundle_their_motion_with_their_power() {
        let mut e = state(0.0);
        e.kind = "minecraft:fireball".into();
        e.on_ground = false;
        let mut s = added(&e);
        e.velocity = [0.0, 0.0, 0.5];
        let out = changes(&mut s, &e);
        assert!(matches!(
            out[..],
            [
                play::Clientbound::BundleDelimiter(_),
                play::Clientbound::SetEntityMotion(_),
                play::Clientbound::ProjectilePower(play::ProjectilePower { power, .. }),
                play::Clientbound::BundleDelimiter(_),
                play::Clientbound::MoveEntityPos(_),
            ] if power == 0.1
        ));
    }

    #[test]
    fn arrows_send_position_and_rotation_every_update() {
        let mut e = state(0.0);
        e.kind = "minecraft:arrow".into();
        let mut s = added(&e);
        assert!(matches!(
            changes(&mut s, &e)[..],
            [play::Clientbound::MoveEntityPosRot(_)]
        ));
    }

    #[test]
    fn precise_entities_sync_their_position() {
        let mut e = state(0.0);
        e.kind = "minecraft:tnt".into();
        e.precise = true;
        let mut s = added(&e);
        assert!(matches!(
            changes(&mut s, &e)[..],
            [play::Clientbound::EntityPositionSync(_)]
        ));
    }

    #[test]
    fn landing_sends_a_position_sync() {
        let mut e = state(0.0);
        let mut s = added(&e);
        changes(&mut s, &e);
        e.on_ground = false;
        changes(&mut s, &e);
        changes(&mut s, &e);
        assert!(matches!(
            changes(&mut s, &e)[..],
            [play::Clientbound::EntityPositionSync(_)]
        ));
    }

    #[test]
    fn attributes_pair_whole_then_send_the_changed_instances_when_due() {
        let speed = |base| AttributeSnapshot {
            attribute: 26,
            base,
            modifiers: Vec::new(),
        };
        let armor = AttributeSnapshot {
            attribute: 1,
            base: 0.0,
            modifiers: Vec::new(),
        };
        let mut e = state(0.0);
        e.attributes = vec![speed(0.23), armor];
        let mut s = added(&e);
        let mut spawn = Vec::new();
        s.pairing(&e, 100, &Riding::default(), &mut spawn);
        let Some(play::Clientbound::UpdateAttributes(p)) = spawn.get(1) else {
            panic!("expected the attributes after the add");
        };
        assert_eq!(p.values, e.attributes);
        changes(&mut s, &e);
        e.attributes[0] = speed(0.3);
        // Not due (a zombie's interval is 3): sendDirtyEntityData waits for the update.
        assert!(changes(&mut s, &e).is_empty());
        assert!(changes(&mut s, &e).is_empty());
        let out = changes(&mut s, &e);
        let Some(play::Clientbound::UpdateAttributes(p)) = out
            .iter()
            .find(|p| matches!(p, play::Clientbound::UpdateAttributes(_)))
        else {
            panic!("expected the changed attribute");
        };
        assert_eq!(p.values, vec![speed(0.3)]);
    }

    #[test]
    fn cleared_flags_and_pose_go_back_to_their_default() {
        let mut e = state(0.0);
        let mut s = added(&e);
        changes(&mut s, &e);
        // A crouching player: the crouch flag and the crouching pose.
        e.meta = vec![
            (0, serializer::BYTE, Value::Byte(2)),
            (6, serializer::POSE, Value::Id(5)),
        ];
        changes(&mut s, &e);
        e.meta.clear();
        let data = changes(&mut s, &e)
            .into_iter()
            .find_map(|p| match p {
                play::Clientbound::SetEntityData(d) => Some(d),
                _ => None,
            })
            .expect("standing up is sent");
        assert_eq!(
            data.values,
            vec![
                (0, serializer::BYTE, Value::Byte(0)),
                (6, serializer::POSE, Value::Id(0)),
            ]
        );
        assert!(changes(&mut s, &e).is_empty());
    }

    #[test]
    fn an_emptied_item_frame_sends_its_empty_item_and_rotation() {
        let mut e = state(0.0);
        e.kind = "minecraft:item_frame".into();
        let mut s = added(&e);
        changes(&mut s, &e);
        e.meta = vec![
            (
                9,
                serializer::ITEM_STACK,
                Value::Item(Some(Stack::new(5, 1))),
            ),
            (10, serializer::INT, Value::Int(3)),
        ];
        changes(&mut s, &e);
        // Punched out (ItemFrame.removeFramedMap, setItem(EMPTY)): both back to the default.
        e.meta.clear();
        let data = changes(&mut s, &e)
            .into_iter()
            .find_map(|p| match p {
                play::Clientbound::SetEntityData(d) => Some(d),
                _ => None,
            })
            .expect("the empty frame is sent");
        assert_eq!(
            data.values,
            vec![
                (9, serializer::ITEM_STACK, Value::Item(None)),
                (10, serializer::INT, Value::Int(0)),
            ]
        );
    }

    #[test]
    fn a_cleared_crystal_beam_is_sent() {
        let mut e = state(0.0);
        e.kind = "minecraft:end_crystal".into();
        e.defaults = vec![(8, serializer::OPTIONAL_BLOCK_POS, Value::OptPos(None))];
        e.meta = vec![(
            8,
            serializer::OPTIONAL_BLOCK_POS,
            Value::OptPos(Some([0, 128, 0])),
        )];
        let mut s = added(&e);
        e.meta.clear();
        let mut packets = Vec::new();
        s.send_dirty(&e, true, &mut packets);
        let sent: Vec<_> = packets
            .iter()
            .filter_map(|p| match p {
                play::Clientbound::SetEntityData(d) => Some(d.values.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(
            sent,
            vec![(8, serializer::OPTIONAL_BLOCK_POS, Value::OptPos(None))]
        );
    }

    #[test]
    fn a_value_back_at_a_listed_default_is_sent() {
        let mut e = state(0.0);
        e.defaults = vec![(16, serializer::BOOLEAN, Value::Bool(false))];
        // A calf: paired with its baby flag, then grown up.
        e.meta = vec![(16, serializer::BOOLEAN, Value::Bool(true))];
        let mut s = added(&e);
        changes(&mut s, &e);
        e.meta.clear();
        let data = changes(&mut s, &e)
            .into_iter()
            .find_map(|p| match p {
                play::Clientbound::SetEntityData(d) => Some(d),
                _ => None,
            })
            .expect("growing up is sent");
        assert_eq!(
            data.values,
            vec![(16, serializer::BOOLEAN, Value::Bool(false))]
        );
        assert!(changes(&mut s, &e).is_empty());
    }

    #[test]
    fn metadata_and_equipment_send_only_the_changes() {
        let mut e = state(0.0);
        let mut s = added(&e);
        changes(&mut s, &e);
        e.meta.push((0, 0, Value::Byte(1)));
        e.equipment.push((0, Some(Stack::new(5, 1))));
        let out = changes(&mut s, &e);
        assert!(
            out.iter()
                .any(|p| matches!(p, play::Clientbound::SetEntityData(_)))
        );
        assert!(
            out.iter()
                .any(|p| matches!(p, play::Clientbound::SetEquipment(_)))
        );
        assert!(changes(&mut s, &e).is_empty());
        e.equipment.clear();
        let out = changes(&mut s, &e);
        let Some(play::Clientbound::SetEquipment(eq)) = out.first() else {
            panic!("expected equipment");
        };
        assert_eq!(eq.slots, vec![(0, None)]);
    }

    #[test]
    fn a_lead_tied_and_untied_reaches_the_clients() {
        let link = |out: &[play::Clientbound]| {
            out.iter().find_map(|p| match p {
                play::Clientbound::SetEntityLink(l) => Some((l.source, l.dest)),
                _ => None,
            })
        };
        let mut e = state(0.0);
        let mut s = added(&e);
        changes(&mut s, &e);
        e.leash = Some(7);
        assert_eq!(link(&changes(&mut s, &e)), Some((e.id, 7)));
        assert_eq!(link(&changes(&mut s, &e)), None);
        e.leash = None;
        assert_eq!(link(&changes(&mut s, &e)), Some((e.id, 0)));
    }

    /// Two clients at the origin; the chunks they hold.
    struct Clients {
        chunks: crate::fast_hash::FxHashSet<(i32, i32)>,
        trackers: [Tracker; 2],
        out: [Vec<Vec<u8>>; 2],
        positions: [[f64; 3]; 2],
        moved: [bool; 2],
        spectators: [bool; 2],
        /// The first client's entities gone, where they were last sent.
        gone: Vec<[f64; 3]>,
    }

    impl Clients {
        fn new() -> Self {
            Self {
                chunks: (-4..4).flat_map(|x| (-4..4).map(move |z| (x, z))).collect(),
                trackers: Default::default(),
                out: Default::default(),
                positions: [[0.0, 64.0, 0.0]; 2],
                moved: [false; 2],
                spectators: [false; 2],
                gone: Vec::new(),
            }
        }
        fn tick(&mut self, level: &mut Entities, entities: &[&EntityState]) {
            for out in &mut self.out {
                out.clear();
            }
            let [t0, t1] = &mut self.trackers;
            let [o0, o1] = &mut self.out;
            let mut viewers = [
                Viewer {
                    entity_id: 1,
                    spectator: self.spectators[0],
                    moved: self.moved[0],
                    position: self.positions[0],
                    view_distance: 10,
                    chunks: &self.chunks,
                    tracker: t0,
                    out: o0,
                    gone: Some(&mut self.gone),
                },
                Viewer {
                    entity_id: 2,
                    spectator: self.spectators[1],
                    moved: self.moved[1],
                    position: self.positions[1],
                    view_distance: 10,
                    chunks: &self.chunks,
                    tracker: t1,
                    out: o1,
                    gone: None,
                },
            ];
            level.tick(entities, &Riding::default(), &mut viewers, |_| Some(100));
        }
    }

    fn body(p: play::Clientbound) -> Vec<u8> {
        p.encode()
    }

    /// ChunkMap checks a pair again only when one side moves: a zombie
    /// walking into range within its section isn't paired with a client
    /// standing still until the client moves, and one walking out stays
    /// tracked until it changes section.
    #[test]
    fn pairs_are_checked_again_when_a_side_moves() {
        let mut level = Entities::default();
        let mut clients = Clients::new();
        clients.positions[0] = [-108.0, 64.0, 0.0];
        let mut me = state(-108.0);
        (me.id, me.kind) = (1, "minecraft:player".into());
        // 128.5 blocks away: past a zombie's 8 chunks.
        let tracks = |clients: &Clients| clients.trackers[0].is_tracking(7);
        clients.tick(&mut level, &[&me, &state(20.5)]);
        assert!(!tracks(&clients));
        // 127.5 blocks away, in the same section: not checked again.
        clients.tick(&mut level, &[&me, &state(19.5)]);
        assert!(!tracks(&clients));
        // The client's move checks every pair of it.
        clients.moved[0] = true;
        clients.tick(&mut level, &[&me, &state(19.5)]);
        assert!(tracks(&clients));
        clients.moved[0] = false;
        clients.tick(&mut level, &[&me, &state(20.5)]);
        assert!(tracks(&clients));
        // Into the next section, out of range: no longer tracked.
        clients.tick(&mut level, &[&me, &state(40.0)]);
        assert!(!tracks(&clients));
        // The other client (at the origin, its entity not in the level)
        // checks every pair each tick.
        assert!(clients.trackers[1].is_tracking(7));
    }

    #[test]
    fn every_client_gets_the_same_changes_and_late_ones_are_paired() {
        let mut level = Entities::default();
        let mut c = Clients::new();
        // The second client is out of range at first.
        c.positions[1] = [500.0, 64.0, 0.0];
        let mut e = state(0.0);
        c.tick(&mut level, &[&e]);
        assert!(c.trackers[0].is_tracking(7) && !c.trackers[1].is_tracking(7));
        assert!(c.out[1].is_empty());
        // Tick 0's forced position goes to the client tracking it.
        c.tick(&mut level, &[&e]);
        assert_eq!(c.out[0].len(), 1);
        // The second client comes in range: paired with the entity as it is,
        // then sent the same changes as the first.
        c.positions[1] = [0.0, 64.0, 0.0];
        e.position[0] = 1.0;
        c.tick(&mut level, &[&e]);
        assert!(c.trackers[1].is_tracking(7));
        assert!(c.out[0].is_empty());
        assert_eq!(c.out[1].len(), 1);
        c.tick(&mut level, &[&e]);
        e.position[0] = 2.0;
        c.tick(&mut level, &[&e]);
        assert!(!c.out[0].is_empty());
        assert_eq!(c.out[0], c.out[1]);
    }

    #[test]
    fn clients_lose_what_leaves_range_or_the_level_and_never_track_themselves() {
        let mut level = Entities::default();
        let mut c = Clients::new();
        let mut me = state(0.0);
        me.id = 1;
        me.kind = "minecraft:player".into();
        let mut e = state(0.0);
        c.tick(&mut level, &[&me, &e]);
        assert!(!c.trackers[0].is_tracking(1) && c.trackers[1].is_tracking(1));
        assert!(c.trackers[0].is_tracking(7));
        // Out of the zombie's 8 chunks: the client last had it where it started.
        let start = e.position;
        e.position[0] = 200.0;
        c.tick(&mut level, &[&me, &e]);
        assert!(!c.trackers[0].is_tracking(7));
        assert_eq!(c.gone, [start]);
        let removed = body(play::Clientbound::RemoveEntities(play::RemoveEntities {
            ids: vec![7],
        }));
        assert_eq!(c.out[0].first(), Some(&removed));
        // Gone from the level: the other client loses it too, and so does
        // the level.
        e.position[0] = 0.0;
        c.tick(&mut level, &[&me, &e]);
        assert!(c.trackers[0].is_tracking(7));
        c.tick(&mut level, &[&me]);
        assert!(!c.trackers[0].is_tracking(7) && !c.trackers[1].is_tracking(7));
        assert_eq!(c.out[0].first(), Some(&removed));
        assert_eq!(level.len(), 1);
    }

    #[test]
    fn entities_in_chunks_a_client_lacks_wait_for_them() {
        let mut level = Entities::default();
        let mut c = Clients::new();
        // Two zombies in range in chunk (4, 0), which the clients don't have
        // yet, and one in chunk (0, 0).
        let mut far = state(70.0);
        let mut near = state(66.0);
        near.id = 8;
        let mut home = state(1.0);
        home.id = 9;
        c.tick(&mut level, &[&far, &near, &home]);
        for t in &c.trackers {
            assert!(!t.is_tracking(7) && !t.is_tracking(8) && t.is_tracking(9));
        }
        // The chunk arrives: both are paired, in order.
        c.chunks.insert((4, 0));
        c.tick(&mut level, &[&far, &near, &home]);
        for t in &c.trackers {
            assert!(t.is_tracking(7) && t.is_tracking(8) && t.is_tracking(9));
        }
        // One walks back into chunk (3, 0) and the other out of range.
        far.position[0] = 200.0;
        near.position[0] = 60.0;
        c.tick(&mut level, &[&far, &near, &home]);
        for t in &c.trackers {
            assert!(!t.is_tracking(7) && t.is_tracking(8) && t.is_tracking(9));
        }
    }

    /// ServerPlayer.broadcastToPlayer: a spectator is shown only to other
    /// spectators, who see everyone. Going in or out of spectator mode checks
    /// its pairs again, though nobody moved.
    #[test]
    fn spectators_are_tracked_only_by_spectators() {
        let mut level = Entities::default();
        let mut c = Clients::new();
        let player = |id: i32| {
            let mut p = state(0.0);
            (p.id, p.uuid, p.kind) = (id, id as u128, "minecraft:player".into());
            p
        };
        let (mut one, mut two) = (player(1), player(2));
        // A third player, already spectating, that neither client plays.
        let mut three = player(3);
        three.spectator = true;
        c.tick(&mut level, &[&one, &two, &three]);
        c.tick(&mut level, &[&one, &two, &three]);
        assert!(c.trackers[0].is_tracking(2) && c.trackers[1].is_tracking(1));
        assert!(!c.trackers[0].is_tracking(3) && !c.trackers[1].is_tracking(3));
        // The second player starts spectating: the first client loses it,
        // and it sees the first player and the other spectator.
        (two.spectator, c.spectators[1]) = (true, true);
        c.tick(&mut level, &[&one, &two, &three]);
        assert!(!c.trackers[0].is_tracking(2));
        let removed = body(play::Clientbound::RemoveEntities(play::RemoveEntities {
            ids: vec![2],
        }));
        assert_eq!(c.out[0].first(), Some(&removed));
        assert!(c.trackers[1].is_tracking(1) && c.trackers[1].is_tracking(3));
        // Its armour changes go to nobody who doesn't track it.
        two.equipment.push((5, Some(Stack::new(5, 1))));
        c.tick(&mut level, &[&one, &two, &three]);
        assert!(c.out[0].is_empty());
        // The first player starts spectating too: both spectators show up.
        (one.spectator, c.spectators[0]) = (true, true);
        c.tick(&mut level, &[&one, &two, &three]);
        assert!(c.trackers[0].is_tracking(2) && c.trackers[0].is_tracking(3));
        // Both back in survival: they still see each other, and lose the
        // third player.
        (one.spectator, c.spectators[0]) = (false, false);
        (two.spectator, c.spectators[1]) = (false, false);
        c.tick(&mut level, &[&one, &two, &three]);
        assert!(c.trackers[0].is_tracking(2) && c.trackers[1].is_tracking(1));
        assert!(!c.trackers[0].is_tracking(3) && !c.trackers[1].is_tracking(3));
    }

    /// ServerPlayer.broadcastToPlayer for a spectator: a spectator watching
    /// through a camera is hidden from the other spectators too.
    #[test]
    fn a_spectator_with_a_camera_is_hidden_from_spectators() {
        let mut level = Entities::default();
        let mut c = Clients::new();
        let player = |id: i32| {
            let mut p = state(0.0);
            (p.id, p.uuid, p.kind) = (id, id as u128, "minecraft:player".into());
            p
        };
        let (mut one, mut two) = (player(1), player(2));
        (one.spectator, two.spectator) = (true, true);
        c.spectators = [true, true];
        c.tick(&mut level, &[&one, &two]);
        c.tick(&mut level, &[&one, &two]);
        assert!(c.trackers[0].is_tracking(2) && c.trackers[1].is_tracking(1));
        // The second spectator takes a camera: the first loses sight of it.
        two.watching = true;
        c.tick(&mut level, &[&one, &two]);
        assert!(!c.trackers[0].is_tracking(2) && c.trackers[1].is_tracking(1));
        // Back to its own view: it shows up again.
        two.watching = false;
        c.tick(&mut level, &[&one, &two]);
        assert!(c.trackers[0].is_tracking(2));
    }

    #[test]
    fn a_forgotten_entity_is_paired_again_at_the_next_tick() {
        // A respawned player: the hub removes it from the other clients and
        // forgets it, and the next tick pairs it again as a new entity.
        let mut level = Entities::default();
        let mut c = Clients::new();
        let mut player = state(0.0);
        player.id = 1;
        player.kind = "minecraft:player".into();
        c.tick(&mut level, &[&player]);
        c.tick(&mut level, &[&player]);
        assert!(c.trackers[1].forget(1));
        assert!(!c.trackers[1].forget(1));
        c.tick(&mut level, &[&player]);
        assert!(c.trackers[1].is_tracking(1));
        let paired = c.out[1]
            .first()
            .and_then(|p| play::Clientbound::decode(i32::from(p[0]), &p[1..]).ok());
        assert!(
            matches!(paired, Some(play::Clientbound::AddEntity(ref a)) if a.id == 1),
            "{paired:?}"
        );
    }
}
