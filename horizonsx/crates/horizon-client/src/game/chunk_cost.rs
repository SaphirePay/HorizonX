//! HorizonsX: what one chunk costs the server's tick, for `/tps`'s "this chunk" line. A plugin asks for a chunk
//! (`watch`); from then on, for a minute after the last ask, its mobs' and block entities' ticks are timed on
//! whatever thread ticks them (the server's or a region's) and the total of each tick kept for the last hundred
//! ticks. Nothing is timed while no chunk is watched: one relaxed load per mob and block entity.
use super::Dim;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// How long a chunk stays watched after its last ask.
const WATCH_FOR: Duration = Duration::from_secs(60);
/// The ticks a chunk's average is taken over.
const HISTORY: usize = 100;

struct Watch {
    dim: Dim,
    chunk: (i32, i32),
    /// Nanoseconds this tick, added to from any thread.
    ns: AtomicU64,
    /// (the last ticks' nanoseconds, the last ask).
    kept: Mutex<(VecDeque<u64>, Instant)>,
}

static ON: AtomicBool = AtomicBool::new(false);
static WATCHED: RwLock<Vec<Arc<Watch>>> = RwLock::new(Vec::new());

fn find(dim: Dim, chunk: (i32, i32)) -> Option<Arc<Watch>> {
    let list = WATCHED.read().unwrap_or_else(|e| e.into_inner());
    list.iter().find(|w| w.dim == dim && w.chunk == chunk).cloned()
}

/// `f`, timed into the chunk of block (`x`, `z`) when that chunk is watched.
#[inline]
pub(super) fn timed<T>(dim: Dim, x: i32, z: i32, f: impl FnOnce() -> T) -> T {
    if !ON.load(Ordering::Relaxed) {
        return f();
    }
    let Some(watch) = find(dim, (x >> 4, z >> 4)) else {
        return f();
    };
    let started = Instant::now();
    let out = f();
    watch.ns.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
    out
}

/// A timing that ends when dropped (`start`): for loop bodies the plugin layer's rows patch line by line.
pub(super) struct Timer(Option<(Arc<Watch>, Instant)>);
impl Drop for Timer {
    fn drop(&mut self) {
        if let Some((watch, started)) = self.0.take() {
            watch.ns.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }
    }
}

thread_local! {
    /// The level a thread with no game at hand (a unit of mobs on a helper) is ticking (`in_level`).
    static LEVEL: std::cell::Cell<Dim> = const { std::cell::Cell::new(Dim::Overworld) };
}

/// `f` with this thread's level set to `dim`, for the timings inside it that have no game (`start_here`).
pub(super) fn in_level<T>(dim: Dim, f: impl FnOnce() -> T) -> T {
    let was = LEVEL.with(|l| l.replace(dim));
    let out = f();
    LEVEL.with(|l| l.set(was));
    out
}

/// A timing into the chunk of block (`x`, `z`) of `dim`, until the returned timer drops.
#[inline]
pub(super) fn start(dim: Dim, x: i32, z: i32) -> Timer {
    if !ON.load(Ordering::Relaxed) {
        return Timer(None);
    }
    Timer(find(dim, (x >> 4, z >> 4)).map(|w| (w, Instant::now())))
}

/// `start` in this thread's level (`in_level`).
#[inline]
pub(super) fn start_here(x: i32, z: i32) -> Timer {
    if !ON.load(Ordering::Relaxed) {
        return Timer(None);
    }
    start(LEVEL.with(std::cell::Cell::get), x, z)
}

/// The end of a server tick: each watched chunk's time goes into its history; chunks nobody asked about for a
/// minute are let go.
pub(super) fn tick_ends() {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    let mut list = WATCHED.write().unwrap_or_else(|e| e.into_inner());
    list.retain(|w| {
        let ns = w.ns.swap(0, Ordering::Relaxed);
        let mut kept = w.kept.lock().unwrap_or_else(|e| e.into_inner());
        if kept.0.len() == HISTORY {
            kept.0.pop_front();
        }
        kept.0.push_back(ns);
        kept.1.elapsed() < WATCH_FOR
    });
    ON.store(!list.is_empty(), Ordering::Relaxed);
}

/// What chunk (`x`, `z`) of `dim` cost the last ticks (it is watched from now on, or a minute more):
/// (average ms a tick, the most in one tick, the ticks measured). `None` the first time it is asked about.
pub(super) fn ask(dim: Dim, x: i32, z: i32) -> Option<(f64, f64, usize)> {
    let watch = match find(dim, (x, z)) {
        Some(w) => w,
        None => {
            let w = Arc::new(Watch {
                dim,
                chunk: (x, z),
                ns: AtomicU64::new(0),
                kept: Mutex::new((VecDeque::with_capacity(HISTORY), Instant::now())),
            });
            WATCHED.write().unwrap_or_else(|e| e.into_inner()).push(Arc::clone(&w));
            ON.store(true, Ordering::Relaxed);
            w
        }
    };
    let mut kept = watch.kept.lock().unwrap_or_else(|e| e.into_inner());
    kept.1 = Instant::now();
    if kept.0.is_empty() {
        return None;
    }
    let total: u64 = kept.0.iter().sum();
    let most = kept.0.iter().copied().max().unwrap_or(0);
    let n = kept.0.len();
    Some((total as f64 / n as f64 / 1e6, most as f64 / 1e6, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watched_chunk_keeps_its_ticks() {
        assert!(ask(Dim::Overworld, 1000, 1000).is_none());
        timed(Dim::Overworld, 16_000 + 3, 16_000 + 5, || std::thread::sleep(Duration::from_millis(2)));
        // Another chunk and another level are not counted.
        timed(Dim::Overworld, 0, 0, || std::thread::sleep(Duration::from_millis(5)));
        timed(Dim::Nether, 16_000, 16_000, || std::thread::sleep(Duration::from_millis(5)));
        tick_ends();
        let (avg, most, n) = ask(Dim::Overworld, 1000, 1000).expect("measured");
        assert_eq!(n, 1);
        assert!((2.0..5.0).contains(&avg), "{avg}");
        assert!(most >= avg);
    }
}
