//! mcMMO (mcMMO-Dev, GPL-3.0) written again natively in Rust for Horizon: the same plugin name, commands,
//! permissions, config files (config.yml, experience.yml, advanced.yml, skillranks.yml, treasures...), locale and
//! FlatFile database (`flatfile/mcmmo.users`, `flatfile/parties.yml`), so an owner's files carry over.
//!
//! Hot events (block breaks, hits, fishing) do a few lookups in memory; player data lives in memory and is
//! written on a thread of its own; placeholders `%mcmmo_...%` are offered to the other plugins.

pub mod blocks;
pub mod combat;
pub mod commands;
pub mod locale;
pub mod misc;
pub mod mmo;
pub mod party;
pub mod placed;
pub mod profile;
pub mod scoreboard;
pub mod settings;
pub mod skills;
pub mod xprate;

use horizon_bukkit::event::{PlayerDeathEvent, PlayerJoinEvent, PlayerQuitEvent};
use horizon_bukkit::fam_ev_entity::{CreatureSpawnEvent, EntityDamageByEntityEvent, EntityDeathEvent, EntityExplodeEvent, EntityShootBowEvent, EntityTameEvent};
use horizon_bukkit::fam_ev_player::{AsyncPlayerChatEvent, BrewEvent, FurnaceExtractEvent, PlayerGameModeChangeEvent, PlayerInteractEntityEvent, PlayerInteractEvent, PlayerRespawnEvent, PlayerTeleportEvent};
use horizon_bukkit::fam_ev_world::{BlockBreakEvent, BlockPistonExtendEvent, BlockPistonRetractEvent, BlockPlaceEvent, StructureGrowEvent};
use horizon_bukkit::{Plugin, bukkit_plugin, enums};
use std::sync::Mutex;

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

/// Runs on the server thread next tick (from anywhere).
pub fn later(task: impl FnOnce() + Send + 'static) {
    let Some(p) = plugin() else { return };
    let mut once = Some(task);
    p.run_task(move || {
        if let Some(t) = once.take() {
            t();
        }
    });
}

/// Every config and the locale read again.
pub fn reload() {
    let f = folder();
    settings::load(&f);
    locale::load(&f);
}

/// A player's session made (join, or a reload with players on).
fn open_session(player: &horizon_bukkit::Player) {
    let s = settings::get();
    let mut sess = mmo::Session::new(player);
    sess.perms = mmo::read_perms(player);
    sess.creative = player.get_game_mode() == "CREATIVE";
    let uuid = sess.uuid.clone();
    let name = sess.name.clone();
    {
        let mut ss = mmo::sessions();
        ss.insert(uuid.clone(), sess);
        let mut st = profile::store();
        let starting = s.adv("Skills.General.StartingLevel", 0.0) as i32;
        let p = st.ensure(&uuid, &name, starting);
        p.last_login = profile::now_secs();
    }
    party::rename_member(&uuid, &name);
    if xprate::active() {
        player.send_message(&locale::get("XPRate.Event", &[&format!("{}", xprate::global())]));
    }
}

fn close_session(uuid: &str) {
    let sess = mmo::sessions().remove(uuid);
    if let Some(sess) = sess {
        mmo::drop_bars(&sess);
    }
    profile::store().mark_dirty();
}

/// Reads permissions again for a few players each second (each is reread every ~2 minutes).
fn refresh_some_perms() {
    static CURSOR: Mutex<usize> = Mutex::new(0);
    let players = horizon_bukkit::get_online_players();
    if players.is_empty() {
        return;
    }
    let per_tick = players.len().div_ceil(120).max(1);
    let mut c = CURSOR.lock().unwrap_or_else(|e| e.into_inner());
    for _ in 0..per_tick {
        *c = (*c + 1) % players.len();
        let p = &players[*c];
        let bits = mmo::read_perms(p);
        if let Some(s) = mmo::sessions().get_mut(&p.get_unique_id().to_ascii_lowercase()) {
            s.perms = bits;
        }
    }
}

/// Party members' places, for XP share range (one location read per member a second).
fn snapshot_positions() {
    let members: Vec<String> = party::parties().member_of.keys().cloned().collect();
    if members.is_empty() {
        return;
    }
    let players: Vec<(String, horizon_bukkit::Player)> = {
        let ss = mmo::sessions();
        members.iter().filter_map(|u| ss.get(u).map(|s| (u.clone(), s.player.clone()))).collect()
    };
    let places: Vec<(String, (String, f64, f64, f64))> = players
        .into_iter()
        .map(|(u, p)| {
            let at = p.get_location();
            (u, (at.get_world().get_name().to_owned(), at.get_x(), at.get_y(), at.get_z()))
        })
        .collect();
    let mut ss = mmo::sessions();
    for (u, pos) in places {
        if let Some(s) = ss.get_mut(&u) {
            s.pos = Some(pos);
        }
    }
}

