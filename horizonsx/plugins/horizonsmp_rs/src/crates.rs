//! Crates (crates.yml): blocks placed by staff that open with a crate's key, spinning a strip of rewards that
//! slows down and lands on the prize (chosen by weight before the spin starts). Left-click a crate to see its
//! rewards and their odds. Keys are real items (tagged, so renamed tripwire hooks don't work), given by votes,
//! daily rewards, playtime rewards, other crates or `/crate give`.

use crate::{data, ui};
use horizon_bukkit::prelude::*;
use horizon_kit::store::File;
use horizon_kit::{Config, app, gui, hooks::economy, item, yaml};
use serde_json::{Value, json};
use serde_yaml::Value as Yaml;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

const SECTION: &str = "Crates";
const DEFAULT: &str = include_str!("../data/crates.yml");
const KEY_TAG: &str = "horizonsmp_crate_key";

#[derive(Clone, Debug, Default)]
pub struct Prize {
    pub item: Option<String>,
    pub money: f64,
    pub keys: Vec<(String, u32)>,
    pub command: Option<String>,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub chance: f64,
    pub broadcast: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Crate {
    pub id: String,
    pub name: String,
    pub colour: String,
    pub key: String,
    pub prizes: Vec<Prize>,
}

static CRATES: RwLock<Option<Arc<Vec<Crate>>>> = RwLock::new(None);

pub fn reload() {
    let c = Config::load(&app::folder(), "crates.yml", DEFAULT);
    let mut out = Vec::new();
    if let Some(Yaml::Mapping(m)) = c.at("crates") {
        for (k, v) in m {
            let Some(id) = yaml::key_str(k) else { continue };
            let mut prizes = Vec::new();
            if let Some(Yaml::Sequence(list)) = v.get("rewards") {
                for r in list {
                    let mut keys = Vec::new();
                    if let Some(Yaml::Mapping(km)) = r.get("keys") {
                        for (kk, kv) in km {
                            if let (Some(kk), Some(kv)) = (yaml::key_str(kk), kv.as_u64()) {
                                keys.push((kk.to_ascii_lowercase(), kv as u32));
                            }
                        }
                    }
                    prizes.push(Prize {
                        item: yaml::string(r, "item"),
                        money: r.get("money").and_then(Yaml::as_f64).unwrap_or(0.0),
                        keys,
                        command: yaml::string(r, "command"),
                        name: yaml::string(r, "name"),
                        icon: yaml::string(r, "icon"),
                        chance: r.get("chance").and_then(Yaml::as_f64).unwrap_or(1.0).max(0.0),
                        broadcast: r.get("broadcast").and_then(Yaml::as_bool).unwrap_or(false),
                    });
                }
            }
            out.push(Crate {
                id: id.to_ascii_lowercase(),
                name: yaml::string(v, "name").unwrap_or_else(|| ui::pretty(&id)),
                colour: yaml::string(v, "colour").or_else(|| yaml::string(v, "color")).unwrap_or_else(|| "#a855f7".into()),
                key: yaml::string(v, "key").unwrap_or_else(|| "TRIPWIRE_HOOK".into()).to_ascii_uppercase(),
                prizes,
            });
        }
    }
    *CRATES.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(out));
}

pub fn all() -> Arc<Vec<Crate>> {
    if let Some(c) = CRATES.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return c.clone();
    }
    reload();
    CRATES.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

pub fn find(id: &str) -> Option<Crate> {
    all().iter().find(|c| c.id.eq_ignore_ascii_case(id)).cloned()
}

pub fn display_name(id: &str) -> String {
    find(id).map(|c| c.name).unwrap_or_else(|| ui::pretty(id))
}

// ---- Items ----

/// `DIAMOND_SWORD 1 sharpness:4,unbreaking:3` -> the item.
fn parse_item(spec: &str) -> Option<ItemStack> {
    let mut w = spec.split_whitespace();
    let mat = Material::get_material(&w.next()?.to_ascii_uppercase())?;
    let n = w.next().and_then(|s| s.parse::<i32>().ok()).unwrap_or(1).max(1);
    let mut s = ItemStack::new(mat, n);
    if let Some(ench) = w.next() {
        for part in ench.split(',') {
            if let Some((e, l)) = part.split_once(':') {
                s.add_enchant(&item::enchantment_key(e), l.parse().unwrap_or(1));
            }
        }
    }
    Some(s)
}

