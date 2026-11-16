//! `EssentialsTimer`: every 50 ticks, activity, mute and jail timers, and the TPS average.
use crate::prelude::*;
use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

const MAX_TIME_NS: u128 = 10_000_000;
const TICK_INTERVAL: f64 = 50.0;
/// The longest gap between two runs that is still a measurement, in microseconds: fifty ticks in 30 s is under
/// 2 TPS (a server that slow trips its watchdog first); more is a paused server or its start.
const MAX_GAP_US: f64 = 30_000_000.0;

struct State {
    online: HashSet<String>,
    history: VecDeque<f64>,
    last_poll: Instant,
    skip1: usize,
    skip2: usize,
    /// A run has set `last_poll` (the first only starts the clock).
    primed: bool,
}

pub struct EssentialsTimer {
    state: Mutex<State>,
}

impl EssentialsTimer {
    pub fn new() -> Self {
        Self { state: Mutex::new(State { online: HashSet::new(), history: VecDeque::from([20.0]), last_poll: Instant::now(), skip1: 0, skip2: 0, primed: false }) }
    }
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `run`.
    pub fn run(&self) {
        let start = Instant::now();
        let current = now_ms();
        let mut skip1 = {
            let mut state = self.state();
            let spent = (start - state.last_poll).as_micros().max(1) as f64;
            // HorizonsX: a gap no ticking server makes is not a measurement. The first run's gap holds the
            // server's start, and one after `pause-when-empty-seconds` holds the whole pause (no ticks run, so
            // fifty ticks "took" minutes): either read as ~0 TPS and pulled `/tps` down for half a minute.
            let measured = state.primed && spent <= MAX_GAP_US;
            state.primed = true;
            if measured {
                if state.history.len() > 10 {
                    state.history.pop_front();
                }
                let tps = TICK_INTERVAL * 1_000_000.0 / spent;
                if tps <= 21.0 {
                    state.history.push_back(tps);
                }
            }
            state.last_poll = start;
            state.online.clear();
            state.skip1
        };
        let mut seen = Vec::new();
        let mut count = 0;
        for user in ess().get_online_users() {
            count += 1;
            if skip1 > 0 {
                skip1 -= 1;
                continue;
            }
            if count % 10 == 0 && start.elapsed().as_nanos() > MAX_TIME_NS / 2 {
                skip1 = count - 1;
                break;
            }
            seen.push(user.get_uuid());
            user.set_last_online_activity(current);
            user.check_activity();
        }
        let mut skip2 = {
            let mut state = self.state();
            state.skip1 = skip1;
            state.online.extend(seen);
            state.skip2
        };
        let online: Vec<String> = self.state().online.iter().cloned().collect();
        let mut count = 0;
        for uuid in online {
            count += 1;
            if skip2 > 0 {
                skip2 -= 1;
                continue;
            }
            if count % 10 == 0 && start.elapsed().as_nanos() > MAX_TIME_NS {
                skip2 = count - 1;
                break;
            }
            let Some(user) = ess().get_user_by_uuid(&uuid) else {
                self.state().online.remove(&uuid);
                continue;
            };
            if user.get_last_online_activity() < current && user.get_last_online_activity() > user.get_last_logout() {
                if !user.is_hidden() {
                    user.set_last_logout(user.get_last_online_activity());
                }
                self.state().online.remove(&uuid);
                continue;
            }
            user.check_mute_timeout(current);
            user.check_jail_timeout(current);
            user.reset_invulnerability_after_teleport();
        }
        self.state().skip2 = skip2;
    }
    /// `getAverageTPS`.
    pub fn get_average_tps(&self) -> f64 {
        let state = self.state();
        state.history.iter().sum::<f64>() / state.history.len().max(1) as f64
    }
}
