//! PlayerVaultsX 4.4 rewritten in Rust for Horizon: `/pv <n>`, other players' vaults for admins, vault counts
//! and sizes by permission (`playervaults.amount.N`, `playervaults.size.N`), blocked items, economy fees, vault
//! signs, the same config.conf/lang.conf and the same `newvaults/<uuid>.yml` files (Paper item bytes: an existing
//! server's vaults open as they were). Files are read and written on an I/O thread; a player's file is read
//! ahead when they join, so `/pv` opens at once. The Java it follows is in java/.

pub mod conf;
pub mod files;
pub mod hocon;

use conf::{Lang, Settings};
use files::{Slots, VaultFile};
use horizon_bukkit::fam_ev_player::{InventoryClickEvent, InventoryCloseEvent, InventoryDragEvent, PlayerInteractEvent};
use horizon_bukkit::prelude::*;
use horizon_bukkit::{CommandSender, Component, Inventory, ItemStack, Player, Plugin, bukkit_plugin};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);
static SETTINGS: Mutex<Option<Arc<(Settings, Lang)>>> = Mutex::new(None);

#[derive(Default)]
struct State {
    /// Players' files read so far (owner uuid -> vaults).
    cache: HashMap<String, VaultFile>,
    /// Vaults open now, by `owner:number`.
    open: HashMap<String, Open>,
    /// viewer uuid -> the vault they have open.
    in_vault: HashMap<String, String>,
    /// `/pvsign`: player name -> (owner, vault).
    set_sign: HashMap<String, (Option<String>, u32)>,
    /// signs.yml: `world;;x;;y;;z` -> (self, owner, vault).
    signs: BTreeMap<String, (bool, Option<String>, i64)>,
}

struct Open {
    owner: String,
    number: u32,
    inv: Inventory,
    viewers: BTreeSet<String>,
    /// What was put in when it opened (to save only what changed).
    loaded: Vec<Option<ItemStack>>,
    raw: Slots,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

type MainJob = Box<dyn FnOnce() + Send>;
static MAIN: Mutex<Vec<MainJob>> = Mutex::new(Vec::new());
static MAIN_PENDING: AtomicBool = AtomicBool::new(false);

fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(STATE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(State::default))
}

fn settings() -> Arc<(Settings, Lang)> {
    SETTINGS.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| Arc::new((Settings::default(), Lang::default())))
}

fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

fn on_main(f: impl FnOnce() + Send + 'static) {
    MAIN.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(f));
    MAIN_PENDING.store(true, Ordering::Release);
}

fn drain_main() {
    if MAIN_PENDING.swap(false, Ordering::AcqRel) {
        let jobs = std::mem::take(&mut *MAIN.lock().unwrap_or_else(|e| e.into_inner()));
        for job in jobs {
            job();
        }
    }
}

fn debug(text: &str) {
    if settings().0.debug
        && let Some(p) = plugin()
    {
        p.info(&format!("[Debug] {text}"));
    }
}

// ---- Messages ----

/// A lang.conf message, the title in front (`.title()`), `<key>` values filled.
fn tell(s: &CommandSender, key: &str, pairs: &[(&str, String)]) {
    let set = settings();
    let lang = &set.1;
    for line in lang.get(key) {
        if line.is_empty() {
            continue;
        }
        let text = lang.prepare(&line, pairs, true);
        match s.as_player() {
            Some(p) => p.send_component(&Component(horizon_kit::mini::parse_mixed(&text))),
            None => s.send_message(&horizon_kit::mini::legacy(&text)),
        }
    }
}

fn tell_p(p: &Player, key: &str, pairs: &[(&str, String)]) {
    tell(&CommandSender::Player(p.clone()), key, pairs);
}

fn title(number: u32) -> String {
    let set = settings();
    let line = set.1.get("vaultTitle").first().cloned().unwrap_or_else(|| "<dark_red>Vault #<vault>".into());
    horizon_kit::mini::legacy(&set.1.prepare(&line, &[("vault", number.to_string())], false))
}

// ---- Permissions and money ----

fn check_perms(p: &Player, number: u32) -> bool {
    let max = settings().0.max_test;
    (number..=max).any(|x| p.has_permission(&format!("playervaults.amount.{x}")))
}

