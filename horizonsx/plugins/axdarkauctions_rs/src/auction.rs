//! A dark auction: its file (auctions/<name>.yml), its run (Auction, AuctionSettings, AuctionPlayers) and the
//! list of them (AuctionManager). The clock moves once a second: STARTING (broadcasts) -> SHOWING (the item and its
//! starting price) -> BIDDING (the bid GUI) -> WAITING -> SHOWING ... -> ENDING (players sent back).

use crate::entities::Entities;
use crate::{finish, later, lock, settings, texts};
use horizon_bukkit::{CommandSender, Location, Player, World};
use horizon_kit::{Config, yaml};
use serde_yaml::Value as Yaml;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Inactive,
    Starting,
    Showing,
    Bidding,
    Waiting,
    Ending,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Inactive => "INACTIVE",
            State::Starting => "STARTING",
            State::Showing => "SHOWING",
            State::Bidding => "BIDDING",
            State::Waiting => "WAITING",
            State::Ending => "ENDING",
        }
    }
}

/// One `items` entry: what can be sold, how likely, its starting price and what the winner gets.
#[derive(Clone, Debug)]
pub struct Reward {
    pub idx: usize,
    pub chance: f64,
    pub display: Yaml,
    pub commands: Vec<String>,
    pub items: Vec<Yaml>,
    pub currency: String,
    pub amount: f64,
}

/// A bid slot of one page: the amount, the stage's item and the step to the next.
#[derive(Clone, Debug)]
pub struct Bidding {
    pub value: f64,
    pub item: Yaml,
    pub increase: f64,
}

pub struct Auction {
    pub name: String,
    pub cfg: Config,
    pub fancy: String,
    pub spawn: Option<Location>,
    pub exit: Option<Location>,
    pub display: Option<Location>,
    pub players_min: i64,
    pub players_max: i64,
    pub start_wait: i64,
    pub show_time: i64,
    pub bid_time: i64,
    pub extend_bid: bool,
    pub extend_time: i64,
    pub between: i64,
    pub end_wait: i64,
    pub starting_broadcast: i64,
    pub bid_slots: Vec<usize>,
    pub rewards: Vec<Reward>,
    // The run.
    pub running: bool,
    pub time: i64,
    pub left: i64,
    pub state: State,
    pub selected: Option<Reward>,
    pub bid: f64,
    pub page: usize,
    pub top: Option<Player>,
    /// Each bidder's latest bid: uuid -> (name, amount).
    pub top_bids: HashMap<String, (String, f64)>,
    pub biddings: HashMap<usize, HashMap<usize, Bidding>>,
    /// Who is in, and where each came from.
    pub players: Vec<(Player, Location)>,
    pub banned: HashSet<String>,
    pub entities: Entities,
}

pub static AUCTIONS: Mutex<BTreeMap<String, Auction>> = Mutex::new(BTreeMap::new());

/// `world;x;y;z;yaw;pitch` (LocationSerializer, DynamicLocation).
pub fn parse_location(text: &str) -> Option<Location> {
    let p: Vec<&str> = text.split(';').collect();
    if p.len() < 4 {
        return None;
    }
    let n = |i: usize| p.get(i).and_then(|v| v.trim().parse::<f64>().ok());
    Some(Location::new(World::of(p[0]), n(1)?, n(2)?, n(3)?, n(4).unwrap_or(0.0) as f32, n(5).unwrap_or(0.0) as f32))
}

pub fn write_location(at: &Location) -> String {
    format!("{};{};{};{};{};{}", at.world.get_name(), at.x, at.y, at.z, at.yaw, at.pitch)
}

/// GuiFrame.getSlots: `5`, `0-8` (a row), `18|45` (a column, either way).
pub fn slots(specs: &[String]) -> Vec<usize> {
    let mut out = Vec::new();
    for s in specs {
        let s = s.trim();
        if let Ok(n) = s.parse::<usize>() {
            out.push(n);
        } else if let Some((a, b)) = s.split_once('|') {
            let (Ok(a), Ok(b)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>()) else { continue };
            let mut v = a;
            if b >= a {
                while v <= b {
                    out.push(v as usize);
                    v += 9;
                }
            } else {
                while v >= b {
                    out.push(v as usize);
                    v -= 9;
                }
            }
        } else if let Some((a, b)) = s.split_once('-') {
            let (Ok(a), Ok(b)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>()) else { continue };
            out.extend(a..=b);
        }
    }
    out
}

