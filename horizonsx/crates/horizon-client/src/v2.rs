//! HorizonsX's switches: the server for many players (docs/v2/README.md).
use std::{cell::RefCell, time::Instant};

/// Whether the regions run (natural spawning, mob ticks and chunk encoding on several cores):
/// on unless `HORIZON_REGIONS=0`.
pub fn regions_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_REGIONS").map_or(true, |v| v.trim() != "0"))
}

/// Item entities merge with others this many blocks away: Spigot's and Paper's 2.5 (vanilla's 0.5
/// with `HORIZON_ITEM_MERGE=0.5`). Fewer, larger stacks: fewer entities to tick and track.
pub fn item_merge_radius() -> f64 {
    static R: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *R.get_or_init(|| {
        std::env::var("HORIZON_ITEM_MERGE")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .filter(|r: &f64| (0.0..=16.0).contains(r))
            .unwrap_or(2.5)
    })
}

/// Whether the tick's worker threads (this pool, chunk encoding) run at the tick thread's raised priority:
/// `HORIZON_FAVOUR_WORKERS=1`. Off by default.
pub fn favour_workers() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_FAVOUR_WORKERS").is_ok_and(|v| v.trim() == "1"))
}

/// The pool's size: the cores but one (the tick's own), or `HORIZON_WORKERS`.
fn pool_size() -> usize {
    static SIZE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *SIZE.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get()).saturating_sub(1);
        std::env::var("HORIZON_WORKERS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(cores)
            .max(1)
    })
}

/// The worker threads every parallel phase runs on: started once and parked between ticks.
/// Starting a phase's threads anew each tick cost more than the cores gave (measured at 75
/// players: 15 threads started per phase took twice the time of 4).
pub fn pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(pool_size())
            .thread_name(|i| format!("horizonsx-{i}"))
            // They are tick threads (Folia's): the tick waits for them, so they run ahead of world
            // generation and lighting, as the tick's own thread does (above normal on Windows; on Linux
            // those helpers go behind instead).
            // Only with HORIZON_FAVOUR_WORKERS=1: measured 11-04, raised workers spinning beside the tick's
            // thread slowed every phase of it (100 spread players: 50 -> 58 ms).
            .start_handler(|_| {
                if favour_workers() {
                    horizon_threads::favour_current();
                }
            })
            .build()
            .expect("the HorizonsX worker threads start")
    })
}

/// HorizonsX: the threads the parallel phases share their work with ([`claim`]): the caller never waits for one
/// that has not started (horizon_threads::share). `HORIZON_SHARE_POOL=0`: rayon's scoped [`pool`] as before.
fn share_pool() -> &'static horizon_threads::share::SharePool {
    static POOL: std::sync::OnceLock<horizon_threads::share::SharePool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        horizon_threads::share::SharePool::new(pool_size(), |i| format!("horizonsx-s{i}"), || {
            if favour_workers() {
                horizon_threads::favour_current();
            }
        })
    })
}

fn share_pool_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_SHARE_POOL").map_or(true, |v| v.trim() != "0"))
}

/// The threads regions tick on (Folia's tick threads), apart from [`pool`]: a region's tick is long and may run
/// past its tick (lag isolation), and on the shared pool a late region held its threads while the server thread's
/// own parallel phases waited behind it for a helper to start (one region's lag in every player's tick: a 418 ms
/// entity-state phase with 125 players). Here the pool runs only short jobs. Its size: the cores but one, or
/// `HORIZON_REGION_WORKERS`.
pub fn region_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let size = std::env::var("HORIZON_REGION_WORKERS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or_else(pool_size)
            .max(1);
        rayon::ThreadPoolBuilder::new()
            .num_threads(size)
            .thread_name(|i| format!("horizonsx-region-{i}"))
            .build()
            .expect("the HorizonsX region threads start")
    })
}

thread_local! {
    /// Whether this thread is ticking a region now (`as_region`).
    static IN_REGION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
/// Regions ticking right now, on any thread.
static REGIONS_RUNNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// `f` as a region's tick: while it runs, the region's parallel phases ([`map_batches`], [`map_here_and_pool`])
/// run on this thread alone as long as another region is ticking too. Folia's regions never split their own
/// work: the regions already fill the cores, and a region that handed its phases to helpers waited for them
/// behind every other region's (measured 11-06 at 125 players, one a home: 60% of the region threads' time was
/// that wait, mob spawning's most). With one region left, its phases use the helpers again.
/// `HORIZON_REGION_NESTED=1` keeps the old way (helpers always).
pub fn as_region<R>(f: impl FnOnce() -> R) -> R {
    use std::sync::atomic::Ordering::Relaxed;
    // With several regions ticking the cores are busy: the share pool's helpers sleep between jobs.
    if REGIONS_RUNNING.fetch_add(1, Relaxed) + 1 >= busy_regions() {
        horizon_threads::share::set_cores_busy(true);
    }
    let was = IN_REGION.with(|r| r.replace(true));
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            IN_REGION.with(|r| r.set(self.0));
            if REGIONS_RUNNING.fetch_sub(1, std::sync::atomic::Ordering::Relaxed) - 1 < busy_regions() {
                horizon_threads::share::set_cores_busy(false);
            }
        }
    }
    let _reset = Reset(was);
    f()
}

