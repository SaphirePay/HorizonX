//! The locator bar of the players on a served world (lan/waypoints.rs): what
//! each player looks like to their level's waypoint manager this tick.
use super::{Game, lan::LanHost};
use horizon_client::{
    items::ItemStack,
    lan::{
        hub::HostState,
        waypoints::{self, HIDING_HEADS, Icon, PLAYER_RANGE},
    },
};

/// The host player's connection id in the waypoint managers: its packets
/// go to this client's locator bar rather than to a guest.
const HOST_CONN: u32 = u32::MAX;

/// `waypoint_transmit_range`: hidden by crouching, invisibility or a head worn.
fn transmit_range(crouching: bool, invisible: bool, head: Option<&ItemStack>) -> f64 {
    let hiding_head = head.is_some_and(|s| HIDING_HEADS.contains(&&*s.item));
    if crouching || invisible || hiding_head {
        0.0
    } else {
        PLAYER_RANGE
    }
}

impl Game<'_> {
    /// The player's team and its colour, as the locator bar shows it.
    fn waypoint_team(&self, name: &str) -> Option<(String, Option<i32>)> {
        let team = self.score.board.players_team(name)?;
        let color = team.color.as_deref().and_then(waypoints::team_rgb);
        Some((team.name.clone(), color))
    }

    /// Each level's `ServerWaypointManager` ticked with the players in that
    /// level: a player who changes level leaves the old one's manager
    /// (`ServerLevel.removePlayerImmediately`: untracked for everyone there)
    /// and joins the new one's (`addDuringTeleport`).
    pub(super) fn lan_update_waypoints(&mut self, host: &mut LanHost, state: Option<&HostState>) {
        let mut players: Vec<(&str, waypoints::Player)> = Vec::new();
        if let Some(h) = state {
            let icon = self
                .cmds
                .waypoint_icons
                .get(&super::commands::Target::Player)
                .map(|i| Icon {
                    style: waypoints::style_name(&i.style),
                    color: i.color,
                })
                .unwrap_or_default();
            players.push((
                self.dim.id(),
                waypoints::Player {
                    uuid: h.uuid,
                    conn: Some(HOST_CONN),
                    position: h.position,
                    spectator: self.spectator(),
                    transmit_range: transmit_range(
                        h.sneaking,
                        self.potions.effects.get("invisibility").is_some(),
                        h.equipment[5].as_ref(),
                    ),
                    receive_range: PLAYER_RANGE,
                    first_tick: false,
                    view_distance: host.hub.config.view_distance,
                    team: self.waypoint_team(&h.name),
                    icon,
                },
            ));
        }
        let mut respawned = Vec::new();
        for g in host.hub.guests.iter().filter(|g| g.placed) {
            // PlayerList.respawn makes a new ServerPlayer.
            if g.dead {
                host.waypoint_dead.insert(g.uuid);
            } else if host.waypoint_dead.remove(&g.uuid) {
                respawned.push(g.uuid);
            }
            // Player.updatePlayerPose: the shift key crouches unless flying.
            let crouching = g.sneaking && !g.flying && !g.fall_flying && !g.sleeping;
            players.push((
                &g.dimension,
                waypoints::Player {
                    uuid: g.uuid,
                    conn: Some(g.conn),
                    position: g.position,
                    spectator: g.spectator(),
                    transmit_range: transmit_range(
                        crouching,
                        g.effects.get("invisibility").is_some(),
                        g.armor[0].as_ref(),
                    ),
                    receive_range: PLAYER_RANGE,
                    first_tick: g.ticks == 0,
                    view_distance: g.view_distance,
                    team: self.waypoint_team(&g.name),
                    icon: Icon::default(),
                },
            ));
        }
        // HorizonsX: players a late region holds (lag isolation) stay on the locator bar where they were, frozen:
        // left out, everyone's connections to them were broken and made again each time their region ran late
        // (hundreds of thousands of them with a large region), and theirs can't be sent anyway.
        let mut frozen: horizon_client::fast_hash::FxHashSet<u128> = Default::default();
        for (level, p) in &players {
            if let Some(conn) = p.conn.filter(|&c| c != HOST_CONN) {
                host.waypoint_last.insert(conn, ((*level).to_owned(), p.clone()));
            }
        }
        let held: Vec<(String, waypoints::Player)> = host
            .hub
            .away
            .iter()
            .filter_map(|conn| host.waypoint_last.get(conn).cloned())
            .collect();
        host.waypoint_last
            .retain(|conn, _| host.hub.away.contains(conn) || host.hub.guests.iter().any(|g| g.conn == *conn && g.placed));
        for (_, p) in &held {
            frozen.insert(p.uuid);
        }
        players.extend(held.iter().map(|(level, p)| (level.as_str(), p.clone())));
        host.waypoint_dead
            .retain(|uuid| players.iter().any(|(_, p)| p.uuid == *uuid));
        let locator_bar = self.ui.rules.bool("locator_bar");
        let mut levels: Vec<String> = host.waypoints.keys().cloned().collect();
        for (level, _) in &players {
            if !levels.iter().any(|l| l == level) {
                levels.push((*level).to_owned());
            }
        }
        levels.sort();
        // HorizonsX: each player's packets together, in their order, as the managers give them (one list per
        // connection, not one for everyone split up again here).
        type Packets = Vec<horizon_protocol::packets::play::TrackedWaypoint>;
        let index: horizon_client::fast_hash::FxHashMap<u32, usize> =
            host.hub.guests.iter().enumerate().map(|(i, g)| (g.conn, i)).collect();
        let mut by_guest: Vec<Packets> = Vec::new();
        by_guest.resize_with(host.hub.guests.len(), Vec::new);
        let mut host_packets: Packets = Vec::new();
        for level in levels {
            let here: Vec<waypoints::Player> = players
                .iter()
                .filter(|(l, _)| *l == level)
                .map(|(_, p)| p.clone())
                .collect();
            let away: Vec<(u128, u32)> = players
                .iter()
                .filter(|(l, _)| *l != level)
                .filter_map(|(_, p)| Some((p.uuid, p.conn?)))
                .collect();
            let manager = host.waypoints.entry(level.clone()).or_default();
            let runs = horizon_client::spans::time("wp_manager", || manager.tick_level_by_conn_frozen(&here, &away, &respawned, locator_bar, &frozen));
            for (conn, run) in runs {
                if conn == HOST_CONN {
                    host_packets.extend(run);
                } else if let Some(&i) = index.get(&conn) {
                    if by_guest[i].is_empty() {
                        by_guest[i] = run;
                    } else {
                        by_guest[i].extend(run);
                    }
                }
            }
            // Nobody left in the level: nothing to remember there.
            if here.is_empty() {
                host.waypoints.remove(&level);
            }
        }
        // The host's own connection: its locator bar.
        for w in &host_packets {
            self.handle_waypoint(w);
        }
        // The packets are encoded on every core, each player's in their order (each player found by the table made
        // once above, not `Hub::guest`'s scan: with 325 players in one place the locator bar sends ~100k packets a
        // tick, and the scans were the cube of the player count). Every one is a TrackedWaypoint, which the movement
        // prediction never records (`Guest::note_change` keeps knockback, velocity, effects, abilities): `Guest::send`
        // of one is only its encode and hand-over, so the players the anticheat records take the same way as the
        // rest (with the prediction on, as by default, all of them went one at a time on this thread: 48 ms at 300).
        let work: Vec<(&horizon_client::lan::conn::PlayerSender, Packets)> = host
            .hub
            .guests
            .iter()
            .zip(by_guest)
            .filter(|(_, ps)| !ps.is_empty())
            .map(|(g, ps)| (&g.sender, ps))
            .collect();
        let hold = horizon_client::lan::conn::holding();
        horizon_client::spans::time("wp_send", || {
            horizon_client::v2::map_batches(work, 4, |(sender, packets)| {
                let _hold = hold.then(horizon_client::lan::conn::Holding::start);
                sender.send_many(packets.into_iter().map(|w| horizon_protocol::packets::play::Clientbound::TrackedWaypoint(w).encode()));
            })
        });
    }
}
