//! The join queue for the network's game servers (not the hub: everyone walks into the hub freely).
//!
//! When a player asks to go to a queued server (the hub's selector or an NPC asks the proxy: `SEND`), the proxy lets
//! them straight in only while nobody waits for that server and it can take them now; otherwise they get a place in
//! its line, the hub sends them back through the proxy, and the proxy holds them on the queue screen (the game's
//! configuration stage: no world, no server) until their turn, when it sends them on to the server.
//!
//! Turns come at a steady rate: a network-wide rate (gentler for the first minutes after a start), each server's own
//! rate and player cap, and no one at all for a server that reports low TPS, long ticks or high memory, or that goes
//! quiet while it has players. An admitted player gets a ticket for that server, which their next login uses.
//!
//! Tiers (from the player's LuckPerms groups and permissions, read from LuckPerms' JSON storage): bypass tiers skip
//! the line (staff); the others share each server's turns by weight (smooth weighted round robin), so a weight-6
//! tier moves six places for each one a weight-1 tier moves, and nobody stands still.
//!
//! The lines survive a proxy restart (saved to a file); a player who drops out keeps their place for a grace period.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---------------------------------------------------------------- configuration

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct QueueConfig {
    pub enabled: bool,
    /// The servers with a line (never the hub).
    #[serde(alias = "targets")]
    pub queued: Vec<String>,
    /// Players waiting for one server at most; more are told to try again later.
    pub max_size: usize,
    /// Turns across the network: `admit` players every `every_seconds`, spread out (at most `burst` at once).
    pub admit: f64,
    pub every_seconds: f64,
    pub burst: f64,
    /// For the first minutes after the proxy starts: gentler.
    pub startup_minutes: f64,
    pub startup_admit: f64,
    pub startup_every_seconds: f64,
    /// A player who leaves the network keeps their place this long.
    pub grace_seconds: u64,
    /// An admitted player has this long to arrive.
    pub ticket_seconds: u64,
    /// Line up even when nobody waits and there's room (testing, events).
    pub always_queue: bool,
    pub state_file: String,
    /// LuckPerms JSON storage folders (`.../plugins/LuckPerms/json-storage`), searched in order.
    pub luckperms: Vec<String>,
    /// The queue screen: how often it updates, the store's address for "Get Priority" (empty: no button), and the
    /// address a player is sent back through (empty: the one they typed).
    pub refresh_seconds: f64,
    pub store_url: String,
    pub transfer_host: String,
    pub transfer_port: u16,
    /// The network's pack, sent in the waiting room before the screen (a transfer drops server packs, and the pack
    /// is what hides the dialog's warning button). Same id and sha1 as the hub's, so the game takes it from its
    /// cache. Empty url: none.
    pub pack_url: String,
    pub pack_sha1: String,
    pub pack_id: String,
    /// The queue world's recording (2b2t style, owner 11-08); empty or missing: the queue screen.
    pub world_file: String,
    pub servers: HashMap<String, ServerLimits>,
    /// Best first. A tier with no groups, no permission and no bypass is the default (the last such one).
    pub tiers: Vec<Tier>,
}