/// Regions ticking at once from which the cores count as busy (the share pool's helpers stop spinning):
/// half the region threads, at least 2.
static BUSY_REGIONS_CELL: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
fn busy_regions() -> usize {
    *BUSY_REGIONS_CELL.get_or_init(|| (pool_size() / 2).max(2))
}

/// The server's choice for [`alone_in_region`] when `HORIZON_REGION_NESTED` is unset ([`set_region_nested`]).
static NESTED_SETTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Whether a region's phases may hand work to the shared helpers while other regions tick (on by default; the
/// one-clock server turns it off before its first tick).
pub fn set_region_nested(on: bool) {
    NESTED_SETTING.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether a region's phases may use the shared helpers while other regions tick (`HORIZON_REGION_NESTED`, else the
/// server's choice, [`set_region_nested`]).
pub fn region_nested_on() -> bool {
    match std::env::var("HORIZON_REGION_NESTED") {
        Ok(v) => v.trim() == "1",
        Err(_) => share_pool_on() && NESTED_SETTING.load(std::sync::atomic::Ordering::Relaxed),
    }
}

/// Whether a parallel phase here runs on this thread alone (`as_region`).
fn alone_in_region() -> bool {
    static NESTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    // HorizonsX: with the share pool a region's phases use the helpers even while other regions tick: its
    // thread never waits for a helper that hasn't started, so the wait that made regions keep their phases to
    // themselves can't happen. A big region (dozens of players) ran alone on one core with most cores idle
    // (300 spread players: 3.9 of 16 cores busy, the slowest region 34 ms). `HORIZON_REGION_NESTED=0`: the
    // old way (serial while others tick); with `HORIZON_SHARE_POOL=0` the old way too.
    let nested = *NESTED.get_or_init(|| match std::env::var("HORIZON_REGION_NESTED") {
        Ok(v) => v.trim() == "1",
        Err(_) => share_pool_on() && NESTED_SETTING.load(std::sync::atomic::Ordering::Relaxed),
    });
    if nested {
        return false;
    }
    IN_REGION.with(std::cell::Cell::get) && REGIONS_RUNNING.load(std::sync::atomic::Ordering::Relaxed) > 1
}

/// Whether regions tick on [`region_pool`] (on, unless `HORIZON_REGION_POOL=0`: on [`pool`] with the phases, as
/// before).
pub fn region_pool_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_REGION_POOL").map_or(true, |v| v.trim() != "0"))
}

/// `f` over `items`, results in their order: the largest (by `weight`) on this thread, every other on the
/// pool's threads. The tick's thread then works instead of waiting, and a worker the system paused (a busy
/// or shared machine) holds up only its own part: with 50 players a 1.3 s tick was 55 ms of the tick
/// thread's work and the rest waiting for the pool.
///
/// Folia's tick threads take the next region off one queue; so do these: the items, largest first, are
/// claimed one at a time by whichever thread is free, the tick's thread among them. Nothing is handed to a
/// thread in advance, so a worker the system paused (world generation, another program) holds up only the
/// item it was on, and with every worker paused the tick's thread simply runs them all.
pub fn map_here_and_pool<T: Send, R: Send>(items: Vec<T>, weight: impl Fn(&T) -> usize, f: impl Fn(T) -> R + Sync) -> Vec<R> {
    if items.len() <= 1 {
        return items.into_iter().map(f).collect();
    }
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(weight(&items[i])));
    claim(items, &order, &f)
}

/// `f` over `items` in batches of `batch`, results in their order, on the tick's thread and the pool's as
/// [`map_here_and_pool`] (each thread claims the next batch when it is free). For many small items (chunks).
pub fn map_batches<T: Send, R: Send>(items: Vec<T>, batch: usize, f: impl Fn(T) -> R + Sync) -> Vec<R> {
    let batch = batch.max(1);
    if items.len() <= batch {
        return items.into_iter().map(f).collect();
    }
    let mut batches: Vec<Vec<T>> = Vec::with_capacity(items.len().div_ceil(batch));
    let mut items = items.into_iter().peekable();
    while items.peek().is_some() {
        batches.push(items.by_ref().take(batch).collect());
    }
    let order: Vec<usize> = (0..batches.len()).collect();
    claim(batches, &order, &|part: Vec<T>| part.into_iter().map(&f).collect::<Vec<R>>())
        .into_iter()
        .flatten()
        .collect()
}