impl Prize {
    pub fn label(&self) -> String {
        if let Some(n) = &self.name {
            return n.clone();
        }
        if let Some(i) = &self.item {
            let mut w = i.split_whitespace();
            let mat = w.next().unwrap_or("");
            let n = w.next().unwrap_or("1");
            let ench = if w.next().is_some() { " <#a855f7>✦" } else { "" };
            return format!("{n}× {}{ench}", ui::pretty(mat));
        }
        if self.money > 0.0 {
            return ui::money(self.money);
        }
        if let Some((k, n)) = self.keys.first() {
            return format!("{n}× {} Key", display_name(k));
        }
        "Mystery prize".into()
    }
    pub fn icon_item(&self) -> ItemStack {
        let mut s = match (&self.icon, &self.item) {
            (Some(i), _) => ItemStack::new(Material::get_material(&i.to_ascii_uppercase()).unwrap_or_else(|| Material::of("CHEST")), 1),
            (None, Some(spec)) => parse_item(spec).unwrap_or_else(|| ItemStack::new(Material::of("CHEST"), 1)),
            _ if self.money > 0.0 => ItemStack::new(Material::of("GOLD_NUGGET"), 1),
            _ => ItemStack::new(Material::of("TRIPWIRE_HOOK"), 1),
        };
        s.set_amount(s.get_amount().clamp(1, 64));
        s
    }
    fn give(&self, p: &Player) {
        if let Some(spec) = &self.item
            && let Some(s) = parse_item(spec)
        {
            crate::give_item(p, &s);
        }
        if self.money > 0.0 {
            economy::deposit(p.get_name(), self.money);
        }
        for (k, n) in &self.keys {
            give_keys(p, k, *n);
        }
        if let Some(cmd) = &self.command {
            horizon_bukkit::dispatch_command(&cmd.replace("%player%", p.get_name()).replace("{player}", p.get_name()));
        }
    }
}

/// A key for a crate.
pub fn key_item(c: &Crate, n: u32) -> ItemStack {
    let mut s = ItemStack::new(Material::get_material(&c.key).unwrap_or_else(|| Material::of("TRIPWIRE_HOOK")), n.clamp(1, 64) as i32);
    item::name(&mut s, &format!("<{}><bold>{} Key", c.colour, c.name));
    item::lore(&mut s, &[
        String::new(),
        format!("<#a1a1aa>Opens a <{}>{}</{}> <#a1a1aa>crate.", c.colour, c.name, c.colour),
        "<#71717a>Find the crates at spawn.".into(),
        String::new(),
        "<#22d3ee>▸ Right-click a crate to open".into(),
        "<#22d3ee>▸ Left-click a crate to preview".into(),
    ]);
    ui::glint(&mut s);
    s.set_component("minecraft:custom_data", json!({ KEY_TAG: c.id }));
    s
}

pub fn key_of(s: &ItemStack) -> Option<String> {
    s.get_component("minecraft:custom_data").and_then(|v| v.get(KEY_TAG)).and_then(Value::as_str).map(str::to_owned)
}

pub fn give_keys(p: &Player, id: &str, n: u32) {
    let Some(c) = find(id) else { return };
    let mut left = n;
    while left > 0 {
        let k = left.min(64);
        crate::give_item(p, &key_item(&c, k));
        left -= k;
    }
}

fn count_keys(p: &Player, id: &str) -> u32 {
    p.get_inventory().get_storage_contents().iter().flatten().filter(|s| key_of(s).as_deref() == Some(id)).map(|s| s.get_amount().max(0) as u32).sum()
}

/// Takes one key of `id` from the hand (else anywhere); false when there is none.
fn take_key(p: &Player, id: &str) -> bool {
    let inv = p.get_inventory();
    let held = inv.get_held_item_slot().max(0) as usize;
    let contents = inv.get_storage_contents();
    let mut order: Vec<usize> = vec![held];
    order.extend((0..contents.len()).filter(|s| *s != held));
    for slot in order {
        let Some(Some(s)) = contents.get(slot) else { continue };
        if key_of(s).as_deref() != Some(id) {
            continue;
        }
        if s.get_amount() <= 1 {
            inv.clear_slot(slot as i32);
        } else {
            let mut rest = s.clone();
            rest.set_amount(s.get_amount() - 1);
            inv.set_item(slot as i32, &rest);
        }
        return true;
    }
    false
}

