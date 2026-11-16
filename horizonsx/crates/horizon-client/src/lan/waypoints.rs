//! The locator bar for remote players: ServerWaypointManager and
//! WaypointTransmitter's connections, for players (the transmitters vanilla has
//! by default: `waypoint_transmit_range` is 0 for every other entity).
//!
//! Vanilla updates a player's connections each time the player's position
//! changes (`Entity.setPosRaw`, then `ServerChunkCache.move`). The hub sees the
//! position once a tick, after the moves it handled, and updates from there:
//! the same packets, since a client sends one move a tick. Hiding (crouching,
//! invisibility, a carved pumpkin or a mob head worn) changes
//! `waypoint_transmit_range` and tracks or untracks the waypoint, as
//! `LivingEntity.onAttributeUpdated` does; joining or leaving a team remakes the
//! connections with the team's colour (`ServerScoreboard.updatePlayerWaypoint`).
use crate::fast_hash::FxHashMap as HashMap;
use horizon_protocol::packets::play::{
    Clientbound as C, TrackedWaypoint, WaypointId, WaypointTarget, waypoint_operation,
};

/// `Player.createAttributes`: both waypoint ranges.
pub const PLAYER_RANGE: f64 = 6.0e7;
/// `WaypointTransmitter.REALLY_FAR_DISTANCE`.
const REALLY_FAR: f32 = 332.0;
/// Players at which the moves run by receiver on several cores ([`Waypoints::moves_by_receiver`]).
const MOVES_BY_RECEIVER_MIN: usize = 16;
#[cfg(test)]
thread_local! {
    /// Tests: the serial moves loop only, to compare the receivers' rows against.
    static SERIAL_MOVES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
/// `EntityAzimuthConnection.update`: half a degree.
const AZIMUTH_STEP: f32 = 0.008_726_646;

/// Items that hide their wearer (`Waypoint.addHideAttribute` on the head slot).
pub const HIDING_HEADS: [&str; 8] = [
    "minecraft:carved_pumpkin",
    "minecraft:skeleton_skull",
    "minecraft:wither_skeleton_skull",
    "minecraft:player_head",
    "minecraft:zombie_head",
    "minecraft:creeper_head",
    "minecraft:dragon_head",
    "minecraft:piglin_head",
];

/// A team colour as the locator bar draws it (`Waypoint.Icon.cloneAndAssignStyle`:
/// `TeamColor.rgb()`, black lightened so it shows).
pub fn team_rgb(color: &str) -> Option<i32> {
    let rgb = match color {
        "black" => return Some(-13_619_152),
        "dark_blue" => 0x0000AA,
        "dark_green" => 0x00AA00,
        "dark_aqua" => 0x00AAAA,
        "dark_red" => 0xAA0000,
        "dark_purple" => 0xAA00AA,
        "gold" => 0xFFAA00,
        "gray" => 0xAAAAAA,
        "dark_gray" => 0x555555,
        "blue" => 0x5555FF,
        "green" => 0x55FF55,
        "aqua" => 0x55FFFF,
        "red" => 0xFF5555,
        "light_purple" => 0xFF55FF,
        "yellow" => 0xFFFF55,
        "white" => 0xFFFFFF,
        _ => return None,
    };
    Some(rgb)
}

/// Waypoint.Icon.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    pub style: std::borrow::Cow<'static, str>,
    pub color: Option<i32>,
}

impl Default for Icon {
    fn default() -> Self {
        Self {
            style: std::borrow::Cow::Borrowed(DEFAULT_STYLE),
            color: None,
        }
    }
}

/// `WaypointStyleAssets.DEFAULT`.
pub const DEFAULT_STYLE: &str = "minecraft:default";

/// A style name as a waypoint carries it: the default borrowed, any other owned.
pub fn style_name(name: &str) -> std::borrow::Cow<'static, str> {
    if name == DEFAULT_STYLE {
        std::borrow::Cow::Borrowed(DEFAULT_STYLE)
    } else {
        std::borrow::Cow::Owned(name.to_owned())
    }
}

/// A player as the waypoint manager sees them this tick.
#[derive(Clone, Debug)]
pub struct Player {
    pub uuid: u128,
    /// The connection that receives the player's locator bar (None: the host,
    /// whose own game draws it).
    pub conn: Option<u32>,
    pub position: [f64; 3],
    pub spectator: bool,
    /// `waypoint_transmit_range`: 0 while hidden.
    pub transmit_range: f64,
    pub receive_range: f64,
    /// `Entity.firstTick`: the player has not ticked since joining.
    pub first_tick: bool,
    /// The chunk view radius (`ChunkMap.getPlayerViewDistance`).
    pub view_distance: i32,
    /// The player's team and its colour (`TeamColor.rgb`).
    pub team: Option<(String, Option<i32>)>,
    /// The icon `/waypoint modify` set.
    pub icon: Icon,
}

impl Player {
    fn block(&self) -> [i32; 3] {
        self.position.map(|v| v.floor() as i32)
    }
    fn chunk(&self) -> [i32; 2] {
        let [x, _, z] = self.block();
        [x >> 4, z >> 4]
    }
    /// `Entity.distanceTo`.
    fn distance_to(&self, other: &Player) -> f32 {
        let d2: f64 = (0..3)
            .map(|k| (self.position[k] - other.position[k]).powi(2))
            .sum();
        d2.sqrt() as f32
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Block([i32; 3]),
    Chunk([i32; 2]),
    Azimuth(f32),
}

#[derive(Clone, Debug)]
struct Connection {
    kind: Kind,
    icon: Icon,
}

/// What the manager remembers of a player between ticks.
struct Memo {
    position: [f64; 3],
    /// The centre of the player's ChunkTrackingView (`ChunkMap.move`).
    view_center: [i32; 2],
    transmitting: bool,
    team: Option<(String, Option<i32>)>,
}

/// ServerWaypointManager for one level.
#[derive(Default)]
pub struct Waypoints {
    /// The transmitters, in the order they were tracked.
    waypoints: Vec<u128>,
    /// The receivers, in the order they joined.
    players: Vec<u128>,
    /// Connections by receiver, then source.
    connections: HashMap<u128, Row>,
    memo: HashMap<u128, Memo>,
    /// The `locator_bar` game rule as last seen.
    enabled: Option<bool>,
    /// HorizonsX: this tick's players held by a late region: their own connections are left as they are
    /// ([`Self::tick_level_by_conn_frozen`]).
    frozen: crate::fast_hash::FxHashSet<u128>,
}

/// One receiver's connections in the order they were made, with each source's place (HorizonsX: a
/// moving player's update looked each one up by a scan of the row, a cube of the player count a tick).
#[derive(Default)]
struct Row {
    list: Vec<(u128, Connection)>,
    at: HashMap<u128, usize>,
}
impl Row {
    fn find(&self, source: u128) -> Option<usize> {
        self.at.get(&source).copied()
    }
    fn push(&mut self, source: u128, c: Connection) {
        self.at.insert(source, self.list.len());
        self.list.push((source, c));
    }
    fn remove(&mut self, i: usize) -> (u128, Connection) {
        let gone = self.list.remove(i);
        self.at.remove(&gone.0);
        for (k, (source, _)) in self.list.iter().enumerate().skip(i) {
            self.at.insert(*source, k);
        }
        gone
    }
}
impl IntoIterator for Row {
    type Item = (u128, Connection);
    type IntoIter = std::vec::IntoIter<(u128, Connection)>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.into_iter()
    }
}