/// The claiming loop of [`map_here_and_pool`]: `order` is the order items are taken in.
fn claim<T: Send, R: Send>(items: Vec<T>, order: &[usize], f: &(impl Fn(T) -> R + Sync)) -> Vec<R> {
    use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};
    let n = items.len();
    if alone_in_region() {
        // A region's phase while other regions tick: this thread does every item, in `order`, the results in
        // the items' order as below.
        let mut slots: Vec<Option<T>> = items.into_iter().map(Some).collect();
        let mut out: Vec<Option<R>> = (0..n).map(|_| None).collect();
        for &i in order {
            if let Some(item) = slots[i].take() {
                out[i] = Some(f(item));
            }
        }
        return out.into_iter().map(|r| r.expect("every part ran")).collect();
    }
    let inputs: Vec<Mutex<Option<T>>> = items.into_iter().map(|t| Mutex::new(Some(t))).collect();
    let outputs: Vec<Mutex<Option<R>>> = (0..n).map(|_| Mutex::new(None)).collect();
    let next = AtomicUsize::new(0);
    // Claims items until none are left; how many it ran.
    let work = || {
        let mut ran = 0usize;
        loop {
            let k = next.fetch_add(1, Ordering::Relaxed);
            let Some(&i) = order.get(k) else { break };
            let item = inputs[i].lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(item) = item {
                let r = f(item);
                *outputs[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                ran += 1;
            }
        }
        ran
    };
    let helpers = pool_size().min(n - 1);
    // The first item (the largest) is the tick thread's before any worker wakes: a worker that took it
    // would leave the tick thread waiting for it at the end.
    let first = next.fetch_add(1, Ordering::Relaxed);
    let item = inputs[order[first]].lock().unwrap_or_else(|e| e.into_inner()).take();
    if share_pool_on() {
        // This thread takes the first item before the rest; the helpers join as they wake, and once this
        // thread's run is over it waits only for those already working (pool_wait in the report).
        let me = std::thread::current().id();
        let first_item = std::sync::Mutex::new(item);
        let mine_done: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let by_helpers = AtomicUsize::new(0);
        share_pool().run(helpers, &|| {
            if std::thread::current().id() == me {
                if let Some(item) = first_item.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let r = f(item);
                    *outputs[order[first]].lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                }
                work();
                if crate::spans::enabled() {
                    let _ = mine_done.set(std::time::Instant::now());
                }
            } else {
                let ran = work();
                by_helpers.fetch_add(ran, Ordering::Relaxed);
            }
        });
        if claim_log_on() {
            claim_log(n, by_helpers.load(Ordering::Relaxed));
        }
        if let Some(t) = mine_done.get() {
            crate::spans::record("pool_wait", t.elapsed());
        }
        return outputs
            .into_iter()
            .map(|slot| slot.into_inner().unwrap_or_else(|e| e.into_inner()).expect("every part ran"))
            .collect();
    }
    let mut mine_done = None;
    pool().in_place_scope(|scope| {
        for _ in 0..helpers {
            scope.spawn(|_| {
                work();
            });
        }
        if let Some(item) = item {
            let r = f(item);
            *outputs[order[first]].lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
        }
        work();
        // From here this thread only waits for the helpers (pool_wait in the report).
        mine_done = crate::spans::enabled().then(std::time::Instant::now);
    });
    if let Some(t) = mine_done {
        crate::spans::record("pool_wait", t.elapsed());
    }
    outputs.into_iter().map(|slot| slot.into_inner().unwrap_or_else(|e| e.into_inner()).expect("every part ran")).collect()
}

/// `HORIZON_CLAIM_LOG=1`: every 5000 parallel phases, how many items there were and roughly how many the
/// helpers took (a diagnostic: helpers that wake late leave the tick's thread doing most of the work).
fn claim_log_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_CLAIM_LOG").is_ok_and(|v| v.trim() == "1"))
}
fn claim_log(items: usize, by_helpers: usize) {
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    static ITEMS: AtomicUsize = AtomicUsize::new(0);
    static HELPED: AtomicUsize = AtomicUsize::new(0);
    ITEMS.fetch_add(items, Relaxed);
    HELPED.fetch_add(by_helpers, Relaxed);
    if CALLS.fetch_add(1, Relaxed) % 5000 == 4999 {
        let (items, helped) = (ITEMS.swap(0, Relaxed), HELPED.swap(0, Relaxed));
        log::info!("parallel phases: {items} items over the last 5000, helpers took ~{helped} ({}%)", 100 * helped / items.max(1));
    }
}

/// How many parts to split `items` into for the pool.
pub fn workers(items: usize) -> usize {
    pool_size().min(items.max(1))
}