/// `countVaults`: the highest `playervaults.amount.N`.
fn count_vaults(p: &Player) -> u32 {
    let max = settings().0.max_test;
    (1..=max).rev().find(|x| p.has_permission(&format!("playervaults.amount.{x}"))).unwrap_or(0)
}

fn size_for(p: Option<&Player>) -> usize {
    let Some(p) = p else { return 54 };
    (1..=6).rev().find(|i| p.has_permission(&format!("playervaults.size.{i}"))).map_or(settings().0.rows * 9, |i| i * 9)
}

fn java_double(v: f64) -> String {
    format!("{v:?}")
}

fn vault_exists(owner: &str, number: u32) -> bool {
    state(|s| s.cache.get(owner).is_some_and(|f| f.contains_key(&number)))
}

fn pay_to_open(p: &Player, number: u32) -> bool {
    let set = settings();
    let eco = &set.0;
    if !eco.eco || p.has_permission("playervaults.free") {
        return true;
    }
    if !vault_exists(&p.get_unique_id(), number) {
        if eco.fee_create == 0.0 {
            return true;
        }
        if horizon_kit::hooks::economy::withdraw(p.get_name(), eco.fee_create) {
            tell_p(p, "costToCreate", &[("price", java_double(eco.fee_create))]);
            return true;
        }
        return false;
    }
    if eco.fee_open == 0.0 {
        return true;
    }
    if horizon_kit::hooks::economy::withdraw(p.get_name(), eco.fee_open) {
        tell_p(p, "costToOpen", &[("price", java_double(eco.fee_open))]);
        return true;
    }
    false
}

// ---- Opening ----

/// The owner's file, read now if it's cached, else read on the I/O thread and `then` run after.
fn with_file(owner: &str, then: impl FnOnce() + Send + 'static) -> bool {
    if state(|s| s.cache.contains_key(owner)) {
        return true;
    }
    let key = owner.to_owned();
    files::load(owner, move |file, errors| {
        on_main(move || {
            for e in errors {
                if let Some(p) = plugin() {
                    p.warning(&e);
                }
            }
            state(|s| {
                s.cache.entry(key).or_insert(file);
            });
            then();
        });
    });
    false
}

/// The vault's window (shared when someone else has it open), made from the stored bytes.
fn vault_inventory(owner: &str, number: u32, size: usize) -> Inventory {
    let key = format!("{owner}:{number}");
    if let Some(inv) = state(|s| s.open.get(&key).map(|o| o.inv.live())) {
        return inv;
    }
    let raw: Slots = state(|s| s.cache.get(owner).and_then(|f| f.get(&number).cloned())).unwrap_or_default();
    let size = if size % 9 != 0 || size == 0 { settings().0.rows * 9 } else { size.min(54) };
    let mut inv = Inventory::create(size, &title(number));
    let stacks: Vec<Option<ItemStack>> = raw.iter().map(|b| b.as_ref().and_then(|b| ItemStack::deserialize_bytes(b)).filter(|s| !s.is_empty())).collect();
    if stacks.len() > size {
        // More slots than allowed now (a permission changed): rescued into the first free slots.
        for s in stacks.iter().flatten() {
            if let Some(rest) = inv.add_item(s) {
                debug(&format!("Vault {number} of {owner} lost {} x{} (too small)", rest.get_type().key(), rest.get_amount()));
            }
        }
    } else {
        for (slot, s) in stacks.iter().enumerate() {
            if s.is_some() {
                inv.set_item(slot, s.clone());
            }
        }
    }
    let loaded = (0..size).map(|i| inv.get_item(i).cloned()).collect();
    let raw = if stacks.len() > size { Vec::new() } else { raw };
    state(|s| s.open.insert(key, Open { owner: owner.to_owned(), number, inv: inv.clone(), viewers: BTreeSet::new(), loaded, raw }));
    inv
}

fn show(p: &Player, owner: &str, number: u32, size: usize) {
    let inv = vault_inventory(owner, number, size);
    p.open_inventory(&inv);
    let key = format!("{owner}:{number}");
    let viewer = p.get_unique_id();
    state(|s| {
        if let Some(o) = s.open.get_mut(&key) {
            o.viewers.insert(viewer.clone());
        }
        s.in_vault.insert(viewer, key);
    });
}

