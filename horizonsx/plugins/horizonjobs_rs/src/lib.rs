//! HorizonJobs: five jobs (Miner, Lumberjack, Farmer, Fisher, Hunter) players join to earn money and exp for
//! the work they do, with levels that pay more, level-up bonuses, a jobs menu with the server's job cards, each
//! job's payouts and its top workers. Everything is in config.yml (jobs, colours, payouts, limits).
//!
//! - [`conf`]: config.yml.
//! - [`data`]: player files, the table of everyone, placed blocks.
//! - [`work`]: what pays, levels, the batched pay and the action bar.
//! - [`menus`]: the menus. [`cmds`]: `/jobs`.
//!
//! Money goes through the Vault economy service (EssentialsX provides it). Rank perks are LuckPerms meta:
//! `horizonjobs.max-jobs`, `horizonjobs.boost`. Placeholders: `%horizonjobs_<key>%` (see [`placeholders`]).

pub mod cmds;
pub mod conf;
pub mod data;
pub mod menus;
pub mod ui;
pub mod work;

use conf::Work;
use horizon_bukkit::event::{PlayerJoinEvent, PlayerQuitEvent};
use horizon_bukkit::fam_ev_entity::{Actor, CreatureSpawnEvent, EntityBreedEvent, EntityDeathEvent};
use horizon_bukkit::fam_ev_player::{AsyncPlayerPreLoginEvent, PlayerFishEvent, PlayerHarvestBlockEvent};
use horizon_bukkit::fam_ev_world::{BlockBreakEvent, BlockPlaceEvent};
use horizon_bukkit::{Plugin, bukkit_plugin, enums};
use horizon_kit::{app, gui, hooks};

fn id(item: &horizon_bukkit::ItemStack) -> String {
    item.get_type().key().trim_start_matches("minecraft:").to_owned()
}

/// `%horizonjobs_jobs%`, `_job` (first job), `_level_<job>`, `_exp_<job>`, `_maxexp_<job>`, `_progress_<job>`,
/// `_earned_<job>`, `_workers_<job>`, `_total_level`.
fn placeholders(uuid: &str, _name: &str, params: &str) -> Option<String> {
    if uuid.is_empty() {
        return None;
    }
    let s = conf::get();
    let d = data::players().peek(uuid, |d| d.clone()).or_else(|| {
        data::summaries().read(|m| m.get(uuid).cloned()).map(|r| data::Profile {
            name: r.name,
            jobs: r.jobs,
            progress: r.levels.into_iter().map(|(k, (l, e))| (k, data::Prog { level: l, exp: e, ..Default::default() })).collect(),
            left_at: 0,
        })
    })?;
    let p = params.to_ascii_lowercase();
    let job_of = |arg: &str| s.job(arg).cloned();
    Some(match p.as_str() {
        "jobs" => d.jobs.iter().filter_map(|j| s.job(j)).map(|j| j.name.clone()).collect::<Vec<_>>().join(", "),
        "job" => d.jobs.first().and_then(|j| s.job(j)).map(|j| j.name.clone()).unwrap_or_else(|| "None".into()),
        "job_count" => d.jobs.len().to_string(),
        "total_level" => s.jobs.iter().map(|j| d.prog(&j.id).level()).sum::<u32>().to_string(),
        _ => {
            let (key, arg) = p.split_once('_')?;
            let job = job_of(arg)?;
            let pr = d.prog(&job.id);
            match key {
                "level" => pr.level().to_string(),
                "exp" => ui::exp(pr.exp),
                "maxexp" => ui::exp(job.needed(pr.level())),
                "progress" => format!("{:.1}", (pr.exp / job.needed(pr.level()) * 100.0).clamp(0.0, 100.0)),
                "earned" => ui::money(pr.earned),
                "workers" => data::workers(&job.id, "").to_string(),
                "working" => d.works(&job.id).to_string(),
                _ => return None,
            }
        }
    })
}

