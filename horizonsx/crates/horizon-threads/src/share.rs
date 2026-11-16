//! HorizonsX: a work-sharing pool where the caller never waits for a helper that has not started.
//!
//! A parallel phase hands its work (a closure that claims items until none are left) to the pool's
//! threads and runs it itself too. With a scoped pool (rayon's `in_place_scope`) the caller then waits
//! for every helper it asked for to start and return, even one that finds nothing left: a helper the
//! system had parked took milliseconds to wake, and the tick's thread waited for it with the work long
//! done (11 ms of a 47 ms tick at 500 players; 7 ms of one phase whose work took 0.08 ms).
//!
//! Here the caller, once its own run of the work is over, closes the job and waits only for the
//! helpers already inside it. A helper joins by counting itself in and only then looking whether the
//! job is still open (both sequentially consistent): either the caller sees it counted and waits for
//! it, or it sees the job closed and never touches the work. The work borrows the caller's stack,
//! which stays alive until every helper inside has left.
use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
    },
};

/// Whether the cores are busy with other work already (HorizonsX: several regions ticking at once):
/// a helper then goes to sleep as soon as it finds the queue empty instead of spinning for the next job.
/// Its spin burned a core's share that a region thread on the same core (or its SMT sibling) needed
/// (600 spread players: ~10% of the busy threads' samples were helpers spinning and reading the clock).
static CORES_BUSY: AtomicBool = AtomicBool::new(false);

/// Tells the pools' helpers whether the cores are busy with other work ([`CORES_BUSY`]).
pub fn set_cores_busy(busy: bool) {
    if CORES_BUSY.load(std::sync::atomic::Ordering::Relaxed) != busy {
        CORES_BUSY.store(busy, std::sync::atomic::Ordering::Relaxed);
    }
}

/// One parallel phase's work, as the helpers see it.
struct Job {
    /// The caller's closure, its lifetime erased: only called while `closed` is false and this
    /// helper is counted in `inside` (see the module's notes).
    work: *const (dyn Fn() + Sync + 'static),
    closed: AtomicBool,
    inside: AtomicUsize,
    /// A helper's run of the work panicked: the caller panics too once it is done.
    panicked: AtomicBool,
    /// The caller, unparked by the last helper to leave after the job closed: it sleeps for them
    /// instead of yielding in a loop (a yield on Windows returns at once with no other thread ready, so
    /// the wait held a core: pool_wait was 86 ms a tick across the regions at 600 spread players).
    waiter: std::thread::Thread,
}
// SAFETY: `work` points to a `Sync` closure, called only under the protocol above.
unsafe impl Send for Job {}
// SAFETY: as above; the other fields are atomics.
unsafe impl Sync for Job {}

struct Shared {
    queue: Mutex<VecDeque<Arc<Job>>>,
    ready: Condvar,
}

/// Threads that share the parallel phases' work.
pub struct SharePool {
    shared: Arc<Shared>,
    threads: usize,
}

impl SharePool {
    /// `threads` helpers named `name(i)`, each running `start` first.
    pub fn new(threads: usize, name: impl Fn(usize) -> String, start: impl Fn() + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
        });
        let start = Arc::new(start);
        for i in 0..threads {
            let (shared, start) = (Arc::clone(&shared), Arc::clone(&start));
            std::thread::Builder::new()
                .name(name(i))
                .spawn(move || {
                    start();
                    helper(&shared);
                })
                .expect("a share pool thread starts");
        }
        Self { shared, threads }
    }

    pub fn threads(&self) -> usize {
        self.threads
    }

    /// Run `work` here and on up to `helpers` of the pool's threads at once; returns when this
    /// thread's run is over and no helper is still inside it. `work` must be safe to run several
    /// times at once and to run with nothing left to do (a claiming loop).
    pub fn run(&self, helpers: usize, work: &(dyn Fn() + Sync)) {
        let helpers = helpers.min(self.threads);
        if helpers == 0 {
            work();
            return;
        }
        // SAFETY: only the lifetime is erased; the protocol keeps every call inside this function.
        let erased: *const (dyn Fn() + Sync + 'static) = unsafe { std::mem::transmute(work as *const (dyn Fn() + Sync)) };
        let job = Arc::new(Job {
            work: erased,
            closed: AtomicBool::new(false),
            inside: AtomicUsize::new(0),
            panicked: AtomicBool::new(false),
            waiter: std::thread::current(),
        });
        {
            let mut queue = self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
            for _ in 0..helpers {
                queue.push_back(Arc::clone(&job));
            }
        }
        for _ in 0..helpers {
            self.shared.ready.notify_one();
        }
        let mine = catch_unwind(AssertUnwindSafe(work));
        job.closed.store(true, SeqCst);
        // Only the helpers already inside are waited for: a short spin, then sleep until the last one
        // leaves (it unparks this thread; the timeout only covers a wake-up lost to some other unpark).
        let mut spins = 0u32;
        while job.inside.load(SeqCst) != 0 {
            if spins < 200 {
                std::hint::spin_loop();
                spins += 1;
            } else {
                std::thread::park_timeout(std::time::Duration::from_millis(1));
            }
        }
        // The copies no helper took yet are dropped here rather than woken for nothing later.
        {
            let mut queue = self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
            queue.retain(|j| !Arc::ptr_eq(j, &job));
        }
        if let Err(panic) = mine {
            std::panic::resume_unwind(panic);
        }
        if job.panicked.load(SeqCst) {
            panic!("a share pool helper panicked in a parallel phase");
        }
    }
}