fn parse_number(p: &CommandSender, arg: &str) -> Option<u32> {
    match arg.parse::<i64>() {
        Ok(n) if n >= 1 => Some(n as u32),
        _ => {
            tell(p, "mustBeNumber", &[]);
            None
        }
    }
}

/// `openOwnVault`: permission, fee, then the window. `free` = a sign (no fee, no message).
fn open_own(p: &Player, arg: &str, free: bool, send: bool) {
    if p.is_sleeping() || p.is_dead() {
        return;
    }
    let s = CommandSender::Player(p.clone());
    let Some(number) = parse_number(&s, arg) else { return };
    if !check_perms(p, number) {
        return tell_p(p, "noPerms", &[]);
    }
    let owner = p.get_unique_id();
    let player = p.clone();
    let arg = arg.to_owned();
    if !with_file(&owner, move || open_own(&player, &arg, free, send)) {
        return;
    }
    if !free && !pay_to_open(p, number) {
        return tell_p(p, "insufficientFunds", &[]);
    }
    show(p, &owner, number, size_for(Some(p)));
    if send {
        tell_p(p, "openVault", &[("vault", number.to_string())]);
    }
}

/// A player's uuid and name from a name (online first, then known players), as `getTarget`.
fn target(name: &str) -> (String, String) {
    if let Some(p) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name().eq_ignore_ascii_case(name)) {
        return (p.get_unique_id(), p.get_name().to_owned());
    }
    let off = horizon_bukkit::get_offline_player(name);
    match off.get_unique_id() {
        Some(u) => (u.to_owned(), off.get_name().to_owned()),
        None => (name.to_owned(), name.to_owned()),
    }
}

/// `openOtherVault`: someone else's vault (admins, signs, the console).
fn open_other(p: &Player, owner: (String, String), arg: &str, send: bool) -> bool {
    if p.is_sleeping() || p.is_dead() {
        return false;
    }
    let s = CommandSender::Player(p.clone());
    let Some(number) = parse_number(&s, arg) else { return false };
    let player = p.clone();
    let arg_owned = arg.to_owned();
    let again = owner.clone();
    if !with_file(&owner.0, move || {
        if !open_other(&player, again.clone(), &arg_owned, send) {
            tell_p(&player, "noOwnerFound", &[("player", again.1.clone())]);
        }
    }) {
        return true;
    }
    let online = horizon_bukkit::get_online_players().into_iter().find(|x| x.get_unique_id() == owner.0);
    show(p, &owner.0, number, size_for(online.as_ref()));
    if send {
        tell_p(p, "openOtherVault", &[("vault", number.to_string()), ("player", owner.1)]);
    }
    true
}

// ---- Saving ----

/// A viewer leaves a vault; the last one out saves it (only when its contents changed).
fn leave(p: &Player, contents: Option<Vec<Option<ItemStack>>>) {
    let viewer = p.get_unique_id();
    let Some(key) = state(|s| s.in_vault.remove(&viewer)) else { return };
    let done = state(|s| {
        let o = s.open.get_mut(&key)?;
        o.viewers.remove(&viewer);
        if !o.viewers.is_empty() {
            return None;
        }
        s.open.remove(&key)
    });
    let Some(o) = done else { return };
    let now: Vec<Option<ItemStack>> = contents.unwrap_or_else(|| o.inv.live().get_contents().to_vec());
    let changed = now.len() != o.loaded.len() || now.iter().zip(&o.loaded).any(|(a, b)| a != b);
    if !changed {
        debug(&format!("Vault {} of {} unchanged, not saved", o.number, o.owner));
        return;
    }
    let slots: Slots = now
        .iter()
        .enumerate()
        .map(|(i, s)| match s.as_ref().filter(|s| !s.is_empty()) {
            None => None,
            Some(stack) if o.loaded.get(i).and_then(Option::as_ref) == Some(stack) && o.raw.get(i).is_some_and(Option::is_some) => o.raw[i].clone(),
            Some(stack) => Some(stack.serialize_as_bytes()),
        })
        .collect();
    let file = state(|s| {
        let f = s.cache.entry(o.owner.clone()).or_default();
        f.insert(o.number, slots);
        f.clone()
    });
    files::save(&o.owner, file);
    debug(&format!("Saved vault {} for {}", o.number, o.owner));
}

