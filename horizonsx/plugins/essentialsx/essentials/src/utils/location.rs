//! `LocationUtil`: is a block safe to stand in, and if not, where is.
use crate::prelude::*;
use crate::utils::material::is_bed;
use crate::utils::number::constrain_to_range;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

pub const RADIUS: i32 = 3;

/// `VOLUME`: every offset within the radius, nearest first.
static VOLUME: LazyLock<Vec<(i32, i32, i32)>> = LazyLock::new(|| {
    let mut all = Vec::new();
    for x in -RADIUS..=RADIUS {
        for y in -RADIUS..=RADIUS {
            for z in -RADIUS..=RADIUS {
                all.push((x, y, z));
            }
        }
    }
    all.sort_by_key(|(x, y, z)| x * x + y * y + z * z);
    all
});

static WATER_IS_HOLLOW: AtomicBool = AtomicBool::new(false);

/// `setIsWaterSafe`.
pub fn set_is_water_safe(safe: bool) {
    WATER_IS_HOLLOW.store(safe, Ordering::Relaxed);
}

fn is_water(material: &Material) -> bool {
    material.name() == "WATER"
}

/// `HOLLOW_MATERIALS`: what a player can stand inside.
fn is_hollow(material: &Material) -> bool {
    let name = material.name();
    if matches!(name.as_str(), "BARRIER" | "DIRT_PATH" | "FARMLAND") {
        return false;
    }
    name == "LIGHT" || material.is_transparent() || (is_water(material) && WATER_IS_HOLLOW.load(Ordering::Relaxed))
}

fn is_damaging_type(material: &Material) -> bool {
    matches!(material.name().as_str(), "CACTUS" | "CAMPFIRE" | "FIRE" | "MAGMA_BLOCK" | "SOUL_CAMPFIRE" | "SOUL_FIRE" | "SWEET_BERRY_BUSH" | "WITHER_ROSE")
}

fn is_lava(material: &Material) -> bool {
    material.name() == "LAVA"
}

/// `getTarget`: the block being looked at, through air and water.
pub fn get_target(player: &horizon_bukkit::Player) -> EssResult<Location> {
    get_target_within(player, 300)
}

pub fn get_target_within(player: &horizon_bukkit::Player, max_distance: i32) -> EssResult<Location> {
    Ok(line_of_sight(player, max_distance, |material| material.is_transparent() || material.name() == "LIGHT" || is_water(material)).get_location())
}

/// `LivingEntity.getTargetBlock`: CraftBukkit walks a `BlockIterator` of at most 120 blocks, counted along
/// the axis the view runs most along, and gives the first block that doesn't let it through, or the last.
pub fn line_of_sight(player: &horizon_bukkit::Player, max_distance: i32, through: impl Fn(&Material) -> bool) -> horizon_bukkit::Block {
    let eyes = player.get_eye_location();
    let world = eyes.get_world().clone();
    let view = eyes.get_direction();
    let (origin, direction) = ([eyes.x, eyes.y, eyes.z], [view.x, view.y, view.z]);
    let length = direction.iter().map(|part| part * part).sum::<f64>().sqrt();
    let main = (0..3).max_by(|a, b| direction[*a].abs().total_cmp(&direction[*b].abs())).unwrap_or(0);
    let limit = (f64::from(max_distance.min(120)) * direction[main].abs() / length).round() as i32;
    let start = origin.map(|part| part.floor() as i32);
    let mut cell = start;
    let step = direction.map(|part| if part < 0.0 { -1 } else { 1 });
    let mut next = [0.0; 3];
    for axis in 0..3 {
        let edge = f64::from(cell[axis]) + if step[axis] > 0 { 1.0 } else { 0.0 };
        next[axis] = if direction[axis] == 0.0 { f64::INFINITY } else { (edge - origin[axis]) / direction[axis] };
    }
    loop {
        let block = world.get_block_at(cell[0], cell[1], cell[2]);
        if !through(&block.get_type()) {
            return block;
        }
        let axis = (0..3).min_by(|a, b| next[*a].total_cmp(&next[*b])).unwrap_or(0);
        if axis == main && (cell[main] - start[main]).abs() >= limit {
            return block;
        }
        cell[axis] += step[axis];
        next[axis] += 1.0 / direction[axis].abs();
    }
}

/// `isBlockAboveAir`.
pub fn is_block_above_air(world: &World, x: i32, y: i32, z: i32) -> bool {
    y > world.get_max_height() || is_hollow(&world.get_block_at(x, y - 1, z).get_type())
}

fn border(world: &World) -> (i32, i32, i32, i32) {
    let border = world.get_world_border();
    let center = border.get_center();
    let radius = border.get_size() as i32 / 2;
    (center.get_block_x() - radius, center.get_block_x() + radius, center.get_block_z() - radius, center.get_block_z() + radius)
}

/// `isBlockOutsideWorldBorder`.
pub fn is_block_outside_world_border(world: &World, x: i32, z: i32) -> bool {
    let (x1, x2, z1, z2) = border(world);
    x < x1 || x > x2 || z < z1 || z > z2
}

/// `getXInsideWorldBorder`.
pub fn get_x_inside_world_border(world: &World, x: i32) -> i32 {
    let (x1, x2, _, _) = border(world);
    x.clamp(x1, x2)
}

/// `getZInsideWorldBorder`.
pub fn get_z_inside_world_border(world: &World, z: i32) -> i32 {
    let (_, _, z1, z2) = border(world);
    z.clamp(z1, z2)
}