/// A route's `slot` (one or a list).
pub fn route_slots(file: &Config, route: &str) -> Vec<usize> {
    slots(&yaml::list_or_one(&file.doc, &format!("{route}.slot")))
}

fn number(v: Option<&Yaml>, default: f64) -> f64 {
    v.and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).unwrap_or(default)
}

/// The rewards of an auction file, each with a currency the server has (the others reported, as in Java).
pub fn read_rewards(name: &str, cfg: &Config) -> Vec<Reward> {
    let mut out = Vec::new();
    let Some(Yaml::Sequence(items)) = cfg.at("items") else { return out };
    for entry in items {
        let price = entry.get("starting-price");
        let currency_name = price.and_then(|p| yaml::string(p, "currency")).unwrap_or_else(|| "Vault".into());
        let Some(currency) = crate::currency(&currency_name) else {
            texts::console(&format!("&#FF0000[AxDarkAuctions] Currency named &#DD0000{currency_name} &#FF0000not found in dark auction {name}!"));
            continue;
        };
        out.push(Reward {
            idx: out.len(),
            chance: number(entry.get("chance"), 50.0),
            display: entry.get("display").cloned().unwrap_or(Yaml::Mapping(Default::default())),
            commands: yaml::list(entry, "commands"),
            items: match entry.get("items") {
                Some(Yaml::Sequence(list)) => list.clone(),
                _ => Vec::new(),
            },
            currency: currency.name,
            amount: number(price.and_then(|p| p.get("amount")), 100.0),
        });
    }
    out
}

impl Auction {
    fn new(name: &str, path: std::path::PathBuf) -> Auction {
        let mut a = Auction {
            name: name.to_owned(),
            cfg: Config { path, doc: Yaml::Null },
            fancy: String::new(),
            spawn: None,
            exit: None,
            display: None,
            players_min: 0,
            players_max: -1,
            start_wait: 60,
            show_time: 5,
            bid_time: 15,
            extend_bid: true,
            extend_time: 8,
            between: 5,
            end_wait: 30,
            starting_broadcast: 5,
            bid_slots: Vec::new(),
            rewards: Vec::new(),
            running: false,
            time: 0,
            left: 0,
            state: State::Inactive,
            selected: None,
            bid: 0.0,
            page: 1,
            top: None,
            top_bids: HashMap::new(),
            biddings: HashMap::new(),
            players: Vec::new(),
            banned: HashSet::new(),
            entities: Entities::default(),
        };
        a.reload();
        a
    }

    fn times(&self, route: &str) -> i64 {
        let own = self.cfg.int(&format!("times.{route}"), -1);
        if own == -1 { crate::config_int(&format!("times.{route}"), 0) } else { own }
    }

    /// AuctionSettings.reload + AuctionEntities.reload.
    pub fn reload(&mut self) {
        self.cfg.doc = yaml::read(&self.cfg.path);
        self.fancy = self.cfg.str("name", "");
        let loc = |cfg: &Config, k: &str| yaml::string(&cfg.doc, k).and_then(|s| parse_location(&s));
        self.spawn = loc(&self.cfg, "locations.spawn");
        self.exit = loc(&self.cfg, "locations.exit");
        self.display = loc(&self.cfg, "locations.display");
        self.players_min = self.cfg.int("players.minimum", 0);
        self.players_max = self.cfg.int("players.maximum", -1);
        self.start_wait = self.times("start-wait-time");
        self.show_time = self.times("show-time");
        self.bid_time = self.times("bid-time");
        self.extend_bid = self.cfg.bool("times.extend-bid-time", crate::config_bool("times.extend-bid-time", true));
        self.extend_time = self.times("extend-time");
        self.between = self.times("time-between-bids");
        self.end_wait = self.times("end-wait-time");
        self.starting_broadcast = self.times("starting-broadcast").max(1);
        self.bid_slots = settings(|s| slots(&yaml::list_or_one(&s.main_gui.doc, "bid-slots"))).unwrap_or_default();
        self.rewards = read_rewards(&self.name, &self.cfg);
        self.entities.reload(&self.cfg, self.display.as_ref());
        if self.running {
            // A reload mid-run keeps what is shown: the entities come back as the run has them.
            self.entities.start();
            if matches!(self.state, State::Showing | State::Bidding) {
                if let Some(r) = &self.selected {
                    self.entities.showing(&r.display, self.state == State::Bidding);
                }
            }
        }
    }