// ---- Where the crates are ----

fn places() -> &'static File<BTreeMap<String, String>> {
    static FILE: OnceLock<File<BTreeMap<String, String>>> = OnceLock::new();
    FILE.get_or_init(|| File::open(&app::folder().join("crate-blocks.json"), Duration::from_secs(2)))
}

pub fn save() {
    places().save_now();
}

/// Forgets the crates in a column area (a spawn being built again), whatever their height.
pub fn remove_near(world: &str, cx: i32, cz: i32, r: i32) {
    places().edit(|m| {
        m.retain(|k, _| {
            let mut w = k.split(';');
            let (Some(wn), Some(x), Some(_), Some(z)) = (w.next(), w.next().and_then(|v| v.parse::<i32>().ok()), w.next(), w.next().and_then(|v| v.parse::<i32>().ok())) else { return true };
            !(wn == world && (x - cx).abs() <= r && (z - cz).abs() <= r)
        });
    });
}

/// Makes the block at a place a crate (spawn setup).
pub fn place(world: &str, x: i32, y: i32, z: i32, id: &str) {
    places().edit(|m| {
        m.insert(format!("{world};{x};{y};{z}"), id.to_owned());
    });
}

fn place_key(b: &Block) -> String {
    format!("{};{};{};{}", b.get_world().get_name(), b.get_x(), b.get_y(), b.get_z())
}

fn crate_at(b: &Block) -> Option<Crate> {
    let id = places().read(|m| m.get(&place_key(b)).cloned())?;
    find(&id)
}

/// What staff is about to do with the next block they click: put a crate there, or take one away.
static PENDING: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

fn pending<R>(f: impl FnOnce(&mut HashMap<String, Option<String>>) -> R) -> R {
    let mut held = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    f(held.get_or_insert_with(HashMap::new))
}

// ---- The spin ----

struct Spin {
    crate_id: String,
    strip: Vec<usize>,
    pos: usize,
    steps: usize,
    wait: u32,
    done: bool,
    awarded: bool,
    close_in: u32,
}

static SPINS: Mutex<Option<HashMap<String, Spin>>> = Mutex::new(None);

fn spins<R>(f: impl FnOnce(&mut HashMap<String, Spin>) -> R) -> R {
    let mut held = SPINS.lock().unwrap_or_else(|e| e.into_inner());
    f(held.get_or_insert_with(HashMap::new))
}

fn pick(c: &Crate) -> usize {
    let total: f64 = c.prizes.iter().map(|p| p.chance).sum();
    if total <= 0.0 {
        return 0;
    }
    let mut r = ui::rand_f64() * total;
    for (i, p) in c.prizes.iter().enumerate() {
        if r < p.chance {
            return i;
        }
        r -= p.chance;
    }
    c.prizes.len() - 1
}

fn prize_icon(c: &Crate, i: usize, winner: bool) -> ItemStack {
    let p = &c.prizes[i];
    let total: f64 = c.prizes.iter().map(|p| p.chance).sum::<f64>().max(1e-9);
    let mut s = p.icon_item();
    item::name(&mut s, &format!("{}<bold>{}", if winner { "<#4ade80>" } else { "<white>" }, p.label()));
    item::lore(&mut s, &[format!("<#71717a>{:.1}% chance", p.chance / total * 100.0)]);
    ui::hide_extras(&mut s);
    if winner {
        ui::glint(&mut s);
    }
    s
}

const STEPS: usize = 34;

fn delay(step: usize) -> u32 {
    1 + (step * step / 140) as u32
}