/// `isBlockUnsafeForUser`.
pub fn is_block_unsafe_for_user(user: &User, world: &World, x: i32, y: i32, z: i32) -> bool {
    if let Some(player) = user.player()
        && player.is_online()
        && *world == player.get_world()
        && (matches!(player.get_game_mode(), "CREATIVE" | "SPECTATOR") || user.is_god_mode_enabled())
        && player.get_allow_flight()
    {
        return false;
    }
    is_block_damaging(world, x, y, z) || is_block_above_air(world, x, y, z) || is_block_outside_world_border(world, x, z)
}

/// `isBlockUnsafe`.
pub fn is_block_unsafe(world: &World, x: i32, y: i32, z: i32) -> bool {
    is_block_damaging(world, x, y, z) || is_block_above_air(world, x, y, z)
}

/// `isBlockDamaging`.
pub fn is_block_damaging(world: &World, x: i32, y: i32, z: i32) -> bool {
    let block = world.get_block_at(x, y, z).get_type();
    let below = world.get_block_at(x, y - 1, z).get_type();
    let above = world.get_block_at(x, y + 1, z).get_type();
    if is_damaging_type(&below) || is_lava(&below) || is_bed(&below) {
        return true;
    }
    if block.name() == "NETHER_PORTAL" {
        return true;
    }
    !is_hollow(&block) || !is_hollow(&above)
}

/// `getRoundedDestination`.
pub fn get_rounded_destination(location: &Location) -> Location {
    Location::new(location.get_world().clone(), f64::from(location.get_block_x()) + 0.5, java_round(location.get_y()) as f64, f64::from(location.get_block_z()) + 0.5, location.get_yaw(), location.get_pitch())
}

/// `Math.round(double)`: half up, unlike Rust's `round`.
pub fn java_round(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

/// `getSafeDestination(IEssentials, IUser, Location)`.
pub fn get_safe_destination_for(user: &User, location: &Location) -> EssResult<Location> {
    if let Some(player) = user.player()
        && player.is_online()
        && !settings().is_always_teleport_safety()
        && (matches!(player.get_game_mode(), "CREATIVE" | "SPECTATOR") || user.is_god_mode_enabled())
    {
        if should_fly(location) && player.get_allow_flight() {
            player.set_flying(true);
        }
        return Ok(if settings().is_teleport_to_center_location() { get_rounded_destination(location) } else { location.clone() });
    }
    get_safe_destination(location)
}

/// `getSafeDestination(IEssentials, Location)`.
pub fn get_safe_destination(location: &Location) -> EssResult<Location> {
    let world = location.get_world();
    let world_min_y = world.get_min_height();
    let world_logical_y = world.get_logical_height();
    let world_max_y = if location.get_block_y() < world_logical_y { world_logical_y } else { world.get_max_height() };
    let mut x = location.get_block_x();
    let mut y = java_round(location.get_y()) as i32;
    let mut z = location.get_block_z();
    if is_block_outside_world_border(world, x, z) {
        x = get_x_inside_world_border(world, x);
        z = get_z_inside_world_border(world, z);
    }
    let (orig_x, orig_y, orig_z) = (x, y, z);
    while is_block_above_air(world, x, y, z) {
        y -= 1;
        if y < 0 {
            y = orig_y;
            break;
        }
    }
    if is_block_unsafe(world, x, y, z) {
        x = if java_round(location.get_x()) == i64::from(orig_x) { x - 1 } else { x + 1 };
        z = if java_round(location.get_z()) == i64::from(orig_z) { z - 1 } else { z + 1 };
    }
    let mut i = 0;
    while is_block_unsafe(world, x, y, z) {
        i += 1;
        if i >= VOLUME.len() {
            x = orig_x;
            y = constrain_to_range(orig_y + RADIUS, world_min_y, world_max_y);
            z = orig_z;
            break;
        }
        let (dx, dy, dz) = VOLUME[i];
        x = orig_x + dx;
        y = constrain_to_range(orig_y + dy, world_min_y, world_max_y);
        z = orig_z + dz;
    }
    while is_block_unsafe(world, x, y, z) {
        y += 1;
        if y >= world_max_y {
            x += 1;
            break;
        }
    }
    while is_block_unsafe(world, x, y, z) {
        y -= 1;
        if y <= world_min_y + 1 {
            x += 1;
            y = (world.get_highest_block_y_at(x, z) + 1).min(world_max_y);
            fail_if!(x - 48 > location.get_block_x(), EssError::tl("holeInFloor", &[]));
        }
    }
    Ok(Location::new(world.clone(), f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5, location.get_yaw(), location.get_pitch()))
}

/// `shouldFly`.
pub fn should_fly(location: &Location) -> bool {
    let world = location.get_world();
    let (x, z) = (location.get_block_x(), location.get_block_z());
    let mut y = java_round(location.get_y()) as i32;
    let mut count = 0;
    while is_block_unsafe(world, x, y, z) && y >= world.get_min_height() {
        y -= 1;
        count += 1;
        if count > 2 {
            return true;
        }
    }
    y < world.get_min_height()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_is_javas() {
        assert_eq!(java_round(-0.5), 0);
        assert_eq!(java_round(0.5), 1);
        assert_eq!(java_round(-1.5), -1);
    }

    #[test]
    fn volume_starts_at_the_center() {
        assert_eq!(VOLUME[0], (0, 0, 0));
        assert_eq!(VOLUME.len(), 343);
    }
}