    pub fn has(&self, player: &Player) -> bool {
        let id = player.get_unique_id();
        self.players.iter().any(|(p, _)| p.get_unique_id() == id)
    }

    /// The auction's chat line to everyone in it (`broadcasts.<route>`, no prefix).
    pub fn broadcast(&self, route: &str, reps: &[(&str, &str)]) {
        let line = texts::fill(&crate::lang(&format!("broadcasts.{route}")), reps);
        for (p, _) in &self.players {
            texts::send(p, &line);
        }
    }

    fn hide_all(&self) {
        for (a, _) in &self.players {
            for (b, _) in &self.players {
                if a.get_unique_id() != b.get_unique_id() {
                    a.hide_player(b);
                }
            }
        }
    }

    fn show_all(&self) {
        for (a, _) in &self.players {
            for (b, _) in &self.players {
                if a.get_unique_id() != b.get_unique_id() {
                    a.show_player(b);
                }
            }
        }
    }

    /// The reward's display item's name as the broadcasts write it (Utils.getFormattedItemName).
    pub fn item_name(reward: &Reward) -> String {
        let spec = horizon_kit::item::Spec::from_yaml(&reward.display);
        match spec.name.filter(|n| !n.trim().is_empty()) {
            Some(n) => n,
            None => crate::names::translated(&spec.material),
        }
    }

    pub fn start(&mut self) {
        if self.rewards.is_empty() {
            texts::console(&format!("&#FF0000[AxDarkAuctions] There are no rewards set up for dark auction named {}!", self.name));
            return;
        }
        if self.running {
            return;
        }
        self.running = true;
        self.set_state(State::Starting, false);
        self.banned.clear();
        let wait = self.start_wait.to_string();
        let wait_f = crate::format_time(self.start_wait * 1000);
        for line in crate::lang_list("starting") {
            let line = texts::fill(&line, &[("%display-name%", &self.fancy), ("%name%", &self.name), ("%time%", &wait), ("%time_formatted%", &wait_f)]);
            for p in horizon_bukkit::get_online_players() {
                texts::send(&p, &line);
            }
        }
        self.entities.start();
        let amount = self.cfg.str("sell-item-amount", "1");
        self.left = match amount.trim().parse::<i64>() {
            Ok(n) => n,
            Err(_) => {
                let (a, b) = amount.split_once('-').unwrap_or((&amount, &amount));
                let (a, b) = (a.trim().parse::<i64>().unwrap_or(1), b.trim().parse::<i64>().unwrap_or(1));
                if b > a { a + random_below((b - a + 1) as usize) as i64 } else { a }
            }
        };
    }

    pub fn stop(&mut self) {
        if !self.running {
            return;
        }
        self.running = false;
        self.state = State::Inactive;
        self.entities.stop();
        crate::menus::close_auction(&self.name);
        // AuctionPlayers.stop: everyone back where they came from.
        let back: Vec<(Player, Location)> = std::mem::take(&mut self.players);
        later(move || {
            for (p, at) in back {
                p.teleport(&at);
            }
        });
    }

    pub fn tick(&mut self) {
        if !self.running {
            return;
        }
        self.time += 1;
        match self.state {
            State::Starting => {
                if self.time < self.start_wait {
                    if (self.start_wait - self.time) % self.starting_broadcast == 0 {
                        let left = self.start_wait - self.time;
                        self.broadcast("starting", &[("%time%", &left.to_string()), ("%time_formatted%", &crate::format_time(left * 1000))]);
                    }
                } else if (self.players.len() as i64) < self.players_min {
                    self.broadcast("not-enough-players", &[("%players%", &self.players_min.to_string()), ("%time%", &self.end_wait.to_string()), ("%time_formatted%", &crate::format_time(self.end_wait * 1000))]);
                    self.set_state(State::Ending, false);
                } else {
                    self.set_state(State::Showing, false);
                    self.hide_all();
                }
            }
            State::Showing => {
                if self.time < self.show_time {
                    let name = self.name.clone();
                    later(move || crate::menus::refresh(&name));
                } else {
                    self.set_state(State::Bidding, false);
                }
            }
            State::Bidding => {
                if self.time < self.bid_time {
                    let name = self.name.clone();
                    later(move || crate::menus::refresh(&name));
                } else {
                    self.finish_bidding();
                }
            }
            State::Waiting => {
                if self.time >= self.between {
                    self.set_state(State::Showing, false);
                }
            }
            State::Ending => {
                if self.time >= self.end_wait {
                    self.stop();
                }
            }
            State::Inactive => {}
        }
    }