// ---- Blocked items ----

fn model_match(pattern: &str, model: &str) -> bool {
    let pattern = if pattern.contains(':') { pattern.to_owned() } else { format!("minecraft:{pattern}") };
    if let Some((a, b)) = pattern.split_once('*') {
        model.starts_with(a) && model.ends_with(b)
    } else {
        pattern == model
    }
}

/// Whether the item may not go in (and what to say).
fn blocked(item: &ItemStack) -> Option<(&'static str, Vec<(&'static str, String)>)> {
    let set = settings();
    let c = &set.0;
    if !c.blocking {
        return None;
    }
    let has_cmd = item.has_custom_model_data() || item.get_component("minecraft:custom_model_data").is_some();
    let model = item.get_component("minecraft:item_model").and_then(|v| v.as_str()).map(str::to_owned);
    let flag = |f: &str| c.blocked.iter().any(|b| b == f);
    if flag("BLOCK_ALL_WITH_CUSTOM_MODEL_DATA") && has_cmd {
        return Some(("blockedItemWithModelData", Vec::new()));
    }
    if flag("BLOCK_ALL_WITHOUT_CUSTOM_MODEL_DATA") && !has_cmd {
        return Some(("blockedItemWithoutModelData", Vec::new()));
    }
    if flag("BLOCK_ALL_WITH_CUSTOM_MODEL") && model.is_some() {
        return Some(("blockedItemWithModelData", Vec::new()));
    }
    if flag("BLOCK_ALL_WITHOUT_CUSTOM_MODEL") && model.is_none() {
        return Some(("blockedItemWithoutModelData", Vec::new()));
    }
    if !c.models.is_empty() {
        let matches = model.as_ref().is_some_and(|m| c.models.iter().any(|p| model_match(p, m)));
        let block = if c.models_whitelist { !matches } else { matches };
        return block.then(|| ("blockedItemModel", Vec::new()));
    }
    let name = item.get_type().name();
    if c.blocked.iter().any(|b| *b == name) {
        return Some(("blockedItem", vec![("item", name)]));
    }
    if !c.enchants.is_empty() {
        let have = item.get_enchants();
        let hit = have.keys().any(|k| {
            let short = k.trim_start_matches("minecraft:");
            c.enchants.iter().any(|e| e.eq_ignore_ascii_case(short) || e.eq_ignore_ascii_case(k))
        });
        if hit {
            return Some(("blockedItemWithEnchantments", Vec::new()));
        }
    }
    None
}

fn in_this_vault(p: &Player, gui: Option<&Inventory>) -> bool {
    let viewer = p.get_unique_id();
    state(|s| s.in_vault.get(&viewer).and_then(|k| s.open.get(k)).is_some_and(|o| gui.is_some_and(|g| g.same_as(&o.inv))))
}

fn on_click(e: &mut InventoryClickEvent) {
    let Some(p) = e.get_who_clicked() else { return };
    let gui = e.get_inventory().gui().cloned();
    if !in_this_vault(&p, gui.as_ref()) || p.has_permission("playervaults.bypassblockeditems") || !settings().0.blocking {
        return;
    }
    let mut items: Vec<ItemStack> = e.get_current_item().cloned().into_iter().collect();
    if e.get_hotbar_button() > -1
        && let Some(s) = p.get_inventory().get_item(e.get_hotbar_button())
    {
        items.push(s);
    }
    if e.get_click() == "SWAP_OFFHAND"
        && let Some(s) = p.get_inventory().get_item_in_off_hand()
    {
        items.push(s);
    }
    for item in items.iter().filter(|i| !i.is_empty()) {
        if let Some((key, pairs)) = blocked(item) {
            e.set_cancelled(true);
            tell_p(&p, key, &pairs);
            return;
        }
    }
}

fn on_drag(e: &mut InventoryDragEvent) {
    let Some(p) = e.get_who_clicked() else { return };
    let gui = e.get_inventory().gui().cloned();
    if !in_this_vault(&p, gui.as_ref()) || p.has_permission("playervaults.bypassblockeditems") || !settings().0.blocking {
        return;
    }
    for item in e.get_new_items().values() {
        if let Some((key, pairs)) = blocked(item) {
            e.set_cancelled(true);
            tell_p(&p, key, &pairs);
            return;
        }
    }
}

