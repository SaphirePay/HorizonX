//! A sampling heap profiler (the `heap-profile` feature, a diagnostic build only): which code holds the
//! process's memory. One allocation in every `RATE` bytes allocated on a thread is sampled with its call
//! stack (`RtlCaptureStackBackTrace`, which allocates nothing), and counts `RATE` bytes until it is freed;
//! an allocation of `RATE` or more is always sampled, at its own size. Every 15 seconds the live samples,
//! added up by stack, go to `heap-profile.txt` in the working directory: one line a stack, the estimated
//! bytes, the live samples, then the return addresses. `tools/v2/heap.py` names them while the process runs.
//!
//! The system heap does the allocating (the Segment Heap with HorizonsX's manifest), as in HorizonsX's normal build.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

/// Bytes allocated between two samples, per thread.
const RATE: usize = 256 * 1024;
const FRAMES: usize = 24;
/// Frames of the profiler itself and the allocator glue, skipped.
const SKIP: u32 = 2;

thread_local! {
    /// Bytes this thread allocated since its last sample.
    static SINCE: Cell<usize> = const { Cell::new(0) };
    /// Inside the profiler: its own allocations aren't sampled.
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

struct Stack {
    frames: [usize; FRAMES],
    depth: usize,
    bytes: usize,
    count: usize,
}

struct State {
    /// A live sampled allocation: its stack's hash and the bytes it stands for.
    live: HashMap<usize, (u64, usize)>,
    stacks: HashMap<u64, Stack>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
/// Which addresses may be sampled ones: a free checks this before it takes the lock.
static MAYBE: [AtomicU64; 1 << 14] = [const { AtomicU64::new(0) }; 1 << 14];
static SAMPLED_BYTES: AtomicUsize = AtomicUsize::new(0);

fn bit(ptr: usize) -> (usize, u64) {
    let h = (ptr >> 4).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 44;
    ((h >> 6) & ((1 << 14) - 1), 1u64 << (h & 63))
}

#[cfg(windows)]
fn capture(frames: &mut [usize; FRAMES]) -> (usize, u64) {
    let mut hash = 0u32;
    // SAFETY: writes at most FRAMES pointers into `frames`; allocates nothing.
    let depth = unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(SKIP, FRAMES as u32, frames.as_mut_ptr().cast(), &mut hash)
    } as usize;
    let mut full = u64::from(hash) ^ (depth as u64) << 40;
    for frame in &frames[..depth] {
        full = (full ^ *frame as u64).wrapping_mul(0x100_0000_01B3);
    }
    (depth, full)
}

#[cfg(not(windows))]
fn capture(_frames: &mut [usize; FRAMES]) -> (usize, u64) {
    (0, 0)
}

fn record(ptr: usize, size: usize) {
    let weight = if size >= RATE {
        size
    } else {
        let since = SINCE.with(|s| s.get()) + size;
        if since < RATE {
            SINCE.with(|s| s.set(since));
            return;
        }
        SINCE.with(|s| s.set(since - RATE));
        RATE
    };
    BUSY.with(|b| b.set(true));
    let mut frames = [0usize; FRAMES];
    let (depth, hash) = capture(&mut frames);
    {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let state = state.get_or_insert_with(|| State { live: HashMap::new(), stacks: HashMap::new() });
        let stack = state.stacks.entry(hash).or_insert(Stack { frames, depth, bytes: 0, count: 0 });
        stack.bytes += weight;
        stack.count += 1;
        state.live.insert(ptr, (hash, weight));
    }
    let (word, mask) = bit(ptr);
    MAYBE[word].fetch_or(mask, Relaxed);
    SAMPLED_BYTES.fetch_add(weight, Relaxed);
    BUSY.with(|b| b.set(false));
}

fn forget(ptr: usize) {
    let (word, mask) = bit(ptr);
    if MAYBE[word].load(Relaxed) & mask == 0 {
        return;
    }
    BUSY.with(|b| b.set(true));
    {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = state.as_mut() {
            if let Some((hash, weight)) = state.live.remove(&ptr) {
                if let Some(stack) = state.stacks.get_mut(&hash) {
                    stack.bytes -= weight;
                    stack.count -= 1;
                }
                SAMPLED_BYTES.fetch_sub(weight, Relaxed);
            }
        }
    }
    BUSY.with(|b| b.set(false));
}

/// The global allocator of a heap-profile build.
pub struct Profiling;

// SAFETY: forwards every call to the system allocator; the bookkeeping only reads the pointers.
unsafe impl GlobalAlloc for Profiling {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() && !BUSY.with(|b| b.get()) {
            record(p as usize, layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() && !BUSY.with(|b| b.get()) {
            record(p as usize, layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if !BUSY.with(|b| b.get()) {
            forget(ptr as usize);
        }
        unsafe { System.dealloc(ptr, layout) };
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        if !BUSY.with(|b| b.get()) {
            forget(ptr as usize);
        }
        let p = unsafe { System.realloc(ptr, layout, new) };
        if !p.is_null() && !BUSY.with(|b| b.get()) {
            record(p as usize, new);
        }
        p
    }
}

/// Writes the live samples by stack to `heap-profile.txt`, every 15 seconds, from a thread of its own.
pub fn start_dumps() {
    let _ = std::thread::Builder::new().name("heap profile".into()).spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(15));
            dump();
        }
    });
}

fn dump() {
    BUSY.with(|b| b.set(true));
    let mut rows: Vec<(usize, usize, Vec<usize>)> = {
        let state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        state.as_ref().map_or_else(Vec::new, |state| {
            state.stacks.values().filter(|s| s.bytes > 0).map(|s| (s.bytes, s.count, s.frames[..s.depth].to_vec())).collect()
        })
    };
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = format!("# sampled live bytes {} (one sample per {} bytes), pid {}\n", SAMPLED_BYTES.load(Relaxed), RATE, std::process::id());
    for (bytes, count, frames) in rows.iter().take(400) {
        out.push_str(&format!("{bytes} {count}"));
        for frame in frames {
            out.push_str(&format!(" {frame:x}"));
        }
        out.push('\n');
    }
    let _ = std::fs::write("heap-profile.tmp", out);
    let _ = std::fs::rename("heap-profile.tmp", "heap-profile.txt");
    BUSY.with(|b| b.set(false));
}