    /// The bidding's end: the winner pays and gets the reward, or the cheater (who can't pay) is thrown out and
    /// the item comes again.
    fn finish_bidding(&mut self) {
        let Some(reward) = self.selected.clone() else {
            self.set_state(State::Ending, false);
            return;
        };
        let item = Self::item_name(&reward);
        let mut repeat = false;
        match self.top.clone() {
            None => self.broadcast("no-bidders", &[("%item%", &item)]),
            Some(winner) => {
                let currency = crate::currency(&reward.currency);
                let online = winner.is_online();
                let pays = currency.as_ref().is_some_and(|c| c.balance(&winner) >= self.bid && (c.works_offline() || online));
                if pays {
                    let amount = crate::format_money(&reward.currency, self.bid);
                    self.broadcast("winner", &[("%player%", winner.get_name()), ("%item%", &item), ("%amount%", &amount)]);
                    let bid = self.bid;
                    later(move || {
                        if let Some(c) = currency {
                            c.take(&winner, bid);
                        }
                        let stacks: Vec<horizon_bukkit::ItemStack> = reward.items.iter().filter_map(|i| horizon_kit::item::Spec::from_yaml(i).build(&|t| t.to_owned())).collect();
                        if !stacks.is_empty() && winner.is_online() {
                            winner.give(&stacks, true);
                        }
                        for command in &reward.commands {
                            horizon_bukkit::dispatch_command(&command.replace("%player%", winner.get_name()));
                        }
                    });
                } else {
                    self.left += 1;
                    self.broadcast("cheater", &[("%player%", winner.get_name())]);
                    self.banned.insert(winner.get_unique_id());
                    let name = self.name.clone();
                    later(move || {
                        leave(&name, &winner);
                    });
                    repeat = true;
                }
            }
        }
        let more = self.left > 0;
        self.left -= 1;
        if more {
            self.set_state(State::Waiting, repeat);
        } else {
            self.set_state(State::Ending, false);
        }
    }

    pub fn set_state(&mut self, state: State, repeat: bool) {
        self.state = state;
        self.time = 0;
        match state {
            State::Starting => self.broadcast("started", &[]),
            State::Showing => {
                if !repeat || self.selected.is_none() {
                    self.selected = pick_reward(&self.rewards);
                }
                let Some(reward) = self.selected.clone() else { return };
                self.entities.showing(&reward.display, false);
                let amount = crate::format_money(&reward.currency, reward.amount);
                self.broadcast("showing", &[("%item%", &Self::item_name(&reward)), ("%amount%", &amount), ("%time%", &self.show_time.to_string()), ("%time_formatted%", &crate::format_time(self.show_time * 1000))]);
                if crate::config_bool("automatically-open-preview", true) {
                    self.open_for_all(crate::menus::Kind::Preview);
                }
            }
            State::Bidding => {
                // The bidding GUI replaces the preview by itself. Closing a window and opening another in the same
                // tick loses the new one on this SDK: the old window's close arrives late and is taken for it.
                let auto_open = crate::config_bool("automatically-open-bidding", true);
                if !auto_open {
                    crate::menus::close_auction(&self.name);
                }
                self.biddings.clear();
                self.top = None;
                self.top_bids.clear();
                self.page = 1;
                self.make_page(1);
                self.bid = 0.0;
                let Some(reward) = self.selected.clone() else { return };
                let amount = crate::format_money(&reward.currency, reward.amount);
                self.broadcast("bidding", &[("%item%", &Self::item_name(&reward)), ("%amount%", &amount), ("%time%", &self.bid_time.to_string()), ("%time_formatted%", &crate::format_time(self.bid_time * 1000))]);
                if auto_open {
                    self.open_for_all(crate::menus::Kind::Bidding);
                }
                self.entities.bidding();
            }
            State::Waiting => {
                self.cleanup();
                self.broadcast("waiting", &[("%time%", &self.between.to_string()), ("%time_formatted%", &crate::format_time(self.between * 1000))]);
            }
            State::Ending => {
                self.cleanup();
                self.broadcast("ended", &[("%time%", &self.end_wait.to_string()), ("%time_formatted%", &crate::format_time(self.end_wait * 1000))]);
                self.show_all();
            }
            State::Inactive => {}
        }
    }

