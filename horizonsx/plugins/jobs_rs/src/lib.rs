//! Jobs Reborn (Zrips, Apache-2.0) written again natively in Rust for Horizon: the plugin name `Jobs`, `/jobs` with
//! its sub-commands and permissions, generalConfig.yml, jobs/*.yml (or jobConfig.yml), locale/messages_*.yml,
//! restrictedBlocks.yml, shopItems.yml, schedule.yml and the SQLite database (jobs.sqlite.db), all as Jobs keeps them.
//!
//! Hot events only look up the player and the job tables in memory; money is paid in batches (economy-batch-delay);
//! player rows are read at pre-login and written by a background thread; `%jobsr_...%` placeholders are offered.

pub mod commands;
pub mod db;
pub mod eq;
pub mod gui;
pub mod model;
pub mod schedule;
pub mod settings;

use horizon_bukkit::event::{PlayerDeathEvent, PlayerJoinEvent, PlayerQuitEvent};
use horizon_bukkit::fam_ev_entity::{Actor, CreatureSpawnEvent, EntityBreedEvent, EntityDamageByEntityEvent, EntityDeathEvent, EntityTameEvent};
use horizon_bukkit::fam_ev_player::{AsyncPlayerPreLoginEvent, BrewEvent, CraftItemEvent, EnchantItemEvent, FurnaceExtractEvent, InventoryClickEvent, InventoryCloseEvent, PlayerBucketEntityEvent, PlayerFishEvent, PlayerGameModeChangeEvent, PlayerHarvestBlockEvent, PlayerInteractEntityEvent, PlayerInteractEvent, PlayerItemConsumeEvent, PlayerShearEntityEvent};
use horizon_bukkit::fam_ev_world::{BlockBreakEvent, BlockPlaceEvent};
use horizon_bukkit::{Plugin, bukkit_plugin, enums};
use settings::Action;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

static PLUGIN: Mutex<Option<Plugin>> = Mutex::new(None);

