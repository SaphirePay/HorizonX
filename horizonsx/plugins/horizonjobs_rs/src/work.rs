//! Earning: the game events that pay, the exp and level-ups they give, and the money paid out in batches.
//!
//! A hot event looks the player's jobs up in memory and the payout in a hash table; money waits in a per-player
//! sum paid every few seconds (one economy call), and the action bar shows the last couple of seconds' earnings
//! twice a second at most.

use crate::conf::{self, Job, Work};
use crate::{data, ui};
use horizon_bukkit::Player;
use horizon_kit::hooks::{chat, economy};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

/// Money waiting to be paid: uuid -> (name, amount).
static PENDING: LazyLock<Mutex<HashMap<String, (String, f64)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// What the action bar shows: earnings of the last moments in one job.
struct Bar {
    job: String,
    money: f64,
    exp: f64,
    since: u64,
    dirty: bool,
}

static BARS: LazyLock<Mutex<HashMap<String, Bar>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Mobs from spawners (they pay `spawner-multiplier`).
static SPAWNER_MOBS: LazyLock<Mutex<HashSet<u64>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

pub fn spawner_mob(id: u64) {
    let mut m = SPAWNER_MOBS.lock().unwrap_or_else(|e| e.into_inner());
    if m.len() > 100_000 {
        m.clear();
    }
    m.insert(id);
}

pub fn from_spawner(id: u64) -> bool {
    SPAWNER_MOBS.lock().unwrap_or_else(|e| e.into_inner()).remove(&id)
}

/// The pay multiplier of a level: +income-per-level each level above 1.
pub fn level_mult(level: u32) -> f64 {
    1.0 + conf::get().income_per_level * f64::from(level.max(1) - 1)
}

/// Server boost plus the rank's (`horizonjobs.boost` meta), as a multiplier.
pub fn boost(name: &str) -> f64 {
    let rank = chat::meta_int(name, "horizonjobs.boost").unwrap_or(0) as f64;
    (1.0 + (conf::get().boost_percent + rank) / 100.0).max(0.0)
}

/// How many jobs this player may have at once (a rank can raise the config's number).
pub fn max_jobs(p: &Player) -> usize {
    let base = conf::get().max_jobs;
    chat::meta_int(p.get_name(), "horizonjobs.max-jobs").map(|v| (v.max(1) as usize).max(base)).unwrap_or(base)
}

/// The player did `work` on `thing` (`amount` of them): every job they work that pays for it pays.
pub fn earn(p: &Player, work: Work, thing: &str, amount: f64, mult: f64) {
    let s = conf::get();
    let uuid = p.get_unique_id();
    let Some(jobs) = data::players().peek(&uuid, |d| d.jobs.clone()) else { return };
    let paying: Vec<(&Job, f64, f64)> = jobs.iter().filter_map(|id| s.job(id)).filter_map(|j| j.pays_for(work, thing).map(|(m, e)| (j, m, e))).collect();
    if paying.is_empty() {
        return;
    }
    if !s.creative_earns && p.get_game_mode().eq_ignore_ascii_case("CREATIVE") {
        return;
    }
    let name = p.get_name().to_owned();
    let boost = boost(&name);
    let now = horizon_kit::time::now_ms();
    for (job, base_money, base_exp) in paying {
        let gained = base_exp * amount * mult * boost;
        let (money, ups) = data::edit(&uuid, |d| {
            d.name.clone_from(&name);
            let pr = d.progress.entry(job.id.clone()).or_insert_with(|| data::Prog { level: 1, ..Default::default() });
            pr.level = pr.level.max(1);
            let money = base_money * amount * mult * boost * level_mult(pr.level);
            pr.earned += money;
            pr.total_exp += gained;
            pr.exp += gained;
            let mut ups = Vec::new();
            while pr.level < job.max_level && pr.exp >= job.needed(pr.level) {
                pr.exp -= job.needed(pr.level);
                pr.level += 1;
                ups.push(pr.level);
            }
            if pr.level >= job.max_level {
                pr.exp = pr.exp.min(job.needed(pr.level));
            }
            (money, ups)
        });
        if money > 0.0 {
            pay(&uuid, &name, money);
        }
        {
            let mut bars = BARS.lock().unwrap_or_else(|e| e.into_inner());
            let b = bars.entry(uuid.clone()).or_insert_with(|| Bar { job: job.id.clone(), money: 0.0, exp: 0.0, since: now, dirty: false });
            if b.job != job.id || now.saturating_sub(b.since) > 2500 {
                *b = Bar { job: job.id.clone(), money: 0.0, exp: 0.0, since: now, dirty: false };
            }
            b.money += money;
            b.exp += gained;
            b.dirty = true;
        }
        for level in ups {
            level_up(p, job, level);
        }
    }
}

fn pay(uuid: &str, name: &str, money: f64) {
    let mut p = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let e = p.entry(uuid.to_owned()).or_insert_with(|| (name.to_owned(), 0.0));
    e.1 += money;
}

