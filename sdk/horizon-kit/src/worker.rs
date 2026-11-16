//! A plugin's per-tick work on a thread of its own, as Java plugins run theirs async: the server thread only
//! wakes it (`wake()` from a 1-tick timer), so its cost doesn't count against the server's tick.
//!
//! Rules for the work (the Rust SDK has no off-thread hop: a game query from this thread fails):
//! - read what it needs from the game on the server thread first (players, their places, placeholder values) and
//!   hand it over (a `Mutex<Option<..>>` inbox the tick takes);
//! - send packets with `kit::packets` (bytes go straight out; anything needing the game waits for the next
//!   server-thread `packets::flush()`, in order);
//! - `stop()` in `on_disable`, before the plugin's code goes away (a hot reload).

use std::sync::{Condvar, Mutex};

pub struct Worker {
    wake: Mutex<(u64, bool)>,
    signal: Condvar,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Worker {
    pub const fn new() -> Self {
        Self { wake: Mutex::new((0, false)), signal: Condvar::new(), handle: Mutex::new(None) }
    }

    /// The thread, running `tick` once per `wake()` (ticks missed while it was busy are skipped, not queued).
    pub fn start(&'static self, name: &str, tick: fn()) {
        *self.wake.lock().unwrap_or_else(|e| e.into_inner()) = (0, false);
        let handle = std::thread::Builder::new()
            .name(name.to_owned())
            .stack_size(16 << 20)
            .spawn(move || {
                crate::packets::set_worker();
                loop {
                    {
                        let mut state = self.wake.lock().unwrap_or_else(|e| e.into_inner());
                        while state.0 == 0 && !state.1 {
                            state = self.signal.wait(state).unwrap_or_else(|e| e.into_inner());
                        }
                        if state.1 {
                            return;
                        }
                        state.0 = 0;
                    }
                    let started = std::time::Instant::now();
                    tick();
                    crate::packets::flush();
                    crate::stats::time("worker tick", started.elapsed());
                }
            })
            .ok();
        *self.handle.lock().unwrap_or_else(|e| e.into_inner()) = handle;
    }

    pub fn wake(&self) {
        self.wake.lock().unwrap_or_else(|e| e.into_inner()).0 += 1;
        self.signal.notify_one();
    }

    /// Ended and joined (call in `on_disable`).
    pub fn stop(&self) {
        self.wake.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
        self.signal.notify_one();
        if let Some(h) = self.handle.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = h.join();
        }
    }
}

impl Default for Worker {
    fn default() -> Self {
        Self::new()
    }
}
