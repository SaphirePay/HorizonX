//! `ClickHandler.onClick` + `ClickActionTask.run`: a handler's actions, each checked for its chance, run on the
//! next tick (or after its `<delay=..>`), in order.

use crate::Holder;
use crate::menu::{Action, ActionKind};
use horizon_bukkit::{Component, Player};
use horizon_kit::hooks::economy;
use horizon_kit::text::colorize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

static SOUNDS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| include_str!("sounds.tsv").lines().filter_map(|l| l.split_once('\t')).collect());

/// A Bukkit `Sound` name (`ENTITY_PLAYER_LEVELUP`) or a key, to the key `playsound` takes.
pub fn sound_key(name: &str) -> Option<String> {
    if name.contains('.') || name.contains(':') {
        return Some(name.to_ascii_lowercase());
    }
    SOUNDS.get(name.to_ascii_uppercase().as_str()).map(|k| format!("minecraft:{k}"))
}

/// What an action needs to run later: who, the arguments and how they're filled.
#[derive(Clone)]
struct Job {
    kind: ActionKind,
    exec: String,
    uuid: String,
    args: Arc<HashMap<String, String>>,
    parse_in: bool,
    parse_after: bool,
}

/// `checkChance`: `<chance=..>` as a percentage (placeholders allowed).
pub fn chance_ok(a: &Action, h: &Holder) -> bool {
    let Some(c) = &a.chance else { return true };
    let Ok(p) = h.fill(c).trim().parse::<f64>() else { return true };
    if p >= 100.0 {
        return true;
    }
    if p <= 0.0 {
        return false;
    }
    random() * 100.0 < p
}

fn random() -> f64 {
    use std::hash::{BuildHasher, Hasher};
    let n = std::collections::hash_map::RandomState::new().build_hasher().finish();
    (n >> 11) as f64 / (1u64 << 53) as f64
}

/// The delay in ticks (`<delay=..>`), 0 for none.
pub fn delay_of(a: &Action, h: &Holder) -> u64 {
    a.delay.as_ref().and_then(|d| h.fill(d).trim().parse::<i64>().ok()).map_or(0, |d| d.max(0) as u64)
}

/// The handler: chances rolled now, the work scheduled (Java's `runTask` / `runTaskLater`).
pub fn run(actions: &[Action], h: &Holder) {
    if actions.is_empty() {
        return;
    }
    let mut now = Vec::new();
    for a in actions {
        if !chance_ok(a, h) {
            continue;
        }
        let job = Job { kind: a.kind, exec: a.exec.clone(), uuid: h.viewer.get_unique_id(), args: h.args.clone(), parse_in: h.parse_in, parse_after: h.parse_after };
        let delay = delay_of(a, h);
        if delay > 0 {
            later(delay, vec![job]);
        } else {
            now.push(job);
        }
    }
    if !now.is_empty() {
        later(1, now);
    }
}

fn later(delay: u64, jobs: Vec<Job>) {
    let Some(plugin) = crate::plugin() else { return };
    let mut jobs = Some(jobs);
    plugin.run_task_later(delay, move || {
        for job in jobs.take().into_iter().flatten() {
            execute(&job);
        }
    });
}

/// `/dm execute`: one action, at once or after its delay; the delay given back.
pub fn run_one(a: &Action, h: &Holder) -> Option<u64> {
    if !chance_ok(a, h) {
        return None;
    }
    let job = Job { kind: a.kind, exec: a.exec.clone(), uuid: h.viewer.get_unique_id(), args: h.args.clone(), parse_in: true, parse_after: true };
    let delay = delay_of(a, h);
    later(delay.max(1), vec![job]);
    Some(delay)
}

