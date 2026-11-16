//! Clicking an NPC: the interaction cooldown, then its actions in order (ActionExecutor): message,
//! player_command, console_command, player_command_as_op (run as player_command), send_to_server (no proxy: does
//! nothing), wait, play_sound, need_permission, need_item, execute_random_action, block_until_done.

use crate::npc::Action;
use crate::view;
use horizon_bukkit::{CommandSender, Material, Player};
use std::time::{Duration, Instant};

pub const TYPES: [&str; 11] = ["message", "player_command", "console_command", "player_command_as_op", "send_to_server", "wait", "play_sound", "need_permission", "need_item", "execute_random_action", "block_until_done"];

pub fn requires_value(action: &str) -> bool {
    !matches!(action, "execute_random_action" | "block_until_done")
}

/// "1h", "30s", "500ms", "2.5" (seconds) -> seconds.
pub fn interval(s: &str) -> Option<f64> {
    let s = s.trim().to_ascii_lowercase();
    if let Ok(n) = s.parse::<f64>() {
        return Some(n);
    }
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let n: f64 = digits.parse().ok()?;
    Some(match &s[digits.len()..] {
        "ms" => n / 1000.0,
        "s" => n,
        "m" => n * 60.0,
        "h" => n * 3600.0,
        "d" => n * 86400.0,
        _ => return None,
    })
}

pub fn format_seconds(secs: f64) -> String {
    if secs >= 3600.0 {
        format!("{}h {}m", (secs / 3600.0) as i64, ((secs % 3600.0) / 60.0) as i64)
    } else if secs >= 60.0 {
        format!("{}m {}s", (secs / 60.0) as i64, (secs % 60.0) as i64)
    } else if secs >= 1.0 {
        format!("{:.1}s", secs)
    } else {
        format!("{}ms", (secs * 1000.0) as i64)
    }
}

/// A click on the NPC with this entity id.
pub fn interact(p: &Player, entity_id: i32, trigger: &str) {
    let uuid = p.get_unique_id();
    let (name, cooldown, list, any) = {
        let mut all = view::npcs();
        let Some(n) = all.iter_mut().find(|n| n.entity_id == entity_id) else { return };
        let cd = n.data.interaction_cooldown;
        if cd > 0.0 && !p.has_permission("fancynpcs.bypass.cooldown") && !p.has_permission(&format!("fancynpcs.bypass.cooldown.{}", n.data.name)) {
            if let Some(last) = n.last_interaction.get(&uuid) {
                let left = Duration::from_secs_f32(cd).saturating_sub(last.elapsed());
                if !left.is_zero() {
                    let silent = crate::cfg_bool("disable_interaction_cooldown_message", false);
                    drop(all);
                    if !silent {
                        crate::say_plain(&CommandSender::Player(p.clone()), "interaction_on_cooldown", &[("time", &format_seconds(left.as_secs_f64()))]);
                    }
                    return;
                }
            }
            n.last_interaction.insert(uuid.clone(), Instant::now());
        }
        (n.data.name.clone(), cd, n.data.actions_of(trigger), n.data.actions_of("ANY_CLICK"))
    };
    let _ = cooldown;
    run(p.clone(), name.clone(), list, 0);
    if trigger == "LEFT_CLICK" || trigger == "RIGHT_CLICK" {
        run(p.clone(), name, any, 0);
    }
}

/// The actions from `index` on; a wait goes on later through the scheduler.
pub fn run(p: Player, npc: String, actions: Vec<Action>, index: usize) {
    let mut i = index;
    while i < actions.len() {
        let a = &actions[i];
        let value = a.value.clone().unwrap_or_default();
        let filled = || horizon_kit::hooks::placeholders(&value.replace("{player}", p.get_name()), Some(&p));
        match a.action.to_ascii_lowercase().as_str() {
            "message" => {
                if !value.is_empty() {
                    p.send_component(&horizon_bukkit::Component(horizon_kit::mini::parse_mixed(&filled())));
                }
            }
            "player_command" | "player_command_as_op" => {
                let cmd = filled();
                let cmd = cmd.trim_start_matches('/').to_owned();
                if cmd.to_ascii_lowercase().starts_with("server ") {
                    crate::log(&format!("send_to_server '{}' for {}: there is no proxy here.", &cmd[7..], p.get_name()));
                } else if !cmd.is_empty() {
                    p.perform_command(&cmd);
                }
            }
            "console_command" => {
                let cmd = filled();
                let cmd = cmd.trim_start_matches('/');
                if !cmd.is_empty() && !horizon_bukkit::dispatch_command(cmd) {
                    crate::log(&format!("Failed to execute command: {cmd}"));
                }
            }
            "send_to_server" => crate::log(&format!("send_to_server '{value}' for {}: there is no proxy here.", p.get_name())),
            "play_sound" => {
                if !value.is_empty() {
                    p.play_sound(&p.get_location(), &filled(), 1.0, 1.0);
                }
            }
            "need_permission" => {
                let (invert, perm) = match value.strip_prefix('!') {
                    Some(rest) => (true, rest),
                    None => (false, value.as_str()),
                };
                if p.has_permission(perm) == invert {
                    crate::say_plain(&CommandSender::Player(p.clone()), "action_missing_permissions", &[]);
                    return;
                }
            }
            "need_item" => {
                let parts: Vec<&str> = value.split_whitespace().collect();
                if let Some(first) = parts.first() {
                    let (invert, item) = match first.strip_prefix('!') {
                        Some(rest) => (true, rest),
                        None => (false, *first),
                    };
                    let amount: i32 = parts.get(1).and_then(|a| a.parse().ok()).unwrap_or(1);
                    let material = Material::of(item);
                    let inv = p.get_inventory();
                    let has = if amount <= 1 { inv.contains(&material) } else { inv.contains_at_least(&horizon_bukkit::ItemStack::new(material.clone(), 1), amount) };
                    if has == invert {
                        crate::say_plain(&CommandSender::Player(p.clone()), "action_missing_item", &[("amount", &amount.to_string()), ("item", &item.to_ascii_lowercase())]);
                        return;
                    }
                }
            }
            "wait" => {
                let secs = interval(&value).unwrap_or(0.0).max(0.0);
                let ticks = (secs * 20.0).round() as u64;
                let (p2, npc2, rest) = (p.clone(), npc.clone(), actions.clone());
                let mut once = Some((p2, npc2, rest));
                horizon_bukkit::scheduler::run_task_later(ticks.max(1), move || {
                    if let Some((p, npc, rest)) = once.take() {
                        run(p, npc, rest, i + 1);
                    }
                });
                return;
            }
            "execute_random_action" => {
                let rest = &actions[i + 1..];
                if rest.is_empty() {
                    crate::log(&format!("Misplaced execute_random_action for npc '{npc}': no following actions to choose from."));
                    return;
                }
                let pick = rest[crate::rand_u32() as usize % rest.len()].clone();
                run(p, npc, vec![pick], 0);
                return;
            }
            _ => {}
        }
        i += 1;
    }
}
