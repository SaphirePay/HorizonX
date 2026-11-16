# Firework rockets as entities plugins reach (World.spawnEntity(loc, FIREWORK_ROCKET), spawn(loc, Firework.class),
# Firework.getFireworkMeta / setFireworkMeta / detonate): a rocket loaded from its data (`/summon`, the SDK's `spawn`)
# had no command Target, so spawn_entity_from_nbt answered nothing and Bukkit's spawn gave null (MySpawn's join
# firework, rank-up and crate effects). Target::Rocket names one by its entity id, which a rocket gets when spawned
# so (others still get theirs when first tracked). Applied last (zzz_): its texts are the engine's as the rows before
# it left them.
GAME = 'horizon-client/src/game/'
LIB = 'horizon-client/src/'

EDITS = [
    # ---- the Target
    (GAME + 'commands.rs', '''    /// The Ender Dragon, by its entity id (`minecraft:ender_dragon`).
    Dragon(u32),
}''', '''    /// The Ender Dragon, by its entity id (`minecraft:ender_dragon`).
    Dragon(u32),
    /// A firework rocket (`minecraft:firework_rocket`), by its entity id.
    Rocket(u32),
}'''),
    (GAME + 'commands.rs', '''    | Target::Falling(id)
    | Target::Dragon(id)) = t''', '''    | Target::Falling(id)
    | Target::Dragon(id)
    | Target::Rocket(id)) = t'''),
    # (horizon-mixins writes this function in its own form: braces round each arm.)
    (GAME + 'commands.rs', '''            Target::Falling(id) => {
                self.falling
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| [f.position.x, f.position.y, f.position.z])
            }
''', '''            Target::Falling(id) => {
                self.falling
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| [f.position.x, f.position.y, f.position.z])
            }
            Target::Rocket(id) => {
                self.rockets
                    .iter()
                    .find(|r| r.id == id)
                    .map(|r| [r.position.x, r.position.y, r.position.z])
            }
'''),
    (GAME + 'commands.rs', '''            Target::Falling(_) => "minecraft:falling_block".into(),
''', '''            Target::Falling(_) => "minecraft:falling_block".into(),
            Target::Rocket(_) => "minecraft:firework_rocket".into(),
'''),
    # Selectors find rockets as vanilla's do (a rocket never tracked has no id yet and is left out).
    (GAME + 'commands.rs', '''        candidates.extend(self.falling.iter().map(|f| Target::Falling(f.id)));
''', '''        candidates.extend(self.falling.iter().map(|f| Target::Falling(f.id)));
        candidates.extend(self.rockets.iter().filter(|r| r.id != 0).map(|r| Target::Rocket(r.id)));
'''),
    (GAME + 'commands.rs', '''            Target::Falling(id),
            Target::Dragon(id),
        ]''', '''            Target::Falling(id),
            Target::Dragon(id),
            Target::Rocket(id),
        ]'''),
    (GAME + 'commands.rs', '''            Target::Dragon(id) => self.dragon.as_ref().is_some_and(|d| d.id == id),
''', '''            Target::Dragon(id) => self.dragon.as_ref().is_some_and(|d| d.id == id),
            Target::Rocket(id) => self.rockets.iter().any(|r| r.id == id),
'''),
    (GAME + 'commands.rs', '''            Target::Falling(id) => {
                if let Some(f) = self.falling.iter_mut().find(|f| f.id == id) {
                    f.position = v;
                    f.previous = v;
                }
            }
''', '''            Target::Falling(id) => {
                if let Some(f) = self.falling.iter_mut().find(|f| f.id == id) {
                    f.position = v;
                    f.previous = v;
                }
            }
            Target::Rocket(id) => {
                if let Some(r) = self.rockets.iter_mut().find(|r| r.id == id) {
                    r.position = v;
                    r.previous = v;
                }
            }
'''),
    (GAME + 'commands.rs', '''            Target::Falling(id) => self.falling.retain(|f| f.id != id),
''', '''            Target::Falling(id) => self.falling.retain(|f| f.id != id),
            Target::Rocket(id) => self.rockets.retain(|r| r.id != id),
'''),
    (GAME + 'native_entities.rs', '''        | Target::Falling(id)
        | Target::Dragon(id) => Some(id),''', '''        | Target::Falling(id)
        | Target::Dragon(id)
        | Target::Rocket(id) => Some(id),'''),
    (GAME + 'scoreboard.rs', '''            | Target::Falling(id)
            | Target::Dragon(id)
            | Target::Other(id) => Some(uuid_text(self.uuid_for(id))),''', '''            | Target::Falling(id)
            | Target::Dragon(id)
            | Target::Rocket(id)
            | Target::Other(id) => Some(uuid_text(self.uuid_for(id))),'''),
    (GAME + 'scoreboard.rs', '''            | Target::Falling(_)
            | Target::Dragon(_) => None,
        };''', '''            | Target::Falling(_)
            | Target::Dragon(_)
            | Target::Rocket(_) => None,
        };'''),

    # ---- its data: FireworkRocketEntity.addAdditionalSaveData / readAdditionalSaveData
    (GAME + 'fireworks.rs', '''    /// One tick of a server's client: the rocket flies on by itself between''', '''    /// What it saves (`FireworkRocketEntity.addAdditionalSaveData`): its data for `/data` and plugins.
    pub(super) fn to_saved(&self) -> RocketNbt {
        RocketNbt {
            position: self.position,
            motion: self.velocity,
            life: self.life,
            lifetime: self.lifetime,
            item: Some(self.item.clone()),
            angled: self.angled,
            owner: self.owner,
            left_owner: self.left_owner,
        }
    }
    /// One tick of a server's client: the rocket flies on by itself between'''),
    (LIB + 'entity_nbt.rs', '''/// `FallingBlockEntity`: BlockState, Time, DropItem, HurtEntities.''', '''/// `FireworkRocketEntity`: Life, LifeTime, FireworksItem, ShotAtAngle, and the projectile's Owner and LeftOwner.
pub fn firework_rocket_to_nbt(r: &RocketNbt, uuid: [i32; 4], owner: Option<[i32; 4]>) -> Tag {
    let base = Base {
        id: "minecraft:firework_rocket",
        uuid,
        position: r.position,
        motion: r.motion,
        rotation: [0.0, 0.0],
        on_ground: false,
        fall_distance: 0.0,
    };
    let mut tag = base.tag(None);
    put(&mut tag, "Life", Tag::Int(r.life));
    put(&mut tag, "LifeTime", Tag::Int(r.lifetime));
    if let Some(item) = &r.item {
        put(&mut tag, "FireworksItem", item_to_nbt(item));
    }
    put(&mut tag, "ShotAtAngle", Tag::Byte(i8::from(r.angled)));
    put(&mut tag, "LeftOwner", Tag::Byte(i8::from(r.left_owner)));
    if let (Some(Who::Player), Some(owner)) = (r.owner, owner) {
        put(&mut tag, "Owner", Tag::IntArray(owner.to_vec()));
    }
    tag
}

/// `FallingBlockEntity`: BlockState, Time, DropItem, HurtEntities.'''),
    (GAME + 'commands/data.rs', '''            Target::Dragon(id) => {
                let d = self.dragon.as_ref().filter(|d| d.id == id)?;
                let (uuid, raw) = self.persist_anon("minecraft:ender_dragon", d.position);
                d.to_nbt(uuid.unwrap_or_else(random_uuid), raw)
            }
''', '''            Target::Dragon(id) => {
                let d = self.dragon.as_ref().filter(|d| d.id == id)?;
                let (uuid, raw) = self.persist_anon("minecraft:ender_dragon", d.position);
                d.to_nbt(uuid.unwrap_or_else(random_uuid), raw)
            }
            Target::Rocket(id) => {
                let saved = self.rockets.iter().find(|r| r.id == id)?.to_saved();
                entity_nbt::firework_rocket_to_nbt(&saved, self.uuid_for(id), player_uuid)
            }
'''),
    # New data for a rocket (Firework.setFireworkMeta: its FireworksItem): rebuilt from it, keeping its id.
    (GAME + 'commands/data.rs', '''            _ => return Err(stale()),
        }
        if let Target::Mob(id) | Target::Item(id) | Target::Stand(id) | Target::Frame(id) = t {''', '''            Target::Rocket(id) => {
                let saved = entity_nbt::firework_rocket_from_nbt(&tag, self.level.player_uuid).ok_or_else(stale)?;
                if let Some(slot) = self.rockets.iter_mut().find(|r| r.id == id) {
                    let mut new = crate::game::fireworks::Rocket::from_nbt(saved);
                    new.id = id;
                    *slot = new;
                }
            }
            _ => return Err(stale()),
        }
        if let Target::Mob(id) | Target::Item(id) | Target::Stand(id) | Target::Frame(id) = t {'''),
    # A rocket loads with no id (it gets one when first tracked): spawned from its data, the new one is the root.
    (GAME + 'commands/data.rs', '''        let mut waiting = std::mem::take(&mut self.world.loaded_entities);
        let before = self.next_entity;''', '''        let mut waiting = std::mem::take(&mut self.world.loaded_entities);
        let before = self.next_entity;
        let rockets_before = self.rockets.len();'''),
    (GAME + 'commands/data.rs', '''        if self.next_entity == before {
            return None;
        }
        // The root of the stack is the first entity spawned.''', '''        if self.next_entity == before {
            if self.rockets.len() > rockets_before {
                let id = crate::game::lan_entities::assign_entity_id(&mut self.next_entity, &mut self.rockets[rockets_before].id);
                return Some(Target::Rocket(id));
            }
            return None;
        }
        // The root of the stack is the first entity spawned.'''),
]
