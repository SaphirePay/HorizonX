//! effects.yml: sounds, titles, particles and potion effects around a teleport.

use crate::settings;
use horizon_bukkit::{Location, Player};

pub fn sound(p: &Player, name: &str) {
    let s = settings();
    if !s.effects.sounds || name.is_empty() {
        return;
    }
    p.play_sound(&p.get_location(), &name.to_ascii_uppercase(), 1.0, 1.0);
}

/// Shows the `Titles.<kind>` title (`%x% %y% %z% %attempts% %time%` filled in). Returns whether the chat message
/// for this moment is sent too (`SendMessage`, or always when titles are off).
pub fn title(p: &Player, kind: &str, at: &Location, attempts: u32, time: u32) -> bool {
    let s = settings();
    if !s.effects.titles {
        return true;
    }
    let f = &s.effects_file;
    let fill = |t: String| {
        let t = t
            .replace("%x%", &(at.x.floor() as i64).to_string())
            .replace("%y%", &(at.y.floor() as i64).to_string())
            .replace("%z%", &(at.z.floor() as i64).to_string())
            .replace("%attempts%", &attempts.to_string())
            .replace("%time%", &time.to_string())
            .replace("%player%", p.get_name());
        horizon_kit::text::colorize(&horizon_kit::hooks::placeholders(&t, Some(p)))
    };
    let title = fill(f.str(&format!("Titles.{kind}.Title"), ""));
    let sub = fill(f.str(&format!("Titles.{kind}.Subtitle"), ""));
    if !title.is_empty() || !sub.is_empty() {
        p.send_title(&title, &sub, 10, 70, 20);
    }
    f.bool(&format!("Titles.{kind}.SendMessage"), true)
}

/// ParticleLib's old names -> the game's.
fn particle_name(name: &str) -> String {
    match name.to_ascii_uppercase().as_str() {
        "EXPLOSION_NORMAL" => "POOF".into(),
        "EXPLOSION_LARGE" => "EXPLOSION".into(),
        "EXPLOSION_HUGE" => "EXPLOSION_EMITTER".into(),
        "FIREWORKS_SPARK" => "FIREWORK".into(),
        "SPELL_WITCH" => "WITCH".into(),
        "SPELL_MOB" => "ENTITY_EFFECT".into(),
        "SMOKE_NORMAL" => "SMOKE".into(),
        "SMOKE_LARGE" => "LARGE_SMOKE".into(),
        "VILLAGER_HAPPY" => "HAPPY_VILLAGER".into(),
        "REDSTONE" => "DUST".into(),
        "PORTAL" => "PORTAL".into(),
        other => other.to_owned(),
    }
}

pub fn particles(p: &Player, at: &Location) {
    let s = settings();
    if !s.effects.particles {
        return;
    }
    let shape = s.effects.particle_shape.to_ascii_uppercase();
    for kind in &s.effects.particle_types {
        let name = particle_name(kind);
        match shape.as_str() {
            "SCAN" => p.spawn_particle_extra(&name, at, 60, 0.6, 0.0, 0.6, 0.0),
            "TELEPORT" => p.spawn_particle_extra(&name, at, 60, 0.4, 1.0, 0.4, 0.05),
            _ => p.spawn_particle_extra(&name, at, 50, 0.3, 0.5, 0.3, 0.1),
        }
    }
}

pub fn potions(p: &Player) {
    let s = settings();
    for (name, dur, amp) in &s.effects.potions {
        let effect = horizon_bukkit::PotionEffect::new(name, *dur, *amp, false, true);
        p.give_potion_effect(&effect);
    }
}