fn level_up(p: &Player, job: &Job, level: u32) {
    let s = conf::get();
    let bonus = s.level_money * f64::from(level);
    if bonus > 0.0 {
        pay(&p.get_unique_id(), p.get_name(), bonus);
    }
    let c = &job.color;
    ui::title(p, &format!("<{c}><bold>{}</bold>", ui::small(&format!("{} level up", job.name))), &format!("<{}>{} Lvl <white>[{level}]", ui::BODY, job.name));
    ui::levelup(p);
    let mut line = format!("<{c}>{}</{c}> <{}>reached <white>{} Lvl [{level}]</white>", ui::small("level up!"), ui::BODY, job.name);
    if bonus > 0.0 {
        line.push_str(&format!(" <{}>• <#55ff55>+{}", ui::DIM, ui::money(bonus)));
    }
    if level >= job.max_level {
        line.push_str(&format!(" <{}>• <{c}>max level!", ui::DIM));
    }
    ui::say(p, &line);
    if s.announce_every > 0 && level % s.announce_every == 0 {
        let msg = format!("<white>{}</white> <{}>reached <{c}>{} Lvl <white>[{level}]</white><{}>!", p.get_name(), ui::BODY, job.name, ui::BODY);
        for other in horizon_bukkit::get_online_players() {
            if other.get_unique_id() != p.get_unique_id() {
                ui::say(&other, &msg);
            }
        }
    }
    let uuid = p.get_unique_id();
    let d = data::get(&uuid);
    data::summarize(&uuid, &d);
}

/// Pays everything waiting (server thread, every pay-every-seconds).
pub fn flush_pay() {
    let batch: Vec<(String, (String, f64))> = PENDING.lock().unwrap_or_else(|e| e.into_inner()).drain().collect();
    if batch.is_empty() {
        return;
    }
    if !economy::enabled() {
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            horizon_kit::app::warn("No economy (Vault service) found: job pay is not being paid. Install Essentials or a Vault economy.");
        }
        return;
    }
    for (_, (name, money)) in batch {
        if money > 0.0 {
            economy::deposit(&name, (money * 100.0).round() / 100.0);
        }
    }
}

/// The action bars that changed (server thread, twice a second).
pub fn flush_bars() {
    let s = conf::get();
    let shown: Vec<(String, String, f64, f64)> = {
        let mut bars = BARS.lock().unwrap_or_else(|e| e.into_inner());
        let now = horizon_kit::time::now_ms();
        bars.retain(|_, b| now.saturating_sub(b.since) < 10_000);
        bars.iter_mut().filter(|(_, b)| b.dirty).map(|(u, b)| {
            b.dirty = false;
            (u.clone(), b.job.clone(), b.money, b.exp)
        }).collect()
    };
    for (uuid, job_id, money, exp) in shown {
        let (Some(p), Some(job)) = (horizon_bukkit::fam_ess::get_player_by_id(&uuid), s.job(&job_id)) else { continue };
        let pr = data::players().peek(&uuid, |d| d.prog(&job_id)).unwrap_or_default();
        let need = job.needed(pr.level());
        let pct = if pr.level() >= job.max_level { 100.0 } else { (pr.exp / need * 100.0).clamp(0.0, 100.0) };
        let c = &job.color;
        let money_part = if money > 0.0 { format!("<#55ff55>+{} <#52525b>| ", ui::money(money)) } else { String::new() };
        ui::bar(&p, &format!("{money_part}<{c}>+{} XP <#52525b>| <{}>{} Lvl <white>[{}] <{}>({:.1}%)", ui::exp(exp), ui::BODY, job.name, pr.level(), ui::GREY, pct));
    }
}

// ---- What the events see ----

/// `minecraft:wheat[age=7]` -> (`wheat`, `age=7`).
pub fn split_data(data: &str) -> (String, String) {
    let s = data.trim_start_matches("minecraft:");
    let (m, props) = s.split_once('[').unwrap_or((s, ""));
    (m.to_owned(), props.trim_end_matches(']').to_owned())
}

fn prop<'a>(props: &'a str, key: &str) -> Option<&'a str> {
    props.split(',').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
}

/// The age a crop is fully grown at (crops pay only then; they are never "placed" blocks).
pub fn ripe_age(mat: &str) -> Option<&'static str> {
    Some(match mat {
        "wheat" | "carrots" | "potatoes" => "7",
        "beetroots" | "nether_wart" | "sweet_berry_bush" => "3",
        "cocoa" => "2",
        "torchflower_crop" => "2",
        "pitcher_crop" => "4",
        _ => return None,
    })
}

/// Whether breaking this block (as its block data reads) may pay, before the placed check.
pub fn natural(mat: &str, props: &str) -> bool {
    if let Some(ripe) = ripe_age(mat) {
        return prop(props, "age") == Some(ripe);
    }
    if mat.ends_with("_leaves") {
        return prop(props, "persistent") != Some("true");
    }
    true
}

/// Blocks worth remembering when placed: paid for breaking, and not a crop or leaves (those have their own rule).
pub fn tracks_placed(mat: &str) -> bool {
    ripe_age(mat).is_none() && !mat.ends_with("_leaves") && conf::get().paid(Work::Break, mat)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crops_and_leaves() {
        let (m, p) = split_data("minecraft:wheat[age=7]");
        assert!(natural(&m, &p));
        let (m, p) = split_data("minecraft:wheat[age=3]");
        assert!(!natural(&m, &p));
        let (m, p) = split_data("minecraft:oak_leaves[distance=1,persistent=true,waterlogged=false]");
        assert!(!natural(&m, &p));
        let (m, p) = split_data("minecraft:oak_leaves[distance=1,persistent=false,waterlogged=false]");
        assert!(natural(&m, &p));
        assert!(tracks_placed("diamond_ore"));
        assert!(!tracks_placed("wheat"));
        assert!(!tracks_placed("dirt"));
        assert!((level_mult(51) - 2.0).abs() < 1e-9);
    }
}