fn on_enable(plugin: Plugin) {
    let started = std::time::Instant::now();
    app::set(plugin);
    gui::install(plugin);
    if let Err(e) = conf::reload() {
        app::warn(&format!("{e}: using the built-in jobs"));
    }
    let _ = data::summaries();
    data::save_placed(false);

    plugin.set_executor("jobs", |s, _, a| cmds::run(s, a));
    plugin.set_tab_completer("jobs", |s, _, a| cmds::complete(s, a));
    hooks::provide_placeholders(&plugin, "horizonjobs", placeholders);

    // ---- Players ----
    plugin.register_event_with("LOWEST", false, |e: &mut AsyncPlayerPreLoginEvent| {
        data::players().load(&e.get_unique_id());
    });
    plugin.register_event_with("MONITOR", false, |e: &mut PlayerJoinEvent| {
        let p = e.get_player();
        let (uuid, name) = (p.get_unique_id(), p.get_name().to_owned());
        let d = data::edit(&uuid, |d| {
            d.name = name;
            d.clone()
        });
        data::summarize(&uuid, &d);
    });
    plugin.register_event_with("MONITOR", false, |e: &mut PlayerQuitEvent| {
        let uuid = e.get_player().get_unique_id();
        let d = data::get(&uuid);
        data::summarize(&uuid, &d);
        app::later(200, move || {
            if horizon_bukkit::fam_ess::get_player_by_id(&uuid).is_none() {
                data::players().unload(&uuid);
            }
        });
    });

    // ---- Blocks ----
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut BlockBreakEvent| {
        let b = e.get_block();
        let (mat, props) = work::split_data(&b.get_block_data());
        let key = data::block_key(b.get_world().get_name(), b.get_x(), b.get_y(), b.get_z());
        if data::take_placed(&key) || !work::natural(&mat, &props) {
            return;
        }
        let p = e.get_player().clone();
        work::earn(&p, Work::Break, &mat, 1.0, 1.0);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut BlockPlaceEvent| {
        let b = e.get_block_placed().clone();
        let (mut mat, _) = work::split_data(&b.get_block_data());
        if mat.is_empty() || mat == "air" {
            mat = e.get_item_in_hand().map(id).unwrap_or_default();
        }
        if work::tracks_placed(&mat) {
            data::mark_placed(data::block_key(b.get_world().get_name(), b.get_x(), b.get_y(), b.get_z()));
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerHarvestBlockEvent| {
        let Some(p) = e.get_player() else { return };
        let items: Vec<(String, i32)> = e.get_items_harvested().iter().map(|i| (id(i), i.get_amount())).collect();
        for (item, n) in items {
            work::earn(&p, Work::Harvest, &item, f64::from(n.max(1)), 1.0);
        }
    });

    // ---- Mobs ----
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut CreatureSpawnEvent| {
        if e.get_spawn_reason() == "SPAWNER"
            && let Some(en) = e.get_entity()
        {
            work::spawner_mob(en.get_entity_id());
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut EntityDeathEvent| {
        let Some(Actor::Player(killer)) = e.get_killer() else { return };
        let kind = e.get_entity_type().trim_start_matches("minecraft:").to_owned();
        if kind.is_empty() || kind == "player" {
            return;
        }
        let mult = if work::from_spawner(e.get_entity().get_entity_id()) { conf::get().spawner_multiplier } else { 1.0 };
        if mult > 0.0 {
            work::earn(&killer, Work::Kill, &kind, 1.0, mult);
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut EntityBreedEvent| {
        let Some(Actor::Player(p)) = e.get_breeder() else { return };
        let Some(en) = e.get_entity() else { return };
        let kind = en.get_type().trim_start_matches("minecraft:").to_owned();
        work::earn(&p, Work::Breed, &kind, 1.0, 1.0);
    });

    // ---- Fishing ----
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerFishEvent| {
        if e.get_state() != "CAUGHT_FISH" {
            return;
        }
        let (Some(p), Some(caught)) = (e.get_player(), e.get_caught()) else { return };
        let Some(stack) = caught.cast::<horizon_bukkit::entities::Item>().get_item_stack() else { return };
        work::earn(&p, Work::Fish, &id(&stack), f64::from(stack.get_amount().max(1)), 1.0);
    });

    // ---- Timers ----
    let pay = conf::get().pay_secs * 20;
    plugin.run_task_timer(pay, pay, work::flush_pay);
    plugin.run_task_timer(10, 10, work::flush_bars);
    plugin.run_task_timer(1200, 1200, || {
        data::save_placed(false);
        for (uuid, d) in data::players().loaded() {
            data::summarize(&uuid, &d);
        }
    });

    // A hot reload: everyone online has their file in memory again.
    for p in horizon_bukkit::get_online_players() {
        data::players().load(&p.get_unique_id());
    }
    let s = conf::get();
    app::info(&format!("HorizonJobs ready in {} ms: {} jobs ({}), {} known workers", started.elapsed().as_millis(), s.jobs.len(), s.jobs.iter().map(|j| j.name.as_str()).collect::<Vec<_>>().join(", "), data::summaries().read(|m| m.len())));
}

fn on_disable(_plugin: Plugin) {
    work::flush_pay();
    gui::close_all();
    for (uuid, d) in data::players().loaded() {
        data::summarize(&uuid, &d);
    }
    data::players().save_all();
    data::summaries().save_now();
    data::save_placed(true);
    horizon_kit::store::flush();
    app::clear();
}

bukkit_plugin!(on_enable, on_disable);