fn on_close(e: &mut InventoryCloseEvent) {
    let Some(p) = e.get_player() else { return };
    let inv = e.get_inventory();
    let gui = inv.gui().cloned();
    let viewer = p.get_unique_id();
    let ours = state(|s| s.in_vault.get(&viewer).and_then(|k| s.open.get(k)).is_some_and(|o| gui.as_ref().is_none_or(|g| g.same_as(&o.inv))));
    if ours {
        let contents = gui.is_some().then(|| inv.get_contents());
        leave(&p, contents);
    }
}

// ---- Signs ----

fn sign_key(b: &horizon_bukkit::Block) -> String {
    let l = b.get_location();
    format!("{};;{};;{};;{}", l.get_world().get_name(), l.get_block_x(), l.get_block_y(), l.get_block_z())
}

fn save_signs() {
    let Some(p) = plugin() else { return };
    let text = state(|s| {
        let mut out = String::new();
        for (k, (me, owner, chest)) in &s.signs {
            out.push_str(&format!("{}:\n", files_quote(k)));
            if *me {
                out.push_str("  self: true\n");
            }
            if let Some(o) = owner {
                out.push_str(&format!("  owner: {}\n", files_quote(o)));
            }
            out.push_str(&format!("  chest: {chest}\n"));
        }
        out
    });
    files::write(p.get_data_folder().join("signs.yml"), text);
}

fn files_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn read_signs(path: &std::path::Path) -> BTreeMap<String, (bool, Option<String>, i64)> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else { return out };
    let Ok(serde_yaml::Value::Mapping(m)) = horizon_kit::yaml::parse(&text) else { return out };
    for (k, v) in m {
        let Some(k) = k.as_str() else { continue };
        let me = v.get("self").and_then(serde_yaml::Value::as_bool).unwrap_or(false);
        let owner = v.get("owner").and_then(serde_yaml::Value::as_str).map(str::to_owned);
        let chest = v.get("chest").and_then(serde_yaml::Value::as_i64).unwrap_or(1);
        out.insert(k.to_owned(), (me, owner, chest));
    }
    out
}

fn is_sign(m: &horizon_bukkit::Material) -> bool {
    m.key().to_ascii_uppercase().contains("SIGN")
}

fn on_interact(e: &mut PlayerInteractEvent) {
    if !settings().0.signs {
        return;
    }
    let p = e.get_player().clone();
    if p.is_sleeping() || p.is_dead() {
        return;
    }
    let action = e.get_action().to_owned();
    let block = e.get_clicked_block().cloned();
    let viewer = p.get_unique_id();
    if action == "RIGHT_CLICK_BLOCK"
        && state(|s| s.in_vault.contains_key(&viewer))
        && let Some(b) = &block
    {
        let t = b.get_type().name();
        if t.contains("ENCHANT") || t == "ENDER_CHEST" || t.contains("CHEST") || t.contains("BARREL") || t.contains("SHULKER_BOX") || t.contains("FURNACE") || t.contains("HOPPER") {
            e.set_cancelled(true);
        }
    }
    if let Some((owner, chest)) = state(|s| s.set_sign.remove(p.get_name())) {
        e.set_cancelled(true);
        match &block {
            Some(b) if (action == "RIGHT_CLICK_BLOCK" || action == "LEFT_CLICK_BLOCK") && is_sign(&b.get_type()) => {
                let key = sign_key(b);
                state(|s| s.signs.insert(key, (owner.is_none(), owner, i64::from(chest))));
                save_signs();
                tell_p(&p, "setSign", &[]);
            }
            _ => tell_p(&p, "notASign", &[]),
        }
        return;
    }
    if action != "RIGHT_CLICK_BLOCK" {
        return;
    }
    let Some(b) = block.filter(|b| is_sign(&b.get_type())) else { return };
    let key = sign_key(&b);
    let Some((me, owner, chest)) = state(|s| s.signs.get(&key).cloned()) else { return };
    if state(|s| s.in_vault.contains_key(&viewer)) {
        return;
    }
    if !(p.has_permission("playervaults.signs.use") || p.has_permission("playervaults.signs.bypass")) {
        return tell_p(&p, "noPerms", &[]);
    }
    let n = chest.max(1).to_string();
    e.set_cancelled(true);
    if me {
        open_own(&p, &n, true, false);
        tell_p(&p, "openWithSign", &[("vault", n), ("player", p.get_name().to_owned())]);
    } else {
        let name = owner.unwrap_or_default();
        let t = target(&name);
        if t.0 == name {
            return tell_p(&p, "vaultDoesNotExist", &[]);
        }
        if open_other(&p, t, &n, false) {
            tell_p(&p, "openWithSign", &[("vault", n), ("player", name)]);
        }
    }
}

