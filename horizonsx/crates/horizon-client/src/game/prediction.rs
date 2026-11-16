//! The movement prediction's replay on the server (`lan::prediction`): each guest's client ticks
//! played again with the client's own movement code, in the guest's level.
use super::Game;
use super::lan::LanHost;
use horizon_client::lan::entity::EntityState;
use horizon_client::lan::guest::Guest;
use horizon_client::lan::prediction::{Replay, near, slack};
use super::lan_actions::GuestPlay;
use horizon_client::motion::{MotionWorld, PlayerMotion, Scene};
use horizon_client::world::World;

impl Game<'_> {
    /// Guest `conn`'s client ticks since the server's last tick, played again, before the server
    /// moves the guest's own body.
    pub(super) fn guest_replay(&mut self, host: &mut LanHost, conn: u32) {
        if !host.hub.recording() {
            return;
        }
        // Played ahead with everyone's on every core (`replay_ahead`), or here.
        let over = match host.play.get_mut(&conn).and_then(|p| p.replay_ahead.take()) {
            Some(over) => over,
            None => {
                let tolerance = replay_tolerance(host);
                let ticks = host.hub.ticks;
                let Some(g) = host.hub.guest_mut(conn) else {
                    return;
                };
                let mut random = replay_random(g, ticks);
                let refs = ReplayRefs {
                    res: &self.res,
                    assets: &self.pack.assets,
                    collision: self.collision,
                    motion: self.motion,
                };
                play_replay(g, &self.world, &mut random, &refs, tolerance)
            }
        };
        let Some(over) = over else {
            return;
        };
        let g = host.hub.guest(conn).expect("played above");
        if excused(g, &self.world, host.states.as_slice(), &host.hub.guests) {
            if let Some(replay) = host
                .hub
                .guest_mut(conn)
                .and_then(|g| g.replay.as_deref_mut())
            {
                replay.excuse();
            }
            return;
        }
        let agreed = g.replay.as_deref().expect("played above").agreed();
        for event in host.hub.anticheat_prediction(conn, &over, agreed) {
            self.lan_log_event(event);
        }
    }

    /// HorizonsX: the replays of the guests whose turn does nothing before it that a replay reads (no digging,
    /// no item in use or delay, not dead, asleep or travelling), played on every core before the turns. A
    /// replay reads only its own guest and the world's blocks, and draws from a random of its own (vanilla
    /// never plays the client's movement, so the level's random stays as vanilla has it); what it found is
    /// judged in the guest's turn, as before. The others play in their turn.
    pub(super) fn replay_ahead(&mut self, host: &mut LanHost, conns: &[u32]) {
        if !host.hub.recording() || conns.len() < REPLAY_AHEAD_MIN || !replay_ahead_on() {
            return;
        }
        let ready: horizon_client::fast_hash::FxHashSet<u32> = conns
            .iter()
            .copied()
            .filter(|&conn| {
                let quiet = host
                    .play
                    .get(&conn)
                    .is_none_or(GuestPlay::replay_reads_nothing_before);
                quiet && !self.on_server_trip(conn)
            })
            .collect();
        let tolerance = replay_tolerance(host);
        let ticks = host.hub.ticks;
        let refs = ReplayRefs {
            res: &self.res,
            assets: &self.pack.assets,
            collision: self.collision,
            motion: self.motion,
        };
        let world = &self.world;
        let work: Vec<&mut Guest> = host
            .hub
            .guests
            .iter_mut()
            .filter(|g| ready.contains(&g.conn) && !g.dead && g.health > 0.0 && g.bed.is_none())
            .collect();
        let played = horizon_client::v2::map_batches(work, 2, |g| {
            let mut random = replay_random(g, ticks);
            (g.conn, play_replay(g, world, &mut random, &refs, tolerance))
        });
        for (conn, over) in played {
            host.play.entry(conn).or_default().replay_ahead = Some(over);
        }
    }
}

/// Guests at which the replays are played ahead on every core.
const REPLAY_AHEAD_MIN: usize = 8;

/// `HORIZON_REPLAY_AHEAD=0`: every replay in its guest's turn.
fn replay_ahead_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_REPLAY_AHEAD").map_or(true, |v| v.trim() != "0"))
}

fn replay_tolerance(host: &LanHost) -> f64 {
    host.hub
        .anticheat
        .as_deref()
        .map_or(f64::INFINITY, |ac| ac.config.prediction.tolerance)
}

/// The replay's own random, the same for a guest's tick wherever it plays.
fn replay_random(g: &Guest, ticks: u64) -> horizon_core::random::LegacyRandom {
    let uuid = g.uuid as u64 ^ (g.uuid >> 64) as u64;
    horizon_core::random::LegacyRandom::new((uuid ^ ticks.wrapping_mul(0x9E37_79B9_7F4A_7C15)) as i64)
}

/// What a replay reads besides its guest and the world.
struct ReplayRefs<'a> {
    res: &'a super::Resources,
    assets: &'a horizon_client::assets::Assets,
    collision: &'a horizon_world::block_collision::BlockCollisionRegistry<'a>,
    motion: &'a horizon_client::motion::MotionResources,
}