/// One tick's players, and the packets for their connections.
struct Tick<'a> {
    players: &'a HashMap<u128, &'a Player>,
    /// The connections of the players in other levels: one who just left this level is
    /// still told of the waypoints it loses (`removePlayer` disconnects them).
    away: &'a [(u128, u32)],
    view: HashMap<u128, ([i32; 2], i32)>,
    /// HorizonsX: one receiver's view kept beside the map (a receiver's row is worked out with only its own
    /// view: found without a lookup for each of its pairs). Kept equal to `view`'s entry for it.
    mine: Option<(u128, ([i32; 2], i32))>,
    /// Players a respawn replaced this tick: a new ServerPlayer, on its first tick.
    fresh: &'a [u128],
    enabled: bool,
    out: Vec<(u32, TrackedWaypoint)>,
    /// HorizonsX: the moves by receiver's packets after `out`, each receiver's run of packets to one connection
    /// kept as it came (copied into `out` and split up again by player, they were a sixth of the tick at 300).
    grouped: Vec<(u32, Vec<TrackedWaypoint>)>,
}

impl<'a> Tick<'a> {
    fn send(
        &mut self,
        receiver: u128,
        source: u128,
        operation: i32,
        icon: &Icon,
        target: WaypointTarget,
    ) {
        let Some(conn) = self
            .players
            .get(&receiver)
            .and_then(|p| p.conn)
            .or_else(|| {
                self.away
                    .iter()
                    .find(|(uuid, _)| *uuid == receiver)
                    .map(|&(_, conn)| conn)
            })
        else {
            return;
        };
        self.out
            .push((conn, waypoint(source, operation, icon, target)));
    }
    fn connect(&mut self, receiver: u128, source: u128, c: &Connection) {
        let target = match c.kind {
            Kind::Block(p) => WaypointTarget::Block(p),
            Kind::Chunk(p) => WaypointTarget::Chunk(p),
            Kind::Azimuth(a) => WaypointTarget::Azimuth(a),
        };
        self.send(receiver, source, waypoint_operation::TRACK, &c.icon, target);
    }
    /// `ClientboundTrackedWaypointPacket.removeWaypoint`: an empty waypoint with the null icon.
    fn disconnect(&mut self, receiver: u128, source: u128) {
        self.send(
            receiver,
            source,
            waypoint_operation::UNTRACK,
            &Icon::default(),
            WaypointTarget::Empty,
        );
    }
    /// `ChunkTrackingView.isInViewDistance` of the receiver's view.
    fn chunk_visible(&self, [x, z]: [i32; 2], receiver: u128) -> bool {
        let found = match self.mine {
            Some((who, v)) if who == receiver => Some(v),
            _ => self.view.get(&receiver).copied(),
        };
        let Some(([cx, cz], r)) = found else {
            return false;
        };
        let dx = i64::from(((x - cx).abs() - 1).max(0));
        let dz = i64::from(((z - cz).abs() - 1).max(0));
        dx * dx + dz * dz < i64::from(r) * i64::from(r)
    }
    /// `WaypointTransmitter.doesSourceIgnoreReceiver`.
    fn ignores(&self, source: &Player, receiver: &Player) -> bool {
        if receiver.spectator {
            return false;
        }
        if source.spectator {
            return true;
        }
        let range = source.transmit_range.min(receiver.receive_range);
        // HorizonsX: well inside the range (the squared distance below a hair under the range's square) the
        // f32 distance can't reach it either; only near the edge is it worked out as vanilla does.
        let d2: f64 = (0..3).map(|k| (source.position[k] - receiver.position[k]).powi(2)).sum();
        if range > 0.0 && d2 < (range * (1.0 - 1e-5)).powi(2) {
            return false;
        }
        f64::from(source.distance_to(receiver)) >= range
    }
    fn really_far(source: &Player, receiver: &Player) -> bool {
        source.distance_to(receiver) > REALLY_FAR
    }
    /// `EntityAzimuthConnection`: the angle of the receiver seen from the source,
    /// `Mth.atan2` of the offset turned clockwise.
    fn azimuth(source: &Player, receiver: &Player) -> f32 {
        let d = [0, 1, 2].map(|k| receiver.position[k] - source.position[k]);
        horizon_core::geometry::trig::atan2(d[0], -d[2]) as f32
    }
    /// The source and the receiver, if both are in the level this tick.
    fn pair(&self, source: u128, receiver: u128) -> Option<(&'a Player, &'a Player)> {
        Some((*self.players.get(&source)?, *self.players.get(&receiver)?))
    }
    /// `LivingEntity.makeWaypointConnectionWith`.
    fn make(&self, (s, r): (&Player, &Player)) -> Option<Connection> {
        if s.first_tick || self.fresh.contains(&s.uuid) || s.uuid == r.uuid || self.ignores(s, r) {
            return None;
        }
        // Waypoint.Icon.cloneAndAssignStyle: the icon's colour, else the team's.
        let mut icon = s.icon.clone();
        if icon.color.is_none() {
            icon.color = s.team.as_ref().and_then(|t| t.1);
        }
        let kind = if Self::really_far(s, r) {
            Kind::Azimuth(Self::azimuth(s, r))
        } else if !self.chunk_visible(s.chunk(), r.uuid) {
            Kind::Chunk(s.chunk())
        } else {
            Kind::Block(s.block())
        };
        Some(Connection { kind, icon })
    }
    /// `Connection.isBroken` for each kind.
    fn broken(&self, c: &Connection, (s, r): (&Player, &Player)) -> bool {
        let receiver = r.uuid;
        match c.kind {
            Kind::Block(last) => {
                let now = s.block();
                (0..3).map(|k| (now[k] - last[k]).abs()).sum::<i32>() > 1 || self.ignores(s, r)
            }
            Kind::Chunk(last) => {
                let now = s.chunk();
                let moved = (now[0] - last[0]).abs().max((now[1] - last[1]).abs());
                moved > 1 || self.ignores(s, r) || self.chunk_visible(last, receiver)
            }
            Kind::Azimuth(_) => {
                self.ignores(s, r)
                    || self.chunk_visible(s.chunk(), receiver)
                    || !Self::really_far(s, r)
            }
        }
    }
    /// `Connection.update`: the packet when the waypoint moved.
    fn update(&mut self, c: &mut Connection, (s, r): (&Player, &Player)) {
        let target = match &mut c.kind {
            Kind::Block(last) => {
                let now = s.block();
                if now == *last {
                    return;
                }
                *last = now;
                WaypointTarget::Block(now)
            }
            Kind::Chunk(last) => {
                let now = s.chunk();
                if now == *last {
                    return;
                }
                *last = now;
                WaypointTarget::Chunk(now)
            }
            Kind::Azimuth(last) => {
                let now = Self::azimuth(s, r);
                if (now - *last).abs() <= AZIMUTH_STEP {
                    return;
                }
                *last = now;
                WaypointTarget::Azimuth(now)
            }
        };
        if let Some(conn) = r.conn {
            self.out.push((
                conn,
                waypoint(s.uuid, waypoint_operation::UPDATE, &c.icon, target),
            ));
        }
    }
}

