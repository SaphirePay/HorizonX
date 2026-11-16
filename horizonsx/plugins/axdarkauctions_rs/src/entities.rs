//! AuctionEntities: the entrance, exit and auctioneer NPCs, the display block (an invisible armor stand wearing
//! the block), the item on show (a floating item) and the holograms over them (text displays). None of it is in
//! the world: each player near enough is sent the entities as packets, once, and only what changes after that (a
//! hologram's text). A click on one comes back as PlayerUseUnknownEntityEvent.

use crate::auction::{State, parse_location};
use horizon_bukkit::{Location, Player};
use horizon_kit::{Config, packets, sync, yaml};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

/// How far away a player still sees them (AxAPI's tracker range for these).
const RANGE: f64 = 48.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Entrance,
    Exit,
    Auctioneer,
    Block,
    Item,
    Text,
}

/// One client-side entity.
#[derive(Clone, Debug)]
pub struct Ent {
    pub id: i32,
    pub role: Role,
    kind: String,
    world: String,
    pos: [f64; 3],
    yaw: f32,
    data: Vec<Value>,
    helmet: Option<Value>,
    /// A hologram's lines (placeholders unfilled).
    lines: Vec<String>,
    pub live: bool,
    /// Who has it, and the text they were last sent.
    seen: HashMap<String, String>,
}

impl Ent {
    fn new(role: Role, kind: &str, at: &Location, data: Vec<Value>) -> Ent {
        Ent { id: packets::next_id(), role, kind: kind.to_owned(), world: at.world.get_name().to_owned(), pos: [at.x, at.y, at.z], yaw: at.yaw, data, helmet: None, lines: Vec::new(), live: false, seen: HashMap::new() }
    }

    fn hide_all(&mut self) {
        let who: Vec<String> = self.seen.drain().map(|(k, _)| k).collect();
        packets::remove(&who, &[self.id]);
    }