// ---- Commands ----

fn pv(s: &CommandSender, label: &str, args: &[&str]) -> bool {
    let Some(p) = s.as_player() else {
        tell(s, "playerOnly", &[]);
        return true;
    };
    if state(|st| st.in_vault.contains_key(&p.get_unique_id())) {
        return true;
    }
    match args {
        [n] => {
            if !p.has_permission("playervaults.commands.use") {
                return {
                    tell(s, "noPerms", &[]);
                    true
                };
            }
            open_own(p, n, false, true);
        }
        [name, what] => {
            if !p.has_permission("playervaults.admin") {
                tell(s, "noPerms", &[]);
                return true;
            }
            let t = target(name);
            if *what == "list" {
                let s2 = s.clone();
                let name = (*name).to_owned();
                let owner = t.0.clone();
                let list = move || {
                    let nums: Option<Vec<u32>> = state(|st| st.cache.get(&owner).map(|f| f.keys().copied().collect()));
                    match nums {
                        Some(n) if !n.is_empty() => tell(&s2, "existingVaults", &[("player", name.clone()), ("vault", n.iter().map(u32::to_string).collect::<Vec<_>>().join(" "))]),
                        _ => tell(&s2, "vaultDoesNotExist", &[]),
                    }
                };
                if with_file(&t.0, list.clone()) {
                    list();
                }
                return true;
            }
            if what.parse::<i64>().is_err() {
                tell(s, "mustBeNumber", &[]);
                return true;
            }
            if !open_other(p, t, what, true) {
                tell(s, "noOwnerFound", &[("player", (*name).to_owned())]);
            }
        }
        _ => {
            let _ = label;
            tell(s, "help", &[]);
        }
    }
    true
}

fn delete_vault(s: &CommandSender, owner: &str, owner_name: &str, number: u32, own: bool) {
    let key = format!("{owner}:{number}");
    let viewers: Vec<String> = state(|st| {
        let v = st.open.remove(&key).map(|o| o.viewers.into_iter().collect()).unwrap_or_default();
        for x in &v {
            st.in_vault.remove(x);
        }
        v
    });
    for p in horizon_bukkit::get_online_players() {
        if viewers.contains(&p.get_unique_id()) {
            p.close_inventory();
        }
    }
    let file = state(|st| {
        let f = st.cache.get_mut(owner)?;
        f.remove(&number);
        Some(f.clone())
    });
    if let Some(f) = file {
        files::save(owner, f);
    }
    if own {
        tell(s, "deleteVault", &[("vault", number.to_string())]);
    } else {
        tell(s, "deleteOtherVault", &[("vault", number.to_string()), ("player", owner_name.to_owned())]);
    }
}