fn execute(job: &Job) {
    let Some(player) = horizon_bukkit::get_online_players().into_iter().find(|p| p.get_unique_id() == job.uuid) else { return };
    let session = crate::session_holder(&job.uuid);
    let target = session.as_ref().and_then(|h| h.placeholder.clone()).unwrap_or_else(|| player.clone());
    let filler = Holder { viewer: player.clone(), placeholder: Some(target), args: job.args.clone(), parse_in: job.parse_in, parse_after: job.parse_after };
    let exec = filler.fill(&job.exec);
    match job.kind {
        ActionKind::BroadcastSound | ActionKind::BroadcastWorldSound | ActionKind::PlaySound => {
            let parts: Vec<&str> = exec.splitn(3, ' ').collect();
            let Some(sound) = parts.first().and_then(|s| sound_key(s)) else {
                crate::warn(&format!("Sound name given for sound action: {exec}, is not a valid sound!"));
                return;
            };
            let volume = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(1.0f32);
            let pitch = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(1.0f32);
            let to: Vec<Player> = match job.kind {
                ActionKind::BroadcastSound => horizon_bukkit::get_online_players(),
                ActionKind::BroadcastWorldSound => player.get_world().get_players(),
                _ => vec![player.clone()],
            };
            for p in to {
                p.play_sound(&p.get_location(), &sound, volume, pitch);
            }
        }
        ActionKind::Meta => {
            let outcome = crate::meta::run_action(&player.get_unique_id(), &exec);
            let note = match outcome {
                crate::meta::Outcome::InvalidSyntax => "Invalid meta action! Make sure you have the right syntax.",
                crate::meta::Outcome::NewValueIsDifferentType => "Invalid meta action! New value is a different type than the old value!",
                crate::meta::Outcome::InvalidType => "Invalid meta action! The specified type is not supported for the specified action!",
                crate::meta::Outcome::ExistentValueIsDifferentType => "Invalid meta action! Existent value is a different type than the new value!",
                _ => "",
            };
            if !note.is_empty() {
                crate::debug(note);
            }
        }
        ActionKind::Player | ActionKind::PlayerCommandEvent => player.chat(&format!("/{exec}")),
        ActionKind::Placeholder => {}
        ActionKind::Chat => player.chat(&exec),
        ActionKind::Console => {
            horizon_bukkit::dispatch_command(&exec);
        }
        ActionKind::MiniMessage => player.send_component(&Component(horizon_kit::mini::parse(&exec))),
        ActionKind::MiniBroadcast => {
            let c = Component(horizon_kit::mini::parse(&exec));
            for p in horizon_bukkit::get_online_players() {
                p.send_component(&c);
            }
        }
        ActionKind::Message => player.send_message(&colorize(&exec)),
        ActionKind::Broadcast => {
            horizon_bukkit::broadcast_message(&colorize(&exec));
        }
        ActionKind::Close => crate::close_menu(&player, true, true),
        ActionKind::OpenGuiMenu | ActionKind::OpenMenu => open_action(&player, &exec, session.as_ref()),
        ActionKind::Connect => crate::debug(&format!("[connect] {exec}: there is no proxy connection on this server; skipped.")),
        ActionKind::JsonMessage => match serde_json::from_str::<Value>(&exec) {
            Ok(v) => player.send_component(&Component(v)),
            Err(_) => crate::warn(&format!("Invalid json in [json] action: {exec}")),
        },
        ActionKind::JsonBroadcast | ActionKind::BroadcastJson => match serde_json::from_str::<Value>(&exec) {
            Ok(v) => {
                let c = Component(v);
                for p in horizon_bukkit::get_online_players() {
                    p.send_component(&c);
                }
            }
            Err(_) => crate::warn(&format!("Invalid json in [jsonbroadcast] action: {exec}")),
        },
        ActionKind::Refresh => {
            if session.is_some() {
                crate::refresh(&player);
            } else {
                crate::debug(&format!("{} does not have menu open! Nothing to refresh!", player.get_name()));
            }
        }
        ActionKind::TakeMoney | ActionKind::GiveMoney => match exec.trim().parse::<f64>() {
            Ok(amount) => {
                if !economy::enabled() {
                    crate::warn("Vault not hooked! Cannot take or give money!");
                } else if job.kind == ActionKind::TakeMoney {
                    economy::withdraw(player.get_name(), amount);
                } else {
                    economy::deposit(player.get_name(), amount);
                }
            }
            Err(_) => crate::warn(&format!("Amount for money action: {exec}, is not a valid number!")),
        },
        ActionKind::TakeExp | ActionKind::GiveExp => {
            let lower = exec.trim().to_ascii_lowercase();
            let levels = lower.ends_with('l');
            match lower.replace('l', "").parse::<i32>() {
                Ok(n) if n > 0 => {
                    let n = if job.kind == ActionKind::TakeExp { -n } else { n };
                    if levels {
                        player.set_level((player.get_level() + n).max(0));
                    } else {
                        set_total_exp(&player, total_exp(&player) + n);
                    }
                }
                Ok(_) => {}
                Err(_) => crate::warn(&format!("Amount for exp action: {exec}, is not a valid number!")),
            }
        }
        ActionKind::GivePerm | ActionKind::TakePerm => {
            let give = job.kind == ActionKind::GivePerm;
            let answered = horizon_kit::hooks::provider("net.milkbowl.vault.permission.Permission")
                .map(|p| p.call(if give { "playerAdd" } else { "playerRemove" }, &json!([Value::Null, player.get_name(), exec])))
                .is_some_and(|v| v.as_bool() == Some(true));
            if !answered {
                let line = if give { format!("lp user {} permission set {exec} true", player.get_name()) } else { format!("lp user {} permission unset {exec}", player.get_name()) };
                horizon_bukkit::dispatch_command(&line);
            }
        }
    }
}