fn facing(data: &str) -> (i32, i32, i32) {
    match blocks::prop(data, "facing") {
        Some("north") => (0, 0, -1),
        Some("south") => (0, 0, 1),
        Some("east") => (1, 0, 0),
        Some("west") => (-1, 0, 0),
        Some("up") => (0, 1, 0),
        _ => (0, -1, 0),
    }
}

fn shift_placed(blocks: &[horizon_bukkit::Block], d: (i32, i32, i32)) {
    let moved: Vec<(String, i32, i32, i32)> = blocks.iter().filter(|b| placed::is_placed(b.get_world().get_name(), b.get_x(), b.get_y(), b.get_z())).map(|b| (b.get_world().get_name().to_owned(), b.get_x(), b.get_y(), b.get_z())).collect();
    for (w, x, y, z) in &moved {
        placed::set(w, *x, *y, *z, false);
    }
    for (w, x, y, z) in moved {
        placed::set(&w, x + d.0, y + d.1, z + d.2, true);
    }
}

fn placeholders(uuid: &str, _name: &str, params: &str) -> Option<String> {
    let s = settings::get();
    let uuid = uuid.to_ascii_lowercase();
    let p = || profile::store().get(&uuid).cloned();
    let lower = params.to_ascii_lowercase();
    // Leaderboards: mctop_<skill|overall>_<n>, mctop_name_<skill|overall>_<n>.
    if let Some(rest) = lower.strip_prefix("mctop_") {
        let (name_wanted, rest) = match rest.strip_prefix("name_") {
            Some(r) => (true, r),
            None => (false, rest),
        };
        let (board, n) = rest.rsplit_once('_')?;
        let n: usize = n.parse().ok()?;
        let boards = profile::boards();
        let b = if board == "overall" || board == "power" { &boards.power } else { &boards.skills[skills::Skill::parse(board)?.idx()] };
        let row = b.rows.get(n.checked_sub(1)?);
        return Some(match (row, name_wanted) {
            (Some(r), true) => r.1.clone(),
            (Some(r), false) => r.2.to_string(),
            (None, _) => String::new(),
        });
    }
    match lower.as_str() {
        "power_level" => return Some(p()?.power_level().to_string()),
        "power_level_cap" => return Some(s.power_cap.to_string()),
        "xprate" => return Some(format!("{}", xprate::global())),
        "is_xp_event_active" => return Some(xprate::active().to_string()),
        "in_party" => return Some(party::party_name(&uuid).is_some().to_string()),
        "party_name" => return Some(party::party_name(&uuid).unwrap_or_default()),
        "party_size" => return Some(party::parties().of(&uuid).map(|p| p.members.len()).unwrap_or(0).to_string()),
        "party_leader" => return Some(party::parties().of(&uuid).map(|p| p.leader.1.clone()).unwrap_or_default()),
        "is_party_leader" => return Some(party::parties().of(&uuid).is_some_and(|p| p.leader.0 == uuid).to_string()),
        "rank_overall" => return Some(profile::boards().power.place.get(&uuid).map(|n| n.to_string()).unwrap_or_default()),
        _ => {}
    }
    for (prefix, what) in [("xp_needed_", 0), ("xp_remaining_", 1), ("xp_", 2), ("level_", 3), ("rank_", 4), ("xprate_", 5), ("skillname_header_", 6), ("skillname_", 7)] {
        if let Some(sk) = lower.strip_prefix(prefix) {
            let skill = skills::Skill::parse(sk)?;
            return Some(match what {
                0 => mmo::xp_to_level(&s, &p()?, skill).to_string(),
                1 => {
                    let pr = p()?;
                    (mmo::xp_to_level(&s, &pr, skill) - pr.xp[skill.idx()] as i64).to_string()
                }
                2 => (p()?.xp[skill.idx()] as i64).to_string(),
                3 => p()?.level(skill).to_string(),
                4 => profile::boards().skills[skill.idx()].place.get(&uuid).map(|n| n.to_string()).unwrap_or_default(),
                5 => format!("{}", xprate::rate(skill)),
                6 => locale::get(&format!("{}.SkillName", skill.name()), &[]),
                _ => mmo::skill_name(skill),
            });
        }
    }
    None
}

