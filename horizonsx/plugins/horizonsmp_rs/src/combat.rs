//! PvP rules: a hit between players puts both "in combat" for a few seconds (shown on the action bar), during
//! which escape commands don't work and leaving the game kills you. New players are protected for their first
//! minutes of play. Teammates don't hurt each other, nor do players inside a claim with PvP off. Kills and deaths
//! are counted for /stats and paid out to bounties.

use crate::{bounty, claims, conf, data, teams, ui};
use horizon_bukkit::fam_ev_entity::Actor;
use horizon_bukkit::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;

const SECTION: &str = "Combat";

#[derive(Clone)]
struct Tag {
    until: u64,
    by: String,
    by_name: String,
    shown_free: bool,
}

static TAGS: Mutex<Option<HashMap<String, Tag>>> = Mutex::new(None);

fn tags<R>(f: impl FnOnce(&mut HashMap<String, Tag>) -> R) -> R {
    let mut held = TAGS.lock().unwrap_or_else(|e| e.into_inner());
    f(held.get_or_insert_with(HashMap::new))
}

fn now() -> u64 {
    horizon_kit::time::now_ms()
}

pub fn tagged(uuid: &str) -> bool {
    let t = now();
    tags(|m| m.get(uuid).is_some_and(|g| g.until > t))
}

/// Whether a player still has their new-player protection.
pub fn protected(uuid: &str) -> bool {
    let mins = conf::get().protection_minutes;
    if mins == 0 {
        return false;
    }
    let d = data::get(uuid);
    !d.pvp_protection_off && d.playtime < mins * 60
}

fn protection_left(uuid: &str) -> u64 {
    let d = data::get(uuid);
    (conf::get().protection_minutes * 60).saturating_sub(d.playtime)
}

fn tag(victim: &Player, attacker: &Player) {
    let secs = conf::get().tag_seconds;
    let until = now() + secs * 1000;
    let (vu, au) = (victim.get_unique_id(), attacker.get_unique_id());
    let (vn, an) = (victim.get_name().to_owned(), attacker.get_name().to_owned());
    let fresh = tags(|m| {
        let mut fresh = Vec::new();
        for (who, by, by_name) in [(&vu, &au, &an), (&au, &vu, &vn)] {
            let was = m.get(who).is_some_and(|g| g.until > now());
            m.insert(who.clone(), Tag { until, by: by.clone(), by_name: by_name.clone(), shown_free: false });
            if !was {
                fresh.push(who.clone());
            }
        }
        fresh
    });
    for (p, other) in [(victim, &an), (attacker, &vn)] {
        if fresh.contains(&p.get_unique_id()) {
            ui::say(p, SECTION, &format!("<#f87171>You're in combat with <white>{other}</white>. Don't log out for {secs} seconds!"));
            ui::sound(p, "ENTITY_ITEM_BREAK:0.4:0.6");
        }
    }
}

/// The player who threw the hit, melee or shot.
fn attacker_of(e: &EntityDamageByEntityEvent) -> Option<Player> {
    horizon_kit::hooks::attacker(e)
}

fn on_damage(e: &mut EntityDamageByEntityEvent) {
    let Some(attacker) = attacker_of(e) else { return };
    let Some(victim) = e.get_entity() else { return };
    let victim = match victim {
        Actor::Player(v) => v,
        Actor::Entity(ent) => {
            if conf::get().claims && !claims::entity_hit(&attacker, &ent) {
                e.set_cancelled(true);
            }
            return;
        }
    };
    let (au, vu) = (attacker.get_unique_id(), victim.get_unique_id());
    if au == vu {
        return;
    }
    let s = conf::get();
    if s.combat {
        if protected(&vu) {
            e.set_cancelled(true);
            ui::deny(&attacker, &format!("{} is a new player and protected from PvP for {}.", victim.get_name(), ui::duration(protection_left(&vu))));
            return;
        }
        if protected(&au) {
            e.set_cancelled(true);
            ui::deny(&attacker, "You're protected from PvP while new. Type /pvp enable to fight.");
            return;
        }
    }
    if teams::friendly_fire(&au, &vu) == Some(false) {
        e.set_cancelled(true);
        ui::deny(&attacker, &format!("{} is on your team.", victim.get_name()));
        return;
    }
    if s.claims {
        for l in [victim.get_location(), attacker.get_location()] {
            if let Some(c) = claims::claim_at_loc(&l)
                && !c.pvp
            {
                e.set_cancelled(true);
                ui::deny(&attacker, "PvP is off in this claim.");
                return;
            }
        }
    }
    if s.combat {
        tag(&victim, &attacker);
    }
}

fn on_command(e: &mut PlayerCommandPreprocessEvent) {
    let p = e.get_player().clone();
    let uuid = p.get_unique_id();
    if !tagged(&uuid) || p.has_permission("horizonsmp.combat.bypass") {
        return;
    }
    let line = e.get_message().trim_start_matches('/').to_ascii_lowercase();
    let words: Vec<&str> = line.split_whitespace().collect();
    let Some(first) = words.first() else { return };
    let first = first.rsplit(':').next().unwrap_or(first);
    let two = words.get(1).map(|w| format!("{first} {w}"));
    let blocked = conf::get().blocked_commands.iter().any(|b| b == first || two.as_deref() == Some(b.as_str()));
    if blocked {
        e.set_cancelled(true);
        let left = tags(|m| m.get(&uuid).map(|g| g.until.saturating_sub(now()) / 1000 + 1)).unwrap_or(0);
        ui::deny(&p, &format!("You can't use that in combat ({left}s left)."));
    }
}