/// HorizonsX: the locator bar's packets travel as the waypoint itself (about 80 bytes) and become a
/// `Clientbound` (416) only to be encoded: tens of thousands a tick with a crowd, moved through several lists.
fn waypoint(source: u128, operation: i32, icon: &Icon, target: WaypointTarget) -> TrackedWaypoint {
    TrackedWaypoint {
        operation,
        id: WaypointId::Uuid(source),
        style: icon.style.clone(),
        color: icon.color,
        target,
    }
}


/// `createConnection` on one receiver's row ([`Waypoints::create_connection`]).
fn row_create(row: &mut Row, t: &mut Tick, receiver: u128, source: u128) {
    let pair = t.pair(source, receiver);
    row_create_pair(row, t, receiver, source, pair);
}

/// [`row_create`] with the pair already found (HorizonsX: the moves by receiver look each player up once).
fn row_create_pair<'a>(
    row: &mut Row,
    t: &mut Tick<'a>,
    receiver: u128,
    source: u128,
    pair: Option<(&'a Player, &'a Player)>,
) {
    if receiver == source || !t.enabled {
        return;
    }
    match pair.and_then(|pair| t.make(pair)) {
        Some(c) => {
            t.connect(receiver, source, &c);
            match row.find(source) {
                Some(i) => row.list[i].1 = c,
                None => row.push(source, c),
            }
        }
        None => {
            if let Some(i) = row.find(source) {
                row.remove(i);
                t.disconnect(receiver, source);
            }
        }
    }
}

/// [`Waypoints::update_or_create`] on one receiver's row.
fn row_update_or_create(row: &mut Row, t: &mut Tick, receiver: u128, source: u128) {
    let pair = t.pair(source, receiver);
    row_update_or_create_pair(row, t, receiver, source, pair);
}

/// [`row_update_or_create`] with the pair already found.
fn row_update_or_create_pair<'a>(
    row: &mut Row,
    t: &mut Tick<'a>,
    receiver: u128,
    source: u128,
    pair: Option<(&'a Player, &'a Player)>,
) {
    if receiver == source || !t.enabled {
        return;
    }
    let Some(i) = row.find(source) else {
        return row_create_pair(row, t, receiver, source, pair);
    };
    let c = &mut row.list[i].1;
    if let Some(pair) = pair
        && !t.broken(c, pair)
    {
        t.update(c, pair);
        return;
    }
    match pair.and_then(|pair| t.make(pair)) {
        Some(new) => {
            t.connect(receiver, source, &new);
            *c = new;
        }
        None => {
            t.disconnect(receiver, source);
            row.remove(i);
        }
    }
}

/// HorizonsX: whether another visit of the pair (`receiver`, `source`) this tick, right after one, would find
/// nothing to do. A block waypoint, a chunk waypoint outside the receiver's view, an azimuth on a chunk it
/// doesn't see, or no connection: the visit leaves each as it found it. A chunk waypoint that has just moved
/// into the view becomes a block one on the next visit, and an azimuth on a chunk in view is made again on each
/// visit (vanilla's connections do that too): those are visited.
fn settled(row: &Row, t: &Tick, receiver: u128, source: u128, s: Option<&Player>) -> bool {
    let Some(i) = row.find(source) else {
        return true;
    };
    match row.list[i].1.kind {
        Kind::Block(_) => true,
        Kind::Chunk(last) => !t.chunk_visible(last, receiver),
        Kind::Azimuth(_) => s.is_some_and(|s| !t.chunk_visible(s.chunk(), receiver)),
    }
}

/// A player who moved this tick, as the moves loop of [`Waypoints::tick_level`] handles them.
struct Moved {
    uuid: u128,
    /// `updateWaypoint`: transmitting, past its first tick.
    as_source: bool,
    /// The first `updatePlayer`: receiving, past its first tick.
    first_update: bool,
    /// The view centre after `ChunkMap.move`, when it changed.
    new_view: Option<([i32; 2], i32)>,
    /// The second `updatePlayer`: receiving.
    second_update: bool,
}