    fn open_for_all(&self, kind: crate::menus::Kind) {
        let who: Vec<Player> = self.players.iter().map(|(p, _)| p.clone()).collect();
        let name = self.name.clone();
        later(move || {
            for p in &who {
                crate::menus::open(p, &name, kind);
            }
        });
    }

    fn cleanup(&mut self) {
        crate::menus::close_auction(&self.name);
        self.entities.cleanup();
    }

    /// One page of bid slots: page 1 from the starting price, each next one on from the page before.
    pub fn make_page(&mut self, page: usize) {
        let Some(reward) = &self.selected else { return };
        let mut amount = if page <= 1 {
            reward.amount
        } else {
            let before = self.biddings.get(&(page - 1));
            let anchor = self.bid_slots.len().checked_sub(2).and_then(|i| self.bid_slots.get(i));
            match (before, anchor) {
                (Some(b), Some(slot)) => b.get(slot).map_or(reward.amount, |x| x.value + x.increase),
                _ => reward.amount,
            }
        };
        let mut row = HashMap::new();
        for slot in &self.bid_slots {
            let Some(stage) = crate::stage_for(amount) else { break };
            row.insert(*slot, Bidding { value: amount, item: stage.item.clone(), increase: stage.increase });
            amount += stage.increase;
        }
        self.biddings.insert(page, row);
    }

    pub fn left_seconds(&self) -> i64 {
        match self.state {
            State::Showing => self.show_time - self.time,
            State::Bidding => self.bid_time - self.time,
            _ => 0,
        }
    }

    /// AuctionPlaceholders: `%item%`, `%bid%`, `%time%`, `%time_formatted%`, `%state%`, `%next-event%` in its
    /// holograms.
    pub fn hologram_values(&self) -> Vec<(&'static str, String)> {
        let bid = match (&self.selected, self.state) {
            (Some(r), State::Showing) => crate::format_money(&r.currency, r.amount),
            (Some(r), State::Bidding) => crate::format_money(&r.currency, self.bid),
            _ => String::new(),
        };
        let (time, time_f) = match self.state {
            State::Showing | State::Bidding => (self.left_seconds().to_string(), crate::format_time(self.left_seconds() * 1000)),
            _ => (String::new(), String::new()),
        };
        let item = self.selected.as_ref().map(Self::item_name).unwrap_or_default();
        vec![("%item%", item), ("%bid%", bid), ("%time_formatted%", time_f), ("%time%", time), ("%state%", self.state.name().to_owned()), ("%next-event%", self.time.to_string())]
    }
}

/// RandomUtils.getRandomKey: by chance (weights, they may add up to anything).
fn pick_reward(rewards: &[Reward]) -> Option<Reward> {
    let total: f64 = rewards.iter().map(|r| r.chance.max(0.0)).sum();
    if rewards.is_empty() {
        return None;
    }
    if total <= 0.0 {
        return rewards.first().cloned();
    }
    let mut roll = random_unit() * total;
    for r in rewards {
        roll -= r.chance.max(0.0);
        if roll < 0.0 {
            return Some(r.clone());
        }
    }
    rewards.last().cloned()
}

fn random_bits() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut x = STATE.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x9E37_79B9, |d| d.as_nanos() as u64) | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    x
}

pub fn random_unit() -> f64 {
    (random_bits() >> 11) as f64 / (1u64 << 53) as f64
}

pub fn random_below(n: usize) -> usize {
    if n == 0 { 0 } else { (random_bits() % n as u64) as usize }
}

// ---- AuctionManager ----