fn pvdel(s: &CommandSender, label: &str, args: &[&str]) -> bool {
    match args {
        [n] => {
            let Some(p) = s.as_player().cloned() else {
                tell(s, "playerOnly", &[]);
                return true;
            };
            let Some(number) = parse_number(s, n) else { return true };
            let owner = p.get_unique_id();
            let p_uuid = owner.clone();
            let s2 = s.clone();
            let run = move || {
                let set = settings();
                if !vault_exists(&owner, number) {
                    if set.0.eco && set.0.refund != 0.0 && !p.has_permission("playervaults.free") {
                        return tell(&s2, "vaultDoesNotExist", &[]);
                    }
                } else if set.0.eco && set.0.refund != 0.0 && !p.has_permission("playervaults.free") {
                    if !horizon_kit::hooks::economy::deposit(p.get_name(), set.0.refund) {
                        return;
                    }
                    tell(&s2, "refundAmount", &[("price", java_double(set.0.refund))]);
                }
                delete_vault(&s2, &owner, p.get_name(), number, true);
            };
            let owner = p_uuid;
            if with_file(&owner, run.clone()) {
                run();
            }
        }
        [name, what] => {
            if !s.has_permission("playervaults.delete") {
                tell(s, "noPerms", &[]);
                return true;
            }
            let t = target(name);
            if what.eq_ignore_ascii_case("all") {
                if !s.has_permission("playervaults.delete.all") {
                    tell(s, "noPerms", &[]);
                    return true;
                }
                state(|st| st.cache.remove(&t.0));
                files::delete(&t.0);
                tell(s, "deleteOtherVaultAll", &[("player", t.0.clone())]);
                if let Some(p) = plugin() {
                    p.info(&format!("{} deleted ALL vaults belonging to {}", s.get_name(), t.0));
                }
                return true;
            }
            let Some(number) = parse_number(s, what) else { return true };
            let s2 = s.clone();
            let t2 = t.clone();
            let run = move || delete_vault(&s2, &t2.0, &t2.1, number, false);
            if with_file(&t.0, run.clone()) {
                run();
            }
        }
        _ => {
            s.send_message(&format!("/{label} <number>"));
            s.send_message(&format!("/{label} <player> <number>"));
            s.send_message(&format!("/{label} <player> all"));
        }
    }
    true
}

fn pvsign(s: &CommandSender, label: &str, args: &[&str]) -> bool {
    if !s.has_permission("playervaults.signs.set") {
        tell(s, "noPerms", &[]);
        return true;
    }
    if !settings().0.signs {
        tell(s, "signsDisabled", &[]);
        return true;
    }
    let Some(p) = s.as_player() else {
        tell(s, "playerOnly", &[]);
        return true;
    };
    let (owner, n) = match args {
        [n] => (None, *n),
        [o, n, ..] => (Some(o.to_ascii_lowercase()), *n),
        [] => {
            tell(s, "invalidArgs", &[]);
            return true;
        }
    };
    let Ok(i) = n.parse::<u32>() else {
        tell(s, "mustBeNumber", &[]);
        s.send_message(&format!("              /{label} [owner] <#>"));
        return true;
    };
    state(|st| st.set_sign.insert(p.get_name().to_owned(), (owner, i)));
    tell(s, "clickASign", &[]);
    true
}

fn pvconsole(s: &CommandSender, label: &str, args: &[&str]) -> bool {
    if s.as_player().is_some() {
        return true;
    }
    if args.first() != Some(&"openforplayer") || args.len() < 4 {
        s.send_message(&format!("/{label} openforplayer <player> <owner> <vaultid>"));
        return true;
    }
    let Some(p) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_name() == args[1]) else {
        s.send_message("NOT ONLINE");
        return true;
    };
    let owner = target(args[2]);
    if state(|st| st.in_vault.contains_key(&p.get_unique_id())) {
        s.send_message(&format!("ALREADY HAS A VAULT OPEN: {}", p.get_name()));
        return true;
    }
    if args[3].parse::<u32>().is_err() {
        s.send_message("NOT NUMBER");
        return true;
    }
    if !open_other(&p, owner, args[3], true) {
        s.send_message("FAILED!?");
    }
    true
}

fn reload(then: impl FnOnce() + Send + 'static) {
    let Some(p) = plugin() else { return };
    let folder = p.get_data_folder();
    std::thread::spawn(move || {
        let loaded = conf::load(&folder);
        on_main(move || {
            *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(loaded));
            horizon_kit::hooks::refresh();
            then();
        });
    });
}