impl Waypoints {
    /// HorizonsX: the moves of every player who moved this tick, each receiver's row on whichever core is
    /// free. A receiver's connections only depend on its own view and the players' positions, so the
    /// updates that touch one row run in the same order as the serial loop's: each receiver gets the same
    /// packets in the same order (the order across receivers differs, which no client sees). False when the
    /// tick has something else for the loop (a transmitter shown or hidden, a team change) or few players:
    /// the serial loop runs then.
    fn moves_by_receiver(&mut self, t: &mut Tick, players: &[Player]) -> bool {
        #[cfg(test)]
        if SERIAL_MOVES.with(|s| s.get()) {
            return false;
        }
        if players.len() < MOVES_BY_RECEIVER_MIN || !t.enabled {
            return false;
        }
        let transmit_changes = players.iter().filter(|p| (p.transmit_range > 0.0) != self.memo[&p.uuid].transmitting).count();
        // Diagnostics (HORIZON_WAYPOINT_LOG=1): every 200 ticks, the players and this tick's hides and shows.
        {
            use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
            static TICKS: AtomicU64 = AtomicU64::new(0);
            static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            if *ON.get_or_init(|| std::env::var("HORIZON_WAYPOINT_LOG").is_ok_and(|v| v.trim() == "1"))
                && TICKS.fetch_add(1, Relaxed) % 200 == 0
            {
                let moved = players.iter().filter(|p| self.memo[&p.uuid].position != p.position).count();
                log::info!("waypoints: {} players, {} moved, {} hides or shows this tick", players.len(), moved, transmit_changes);
            }
        }
        // HorizonsX: hiding and showing (crouching, invisibility, a head worn) and team changes are handled here
        // too, in the serial loop's order (each touches every receiver's own row); only a tick with very many
        // of them takes the serial loop. Before, any one of them did, every tick a player crouched: with 300
        // spread players that was most ticks (23 ms of the tick on one thread).
        if transmit_changes > MAX_TOGGLES {
            return false;
        }
        // The serial loop's steps, player by player: the move, then hiding or showing, then a team change.
        let mut steps: Vec<Step> = Vec::new();
        // The waypoints as each hide or show leaves them (`track` adds at the end, `untrack` takes out).
        let mut lists: Vec<Vec<u128>> = vec![self.waypoints.clone()];
        let mut listed: crate::fast_hash::FxHashSet<u128> = self.waypoints.iter().copied().collect();
        for p in players {
            let memo = self.memo.get_mut(&p.uuid).expect("every player has a memo");
            let epoch = lists.len() - 1;
            let mut moved = None;
            if memo.position != p.position {
                memo.position = p.position;
                let new_view = (memo.view_center != p.chunk()).then(|| {
                    memo.view_center = p.chunk();
                    (p.chunk(), p.view_distance)
                });
                let receiving = p.receive_range > 0.0;
                moved = Some(Moved {
                    uuid: p.uuid,
                    as_source: !p.first_tick && memo.transmitting && listed.contains(&p.uuid),
                    first_update: !p.first_tick && receiving,
                    new_view,
                    second_update: receiving,
                });
            }
            let transmitting = p.transmit_range > 0.0;
            let toggle = (transmitting != memo.transmitting).then(|| {
                memo.transmitting = transmitting;
                transmitting
            });
            let team = p.team != memo.team;
            if team {
                memo.team = p.team.clone();
            }
            if let Some(on) = toggle {
                let mut list = lists.last().expect("one list at least").clone();
                if on {
                    if listed.insert(p.uuid) {
                        list.push(p.uuid);
                    }
                } else if listed.remove(&p.uuid) {
                    list.retain(|w| *w != p.uuid);
                }
                lists.push(list);
            }
            if moved.is_some() || toggle.is_some() || team {
                steps.push(Step { uuid: p.uuid, moved, toggle, team, epoch });
            }
        }
        self.waypoints = lists.last().expect("one list at least").clone();
        if steps.is_empty() {
            return true;
        }
        let toggled = lists.len() > 1;
        let steps = &steps;
        let frozen = &self.frozen;
        let work: Vec<(u128, Option<Row>)> = self
            .players
            .iter()
            .filter(|r| !frozen.contains(r))
            .map(|&r| (r, self.connections.remove(&r)))
            .collect();
        let (players_by_uuid, away, fresh, view) = (t.players, t.away, t.fresh, &t.view);
        // Each player found once for every receiver (each pair looked both up: two of a pair's three lookups).
        let step_players: Vec<Option<&Player>> = steps.iter().map(|st| players_by_uuid.get(&st.uuid).copied()).collect();
        // HorizonsX: a pair of players who both moved is gone through once. Each one's update leaves the
        // connection up to date with both final positions, so the later of the two visits (the receiver's pass
        // over every waypoint, or the source's own move) finds nothing to send when the connection is settled:
        // it is skipped. Only the receiver's second pass (after its view moved) still goes over everyone. Same
        // packets, same order. Not in a tick with a hide or show: a waypoint shown after a receiver's pass would
        // be missed by the skip.
        let moved_at: HashMap<u128, usize> = steps
            .iter()
            .enumerate()
            .filter(|(_, st)| st.moved.as_ref().is_some_and(|m| m.as_source))
            .map(|(k, st)| (st.uuid, k))
            .collect();
        let lists_players: Vec<Vec<(u128, Option<&Player>, Option<usize>)>> = lists
            .iter()
            .map(|list| {
                list.iter()
                    .map(|&w| (w, players_by_uuid.get(&w).copied(), moved_at.get(&w).copied()))
                    .collect()
            })
            .collect();
        let (step_players, lists_players) = (&step_players, &lists_players);
        let moves = steps.iter().filter(|st| st.moved.is_some()).count();
        let rows_started = crate::spans::enabled().then(std::time::Instant::now);
        let done = crate::v2::map_batches(work, 4, |(receiver, row)| {
            let existed = row.is_some();
            let mut row = row.unwrap_or_default();
            let mut own = Tick {
                players: players_by_uuid,
                away,
                view: view.get(&receiver).map(|v| (receiver, *v)).into_iter().collect(),
                mine: view.get(&receiver).map(|v| (receiver, *v)),
                fresh,
                enabled: true,
                // Room for an update from every player who moved (grown a packet at a time, it was a quarter of
                // the helpers' time at 400 players).
                out: Vec::with_capacity(moves),
                grouped: Vec::new(),
            };
            let me = players_by_uuid.get(&receiver).copied();
            // Where this receiver's own move is, and whether a pass over every waypoint runs there.
            let mine = steps.iter().position(|st| st.uuid == receiver && st.moved.is_some());
            let full_pass = !toggled
                && mine.is_some_and(|k| {
                    let m = steps[k].moved.as_ref().expect("a move");
                    m.first_update || m.second_update
                });
            for (k, (st, &source)) in steps.iter().zip(step_players).enumerate() {
                if let Some(m) = &st.moved {
                    if m.as_source {
                        let later = full_pass && mine.is_some_and(|at| k > at);
                        if !(later && settled(&row, &own, receiver, st.uuid, source)) {
                            row_update_or_create_pair(&mut row, &mut own, receiver, st.uuid, source.zip(me));
                        }
                    }
                    if st.uuid == receiver {
                        let list = &lists_players[st.epoch];
                        if m.first_update {
                            for &(w, source, moved) in list {
                                // A source whose own move came first already brought this pair up to date.
                                if !toggled && moved.is_some_and(|at| at < k) && settled(&row, &own, receiver, w, source) {
                                    continue;
                                }
                                row_update_or_create_pair(&mut row, &mut own, receiver, w, source.zip(me));
                            }
                        }
                        let new_view = m.new_view.is_some();
                        if let Some(v) = m.new_view {
                            own.view.insert(receiver, v);
                            own.mine = Some((receiver, v));
                        }
                        // The second updatePlayer. HorizonsX: a connection the first pass left settled (see
                        // `settled`) sends nothing on this one and is skipped; the others are visited (a chunk
                        // waypoint that has just come into view turns into a block one here, as vanilla's does).
                        if m.second_update {
                            let first_ran = m.first_update && !new_view;
                            for &(w, source, _) in list {
                                if first_ran && settled(&row, &own, receiver, w, source) {
                                    continue;
                                }
                                row_update_or_create_pair(&mut row, &mut own, receiver, w, source.zip(me));
                            }
                        }
                    }
                }
                // LivingEntity.onAttributeUpdated: shown (trackWaypoint's createConnection) or hidden
                // (untrackWaypoint), for this receiver.
                if let Some(on) = st.toggle
                    && st.uuid != receiver
                {
                    if on {
                        row_create_pair(&mut row, &mut own, receiver, st.uuid, source.zip(me));
                    } else if let Some(i) = row.find(st.uuid) {
                        row.remove(i);
                        own.disconnect(receiver, st.uuid);
                    }
                }
                // ServerScoreboard: the connection made again with the team's colour.
                if st.team && st.uuid != receiver {
                    row_create_pair(&mut row, &mut own, receiver, st.uuid, source.zip(me));
                }
            }
            // Grouped here, on this core: each run of packets to one connection as one list (the main thread
            // only links the lists; moving every packet out of its pair there was most of its share).
            let mut grouped: Vec<(u32, Vec<TrackedWaypoint>)> = Vec::new();
            let mut out = own.out.into_iter().peekable();
            while let Some((conn, packet)) = out.next() {
                let mut run = vec![packet];
                while let Some((_, p)) = out.next_if(|(c, _)| *c == conn) {
                    run.push(p);
                }
                grouped.push((conn, run));
            }
            (receiver, row, existed, grouped)
        });
        if let Some(t) = rows_started {
            crate::spans::record("wp_rows", t.elapsed());
        }
        for (receiver, row, existed, runs) in done {
            if existed || !row.list.is_empty() {
                self.connections.insert(receiver, row);
            }
            for (conn, run) in runs {
                match t.grouped.last_mut() {
                    Some((last, prev)) if *last == conn => prev.extend(run),
                    _ => t.grouped.push((conn, run)),
                }
            }
        }
        for st in steps {
            if let Some(v) = st.moved.as_ref().and_then(|m| m.new_view) {
                t.view.insert(st.uuid, v);
            }
        }
        true
    }
}