/// The auctions folder read again: files changed are reloaded, new ones added, gone ones stopped and dropped.
pub fn reload_all() {
    let dir = horizon_kit::app::folder().join("auctions");
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let file = e.file_name().to_string_lossy().to_string();
            if let Some(name) = file.strip_suffix(".yml").or_else(|| file.strip_suffix(".yaml")) {
                found.push((name.to_owned(), e.path()));
            }
        }
    }
    let mut all = lock(&AUCTIONS);
    for (name, path) in &found {
        match all.get_mut(name) {
            Some(a) => a.reload(),
            None => {
                all.insert(name.clone(), Auction::new(name, path.clone()));
            }
        }
    }
    let keep: HashSet<&String> = found.iter().map(|(n, _)| n).collect();
    let gone: Vec<String> = all.keys().filter(|k| !keep.contains(k)).cloned().collect();
    for name in gone {
        if let Some(mut a) = all.remove(&name) {
            a.stop();
        }
    }
}

/// `/axda create`: auctions/<name>.yml from the example; false when it exists.
pub fn create(name: &str) -> bool {
    let mut all = lock(&AUCTIONS);
    if all.contains_key(name) {
        return false;
    }
    let path = horizon_kit::app::folder().join("auctions").join(format!("{name}.yml"));
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if !path.exists() {
        let _ = std::fs::write(&path, crate::DEFAULT_AUCTION);
    }
    all.insert(name.to_owned(), Auction::new(name, path));
    true
}

pub fn delete(name: &str) {
    if let Some(mut a) = lock(&AUCTIONS).remove(name) {
        a.stop();
        let _ = std::fs::remove_file(&a.cfg.path);
    }
}

pub fn names() -> Vec<String> {
    lock(&AUCTIONS).keys().cloned().collect()
}

pub fn exists(name: &str) -> bool {
    lock(&AUCTIONS).contains_key(name)
}

pub fn with<R>(name: &str, f: impl FnOnce(&mut Auction) -> R) -> Option<R> {
    lock(&AUCTIONS).get_mut(name).map(f)
}

pub fn in_any(p: &Player) -> bool {
    lock(&AUCTIONS).values().any(|a| a.has(p))
}

/// A running auction (the first, as `findAny` would).
pub fn any_running() -> Option<String> {
    lock(&AUCTIONS).values().find(|a| a.running).map(|a| a.name.clone())
}

pub fn tick_all() {
    let mut all = lock(&AUCTIONS);
    let online: Vec<Player> = if all.values().any(|a| a.entities.any_live()) { horizon_bukkit::get_online_players() } else { Vec::new() };
    for a in all.values_mut() {
        a.tick();
        let values = a.hologram_values();
        a.entities.tick(&online, &values);
        for (p, _) in &a.players {
            crate::refresh_admin(p);
        }
    }
}

pub fn stop_all() -> Vec<String> {
    let mut stopped = Vec::new();
    for a in lock(&AUCTIONS).values_mut() {
        if a.running {
            stopped.push(a.name.clone());
        }
        a.stop();
    }
    stopped
}

/// AuctionPlayers.join: false (with the reason told) when they can't.
pub fn join(name: &str, p: &Player, force: bool) -> bool {
    let mut all = lock(&AUCTIONS);
    let Some(a) = all.get_mut(name) else { return false };
    if a.has(p) {
        return false;
    }
    if a.running && a.state != State::Starting && !crate::config_bool("allow-joining-after-start", true) {
        crate::tell_lang(p, "errors.started", &[]);
        return false;
    }
    if a.banned.contains(&p.get_unique_id()) {
        crate::tell_lang(p, "errors.banned", &[]);
        return false;
    }
    let count = a.players.len() as i64;
    if a.players_max != -1 && !force && count >= a.players_max {
        crate::tell_lang(p, "errors.full", &[("%auction%", &a.fancy), ("%players%", &count.to_string()), ("%max%", &a.players_max.to_string())]);
        return false;
    }
    a.players.push((p.clone(), p.get_location()));
    crate::refresh_admin(p);
    if let Some(spawn) = a.spawn.clone() {
        let who = p.clone();
        later(move || {
            who.teleport(&spawn);
        });
    }
    a.broadcast("join", &[("%player%", p.get_name())]);
    if a.state != State::Starting && a.state != State::Ending {
        for (other, _) in &a.players {
            if other.get_unique_id() != p.get_unique_id() {
                p.hide_player(other);
                other.hide_player(p);
            }
        }
    }
    true
}