    fn text(&self, values: &[(&'static str, String)]) -> String {
        let mut joined = self.lines.join("\n");
        for (k, v) in values {
            joined = joined.replace(k, v);
        }
        crate::texts::legacy(&joined)
    }
}

/// The hologram style of config.yml `holograms`, as text display data.
fn hologram_data() -> Vec<Value> {
    let (background, alignment, billboard, see_through, shadow) = crate::settings(|s| {
        (s.config.str("holograms.background-color", "00000000"), s.config.str("holograms.alignment", "center"), s.config.str("holograms.billboard", "vertical"), s.config.bool("holograms.see-through", false), s.config.bool("holograms.shadow", true))
    })
    .unwrap_or_else(|| ("00000000".into(), "center".into(), "vertical".into(), false, true));
    let argb = u32::from_str_radix(background.trim().trim_start_matches('#'), 16).unwrap_or(0) as i32;
    let mut style = 0i64;
    if shadow {
        style |= 1;
    }
    if see_through {
        style |= 2;
    }
    match alignment.to_ascii_lowercase().as_str() {
        "left" => style |= 8,
        "right" => style |= 16,
        _ => {}
    }
    let board = match billboard.to_ascii_lowercase().as_str() {
        "vertical" => 1,
        "horizontal" => 2,
        "center" => 3,
        _ => 0,
    };
    vec![packets::entry(15, packets::BYTE, json!(board)), packets::entry(25, packets::INT, json!(argb)), packets::entry(27, packets::BYTE, json!(style))]
}

/// An item section (`material`, `glow`...) as the client's item data: the item and its glint.
pub fn item_json(section: &serde_yaml::Value) -> Value {
    let spec = horizon_kit::item::Spec::from_yaml(section);
    let material = horizon_bukkit::Material::of(&spec.material.to_ascii_lowercase().replace([' ', '-'], "_"));
    let glow = spec.glow.as_deref().is_some_and(|g| g.trim().eq_ignore_ascii_case("true"));
    let components = if glow { json!({ "minecraft:enchantment_glint_override": "true" }) } else { json!({}) };
    json!({ "item": material.key(), "count": 1, "components": components })
}

#[derive(Default)]
pub struct Entities {
    pub npcs: Vec<Ent>,
    pub block: Option<Ent>,
    pub item: Option<Ent>,
    pub holos: Vec<(i32, Ent)>,
    /// The display's hologram lines while showing and while bidding.
    preview_lines: Vec<String>,
    bidding_lines: Vec<String>,
    block_lines: Vec<String>,
}

impl Entities {
    fn every(&mut self) -> impl Iterator<Item = &mut Ent> {
        self.npcs.iter_mut().chain(self.block.iter_mut()).chain(self.item.iter_mut()).chain(self.holos.iter_mut().map(|(_, h)| h))
    }

    pub fn any_live(&self) -> bool {
        self.npcs.iter().chain(self.block.iter()).chain(self.item.iter()).chain(self.holos.iter().map(|(_, h)| h)).any(|e| e.live || !e.seen.is_empty())
    }

    pub fn find(&self, id: i32) -> Option<Role> {
        self.npcs.iter().chain(self.block.iter()).chain(self.item.iter()).find(|e| e.id == id).map(|e| e.role)
    }

    /// AuctionEntities.reload: the entities made again from the file (shown again by `start`).
    pub fn reload(&mut self, cfg: &Config, display: Option<&Location>) {
        for e in self.every() {
            e.hide_all();
        }
        self.npcs.clear();
        self.holos.clear();
        self.block = None;
        self.item = None;
        for (route, role) in [("entrance", Role::Entrance), ("exit", Role::Exit), ("auctioneer", Role::Auctioneer)] {
            let base = format!("npcs.{route}");
            if !cfg.bool(&format!("{base}.enabled"), false) {
                continue;
            }
            let Some(at) = yaml::string(&cfg.doc, &format!("{base}.location")).and_then(|s| parse_location(&s)) else { continue };
            let kind = yaml::string(&cfg.doc, &format!("{base}.type")).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "VILLAGER".into());
            let kind = format!("minecraft:{}", kind.trim().to_ascii_lowercase());
            let npc = Ent::new(role, &kind, &at, vec![packets::entry(packets::NO_GRAVITY, packets::BOOL, json!(true)), packets::entry(packets::SILENT, packets::BOOL, json!(true))]);
            let lines = cfg.list(&format!("{base}.name"));
            if !lines.is_empty() {
                self.holos.push((npc.id, hologram(&at, &lines, true)));
            }
            self.npcs.push(npc);
        }
        self.preview_lines = cfg.list("display.preview-lines");
        self.bidding_lines = cfg.list("display.bidding-lines");
        self.block_lines = cfg.list("npcs.display.lines");
        if cfg.bool("display.enabled", false)
            && let Some(d) = display
        {
            let mut stand_at = d.clone();
            stand_at.add(0.5, -1.4, 0.5);
            let mut stand = Ent::new(Role::Block, "minecraft:armor_stand", &stand_at, packets::stand_data(None, false, false));
            stand.helmet = Some(item_json(cfg.at("display.block").unwrap_or(&serde_yaml::Value::Null)));
            if !self.block_lines.is_empty() {
                self.holos.push((stand.id, hologram(&stand_at, &self.block_lines, true)));
            }
            self.block = Some(stand);
            let mut item_at = d.clone();
            item_at.add(0.5, 0.0, 0.5);
            self.item = Some(Ent::new(Role::Item, "minecraft:item", &item_at, vec![packets::entry(packets::NO_GRAVITY, packets::BOOL, json!(true))]));
        }
    }

    fn holo_of(&mut self, owner: i32) -> Option<&mut Ent> {
        self.holos.iter_mut().find(|(o, _)| *o == owner).map(|(_, h)| h)
    }

    /// start: the NPCs and the display block, with their holograms.
    pub fn start(&mut self) {
        let owners: Vec<i32> = self.npcs.iter().map(|n| n.id).chain(self.block.iter().map(|b| b.id)).collect();
        for n in self.npcs.iter_mut() {
            n.live = true;
        }
        if let Some(b) = self.block.as_mut() {
            b.live = true;
        }
        for o in owners {
            if let Some(h) = self.holo_of(o) {
                h.live = true;
            }
        }
    }

    pub fn stop(&mut self) {
        for e in self.every() {
            e.live = false;
            e.hide_all();
        }
    }

    /// showing: the item floats over the block, its hologram says what it is (`bidding`: the bidding lines).
    pub fn showing(&mut self, display: &serde_yaml::Value, bidding: bool) {
        let Some(item) = self.item.as_mut() else { return };
        let id = item.id;
        let at = Location::new(horizon_bukkit::World::of(&item.world), item.pos[0], item.pos[1], item.pos[2], 0.0, 0.0);
        item.hide_all();
        item.data = vec![packets::entry(packets::NO_GRAVITY, packets::BOOL, json!(true)), packets::entry(packets::ITEM_STACK, packets::ITEM, item_json(display))];
        item.live = true;
        if let Some(b) = self.block.as_mut() {
            b.live = true;
        }
        let lines = if bidding { self.bidding_lines.clone() } else { self.preview_lines.clone() };
        self.holos.retain_mut(|(o, h)| {
            if *o == id {
                h.hide_all();
                false
            } else {
                true
            }
        });
        if !lines.is_empty() {
            let mut h = hologram(&at, &lines, false);
            h.live = true;
            self.holos.push((id, h));
        }
    }

    /// bidding: the item's hologram changes to the bidding lines.
    pub fn bidding(&mut self) {
        let Some(id) = self.item.as_ref().map(|i| i.id) else { return };
        let lines = self.bidding_lines.clone();
        if let Some(h) = self.holo_of(id) {
            h.lines = lines;
        }
    }

    /// cleanup: the item, its hologram and the block go until the next item.
    pub fn cleanup(&mut self) {
        let item = self.item.as_ref().map(|i| i.id);
        for e in self.item.iter_mut().chain(self.block.iter_mut()) {
            e.live = false;
            e.hide_all();
        }
        if let Some(id) = item {
            self.holos.retain_mut(|(o, h)| {
                if *o == id {
                    h.hide_all();
                    false
                } else {
                    true
                }
            });
        }
    }

    /// Once a second: who should see what (near enough, same world), and holograms whose text changed.
    pub fn tick(&mut self, online: &[Player], values: &[(&'static str, String)]) {
        if !self.any_live() {
            return;
        }
        let where_: Vec<(String, String, [f64; 3])> = online
            .iter()
            .map(|p| {
                let at = p.get_location();
                (p.get_unique_id(), at.world.get_name().to_owned(), [at.x, at.y, at.z])
            })
            .collect();
        let holo_data = hologram_data();
        for e in self.every() {
            let wanted: HashSet<&str> = if e.live {
                where_
                    .iter()
                    .filter(|(_, w, p)| *w == e.world && (p[0] - e.pos[0]).powi(2) + (p[1] - e.pos[1]).powi(2) + (p[2] - e.pos[2]).powi(2) <= RANGE * RANGE)
                    .map(|(u, _, _)| u.as_str())
                    .collect()
            } else {
                HashSet::new()
            };
            let gone: Vec<String> = e.seen.keys().filter(|u| !wanted.contains(u.as_str())).cloned().collect();
            for u in &gone {
                e.seen.remove(u);
            }
            packets::remove(&gone, &[e.id]);
            let text = (e.role == Role::Text).then(|| e.text(values));
            let new: Vec<String> = wanted.iter().filter(|u| !e.seen.contains_key(**u)).map(|u| (*u).to_owned()).collect();
            if !new.is_empty() {
                packets::spawn(&new, e.id, &e.kind, e.pos, e.yaw);
                let mut data = e.data.clone();
                if let Some(t) = &text {
                    data.extend(holo_data.iter().cloned());
                    data.push(packets::entry(23, packets::COMPONENT, horizon_kit::text::component(t)));
                }
                packets::data(&new, e.id, data);
                if let Some(h) = &e.helmet {
                    packets::equip(&new, e.id, 5, h.clone());
                }
                for u in new {
                    e.seen.insert(u, text.clone().unwrap_or_default());
                }
            }
            if let Some(t) = text {
                let changed: Vec<String> = e.seen.iter().filter(|(_, last)| **last != t).map(|(u, _)| u.clone()).collect();
                if !changed.is_empty() {
                    packets::data(&changed, e.id, vec![packets::entry(23, packets::COMPONENT, horizon_kit::text::component(&t))]);
                    for u in changed {
                        e.seen.insert(u, t.clone());
                    }
                }
            }
        }
    }
}

/// spawnHologram: a text display over an entity (NPCs 2.1 blocks up, the item 1.1), all lines in one.
fn hologram(at: &Location, lines: &[String], npc: bool) -> Ent {
    let n = lines.len() as f64;
    let height = (if npc { 1.6 } else { 0.6 }) + n * 0.25 - 0.25 * (n - 1.0) + 0.25;
    let mut above = at.clone();
    above.add(0.0, height, 0.0);
    let mut h = Ent::new(Role::Text, "minecraft:text_display", &above, Vec::new());
    h.lines = lines.iter().map(|l| format!("{l}&r")).collect();
    h
}

static COOLDOWN: sync::Cooldowns = sync::Cooldowns::new();

/// A click on one of them: the entrance joins, the exit leaves (each at most every 250 ms), the auctioneer, the
/// block and the item open the auction's GUI of the moment.
pub fn clicked(p: &Player, id: i32) {
    let found = {
        let all = crate::lock(&crate::auction::AUCTIONS);
        all.values().find_map(|a| a.entities.find(id).map(|r| (a.name.clone(), r, a.state)))
    };
    let Some((name, role, state)) = found else { return };
    let who = p.clone();
    match role {
        Role::Entrance | Role::Exit => {
            if !COOLDOWN.try_start(&p.get_unique_id(), 250) {
                return;
            }
            crate::later(move || {
                if role == Role::Entrance {
                    crate::commands::join(&who, Some(name));
                } else {
                    crate::commands::leave(&who);
                }
            });
        }
        _ => {
            let kind = match state {
                State::Bidding => crate::menus::Kind::Bidding,
                State::Showing => crate::menus::Kind::Preview,
                _ => return,
            };
            crate::later(move || crate::menus::open(&who, &name, kind));
        }
    }
}