fn open_crate(p: &Player, c: &Crate) {
    let uuid = p.get_unique_id();
    if c.prizes.is_empty() || spins(|s| s.contains_key(&uuid)) {
        return;
    }
    if !crate::allowed(p, "horizonsmp.crates.open") {
        return;
    }
    if !take_key(p, &c.id) {
        ui::deny(p, &format!("You need a {} Key to open this crate.", c.name));
        preview(p, c);
        return;
    }
    let winner = pick(c);
    let mut strip: Vec<usize> = (0..STEPS + 9).map(|_| pick(c)).collect();
    strip[STEPS + 4] = winner;
    let mut m = gui::Menu::new(3, &format!("&8Opening {} Crate...", ui::pretty(&c.name)));
    m.fill(gui::filler("BLACK_STAINED_GLASS_PANE"));
    let marker = ui::icon("LIME_STAINED_GLASS_PANE", "<#4ade80>▼", &[]);
    m.item(4, marker.clone());
    m.item(22, ui::icon("LIME_STAINED_GLASS_PANE", "<#4ade80>▲", &[]));
    for i in 0..9 {
        m.item(9 + i, prize_icon(c, strip[i], false));
    }
    m.tag("horizonsmp:spin");
    let u2 = uuid.clone();
    m.on_close(move |pl| {
        // Closed early: the prize is still theirs.
        let unpaid = spins(|s| s.get_mut(&u2).map(|sp| !sp.awarded).unwrap_or(false));
        if unpaid {
            finish(pl);
        }
        spins(|s| s.remove(&u2));
    });
    m.open(p);
    spins(|s| s.insert(uuid, Spin { crate_id: c.id.clone(), strip, pos: 0, steps: STEPS, wait: 1, done: false, awarded: false, close_in: 0 }));
    data::edit(&p.get_unique_id(), |d| d.crates_opened += 1);
    ui::sound(p, "BLOCK_CHEST_OPEN:0.7:1.0");
}

fn finish(p: &Player) {
    let uuid = p.get_unique_id();
    let got = spins(|s| {
        let sp = s.get_mut(&uuid)?;
        if sp.awarded {
            return None;
        }
        sp.awarded = true;
        Some((sp.crate_id.clone(), sp.strip[sp.steps + 4]))
    });
    let Some((cid, idx)) = got else { return };
    let Some(c) = find(&cid) else { return };
    let Some(prize) = c.prizes.get(idx) else { return };
    prize.give(p);
    ui::say(p, SECTION, &format!("You won <white>{}</white> from the <{}>{}</{}> crate!", prize.label(), c.colour, c.name, c.colour));
    ui::title(p, &format!("<{}><bold>{}</bold>", c.colour, prize.label()), &format!("<#a1a1aa>from the {} crate", c.name));
    ui::fanfare(p);
    if prize.broadcast {
        horizon_bukkit::server::send_rich_message(&format!("<gradient:{}:{}><bold>Crates</bold></gradient> <#3f3f46>» <white>{}</white> <#a1a1aa>won <white>{}</white> from a <{}><bold>{}</bold></{}> <#a1a1aa>crate!", ui::ACCENT, ui::ACCENT2, p.get_name(), prize.label(), c.colour, c.name, c.colour));
    }
}

/// Every tick: each spin moves on when its wait is up.
pub fn tick() {
    let ids: Vec<String> = spins(|s| s.keys().cloned().collect());
    for uuid in ids {
        let Some(p) = horizon_bukkit::fam_ess::get_player_by_id(&uuid) else {
            spins(|s| s.remove(&uuid));
            continue;
        };
        enum Do {
            Nothing,
            Move(String, Vec<usize>, usize),
            Land(String, usize),
            Close,
        }
        let what = spins(|s| {
            let Some(sp) = s.get_mut(&uuid) else { return Do::Nothing };
            if sp.done {
                if sp.close_in > 0 {
                    sp.close_in -= 1;
                    return Do::Nothing;
                }
                return Do::Close;
            }
            if sp.wait > 1 {
                sp.wait -= 1;
                return Do::Nothing;
            }
            sp.pos += 1;
            if sp.pos >= sp.steps {
                sp.done = true;
                sp.close_in = 50;
                return Do::Land(sp.crate_id.clone(), sp.strip[sp.steps + 4]);
            }
            sp.wait = delay(sp.pos);
            Do::Move(sp.crate_id.clone(), sp.strip[sp.pos..sp.pos + 9].to_vec(), sp.pos)
        });
        match what {
            Do::Nothing => {}
            Do::Move(cid, window, pos) => {
                if let Some(c) = find(&cid) {
                    for (i, idx) in window.iter().enumerate() {
                        gui::set_slot(&p, 9 + i, Some(prize_icon(&c, *idx, false)));
                    }
                    let pitch = 0.6 + (pos as f32 / STEPS as f32) * 1.2;
                    p.play_sound(&p.get_location(), "minecraft:block.note_block.hat", 0.6, pitch);
                }
            }
            Do::Land(cid, idx) => {
                if let Some(c) = find(&cid) {
                    gui::set_slot(&p, 13, Some(prize_icon(&c, idx, true)));
                    for s in [0usize, 1, 2, 3, 5, 6, 7, 8, 18, 19, 20, 21, 23, 24, 25, 26] {
                        gui::set_slot(&p, s, Some(gui::filler("LIME_STAINED_GLASS_PANE")));
                    }
                }
                finish(&p);
            }
            Do::Close => {
                spins(|s| s.remove(&uuid));
                if gui::open_tag(&p).as_deref() == Some("horizonsmp:spin") {
                    p.close_inventory();
                }
            }
        }
    }
}