/// AuctionPlayers.leave: back where they came from, the others shown again.
pub fn leave(name: &str, p: &Player) -> bool {
    let mut all = lock(&AUCTIONS);
    let Some(a) = all.get_mut(name) else { return false };
    let id = p.get_unique_id();
    let Some(at) = a.players.iter().position(|(q, _)| q.get_unique_id() == id) else { return false };
    let (_, back) = a.players.remove(at);
    let who = p.clone();
    later(move || {
        who.teleport(&back);
        who.close_inventory();
    });
    a.broadcast("leave", &[("%player%", p.get_name())]);
    for (other, _) in &a.players {
        p.show_player(other);
        other.show_player(p);
    }
    true
}

/// The auction they are in, if any.
pub fn of_player(p: &Player) -> Option<String> {
    lock(&AUCTIONS).values().find(|a| a.has(p)).map(|a| a.name.clone())
}

pub fn leave_everywhere(p: &Player) {
    for name in names() {
        leave(&name, p);
    }
}

// ---- Schedules ----

static LAST_MINUTE: Mutex<Option<i64>> = Mutex::new(None);

/// Each new minute: the schedulers due in it (and in minutes a paused server skipped, up to an hour) start their
/// auctions.
pub fn check_schedules() {
    let minute = chrono::Utc::now().timestamp() / 60;
    let previous = std::mem::replace(&mut *lock(&LAST_MINUTE), Some(minute));
    // The first check (enable) only marks the minute: a schedule of this very minute ran before, or won't.
    let Some(previous) = previous else { return };
    let mut due = Vec::new();
    for m in (previous + 1).max(minute - 60)..=minute {
        let Some(at) = chrono::DateTime::from_timestamp(m * 60, 0) else { continue };
        due.extend(settings(|s| s.schedules.due(at, &mut random_below)).unwrap_or_default());
    }
    for name in due {
        let started = with(&name, |a| a.start());
        if started.is_none() {
            texts::console(&format!("&#FF0000[AxDarkAuctions] A scheduler just activated, but we can't find the following dark auction: {name}"));
            texts::console("&#FF0000[AxDarkAuctions] You can fix this by removing the dark auction from the schedulers.yml!");
        }
    }
}

/// Milliseconds until the next scheduled run of an auction (`None`: not scheduled).
pub fn next_run_in(name: &str) -> Option<i64> {
    let now = chrono::Utc::now();
    settings(|s| s.schedules.next_for_auction(name, now)).flatten().map(|t| t - now.timestamp_millis())
}

pub fn next_scheduler_in(scheduler: &str) -> Option<i64> {
    let now = chrono::Utc::now();
    settings(|s| s.schedules.next_of(scheduler, now)).flatten().map(|t| t - now.timestamp_millis())
}

/// PlaceholderAPI `%axdarkauctions_...%`: `next_<schedule>`, `next_<schedule>_formatted`, `<auction>_next`,
/// `<auction>_next_formatted`, `<auction>_active`, `<auction>_state`.
pub fn placeholder(key: &str) -> Option<String> {
    let seconds = |ms: Option<i64>| ms.map_or_else(|| "---".to_owned(), |ms| (ms / 1000).to_string());
    let formatted = |ms: Option<i64>| ms.map_or_else(|| "---".to_owned(), crate::format_time);
    if let Some(rest) = key.strip_prefix("next_") {
        return Some(match rest.strip_suffix("_formatted") {
            Some(s) => formatted(next_scheduler_in(s)),
            None => seconds(next_scheduler_in(rest)),
        });
    }
    for (suffix, kind) in [("_next_formatted", 0), ("_next", 1), ("_active", 2), ("_state", 3)] {
        if let Some(name) = key.strip_suffix(suffix)
            && exists(name)
        {
            return Some(match kind {
                0 => formatted(next_run_in(name)),
                1 => seconds(next_run_in(name)),
                2 => with(name, |a| a.running.to_string()).unwrap_or_default(),
                _ => with(name, |a| a.state.name().to_owned()).unwrap_or_default(),
            });
        }
    }
    None
}

/// For commands that send to a CommandSender.
pub fn tell(sender: &CommandSender, key: &str, reps: &[(&str, &str)]) {
    crate::send_lang(sender, key, reps);
}

/// Runs a closure with the auctions let go and the deferred work done: the entry points that aren't the tick.
pub fn run_now(f: impl FnOnce()) {
    f();
    finish();
}
