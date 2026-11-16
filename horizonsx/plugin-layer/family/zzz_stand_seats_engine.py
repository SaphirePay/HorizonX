# A server player riding an armor stand, as vanilla lets them (`/ride`, `Entity.addPassenger`): a sit plugin (GSit) seats
# the player on an invisible marker stand. The rider sits where vanilla puts it (the stand's passenger point, its
# height, less the player's vehicle attachment 0.6), stays until they sneak (EntityDismountEvent first, as a cushion),
# and is let off when the stand goes. The stand's passengers reach the clients as any guest vehicle's (lan_riding).
GAME = 'horizon-client/src/game/'

SEAT = '''
/// `ArmorStand`'s passenger point for a rider (its height: 0 for a marker, scaled when small) less the player's own
/// vehicle attachment (`Avatar`: 0.6): where vanilla seats a player on it.
pub(super) fn stand_seat(a: &horizon_client::entities::ArmorStand) -> horizon_core::geometry::Vec3 {
    a.position.add_vector(horizon_core::geometry::Vec3::new(0.0, stand_height(a) - 0.6, 0.0))
}

/// `Entity.getDismountLocationForPassenger` (an armor stand keeps the default): its box's top.
pub(super) fn stand_dismount(a: &horizon_client::entities::ArmorStand) -> horizon_core::geometry::Vec3 {
    a.position.add_vector(horizon_core::geometry::Vec3::new(0.0, stand_height(a), 0.0))
}

fn stand_height(a: &horizon_client::entities::ArmorStand) -> f64 {
    if a.marker {
        0.0
    } else if a.small {
        0.9875
    } else {
        1.975
    }
}
'''

EDITS = [
    # /ride: a player onto an armor stand (RideCommand.mount forces it: no canRide, no canAddPassenger).
    (GAME + 'commands/world_cmds.rs', '''            Target::Cart(id) => {
                if target != Target::Player || self.ridden_by_guest(id) || crate::game::paper_fam_ev_entity::mount_refused(self, id) {
                    return Err(generic());
                }''', '''            Target::Stand(id) => {
                if target != Target::Player || self.ridden_by_guest(id) || crate::game::paper_fam_ev_entity::mount_refused(self, id) {
                    return Err(generic());
                }
                self.riding = Some(id);
                self.trigger_player("minecraft:started_riding");
                self.feedback(Text::translate(
                    "commands.ride.mount.success",
                    &[name, vehicle_name],
                ));
                return Ok(1);
            }
            Target::Cart(id) => {
                if target != Target::Player || self.ridden_by_guest(id) || crate::game::paper_fam_ev_entity::mount_refused(self, id) {
                    return Err(generic());
                }'''),
    # Entity.getVehicle: a stand is one too.
    (GAME + 'commands/world_cmds.rs', '''        } else if self.carts.iter().any(|c| c.id == id) {
            Target::Cart(id)
        } else {
            Target::Mob(id)
        })''', '''        } else if self.carts.iter().any(|c| c.id == id) {
            Target::Cart(id)
        } else if self.armor_stands.iter().any(|a| a.id == id) {
            Target::Stand(id)
        } else {
            Target::Mob(id)
        })'''),
    # Player.rideTick for a server player on a stand: seated, off when they sneak, dropped when the stand goes.
    (GAME + 'lan_vehicles.rs', '''        let cushion = self.cushions.iter().any(|c| c.id == vehicle);
        // The vehicle is gone (broken, in another level) or the rider died:
        // its client drops it with the entity or the death screen.
        if dead || (boat.is_none() && !cart && !mount && !cushion) {''', '''        let cushion = self.cushions.iter().any(|c| c.id == vehicle);
        let stand = self.armor_stands.iter().any(|a| a.id == vehicle);
        // The vehicle is gone (broken, in another level) or the rider died:
        // its client drops it with the entity or the death screen.
        if dead || (boat.is_none() && !cart && !mount && !cushion && !stand) {'''),
    (GAME + 'lan_vehicles.rs', '''        let seat = self.as_guest(host, conn, |game| {
            // A cushion does not move: the rider sits on it until it sneaks''', '''        let seat = self.as_guest(host, conn, |game| {
            // An armor stand (a sit plugin's seat): the rider sits on it until it sneaks, if no listener keeps it on.
            if let Some((id, seat, off)) = game
                .armor_stands
                .iter()
                .find(|a| Some(a.id) == game.riding)
                .map(|a| (a.id, stand_seat(a), stand_dismount(a)))
            {
                if keys.shift && !super::paper_fam_ev_entity::dismount_refused(game, id) {
                    game.riding = None;
                    if let Some(p) = game.player.as_mut() {
                        game.previous_position = p.position;
                        p.position = off;
                        p.velocity = Vec3::ZERO;
                        p.fall_distance = 0.0;
                    }
                    return None;
                }
                if let Some(p) = game.player.as_mut() {
                    p.velocity = Vec3::ZERO;
                    p.fall_distance = 0.0;
                }
                return Some([seat.x, seat.y, seat.z]);
            }
            // A cushion does not move: the rider sits on it until it sneaks'''),
    # Boarding: the rider goes to its seat on the stand.
    (GAME + 'lan_vehicles.rs', '''            .or_else(|| {
                self.cushions
                    .iter()
                    .find(|c| c.id == vehicle)
                    .map(|c| (c.position, c.seat()))
            });''', '''            .or_else(|| {
                self.cushions
                    .iter()
                    .find(|c| c.id == vehicle)
                    .map(|c| (c.position, c.seat()))
            })
            .or_else(|| {
                self.armor_stands
                    .iter()
                    .find(|a| a.id == vehicle)
                    .map(|a| (a.position, stand_seat(a)))
            });'''),
    (GAME + 'lan_vehicles.rs', '''use super::{Game, Keys, lan::LanHost, minecarts::SEAT};''', '''use super::{Game, Keys, lan::LanHost, minecarts::SEAT};''' + SEAT),
]