/// HorizonsX: the hides and shows at most in one tick that the moves by receiver go through ([`Waypoints::moves_by_receiver`]).
const MAX_TOGGLES: usize = 64;

/// One player's part of the serial loop, in its order: its move, then its hiding or showing, then its team.
struct Step {
    uuid: u128,
    moved: Option<Moved>,
    /// Shown (true) or hidden (false) this tick.
    toggle: Option<bool>,
    team: bool,
    /// The waypoint list (of `lists`) as it stands at this step's move.
    epoch: usize,
}

impl Waypoints {
    fn put(&mut self, receiver: u128, source: u128, c: Connection) {
        let row = self.connections.entry(receiver).or_default();
        match row.find(source) {
            Some(i) => row.list[i].1 = c,
            None => row.push(source, c),
        }
    }
    fn take(&mut self, receiver: u128, source: u128) -> Option<Connection> {
        let row = self.connections.get_mut(&receiver)?;
        let i = row.find(source)?;
        Some(row.remove(i).1)
    }

    /// `createConnection`.
    fn create_connection(&mut self, t: &mut Tick, receiver: u128, source: u128) {
        if receiver == source || !t.enabled {
            return;
        }
        match t.pair(source, receiver).and_then(|pair| t.make(pair)) {
            Some(c) => {
                t.connect(receiver, source, &c);
                self.put(receiver, source, c);
            }
            None => {
                if self.take(receiver, source).is_some() {
                    t.disconnect(receiver, source);
                }
            }
        }
    }
    /// The receiver's connection to the source updated in its place
    /// (`updateConnection`), or made if it has none (`createConnection`).
    fn update_or_create(&mut self, t: &mut Tick, receiver: u128, source: u128) {
        if receiver == source || !t.enabled {
            return;
        }
        let Some(row) = self.connections.get_mut(&receiver) else {
            return self.create_connection(t, receiver, source);
        };
        let Some(i) = row.find(source) else {
            return self.create_connection(t, receiver, source);
        };
        let c = &mut row.list[i].1;
        // A connection to a player who isn't in the level is broken.
        let pair = t.pair(source, receiver);
        if let Some(pair) = pair
            && !t.broken(c, pair)
        {
            t.update(c, pair);
            return;
        }
        match pair.and_then(|pair| t.make(pair)) {
            Some(new) => {
                t.connect(receiver, source, &new);
                *c = new;
            }
            None => {
                t.disconnect(receiver, source);
                row.remove(i);
            }
        }
    }
    /// `trackWaypoint`.
    fn track(&mut self, t: &mut Tick, source: u128) {
        if !self.waypoints.contains(&source) {
            self.waypoints.push(source);
        }
        for receiver in self.players.clone() {
            self.create_connection(t, receiver, source);
        }
    }
    /// `updateWaypoint`.
    fn update_waypoint(&mut self, t: &mut Tick, source: u128) {
        if !self.waypoints.contains(&source) {
            return;
        }
        // Connections change; the players don't.
        for k in 0..self.players.len() {
            self.update_or_create(t, self.players[k], source);
        }
    }
    /// `untrackWaypoint`.
    fn untrack(&mut self, t: &mut Tick, source: u128) {
        for receiver in self.players.clone() {
            if self.take(receiver, source).is_some() {
                t.disconnect(receiver, source);
            }
        }
        self.waypoints.retain(|w| *w != source);
    }
    /// `addPlayer`.
    fn add_player(&mut self, t: &mut Tick, player: u128, transmitting: bool) {
        if !self.players.contains(&player) {
            self.players.push(player);
        }
        for source in self.waypoints.clone() {
            self.create_connection(t, player, source);
        }
        if transmitting {
            self.track(t, player);
        }
    }
    /// `updatePlayer`.
    fn update_player(&mut self, t: &mut Tick, player: u128) {
        // Connections change; the waypoints don't.
        for k in 0..self.waypoints.len() {
            self.update_or_create(t, player, self.waypoints[k]);
        }
    }
    /// `removePlayer`.
    fn remove_player(&mut self, t: &mut Tick, player: u128) {
        for (source, _) in self.connections.remove(&player).unwrap_or_default() {
            t.disconnect(player, source);
        }
        self.untrack(t, player);
        self.players.retain(|p| *p != player);
    }
    /// `breakAllConnections`.
    fn break_all(&mut self, t: &mut Tick) {
        for receiver in self.players.clone() {
            for (source, _) in self.connections.remove(&receiver).unwrap_or_default() {
                t.disconnect(receiver, source);
            }
        }
        self.connections.clear();
    }

