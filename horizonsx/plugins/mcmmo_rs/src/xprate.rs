//! `/xprate`: an XP rate event (global, or for chosen skills) over the configured rates.

use crate::skills::{COUNT, Skill};
use std::sync::Mutex;

pub struct Event {
    pub global: f64,
    pub skills: [Option<f64>; COUNT],
    pub active: bool,
}

static EVENT: Mutex<Event> = Mutex::new(Event { global: 1.0, skills: [None; COUNT], active: false });

/// The multiplier a gain in `skill` gets: the configured global rate, or the event's.
pub fn rate(skill: Skill) -> f64 {
    let e = EVENT.lock().unwrap_or_else(|e| e.into_inner());
    if e.active {
        if let Some(r) = e.skills[skill.idx()] {
            return r;
        }
        return e.global;
    }
    crate::settings::get().global_mult
}

pub fn active() -> bool {
    EVENT.lock().unwrap_or_else(|e| e.into_inner()).active
}

pub fn global() -> f64 {
    let e = EVENT.lock().unwrap_or_else(|e| e.into_inner());
    if e.active { e.global } else { crate::settings::get().global_mult }
}

pub fn set_global(rate: f64) {
    let mut e = EVENT.lock().unwrap_or_else(|e| e.into_inner());
    e.global = rate;
    e.skills = [None; COUNT];
    e.active = true;
}

pub fn set_skill(skill: Skill, rate: f64) {
    let mut e = EVENT.lock().unwrap_or_else(|e| e.into_inner());
    if !e.active {
        e.global = crate::settings::get().global_mult;
    }
    e.skills[skill.idx()] = Some(rate);
    e.active = true;
}

pub fn reset() {
    let mut e = EVENT.lock().unwrap_or_else(|e| e.into_inner());
    e.active = false;
    e.skills = [None; COUNT];
    e.global = 1.0;
}
