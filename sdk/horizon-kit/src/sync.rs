//! Getting back to the server thread, and keeping an empty server ticking while work is out.
//!
//! - [`on_main`]: run a closure on the server thread at the start of the next tick (from an HTTP thread, a
//!   database thread, the chat thread). Call [`drain`] from a 1-tick timer (`plugin.run_task_timer(1, 1,
//!   horizon_kit::sync::drain)`); with nothing queued it is one atomic load. Tebex's pattern, shared.
//! - [`hold`]: an empty server pauses its ticks, so timers (and `on_main` jobs) stop. While a request or a task
//!   is out, hold pausing off; [`release`] when done. Counted: several holders can overlap.
//! - [`Cooldowns`]: per key (player, player+thing) "may this run yet?", in milliseconds.

use horizon_bukkit::Plugin;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

type Job = Box<dyn FnOnce() + Send>;
static JOBS: Mutex<Vec<Job>> = Mutex::new(Vec::new());
static PENDING: AtomicBool = AtomicBool::new(false);

/// Runs `f` on the server thread at the start of the next tick ([`drain`] runs it).
pub fn on_main(f: impl FnOnce() + Send + 'static) {
    JOBS.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(f));
    PENDING.store(true, Ordering::Release);
}

/// The queued jobs, run now (from a 1-tick timer on the server thread).
pub fn drain() {
    if !PENDING.swap(false, Ordering::AcqRel) {
        return;
    }
    let jobs = std::mem::take(&mut *JOBS.lock().unwrap_or_else(|e| e.into_inner()));
    for job in jobs {
        job();
    }
}

/// Forget queued jobs (on_disable: they hold closures from the DLL being unloaded).
pub fn clear() {
    JOBS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    PENDING.store(false, Ordering::Release);
}

static HOLDERS: AtomicUsize = AtomicUsize::new(0);

/// Keeps the server ticking while it's empty (Paper's `allowPausing(false)`), until [`release`].
pub fn hold(plugin: &Plugin) {
    if HOLDERS.fetch_add(1, Ordering::AcqRel) == 0 {
        horizon_bukkit::fam_core::allow_pausing(plugin, false);
    }
}

/// One [`hold`] done; the last lets an empty server pause again.
pub fn release(plugin: &Plugin) {
    let before = HOLDERS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| Some(n.saturating_sub(1))).unwrap_or(0);
    if before == 1 {
        horizon_bukkit::fam_core::allow_pausing(plugin, true);
    }
}

/// Every hold let go (on_disable).
pub fn release_all(plugin: &Plugin) {
    if HOLDERS.swap(0, Ordering::AcqRel) > 0 {
        horizon_bukkit::fam_core::allow_pausing(plugin, true);
    }
}

/// Cooldowns by key: `check(key, ms)` says how long is left, `start(key)` begins one. Old keys are dropped as
/// they're seen expired, so the map stays small.
#[derive(Default)]
pub struct Cooldowns {
    until: Mutex<Option<HashMap<String, u64>>>,
}

impl Cooldowns {
    pub const fn new() -> Self {
        Self { until: Mutex::new(None) }
    }
    /// Milliseconds left on `key`, 0 when it may run.
    pub fn left(&self, key: &str) -> u64 {
        let now = crate::time::now_ms();
        let mut held = self.until.lock().unwrap_or_else(|e| e.into_inner());
        let m = held.get_or_insert_with(HashMap::new);
        match m.get(key).copied() {
            Some(t) if t > now => t - now,
            Some(_) => {
                m.remove(key);
                0
            }
            None => 0,
        }
    }
    /// Starts `ms` on `key`.
    pub fn start(&self, key: &str, ms: u64) {
        let until = crate::time::now_ms() + ms;
        self.until.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(key.to_owned(), until);
    }
    /// `left` and, when 0, `start`: true = go ahead.
    pub fn try_start(&self, key: &str, ms: u64) -> bool {
        if self.left(key) > 0 {
            return false;
        }
        self.start(key, ms);
        true
    }
    pub fn clear(&self, key: &str) {
        if let Some(m) = self.until.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            m.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_and_cooldowns() {
        static RAN: AtomicUsize = AtomicUsize::new(0);
        on_main(|| {
            RAN.fetch_add(1, Ordering::Relaxed);
        });
        drain();
        drain();
        assert_eq!(RAN.load(Ordering::Relaxed), 1);
        let c = Cooldowns::new();
        assert!(c.try_start("a", 10_000));
        assert!(!c.try_start("a", 10_000));
        assert!(c.left("a") > 9000);
        assert!(c.try_start("b", 0));
    }
}