/// The parts of a tick that can run on several cores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Spawn,
    Mobs,
    Chunks,
    Track,
    RandomTicks,
}
const PHASES: usize = 5;

/// Every this many runs of a phase, the mode it isn't using runs once, to see if it has become
/// the faster (other programs took the cores, or let them go).
const RECHECK: u32 = 50;
/// The first runs of a mode are warm-up (caches filling): each replaces the measurement.
const WARMUP_SAMPLES: u8 = 3;

/// Each phase's measured time per unit of work (a chunk, a mob, a column) on one core and on
/// several, as a moving average (milliseconds).
#[derive(Clone, Copy, Default)]
struct Timing {
    serial: Option<f32>,
    parallel: Option<f32>,
    runs: u32,
    /// Samples taken of each mode (serial, parallel), up to WARMUP_SAMPLES.
    samples: [u8; 2],
}
impl Timing {
    fn choose(&mut self) -> bool {
        self.runs = self.runs.wrapping_add(1);
        let parallel = match (self.serial, self.parallel) {
            // Each mode is measured once first.
            (_, None) => true,
            (None, _) => false,
            (Some(s), Some(p)) => {
                let better = p <= s;
                if self.runs.is_multiple_of(RECHECK) { !better } else { better }
            }
        };
        parallel
    }
    fn record(&mut self, parallel: bool, ms: f32) {
        let count = &mut self.samples[usize::from(parallel)];
        let warming = *count < WARMUP_SAMPLES;
        *count = count.saturating_add(1);
        let slot = if parallel { &mut self.parallel } else { &mut self.serial };
        *slot = Some(match *slot {
            // A cold first run (caches filling) must not decide for the next hundred.
            _ if warming => ms,
            None => ms,
            // A faster run counts more: the slow ones are mostly waits for a busy core.
            Some(old) if ms < old => old * 0.3 + ms * 0.7,
            Some(old) => old * 0.6 + ms * 0.4,
        });
    }
}

thread_local! {
    static TIMINGS: RefCell<[Timing; PHASES]> = RefCell::new([Timing::default(); PHASES]);
}

/// A phase's run, timed: [`Run::parallel`] says which mode to use; dropping it records the time.
/// Ember's lesson ("workers by measured load"): on a busy machine the threads wait for cores, so
/// the faster mode is measured, not assumed. `HORIZON_REGIONS=0`: always one core.
pub struct Run {
    phase: Phase,
    parallel: bool,
    started: Instant,
    units: usize,
}
impl Run {
    pub fn start(phase: Phase) -> Self {
        // On the pool every parallel phase beat its serial loop (75 players, quiet machine: 46.8
        // against 58.9 ms a tick), so parallel is the default; HORIZON_PARALLEL=auto picks per
        // phase by measured time instead (for a machine shared with other heavy programs).
        static ALWAYS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let always = *ALWAYS.get_or_init(|| {
            !std::env::var("HORIZON_PARALLEL").is_ok_and(|v| v.trim() == "auto")
        });
        let parallel = regions_on()
            && (always || TIMINGS.with(|t| t.borrow_mut()[phase as usize].choose()));
        Self {
            phase,
            parallel,
            started: Instant::now(),
            units: 1,
        }
    }
    pub fn parallel(&self) -> bool {
        self.parallel
    }
    /// How much work this run had: its time is compared per unit.
    pub fn units(&mut self, units: usize) {
        self.units = units.max(1);
    }
    /// The run used the serial path after all (too little work, one region): not timed as parallel.
    pub fn fell_back(&mut self) {
        self.parallel = false;
        self.started = Instant::now();
    }
}
impl Drop for Run {
    fn drop(&mut self) {
        let ms = self.started.elapsed().as_secs_f32() * 1000.0 / self.units as f32;
        let (phase, parallel) = (self.phase, self.parallel);
        TIMINGS.with(|t| t.borrow_mut()[phase as usize].record(parallel, ms));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_faster_mode_wins_and_the_other_is_rechecked() {
        let mut t = Timing::default();
        assert!(t.choose());
        t.record(true, 10.0);
        assert!(!t.choose());
        t.record(false, 4.0);
        // Serial measured faster: serial, but every RECHECK-th run tries parallel again.
        let mut parallel_runs = 0;
        for _ in 0..(RECHECK * 3) {
            if t.choose() {
                parallel_runs += 1;
                t.record(true, 10.0);
            } else {
                t.record(false, 4.0);
            }
        }
        assert_eq!(parallel_runs, 3);
        // The cores come back: parallel becomes faster and takes over.
        for _ in 0..(RECHECK * 6) {
            if t.choose() {
                t.record(true, 1.0);
            } else {
                t.record(false, 4.0);
            }
        }
        assert!(t.choose());
    }
}