pub fn preview(p: &Player, c: &Crate) {
    let mut m = ui::frame(6, &format!("✦ {} Crate", c.name));
    let have = count_keys(p, &c.id);
    let mut k = key_item(c, 1);
    item::lore(&mut k, &[String::new(), format!("<#a1a1aa>Your keys  <white>{have}"), String::new(), "<#71717a>Get keys from /daily, /rewards,".into(), "<#71717a>voting and other crates.".into()]);
    m.item(4, k);
    let mut order: Vec<usize> = (0..c.prizes.len()).collect();
    order.sort_by(|a, b| c.prizes[*b].chance.total_cmp(&c.prizes[*a].chance));
    for (slot, i) in ui::inner(6).into_iter().zip(order) {
        m.item(slot, prize_icon(c, i, false));
    }
    ui::back_button(&mut m, "crates", open_menu);
    ui::close_button(&mut m);
    m.open(p);
}

/// All crates: what each holds and how many keys the player has.
pub fn open_menu(p: &Player) {
    if !crate::allowed(p, "horizonsmp.crates") {
        return;
    }
    let list = all();
    let mut m = ui::frame(4, "✦ Crates");
    let slots: Vec<usize> = match list.len() {
        1 => vec![13],
        2 => vec![12, 14],
        3 => vec![11, 13, 15],
        4 => vec![10, 12, 14, 16],
        _ => (10..17).collect(),
    };
    for (slot, c) in slots.into_iter().zip(list.iter()) {
        let have = count_keys(p, &c.id);
        let mut icon = key_item(c, have.max(1));
        item::name(&mut icon, &format!("<{}><bold>{} Crate", c.colour, c.name));
        item::lore(&mut icon, &[String::new(), format!("<#a1a1aa>Your keys  <white>{have}"), format!("<#a1a1aa>Prizes      <white>{}", c.prizes.len()), String::new(), "<#22d3ee>▸ Click to see the prizes".into()]);
        let c2 = c.clone();
        m.button(slot, icon, move |cx| {
            ui::click(&cx.player);
            preview(&cx.player, &c2);
        });
    }
    m.item(31, ui::icon("COMPASS", "<#22d3ee><bold>Where are the crates?", &ui::lines(&["", "<#a1a1aa>At spawn. Right-click one", "<#a1a1aa>with its key to open it."])));
    ui::back_button(&mut m, "the server menu", crate::menu::open);
    ui::close_button(&mut m);
    m.open(p);
}

// ---- Events ----

fn on_interact(e: &mut PlayerInteractEvent) {
    let action = e.get_action().to_owned();
    if action != "RIGHT_CLICK_BLOCK" && action != "LEFT_CLICK_BLOCK" {
        return;
    }
    if e.get_hand().is_some_and(|h| h.eq_ignore_ascii_case("OFF_HAND")) {
        return;
    }
    let Some(b) = e.get_clicked_block().cloned() else { return };
    let p = e.get_player().clone();
    let uuid = p.get_unique_id();
    if let Some(job) = pending(|m| m.remove(&uuid)) {
        e.set_cancelled(true);
        match job {
            Some(id) => {
                places().edit(|m| m.insert(place_key(&b), id.clone()));
                ui::say(&p, SECTION, &format!("<#4ade80>That block is now a {} crate.", display_name(&id)));
            }
            None => {
                let gone = places().edit(|m| m.remove(&place_key(&b)));
                ui::say(&p, SECTION, if gone.is_some() { "Crate removed (the block stays)." } else { "<#f87171>That block wasn't a crate." });
            }
        }
        return;
    }
    let Some(c) = crate_at(&b) else { return };
    e.set_cancelled(true);
    if action == "LEFT_CLICK_BLOCK" {
        preview(&p, &c);
        return;
    }
    let p2 = p.clone();
    app::later(0, move || open_crate(&p2, &c));
}