/// How long a helper keeps looking for the next job before it sleeps (`HORIZON_SHARE_SPIN_US`, default 50 us):
/// a tick runs its parallel phases one after another, and a helper that slept between them woke too late to
/// help with the next.
fn spin_for() -> std::time::Duration {
    static SPIN: OnceLock<std::time::Duration> = OnceLock::new();
    *SPIN.get_or_init(|| {
        let us = std::env::var("HORIZON_SHARE_SPIN_US").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(50u64);
        std::time::Duration::from_micros(us)
    })
}

fn helper(shared: &Shared) {
    loop {
        // A short look before sleeping.
        let spun = if CORES_BUSY.load(std::sync::atomic::Ordering::Relaxed) {
            None
        } else {
            let started = std::time::Instant::now();
            let spin = spin_for();
            let mut found = None;
            let mut round = 0u32;
            // The clock is read every 32 looks (reading it each time was a third of the spin's cost).
            while round % 32 != 0 || started.elapsed() < spin {
                round = round.wrapping_add(1);
                if let Ok(mut queue) = shared.queue.try_lock()
                    && let Some(job) = queue.pop_front()
                {
                    found = Some(job);
                    break;
                }
                std::hint::spin_loop();
            }
            found
        };
        let job = match spun {
            Some(job) => job,
            None => {
                let mut queue = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
                loop {
                    if let Some(job) = queue.pop_front() {
                        break job;
                    }
                    queue = shared.ready.wait(queue).unwrap_or_else(PoisonError::into_inner);
                }
            }
        };
        job.inside.fetch_add(1, SeqCst);
        if !job.closed.load(SeqCst) {
            // SAFETY: counted in and the job still open: the caller waits for this call to end.
            let work = unsafe { &*job.work };
            if catch_unwind(AssertUnwindSafe(work)).is_err() {
                job.panicked.store(true, SeqCst);
            }
        }
        if job.inside.fetch_sub(1, SeqCst) == 1 && job.closed.load(SeqCst) {
            job.waiter.unpark();
        }
    }
}

/// A pool made once on first use (`size()` threads named `name(i)`, each running `start` first).
pub fn lazy(cell: &'static OnceLock<SharePool>, size: impl FnOnce() -> usize, name: fn(usize) -> String, start: fn()) -> &'static SharePool {
    cell.get_or_init(|| SharePool::new(size(), name, start))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn every_item_runs_once_and_the_caller_needs_no_helper() {
        let pool = SharePool::new(4, |i| format!("share-test-{i}"), || {});
        for round in 0..200 {
            let items: Vec<AtomicUsize> = (0..(round % 50 + 1)).map(|_| AtomicUsize::new(0)).collect();
            let next = AtomicUsize::new(0);
            pool.run(4, &|| {
                loop {
                    let k = next.fetch_add(1, SeqCst);
                    let Some(item) = items.get(k) else { break };
                    item.fetch_add(1, SeqCst);
                }
            });
            assert!(items.iter().all(|i| i.load(SeqCst) == 1), "round {round}");
        }
    }

    #[test]
    fn a_helper_panic_reaches_the_caller() {
        let pool = SharePool::new(2, |i| format!("share-panic-{i}"), || {});
        let next = AtomicUsize::new(0);
        let result = catch_unwind(AssertUnwindSafe(|| {
            pool.run(2, &|| {
                loop {
                    let k = next.fetch_add(1, SeqCst);
                    if k >= 4 {
                        break;
                    }
                    if k == 1 {
                        panic!("item one");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            })
        }));
        // Either the caller or a helper took item 1; both reach the caller.
        assert!(result.is_err());
    }
}