    /// One tick: players who left, joined, moved, hid or changed team, and the
    /// `locator_bar` game rule. `respawned` players are removed and added again,
    /// as `PlayerList.respawn` replaces the ServerPlayer. Returns the packets for
    /// each connection.
    pub fn tick(
        &mut self,
        players: &[Player],
        respawned: &[u128],
        locator_bar: bool,
    ) -> Vec<(u32, C)> {
        self.tick_level(players, &[], respawned, locator_bar)
    }

    /// [`Self::tick`] for one level of several: `away` are the connections of the players
    /// in the other levels, for the untracks owed to one who has just left this one.
    pub fn tick_level(
        &mut self,
        players: &[Player],
        away: &[(u128, u32)],
        respawned: &[u128],
        locator_bar: bool,
    ) -> Vec<(u32, C)> {
        let (out, grouped) = self.tick_parts(players, away, respawned, locator_bar);
        let mut all: Vec<(u32, C)> = out.into_iter().map(|(conn, w)| (conn, C::TrackedWaypoint(w))).collect();
        for (conn, run) in grouped {
            all.extend(run.into_iter().map(|w| (conn, C::TrackedWaypoint(w))));
        }
        all
    }

    /// [`Self::tick_level_by_conn`] with `frozen` players (held by a late region, standing where they were last
    /// seen): their own connections are left alone this tick (their packets couldn't reach them), the others'
    /// to them are kept.
    pub fn tick_level_by_conn_frozen(
        &mut self,
        players: &[Player],
        away: &[(u128, u32)],
        respawned: &[u128],
        locator_bar: bool,
        frozen: &crate::fast_hash::FxHashSet<u128>,
    ) -> Vec<(u32, Vec<TrackedWaypoint>)> {
        self.frozen.clone_from(frozen);
        let out = self.tick_level_by_conn(players, away, respawned, locator_bar);
        self.frozen.clear();
        out
    }

    /// HorizonsX: [`Self::tick_level`]'s packets by connection, each one's in the order `tick_level` gives them.
    pub fn tick_level_by_conn(
        &mut self,
        players: &[Player],
        away: &[(u128, u32)],
        respawned: &[u128],
        locator_bar: bool,
    ) -> Vec<(u32, Vec<TrackedWaypoint>)> {
        let (out, grouped) = self.tick_parts(players, away, respawned, locator_bar);
        let mut by_conn: Vec<(u32, Vec<TrackedWaypoint>)> = Vec::new();
        let mut at: HashMap<u32, usize> = HashMap::default();
        for (conn, packet) in out {
            let i = *at.entry(conn).or_insert_with(|| {
                by_conn.push((conn, Vec::new()));
                by_conn.len() - 1
            });
            by_conn[i].1.push(packet);
        }
        for (conn, run) in grouped {
            match at.get(&conn) {
                Some(&i) => by_conn[i].1.extend(run),
                None => {
                    at.insert(conn, by_conn.len());
                    by_conn.push((conn, run));
                }
            }
        }
        by_conn
    }