fn on_break(e: &mut BlockBreakEvent) {
    if crate_at(e.get_block()).is_some() {
        e.set_cancelled(true);
        let p = e.get_player().clone();
        if p.has_permission("horizonsmp.crates.admin") {
            ui::deny(&p, "That's a crate. /crate remove, then click it, to take it away.");
        }
    }
}

/// Keys are not for placing (a tripwire hook key would be lost).
fn on_place(e: &mut BlockPlaceEvent) {
    if e.get_item_in_hand().and_then(key_of).is_some() {
        e.set_cancelled(true);
    }
}

/// Every two seconds: a little sparkle over every crate someone is near.
pub fn sparkle() {
    let near: Vec<(String, f64, f64, f64)> = places().read(|m| {
        m.keys()
            .filter_map(|k| {
                let mut w = k.split(';');
                let world = w.next()?.to_owned();
                let x: f64 = w.next()?.parse().ok()?;
                let y: f64 = w.next()?.parse().ok()?;
                let z: f64 = w.next()?.parse().ok()?;
                Some((world, x, y, z))
            })
            .collect()
    });
    if near.is_empty() {
        return;
    }
    let players = horizon_bukkit::get_online_players();
    let ids: Vec<(String, String)> = places().read(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    // Who stands at a crate gets its name and how to use it.
    for p in &players {
        let l = p.get_location();
        for (key, id) in &ids {
            let mut w = key.split(';');
            let (Some(world), Some(x), Some(y), Some(z)) = (w.next(), w.next().and_then(|s| s.parse::<f64>().ok()), w.next().and_then(|s| s.parse::<f64>().ok()), w.next().and_then(|s| s.parse::<f64>().ok())) else { continue };
            if l.get_world().get_name() == world && (l.x - x - 0.5).powi(2) + (l.z - z - 0.5).powi(2) < 12.0 && (l.y - y).abs() < 3.0
                && let Some(c) = find(id)
            {
                let keys = count_keys(p, &c.id);
                ui::bar(p, &format!("<{}><bold>✦ {} Crate</bold></{}> <#71717a>• <white>right-click</white> <#71717a>with a key ({keys}) • <white>left-click</white> <#71717a>to preview", c.colour, c.name, c.colour));
            }
        }
    }
    for (world, x, y, z) in near {
        let seen = players.iter().any(|p| {
            let l = p.get_location();
            l.get_world().get_name() == world && (l.x - x).abs() < 24.0 && (l.z - z).abs() < 24.0
        });
        if !seen {
            continue;
        }
        if let Some(w) = horizon_bukkit::get_world(&world) {
            let at = Location::new(w.clone(), x + 0.5, y + 1.2, z + 0.5, 0.0, 0.0);
            w.spawn_particle_forced("end_rod", &at, 6, 0.3, 0.3, 0.3, 0.01, false);
        }
    }
}

pub fn register(plugin: Plugin) {
    plugin.register_event_with("LOW", false, on_interact);
    plugin.register_event_with("LOW", false, on_break);
    plugin.register_event_with("LOW", false, on_place);
}

// ---- Commands ----

pub fn crate_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    let admin = sender.has_permission("horizonsmp.crates.admin");
    let sub = args.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    match sub.as_str() {
        "give" if admin => {
            let (Some(who), Some(id)) = (args.get(1), args.get(2)) else {
                ui::tell(sender, SECTION, "Usage: /crate give <player> <crate> [amount]");
                return true;
            };
            let n = args.get(3).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1).clamp(1, 640);
            let Some(c) = find(id) else {
                ui::tell(sender, SECTION, &format!("No crate {}. Crates: {}", ui::esc(id), all().iter().map(|c| c.id.clone()).collect::<Vec<_>>().join(", ")));
                return true;
            };
            match horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(who)) {
                Some(p) => {
                    give_keys(&p, &c.id, n);
                    ui::say(&p, SECTION, &format!("You got <white>{n}× <{}>{} Key</{}></white>!", c.colour, c.name, c.colour));
                    ui::good(&p);
                    ui::tell(sender, SECTION, &format!("Gave {} {n}× {} Key.", p.get_name(), c.name));
                }
                None => ui::tell(sender, SECTION, &format!("{} isn't online.", ui::esc(who))),
            }
        }
        "giveall" if admin => {
            let Some(c) = args.get(1).and_then(|id| find(id)) else {
                ui::tell(sender, SECTION, "Usage: /crate giveall <crate> [amount]");
                return true;
            };
            let n = args.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1).clamp(1, 64);
            for p in horizon_bukkit::get_online_players() {
                give_keys(&p, &c.id, n);
                ui::good(&p);
            }
            horizon_bukkit::server::send_rich_message(&format!("<gradient:{}:{}><bold>Crates</bold></gradient> <#3f3f46>» <#a1a1aa>Everyone online got <white>{n}× <{}>{} Key</{}></white>!", ui::ACCENT, ui::ACCENT2, c.colour, c.name, c.colour));
        }
        "set" | "place" if admin => {
            let Some(p) = sender.as_player() else { return true };
            let Some(c) = args.get(1).and_then(|id| find(id)) else {
                ui::tell(sender, SECTION, "Usage: /crate set <crate>, then click a block.");
                return true;
            };
            pending(|m| m.insert(p.get_unique_id(), Some(c.id.clone())));
            ui::say(p, SECTION, &format!("Now click the block that should be the <white>{}</white> crate.", c.name));
        }
        "remove" | "delete" if admin => {
            let Some(p) = sender.as_player() else { return true };
            pending(|m| m.insert(p.get_unique_id(), None));
            ui::say(p, SECTION, "Now click the crate to remove.");
        }
        "list" if admin => {
            let rows: Vec<String> = places().read(|m| m.iter().map(|(k, v)| format!(" <#52525b>•</#52525b> <white>{v}</white> <#71717a>{}", k.replace(';', " "))).collect());
            ui::tell(sender, SECTION, &format!("{} crate block(s):", rows.len()));
            ui::tell_raw(sender, &rows.join("\n"));
        }
        "reload" if admin => {
            reload();
            ui::tell(sender, SECTION, "Crates reloaded.");
        }
        "preview" => {
            if let (Some(p), Some(c)) = (sender.as_player(), args.get(1).and_then(|id| find(id))) {
                preview(&p.clone(), &c);
            }
        }
        _ => {
            if let Some(p) = sender.as_player() {
                open_menu(&p.clone());
            } else {
                ui::tell(sender, SECTION, "/crate give <player> <crate> [n] • giveall <crate> [n] • list • reload");
            }
        }
    }
    true
}