pub fn plugin() -> Option<Plugin> {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn folder() -> std::path::PathBuf {
    plugin().map(|p| p.get_data_folder()).unwrap_or_default()
}

pub fn log(text: &str) {
    if let Some(p) = plugin() {
        p.info(text);
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Runs on the server thread next tick.
pub fn later(task: impl FnOnce() + Send + 'static) {
    let Some(p) = plugin() else { return };
    let mut once = Some(task);
    p.run_task(move || {
        if let Some(t) = once.take() {
            t();
        }
    });
}

pub fn reload() {
    settings::load(&folder());
}

/// `minecraft:wheat[age=7]` -> `wheat`, and its age.
fn mat_age(data: &str) -> (String, Option<String>) {
    let s = data.trim_start_matches("minecraft:");
    let (m, props) = s.split_once('[').unwrap_or((s, ""));
    let age = props.trim_end_matches(']').split(',').find_map(|kv| kv.strip_prefix("age=")).map(str::to_owned);
    (m.to_owned(), age)
}

fn id(item: &horizon_bukkit::ItemStack) -> String {
    item.get_type().key().trim_start_matches("minecraft:").to_owned()
}

fn uuid(p: &horizon_bukkit::Player) -> String {
    p.get_unique_id().to_ascii_lowercase()
}

/// Spawner mobs (pay nothing when `enable-pay-near-spawner` is off).
static SPAWNER_MOBS: LazyLock<Mutex<HashSet<u64>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
/// Brewing stand -> who last used it (BrewEvent names no player).
static STANDS: LazyLock<Mutex<HashMap<(String, i32, i32, i32), String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
/// Player victim -> (attacking player, when): PlayerDeathEvent names no killer.
static LAST_HIT: LazyLock<Mutex<HashMap<String, (String, i64)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static STOP: AtomicBool = AtomicBool::new(false);
static SAVER: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

fn start_saver() {
    STOP.store(false, Ordering::SeqCst);
    let h = std::thread::Builder::new()
        .name("Jobs-data".into())
        .spawn(|| {
            model::load_protection();
            model::load_explore();
            let mut last = std::time::Instant::now();
            loop {
                for _ in 0..10 {
                    if STOP.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                if last.elapsed().as_secs() >= settings::get().save_minutes * 60 {
                    save_all(false);
                    last = std::time::Instant::now();
                }
            }
        })
        .ok();
    *SAVER.lock().unwrap_or_else(|e| e.into_inner()) = h;
}

fn save_all(everyone: bool) {
    let rows = if everyone { model::all_rows() } else { model::dirty_rows() };
    db::save(&rows);
    model::save_protection();
    model::save_explore();
}

/// Each player's chunk once a second: Explore pays the first visitors of a chunk.
fn explore_tick() {
    let s = settings::get();
    if !s.uses(Action::Explore) {
        return;
    }
    let players: Vec<horizon_bukkit::Player> = model::players().values().map(|j| j.player.clone()).collect();
    for p in players {
        let at = p.get_location();
        let (cx, cz) = ((at.get_x().floor() as i32) >> 4, (at.get_z().floor() as i32) >> 4);
        let world = at.get_world().get_name().to_owned();
        let u = uuid(&p);
        let changed = {
            let mut ps = model::players();
            let Some(jp) = ps.get_mut(&u) else { continue };
            let here = Some((world.clone(), cx, cz));
            if jp.last_chunk == here {
                false
            } else {
                jp.last_chunk = here;
                true
            }
        };
        if !changed {
            continue;
        }
        if !s.explore_flying && p.is_flying() {
            continue;
        }
        if let Some(n) = model::explore(&world, cx, cz, p.get_name()) {
            model::action(&u, Action::Explore, &n.to_string(), &world, 1.0);
        }
    }
}

fn placeholders(uuid: &str, _name: &str, params: &str) -> Option<String> {
    let s = settings::get();
    let p = params.to_ascii_lowercase();
    let ps = model::players();
    let jp = ps.get(&uuid.to_ascii_lowercase());
    let job_of = |arg: &str| -> Option<&settings::Job> {
        match arg.parse::<usize>() {
            Ok(n) => jp.and_then(|j| j.jobs.get(n.checked_sub(1)?)).and_then(|pr| s.job(&pr.job)),
            Err(_) => s.job(arg),
        }
    };
    let prog_of = |arg: &str| -> Option<model::Prog> {
        let job = job_of(arg)?;
        jp?.jobs.iter().find(|x| x.job == job.name).cloned()
    };
    Some(match p.as_str() {
        "user_points" => s.points_fmt(jp?.points),
        "user_points_fixed" => format!("{:.0}", jp?.points),
        "user_total_points" => s.points_fmt(jp?.total_points),
        "user_totallevels" => jp?.total_level().to_string(),
        "user_joinedjobcount" => jp?.jobs.len().to_string(),
        "user_jobs" => jp?.jobs.iter().map(|x| s.job(&x.job).map(|j| format!("{}{}", settings::chat_color(&j.color), j.name)).unwrap_or_default()).collect::<Vec<_>>().join("§f, "),
        "user_jobs_clean" => jp?.jobs.iter().map(|x| x.job.clone()).collect::<Vec<_>>().join(", "),
        "user_id" => jp?.user_id.to_string(),
        "maxjobs" => jp.map(|j| j.max_jobs).unwrap_or(s.max_jobs).to_string(),
        "total_workers" => ps.values().filter(|j| !j.jobs.is_empty()).count().to_string(),
        _ => {
            let (prefix, arg) = [
                "user_jlevel_", "user_jexp_rounded_", "user_jexp_int_", "user_jexpunf_", "user_jexp_", "user_jmaxexpunf_", "user_jmaxexp_", "user_jmaxlvl_", "user_jprogress_", "user_isin_", "user_canjoin_", "user_jobfull_", "user_jobshort_", "user_job_", "name_", "shortname_", "fullname_", "chatcolor_", "description_", "maxlvl_",
            ]
            .iter()
            .find_map(|pre| p.strip_prefix(pre).map(|a| (*pre, a.to_owned())))?;
            match prefix {
                "user_jlevel_" => prog_of(&arg).map(|x| x.level).unwrap_or(0).to_string(),
                "user_jexp_rounded_" | "user_jexp_int_" => format!("{:.0}", prog_of(&arg).map(|x| x.exp).unwrap_or(0.0)),
                "user_jexpunf_" => prog_of(&arg).map(|x| x.exp).unwrap_or(0.0).to_string(),
                "user_jexp_" => s.exp_fmt(prog_of(&arg).map(|x| x.exp).unwrap_or(0.0)),
                "user_jmaxexp_" | "user_jmaxexpunf_" => {
                    let job = job_of(&arg)?;
                    let lv = prog_of(&arg).map(|x| x.level).unwrap_or(1);
                    format!("{:.0}", job.max_exp(lv, jp.map(|j| j.jobs.len()).unwrap_or(1)))
                }
                "user_jmaxlvl_" => jp.map(|j| j.max_level(&s, &job_of(&arg).map(|x| x.name.clone()).unwrap_or_default())).unwrap_or(0).to_string(),
                "user_jprogress_" => {
                    let job = job_of(&arg)?;
                    let pr = prog_of(&arg)?;
                    format!("{:.0}", pr.exp / job.max_exp(pr.level, jp.map(|j| j.jobs.len()).unwrap_or(1)) * 100.0)
                }
                "user_isin_" => prog_of(&arg).is_some().to_string(),
                "user_canjoin_" => jp.is_some_and(|j| job_of(&arg).is_some_and(|x| j.join_perm.contains(&x.name.to_ascii_lowercase()) && !j.in_job(&x.name))).to_string(),
                "user_job_" | "user_jobfull_" => job_of(&arg).filter(|_| prog_of(&arg).is_some()).map(|j| j.name.clone()).unwrap_or_default(),
                "user_jobshort_" => job_of(&arg).filter(|_| prog_of(&arg).is_some()).map(|j| j.short.clone()).unwrap_or_default(),
                "name_" | "fullname_" => job_of(&arg)?.name.clone(),
                "shortname_" => job_of(&arg)?.short.clone(),
                "chatcolor_" => settings::chat_color(&job_of(&arg)?.color).to_owned(),
                "description_" => job_of(&arg)?.description.clone(),
                _ => job_of(&arg)?.max_level.to_string(),
            }
        }
    })
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    let started = std::time::Instant::now();
    let f = plugin.get_data_folder();
    let _ = std::fs::write(f.join("LICENSE"), include_str!("../LICENSE"));
    reload();
    if !db::open(&f) {
        log("Could not open jobs.sqlite.db; Jobs is off.");
        return;
    }
    start_saver();
    plugin.set_executor("jobs", commands::run);
    plugin.set_tab_completer("jobs", commands::complete);
    horizon_kit::hooks::provide_placeholders(&plugin, "jobsr", placeholders);

    // ---- Players ----
    plugin.register_event(|e: &mut AsyncPlayerPreLoginEvent| {
        let (u, n) = (e.get_unique_id().to_ascii_lowercase(), e.get_name().to_owned());
        if let Some(l) = db::load_player(&u, &n) {
            model::PENDING.lock().unwrap_or_else(|e| e.into_inner()).insert(u, l);
        }
    });
    plugin.register_event(|e: &mut PlayerJoinEvent| model::join(e.get_player()));
    plugin.register_event(|e: &mut PlayerQuitEvent| {
        let u = uuid(e.get_player());
        model::quit(&u);
        gui::closed(&u);
    });
    plugin.register_event(|e: &mut PlayerGameModeChangeEvent| {
        if let Some(p) = e.get_player() {
            let creative = e.get_new_game_mode().eq_ignore_ascii_case("CREATIVE");
            if let Some(jp) = model::players().get_mut(&uuid(&p)) {
                jp.creative = creative;
            }
        }
    });

    // ---- Blocks ----
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut BlockBreakEvent| {
        let s = settings::get();
        if !s.uses(Action::Break) {
            return;
        }
        let b = e.get_block();
        let world = b.get_world().get_name().to_owned();
        if !model::on_break(&world, b.get_x(), b.get_y(), b.get_z()) {
            return;
        }
        let (m, age) = mat_age(&b.get_block_data());
        let u = uuid(e.get_player());
        if let Some(a) = age {
            let aged = format!("{m}-{a}");
            let has_aged = s.jobs.iter().chain(s.none.iter()).any(|j| j.actions.get(&Action::Break).is_some_and(|t| t.contains_key(&aged)));
            if has_aged {
                model::action(&u, Action::Break, &aged, &world, 1.0);
                return;
            }
            // A crop without an aged entry pays only when grown.
            if matches!(m.as_str(), "wheat" | "carrots" | "potatoes" | "beetroots" | "nether_wart" | "cocoa" | "sweet_berry_bush") {
                let max = match m.as_str() {
                    "beetroots" | "nether_wart" | "sweet_berry_bush" => "3",
                    "cocoa" => "2",
                    _ => "7",
                };
                if a != max {
                    return;
                }
            }
        }
        model::action(&u, Action::Break, &m, &world, 1.0);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut BlockPlaceEvent| {
        let b = e.get_block_placed().clone();
        let item = e.get_item_in_hand().map(id).unwrap_or_default();
        let world = b.get_world().get_name().to_owned();
        let pays = model::on_place(&world, b.get_x(), b.get_y(), b.get_z(), &item);
        if pays && settings::get().uses(Action::Place) {
            model::action(&uuid(e.get_player()), Action::Place, &item, &world, 1.0);
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut PlayerInteractEvent| {
        if e.action != "RIGHT_CLICK_BLOCK" {
            return;
        }
        let Some(b) = e.block.clone() else { return };
        let s = settings::get();
        let held = e.get_item().map(|i| id(&i)).unwrap_or_default();
        let strip = s.uses(Action::StripLogs) && held.ends_with("_axe");
        let stand = s.uses(Action::Brew) && true;
        if !strip && !stand {
            return;
        }
        let (m, _) = mat_age(&b.get_block_data());
        let world = b.get_world().get_name().to_owned();
        if m == "brewing_stand" {
            let mut st = STANDS.lock().unwrap_or_else(|e| e.into_inner());
            if st.len() > 20_000 {
                st.clear();
            }
            st.insert((world, b.get_x(), b.get_y(), b.get_z()), uuid(&e.player));
            return;
        }
        if strip && !m.starts_with("stripped_") && (m.ends_with("_log") || m.ends_with("_wood") || m.ends_with("_stem") || m.ends_with("_hyphae") || m == "bamboo_block") {
            model::action(&uuid(&e.player), Action::StripLogs, &m, &world, 1.0);
        }
    });

    // ---- Mobs ----
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut CreatureSpawnEvent| {
        if e.get_spawn_reason() == "SPAWNER" && !settings::get().pay_near_spawner
            && let Some(en) = e.get_entity()
        {
            let mut m = SPAWNER_MOBS.lock().unwrap_or_else(|e| e.into_inner());
            if m.len() > 100_000 {
                m.clear();
            }
            m.insert(en.get_entity_id());
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut EntityDeathEvent| {
        let s = settings::get();
        if !s.uses(Action::Kill) {
            return;
        }
        let Some(Actor::Player(killer)) = e.get_killer() else { return };
        let en = e.get_entity();
        if SPAWNER_MOBS.lock().unwrap_or_else(|e| e.into_inner()).remove(&en.get_entity_id()) {
            return;
        }
        let kind = e.get_entity_type().trim_start_matches("minecraft:").to_owned();
        if kind.is_empty() || kind == "player" {
            return;
        }
        let world = killer.get_world().get_name().to_owned();
        model::action(&uuid(&killer), Action::Kill, &kind, &world, 1.0);
    });
    plugin.register_event(|e: &mut PlayerDeathEvent| {
        let s = settings::get();
        if !s.uses(Action::CustomKill) && !s.uses(Action::Kill) {
            return;
        }
        let victim = e.get_player().clone();
        let hit = LAST_HIT.lock().unwrap_or_else(|e| e.into_inner()).remove(&uuid(&victim));
        let Some(killer) = hit.filter(|(_, at)| now_ms() - at < 10_000).and_then(|(k, _)| model::players().get(&k).map(|j| j.player.clone())) else { return };
        let world = killer.get_world().get_name().to_owned();
        model::action(&uuid(&killer), Action::CustomKill, &victim.get_name().to_ascii_lowercase(), &world, 1.0);
        model::action(&uuid(&killer), Action::Kill, "player", &world, 1.0);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut EntityDamageByEntityEvent| {
        let Some(Actor::Player(victim)) = e.get_entity() else { return };
        let attacker = match e.get_damager() {
            Some(Actor::Player(p)) => Some(uuid(&p)),
            Some(Actor::Entity(_)) => e.get_damage_source().get_causing_entity().map(|c| c.get_unique_id().to_ascii_lowercase()),
            None => None,
        };
        if let Some(a) = attacker.filter(|a| *a != uuid(&victim)) {
            LAST_HIT.lock().unwrap_or_else(|e| e.into_inner()).insert(uuid(&victim), (a, now_ms()));
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut EntityBreedEvent| {
        let Some(Actor::Player(p)) = e.get_breeder() else { return };
        let Some(en) = e.get_entity() else { return };
        let kind = en.get_type().trim_start_matches("minecraft:").to_owned();
        model::action(&uuid(&p), Action::Breed, &kind, p.get_world().get_name(), 1.0);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut EntityTameEvent| {
        let (Some(p), Some(en)) = (e.get_owner(), e.get_entity()) else { return };
        let kind = en.get_type().trim_start_matches("minecraft:").to_owned();
        model::action(&uuid(&p), Action::Tame, &kind, p.get_world().get_name(), 1.0);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerShearEntityEvent| {
        if !settings::get().uses(Action::Shear) {
            return;
        }
        let (Some(p), Some(en)) = (e.get_player(), e.get_entity()) else { return };
        let kind = en.get_type().trim_start_matches("minecraft:").to_owned();
        let key = if kind == "sheep" {
            const COLORS: [&str; 16] = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];
            let c = en.data_at("Color").as_i64().unwrap_or(0) as usize;
            COLORS.get(c).copied().unwrap_or("white").to_owned()
        } else {
            kind
        };
        model::action(&uuid(&p), Action::Shear, &key, p.get_world().get_name(), 1.0);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerInteractEntityEvent| {
        let s = settings::get();
        if !s.uses(Action::Milk) || e.get_hand() != "HAND" {
            return;
        }
        let kind = e.kind.trim_start_matches("minecraft:").to_owned();
        if !matches!(kind.as_str(), "cow" | "goat" | "mooshroom") {
            return;
        }
        let p = e.player.clone();
        let held = p.get_inventory().get_item_in_main_hand().map(|i| id(&i)).unwrap_or_default();
        if held == "bucket" || (kind == "mooshroom" && held == "bowl") {
            model::action(&uuid(&p), Action::Milk, &kind, p.get_world().get_name(), 1.0);
        }
    });

    // ---- Items ----
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerFishEvent| {
        if e.get_state() != "CAUGHT_FISH" || !settings::get().uses(Action::Fish) {
            return;
        }
        let (Some(p), Some(caught)) = (e.get_player(), e.get_caught()) else { return };
        let Some(stack) = caught.cast::<horizon_bukkit::entities::Item>().get_item_stack() else { return };
        model::action(&uuid(&p), Action::Fish, &id(&stack), p.get_world().get_name(), stack.get_amount() as f64);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut CraftItemEvent| {
        if !settings::get().uses(Action::Craft) {
            return;
        }
        let Some(p) = e.get_who_clicked() else { return };
        let Some(result) = e.get_current_item().cloned() else { return };
        let inv = e.get_inventory();
        let mut times = 1;
        if e.is_shift_click() {
            times = (1..inv.get_size()).filter_map(|i| inv.get_item(i)).map(|st| st.get_amount()).filter(|a| *a > 0).min().unwrap_or(1);
        }
        model::action(&uuid(&p), Action::Craft, &id(&result), p.get_world().get_name(), (result.get_amount() * times) as f64);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut FurnaceExtractEvent| {
        let (Some(p), Some(t)) = (e.get_player(), e.get_item_type()) else { return };
        let item = t.key().trim_start_matches("minecraft:").to_owned();
        model::action(&uuid(&p), Action::Smelt, &item, p.get_world().get_name(), e.get_item_amount() as f64);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut BrewEvent| {
        let Some(b) = e.get_block() else { return };
        let world = b.get_world().get_name().to_owned();
        let owner = STANDS.lock().unwrap_or_else(|e| e.into_inner()).get(&(world.clone(), b.get_x(), b.get_y(), b.get_z())).cloned();
        let Some(u) = owner else { return };
        let contents = e.get_contents();
        let Some(ing) = contents.get_item(3) else { return };
        let potions = (0..3).filter(|i| contents.get_item(*i).is_some()).count().max(1);
        model::action(&u, Action::Brew, &id(&ing), &world, potions as f64);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut EnchantItemEvent| {
        let Some(p) = e.get_enchanter() else { return };
        let world = p.get_world().get_name().to_owned();
        let u = uuid(&p);
        if let Some(item) = e.get_item() {
            model::action(&u, Action::Enchant, &id(&item), &world, 1.0);
        }
        for (ench, level) in e.get_enchants_to_add() {
            let name = ench.trim_start_matches("minecraft:").to_ascii_lowercase();
            model::action(&u, Action::Enchant, &format!("{name}-{level}"), &world, 1.0);
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerItemConsumeEvent| {
        if !settings::get().uses(Action::Eat) {
            return;
        }
        let (Some(p), Some(item)) = (e.get_player(), e.get_item()) else { return };
        model::action(&uuid(&p), Action::Eat, &id(&item), p.get_world().get_name(), 1.0);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerHarvestBlockEvent| {
        if !settings::get().uses(Action::Collect) {
            return;
        }
        let Some(p) = e.get_player() else { return };
        let world = p.get_world().get_name().to_owned();
        let items: Vec<(String, i32)> = e.get_items_harvested().iter().map(|i| (id(i), i.get_amount())).collect();
        for (item, n) in items {
            model::action(&uuid(&p), Action::Collect, &item, &world, n as f64);
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerBucketEntityEvent| {
        if !settings::get().uses(Action::Bucket) {
            return;
        }
        let (Some(p), Some(bucket)) = (e.get_player(), e.get_entity_bucket()) else { return };
        model::action(&uuid(&p), Action::Bucket, &id(&bucket), p.get_world().get_name(), 1.0);
    });
    plugin.register_event_with(enums::event_priority::HIGH, false, |e: &mut InventoryClickEvent| {
        gui::on_click(e);
        if e.is_cancelled() {
            return;
        }
        // Repair: taking the result out of an anvil.
        if e.get_inventory().get_type() == "ANVIL" && e.get_raw_slot() == 2 && settings::get().uses(Action::Repair)
            && let (Some(p), Some(item)) = (e.get_who_clicked(), e.get_current_item().cloned())
        {
            model::action(&uuid(&p), Action::Repair, &id(&item), p.get_world().get_name(), 1.0);
        }
    });
    plugin.register_event(|e: &mut InventoryCloseEvent| {
        if let Some(p) = e.get_player() {
            gui::closed(&uuid(&p));
        }
    });

    // ---- Timers ----
    let batch = settings::get().batch_secs * 20;
    plugin.run_task_timer(batch, batch, model::flush_payments);
    plugin.run_task_timer(10, 10, model::flush_bars);
    plugin.run_task_timer(20, 20, explore_tick);
    plugin.run_task_timer(100, 100, schedule::tick);
    // Permissions read again a few players at a time (boosts, max jobs, join rights).
    plugin.run_task_timer(40, 40, || {
        static CURSOR: Mutex<usize> = Mutex::new(0);
        let s = settings::get();
        let list: Vec<horizon_bukkit::Player> = model::players().values().map(|j| j.player.clone()).collect();
        if list.is_empty() {
            return;
        }
        let mut c = CURSOR.lock().unwrap_or_else(|e| e.into_inner());
        *c = (*c + 1) % list.len();
        let p = &list[*c];
        let (max, boost, join, vip) = model::read_perms(p, &s);
        if let Some(jp) = model::players().get_mut(&uuid(p)) {
            jp.max_jobs = max;
            jp.boost = boost;
            jp.join_perm = join;
            jp.vip = vip;
        }
    });

    for p in horizon_bukkit::get_online_players() {
        model::join(&p);
    }
    log(&format!("Jobs 5.2.6.3 enabled in {} ms (Horizon native): {} jobs.", started.elapsed().as_millis(), settings::get().jobs.len()));
}

fn on_disable(_plugin: Plugin) {
    model::flush_payments();
    for jp in model::players().values() {
        if let Some((b, _)) = &jp.bossbar {
            b.remove_all();
        }
    }
    STOP.store(true, Ordering::SeqCst);
    if let Some(h) = SAVER.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = h.join();
    }
    save_all(true);
    db::close();
}

bukkit_plugin!(on_enable, on_disable);