fn on_enable(p: Plugin) {
    let started = Instant::now();
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
    p.set_executor("pv", |s, l, a| pv(s, l, a));
    p.set_executor("pvdel", |s, l, a| pvdel(s, l, a));
    p.set_executor("pvsign", |s, l, a| pvsign(s, l, a));
    p.set_executor("pvconsole", |s, l, a| pvconsole(s, l, a));
    p.set_executor("pvconvert", |s, _, _| {
        tell(s, "convertPluginNotFound", &[]);
        true
    });
    p.set_executor("pvreload", |s, _, _| {
        let s = s.clone();
        reload(move || s.send_message("§aReloaded PlayerVault's configuration and lang files."));
        true
    });
    p.set_executor("pvhelpme", |s, _, _| {
        let set = settings();
        let open = state(|st| st.open.len());
        s.send_message(&format!("§ePlayerVaults (Rust) {} | vaults open: {open} | signs: {} | economy: {} ({}) | blocking: {} | rows: {}", plugin().map(|p| p.get_version().to_owned()).unwrap_or_default(), set.0.signs, set.0.eco, if horizon_kit::hooks::economy::enabled() { "found" } else { "none" }, set.0.blocking, set.0.rows));
        true
    });
    p.set_tab_completer("pv", |s, _, a| {
        if a.len() == 1 && s.has_permission("playervaults.admin") {
            let t = a[0].to_ascii_lowercase();
            return horizon_bukkit::get_online_players().into_iter().map(|p| p.get_name().to_owned()).filter(|n| n.to_ascii_lowercase().starts_with(&t)).collect();
        }
        Vec::new()
    });
    p.register_event_with("NORMAL", true, on_click);
    p.register_event_with("NORMAL", true, on_drag);
    p.register_event_with("HIGH", false, on_close);
    p.register_event_with("HIGH", true, on_interact);
    p.register_event_with("HIGH", true, |e: &mut BlockBreakEvent| {
        if !settings().0.signs {
            return;
        }
        let key = sign_key(e.get_block());
        if state(|s| s.signs.remove(&key)).is_some() {
            save_signs();
        }
    });
    p.register_event_with("HIGH", false, |e: &mut PlayerQuitEvent| {
        let player = e.get_player().clone();
        leave(&player, None);
        let uuid = player.get_unique_id();
        state(|s| {
            let busy = s.open.values().any(|o| o.owner == uuid);
            if !busy {
                s.cache.remove(&uuid);
            }
            s.set_sign.remove(player.get_name());
        });
    });
    p.register_event_with("HIGH", false, |e: &mut PlayerDeathEvent| {
        let player = e.get_player().clone();
        leave(&player, None);
    });
    p.register_event(|e: &mut PlayerJoinEvent| {
        // VaultPreloadListener: the file read ahead, so `/pv` opens at once.
        with_file(&e.get_player().get_unique_id(), || {});
    });
    horizon_kit::hooks::provide_placeholders(&p, "playervaults", |uuid, _, params| {
        if params != "vaults_max_by_perms" || uuid.is_empty() {
            return None;
        }
        let player = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_unique_id() == uuid)?;
        Some(count_vaults(&player).to_string())
    });
    p.run_task_timer(1, 1, drain_main);
    let folder = p.get_data_folder();
    std::thread::spawn(move || {
        let loaded = conf::load(&folder);
        let dir = folder.join("newvaults");
        let _ = std::fs::create_dir_all(&dir);
        let purged = loaded.0.purge.map_or(0, |days| files::purge(&dir, days));
        let signs = read_signs(&folder.join("signs.yml"));
        let backups = loaded.0.backups;
        on_main(move || {
            *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(loaded));
            files::start(dir, backups);
            state(|s| s.signs = signs);
            // Players already on (a hot reload): their files read ahead.
            for pl in horizon_bukkit::get_online_players() {
                with_file(&pl.get_unique_id(), || {});
            }
            if let Some(p) = plugin() {
                if purged > 0 {
                    p.info(&format!("Cleanup: deleted {purged} vault files not changed recently."));
                }
                p.info(&format!("Enabled in {} ms (Rust).", started.elapsed().as_millis()));
            }
        });
    });
}

fn on_disable(_p: Plugin) {
    // Every open vault saved before the plugin goes.
    for player in horizon_bukkit::get_online_players() {
        if state(|s| s.in_vault.contains_key(&player.get_unique_id())) {
            leave(&player, None);
            player.close_inventory();
        }
    }
    files::stop();
    MAIN.lock().unwrap_or_else(|e| e.into_inner()).clear();
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

bukkit_plugin!(on_enable, on_disable);