/// Guest `g`'s client ticks played again on `world` (read only), drawing from `random`: the ticks to
/// judge, if any landed too far and count.
fn play_replay(
    g: &mut Guest,
    world: &World,
    random: &mut horizon_core::random::LegacyRandom,
    refs: &ReplayRefs<'_>,
    tolerance: f64,
) -> Option<horizon_client::lan::prediction::Over> {
    // What moves the player isn't its own movement (a vehicle, a bed), or the server holds it:
    // the replay starts again at the teleport that ends it (dismountTo, stopSleeping, respawn).
    let held = g.vehicle.is_some() || g.sleeping || g.dead || g.spectator() || !g.client_loaded();
    // A teleport not yet accepted (a setback): the client still moves on its own until it
    // hears it, and the replay follows, but nothing counts.
    let waiting = g.awaiting_teleport.is_some();
    // The server's copy of a guest doesn't follow its armor (Game.glider_worn).
    let glider = g.armor[1].as_ref().is_some_and(|s| {
        refs.res
            .items()
            .get(&s.item)
            .is_some_and(|i| i.glider && (i.max_damage == 0 || s.damage + 1 < i.max_damage))
    });
    if let Some(motion) = g.motion.as_deref_mut() {
        motion.glider = glider;
    }
    let (Some(record), Some(server)) = (g.anticheat.record.as_deref_mut(), g.motion.as_deref())
    else {
        return None;
    };
    let mut run = |body: &mut PlayerMotion, keys, sprinting| {
        // Game.sync_player_body's part that depends on the blocks.
        body.nudge = super::closest_space_nudge(body, world, refs.res.facts());
        MotionWorld {
            entities: &[],
            player: body,
            scene: Scene::Shared(world, &mut *random),
            assets: refs.assets,
            collision_binding: refs.collision,
            resources: refs.motion,
        }
        .replayed_tick(keys, sprinting)
    };
    if let Some(restart) = record.take_restart() {
        // What the client keeps through a teleport (its jump delay, a glide...) is the last
        // replay's, once it has played the ticks before it. The client's motion through the
        // teleport is its own (an End gateway's), and a replay that was excused when the teleport
        // came may be off in what it can't see: either way the replay finds the client again.
        let mut excused = restart.kept_motion;
        let mut replay = g
            .replay
            .take()
            .and_then(|mut replay| {
                let finished = replay.finish(record, server, &restart, &mut run);
                excused |= replay.excused();
                finished.then_some(replay)
            })
            .unwrap_or_else(|| Box::new(Replay::new(server, &restart)));
        replay.restart(&restart);
        if excused {
            replay.excuse();
        }
        g.replay = Some(replay);
    }
    if held {
        g.replay = None;
        return None;
    }
    let replay = g.replay.as_deref_mut()?;
    // Ticks no longer kept (a server that stalled for seconds): wait for the next teleport.
    let Some(over) = replay.catch_up(record, server, tolerance, &mut run) else {
        g.replay = None;
        return None;
    };
    (over.ticks != 0 && !waiting).then_some(over)
}

/// Whether guest `g`'s last moves may be off for a reason the replay can't play yet: an entity near
/// it (a push, a boat or shulker to stand on, a firework it flies with); a block change near it the
/// server hasn't sent yet (one sent but unseen, or an entity gone that the client may still have, the
/// replay excuses itself); a moving piston; a chunk around it the client doesn't have; a riptide
/// launch (the trident's push is the client's own).
fn excused(g: &Guest, world: &World, entities: &[EntityState], guests: &[Guest]) -> bool {
    let at = g.position;
    let slack = slack(std::array::from_fn(|i| at[i] - g.previous_position[i]));
    let close = |p: [f64; 3]| {
        let r = 3.0 + slack;
        (p[0] - at[0]).abs() <= r
            && (p[2] - at[2]).abs() <= r
            && (-3.0 - slack..=4.0 + slack).contains(&(p[1] - at[1]))
    };
    let unsent = world
        .block_log
        .as_ref()
        .is_some_and(|log| log.iter().any(|&(x, y, z)| near(at, [x, y, z], slack)));
    let pistons = world
        .moving_pistons
        .iter()
        .any(|&(p, _)| near(at, p, slack));
    // A client doesn't move in a chunk it doesn't have.
    let (cx, cz) = ((at[0].floor() as i32) >> 4, (at[2].floor() as i32) >> 4);
    let unloaded =
        (-1..=1).any(|dx| (-1..=1).any(|dz| !g.sent_chunks.contains(&(cx + dx, cz + dz))));
    let riptide = g
        .motion
        .as_deref()
        .is_some_and(|m: &PlayerMotion| m.auto_spin_attack_ticks > 0);
    // Items and experience orbs neither push nor hold anyone up; the list holds the player itself.
    let entity = entities.iter().any(|e| {
        e.id != g.entity_id
            && !matches!(
                e.kind.as_str(),
                "minecraft:item" | "minecraft:experience_orb"
            )
            && close(e.position)
    });
    let player = guests
        .iter()
        .any(|o| o.conn != g.conn && o.placed && o.dimension == g.dimension && close(o.position));
    unsent || pistons || unloaded || riptide || entity || player
}