pub fn complete(sender: &CommandSender, args: &[&str]) -> Vec<String> {
    let admin = sender.has_permission("horizonsmp.crates.admin");
    let ids: Vec<String> = all().iter().map(|c| c.id.clone()).collect();
    let filter = |list: Vec<String>, w: &str| list.into_iter().filter(|s| s.to_ascii_lowercase().starts_with(&w.to_ascii_lowercase())).collect::<Vec<_>>();
    match args {
        [one] => filter(if admin { ["give", "giveall", "set", "remove", "list", "reload", "preview"].map(String::from).to_vec() } else { vec!["preview".into()] }, one),
        [sub, two] if *sub == "give" => filter(horizon_bukkit::get_online_players().into_iter().map(|p| p.get_name().to_owned()).collect(), two),
        [sub, two] if matches!(*sub, "giveall" | "set" | "preview") => filter(ids, two),
        [sub, _, three] if *sub == "give" => filter(ids, three),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spin_timing() {
        let total: u32 = (1..STEPS).map(delay).sum();
        // About four to six seconds.
        assert!((70..130).contains(&total), "{total}");
        let v: serde_yaml::Value = serde_yaml::from_str(DEFAULT).unwrap();
        assert_eq!(v["crates"].as_mapping().unwrap().len(), 3);
    }
}