/// Experience needed to reach `level` from 0.
fn exp_at(level: i32) -> i64 {
    let l = i64::from(level);
    if level <= 16 {
        l * l + 6 * l
    } else if level <= 31 {
        (5 * l * l - 81 * l + 720) / 2
    } else {
        (9 * l * l - 325 * l + 4440) / 2
    }
}

fn exp_to_next(level: i32) -> i64 {
    if level <= 15 {
        2 * i64::from(level) + 7
    } else if level <= 30 {
        5 * i64::from(level) - 38
    } else {
        9 * i64::from(level) - 158
    }
}

fn total_exp(p: &Player) -> i32 {
    let level = p.get_level();
    (exp_at(level) + (f64::from(p.get_exp()) * exp_to_next(level) as f64).round() as i64) as i32
}

fn set_total_exp(p: &Player, total: i32) {
    let total = i64::from(total.max(0));
    let mut level = 0;
    while exp_at(level + 1) <= total && level < 21863 {
        level += 1;
    }
    let progress = (total - exp_at(level)) as f32 / exp_to_next(level) as f32;
    p.set_level(level);
    p.set_exp(progress.clamp(0.0, 0.9999));
}

fn open_action(player: &Player, exec: &str, holder: Option<&Holder>) {
    let squashed = exec.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut parts = squashed.splitn(2, ' ');
    let Some(name) = parts.next().filter(|n| !n.is_empty()) else {
        crate::warn(&format!("Could not find and open menu {exec}"));
        return;
    };
    let Some(menu) = crate::menu_named(name) else {
        crate::warn(&format!("Could not find and open menu {exec}"));
        return;
    };
    let values: Vec<String> = parts.next().map(|r| r.split(' ').map(str::to_owned).collect()).unwrap_or_default();
    let inherited = holder.map(|h| (*h.args).clone());
    let placeholder = holder.and_then(|h| h.placeholder.clone()).filter(|p| p != player);
    if menu.args.is_empty() || values.is_empty() {
        if !menu.args.is_empty() || values.is_empty() {
            crate::open_menu(&menu, player, inherited, placeholder);
        } else {
            crate::debug(&format!("Arguments were given for menu {name} in action [openguimenu] or [openmenu], but the menu does not support arguments!"));
            crate::open_menu(&menu, player, inherited, placeholder);
        }
        return;
    }
    if values.len() < menu.args.len() {
        crate::warn(&format!("Not enough arguments given for menu {name} when opening using the [openguimenu] or [openmenu] action!"));
        return;
    }
    let mut map = inherited.unwrap_or_default();
    for (i, arg) in menu.args.iter().enumerate() {
        if i + 1 == menu.args.len() {
            map.insert(arg.clone(), values[i..].join(" "));
        } else {
            map.insert(arg.clone(), values[i].clone());
        }
    }
    crate::open_menu(&menu, player, Some(map), placeholder);
}