impl Default for QueueConfig {
    fn default() -> Self {
        QueueConfig {
            enabled: false,
            queued: Vec::new(),
            max_size: 5000,
            admit: 10.0,
            every_seconds: 10.0,
            burst: 2.0,
            startup_minutes: 5.0,
            startup_admit: 4.0,
            startup_every_seconds: 10.0,
            grace_seconds: 300,
            ticket_seconds: 60,
            always_queue: false,
            state_file: "queue-state.json".into(),
            luckperms: Vec::new(),
            refresh_seconds: 2.0,
            store_url: String::new(),
            transfer_host: String::new(),
            transfer_port: 0,
            pack_url: String::new(),
            pack_sha1: String::new(),
            pack_id: String::new(),
            world_file: "queue-world.bin".into(),
            servers: HashMap::new(),
            tiers: vec![Tier { name: "standard".into(), display: "Standard".into(), color: "#a1a1aa".into(), weight: 1.0, ..Tier::default() }],
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ServerLimits {
    /// How the server is called on screen ("Survival").
    pub display: String,
    pub max_players: usize,
    pub admit: f64,
    pub every_seconds: f64,
    pub min_tps: f64,
    pub max_mspt: f64,
    pub max_memory_percent: f64,
    /// A server with players that reported its health before and then goes quiet this long gets no one.
    pub health_timeout_seconds: f64,
}

impl Default for ServerLimits {
    fn default() -> Self {
        ServerLimits { display: String::new(), max_players: 100, admit: 2.0, every_seconds: 10.0, min_tps: 17.0, max_mspt: 45.0, max_memory_percent: 92.0, health_timeout_seconds: 20.0 }
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Tier {
    pub name: String,
    pub display: String,
    pub color: String,
    pub weight: f64,
    pub bypass: bool,
    pub groups: Vec<String>,
    pub permission: String,
}

// ---------------------------------------------------------------- rate limits and health

/// A token bucket: `rate` tokens a second, at most `burst` saved up.
#[derive(Debug, Clone)]
pub struct Bucket {
    pub tokens: f64,
    last: Instant,
}

impl Bucket {
    pub fn new(start: f64) -> Self {
        Bucket { tokens: start, last: Instant::now() }
    }
    pub fn refill(&mut self, rate: f64, burst: f64, now: Instant) {
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + dt * rate).min(burst.max(1.0));
    }
}

#[derive(Debug, Clone, Default)]
pub struct Health {
    pub tps: f64,
    pub mspt: f64,
    pub memory_percent: f64,
    pub at: Option<Instant>,
}

/// Why a server takes no one now (None: it can). A server with nobody on may pause its ticks and stop reporting:
/// only one with players that goes quiet counts as not responding.
pub fn health_problem(h: Option<&Health>, l: &ServerLimits, now: Instant, players: usize) -> Option<&'static str> {
    let h = h?;
    let at = h.at?;
    if now.saturating_duration_since(at).as_secs_f64() > l.health_timeout_seconds {
        return (players > 0).then_some("not responding");
    }
    if h.tps < l.min_tps {
        return Some("low TPS");
    }
    if h.mspt > l.max_mspt {
        return Some("slow ticks");
    }
    if h.memory_percent > l.max_memory_percent {
        return Some("high memory");
    }
    None
}

// ---------------------------------------------------------------- the lines

pub struct Entry {
    pub name: String,
    pub tier: usize,
    pub server: String,
    pub joined: u64,
    pub seq: u64,
    /// On the network now (else away, keeping their place for the grace period).
    pub connected: bool,
    pub left_at: u64,
}

/// One server's line: per tier, (joined, seq, uuid) oldest first; the tiers' round-robin credit; its own rate.
struct Line {
    order: Vec<BTreeSet<(u64, u64, u128)>>,
    credit: Vec<f64>,
    bucket: Bucket,
    admitted: VecDeque<Instant>,
}

impl Line {
    fn new(tiers: usize) -> Self {
        Line { order: vec![BTreeSet::new(); tiers], credit: vec![0.0; tiers], bucket: Bucket::new(1.0), admitted: VecDeque::new() }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    entries: Vec<SavedEntry>,
    tickets: Vec<(String, String, u64)>,
}

#[derive(Serialize, Deserialize)]
struct SavedEntry {
    uuid: String,
    name: String,
    tier: String,
    server: String,
    joined: u64,
    seq: u64,
}

/// Where a waiting player stands in their line.
#[derive(Debug, Clone, PartialEq)]
pub struct Standing {
    pub server: String,
    pub position: usize,
    pub total: usize,
    pub eta_seconds: f64,
    pub tier: usize,
    /// Turns are held (paused by the staff, the server full or struggling): why.
    pub held: Option<String>,
}

/// The answer to a player asking to go to a server.
#[derive(Debug, PartialEq)]
pub enum Reply {
    /// Go now (a ticket is issued).
    Go,
    Queued(Standing),
    /// The line is full (this many wait).
    Full(usize),
}

pub struct Queue {
    pub cfg: QueueConfig,
    pub entries: HashMap<u128, Entry>,
    lines: HashMap<String, Line>,
    /// uuid -> (server, expires ms): admitted, on their way.
    pub tickets: HashMap<u128, (String, u64)>,
    global: Bucket,
    pub health: HashMap<String, Health>,
    pub started: Instant,
    pub paused: bool,
    seq: u64,
    pub dirty: bool,
    pub total_admitted: u64,
}

impl Queue {
    pub fn new(cfg: QueueConfig) -> Self {
        let tiers = cfg.tiers.len().max(1);
        Queue {
            lines: cfg.queued.iter().map(|s| (s.clone(), Line::new(tiers))).collect(),
            cfg,
            entries: HashMap::new(),
            tickets: HashMap::new(),
            global: Bucket::new(1.0),
            health: HashMap::new(),
            started: Instant::now(),
            paused: false,
            seq: 0,
            dirty: false,
            total_admitted: 0,
        }
    }

    pub fn is_queued(&self, server: &str) -> bool {
        self.cfg.enabled && self.lines.contains_key(server)
    }

    pub fn default_tier(&self) -> usize {
        self.cfg.tiers.iter().rposition(|t| t.groups.is_empty() && t.permission.is_empty() && !t.bypass).unwrap_or(self.cfg.tiers.len().saturating_sub(1))
    }

    pub fn tier_of_name(&self, name: &str) -> usize {
        self.cfg.tiers.iter().position(|t| t.name == name).unwrap_or_else(|| self.default_tier())
    }

    pub fn is_bypass(&self, tier: usize) -> bool {
        self.cfg.tiers.get(tier).is_some_and(|t| t.bypass)
    }

    pub fn in_startup(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started).as_secs_f64() < self.cfg.startup_minutes * 60.0
    }

    /// Turns a second, network-wide, as configured now.
    pub fn rate(&self, now: Instant) -> f64 {
        let (n, every) = if self.in_startup(now) { (self.cfg.startup_admit, self.cfg.startup_every_seconds) } else { (self.cfg.admit, self.cfg.every_seconds) };
        (n / every.max(0.1)).max(0.001)
    }

    pub fn limits(&self, server: &str) -> ServerLimits {
        self.cfg.servers.get(server).cloned().unwrap_or_default()
    }

    pub fn display(&self, server: &str) -> String {
        let d = self.limits(server).display;
        if !d.is_empty() {
            return d;
        }
        let mut c = server.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }

    fn connected(&self, uuid: &u128) -> bool {
        self.entries.get(uuid).is_some_and(|e| e.connected)
    }

    /// Players waiting for a server now (not those away).
    pub fn waiting_for(&self, server: &str) -> usize {
        self.lines.get(server).map_or(0, |l| l.order.iter().flatten().filter(|(_, _, u)| self.connected(u)).count())
    }

    pub fn waiting(&self) -> usize {
        self.entries.values().filter(|e| e.connected).count()
    }

    fn waiting_in(&self, server: &str, tier: usize) -> usize {
        self.lines.get(server).and_then(|l| l.order.get(tier)).map_or(0, |o| o.iter().filter(|(_, _, u)| self.connected(u)).count())
    }

    fn inflight(&self, server: &str) -> usize {
        self.tickets.values().filter(|(s, _)| s == server).count()
    }

    /// Whether the server can take one more now: healthy, under its cap (counting tickets on their way) and, when
    /// `use_rate`, with a turn in its own bucket.
    fn can_take(&mut self, server: &str, counts: &HashMap<String, usize>, now: Instant, use_rate: bool) -> bool {
        let l = self.limits(server);
        let on = counts.get(server).copied().unwrap_or(0);
        if health_problem(self.health.get(server), &l, now, on).is_some() || on + self.inflight(server) >= l.max_players {
            return false;
        }
        if !use_rate {
            return true;
        }
        let Some(line) = self.lines.get_mut(server) else { return false };
        line.bucket.refill(l.admit / l.every_seconds.max(0.1), (l.admit / 2.0).max(1.0), now);
        line.bucket.tokens >= 1.0
    }

    fn issue(&mut self, uuid: u128, server: &str, now: Instant, use_rate: bool) {
        if let Some(line) = self.lines.get_mut(server) {
            if use_rate {
                line.bucket.tokens -= 1.0;
            }
            line.admitted.push_back(now);
        }
        self.tickets.insert(uuid, (server.to_owned(), now_ms() + self.cfg.ticket_seconds * 1000));
        self.total_admitted += 1;
        self.dirty = true;
    }

    fn insert(&mut self, uuid: u128, name: &str, tier: usize, server: &str) {
        self.seq += 1;
        let joined = now_ms();
        let tiers = self.cfg.tiers.len().max(1);
        let tier = tier.min(tiers - 1);
        self.lines.entry(server.to_owned()).or_insert_with(|| Line::new(tiers)).order[tier].insert((joined, self.seq, uuid));
        self.entries.insert(uuid, Entry { name: name.to_owned(), tier, server: server.to_owned(), joined, seq: self.seq, connected: true, left_at: 0 });
        self.dirty = true;
    }

    /// A player asks to go to a queued server.
    pub fn request(&mut self, uuid: u128, name: &str, tier: usize, server: &str, counts: &HashMap<String, usize>) -> Reply {
        let now = Instant::now();
        if self.is_bypass(tier) {
            self.remove(uuid);
            self.issue(uuid, server, now, false);
            return Reply::Go;
        }
        if let Some(e) = self.entries.get_mut(&uuid) {
            if e.server == server {
                e.connected = true;
                if e.tier != tier {
                    // Bought priority (or lost it): same place in time, the other line.
                    let key = (e.joined, e.seq, uuid);
                    let (old, new) = (e.tier, tier.min(self.cfg.tiers.len().max(1) - 1));
                    e.tier = new;
                    if let Some(line) = self.lines.get_mut(server) {
                        line.order[old].remove(&key);
                        line.order[new].insert(key);
                    }
                    self.dirty = true;
                }
                return Reply::Queued(self.standing(uuid, counts).expect("just there"));
            }
            self.remove(uuid);
        }
        let nobody_waits = self.waiting_for(server) == 0;
        if !self.cfg.always_queue && !self.paused && nobody_waits {
            self.global.refill(self.rate(now), self.cfg.burst, now);
            if self.global.tokens >= 1.0 && self.can_take(server, counts, now, true) {
                self.global.tokens -= 1.0;
                self.issue(uuid, server, now, true);
                return Reply::Go;
            }
        }
        let waiting = self.waiting_for(server);
        if waiting >= self.cfg.max_size {
            return Reply::Full(waiting);
        }
        self.insert(uuid, name, tier, server);
        Reply::Queued(self.standing(uuid, counts).expect("just added"))
    }

    /// The player logged in to the proxy again: back in their line if they had a place.
    pub fn arrived(&mut self, uuid: u128) -> bool {
        match self.entries.get_mut(&uuid) {
            Some(e) => {
                e.connected = true;
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    /// The player left the network: their place is kept for the grace period.
    pub fn left(&mut self, uuid: u128) {
        if let Some(e) = self.entries.get_mut(&uuid) {
            e.connected = false;
            e.left_at = now_ms();
            self.dirty = true;
        }
    }

    /// Out of the line for good.
    pub fn remove(&mut self, uuid: u128) {
        if let Some(e) = self.entries.remove(&uuid) {
            if let Some(line) = self.lines.get_mut(&e.server) {
                line.order[e.tier].remove(&(e.joined, e.seq, uuid));
            }
            self.dirty = true;
        }
    }

    /// Takes the player's ticket, if they hold one that hasn't run out.
    pub fn take_ticket(&mut self, uuid: u128) -> Option<String> {
        let (server, exp) = self.tickets.remove(&uuid)?;
        self.dirty = true;
        (exp > now_ms()).then_some(server)
    }

    pub fn ticket(&self, uuid: u128) -> Option<&str> {
        self.tickets.get(&uuid).filter(|(_, exp)| *exp > now_ms()).map(|(s, _)| s.as_str())
    }

    /// Which tier of a server's line goes next: smooth weighted round robin over the tiers with someone waiting.
    fn next_tier(&mut self, server: &str) -> Option<usize> {
        let tiers = self.cfg.tiers.len().max(1);
        let ready: Vec<usize> = (0..tiers).filter(|t| self.waiting_in(server, *t) > 0).collect();
        if ready.is_empty() {
            return None;
        }
        let weights: Vec<f64> = (0..tiers).map(|t| self.weight(t)).collect();
        let total: f64 = ready.iter().map(|t| weights[*t]).sum();
        let line = self.lines.get_mut(server)?;
        for t in &ready {
            line.credit[*t] += weights[*t];
        }
        let pick = *ready.iter().max_by(|a, b| line.credit[**a].total_cmp(&line.credit[**b]).then(b.cmp(a)))?;
        line.credit[pick] -= total;
        for t in 0..tiers {
            if !ready.contains(&t) {
                line.credit[t] = 0.0;
            }
        }
        Some(pick)
    }

    fn weight(&self, tier: usize) -> f64 {
        self.cfg.tiers.get(tier).map_or(1.0, |t| t.weight.max(0.01))
    }

    fn oldest_connected(&self, server: &str, tier: usize) -> Option<u128> {
        self.lines.get(server)?.order.get(tier)?.iter().find(|(_, _, u)| self.connected(u)).map(|(_, _, u)| *u)
    }

    /// Housekeeping and turns; returns who goes where now.
    pub fn tick(&mut self, counts: &HashMap<String, usize>) -> Vec<(u128, String)> {
        let now = Instant::now();
        let ms = now_ms();
        let before = self.tickets.len();
        self.tickets.retain(|_, (_, exp)| *exp > ms);
        if self.tickets.len() != before {
            self.dirty = true;
        }
        let grace = self.cfg.grace_seconds * 1000;
        let gone: Vec<u128> = self.entries.iter().filter(|(_, e)| !e.connected && ms.saturating_sub(e.left_at) > grace).map(|(u, _)| *u).collect();
        for u in gone {
            self.remove(u);
        }
        for line in self.lines.values_mut() {
            while line.admitted.front().is_some_and(|t| now.saturating_duration_since(*t) > Duration::from_secs(60)) {
                line.admitted.pop_front();
            }
        }
        self.global.refill(self.rate(now), self.cfg.burst, now);
        let mut out = Vec::new();
        if self.paused {
            return out;
        }
        // Round robin over the servers, one turn each, while the network has turns.
        let servers: Vec<String> = self.cfg.queued.clone();
        loop {
            let mut any = false;
            for s in &servers {
                if self.global.tokens < 1.0 {
                    return out;
                }
                if self.waiting_for(s) == 0 || !self.can_take(s, counts, now, true) {
                    continue;
                }
                let Some(tier) = self.next_tier(s) else { continue };
                let Some(uuid) = self.oldest_connected(s, tier) else { continue };
                self.global.tokens -= 1.0;
                self.remove(uuid);
                self.issue(uuid, s, now, true);
                out.push((uuid, s.clone()));
                any = true;
            }
            if !any {
                return out;
            }
        }
    }

    /// Why nobody goes to this server right now (None: turns are running).
    pub fn held(&self, server: &str, counts: &HashMap<String, usize>) -> Option<String> {
        if self.paused {
            return Some("paused for a moment".into());
        }
        let l = self.limits(server);
        let on = counts.get(server).copied().unwrap_or(0);
        if health_problem(self.health.get(server), &l, Instant::now(), on).is_some() {
            return Some("the server is catching its breath".into());
        }
        if on + self.inflight(server) >= l.max_players {
            return Some("the server is full".into());
        }
        None
    }

    /// Turns a second for a server lately (measured over the last minute; the configured rates when there's
    /// little to go on).
    pub fn rate_for(&self, server: &str, now: Instant) -> f64 {
        let l = self.limits(server);
        let configured = self.rate(now).min(l.admit / l.every_seconds.max(0.1));
        let Some(line) = self.lines.get(server) else { return configured };
        if line.admitted.len() < 5 {
            return configured;
        }
        let span = now.saturating_duration_since(*line.admitted.front().unwrap_or(&now)).as_secs_f64().max(10.0);
        (line.admitted.len() as f64 / span).clamp(configured * 0.25, configured * 2.0)
    }

    pub fn standing(&self, uuid: u128, counts: &HashMap<String, usize>) -> Option<Standing> {
        let e = self.entries.get(&uuid)?;
        let line = self.lines.get(&e.server)?;
        let tier = e.tier;
        let key = (e.joined, e.seq, uuid);
        let rank = line.order[tier].range(..key).filter(|(_, _, u)| self.connected(u)).count();
        let w = self.weight(tier);
        let mut position = rank + 1;
        for t in 0..line.order.len() {
            if t == tier {
                continue;
            }
            let n = self.waiting_in(&e.server, t);
            if n > 0 {
                // While this line moves `rank + 1` places, tier t moves (rank + 1) * w_t / w.
                position += n.min((((rank + 1) as f64) * self.weight(t) / w).ceil() as usize);
            }
        }
        let total = self.waiting_for(&e.server).max(1);
        let position = position.min(total);
        let now = Instant::now();
        Some(Standing {
            server: e.server.clone(),
            position,
            total,
            eta_seconds: position as f64 / self.rate_for(&e.server, now).max(0.0001),
            tier,
            held: self.held(&e.server, counts),
        })
    }

    // ---------------------------------------------------------------- saving

    pub fn save_json(&self) -> String {
        let tier_name = |t: usize| self.cfg.tiers.get(t).map(|t| t.name.clone()).unwrap_or_default();
        let saved = Saved {
            entries: self.entries.iter().map(|(u, e)| SavedEntry { uuid: format!("{u:032x}"), name: e.name.clone(), tier: tier_name(e.tier), server: e.server.clone(), joined: e.joined, seq: e.seq }).collect(),
            tickets: self.tickets.iter().map(|(u, (s, exp))| (format!("{u:032x}"), s.clone(), *exp)).collect(),
        };
        serde_json::to_string(&saved).unwrap_or_default()
    }

    /// The lines as saved: everyone comes back as away, with the grace period to reconnect.
    pub fn load_json(&mut self, text: &str) -> usize {
        let Ok(saved) = serde_json::from_str::<Saved>(text) else { return 0 };
        let ms = now_ms();
        let tiers = self.cfg.tiers.len().max(1);
        for s in saved.entries {
            let Ok(uuid) = u128::from_str_radix(&s.uuid, 16) else { continue };
            if !self.lines.contains_key(&s.server) {
                continue;
            }
            let tier = self.tier_of_name(&s.tier);
            self.seq = self.seq.max(s.seq);
            self.lines.entry(s.server.clone()).or_insert_with(|| Line::new(tiers)).order[tier].insert((s.joined, s.seq, uuid));
            self.entries.insert(uuid, Entry { name: s.name, tier, server: s.server, joined: s.joined, seq: s.seq, connected: false, left_at: ms });
        }
        for (u, s, exp) in saved.tickets {
            if let Ok(uuid) = u128::from_str_radix(&u, 16)
                && exp > ms
            {
                self.tickets.insert(uuid, (s, exp));
            }
        }
        self.entries.len()
    }
}

// ---------------------------------------------------------------- LuckPerms

fn read_json(path: &std::path::Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// A LuckPerms holder's nodes that apply now: granted, without contexts, not past their expiry.
fn nodes(v: &serde_json::Value, now_s: u64) -> Vec<String> {
    v["permissions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["value"].as_bool().unwrap_or(true))
        .filter(|n| n["expiry"].as_u64().is_none_or(|e| e > now_s))
        .filter(|n| n["context"].as_object().is_none_or(|c| c.is_empty()))
        .filter_map(|n| n["permission"].as_str().map(str::to_ascii_lowercase))
        .collect()
}

/// A player's groups (with the ones those inherit) and permissions, from LuckPerms' JSON storage.
pub fn lp_lookup(dirs: &[String], uuid: u128) -> (Vec<String>, Vec<String>) {
    let dashed = {
        let h = format!("{uuid:032x}");
        format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
    };
    let now_s = now_ms() / 1000;
    for dir in dirs {
        let base = std::path::Path::new(dir);
        let Some(user) = read_json(&base.join("users").join(format!("{dashed}.json"))) else { continue };
        let mut perms = nodes(&user, now_s);
        let mut groups: Vec<String> = Vec::new();
        let mut todo: Vec<String> = perms.iter().filter_map(|p| p.strip_prefix("group.").map(str::to_owned)).collect();
        if let Some(primary) = user["primaryGroup"].as_str() {
            todo.push(primary.to_ascii_lowercase());
        }
        while let Some(g) = todo.pop() {
            if groups.contains(&g) || groups.len() > 32 {
                continue;
            }
            groups.push(g.clone());
            if let Some(gv) = read_json(&base.join("groups").join(format!("{g}.json"))) {
                let gp = nodes(&gv, now_s);
                todo.extend(gp.iter().filter_map(|p| p.strip_prefix("group.").map(str::to_owned)));
                perms.extend(gp);
            }
        }
        return (groups, perms);
    }
    (Vec::new(), Vec::new())
}

/// The best tier a player's groups and permissions give.
pub fn tier_for(cfg: &QueueConfig, groups: &[String], perms: &[String], default: usize) -> usize {
    for (i, t) in cfg.tiers.iter().enumerate() {
        let by_group = t.groups.iter().any(|g| groups.iter().any(|h| h.eq_ignore_ascii_case(g)));
        let by_perm = !t.permission.is_empty() && perms.iter().any(|p| p.eq_ignore_ascii_case(&t.permission));
        if by_group || by_perm {
            return i;
        }
    }
    default
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> QueueConfig {
        QueueConfig {
            enabled: true,
            queued: vec!["smp".into()],
            admit: 1000.0,
            every_seconds: 1.0,
            burst: 1000.0,
            startup_minutes: 0.0,
            servers: [("smp".to_owned(), ServerLimits { max_players: 1000, admit: 1000.0, every_seconds: 1.0, ..ServerLimits::default() })].into(),
            tiers: vec![
                Tier { name: "staff".into(), bypass: true, groups: vec!["owner".into()], ..Tier::default() },
                Tier { name: "plus".into(), weight: 3.0, permission: "q.plus".into(), ..Tier::default() },
                Tier { name: "standard".into(), weight: 1.0, ..Tier::default() },
            ],
            ..QueueConfig::default()
        }
    }

    fn line_up(q: &mut Queue, uuid: u128, tier: usize) {
        q.insert(uuid, "p", tier, "smp");
    }

    #[test]
    fn weighted_tiers_share_turns() {
        let mut q = Queue::new(cfg());
        for i in 0..40u128 {
            line_up(&mut q, i, if i < 20 { 1 } else { 2 });
        }
        let mut order = Vec::new();
        for _ in 0..8 {
            let tier = q.next_tier("smp").unwrap();
            order.push(tier);
            let u = q.oldest_connected("smp", tier).unwrap();
            q.remove(u);
        }
        assert_eq!(order.iter().filter(|t| **t == 1).count(), 6, "weight 3 against 1: 6 of 8 ({order:?})");
        assert!(order.contains(&2), "standard still moves");
    }

    #[test]
    fn standing_counts_the_faster_line_ahead() {
        let mut q = Queue::new(cfg());
        for i in 0..10u128 {
            line_up(&mut q, i, 2);
        }
        for i in 10..20u128 {
            line_up(&mut q, i, 1);
        }
        let counts = HashMap::new();
        let s = q.standing(4, &counts).unwrap();
        assert_eq!((s.position, s.total), (15, 20), "5th of standard; all 10 of plus go first");
        assert_eq!(q.standing(10, &counts).unwrap().position, 2);
    }

    #[test]
    fn straight_in_when_nobody_waits_else_a_place() {
        let mut q = Queue::new(cfg());
        let counts: HashMap<String, usize> = HashMap::new();
        assert_eq!(q.request(1, "a", 2, "smp", &counts), Reply::Go);
        assert_eq!(q.ticket(1), Some("smp"));
        line_up(&mut q, 5, 2);
        assert!(matches!(q.request(2, "b", 2, "smp", &counts), Reply::Queued(s) if s.position == 2));
        assert!(matches!(q.request(2, "b", 2, "smp", &counts), Reply::Queued(_)), "asking again keeps the place");
        assert_eq!(q.waiting_for("smp"), 2);
        assert_eq!(q.request(3, "staff", 0, "smp", &counts), Reply::Go, "staff skip the line");
    }

    #[test]
    fn full_line_says_so() {
        let mut c = cfg();
        c.max_size = 2;
        c.always_queue = true;
        let mut q = Queue::new(c);
        let counts = HashMap::new();
        q.request(1, "a", 2, "smp", &counts);
        q.request(2, "b", 2, "smp", &counts);
        assert_eq!(q.request(3, "c", 2, "smp", &counts), Reply::Full(2));
    }

    #[test]
    fn grace_keeps_the_place_then_runs_out() {
        let mut c = cfg();
        c.grace_seconds = 0;
        let mut q = Queue::new(c);
        line_up(&mut q, 1, 2);
        line_up(&mut q, 2, 2);
        q.left(1);
        assert_eq!(q.waiting_for("smp"), 1);
        assert!(q.arrived(1), "back with their place");
        assert_eq!(q.standing(1, &HashMap::new()).unwrap().position, 1);
        q.left(1);
        std::thread::sleep(Duration::from_millis(5));
        q.paused = true;
        q.tick(&HashMap::new());
        assert!(!q.entries.contains_key(&1), "grace over");
    }

    #[test]
    fn turns_respect_rate_cap_and_health() {
        let mut c = cfg();
        c.admit = 2.0;
        c.every_seconds = 1.0;
        c.burst = 2.0;
        c.servers.get_mut("smp").unwrap().max_players = 3;
        let mut q = Queue::new(c);
        for i in 0..10u128 {
            line_up(&mut q, i, 2);
        }
        q.global.tokens = 2.0;
        q.lines.get_mut("smp").unwrap().bucket.tokens = 5.0;
        let counts: HashMap<String, usize> = [("smp".to_owned(), 0usize)].into();
        let first = q.tick(&counts);
        assert_eq!(first.len(), 2, "two network turns");
        assert_eq!(first[0].0, 0, "oldest first");
        q.global.tokens = 5.0;
        assert_eq!(q.tick(&counts).len(), 1, "cap 3 counts the tickets on their way");
        assert_eq!(q.held("smp", &counts).as_deref(), Some("the server is full"));
        q.tickets.clear();
        q.health.insert("smp".into(), Health { tps: 20.0, mspt: 5.0, memory_percent: 10.0, at: Some(Instant::now() - Duration::from_secs(60)) });
        let busy: HashMap<String, usize> = [("smp".to_owned(), 1usize)].into();
        q.global.tokens = 5.0;
        assert!(q.tick(&busy).is_empty(), "players on it and silent: nobody goes");
        assert!(q.held("smp", &counts).is_none(), "empty and silent: just paused");
        q.health.insert("smp".into(), Health { tps: 12.0, mspt: 5.0, memory_percent: 10.0, at: Some(Instant::now()) });
        assert!(q.tick(&counts).is_empty(), "low TPS: nobody goes");
        q.health.insert("smp".into(), Health { tps: 19.9, mspt: 5.0, memory_percent: 10.0, at: Some(Instant::now()) });
        assert!(!q.tick(&counts).is_empty());
    }

    #[test]
    fn saved_and_loaded() {
        let mut q = Queue::new(cfg());
        line_up(&mut q, 7, 1);
        line_up(&mut q, 8, 2);
        let text = q.save_json();
        let mut r = Queue::new(cfg());
        assert_eq!(r.load_json(&text), 2);
        assert_eq!(r.waiting(), 0, "everyone comes back as away");
        assert!(r.arrived(8));
        assert_eq!(r.entries[&7].tier, 1);
        assert_eq!(r.entries[&8].server, "smp");
    }

    #[test]
    fn tiers_from_groups_and_permissions() {
        let c = cfg();
        assert_eq!(tier_for(&c, &["owner".into()], &[], 2), 0);
        assert_eq!(tier_for(&c, &["default".into()], &["q.plus".into()], 2), 1);
        assert_eq!(tier_for(&c, &[], &[], 2), 2);
        let user = serde_json::json!({ "permissions": [
            { "permission": "q.plus", "value": true, "expiry": 1 },
            { "permission": "group.vip", "value": true },
            { "permission": "fly", "value": true, "context": { "server": "smp" } }
        ]});
        assert_eq!(nodes(&user, 100), vec!["group.vip".to_owned()]);
    }
}