/// Every half second: the combat bar, and "out of combat" when it ends.
pub fn tick() {
    let t = now();
    let secs = conf::get().tag_seconds.max(1);
    let list: Vec<(String, Tag)> = tags(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
    for (uuid, g) in list {
        let Some(p) = horizon_bukkit::fam_ess::get_player_by_id(&uuid) else {
            tags(|m| m.remove(&uuid));
            continue;
        };
        if g.until > t {
            let left = (g.until - t) as f64 / 1000.0;
            ui::bar(&p, &format!("<#f87171><bold>⚔ COMBAT</bold> {} <white>{:.0}s</white> <#71717a>• {}", ui::meter(left / secs as f64, 16, "#f87171", "#3f3f46"), left.ceil(), g.by_name));
        } else if !g.shown_free {
            ui::bar(&p, "<#4ade80>✔ You are no longer in combat.");
            ui::sound(&p, "BLOCK_NOTE_BLOCK_CHIME:0.5:1.4");
            tags(|m| m.remove(&uuid));
        }
    }
}

pub fn on_quit(p: &Player) {
    let uuid = p.get_unique_id();
    let tag = tags(|m| m.remove(&uuid));
    let Some(tag) = tag else { return };
    if tag.until <= now() || !conf::get().kill_on_logout || p.has_permission("horizonsmp.combat.bypass") {
        return;
    }
    // The death (drops) happens as they leave; the death event counts the death, the kill is credited here.
    if let Some(k) = horizon_bukkit::fam_ess::get_player_by_id(&tag.by) {
        data::edit(&tag.by, |d| d.kills += 1);
        bounty::on_kill(&k, p);
    }
    p.set_health(0.0);
    horizon_bukkit::server::send_rich_message(&format!("<#f87171><bold>⚔</bold> <white>{}</white> <#fca5a5>logged out during combat and was killed.", p.get_name()));
}

fn record_kill(killer: &str, victim: &Player) {
    let vu = victim.get_unique_id();
    data::edit(&vu, |d| d.deaths += 1);
    if let Some(k) = horizon_bukkit::fam_ess::get_player_by_id(killer) {
        data::edit(killer, |d| d.kills += 1);
        bounty::on_kill(&k, victim);
    }
}

fn on_death(e: &mut PlayerDeathEvent) {
    let victim = e.get_player().clone();
    let vu = victim.get_unique_id();
    let t = now();
    let tag = tags(|m| m.remove(&vu));
    match tag {
        Some(g) if g.until + 2000 > t => {
            tags(|m| {
                if let Some(other) = m.get_mut(&g.by) {
                    // The fight is over for the killer too, but keep them tagged a moment (third parties).
                    other.until = other.until.min(t + 3000);
                }
            });
            record_kill(&g.by, &victim);
        }
        _ => data::edit(&vu, |d| d.deaths += 1),
    }
}

fn on_mob_death(e: &mut EntityDeathEvent) {
    let ent = e.get_entity();
    if ent.get_type().to_ascii_uppercase().ends_with("PLAYER") {
        return;
    }
    if let Some(k) = ent.get_killer() {
        data::edit(&k.get_unique_id(), |d| d.mob_kills += 1);
    }
}

pub fn register(plugin: Plugin) {
    plugin.register_event_with("HIGH", true, on_damage);
    plugin.register_event_with("HIGH", false, on_command);
    plugin.register_event_with("MONITOR", false, on_death);
    plugin.register_event_with("MONITOR", true, on_mob_death);
}

pub fn pvp_cmd(sender: &CommandSender, args: &[&str]) -> bool {
    let Some(p) = sender.as_player() else { return true };
    let p = p.clone();
    let uuid = p.get_unique_id();
    match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("enable" | "on") => {
            if !protected(&uuid) {
                ui::say(&p, SECTION, "You aren't protected: PvP is already on for you.");
                return true;
            }
            crate::claims::confirm(&p, "Turn off your PvP protection?", |p| {
                data::edit(&p.get_unique_id(), |d| d.pvp_protection_off = true);
                ui::say(p, SECTION, "<#f87171>Your protection is off.</#f87171> Other players can now fight you, and you them.");
                ui::sound(p, "ENTITY_ENDER_DRAGON_GROWL:0.4:1.4");
            });
        }
        _ => {
            if protected(&uuid) {
                ui::say(&p, SECTION, &format!("<#4ade80>You're protected from PvP</#4ade80> for another <white>{}</white> of play. <click:run_command:/pvp enable><hover:show_text:'Fight other players now'><#f87171><u>Turn it off</u></#f87171></hover></click>", ui::duration(protection_left(&uuid))));
            } else {
                ui::say(&p, SECTION, "PvP is <#f87171>on</#f87171> for you in the wild. Claims with PvP off and teammates are safe.");
            }
        }
    }
    true
}