fn on_enable(plugin: Plugin) {
    *PLUGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(plugin);
    let started = std::time::Instant::now();
    let f = plugin.get_data_folder();
    let _ = std::fs::write(f.join("LICENSE"), include_str!("../LICENSE"));
    reload();
    party::load(&f);
    profile::start(f.clone(), || {
        // Players already on (a reload): their sessions, once the file is in.
        later(|| {
            for p in horizon_bukkit::get_online_players() {
                let has = mmo::sessions().contains_key(&p.get_unique_id().to_ascii_lowercase());
                if !has {
                    open_session(&p);
                }
            }
        });
    });
    {
        let f2 = f.clone();
        std::thread::Builder::new().name("mcMMO-placed".into()).spawn(move || placed::load(f2)).ok();
    }
    commands::register(plugin);
    horizon_kit::hooks::provide_placeholders(&plugin, "mcmmo", placeholders);

    // ---- Players ----
    plugin.register_event(|e: &mut PlayerJoinEvent| open_session(e.get_player()));
    plugin.register_event(|e: &mut PlayerQuitEvent| close_session(&e.get_player().get_unique_id().to_ascii_lowercase()));
    plugin.register_event(|e: &mut PlayerGameModeChangeEvent| {
        if let Some(p) = e.get_player() {
            let creative = e.get_new_game_mode().eq_ignore_ascii_case("CREATIVE");
            if let Some(s) = mmo::sessions().get_mut(&p.get_unique_id().to_ascii_lowercase()) {
                s.creative = creative;
            }
        }
    });
    plugin.register_event(|e: &mut PlayerTeleportEvent| {
        if let Some(s) = mmo::sessions().get_mut(&e.player.get_unique_id().to_ascii_lowercase()) {
            s.teleport_at = profile::now_ms();
        }
    });
    plugin.register_event(|e: &mut PlayerRespawnEvent| {
        if let Some(s) = mmo::sessions().get_mut(&e.player.get_unique_id().to_ascii_lowercase()) {
            s.respawn_at = profile::now_ms();
        }
    });
    plugin.register_event(|e: &mut PlayerDeathEvent| {
        let victim = e.get_player().clone();
        let killer = combat::last_attacker(&victim.get_unique_id().to_ascii_lowercase());
        combat::on_player_death(&victim, killer.as_ref());
    });
    plugin.register_event_with(enums::event_priority::LOWEST, false, |e: &mut AsyncPlayerChatEvent| {
        let p = e.get_player().clone();
        let mode = mmo::sessions().get(&p.get_unique_id().to_ascii_lowercase()).map(|s| s.chat).unwrap_or(mmo::Chat::None);
        let s = settings::get();
        match mode {
            mmo::Chat::Party if s.chat_party => {
                e.set_cancelled(true);
                let msg = e.get_message().to_owned();
                later(move || party::send_party_chat(&p, &msg));
            }
            mmo::Chat::Admin if s.chat_admin => {
                e.set_cancelled(true);
                let msg = e.get_message().to_owned();
                later(move || party::send_admin_chat(p.get_display_name(), &msg));
            }
            _ => {}
        }
    });

    // ---- Blocks ----
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut BlockBreakEvent| {
        let player = e.get_player().clone();
        let block = e.get_block().clone();
        horizon_kit::stats::timed("block break", || blocks::on_break(&player, &block));
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut BlockPlaceEvent| {
        let b = e.get_block_placed();
        placed::set(b.get_world().get_name(), b.get_x(), b.get_y(), b.get_z(), true);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut BlockPistonExtendEvent| {
        let Some(piston) = e.get_block() else { return };
        let d = facing(&piston.get_block_data());
        shift_placed(&e.get_blocks(), d);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut BlockPistonRetractEvent| {
        let Some(piston) = e.get_block() else { return };
        let (dx, dy, dz) = facing(&piston.get_block_data());
        shift_placed(&e.get_blocks(), (-dx, -dy, -dz));
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut StructureGrowEvent| {
        let grown: Vec<(String, i32, i32, i32)> = e
            .get_blocks()
            .iter()
            .map(|s| {
                let b = s.get_block();
                (b.get_world().get_name().to_owned(), b.get_x(), b.get_y(), b.get_z())
            })
            .collect();
        for (w, x, y, z) in grown {
            placed::set(&w, x, y, z, false);
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut EntityExplodeEvent| {
        let Some(at) = e.get_location() else { return };
        let list = e.block_list().clone();
        blocks::on_explode(&at, &list);
    });

    // ---- Interaction ----
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut PlayerInteractEvent| {
        let player = e.player.clone();
        let action = e.action;
        if action == "LEFT_CLICK_BLOCK" {
            // The tool, read when digging starts: inside BlockBreakEvent the engine's equipment reads empty.
            blocks::remember_held(&player);
        }
        match action {
            "RIGHT_CLICK_BLOCK" | "RIGHT_CLICK_AIR" => {
                if e.get_hand() == Some("OFF_HAND") {
                    return;
                }
                let item = e.get_item();
                let held = item.as_ref().map(|i| i.get_type().key().to_owned()).unwrap_or_default();
                if action == "RIGHT_CLICK_BLOCK"
                    && let Some(block) = e.block.clone()
                {
                    let data = block.get_block_data();
                    let m = blocks::mat(&data).to_owned();
                    let s = settings::get();
                    if m == s.repair_anvil || m == s.salvage_anvil {
                        if misc::on_anvil(&player, &m, item) {
                            e.set_cancelled(true);
                        }
                        return;
                    }
                    if m == "brewing_stand" {
                        misc::used_stand(&player, &block);
                        return;
                    }
                    if m == "tnt" && blocks::mat(&held) == s.detonator && player.is_sneaking() {
                        blocks::on_detonate(&player, &block);
                        return;
                    }
                    if blocks::green_thumb_block(&player, &block, &held) {
                        e.set_cancelled(true);
                        return;
                    }
                    // Interactive blocks (doors, chests...) don't ready tools.
                    if matches!(m.as_str(), "chest" | "trapped_chest" | "barrel" | "furnace" | "blast_furnace" | "smoker" | "crafting_table" | "anvil" | "enchanting_table" | "lever" | "shulker_box") || m.ends_with("_door") || m.ends_with("_button") || m.ends_with("_trapdoor") || m.ends_with("_fence_gate") || m.ends_with("_bed") {
                        return;
                    }
                }
                mmo::ready_tool(&player.get_unique_id().to_ascii_lowercase(), &held, player.is_sneaking());
            }
            "LEFT_CLICK_BLOCK" => {
                if let Some(block) = e.block.clone() {
                    let held = e.get_item().map(|i| i.get_type().key().to_owned()).unwrap_or_default();
                    blocks::on_left_click(&player, &block, &held);
                }
            }
            _ => {}
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut PlayerInteractEntityEvent| {
        if e.get_hand() != "HAND" {
            return;
        }
        let player = e.player.clone();
        let held = player.get_inventory().get_item_in_main_hand().map(|i| i.get_type().key().to_owned()).unwrap_or_default();
        if blocks::mat(&held) == "bone" {
            let target = e.get_right_clicked();
            combat::beast_lore(&player, &target);
        }
    });

    // ---- Combat ----
    plugin.register_event_with(enums::event_priority::HIGHEST, true, |e: &mut EntityDamageByEntityEvent| horizon_kit::stats::timed("hit", || combat::on_hit(e)));
    plugin.register_event_with(enums::event_priority::HIGHEST, true, combat::on_damage);
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut EntityDeathEvent| {
        let en = e.get_entity();
        let id = en.get_entity_id();
        let at = en.get_location();
        combat::on_death(id, at);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut CreatureSpawnEvent| {
        if let Some(en) = e.get_entity() {
            combat::on_spawn(en.get_entity_id(), &e.get_spawn_reason());
        }
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut EntityShootBowEvent| {
        let Some(horizon_bukkit::fam_ev_entity::Actor::Player(p)) = e.get_entity() else { return };
        let Some(proj) = e.get_projectile() else { return };
        let crossbow = e.get_bow().is_some_and(|b| b.get_type().key() == "minecraft:crossbow");
        combat::on_shoot(proj.get_entity_id(), &p, crossbow);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut EntityTameEvent| {
        let (Some(owner), Some(en)) = (e.get_owner(), e.get_entity()) else { return };
        let kind = combat::kind_of(&en);
        combat::on_tame(en.get_entity_id());
        misc::on_tame(&owner, &kind);
    });

    // ---- Fishing, smelting, brewing ----
    plugin.register_event_with(enums::event_priority::HIGHEST, true, misc::on_fish);
    plugin.register_event_with(enums::event_priority::MONITOR, false, |e: &mut FurnaceExtractEvent| {
        let (Some(p), Some(t)) = (e.get_player(), e.get_item_type()) else { return };
        let amount = e.get_item_amount();
        misc::on_extract(&p, blocks::mat(t.key()), amount);
    });
    plugin.register_event_with(enums::event_priority::MONITOR, true, |e: &mut BrewEvent| misc::on_brew(e));

    // ---- Timers ----
    plugin.run_task_timer(20, 20, || {
        mmo::tick_abilities();
        scoreboard::tick();
        snapshot_positions();
        refresh_some_perms();
    });
    plugin.run_task_timer(4, 4, mmo::flush_bars);
    plugin.run_task_timer(10, 10, combat::tick_bleeds);

    for p in horizon_bukkit::get_online_players() {
        open_session(&p);
    }
    log(&format!("mcMMO {} enabled in {} ms (Horizon native, GPL-3.0).", "2.3.003", started.elapsed().as_millis()));
}

fn on_disable(_plugin: Plugin) {
    let bars: Vec<horizon_bukkit::BossBar> = mmo::sessions().values().flat_map(|s| s.bars.values().map(|b| b.bar.clone()).collect::<Vec<_>>()).collect();
    for b in bars {
        b.remove_all();
    }
    profile::stop();
    party::save();
    placed::save_dirty();
}

bukkit_plugin!(on_enable, on_disable);