    fn tick_parts(
        &mut self,
        players: &[Player],
        away: &[(u128, u32)],
        respawned: &[u128],
        locator_bar: bool,
    ) -> (Vec<(u32, TrackedWaypoint)>, Vec<(u32, Vec<TrackedWaypoint>)>) {
        let by_uuid: HashMap<u128, &Player> = players.iter().map(|p| (p.uuid, p)).collect();
        let mut t = Tick {
            players: &by_uuid,
            away,
            view: HashMap::default(),
            mine: None,
            fresh: respawned,
            enabled: locator_bar,
            out: Vec::new(),
            grouped: Vec::new(),
        };
        // Vanilla reads the rule when it connects; changing it breaks or remakes
        // every connection (MinecraftServer.onGameRuleChanged).
        let was = self.enabled.replace(locator_bar);
        for (uuid, memo) in &self.memo {
            if let Some(p) = t.players.get(uuid) {
                t.view.insert(*uuid, (memo.view_center, p.view_distance));
            }
        }
        // Players who left, and the ServerPlayer a respawn replaced.
        let gone: Vec<u128> = self
            .players
            .iter()
            .copied()
            .filter(|uuid| !t.players.contains_key(uuid) || respawned.contains(uuid))
            .collect();
        for uuid in gone {
            self.remove_player(&mut t, uuid);
            self.memo.remove(&uuid);
        }
        if was == Some(true) && !locator_bar {
            self.break_all(&mut t);
        }
        // Players who joined (PlayerList.placeNewPlayer: addPlayer).
        for p in players {
            if self.memo.contains_key(&p.uuid) {
                continue;
            }
            let center = p.chunk();
            t.view.insert(p.uuid, (center, p.view_distance));
            let transmitting = p.transmit_range > 0.0;
            self.memo.insert(
                p.uuid,
                Memo {
                    position: p.position,
                    view_center: center,
                    transmitting,
                    team: p.team.clone(),
                },
            );
            if p.receive_range > 0.0 {
                self.add_player(&mut t, p.uuid, transmitting);
            } else if transmitting {
                self.track(&mut t, p.uuid);
            }
        }
        if was == Some(false) && locator_bar {
            for player in self.players.clone() {
                self.update_player(&mut t, player);
            }
        }
        if self.moves_by_receiver(&mut t, players) {
            return (t.out, t.grouped);
        }
        for p in players {
            let memo = &self.memo[&p.uuid];
            // Entity.setPosRaw, then ServerChunkCache.move.
            if memo.position != p.position {
                let moved_view = memo.view_center != p.chunk();
                let transmitting = memo.transmitting;
                self.memo.get_mut(&p.uuid).unwrap().position = p.position;
                if !p.first_tick {
                    if transmitting {
                        self.update_waypoint(&mut t, p.uuid);
                    }
                    if p.receive_range > 0.0 {
                        self.update_player(&mut t, p.uuid);
                    }
                }
                if moved_view {
                    self.memo.get_mut(&p.uuid).unwrap().view_center = p.chunk();
                    t.view.insert(p.uuid, (p.chunk(), p.view_distance));
                }
                if p.receive_range > 0.0 {
                    self.update_player(&mut t, p.uuid);
                }
            }
            // LivingEntity.onAttributeUpdated(WAYPOINT_TRANSMIT_RANGE).
            let transmitting = p.transmit_range > 0.0;
            if transmitting != self.memo[&p.uuid].transmitting {
                self.memo.get_mut(&p.uuid).unwrap().transmitting = transmitting;
                if transmitting {
                    self.track(&mut t, p.uuid);
                } else {
                    self.untrack(&mut t, p.uuid);
                }
            }
            // ServerScoreboard: a team joined, left or recoloured remakes the connections.
            if p.team != self.memo[&p.uuid].team {
                self.memo.get_mut(&p.uuid).unwrap().team = p.team.clone();
                for receiver in self.players.clone() {
                    self.create_connection(&mut t, receiver, p.uuid);
                }
            }
        }
        (t.out, t.grouped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HorizonsX: the moves by receiver (on every core) give each connection the same packets, in the same
    /// order, as the serial loop: twenty players near and far apart, moving across blocks, chunks and
    /// views for several ticks.
    #[test]
    fn moves_by_receiver_match_the_serial_loop() {
        fn run(serial: bool) -> Vec<Vec<(u32, String)>> {
            SERIAL_MOVES.with(|s| s.set(serial));
            let mut w = Waypoints::default();
            let mut players: Vec<Player> = (0..20u32)
                .map(|i| {
                    let x = f64::from(i % 5) * if i % 3 == 0 { 400.0 } else { 37.0 };
                    let z = f64::from(i / 5) * if i % 2 == 0 { 90.0 } else { 15.0 };
                    player(100 + u128::from(i), 10 + i, [x, 64.0, z])
                })
                .collect();
            let mut ticks = Vec::new();
            for tick in 0..8u32 {
                let out = w.tick(&players, &[], true);
                let mut by_conn: Vec<(u32, String)> = out.iter().map(|(c, p)| (*c, format!("{p:?}"))).collect();
                by_conn.sort_by_key(|(c, _)| *c);
                ticks.push(by_conn);
                for (i, p) in players.iter_mut().enumerate() {
                    let step = f64::from((tick * 7 + i as u32 * 3) % 11) * 1.7;
                    if i % 4 != 0 {
                        p.position[0] += step;
                        p.position[2] -= step * 0.6;
                    }
                }
            }
            SERIAL_MOVES.with(|s| s.set(false));
            ticks
        }
        let serial = run(true);
        let parallel = run(false);
        assert_eq!(serial.len(), parallel.len());
        for (k, (a, b)) in serial.iter().zip(&parallel).enumerate() {
            assert_eq!(a, b, "tick {k}");
        }
        assert!(serial.iter().map(Vec::len).sum::<usize>() > 100, "the players saw each other move");
    }

    /// HorizonsX: a crowd: sixty players a few chunks across, walking over chunk borders (views moving) for
    /// twenty ticks; some far off (azimuths), a few standing, some crouching (hidden) and changing team. The moves by receiver (with the settled pairs
    /// skipped) give every connection the serial loop's packets in the same order.
    #[test]
    fn a_walking_crowd_matches_the_serial_loop() {
        fn run(serial: bool) -> Vec<Vec<(u32, String)>> {
            SERIAL_MOVES.with(|s| s.set(serial));
            let mut w = Waypoints::default();
            let mut players: Vec<Player> = (0..60u32)
                .map(|i| {
                    let far = if i % 13 == 0 { 700.0 } else { 0.0 };
                    let x = f64::from(i % 8) * 9.5 + far;
                    let z = f64::from(i / 8) * 7.25 - far;
                    let mut p = player(1000 + u128::from(i), 200 + i, [x, 64.0, z]);
                    p.view_distance = 2 + (i % 3) as i32;
                    p
                })
                .collect();
            let mut ticks = Vec::new();
            for tick in 0..20u32 {
                let out = w.tick(&players, &[], true);
                let mut by_conn: Vec<(u32, String)> = out.iter().map(|(c, p)| (*c, format!("{p:?}"))).collect();
                by_conn.sort_by_key(|(c, _)| *c);
                ticks.push(by_conn);
                for (i, p) in players.iter_mut().enumerate() {
                    // Some crouch now and then (hidden), and some join or leave a team.
                    let r = (tick * 7 + i as u32 * 13) % 17;
                    p.transmit_range = if r == 0 || (r == 1 && tick % 2 == 0) { 0.0 } else { PLAYER_RANGE };
                    if (tick + i as u32) % 23 == 0 {
                        p.team = if p.team.is_some() { None } else { Some((format!("t{}", i % 3), Some(0x55ff55))) };
                    }
                    if i % 7 == 3 {
                        continue;
                    }
                    let step = f64::from((tick * 11 + i as u32 * 5) % 13) * 1.9 - 6.0;
                    p.position[0] += step;
                    p.position[2] += step * 0.7 - 1.5;
                }
            }
            SERIAL_MOVES.with(|s| s.set(false));
            ticks
        }
        let serial = run(true);
        let parallel = run(false);
        for (k, (a, b)) in serial.iter().zip(&parallel).enumerate() {
            assert_eq!(a, b, "tick {k}");
        }
        assert!(serial.iter().map(Vec::len).sum::<usize>() > 2000, "the crowd saw itself move");
    }

    /// HorizonsX: `tick_level_by_conn` gives each connection the packets `tick_level` does, in the same order:
    /// the serial loop's ticks (joins) and the moves by receiver's alike.
    #[test]
    fn packets_by_conn_match_the_flat_list() {
        let start: Vec<Player> = (0..24u32)
            .map(|i| player(500 + u128::from(i), 40 + i, [f64::from(i % 6) * 21.0, 64.0, f64::from(i / 6) * 13.0]))
            .collect();
        let (mut flat, mut grouped) = (Waypoints::default(), Waypoints::default());
        let mut players = start.clone();
        let mut sent = 0;
        for tick in 0..10u32 {
            // Some leave and come back, so the serial loop's packets come before the moves'.
            let here: Vec<Player> = players.iter().filter(|p| tick % 4 != 3 || p.uuid % 5 != 0).cloned().collect();
            let a = flat.tick_level(&here, &[], &[], true);
            let b = grouped.tick_level_by_conn(&here, &[], &[], true);
            let mut conns: Vec<u32> = a.iter().map(|(c, _)| *c).collect();
            conns.sort_unstable();
            conns.dedup();
            assert_eq!(conns.len(), b.len(), "tick {tick}: one list per connection");
            for (conn, run) in &b {
                let want: Vec<String> = a.iter().filter(|(c, _)| c == conn).map(|(_, p)| format!("{p:?}")).collect();
                let got: Vec<String> = run.iter().map(|w| format!("{:?}", C::TrackedWaypoint(w.clone()))).collect();
                assert_eq!(want, got, "tick {tick}, connection {conn}");
            }
            sent += a.len();
            for (i, p) in players.iter_mut().enumerate() {
                let step = f64::from((tick * 5 + i as u32 * 7) % 9) * 1.3;
                p.position[0] += step;
                p.position[2] += step * 0.4;
            }
        }
        assert!(sent > 200, "the players saw each other move");
    }

    fn player(uuid: u128, conn: u32, position: [f64; 3]) -> Player {
        Player {
            uuid,
            conn: Some(conn),
            position,
            spectator: false,
            transmit_range: PLAYER_RANGE,
            receive_range: PLAYER_RANGE,
            first_tick: false,
            view_distance: 4,
            team: None,
            icon: Icon::default(),
        }
    }

    /// The packets to `conn`, as (operation, target).
    fn to(out: &[(u32, C)], conn: u32) -> Vec<(i32, WaypointTarget)> {
        out.iter()
            .filter(|(c, _)| *c == conn)
            .map(|(_, p)| match p {
                C::TrackedWaypoint(w) => (w.operation, w.target),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_new_player_shows_once_it_moves() {
        let mut w = Waypoints::default();
        let a = player(1, 10, [0.5, 100.0, 0.5]);
        let mut b = player(2, 20, [5.5, 100.0, 0.5]);
        b.first_tick = true;
        w.tick(std::slice::from_ref(&a), &[], true);
        let out = w.tick(&[a.clone(), b.clone()], &[], true);
        // B gets A at once; A sees nothing of B while B hasn't ticked.
        assert_eq!(to(&out, 20), [(0, WaypointTarget::Block([0, 100, 0]))]);
        assert!(to(&out, 10).is_empty());
        b.first_tick = false;
        assert!(w.tick(&[a.clone(), b.clone()], &[], true).is_empty());
        b.position[0] += 1.0;
        let out = w.tick(&[a.clone(), b.clone()], &[], true);
        assert_eq!(to(&out, 10), [(0, WaypointTarget::Block([6, 100, 0]))]);
        // One block: an update. Three: a new connection. Out of view: a chunk.
        b.position[0] += 1.0;
        let out = w.tick(&[a.clone(), b.clone()], &[], true);
        assert_eq!(to(&out, 10), [(2, WaypointTarget::Block([7, 100, 0]))]);
        b.position[0] += 3.0;
        let out = w.tick(&[a.clone(), b.clone()], &[], true);
        assert_eq!(to(&out, 10), [(0, WaypointTarget::Block([10, 100, 0]))]);
        b.position[0] = 100.5;
        let out = w.tick(&[a.clone(), b.clone()], &[], true);
        assert_eq!(to(&out, 10), [(0, WaypointTarget::Chunk([6, 0]))]);
        // Really far: an angle.
        b.position[0] = 1000.5;
        let out = w.tick(&[a.clone(), b.clone()], &[], true);
        assert!(matches!(
            to(&out, 10)[..],
            [(0, WaypointTarget::Azimuth(_))]
        ));
        // Hidden: untracked; the rule off: every connection broken.
        b.transmit_range = 0.0;
        let out = w.tick(&[a.clone(), b.clone()], &[], true);
        assert_eq!(to(&out, 10), [(1, WaypointTarget::Empty)]);
        let out = w.tick(&[a.clone(), b.clone()], &[], false);
        assert_eq!(to(&out, 20), [(1, WaypointTarget::Empty)]);
        assert!(to(&out, 10).is_empty());
        // Leaving untracks what was left.
        let out = w.tick(&[a.clone(), b.clone()], &[], true);
        assert!(matches!(
            to(&out, 20)[..],
            [(0, WaypointTarget::Azimuth(_))]
        ));
        let out = w.tick(&[b.clone()], &[], true);
        assert_eq!(to(&out, 20), [(1, WaypointTarget::Empty)]);
    }

    #[test]
    fn a_player_leaving_the_level_loses_its_waypoints() {
        let (a, b) = (
            player(1, 10, [0.5, 64.0, 0.5]),
            player(2, 20, [4.5, 64.0, 0.5]),
        );
        let mut w = Waypoints::default();
        w.tick(&[a.clone(), b.clone()], &[], true);
        let mut moved = b.clone();
        moved.position = [5.5, 64.0, 0.5];
        w.tick(&[a.clone(), moved], &[], true);
        // B goes to another level: A loses B, and B (reached through `away`) loses A.
        let out = w.tick_level(std::slice::from_ref(&a), &[(2, 20)], &[], true);
        assert_eq!(
            to(&out, 10),
            vec![(waypoint_operation::UNTRACK, WaypointTarget::Empty)]
        );
        assert_eq!(
            to(&out, 20),
            vec![(waypoint_operation::UNTRACK, WaypointTarget::Empty)]
        );
    }

    #[test]
    fn team_colours() {
        assert_eq!(team_rgb("red"), Some(0xFF5555));
        assert_eq!(team_rgb("black"), Some(0xFF30_3030_u32 as i32));
        assert_eq!(team_rgb("reset"), None);
    }
}
