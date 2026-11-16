//! `java.util.concurrent`, `java.util.regex`, `java.time.Instant`, threads, and the libraries plugins
//! shade most: Guava's collections and Gson's tree.
use super::*;
use crate::javaio::JDuration;
use crate::stdlib::Cell;
use fancy_regex::Regex;
use std::sync::Condvar;
use std::thread::ThreadId;

// ---- Locks ----

/// `ReentrantLock`, and each side of a `ReentrantReadWriteLock`: readers wait for each other too, which
/// only costs speed.
pub struct JLock {
    state: Mutex<(Option<ThreadId>, u32)>,
    free: Condvar,
}

impl JLock {
    fn new() -> Self {
        Self { state: Mutex::new((None, 0)), free: Condvar::new() }
    }
    fn acquire(&self) {
        let me = std::thread::current().id();
        let mut state = lock(&self.state);
        while state.0.is_some_and(|owner| owner != me) {
            state = self.free.wait_timeout(state, std::time::Duration::from_millis(2)).unwrap_or_else(|e| e.into_inner()).0;
            drop(state);
            crate::pump();
            state = lock(&self.state);
        }
        state.0 = Some(me);
        state.1 += 1;
    }
    fn try_acquire(&self) -> bool {
        let me = std::thread::current().id();
        let mut state = lock(&self.state);
        if state.0.is_some_and(|owner| owner != me) {
            return false;
        }
        state.0 = Some(me);
        state.1 += 1;
        true
    }
    fn release(&self) -> Result<(), V> {
        let mut state = lock(&self.state);
        if state.0 != Some(std::thread::current().id()) {
            return Err(exc("IllegalMonitorStateException", "not the owner"));
        }
        state.1 -= 1;
        if state.1 == 0 {
            state.0 = None;
            self.free.notify_all();
        }
        Ok(())
    }
}

/// `java.util.concurrent.Semaphore`: permits that `acquire` waits for and `release` gives back (FAWE's player
/// actors and chunk sections). Waiting pumps, as a lock does, so the server thread can still lend the game.
pub struct JSemaphore {
    permits: Mutex<i64>,
    freed: Condvar,
}

fn semaphore_method(s: &JSemaphore, name: &str, a: &[V]) -> Option<R> {
    use std::time::{Duration, Instant};
    let count = |at: usize| a.get(at).filter(|v| matches!(v, V::Int(_) | V::Long(_))).map_or(1, |v| v.long());
    let take = |wanted: i64, until: Option<Instant>| -> bool {
        let mut permits = lock(&s.permits);
        loop {
            if *permits >= wanted {
                *permits -= wanted;
                return true;
            }
            if until.is_some_and(|end| Instant::now() >= end) {
                return false;
            }
            permits = s.freed.wait_timeout(permits, Duration::from_millis(2)).unwrap_or_else(|e| e.into_inner()).0;
            drop(permits);
            crate::pump();
            permits = lock(&s.permits);
        }
    };
    Some(Ok(match (name, a.len()) {
        ("acquire" | "acquireUninterruptibly", 0 | 1) => {
            take(count(0), None);
            V::Null
        }
        ("tryAcquire", 0 | 1) => V::Bool(take(count(0), Some(Instant::now()))),
        // `tryAcquire(timeout, unit)` and `tryAcquire(permits, timeout, unit)`.
        ("tryAcquire", 2 | 3) => {
            let (wanted, timeout, unit) = if a.len() == 3 { (a[0].long(), &a[1], &a[2]) } else { (1, &a[0], &a[1]) };
            let ms = call(unit, "toMillis", std::slice::from_ref(timeout)).map_or(0, |m| m.long()).max(0) as u64;
            V::Bool(take(wanted, Some(Instant::now() + Duration::from_millis(ms))))
        }
        ("release", 0 | 1) => {
            *lock(&s.permits) += count(0);
            s.freed.notify_all();
            V::Null
        }
        ("availablePermits", 0) => V::Int(*lock(&s.permits) as i32),
        ("drainPermits", 0) => {
            let mut permits = lock(&s.permits);
            let had = (*permits).max(0);
            *permits -= had;
            V::Int(had as i32)
        }
        ("isFair", 0) => V::Bool(false),
        ("hasQueuedThreads", 0) => V::Bool(false),
        ("getQueueLength", 0) => V::Int(0),
        _ => return None,
    }))
}

/// Guava's `Joiner`.
#[derive(Clone)]
pub struct JGJoiner {
    sep: String,
    skip_nulls: bool,
    for_null: Option<String>,
}

fn guava_joiner_method(j: &JGJoiner, name: &str, a: &[V]) -> Option<R> {
    let parts = |a: &[V]| -> Result<Vec<V>, V> {
        match a {
            [V::Arr(items)] | [V::List(items)] | [V::Stream(items)] => Ok(lock(items).clone()),
            [V::Set(_)] | [V::Obj(_)] | [V::Native(_)] if !a[0].is_null() => a[0].items(),
            more => Ok(more.to_vec()),
        }
    };
    let join = |items: Vec<V>| -> Result<String, V> {
        let mut out = Vec::new();
        for item in items {
            if item.is_null() {
                if j.skip_nulls {
                    continue;
                }
                match &j.for_null {
                    Some(text) => out.push(text.clone()),
                    None => return Err(exc("NullPointerException", "Joiner given a null")),
                }
            } else {
                out.push(item.text());
            }
        }
        Ok(out.join(&j.sep))
    };
    Some(match name {
        "join" => parts(a).and_then(join).map(|t| s(&t)),
        "skipNulls" => Ok(native(JGJoiner { skip_nulls: true, ..j.clone() })),
        "useForNull" => Ok(native(JGJoiner { for_null: a.first().map(V::text), ..j.clone() })),
        "appendTo" if a.len() >= 2 => parts(&a[1..]).and_then(join).and_then(|t| call(&a[0], "append", &[s(&t)])),
        _ => return None,
    })
}

/// `Stream.Builder`.
pub struct JStreamBuilder(Mutex<Vec<V>>);

/// `java.util.concurrent.FutureTask`: a Callable (or a Runnable and its result) run once by whoever calls `run()`,
/// its answer waited for by `get()` (FAWE's per-player task queue).
pub struct JFutureTask {
    work: Mutex<Option<V>>,
    result: V,
    done: Mutex<Option<R>>,
    finished: Condvar,
}

fn future_task_method(f: &JFutureTask, name: &str, a: &[V]) -> Option<R> {
    use std::time::{Duration, Instant};
    let wait = |until: Option<Instant>| -> Option<R> {
        let mut done = lock(&f.done);
        loop {
            if let Some(answer) = done.as_ref() {
                return Some(answer.clone());
            }
            if until.is_some_and(|end| Instant::now() >= end) {
                return None;
            }
            done = f.finished.wait_timeout(done, Duration::from_millis(2)).unwrap_or_else(|e| e.into_inner()).0;
            drop(done);
            crate::pump();
            done = lock(&f.done);
        }
    };
    Some(match (name, a.len()) {
        ("run", 0) => {
            let Some(work) = lock(&f.work).take() else { return Some(Ok(V::Null)) };
            let answer = if matches!(work, V::Func(_)) || !crate::instance_of(&work, "Callable") {
                call(&work, if matches!(work, V::Func(_)) { "call" } else { "run" }, &[]).map(|v| if f.result.is_null() { v } else { f.result.clone() })
            } else {
                call(&work, "call", &[])
            };
            *lock(&f.done) = Some(answer);
            f.finished.notify_all();
            Ok(V::Null)
        }
        ("get", 0) => wait(None).unwrap_or(Ok(V::Null)).map_err(|e| exc("ExecutionException", throwable_text(&e))),
        ("get", 2) => {
            let ms = call(&a[1], "toMillis", std::slice::from_ref(&a[0])).map_or(0, |m| m.long()).max(0) as u64;
            match wait(Some(Instant::now() + Duration::from_millis(ms))) {
                Some(answer) => answer.map_err(|e| exc("ExecutionException", throwable_text(&e))),
                None => Err(exc("TimeoutException", "")),
            }
        }
        ("isDone", 0) => Ok(V::Bool(lock(&f.done).is_some())),
        ("cancel", 1) => {
            let had = lock(&f.work).take().is_some();
            if had {
                *lock(&f.done) = Some(Err(exc("CancellationException", "")));
                f.finished.notify_all();
            }
            Ok(V::Bool(had))
        }
        ("isCancelled", 0) => Ok(V::Bool(matches!(lock(&f.done).as_ref(), Some(Err(e)) if throwable_text(e).contains("CancellationException")))),
        _ => return None,
    })
}

// ---- `synchronized`: a reentrant lock for each object, as Java has ----

static MONITORS: Mutex<Option<BTreeMap<usize, Arc<JLock>>>> = Mutex::new(None);

pub struct Monitor(usize, Arc<JLock>);

fn identity(value: &V) -> usize {
    match value {
        V::Obj(object) => Arc::as_ptr(object) as usize,
        V::Native(held) => Arc::as_ptr(held).cast::<()>() as usize,
        V::List(items) | V::Arr(items) | V::Stream(items) => Arc::as_ptr(items) as usize,
        V::Map(map) | V::Set(map) => Arc::as_ptr(map) as usize,
        other => other.text().bytes().fold(5381_usize, |hash, byte| hash.wrapping_mul(33).wrapping_add(usize::from(byte))),
    }
}

pub fn monitor(value: &V) -> Monitor {
    let key = identity(value);
    let held = lock(&MONITORS).get_or_insert_default().entry(key).or_insert_with(|| Arc::new(JLock::new())).clone();
    held.acquire();
    Monitor(key, held)
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.1.release();
        let mut monitors = lock(&MONITORS);
        // Nobody else is on it: a monitor lasts only as long as it's in use.
        if Arc::strong_count(&self.1) == 2 && lock(&self.1.state).0.is_none() {
            monitors.get_or_insert_default().remove(&self.0);
        }
    }
}

// ---- Futures ----

type Callback = Box<dyn FnOnce(&R) + Send>;

pub struct JFuture {
    state: Mutex<(Option<R>, Vec<Callback>)>,
    done: Condvar,
}

fn future(result: Option<R>) -> Arc<JFuture> {
    Arc::new(JFuture { state: Mutex::new((result, Vec::new())), done: Condvar::new() })
}

fn future_v(f: Arc<JFuture>) -> V {
    V::Native(f)
}

/// A CompletableFuture that Rust finishes later (an SDK callback): the value the plugin holds, and the
/// future to `complete_with` when the answer comes.
pub fn pending_future() -> (V, Arc<JFuture>) {
    let f = future(None);
    (future_v(f.clone()), f)
}

impl JFuture {
    /// Completes it, running what the plugin chained on (`thenAccept`, `whenComplete`...).
    pub fn complete_with(&self, result: R) -> bool {
        self.finish(result)
    }
}

impl JFuture {
    fn finish(&self, result: R) -> bool {
        let mut state = lock(&self.state);
        if state.0.is_some() {
            return false;
        }
        state.0 = Some(result.clone());
        let callbacks = std::mem::take(&mut state.1);
        self.done.notify_all();
        drop(state);
        for callback in callbacks {
            callback(&result);
        }
        true
    }
    fn when(self: &Arc<Self>, callback: impl FnOnce(&R) + Send + 'static) {
        let mut state = lock(&self.state);
        if let Some(result) = state.0.clone() {
            drop(state);
            callback(&result);
        } else {
            state.1.push(Box::new(callback));
        }
    }
    fn wait(&self) -> R {
        let mut state = lock(&self.state);
        while state.0.is_none() {
            state = self.done.wait_timeout(state, std::time::Duration::from_millis(2)).unwrap_or_else(|e| e.into_inner()).0;
            drop(state);
            crate::pump();
            state = lock(&self.state);
        }
        state.0.clone().expect("finished")
    }
    /// A new future with `step` applied to this one's result.
    fn then(self: &Arc<Self>, step: impl FnOnce(&R) -> R + Send + 'static) -> V {
        let next = future(None);
        let out = next.clone();
        self.when(move |result| {
            next.finish(step(result));
        });
        future_v(out)
    }
}

/// What a dependent stage completes with when what it depends on failed (or its function threw): a
/// `CompletionException` with that as its cause and `cause.toString()` as its message, as Java's
/// (`exceptionally(ex -> ex.getMessage())` after a `thenAccept` reads "a.b.FooException: why").
fn wrapped(thrown: &V) -> V {
    if matches!(thrown, V::Exc(t) if t.class == "CompletionException") {
        return thrown.clone();
    }
    V::Exc(Arc::new(Thrown { class: "CompletionException".into(), message: Some(crate::throwable_text(thrown)), cause: Some(thrown.clone()), trace: Vec::new() }))
}

/// `future.get()`'s failure: an `ExecutionException` around the real cause.
fn execution(thrown: &V) -> V {
    let cause = match thrown {
        V::Exc(t) if t.class == "CompletionException" => t.cause.clone().unwrap_or_else(|| thrown.clone()),
        other => other.clone(),
    };
    V::Exc(Arc::new(Thrown { class: "ExecutionException".into(), message: Some(crate::throwable_text(&cause)), cause: Some(cause), trace: Vec::new() }))
}

/// A dependent stage's result: a failure, its own or passed on, wrapped as Java wraps it.
fn relay(result: R) -> R {
    result.map_err(|e| wrapped(&e))
}

/// A background thread's exception, with where it came from: nobody on that thread will report it.
fn traced(result: R) -> R {
    result.map_err(|error| {
        let trace = crate::take_trace();
        if let V::Exc(thrown) = &error {
            return V::Exc(Arc::new(Thrown { class: thrown.class.clone(), message: Some(format!("{} at {trace}", thrown.message.clone().unwrap_or_default())), cause: thrown.cause.clone(), trace: thrown.trace.clone() }));
        }
        error
    })
}

fn future_method(f: &Arc<JFuture>, this: &V, name: &str, a: &[V]) -> Option<R> {
    let func = a.first().cloned().unwrap_or(V::Null);
    let call1 = |g: &V, value: V| call(g, "apply", &[value]);
    let _ = call1;
    Some(match name {
        "join" => f.wait().map_err(|e| wrapped(&e)),
        "get" => f.wait().map_err(|e| execution(&e)),
        "getNow" => Ok(lock(&f.state).0.clone().map_or_else(|| a[0].clone(), |r| r.unwrap_or(V::Null))),
        "isDone" => Ok(V::Bool(lock(&f.state).0.is_some())),
        "isCompletedExceptionally" => Ok(V::Bool(matches!(lock(&f.state).0, Some(Err(_))))),
        "complete" => Ok(V::Bool(f.finish(Ok(a[0].clone())))),
        "completeExceptionally" => Ok(V::Bool(f.finish(Err(a[0].clone())))),
        "cancel" => Ok(V::Bool(f.finish(Err(exc("CancellationException", "cancelled"))))),
        "thenApply" | "thenApplyAsync" => Ok(f.then(move |r| relay(r.clone().and_then(|v| call(&func, "apply", &[v]))))),
        "thenAccept" | "thenAcceptAsync" => Ok(f.then(move |r| relay(r.clone().and_then(|v| call(&func, "accept", &[v]).map(|_| V::Null))))),
        "thenRun" | "thenRunAsync" => Ok(f.then(move |r| relay(r.clone().and_then(|_| call(&func, "run", &[]).map(|_| V::Null))))),
        "thenCompose" | "thenComposeAsync" => Ok(f.then(move |r| relay(r.clone().and_then(|v| call(&func, "apply", &[v]).and_then(|next| call(&next, "join", &[])))))),
        // These see the failure as it was stored: the original on the future that failed, a `CompletionException`
        // on a stage after it.
        "exceptionally" => Ok(f.then(move |r| match r {
            Ok(v) => Ok(v.clone()),
            Err(e) => relay(call(&func, "apply", &[e.clone()])),
        })),
        "handle" | "handleAsync" => Ok(f.then(move |r| relay(match r {
            Ok(v) => call(&func, "apply", &[v.clone(), V::Null]),
            Err(e) => call(&func, "apply", &[V::Null, e.clone()]),
        }))),
        "whenComplete" | "whenCompleteAsync" => Ok(f.then(move |r| {
            let thrown = match r {
                Ok(v) => call(&func, "accept", &[v.clone(), V::Null]),
                Err(e) => call(&func, "accept", &[V::Null, e.clone()]),
            };
            relay(match (r, thrown) {
                (Ok(_), Err(e)) => Err(e),
                _ => r.clone(),
            })
        })),
        "toCompletableFuture" => Ok(this.clone()),
        _ => return None,
    })
}

/// Off the calling thread, as an `Executor` or `supplyAsync` would.
fn spawn(job: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().stack_size(16 << 20).spawn(job);
}

pub struct JExecutor {
    stopped: std::sync::atomic::AtomicBool,
}

fn executor_method(this: &JExecutor, name: &str, a: &[V]) -> Option<R> {
    use std::sync::atomic::Ordering;
    let task = a.first().cloned().unwrap_or(V::Null);
    let run = |task: &V| call(task, if matches!(task, V::Func(_)) { "run" } else if crate::instance_of(task, "Callable") { "call" } else { "run" }, &[]);
    Some(match name {
        "execute" => {
            spawn(move || {
                let _ = report("task", run(&task));
            });
            Ok(V::Null)
        }
        "submit" => {
            let out = future(None);
            let done = out.clone();
            spawn(move || {
                done.finish(run(&task));
            });
            Ok(future_v(out))
        }
        "schedule" => {
            let wait = a[1].long().max(0) as u64 * unit_ms(&a[2]);
            let out = future(None);
            let done = out.clone();
            spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(wait));
                done.finish(run(&task));
            });
            Ok(future_v(out))
        }
        "scheduleAtFixedRate" | "scheduleWithFixedDelay" => {
            let (first, period) = (a[1].long().max(0) as u64 * unit_ms(&a[3]), (a[2].long().max(1) as u64 * unit_ms(&a[3])).max(1));
            let out = future(None);
            let handle = out.clone();
            spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(first));
                while lock(&handle.state).0.is_none() {
                    let _ = report("task", run(&task));
                    std::thread::sleep(std::time::Duration::from_millis(period));
                }
            });
            Ok(future_v(out))
        }
        "shutdown" | "shutdownNow" | "close" => {
            this.stopped.store(true, Ordering::Release);
            Ok(if name == "shutdownNow" { list(Vec::new()) } else { V::Null })
        }
        "isShutdown" | "isTerminated" => Ok(V::Bool(this.stopped.load(Ordering::Acquire))),
        "awaitTermination" => Ok(V::Bool(true)),
        "setRemoveOnCancelPolicy" | "allowCoreThreadTimeOut" | "setKeepAliveTime" | "setCorePoolSize" | "setMaximumPoolSize" | "setThreadFactory" | "setExecuteExistingDelayedTasksAfterShutdownPolicy" | "setContinueExistingPeriodicTasksAfterShutdownPolicy" => Ok(V::Null),
        _ => return None,
    })
}

fn unit_ms(unit: &V) -> u64 {
    match unit.text().as_str() {
        "SECONDS" => 1000,
        "MINUTES" => 60_000,
        "HOURS" => 3_600_000,
        "DAYS" => 86_400_000,
        _ => 1,
    }
}

// ---- Threads ----

pub struct JThread {
    task: Mutex<Option<V>>,
    name: Mutex<String>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    interrupted: std::sync::atomic::AtomicBool,
}

fn thread_method(t: &JThread, name: &str, a: &[V]) -> Option<R> {
    use std::sync::atomic::Ordering;
    Some(Ok(match name {
        "start" => {
            if let Some(task) = lock(&t.task).take() {
                *lock(&t.handle) = Some(std::thread::spawn(move || {
                    let _ = report("thread", call(&task, "run", &[]));
                }));
            }
            V::Null
        }
        "join" => {
            if let Some(handle) = lock(&t.handle).take() {
                let _ = handle.join();
            }
            V::Null
        }
        "setName" => {
            *lock(&t.name) = a[0].text();
            V::Null
        }
        "getName" => s(&lock(&t.name)),
        "toString" => s(&format!("Thread[{}]", lock(&t.name))),
        "getId" | "threadId" => V::Long(t as *const JThread as i64),
        "setDaemon" | "setPriority" | "setUncaughtExceptionHandler" | "setContextClassLoader" => V::Null,
        "interrupt" => {
            t.interrupted.store(true, Ordering::Release);
            V::Null
        }
        "isInterrupted" => V::Bool(t.interrupted.load(Ordering::Acquire)),
        "isAlive" => V::Bool(lock(&t.handle).as_ref().is_some_and(|h| !h.is_finished())),
        "getStackTrace" => arr(Vec::new()),
        _ => return None,
    }))
}

pub struct JThreadLocal {
    values: Mutex<BTreeMap<String, V>>,
    initial: Option<V>,
}

// ---- Regular expressions ----

pub struct JPattern {
    pub text: String,
    pub regex: Regex,
}

/// `Class.getPackage()`: a plugin converted to Rust has no packages left to name.
pub struct JPackage;

pub struct JMatcher {
    regex: Regex,
    input: String,
    at: Mutex<usize>,
    groups: Mutex<Vec<Option<(usize, usize)>>>,
    /// Where `appendReplacement` stopped: the next append starts there.
    appended: Mutex<usize>,
}

/// A Java replacement string against the current match: `$1`, `${name}`, `\$`.
fn expand(m: &JMatcher, with: &str) -> Result<String, V> {
    let groups = lock(&m.groups).clone();
    let text = |at: usize| groups.get(at).copied().flatten().map_or(String::new(), |(from, to)| m.input[from..to].to_owned());
    let mut out = String::new();
    let mut chars = with.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '$' if chars.peek() == Some(&'{') => {
                chars.next();
                let name: String = chars.by_ref().take_while(|c| *c != '}').collect();
                let index = m.regex.capture_names().position(|n| n == Some(name.as_str())).ok_or_else(|| exc("IllegalArgumentException", format!("No group with name {{{name}}}")))?;
                out.push_str(&text(index));
            }
            '$' => {
                // Java reads as many digits as still make a group number.
                let mut index = 0usize;
                let mut any = false;
                while let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                    let next = index * 10 + d as usize;
                    if any && next >= groups.len() {
                        break;
                    }
                    index = next;
                    any = true;
                    chars.next();
                }
                if !any {
                    return Err(exc("IllegalArgumentException", "Illegal group reference"));
                }
                out.push_str(&text(index));
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

/// Java's syntax, as far as the two differ.
pub fn pattern(text: &str, flags: i32) -> R {
    let prefix = format!("{}{}{}", if flags & 2 != 0 { "(?i)" } else { "" }, if flags & 8 != 0 { "(?m)" } else { "" }, if flags & 32 != 0 { "(?s)" } else { "" });
    let source = text.replace("\\p{Alpha}", "[a-zA-Z]").replace("\\p{Digit}", "[0-9]").replace("\\p{Alnum}", "[a-zA-Z0-9]").replace("\\p{Punct}", "[!-/:-@\\[-`{-~]");
    let source = named_backrefs(&java_classes(&source));
    match Regex::new(&format!("{prefix}{source}")) {
        Ok(regex) => Ok(native(JPattern { text: text.to_owned(), regex })),
        Err(error) => Err(exc("PatternSyntaxException", format!("{error} near {text}"))),
    }
}

/// Java's escapes fancy-regex lacks: `\R` (any line break) and `\h` (horizontal space). Outside a class only.
fn java_classes(source: &str) -> String {
    if !source.contains("\\R") && !source.contains("\\h") {
        return source.to_owned();
    }
    let mut out = String::new();
    let mut chars = source.chars();
    let mut class = 0usize;
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('R') if class == 0 => out.push_str("(?:\\r\\n|[\\n\\x0B\\x0C\\r\\x{85}\\x{2028}\\x{2029}])"),
                Some('h') if class == 0 => out.push_str("[ \\t\\x{A0}\\x{1680}\\x{180E}\\x{2000}-\\x{200A}\\x{202F}\\x{205F}\\x{3000}]"),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            '[' => {
                class += 1;
                out.push(c);
            }
            ']' if class > 0 => {
                class -= 1;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Java lets `\1` name a group in a pattern that also has named groups; fancy-regex doesn't. Every group
/// gets a name (`__gN` for an unnamed one) and every `\N` becomes `\k<name>`, so the numbers stay Java's.
fn named_backrefs(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let has_named = source.contains("(?<") && (0..chars.len()).any(|i| chars[i] == '(' && chars.get(i + 1) == Some(&'?') && chars.get(i + 2) == Some(&'<') && !matches!(chars.get(i + 3), Some('=' | '!')));
    let has_ref = (0..chars.len().saturating_sub(1)).any(|i| chars[i] == '\\' && chars[i + 1].is_ascii_digit() && chars[i + 1] != '0');
    if !has_named || !has_ref {
        return source.to_owned();
    }
    let mut names: Vec<String> = Vec::new();
    let mut out = String::new();
    let (mut i, mut class) = (0usize, 0usize);
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let d = chars[i + 1];
            if class == 0 && d.is_ascii_digit() && d != '0' {
                let mut j = i + 1;
                let mut number = 0usize;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    let next = number * 10 + chars[j].to_digit(10).unwrap_or(0) as usize;
                    if number != 0 && next > names.len() {
                        break;
                    }
                    number = next;
                    j += 1;
                }
                match names.get(number.wrapping_sub(1)) {
                    Some(name) => out.push_str(&format!("\\k<{name}>")),
                    None => out.extend(&chars[i..j]),
                }
                i = j;
                continue;
            }
            out.push(c);
            out.push(d);
            i += 2;
            continue;
        }
        match c {
            '[' => class += 1,
            ']' if class > 0 => class -= 1,
            '(' if class == 0 => {
                if chars.get(i + 1) == Some(&'?') {
                    if chars.get(i + 2) == Some(&'<') && !matches!(chars.get(i + 3), Some('=' | '!')) {
                        let name: String = chars[i + 3..].iter().take_while(|c| **c != '>').collect();
                        names.push(name);
                    }
                } else {
                    let name = format!("__g{}", names.len() + 1);
                    out.push_str(&format!("(?<{name}>"));
                    names.push(name);
                    i += 1;
                    continue;
                }
            }
            _ => {}
        }
        out.push(c);
        i += 1;
    }
    out
}

fn compiled(text: &str) -> Result<Regex, V> {
    let made = pattern(text, 0)?;
    Ok(made.native::<JPattern>().expect("a pattern").regex.clone())
}

/// `$1` is the same in Rust's syntax, `\$` isn't.
fn replacement(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    if next == '$' {
                        out.push_str("$$");
                    } else {
                        out.push(next);
                    }
                }
            }
            '$' => {
                out.push_str("${");
                while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                    out.push(*d);
                    chars.next();
                }
                out.push('}');
            }
            other => out.push(other),
        }
    }
    out
}

pub fn replace_regex(input: &str, regex: &Regex, with: &str, all: bool) -> String {
    let with = replacement(with);
    if all { regex.replace_all(input, with.as_str()).into_owned() } else { regex.replace(input, with.as_str()).into_owned() }
}

pub fn split_regex(input: &str, regex: &Regex, limit: i32) -> Vec<V> {
    let mut out = Vec::new();
    let mut last = 0;
    for found in regex.find_iter(input).flatten() {
        if limit > 0 && out.len() as i32 == limit - 1 {
            break;
        }
        if found.end() == 0 {
            continue;
        }
        out.push(s(&input[last..found.start()]));
        last = found.end();
    }
    out.push(s(&input[last..]));
    if limit == 0 {
        while out.len() > 1 && out.last().is_some_and(|v| v.text().is_empty()) {
            out.pop();
        }
    }
    out
}

fn matcher_method(m: &JMatcher, name: &str, a: &[V]) -> Option<R> {
    let store = |caps: Option<fancy_regex::Captures>| match caps {
        Some(caps) => {
            *lock(&m.groups) = caps.iter().map(|g| g.map(|g| (g.start(), g.end()))).collect();
            true
        }
        None => {
            lock(&m.groups).clear();
            false
        }
    };
    let group = |at: usize| -> R {
        let groups = lock(&m.groups);
        match groups.get(at) {
            Some(Some((from, to))) => Ok(s(&m.input[*from..*to])),
            Some(None) => Ok(V::Null),
            None if groups.is_empty() => Err(exc("IllegalStateException", "No match found")),
            None => Err(exc("IndexOutOfBoundsException", format!("No group {at}"))),
        }
    };
    Some(match (name, a.len()) {
        ("find", 0) => {
            let at = *lock(&m.at);
            if at > m.input.len() {
                return Some(Ok(V::Bool(false)));
            }
            let caps = m.regex.captures_from_pos(&m.input, at).ok().flatten();
            if let Some(whole) = caps.as_ref().and_then(|c| c.get(0)) {
                *lock(&m.at) = if whole.end() == whole.start() { whole.end() + 1 } else { whole.end() };
            }
            Ok(V::Bool(store(caps)))
        }
        ("matches", 0) => {
            let caps = m.regex.captures(&m.input).ok().flatten().filter(|c| c.get(0).is_some_and(|w| w.start() == 0 && w.end() == m.input.len()));
            Ok(V::Bool(store(caps)))
        }
        ("lookingAt", 0) => {
            let caps = m.regex.captures(&m.input).ok().flatten().filter(|c| c.get(0).is_some_and(|w| w.start() == 0));
            Ok(V::Bool(store(caps)))
        }
        ("group", 0) => group(0),
        ("group", 1) => {
            if let V::Str(wanted) = &a[0] {
                let index = m.regex.capture_names().position(|n| n == Some(&**wanted));
                return Some(index.map_or_else(|| Err(exc("IllegalArgumentException", format!("No group with name <{wanted}>"))), group));
            }
            group(a[0].int().max(0) as usize)
        }
        ("groupCount", 0) => Ok(V::Int(m.regex.captures_len() as i32 - 1)),
        ("start", 0 | 1) => Ok(lock(&m.groups).get(a.first().map_or(0, |v| v.int() as usize)).copied().flatten().map_or(V::Int(-1), |g| V::Int(g.0 as i32))),
        ("end", 0 | 1) => Ok(lock(&m.groups).get(a.first().map_or(0, |v| v.int() as usize)).copied().flatten().map_or(V::Int(-1), |g| V::Int(g.1 as i32))),
        ("replaceAll", 1) if !matches!(a[0], V::Func(_)) => Ok(s(&replace_regex(&m.input, &m.regex, &a[0].text(), true))),
        ("replaceFirst", 1) => Ok(s(&replace_regex(&m.input, &m.regex, &a[0].text(), false))),
        ("reset", 0) => {
            *lock(&m.at) = 0;
            *lock(&m.appended) = 0;
            Ok(V::Null)
        }
        // The `while (find()) appendReplacement(sb, ...); appendTail(sb);` loop, e.g. `&#rrggbb` to colours.
        ("appendReplacement", 2) => {
            let Some((start, end)) = lock(&m.groups).first().copied().flatten() else {
                return Some(Err(exc("IllegalStateException", "No match available")));
            };
            let with = match expand(m, &a[1].text()) {
                Ok(with) => with,
                Err(error) => return Some(Err(error)),
            };
            let from = std::mem::replace(&mut *lock(&m.appended), end);
            let piece = format!("{}{}", &m.input[from.min(start)..start], with);
            append_to(&a[0], &piece);
            Ok(V::Null)
        }
        ("appendTail", 1) => {
            let from = (*lock(&m.appended)).min(m.input.len());
            append_to(&a[0], &m.input[from..]);
            Ok(a[0].clone())
        }
        _ => return None,
    })
}

fn append_to(builder: &V, text: &str) {
    if let V::Sb(buffer) = builder {
        lock(buffer).push_str(text);
    }
}

fn pattern_method(p: &JPattern, name: &str, a: &[V]) -> Option<R> {
    Some(match (name, a.len()) {
        ("matcher", 1) => Ok(native(JMatcher { regex: p.regex.clone(), input: a[0].text(), at: Mutex::new(0), groups: Mutex::new(Vec::new()), appended: Mutex::new(0) })),
        ("pattern" | "toString", 0) => Ok(s(&p.text)),
        ("split", 1) => Ok(arr(split_regex(&a[0].text(), &p.regex, 0))),
        ("split", 2) => Ok(arr(split_regex(&a[0].text(), &p.regex, a[1].int()))),
        ("asPredicate", 0) => {
            let regex = p.regex.clone();
            Ok(func(move |x| Ok(V::Bool(regex.is_match(&x[0].text()).unwrap_or(false)))))
        }
        _ => return None,
    })
}

// ---- Time ----

pub struct JInstant(pub i64);

pub fn instant(ms: i64) -> V {
    native(JInstant(ms))
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn instant_method(at: i64, name: &str, a: &[V]) -> Option<R> {
    let other = || a.first().and_then(|v| v.native::<JInstant>()).map_or(0, |o| o.0);
    let span = || a.first().and_then(|v| v.native::<JDuration>()).map_or(0, |d| d.0);
    Some(Ok(match name {
        "getEpochSecond" => V::Long(at.div_euclid(1000)),
        "toEpochMilli" => V::Long(at),
        "isBefore" => V::Bool(at < other()),
        "isAfter" => V::Bool(at > other()),
        "compareTo" => V::Int(at.cmp(&other()) as i32),
        "plus" => instant(at + span()),
        "minus" => instant(at - span()),
        "truncatedTo" => instant(at - at.rem_euclid(1000)),
        "plusSeconds" => instant(at + a[0].long() * 1000),
        "minusSeconds" => instant(at - a[0].long() * 1000),
        "plusMillis" => instant(at + a[0].long()),
        "equals" => V::Bool(a.first().and_then(|v| v.native::<JInstant>()).is_some_and(|o| o.0 == at)),
        "hashCode" => V::Int(at as i32),
        // `Instant.now().atZone(ZoneOffset.UTC).format(...)` (Tebex's event dates): the date and time at that instant,
        // which jrt keeps in UTC.
        "atZone" | "atOffset" => return Some(crate::call_static("LocalDateTime", "ofInstant", &[instant(at)])),
        // Java's ISO-8601: 2026-11-04T05:39:52Z, with the milliseconds when there are any.
        "toString" => {
            let base = crate::call_static("LocalDateTime", "ofInstant", &[instant(at)]).map(|t| t.text()).unwrap_or_default();
            let base = base.split('.').next().unwrap_or(&base).to_owned();
            let ms = at.rem_euclid(1000);
            s(&if ms == 0 { format!("{base}Z") } else { format!("{base}.{ms:03}Z") })
        }
        _ => return None,
    }))
}

// ---- Guava ----

/// `ImmutableList.builder()` and its kin.
pub struct JBuilder {
    kind: &'static str,
    items: Mutex<Vec<V>>,
}

fn into_map(pairs: &[V]) -> V {
    let mut out = JMap::new();
    for pair in pairs {
        if let V::Entry(entry) = pair {
            out.insert(entry.0.key(), (entry.0.clone(), entry.1.clone()));
        }
    }
    V::Map(Arc::new(Mutex::new(out)))
}

fn into_set(items: &[V]) -> V {
    V::Set(Arc::new(Mutex::new(items.iter().map(|k| (k.key(), (k.clone(), V::Null))).collect())))
}

fn builder_method(this: &V, b: &JBuilder, name: &str, a: &[V]) -> Option<R> {
    let mut items = lock(&b.items);
    Some(Ok(match name {
        // `add(E... elements)` given the array: each element, as Java spreads it (piston's addParts(CommandPart...)).
        "add" if a.len() == 1 && matches!(a[0], V::Arr(_)) => {
            items.extend(a[0].items().unwrap_or_default());
            this.clone()
        }
        "add" | "addAll" if name == "add" && a.len() == 1 => {
            items.push(a[0].clone());
            this.clone()
        }
        "add" => {
            items.extend(a.iter().cloned());
            this.clone()
        }
        "addAll" => {
            match a[0].items() {
                Ok(more) => items.extend(more),
                Err(e) => return Some(Err(e)),
            }
            this.clone()
        }
        "put" if a.len() == 2 => {
            items.push(V::Entry(Arc::new((a[0].clone(), a[1].clone()))));
            this.clone()
        }
        "put" => {
            items.push(a[0].clone());
            this.clone()
        }
        "putAll" => {
            match a[0].items() {
                Ok(more) => items.extend(more),
                Err(e) => return Some(Err(e)),
            }
            this.clone()
        }
        "build" | "buildOrThrow" | "buildKeepingLast" => match b.kind {
            "list" => list(items.clone()),
            "set" => into_set(&items),
            _ => into_map(&items),
        },
        _ => return None,
    }))
}

fn builder(kind: &'static str) -> V {
    native(JBuilder { kind, items: Mutex::new(Vec::new()) })
}

pub struct JSplitter {
    on: String,
    /// Any one of those characters, not the whole text.
    any: bool,
    limit: Option<usize>,
    omit: bool,
    trim: bool,
    pair: Option<String>,
    /// `Splitter.onPattern(regex)`: `on` is a regular expression, as Java's.
    pattern: bool,
}

fn splitter(sp: JSplitter) -> V {
    native(sp)
}

fn splitter_method(sp: &JSplitter, name: &str, a: &[V]) -> Option<R> {
    let copy = || JSplitter { on: sp.on.clone(), any: sp.any, limit: sp.limit, omit: sp.omit, trim: sp.trim, pair: sp.pair.clone(), pattern: sp.pattern };
    Some(Ok(match name {
        "omitEmptyStrings" => splitter(JSplitter { omit: true, ..copy() }),
        "trimResults" => splitter(JSplitter { trim: true, ..copy() }),
        "limit" => splitter(JSplitter { limit: Some(a[0].int() as usize), ..copy() }),
        "withKeyValueSeparator" => splitter(JSplitter { pair: Some(a[0].text()), ..copy() }),
        "split" | "splitToList" => {
            let text = a[0].text();
            let parts: Vec<String> = match (sp.any, sp.limit) {
                // Java's regex split, every piece kept (Guava drops none at the end, unlike String.split).
                _ if sp.pattern => match crate::call(&s(&text), "split", &[s(&sp.on), V::Int(sp.limit.map_or(-1, |l| l as i32))]).and_then(|parts| parts.items()) {
                    Ok(parts) => parts.iter().map(V::text).collect(),
                    Err(e) => return Some(Err(e)),
                },
                (true, _) => text.split(|c| sp.on.contains(c)).map(str::to_owned).collect(),
                (false, Some(limit)) => text.splitn(limit, sp.on.as_str()).map(str::to_owned).collect(),
                (false, None) => text.split(sp.on.as_str()).map(str::to_owned).collect(),
            };
            let parts = parts.into_iter().map(|p| if sp.trim { p.trim().to_owned() } else { p }).filter(|p| !sp.omit || !p.is_empty());
            match &sp.pair {
                Some(pair) => into_map(&parts.filter_map(|p| p.split_once(pair.as_str()).map(|(k, v)| V::Entry(Arc::new((s(k), s(v)))))).collect::<Vec<_>>()),
                None => list(parts.map(|p| s(&p)).collect()),
            }
        }
        _ => return None,
    }))
}

// ---- Gson: objects are maps, arrays are lists, and primitives are themselves ----

fn gson_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    // `getAsInt` is `OptionalInt`'s too, and streams and suppliers have their own.
    if !matches!(this, V::Null | V::Bool(_) | V::Int(_) | V::Long(_) | V::Double(_) | V::Str(_) | V::Map(_) | V::List(_)) {
        return None;
    }
    Some(Ok(match (name, a.len()) {
        ("getAsString", 0) => s(&this.text()),
        ("getAsInt", 0) => V::Int(this.int()),
        ("getAsLong", 0) => V::Long(this.long()),
        ("getAsDouble", 0) => V::Double(this.double()),
        ("getAsBoolean", 0) => V::Bool(this.truth()),
        ("getAsJsonObject" | "getAsJsonArray" | "getAsJsonPrimitive", 0) => this.clone(),
        // `json.getAsJsonObject("_info")`: the member, as JsonObject's own accessors give it (null when absent).
        ("getAsJsonObject" | "getAsJsonArray" | "getAsJsonPrimitive", 1) if matches!(this, V::Map(_)) => return call(this, "get", a).into(),
        ("isJsonObject", 0) => V::Bool(matches!(this, V::Map(_))),
        ("isJsonArray", 0) => V::Bool(matches!(this, V::List(_))),
        ("isJsonNull", 0) => V::Bool(this.is_null()),
        ("isJsonPrimitive", 0) => V::Bool(matches!(this, V::Str(_) | V::Int(_) | V::Long(_) | V::Double(_) | V::Bool(_))),
        ("isString", 0) => V::Bool(matches!(this, V::Str(_))),
        ("isNumber", 0) => V::Bool(matches!(this, V::Int(_) | V::Long(_) | V::Double(_))),
        ("isBoolean", 0) => V::Bool(matches!(this, V::Bool(_))),
        ("addProperty" | "add", 2) if matches!(this, V::Map(_)) => {
            if let V::Map(map) = this {
                lock(map).insert(a[0].key(), (a[0].clone(), a[1].clone()));
            }
            V::Null
        }
        ("has", 1) if matches!(this, V::Map(_)) => return call(this, "containsKey", a).into(),
        _ => return None,
    }))
}

/// Gson / GsonBuilder / JsonParser. `true`: `FieldNamingPolicy.LOWER_CASE_WITH_UNDERSCORES` (a field `expiresAt` is
/// `"expires_at"`), which GsonBuilder.setFieldNamingPolicy sets and `create()` keeps.
pub struct JGson(pub bool);

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// What Gson writes for a value: the plugin's own object as its instance fields (nulls left out, as Gson does by
/// default; jrt's own `$`/`__` slots never), an enum as its constant's name, lists and maps item by item (map keys as
/// they are: the naming policy is for fields).
fn gson_json(value: &V, snake_case: bool) -> serde_json::Value {
    match value {
        V::Obj(object) => {
            if crate::instance_of(value, "Enum") {
                return serde_json::Value::String(call(value, "name", &[]).map(|n| n.text()).unwrap_or_else(|_| value.text()));
            }
            let fields: Vec<(&'static str, V)> = lock(&object.fields).iter().map(|(k, v)| (*k, v.clone())).collect();
            let mut out = serde_json::Map::new();
            for (name, field) in fields {
                if name.starts_with('$') || name.starts_with("__") || field.is_null() {
                    continue;
                }
                out.insert(if snake_case { snake(name) } else { name.to_owned() }, gson_json(&field, snake_case));
            }
            serde_json::Value::Object(out)
        }
        V::Arr(items) | V::List(items) | V::Stream(items) => serde_json::Value::Array(lock(items).clone().iter().map(|v| gson_json(v, snake_case)).collect()),
        V::Map(map) => serde_json::Value::Object(lock(map).values().map(|(k, v)| (k.text(), (k.clone(), v.clone()))).collect::<Vec<_>>().into_iter().map(|(k, (_, v))| (k, gson_json(&v, snake_case))).collect()),
        other => crate::javaio::to_json(other),
    }
}

fn gson_object_method(name: &str, a: &[V], snake_case: bool) -> Option<R> {
    Some(match name {
        "toJson" => {
            let json = gson_json(&a[0], snake_case);
            match a.get(1) {
                Some(writer) => call(writer, "write", &[s(&serde_json::to_string_pretty(&json).unwrap_or_default())]).map(|_| V::Null),
                None => Ok(s(&json.to_string())),
            }
        }
        "toJsonTree" => Ok(a[0].clone()),
        // `new JsonParser().parse(reader or text)`: the tree, as `JsonParser.parseReader` gives it.
        "parse" if a.len() == 1 => return gson_object_method("fromJson", &a[..1], snake_case),
        "fromJson" => {
            let text = match a[0].native::<crate::javaio::JBytes>() {
                Some(_) => String::from_utf8_lossy(&crate::javaio::bytes_of(&a[0]).unwrap_or_default()).into_owned(),
                None => a[0].text(),
            };
            if text.trim().is_empty() {
                return Some(Ok(V::Null));
            }
            match serde_json::from_str::<serde_json::Value>(&text) {
                // `fromJson(text, Settings.class)`: a Settings, its fields filled by their types.
                Ok(value) => match a.get(1) {
                    Some(V::Class(class)) => crate::reflect::gson_bind(&value, class),
                    _ => Ok(crate::javaio::from_json(&value)),
                },
                Err(error) => Err(exc("JsonSyntaxException", error.to_string())),
            }
        }
        "setPrettyPrinting" | "disableHtmlEscaping" | "serializeNulls" | "registerTypeAdapter" | "create" | "enableComplexMapKeySerialization" | "setLenient"
        | "excludeFieldsWithoutExposeAnnotation" | "setDateFormat" | "registerTypeHierarchyAdapter" | "registerTypeAdapterFactory" | "setVersion" => Ok(native(JGson(snake_case))),
        "setFieldNamingPolicy" | "setFieldNamingStrategy" => Ok(native(JGson(a.first().is_some_and(|p| p.text().contains("UNDERSCORES"))))),
        _ => return None,
    })
}

/// `java.util.logging.LogRecord`: its fields by name (level, message, parameters, thrown, loggerName...).
pub struct JLogRecord(pub Mutex<BTreeMap<&'static str, V>>);

impl JLogRecord {
    /// The message with its `{0}` parameters filled in, as `java.util.logging.Formatter.formatMessage`.
    pub fn text(&self) -> String {
        let fields = lock(&self.0);
        let mut text = fields.get("message").map(V::text).unwrap_or_default();
        if let Some(params) = fields.get("parameters").and_then(|p| p.items().ok()) {
            for (at, value) in params.iter().enumerate() {
                text = text.replace(&format!("{{{at}}}"), &value.text());
            }
        }
        text
    }
    pub fn level(&self) -> String {
        lock(&self.0).get("level").map_or_else(|| "INFO".to_owned(), V::text)
    }
}

fn log_record_method(record: &JLogRecord, this: &V, name: &str, a: &[V]) -> Option<R> {
    const FIELDS: &[&str] = &["level", "message", "parameters", "thrown", "loggerName", "millis", "sourceClassName", "sourceMethodName", "resourceBundle", "resourceBundleName", "sequenceNumber", "instant", "longThreadID", "threadID"];
    let field = |prefix: &str| FIELDS.iter().find(|f| name.strip_prefix(prefix).is_some_and(|rest| rest.eq_ignore_ascii_case(f))).copied();
    if a.is_empty() {
        if let Some(found) = field("get") {
            return Some(Ok(match found {
                "millis" => lock(&record.0).get("millis").cloned().unwrap_or(V::Long(time_ms())),
                _ => lock(&record.0).get(found).cloned().unwrap_or(V::Null),
            }));
        }
        if name == "toString" {
            return Some(Ok(s(&record.text())));
        }
    }
    if a.len() == 1 {
        if let Some(found) = field("set") {
            lock(&record.0).insert(found, a[0].clone());
            return Some(Ok(V::Null));
        }
    }
    let _ = this;
    None
}

fn time_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

// ---- The doors ----

pub fn field(class: &str, name: &str) -> Option<V> {
    Some(match (class, name) {
        ("System", "out" | "err") => native(JPrint),
        ("TimeUnit", _) | ("ChronoUnit", _) | ("Level", _) => s(name),
        ("Pattern", "CASE_INSENSITIVE") => V::Int(2),
        ("Pattern", "MULTILINE") => V::Int(8),
        ("Pattern", "DOTALL") => V::Int(32),
        ("Instant", "EPOCH") => instant(0),
        _ => return None,
    })
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    Some(Ok(match class {
        "SimpleDateFormat" => native(JDateFormat(a.first().map_or("yyyy-MM-dd HH:mm:ss".to_owned(), V::text))),
        "URI" => a.first().cloned().unwrap_or(V::Null),
        "AbstractMap.SimpleImmutableEntry" | "AbstractMap.SimpleEntry" | "SimpleImmutableEntry" | "SimpleEntry" if a.len() == 2 => V::Entry(Arc::new((a[0].clone(), a[1].clone()))),
        "JStub" | "OkHttpClient.Builder" | "ThreadFactoryBuilder" => native(JStub),
        // Guava's `MapMaker`: weak keys and concurrency are the runtime's own; `makeMap` is a map.
        "MapMaker" => native(JMapMaker),
        // json-simple: `JSONParser.parse` reads as Gson's parser does; a `JSONObject` is a map, a `JSONArray` a list.
        "JSONParser" => native(JGson(false)),
        "JSONObject" if a.is_empty() => V::Map(Arc::new(Mutex::new(JMap::new()))),
        "JSONObject" => a[0].clone(),
        "JSONArray" if a.is_empty() => list(Vec::new()),
        "JSONArray" => a[0].clone(),
        // `new LogRecord(level, message)`: what a logger's `log(record)` writes.
        "LogRecord" if a.len() == 2 => native(JLogRecord(Mutex::new(BTreeMap::from([("level", a[0].clone()), ("message", a[1].clone())])))),
        "ImmutableList.Builder" => builder("list"),
        "ImmutableSet.Builder" | "ImmutableSortedSet.Builder" => builder("set"),
        "ImmutableMap.Builder" | "ImmutableSortedMap.Builder" | "ImmutableBiMap.Builder" => builder("map"),
        "ImmutableSetMultimap.Builder" => multimap(true),
        "ImmutableListMultimap.Builder" => multimap(false),
        "UUID" => s(&{
            let (most, least) = (a[0].long() as u64, a[1].long() as u64);
            format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", most >> 32, (most >> 16) & 0xffff, most & 0xffff, least >> 48, least & 0xffff_ffff_ffff)
        }),
        "ByteArrayOutputStream" | "StringWriter" => native(JSink { text: Mutex::new(Vec::new()) }),
        "StringReader" => crate::javaio::stream(a[0].text().into_bytes()),
        "IdentityHashMap" | "ConcurrentSkipListMap" | "Hashtable" => V::Map(Arc::new(Mutex::new(JMap::new()))),
        "ConcurrentSkipListSet" | "CopyOnWriteArraySet" => into_set(&[]),
        "ConcurrentLinkedQueue" | "ConcurrentLinkedDeque" | "LinkedBlockingQueue" | "LinkedBlockingDeque" | "PriorityQueue" | "ArrayBlockingQueue" => list(Vec::new()),
        "HandlerList" => native(JHandlers),
        "Date" => instant(a.first().map_or_else(now_ms, V::long)),
        "CountDownLatch" => native(Cell(Mutex::new(a[0].clone()))),
        "Object" => native(JPlain),
        // `new URL(text)` is its text, as `URI.create` is (crate::net): reading it (`openConnection`,
        // `openStream`) meets the missing network as an IOException. No scheme is Java's MalformedURLException.
        "URL" if a.len() == 1 => {
            let text = a[0].text();
            if !text.split_once(':').is_some_and(|(scheme, _)| !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))) {
                return Some(Err(exc("MalformedURLException", format!("no protocol: {text}"))));
            }
            s(&text)
        }
        "URL" => return Some(Err(exc("MalformedURLException", "converted plugins have no network yet"))),
        "ReentrantLock" => native(JLock::new()),
        "FutureTask" => native(JFutureTask { work: Mutex::new(a.first().cloned()), result: a.get(1).cloned().unwrap_or(V::Null), done: Mutex::new(None), finished: Condvar::new() }),
        "Semaphore" => native(JSemaphore { permits: Mutex::new(a.first().map_or(0, V::long)), freed: Condvar::new() }),
        "ReentrantReadWriteLock" => native(JLock::new()),
        "CompletableFuture" => future_v(future(None)),
        "Thread" => {
            // `new Thread(group, task, name...)`: the group is only a grouping here.
            let a = if a.first().is_some_and(|g| g.native::<crate::javacore::JThreadGroup>().is_some()) { &a[1..] } else { a };
            let (task, name) = match a {
                [] => (None, "Thread".to_owned()),
                [V::Str(name)] => (None, name.to_string()),
                [task] => (Some(task.clone()), "Thread".to_owned()),
                [task, name, ..] => (Some(task.clone()), name.text()),
            };
            native(JThread { task: Mutex::new(task), name: Mutex::new(name), handle: Mutex::new(None), interrupted: Default::default() })
        }
        "ThreadLocal" => native(JThreadLocal { values: Mutex::new(BTreeMap::new()), initial: None }),
        "JsonObject" => V::Map(Arc::new(Mutex::new(JMap::new()))),
        "JsonArray" => list(Vec::new()),
        "JsonPrimitive" => native(JPrim(a[0].clone())),
        "Gson" | "GsonBuilder" | "JsonParser" => native(JGson(false)),
        "ScheduledThreadPoolExecutor" | "ThreadPoolExecutor" | "ForkJoinPool" => native(JExecutor { stopped: Default::default() }),
        _ => return None,
    }))
}

/// `ByteArrayOutputStream` and `StringWriter`.
pub struct JSink {
    text: Mutex<Vec<u8>>,
}
pub struct JPrint;
/// `JsonPrimitive`: a value that Java's overloads can tell from the value itself.
pub struct JPrim(pub V);
pub struct JCharMatcher(String);
/// Guava's `Range.closed`.
pub struct JRange(V, V);
/// A static field of the plugin's own class, as `Class.getFields()` gives it.
pub struct JField {
    name: &'static str,
    kind: &'static str,
    value: V,
    owner: &'static str,
}
pub struct JDateFormat(pub String);
pub struct JLocalDate(pub (i64, u32, u32));

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = ((doy - (153 * mp + 2) / 5 + 1) as u32, if mp < 10 { mp + 3 } else { mp - 9 } as u32);
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn format_date(pattern: &str, ms: i64) -> String {
    let (y, mo, d) = civil(ms.div_euclid(86_400_000));
    let second = ms.div_euclid(1000).rem_euclid(86_400);
    let (mut out, mut chars) = (String::new(), pattern.chars().peekable());
    while let Some(c) = chars.next() {
        if c == '\'' {
            for q in chars.by_ref() {
                if q == '\'' {
                    break;
                }
                out.push(q);
            }
            continue;
        }
        if !c.is_ascii_alphabetic() {
            out.push(c);
            continue;
        }
        let mut run = 1;
        while chars.peek() == Some(&c) {
            chars.next();
            run += 1;
        }
        let value = match c {
            'y' | 'u' => y,
            'M' => i64::from(mo),
            'd' => i64::from(d),
            'H' => second / 3600,
            'm' => second / 60 % 60,
            's' => second % 60,
            'z' | 'Z' | 'X' | 'O' => {
                out.push_str("UTC");
                continue;
            }
            _ => 0,
        };
        out.push_str(&format!("{value:0run$}"));
    }
    out
}

/// A key to many values: `HashMultimap` and its kin, and their builders.
pub struct JMultimap {
    map: Mutex<JMap>,
    set: bool,
}

fn multimap(set: bool) -> V {
    native(JMultimap { map: Mutex::new(JMap::new()), set })
}

fn multimap_method(m: &JMultimap, this: &V, name: &str, a: &[V]) -> Option<R> {
    let fresh = || if m.set { into_set(&[]) } else { list(Vec::new()) };
    let entries = || -> Vec<V> {
        lock(&m.map).values().flat_map(|(k, group)| group.items().unwrap_or_default().into_iter().map(|v| V::Entry(Arc::new((k.clone(), v)))).collect::<Vec<_>>()).collect()
    };
    let put = |k: &V, v: &V| -> R {
        let group = lock(&m.map).entry(k.key()).or_insert_with(|| (k.clone(), fresh())).1.clone();
        call(&group, "add", std::slice::from_ref(v))
    };
    Some(Ok(match (name, a.len()) {
        ("put", 2) => return Some(put(&a[0], &a[1]).map(|_| this.clone())),
        ("putAll", 1) => {
            let source = a[0].clone();
            let rows = match source.native::<JMultimap>() {
                Some(_) => match call(&source, "entries", &[]).and_then(|e| e.items()) {
                    Ok(rows) => rows,
                    Err(error) => return Some(Err(error)),
                },
                None => match call(&source, "entrySet", &[]).and_then(|e| e.items()) {
                    Ok(rows) => rows,
                    Err(error) => return Some(Err(error)),
                },
            };
            for row in &rows {
                if let V::Entry(pair) = row {
                    if let Err(error) = put(&pair.0, &pair.1) {
                        return Some(Err(error));
                    }
                }
            }
            this.clone()
        }
        ("putAll", 2) => {
            for value in a[1].items().unwrap_or_default() {
                if let Err(error) = put(&a[0], &value) {
                    return Some(Err(error));
                }
            }
            this.clone()
        }
        ("get", 1) => lock(&m.map).get(&a[0].key()).map_or_else(fresh, |(_, group)| group.clone()),
        ("removeAll", 1) => lock(&m.map).remove(&a[0].key()).map_or_else(fresh, |(_, group)| group),
        ("remove", 2) => {
            let group = lock(&m.map).get(&a[0].key()).map(|(_, group)| group.clone());
            let Some(group) = group else { return Some(Ok(V::Bool(false))) };
            let done = call(&group, "remove", &a[1..]);
            if call(&group, "isEmpty", &[]).is_ok_and(|e| e.truth()) {
                lock(&m.map).remove(&a[0].key());
            }
            return Some(done);
        }
        ("containsKey", 1) => V::Bool(lock(&m.map).contains_key(&a[0].key())),
        ("containsEntry", 2) => {
            let group = lock(&m.map).get(&a[0].key()).map(|(_, group)| group.clone());
            match group {
                Some(group) => return Some(call(&group, "contains", &a[1..])),
                None => V::Bool(false),
            }
        }
        ("isEmpty", 0) => V::Bool(lock(&m.map).is_empty()),
        ("size", 0) => V::Int(entries().len() as i32),
        ("entries", 0) => list(entries()),
        ("keySet" | "keys", 0) => into_set(&lock(&m.map).values().map(|(k, _)| k.clone()).collect::<Vec<_>>()),
        ("values", 0) => list(entries().iter().filter_map(|e| if let V::Entry(p) = e { Some(p.1.clone()) } else { None }).collect()),
        ("asMap", 0) => V::Map(Arc::new(Mutex::new(lock(&m.map).clone()))),
        ("clear", 0) => {
            lock(&m.map).clear();
            V::Null
        }
        ("build", 0) => this.clone(),
        ("hashCode", 0) => V::Int(entries().iter().fold(0, |h: i32, e| h.wrapping_add(e.text().bytes().fold(0, |x: i32, b| x.wrapping_mul(31).wrapping_add(i32::from(b)))))),
        ("equals", 1) => V::Bool(a[0].native::<JMultimap>().is_some_and(|o| {
            let (mut x, mut y) = (entries().iter().map(V::text).collect::<Vec<_>>(), lock(&o.map).values().flat_map(|(k, g)| g.items().unwrap_or_default().into_iter().map(|v| format!("{}={}", k.text(), v.text())).collect::<Vec<_>>()).collect::<Vec<_>>());
            x.sort();
            y.sort();
            x == y
        })),
        ("toString", 0) => s(&format!("{{{}}}", entries().iter().map(V::text).collect::<Vec<_>>().join(", "))),
        _ => return None,
    }))
}

/// Guava's `AbstractIterator`.
pub fn abstract_iterator(this: &V, name: &str) -> R {
    if name == "endOfData" {
        this.set("$done", V::Bool(true))?;
        return Ok(V::Null);
    }
    if !this.get("$ready")?.truth() && !this.get("$done")?.truth() {
        let value = call(this, "computeNext", &[])?;
        this.set("$value", value)?;
        this.set("$ready", V::Bool(true))?;
    }
    let done = this.get("$done")?.truth();
    if name == "hasNext" {
        return Ok(V::Bool(!done));
    }
    if done {
        return Err(exc("NoSuchElementException", "end"));
    }
    this.set("$ready", V::Bool(false))?;
    this.get("$value")
}
/// What's out of a plugin's reach here, such as an HTTP client: any call gives itself back.
pub struct JStub;
/// Guava's `MapMaker`, until `makeMap`.
pub struct JMapMaker;

/// `Class.getProtectionDomain()` and its `getCodeSource()`: one value answering both.
pub struct JCodeSource;

/// The plugin's jar as a `file:` URL: the server's plugins folder, where Paper would have it.
pub fn plugin_jar_url() -> String {
    let file = std::env::current_dir().unwrap_or_default().join("plugins").join("plugin.jar");
    format!("file:/{}", file.to_string_lossy().replace('\\', "/").trim_start_matches('/'))
}
pub struct JHandlers;
pub struct JPlain;

/// Default methods of `java.util.function` and `Comparator`, on a lambda.
pub fn func_default(this: &V, name: &str, a: &[V]) -> Option<R> {
    let this = this.clone();
    let other = a.first().cloned();
    Some(Ok(match (name, a.len()) {
        ("reversed", 0) => func(move |x| Ok(V::Int(-call(&this, "$invoke", x)?.int()))),
        ("thenComparing" | "thenComparingInt" | "thenComparingLong" | "thenComparingDouble", 1) => {
            let other = other?;
            func(move |x| {
                let first = call(&this, "$invoke", x)?.int();
                if first != 0 {
                    return Ok(V::Int(first));
                }
                if crate::instance_of(&other, "Comparator") && false {
                    return call(&other, "$invoke", x);
                }
                match call(&other, "$invoke", &x[..1]) {
                    Ok(p) if x.len() == 2 => match call(&other, "$invoke", &x[1..2]) {
                        Ok(q) => Ok(V::Int(cmp_values(&p, &q))),
                        Err(_) => call(&other, "$invoke", x),
                    },
                    _ => call(&other, "$invoke", x),
                }
            })
        }
        ("andThen", 1) if matches!(other, Some(V::Func(_))) => {
            let other = other?;
            func(move |x| {
                let out = call(&this, "$invoke", x)?;
                if out.is_null() { call(&other, "$invoke", x) } else { call(&other, "$invoke", &[out]) }
            })
        }
        ("compose", 1) if matches!(other, Some(V::Func(_))) => {
            let other = other?;
            func(move |x| {
                let out = call(&other, "$invoke", x)?;
                call(&this, "$invoke", &[out])
            })
        }
        ("negate", 0) => func(move |x| Ok(V::Bool(!call(&this, "$invoke", x)?.truth()))),
        ("and", 1) if matches!(other, Some(V::Func(_))) => {
            let other = other?;
            func(move |x| Ok(V::Bool(call(&this, "$invoke", x)?.truth() && call(&other, "$invoke", x)?.truth())))
        }
        ("or", 1) if matches!(other, Some(V::Func(_))) => {
            let other = other?;
            func(move |x| Ok(V::Bool(call(&this, "$invoke", x)?.truth() || call(&other, "$invoke", x)?.truth())))
        }
        _ => return None,
    }))
}

fn plain_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(primitive) = this.native::<JPrim>() {
        return gson_method(&primitive.0, name, a);
    }
    if let Some(m) = this.native::<JMultimap>() {
        return multimap_method(m, this, name, a);
    }
    if let Some(f) = this.native::<JField>() {
        return Some(Ok(match name {
            "getName" => s(f.name),
            "getType" => V::Class(f.kind),
            "get" => f.value.clone(),
            // `field.getInt(null)` and the other typed reads of a static field (WorldEdit's reserved block ids).
            "getInt" | "getShort" | "getByte" => V::Int(f.value.int()),
            "getLong" => V::Long(f.value.long()),
            "getDouble" | "getFloat" => V::Double(f.value.double()),
            "getBoolean" => V::Bool(f.value.truth()),
            "getChar" => f.value.clone(),
            "getModifiers" => V::Int(0x19),
            "setAccessible" => V::Null,
            "getDeclaringClass" => V::Class(f.owner),
            "getGenericType" => V::Class(f.kind),
            "isSynthetic" | "isEnumConstant" => V::Bool(false),
            // The plugin's own annotations on the field (`@Comment("...")`), kept by the converter as `Owner.field`.
            "getAnnotation" | "getDeclaredAnnotation" if !a.is_empty() => return Some(crate::reflect::class_annotation(&format!("{}.{}", f.owner, f.name), &a[0])),
            "isAnnotationPresent" if !a.is_empty() => return Some(crate::reflect::class_annotation(&format!("{}.{}", f.owner, f.name), &a[0]).map(|found| V::Bool(!found.is_null()))),
            "getAnnotations" | "getDeclaredAnnotations" => arr(Vec::new()),
            _ => return None,
        }));
    }
    if let Some(range) = this.native::<JRange>() {
        return Some(Ok(match name {
            "contains" | "apply" | "test" => V::Bool(cmp_values(&a[0], &range.0) >= 0 && cmp_values(&a[0], &range.1) <= 0),
            "lowerEndpoint" => range.0.clone(),
            "upperEndpoint" => range.1.clone(),
            _ => return None,
        }));
    }
    if let Some(date) = this.native::<JLocalDate>() {
        const MONTHS: [&str; 12] = ["JANUARY", "FEBRUARY", "MARCH", "APRIL", "MAY", "JUNE", "JULY", "AUGUST", "SEPTEMBER", "OCTOBER", "NOVEMBER", "DECEMBER"];
        let (year, month, day) = date.0;
        return Some(Ok(match name {
            "getYear" => V::Int(year as i32),
            "getMonthValue" => V::Int(month as i32),
            "getMonth" => s(MONTHS[month as usize - 1]),
            "getDayOfMonth" => V::Int(day as i32),
            "getDayOfYear" => {
                let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
                const BEFORE: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
                V::Int(BEFORE[month as usize - 1] + day as i32 + i32::from(leap && month > 2))
            }
            "lengthOfYear" => V::Int(if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 { 366 } else { 365 }),
            "toString" => s(&format!("{year:04}-{month:02}-{day:02}")),
            _ => return None,
        }));
    }
    if let Some(format) = this.native::<JDateFormat>() {
        return match name {
            "withZone" | "withLocale" => Some(Ok(this.clone())),
            // An Instant, a LocalDateTime or LocalTime (`formatter.format(dateTime)`), or epoch milliseconds.
            "format" => Some(Ok(s(&format_date(&format.0, a[0].native::<JInstant>().map(|i| i.0).or_else(|| crate::jtime::millis(&a[0])).unwrap_or_else(|| a[0].long()))))),
            "setTimeZone" | "setLenient" | "applyPattern" => Some(Ok(V::Null)),
            "toPattern" => Some(Ok(s(&format.0))),
            _ => None,
        };
    }
    if matches!(this, V::Str(_)) {
        // `UUID`, which is its text here.
        let hex: Vec<u8> = this.text().bytes().filter(u8::is_ascii_hexdigit).collect();
        let part = |from: usize, to: usize| u64::from_str_radix(std::str::from_utf8(hex.get(from..to).unwrap_or(b"0")).unwrap_or("0"), 16).unwrap_or(0);
        match name {
            "version" if hex.len() == 32 => return Some(Ok(V::Int(part(12, 13) as i32))),
            "getMostSignificantBits" if hex.len() == 32 => return Some(Ok(V::Long(part(0, 16) as i64))),
            "getLeastSignificantBits" if hex.len() == 32 => return Some(Ok(V::Long(part(16, 32) as i64))),
            "getLanguage" => return Some(Ok(this.clone())),
            _ => {}
        }
    }
    if this.native::<JMapMaker>().is_some() {
        return Some(Ok(match name {
            "makeMap" => V::Map(Arc::new(Mutex::new(JMap::new()))),
            _ => this.clone(),
        }));
    }
    if this.native::<JStub>().is_some() {
        return match name {
            "toString" => Some(Ok(s("stub"))),
            // An event system's answer: nobody listened, so what came in goes out.
            "dispatchUniqueIdDetermineType" | "dispatchUniqueIdLookup" | "dispatchUsernameLookup" => Some(Ok(a.last().cloned().unwrap_or(V::Null))),
            "dispatchUserUnload" => Some(Ok(V::Bool(true))),
            dispatch if dispatch.starts_with("dispatch") => Some(Ok(a.iter().find(|v| matches!(v, V::Bool(_))).cloned().unwrap_or_else(|| this.clone()))),
            "hashCode" => Some(Ok(V::Int(0))),
            "equals" => None,
            _ => Some(Ok(this.clone())),
        };
    }
    if this.native::<JHandlers>().is_some() {
        return Some(Ok(V::Null));
    }
    if this.native::<JPrint>().is_some() {
        return match name {
            "println" | "print" | "printf" | "flush" => {
                eprintln!("{}", a.first().map_or_else(String::new, V::text));
                Some(Ok(V::Null))
            }
            _ => None,
        };
    }
    if let Some(sink) = this.native::<JSink>() {
        let piece = |v: &V| crate::javaio::bytes_of(v).filter(|_| !matches!(v, V::Str(_))).unwrap_or_else(|| v.text().into_bytes());
        return Some(Ok(match name {
            // `OutputStream.write(int)`: one byte, not its digits.
            "write" if a.len() == 1 && matches!(a[0], V::Int(_)) => {
                lock(&sink.text).push(a[0].int() as u8);
                V::Null
            }
            // `write(String, off, len)` / `write(char[], off, len)` and `append(text, start, end)`: a piece of the text, by
            // Java's (UTF-16) positions. snakeyaml-engine's emitter writes long scalars this way; all of it was lost.
            "write" | "append" if a.len() == 3 && matches!(a[0], V::Str(_) | V::Sb(_)) || (name == "write" && a.len() == 3 && matches!(a[0], V::Arr(_)) && a[0].items().is_ok_and(|i| i.first().is_some_and(|c| matches!(c, V::Char(_))))) => {
                let units: Vec<u16> = match &a[0] {
                    V::Arr(_) => a[0].items().unwrap_or_default().iter().map(|c| c.text()).collect::<String>().encode_utf16().collect(),
                    other => other.text().encode_utf16().collect(),
                };
                let from = (a[1].int().max(0) as usize).min(units.len());
                let to = if name == "append" { a[2].int().max(0) as usize } else { from + a[2].int().max(0) as usize }.min(units.len()).max(from);
                lock(&sink.text).extend(String::from_utf16_lossy(&units[from..to]).into_bytes());
                if name == "append" { this.clone() } else { V::Null }
            }
            "write" if a.len() == 3 => {
                let all = crate::javaio::bytes_of(&a[0]).unwrap_or_default();
                let (off, len) = (a[1].int().max(0) as usize, a[2].int().max(0) as usize);
                lock(&sink.text).extend(all.get(off..off + len).unwrap_or_default());
                V::Null
            }
            "write" | "append" | "print" => {
                lock(&sink.text).extend(piece(&a[0]));
                this.clone()
            }
            "println" => {
                let mut text = lock(&sink.text);
                if let Some(line) = a.first() {
                    text.extend(piece(line));
                }
                text.push(b'\n');
                V::Null
            }
            "toByteArray" => crate::javaio::to_byte_array(&lock(&sink.text)),
            "toString" => s(&String::from_utf8_lossy(&lock(&sink.text))),
            "size" => V::Int(lock(&sink.text).len() as i32),
            "reset" => {
                lock(&sink.text).clear();
                V::Null
            }
            "close" | "flush" => V::Null,
            _ => return None,
        }));
    }
    if let Some(cell) = this.native::<Cell>() {
        // `CountDownLatch`, and what atomics add to `stdlib`.
        let mut value = lock(&cell.0);
        let old = value.clone();
        return Some(Ok(match name {
            "countDown" => {
                *value = crate::sub(&old, &V::Int(1));
                V::Null
            }
            "getCount" => old,
            "await" => V::Bool(true),
            "compareAndSet" | "weakCompareAndSet" | "compareAndExchange" => {
                let same = crate::same(&old, &a[0]);
                if same {
                    *value = a[1].clone();
                }
                if name == "compareAndExchange" { old } else { V::Bool(same) }
            }
            "getAndIncrement" => {
                *value = crate::add(&old, &V::Int(1));
                old
            }
            "getAndDecrement" => {
                *value = crate::sub(&old, &V::Int(1));
                old
            }
            "incrementAndGet" => {
                *value = crate::add(&old, &V::Int(1));
                value.clone()
            }
            "decrementAndGet" => {
                *value = crate::sub(&old, &V::Int(1));
                value.clone()
            }
            "addAndGet" => {
                *value = crate::add(&old, &a[0]);
                value.clone()
            }
            "getAndAdd" => {
                *value = crate::add(&old, &a[0]);
                old
            }
            "getAndSet" => {
                *value = a[0].clone();
                old
            }
            _ => return None,
        }));
    }
    if matches!(this, V::Class(_)) {
        let V::Class(class) = this else { return None };
        return Some(Ok(match name {
            "getSimpleName" => s(class.rsplit('.').next().unwrap_or(class)),
            "getName" | "getTypeName" => s(&crate::java_class_name(class)),
            "toString" => s(&format!("class {}", crate::java_class_name(class))),
            // A class is its own type token: `TypeToken.get(Foo.class).getType()`.
            "getType" | "getRawType" if a.is_empty() => this.clone(),
            "isInstance" => V::Bool(crate::instance_of(&a[0], class)),
            "cast" => a[0].clone(),
            "getClassLoader" => native(JPlain),
            // Reflection finds the plugin's own methods; nothing else: Java's answer for a class that
            // isn't there, which plugins handle.
            "getMethod" | "getDeclaredMethod" => return Some(crate::reflect::find_method(class, a)),
            // A field of the plugin's own class (LZ4Factory's `getField("INSTANCE").get(null)`): read and written through
            // the class's statics or the object; anything else is not there, as Java says of a field it can't find.
            "getField" | "getDeclaredField" if a.len() == 1 => {
                let wanted = a[0].text();
                return Some(crate::reflect::plugin_field(class, &wanted).ok_or_else(|| exc("NoSuchFieldException", wanted)));
            }
            "getConstructor" | "getDeclaredConstructor" => return Some(crate::reflect::find_constructor(class, a)),
            // The deprecated `Foo.class.newInstance()`: the no-argument constructor of a plugin class.
            "newInstance" if a.is_empty() => {
                return Some(match crate::plugin_class(class) {
                    Some(k) => crate::reflect::instantiate(k, &[]),
                    None => Err(exc("InstantiationException", *class)),
                });
            }
            // `A.isAssignableFrom(B)`: B is A or a kind of A. `Number` takes the boxed numbers, `Object` everything,
            // a plugin type its subclasses (WorldEdit checks its expression functions return a Number).
            "isAssignableFrom" => V::Bool(match a.first() {
                Some(V::Class(other)) => {
                    other == class
                        || *class == "Object"
                        || (*class == "Number" && matches!(*other, "Double" | "Integer" | "Long" | "Float" | "Short" | "Byte" | "BigDecimal" | "BigInteger" | "AtomicInteger" | "AtomicLong"))
                        || (*class == "Comparable" && matches!(*other, "String" | "Double" | "Integer" | "Long" | "Float" | "Short" | "Byte" | "Character" | "Boolean"))
                        || (*class == "CharSequence" && matches!(*other, "String" | "StringBuilder" | "StringBuffer"))
                        || crate::plugin_class(other).is_some_and(|k| crate::has_type(k, class))
                }
                _ => false,
            }),
            "isArray" => V::Bool(class.ends_with("[]")),
            "getComponentType" if class.ends_with("[]") => V::Class(crate::intern(&class[..class.len() - 2])),
            "isPrimitive" => V::Bool(matches!(*class, "int" | "long" | "double" | "float" | "boolean" | "char" | "byte" | "short" | "void")),
            "isInterface" | "isEnum" => V::Bool(false),
            // A plugin class's own fields (and its parents' for getFields), as reflection-driven config code walks them.
            // Only when the converter's own field list for the class is empty: that list has a class's static fields
            // before it is initialised, which the field-type table does not (LuckPerms' ConfigKeys came back empty).
            "getFields" | "getDeclaredFields"
                if crate::plugin_class(class).is_some() && crate::hooks().and_then(|h| (h.class_fields)(class)).is_none_or(|fields| fields.is_empty()) =>
            {
                return Some(Ok(crate::reflect::plugin_fields(class, name == "getFields").unwrap_or_else(|| arr(Vec::new()))));
            }
            "getFields" | "getDeclaredFields" => arr(crate::hooks().and_then(|h| (h.class_fields)(class)).unwrap_or_default().into_iter().map(|(name, kind, value)| native(JField { name, kind, value, owner: class })).collect()),
            "equals" => V::Bool(matches!(a.first(), Some(V::Class(other)) if other == class)),
            // `X.class.getResource("X.class")`: where the class file is, a `jar:` URL into the plugin's jar, whose
            // connection reads the jar's manifest (WorldEdit's version and jar type).
            "getResource" if a.first().is_some_and(|n| n.text().ends_with(".class")) => {
                let name = a[0].text();
                let package = crate::java_class_name(class).rsplit_once('.').map(|(p, _)| p.replace('.', "/")).unwrap_or_default();
                let entry = if name.starts_with('/') { name.trim_start_matches('/').to_owned() } else if package.is_empty() { name } else { format!("{package}/{name}") };
                return Some(Ok(s(&format!("jar:{}!/{entry}", crate::javautil::plugin_jar_url()))));
            }
            "getProtectionDomain" if a.is_empty() => native(JCodeSource),
            "getResourceAsStream" | "getResource" => return call(&native(JPlain), name, a).into(),
            "getEnumConstants" => match crate::hooks().and_then(|h| (h.class_fields)(class)) {
                Some(rows) => arr(rows.into_iter().filter(|(_, kind, _)| *kind == *class).map(|(_, _, value)| value).collect()),
                None => call_static(class, "values", &[]).unwrap_or(V::Null),
            },
            "desiredAssertionStatus" => V::Bool(false),
            // The rest of what metrics, config and command libraries ask a class: answers that mean
            // "nothing special here", which they all handle.
            "getPackage" => native(JPackage),
            "getPackageName" => s(""),
            "getCanonicalName" => s(class),
            // `Object`'s is null, as Java's: a walk up the classes (`while (c != null) c = c.getSuperclass()`)
            // ends there. A plugin class's is its parent.
            "getSuperclass" => match *class {
                "Object" | "java.lang.Object" | "int" | "long" | "double" | "float" | "boolean" | "char" | "byte" | "short" | "void" => V::Null,
                _ => match crate::plugin_class(class) {
                    Some(own) => own.parent.map_or(V::Class("Object"), |parent| V::Class(parent().name)),
                    None => V::Class("Object"),
                },
            },
            "getGenericSuperclass" => crate::reflect::generic_superclass(class),
            "getModifiers" => V::Int(1),
            "getDeclaredMethods" | "getMethods" if a.is_empty() => crate::reflect::methods_of(class, name == "getDeclaredMethods"),
            // A plugin class's own interfaces (an event bus delivers to handlers of each supertype).
            "getInterfaces" if crate::plugin_class(class).is_some() => arr(crate::plugin_class(class).map(|k| k.interfaces.iter().map(|i| V::Class(i().name)).collect()).unwrap_or_default()),
            "getAnnotations" | "getDeclaredAnnotations" if a.is_empty() && crate::plugin_class(class).is_some() => return Some(crate::reflect::annotations_of(class)),
            "getInterfaces" | "getDeclaredMethods" | "getMethods" | "getDeclaredClasses" | "getAnnotations" | "getDeclaredAnnotations" | "getConstructors" | "getDeclaredConstructors" => arr(Vec::new()),
            // The plugin's own annotations on its classes (`@PerkInfo(name = ..)`), which the converter keeps.
            "getAnnotation" | "getDeclaredAnnotation" if !a.is_empty() => return Some(crate::reflect::class_annotation(class, &a[0])),
            "isAnnotationPresent" if !a.is_empty() => return Some(crate::reflect::class_annotation(class, &a[0]).map(|found| V::Bool(!found.is_null()))),
            "getAnnotation" | "getDeclaredAnnotation" | "getEnclosingClass" | "getDeclaringClass" | "getComponentType" => V::Null,
            "isAnnotationPresent" | "isAnonymousClass" | "isRecord" | "isSealed" | "isSynthetic" | "isAnnotation" | "isMemberClass" | "isLocalClass" => V::Bool(false),
            "asSubclass" => this.clone(),
            "hashCode" => V::Int(class.bytes().fold(0i32, |h, b| h.wrapping_mul(31).wrapping_add(b as i32))),
            _ => return None,
        }));
    }
    if this.native::<JPackage>().is_some() {
        return Some(Ok(match name {
            "getName" => s(""),
            "getImplementationVersion" | "getSpecificationVersion" | "getImplementationTitle" | "getImplementationVendor" => V::Null,
            "isSealed" => V::Bool(false),
            _ => return None,
        }));
    }
    if let Some(record) = this.native::<JLogRecord>() {
        return log_record_method(record, this, name, a);
    }
    if let V::Obj(object) = this {
        if name == "getClass" {
            return Some(Ok(V::Class(object.class.name)));
        }
        // A plugin's own class loader (`extends URLClassLoader`, a jar-in-jar loader): its classes are all here.
        if name == "loadClass" && !a.is_empty() && ["ClassLoader", "URLClassLoader", "SecureClassLoader"].iter().any(|k| crate::instance_of(this, k)) {
            return Some(call_static("Class", "forName", &a[..1]));
        }
    }
    if name == "getName" && matches!(this, V::Exc(_)) {
        return Some(Ok(s("Throwable")));
    }
    None
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(found) = plain_method(this, name, a) {
        return Some(found);
    }
    if let V::Native(native_value) = this {
        if let Ok(f) = native_value.clone().downcast::<JFuture>() {
            return future_method(&f, this, name, a);
        }
    }
    if let Some(j) = this.native::<JGJoiner>() {
        return guava_joiner_method(j, name, a);
    }
    if let Some(b) = this.native::<JStreamBuilder>() {
        return Some(match (name, a.len()) {
            ("add", 1) => {
                lock(&b.0).push(a[0].clone());
                Ok(this.clone())
            }
            ("accept", 1) => {
                lock(&b.0).push(a[0].clone());
                Ok(V::Null)
            }
            ("build", 0) => Ok(V::Stream(Arc::new(Mutex::new(std::mem::take(&mut *lock(&b.0)))))),
            _ => return None,
        });
    }
    if let Some(f) = this.native::<JFutureTask>() {
        return future_task_method(f, name, a);
    }
    if let Some(s) = this.native::<JSemaphore>() {
        return semaphore_method(s, name, a);
    }
    if let Some(l) = this.native::<JLock>() {
        return Some(match name {
            "lock" | "lockInterruptibly" => {
                l.acquire();
                Ok(V::Null)
            }
            "tryLock" => Ok(V::Bool(l.try_acquire())),
            "unlock" => l.release().map(|()| V::Null),
            "readLock" | "writeLock" => Ok(this.clone()),
            "isLocked" => Ok(V::Bool(lock(&l.state).0.is_some())),
            "isHeldByCurrentThread" => Ok(V::Bool(lock(&l.state).0 == Some(std::thread::current().id()))),
            _ => return None,
        });
    }
    if let Some(e) = this.native::<JExecutor>() {
        return executor_method(e, name, a);
    }
    if let Some(t) = this.native::<JThread>() {
        return thread_method(t, name, a);
    }
    if let Some(t) = this.native::<JThreadLocal>() {
        let key = format!("{:?}", std::thread::current().id());
        return Some(Ok(match name {
            "get" => {
                let mut values = lock(&t.values);
                if !values.contains_key(&key) {
                    let first = t.initial.as_ref().map_or(Ok(V::Null), |f| call(f, "get", &[]));
                    match first {
                        Ok(v) => {
                            values.insert(key.clone(), v);
                        }
                        Err(e) => return Some(Err(e)),
                    }
                }
                values.get(&key).cloned().unwrap_or(V::Null)
            }
            "set" => {
                lock(&t.values).insert(key, a[0].clone());
                V::Null
            }
            "remove" => {
                lock(&t.values).remove(&key);
                V::Null
            }
            _ => return None,
        }));
    }
    if let Some(p) = this.native::<JPattern>() {
        return pattern_method(p, name, a);
    }
    if let Some(m) = this.native::<JMatcher>() {
        // `matcher.replaceAll(match -> ...)`: each match's replacement from the function (given the matcher
        // as its MatchResult), `$1` and `\` read in it as Java does, through appendReplacement.
        if matches!(name, "replaceAll" | "replaceFirst") && a.len() == 1 && matches!(a[0], V::Func(_) | V::Obj(_)) {
            let _ = crate::call(this, "reset", &[]);
            let out = crate::new_builtin("StringBuilder", &[]).unwrap_or_else(|| Ok(V::Null)).ok()?;
            while crate::call(this, "find", &[]).map(|found| found.truth()).unwrap_or(false) {
                let with = match crate::call(&a[0], if matches!(a[0], V::Func(_)) { "$invoke" } else { "apply" }, std::slice::from_ref(this)) {
                    Ok(with) => with,
                    Err(thrown) => return Some(Err(thrown)),
                };
                if let Err(thrown) = crate::call(this, "appendReplacement", &[out.clone(), with]) {
                    return Some(Err(thrown));
                }
                if name == "replaceFirst" {
                    break;
                }
            }
            return Some(crate::call(this, "appendTail", std::slice::from_ref(&out)).map(|done| crate::s(&done.text())));
        }
        return matcher_method(m, name, a);
    }
    if let Some(i) = this.native::<JInstant>() {
        return instant_method(i.0, name, a);
    }
    if let Some(b) = this.native::<JBuilder>() {
        return builder_method(this, b, name, a);
    }
    if let Some(sp) = this.native::<JSplitter>() {
        return splitter_method(sp, name, a);
    }
    if let Some(gson) = this.native::<JGson>() {
        return gson_object_method(name, a, gson.0);
    }
    if matches!(this, V::Str(_)) {
        let text = this.text();
        match (name, a.len()) {
            ("matches", 1) => return Some(compiled(&a[0].text()).map(|r| V::Bool(native_full_match(&r, &text)))),
            ("replaceAll", 2) => return Some(compiled(&a[0].text()).map(|r| s(&replace_regex(&text, &r, &a[1].text(), true)))),
            ("replaceFirst", 2) => return Some(compiled(&a[0].text()).map(|r| s(&replace_regex(&text, &r, &a[1].text(), false)))),
            ("split", 1 | 2) => return Some(compiled(&a[0].text()).map(|r| arr(split_regex(&text, &r, a.get(1).map_or(0, V::int))))),
            _ => {}
        }
    }
    if matches!(this, V::Exc(_) | V::Obj(_)) {
        match name {
            "printStackTrace" => {
                let _ = crate::report("stack trace", Err(this.clone()));
                return Some(Ok(V::Null));
            }
            "addSuppressed" | "setStackTrace" | "initCause" => return Some(Ok(V::Null)),
            "getStackTrace" | "getSuppressed" => return Some(Ok(arr(Vec::new()))),
            _ => {}
        }
    }
    misc_method(this, name, a).or_else(|| gson_method(this, name, a))
}

fn native_full_match(regex: &Regex, text: &str) -> bool {
    regex.find(text).ok().flatten().is_some_and(|m| m.start() == 0 && m.end() == text.len())
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    let items = |at: usize| a.get(at).map_or(Ok(Vec::new()), V::items);
    let all = || -> Vec<V> {
        match a {
            [one] if matches!(one, V::Arr(_)) => one.items().unwrap_or_default(),
            _ => a.to_vec(),
        }
    };
    Some(match (class, name) {
        ("System", "getenv") if a.is_empty() => Ok(into_map(&std::env::vars().map(|(k, v)| V::Entry(Arc::new((s(&k), s(&v))))).collect::<Vec<_>>())),
        ("System", "getenv") => Ok(std::env::var(a[0].text()).map_or(V::Null, |v| s(&v))),
        // A server runs without an interactive console: Java answers null.
        ("System", "console") => Ok(V::Null),
        ("Long", "parseUnsignedLong") => a[0].text().parse::<u64>().map(|n| V::Long(n as i64)).map_err(|_| exc("NumberFormatException", a[0].text())),
        ("Boolean", "getBoolean") => Ok(V::Bool(false)),
        ("Entry", "comparingByKey") => Ok(func(|x| Ok(V::Int(match (&x[0], &x[1]) {
            (V::Entry(p), V::Entry(q)) => cmp_values(&p.0, &q.0),
            _ => 0,
        })))),
        ("Entry", "comparingByValue") => Ok(func(|x| Ok(V::Int(match (&x[0], &x[1]) {
            (V::Entry(p), V::Entry(q)) => cmp_values(&p.1, &q.1),
            _ => 0,
        })))),
        ("Stream", "empty") => Ok(V::Stream(Arc::new(Mutex::new(Vec::new())))),
        // `Stream.builder()`: `add`/`accept` items, then `build()` (WorldEdit's command registration).
        // Guava's `Joiner.on(separator)`.
        ("Joiner", "on") => Ok(native(JGJoiner { sep: a.first().map(V::text).unwrap_or_default(), skip_nulls: false, for_null: None })),
        ("Stream", "builder") => Ok(native(JStreamBuilder(Mutex::new(Vec::new())))),
        ("Stream", "concat") => match (a[0].items(), a[1].items()) {
            (Ok(mut x), Ok(y)) => {
                x.extend(y);
                Ok(V::Stream(Arc::new(Mutex::new(x))))
            }
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        ("StreamSupport", "stream") => items(0).map(|i| V::Stream(Arc::new(Mutex::new(i)))),
        ("Collections", "emptyIterator") => Ok(V::Iter(Arc::new(Mutex::new((Vec::new(), 0))))),
        ("EnumSet", "allOf") => {
            let V::Class(class) = a[0] else { return None };
            let rows = crate::hooks().and_then(|h| (h.class_fields)(class)).unwrap_or_default();
            Ok(into_set(&rows.into_iter().filter(|(_, kind, _)| *kind == class).map(|(_, _, value)| value).collect::<Vec<_>>()))
        }
        ("ImmutableSortedSet", "copyOf") => items(a.len() - 1).map(|i| into_set(&i)),
        ("ImmutableSortedMap", "copyOf" | "copyOfSorted") => call(a.last()?, "entrySet", &[]).and_then(|e| e.items()).map(|i| into_map(&i)),
        ("Lists" | "Iterables", "transform") => items(0).and_then(|i| i.into_iter().map(|v| call(&a[1], "$invoke", &[v])).collect::<Result<Vec<_>, V>>()).map(list),
        // `System.setProperty`: the plugin's own settings, kept for the process (metrics libraries set one).
        ("System", "setProperty") if a.len() == 2 => {
            let old = jvm_properties().insert(a[0].text(), a[1].text());
            Ok(old.map_or(V::Null, |v| s(&v)))
        }
        ("System", "clearProperty") => Ok(jvm_properties().remove(&a[0].text()).map_or(V::Null, |v| s(&v))),
        ("System", "getProperty") if jvm_properties().contains_key(&a[0].text()) => Ok(s(&jvm_properties()[&a[0].text()])),
        ("System", "getProperty") => Ok(match a[0].text().as_str() {
            "line.separator" => s("\n"),
            "file.separator" => s(std::path::MAIN_SEPARATOR_STR),
            "os.name" => s(if cfg!(windows) { "Windows 11" } else { "Linux" }),
            "java.version" => s("21"),
            "user.dir" => s(&std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()),
            // What every JVM sets: a library that reads one (Jansi's temp folder) got null and threw.
            "java.io.tmpdir" => s(&std::env::temp_dir().to_string_lossy()),
            "user.home" => s(&std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default()),
            "user.name" => s(&std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "server".to_owned())),
            "os.arch" => s(if cfg!(target_arch = "x86_64") { "amd64" } else if cfg!(target_arch = "aarch64") { "aarch64" } else { std::env::consts::ARCH }),
            "os.version" => s("10.0"),
            "path.separator" => s(if cfg!(windows) { ";" } else { ":" }),
            "file.encoding" | "sun.jnu.encoding" | "native.encoding" | "stdout.encoding" => s("UTF-8"),
            "java.vendor" | "java.vm.vendor" => s("Horizon"),
            "java.vm.name" => s("Horizon Rust"),
            "java.home" => s(&std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()),
            "java.specification.version" | "java.vm.specification.version" | "java.class.version" => s(if a[0].text() == "java.class.version" { "65.0" } else { "21" }),
            "java.runtime.version" | "java.vm.version" => s("21+35"),
            "java.class.path" => s(""),
            "sun.arch.data.model" => s("64"),
            _ => a.get(1).cloned().unwrap_or(V::Null),
        }),
        ("System", "identityHashCode") => Ok(V::Int(match &a[0] {
            V::Obj(o) => Arc::as_ptr(o) as usize as i32,
            other => other.text().bytes().fold(0, |h: i32, b| h.wrapping_mul(31).wrapping_add(i32::from(b))),
        })),
        ("System", "arraycopy") => {
            let (from, at, to, there, count) = (&a[0], a[1].int() as usize, &a[2], a[3].int() as usize, a[4].int() as usize);
            let items = from.items().ok()?;
            let Some(part) = items.get(at..at + count) else { return Some(Err(exc("ArrayIndexOutOfBoundsException", "arraycopy"))) };
            for (offset, value) in part.iter().enumerate() {
                if let Err(error) = crate::store(to, &V::Int((there + offset) as i32), value.clone()) {
                    return Some(Err(error));
                }
            }
            Ok(V::Null)
        }
        ("Math", "clamp") => Ok(if crate::cmp(&a[0], &a[1]).is_lt() { a[1].clone() } else if crate::cmp(&a[0], &a[2]).is_gt() { a[2].clone() } else { a[0].clone() }),
        ("String", "copyValueOf") => items(0).map(|i| s(&i.iter().map(V::text).collect::<String>())),
        ("Arrays", "fill") => {
            let n = a[0].items().ok()?.len();
            for at in 0..n {
                if let Err(error) = crate::store(&a[0], &V::Int(at as i32), a[1].clone()) {
                    return Some(Err(error));
                }
            }
            Ok(V::Null)
        }
        ("Collections", "shuffle") => {
            if let V::List(list) = &a[0] {
                let mut list = lock(list);
                let mut seed = now_ms() as u64 | 1;
                for at in (1..list.len()).rev() {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    list.swap(at, (seed % (at as u64 + 1)) as usize);
                }
            }
            Ok(V::Null)
        }
        ("UUID", "nameUUIDFromBytes") => {
            use md5::Digest;
            let mut digest = md5::Md5::digest(crate::javaio::bytes_of(&a[0]).unwrap_or_default());
            digest[6] = (digest[6] & 0x0f) | 0x30;
            digest[8] = (digest[8] & 0x3f) | 0x80;
            let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
            Ok(s(&format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])))
        }
        // Web editor signing and HTTP: out of reach for now, and nothing here needs them at startup.
        ("MediaType", "parse") | ("KeyPairGenerator" | "Signature" | "KeyFactory", "getInstance") => Ok(native(JStub)),
        ("Range", "closed") => Ok(native(JRange(a[0].clone(), a[1].clone()))),
        ("Iterators", "forArray") => Ok(V::Iter(Arc::new(Mutex::new((all(), 0))))),
        ("Iterators", "singletonIterator") => Ok(V::Iter(Arc::new(Mutex::new((a.to_vec(), 0))))),
        ("Iterators", "emptyIterator") => Ok(V::Iter(Arc::new(Mutex::new((Vec::new(), 0))))),
        ("Types", "asString") => Ok(if a[0].is_null() { V::Null } else { s(&a[0].text()) }),
        ("Types", "asInt") => Ok(V::Int(a[0].int())),
        ("Types", "asLong") => Ok(V::Long(a[0].long())),
        ("Types", "asBoolean") => Ok(V::Bool(a[0].truth())),
        ("Types", "strictAsString") => Ok(if matches!(a[0], V::Str(_)) { a[0].clone() } else { V::Null }),
        ("Types", "strictAsBoolean") => Ok(if matches!(a[0], V::Bool(_)) { a[0].clone() } else { V::Null }),
        ("LocalDate", "now") => Ok(native(JLocalDate(civil(now_ms().div_euclid(86_400_000))))),
        ("DateTimeFormatter", "ofPattern") => Ok(native(JDateFormat(a[0].text()))),
        // `URI.create(text)`: kept as its text, which is what plugins read back from it.
        ("URI", "create") => Ok(a[0].clone()),
        // Guava's cache builder, the same cache as Caffeine's.
        ("CacheBuilder", "newBuilder") => return crate::configurate::static_call("Caffeine", "newBuilder", a),
        // Guava's `MoreExecutors.listeningDecorator(pool)`: the pool, whose futures already listen.
        ("MoreExecutors", "listeningDecorator" | "getExitingExecutorService" | "getExitingScheduledExecutorService") => Ok(a[0].clone()),
        ("ZoneId", "systemDefault") => Ok(s("UTC")),
        ("ZoneId" | "ZoneOffset", "of") => Ok(a[0].clone()),
        ("Collections", "singletonMap") => Ok(into_map(&[V::Entry(Arc::new((a[0].clone(), a[1].clone())))])),
        ("Maps", "transformValues") => {
            let mut out = Vec::new();
            for entry in call(&a[0], "entrySet", &[]).and_then(|e| e.items()).ok()? {
                if let V::Entry(pair) = &entry {
                    out.push(V::Entry(Arc::new((pair.0.clone(), match call(&a[1], "$invoke", &[pair.1.clone()]) {
                        Ok(value) => value,
                        Err(error) => return Some(Err(error)),
                    }))));
                }
            }
            Ok(into_map(&out))
        }
        // Guava's `Maps.asMap(set, f)` / `Maps.toMap(keys, f)`: each key to f(key), in the keys' order (a copy, not a
        // live view: FAWE's BlockState.getStates() builds its property map this way).
        ("Maps", "asMap" | "toMap") if a.len() == 2 => {
            let mut out = Vec::new();
            for key in a[0].items().ok()? {
                match call(&a[1], "$invoke", std::slice::from_ref(&key)) {
                    Ok(value) => out.push(V::Entry(Arc::new((key, value)))),
                    Err(error) => return Some(Err(error)),
                }
            }
            Ok(into_map(&out))
        }
        ("Iterables", "addAll") => items(1).and_then(|more| {
            for item in more {
                call(&a[0], "add", &[item])?;
            }
            Ok(V::Bool(true))
        }),
        ("Arrays", "binarySearch") => items(0).map(|list| {
            let (key, by) = (a[1].clone(), a.get(2).cloned());
            let order = |x: &V| by.as_ref().map_or_else(|| cmp_values(x, &key), |f| call(f, "$invoke", &[x.clone(), key.clone()]).map_or(0, |c| c.int()));
            let (mut low, mut high) = (0i32, list.len() as i32 - 1);
            while low <= high {
                let mid = (low + high) / 2;
                match order(&list[mid as usize]) {
                    c if c < 0 => low = mid + 1,
                    c if c > 0 => high = mid - 1,
                    _ => return V::Int(mid),
                }
            }
            V::Int(-(low + 1))
        }),
        ("Collector", "of") => {
            let (make, add) = (a[0].clone(), a[1].clone());
            let finish = a.get(3).filter(|f| matches!(f, V::Func(_))).cloned();
            Ok(func(move |x| {
                let holder = call(&make, "$invoke", &[])?;
                for item in x[0].items()? {
                    call(&add, "$invoke", &[holder.clone(), item])?;
                }
                finish.as_ref().map_or(Ok(holder.clone()), |f| call(f, "$invoke", std::slice::from_ref(&holder)))
            }))
        }
        ("Collectors", "groupingBy") => {
            let key = a[0].clone();
            let downstream = a.last().filter(|_| a.len() > 1).cloned();
            Ok(func(move |x| {
                let mut groups: JMap = JMap::new();
                for item in x[0].items()? {
                    let k = call(&key, "$invoke", std::slice::from_ref(&item))?;
                    match groups.entry(k.key()).or_insert_with(|| (k, list(Vec::new()))) {
                        (_, V::List(members)) => lock(members).push(item),
                        _ => {}
                    }
                }
                if let Some(down) = &downstream {
                    for pair in groups.values_mut() {
                        pair.1 = call(&pair.1, "stream", &[]).and_then(|st| call(&st, "collect", std::slice::from_ref(down)))?;
                    }
                }
                Ok(V::Map(Arc::new(Mutex::new(groups))))
            }))
        }
        ("HashMultimap" | "LinkedHashMultimap", "create") => Ok(multimap(true)),
        ("ArrayListMultimap" | "LinkedListMultimap", "create") => Ok(multimap(false)),
        // Guava's `Multimaps.transformValues(multimap, fn)`: the same keys, each value through fn (WorldEdit's
        // expression functions, which also read numeric command arguments such as `//undo 3`).
        ("Multimaps", "transformValues") => {
            let set = a[0].native::<JMultimap>().is_some_and(|m| m.set);
            let made = multimap(set);
            let entries = call(&a[0], "entries", &[]).and_then(|e| e.items());
            match entries {
                Ok(entries) => {
                    for entry in entries {
                        let pair = call(&entry, "getKey", &[]).and_then(|k| call(&entry, "getValue", &[]).and_then(|v| call(&a[1], "apply", &[v])).map(|v| (k, v)));
                        if let Err(e) = pair.and_then(|(k, v)| call(&made, "put", &[k, v])) {
                            return Some(Err(e));
                        }
                    }
                    Ok(made)
                }
                Err(e) => Err(e),
            }
        }
        ("Multimaps", "synchronizedSetMultimap" | "synchronizedMultimap" | "unmodifiableSetMultimap" | "unmodifiableMultimap") => Ok(a[0].clone()),
        ("Multimaps", "newSetMultimap") => Ok(multimap(true)),
        ("Multimaps", "asMap") => call(&a[0], "asMap", &[]),
        ("ImmutableSetMultimap", "builder" | "of") => Ok(multimap(true)),
        ("ImmutableListMultimap", "builder" | "of") => Ok(multimap(false)),
        ("ImmutableSetMultimap" | "ImmutableListMultimap", "copyOf") => Ok(a[0].clone()),
        // `Modifier.isX(m)`: the bit, as Java's (PUBLIC 1, PRIVATE 2, PROTECTED 4, STATIC 8, FINAL 16 ...).
        ("Modifier", check) if check.starts_with("is") && a.len() == 1 && matches!(a[0], V::Int(_)) => {
            let bit = match check {
                "isPublic" => 0x1,
                "isPrivate" => 0x2,
                "isProtected" => 0x4,
                "isStatic" => 0x8,
                "isFinal" => 0x10,
                "isSynchronized" => 0x20,
                "isVolatile" => 0x40,
                "isTransient" => 0x80,
                "isNative" => 0x100,
                "isInterface" => 0x200,
                "isAbstract" => 0x400,
                "isStrict" => 0x800,
                _ => return Some(Ok(V::Bool(false))),
            };
            Ok(V::Bool(a[0].int() & bit != 0))
        }
        ("Modifier", "isStatic" | "isPublic" | "isFinal") => Ok(V::Bool(true)),
        // A class loader's static `registerAsParallelCapable()`: there is no class loading to make parallel.
        ("ClassLoader" | "URLClassLoader" | "SecureClassLoader", "registerAsParallelCapable") => Ok(V::Bool(true)),
        ("ClassLoader", "getSystemClassLoader" | "getPlatformClassLoader") => Ok(native(JPlain)),
        ("Class", "forName") => {
            let wanted = a[0].text();
            // One of the plugin's own classes, by its full name: all of them are here, loaded or not.
            if wanted.contains('.') {
                if let Some(own) = crate::class_named(&wanted) {
                    return Some(Ok(V::Class(own.name)));
                }
            }
            // Paper ships the SQLite driver, and HikariCP comes in through a plugin's `libraries`: both are here.
            // A plugin's own relocated copy of them too: `com.x.libs.sqlite.JDBC` is the same driver.
            let relocated = wanted.ends_with(".sqlite.JDBC") || wanted.contains(".hikari.");
            let present = relocated || (["java.", "javax.", "com.google.", "org.bukkit.", "net.kyori.", "com.destroystokyo.paper.", "io.papermc.paper.", "org.sqlite.", "com.zaxxer.hikari."].iter().any(|p| wanted.starts_with(p)) && !wanted.contains("threadedregions") && !wanted.contains("craftbukkit") && !wanted.contains("net.minecraft"));
            // Another plugin's class (Vault's Economy, PlaceholderAPI's expansion): on Paper the class path is shared.
            let present = present || crate::class_elsewhere(&wanted);
            if present { Ok(V::Class(Box::leak(wanted.rsplit('.').next().unwrap_or("").to_owned().into_boxed_str()))) } else { Err(exc("ClassNotFoundException", wanted)) }
        }
        ("Locale", "getDefault") => Ok(s("en")),
        ("Executors", "defaultThreadFactory") => Ok(native(JStub)),
        ("Thread", "getAllStackTraces") => Ok(V::Map(Arc::new(Mutex::new(JMap::new())))),
        ("Objects", "nonNull") => Ok(V::Bool(!a[0].is_null())),
        ("Objects", "hash") => Ok(V::Int(a.iter().fold(1, |h: i32, v| h.wrapping_mul(31).wrapping_add(hash_of(v))))),
        ("Arrays", "hashCode") => items(0).map(|i| V::Int(i.iter().fold(1, |h: i32, v| h.wrapping_mul(31).wrapping_add(hash_of(v))))),
        ("Arrays", "equals") => match (a[0].items(), a[1].items()) {
            (Ok(x), Ok(y)) => Ok(V::Bool(x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| crate::same(p, q)))),
            _ => Ok(V::Bool(a[0].is_null() && a[1].is_null())),
        },
        ("Integer" | "Long", "toHexString") => Ok(s(&format!("{:x}", a[0].long()))),
        ("Integer" | "Long", "toBinaryString") => Ok(s(&format!("{:b}", a[0].long()))),
        ("Character", "toChars") => Ok(arr(char::from_u32(a[0].int() as u32).map(V::Char).into_iter().collect())),
        ("Runtime", "getRuntime") => Ok(native(JRuntime)),
        // The Java a plugin believes it runs on: 21, what Paper 1.21 asks for (FAWE's vector-API check reads it).
        ("Runtime", "version") => Ok(native(JRuntimeVersion)),
        ("ByteBuffer", "allocate" | "allocateDirect") => Ok(native(JByteBuffer(Mutex::new((vec![0; a[0].int().max(0) as usize], 0, a[0].int().max(0) as usize))))),
        ("ByteBuffer", "wrap") => {
            let bytes = crate::javaio::bytes_of(&a[0]).unwrap_or_default();
            let n = bytes.len();
            Ok(native(JByteBuffer(Mutex::new((bytes, 0, n)))))
        }
        ("Comparator", "comparing" | "comparingInt" | "comparingLong" | "comparingDouble") => {
            let key = a[0].clone();
            Ok(func(move |x| {
                let (p, q) = (call(&key, "apply", &x[..1])?, call(&key, "apply", &x[1..2])?);
                Ok(V::Int(cmp_values(&p, &q)))
            }))
        }
        ("Collectors", "toMap") => {
            let (key, value) = (a[0].clone(), a[1].clone());
            Ok(func(move |x| {
                let mut out = JMap::new();
                for item in x[0].items()? {
                    let k = call(&key, "apply", std::slice::from_ref(&item))?;
                    let v = call(&value, "apply", std::slice::from_ref(&item))?;
                    out.insert(k.key(), (k, v));
                }
                Ok(V::Map(Arc::new(Mutex::new(out))))
            }))
        }
        ("Base64", "getEncoder" | "getDecoder" | "getMimeEncoder" | "getMimeDecoder") => Ok(native(JB64 { url: false, pad: true })),
        ("Base64", "getUrlEncoder" | "getUrlDecoder") => Ok(native(JB64 { url: true, pad: true })),
        ("Pattern", "compile") => pattern(&a[0].text(), a.get(1).map_or(0, V::int)),
        ("Pattern", "quote") => Ok(s(&fancy_regex::escape(&a[0].text()))),
        ("Pattern", "matches") => compiled(&a[0].text()).map(|r| V::Bool(native_full_match(&r, &a[1].text()))),
        ("Matcher", "quoteReplacement") => Ok(s(&a[0].text().replace('\\', "\\\\").replace('$', "\\$"))),
        ("Instant", "now") => Ok(instant(now_ms())),
        ("Instant", "ofEpochSecond") => Ok(instant(a[0].long() * 1000)),
        ("Instant", "ofEpochMilli") => Ok(instant(a[0].long())),
        ("Duration", "between") => {
            let at = |v: &V| v.native::<JInstant>().map(|i| i.0).or_else(|| crate::jtime::millis(v)).unwrap_or(0);
            Ok(native(JDuration(at(&a[1]) - at(&a[0]))))
        }
        ("Thread", "sleep") => {
            std::thread::sleep(std::time::Duration::from_millis(a[0].long().max(0) as u64));
            Ok(V::Null)
        }
        // One Thread object per OS thread, as Java has: FAWE keeps `Thread.currentThread()` and later asks
        // `thread == Thread.currentThread()` (Fawe.isMainThread), and keys its per-thread Request map by it.
        ("Thread", "currentThread") => {
            thread_local! { static CURRENT: std::cell::OnceCell<V> = const { std::cell::OnceCell::new() }; }
            Ok(CURRENT.with(|c| c.get_or_init(|| native(JThread { task: Mutex::new(None), name: Mutex::new(format!("Thread-{:?}", std::thread::current().id())), handle: Mutex::new(None), interrupted: Default::default() })).clone()))
        }
        ("Thread", "interrupted") => Ok(V::Bool(false)),
        ("ThreadLocal", "withInitial") => Ok(native(JThreadLocal { values: Mutex::new(BTreeMap::new()), initial: Some(a[0].clone()) })),
        ("CompletableFuture", "completedFuture") => Ok(future_v(future(Some(Ok(a[0].clone()))))),
        // Guava's done futures: the same done future.
        ("Futures", "immediateFuture") => Ok(future_v(future(Some(Ok(a[0].clone()))))),
        ("Futures", "immediateVoidFuture") => Ok(future_v(future(Some(Ok(V::Null))))),
        ("Futures", "immediateFailedFuture") => Ok(future_v(future(Some(Err(a[0].clone()))))),
        ("Futures", "immediateCancelledFuture") => Ok(future_v(future(Some(Err(exc("CancellationException", "cancelled")))))),
        ("Futures", "getUnchecked" | "getDone") => call(&a[0], "get", &[]),
        // `Futures.catching(future, Type.class, fallback, executor)`: its value, or the fallback's for a failure of
        // that type. (`catchingAsync`'s fallback gives a future.)
        ("Futures", "catching" | "catchingAsync") if a.len() >= 3 => match call(&a[0], "get", &[]) {
            Ok(value) => Ok(future_v(future(Some(Ok(value))))),
            Err(thrown) => {
                let cause = if crate::instance_of(&thrown, "ExecutionException") { call(&thrown, "getCause", &[]).unwrap_or(thrown.clone()) } else { thrown.clone() };
                let wanted = match &a[1] { V::Class(c) => crate::instance_of(&cause, c), _ => true };
                if !wanted {
                    return Some(Ok(future_v(future(Some(Err(thrown))))));
                }
                match call(&a[2], "$invoke", &[cause]) {
                    Ok(value) if name == "catchingAsync" => Ok(value),
                    Ok(value) => Ok(future_v(future(Some(Ok(value))))),
                    Err(e) => Ok(future_v(future(Some(Err(e))))),
                }
            }
        },
        ("CompletableFuture", "failedFuture") => Ok(future_v(future(Some(Err(a[0].clone()))))),
        ("CompletableFuture", "supplyAsync" | "runAsync") => {
            let out = future(None);
            let done = out.clone();
            let task = a[0].clone();
            let method = if name == "runAsync" { "run" } else { "get" };
            spawn(move || {
                done.finish(traced(call(&task, method, &[])));
            });
            Ok(future_v(out))
        }
        ("CompletableFuture", "allOf") => {
            let parts = all();
            let out = future(None);
            let done = out.clone();
            spawn(move || {
                let mut result = Ok(V::Null);
                for part in &parts {
                    if let Err(error) = call(part, "join", &[]) {
                        result = Err(error);
                    }
                }
                done.finish(result);
            });
            Ok(future_v(out))
        }
        ("Executors", _) => Ok(native(JExecutor { stopped: Default::default() })),
        ("ForkJoinPool", "commonPool") => Ok(native(JExecutor { stopped: Default::default() })),
        // No work-stealing pool: "plenty queued here", so ForkJoin code that asks (FAWE's ApplyTask) works the part
        // out where it is instead of forking it.
        ("ForkJoinTask", "getSurplusQueuedTaskCount") => Ok(V::Int(i32::MAX)),
        ("ForkJoinTask", "inForkJoinPool") => Ok(V::Bool(false)),
        ("ForkJoinTask", "getQueuedTaskCount") => Ok(V::Int(0)),
        // Guava's `of(e1, ..., e12, E... others)`: past twelve, the rest come as one array, spread out here.
        ("ImmutableList", "of") if a.len() >= 13 && matches!(a.last(), Some(V::Arr(_))) => {
            let mut all = a[..a.len() - 1].to_vec();
            all.extend(a[a.len() - 1].items().unwrap_or_default());
            Ok(list(all))
        }
        ("ImmutableList", "of") => Ok(list(a.to_vec())),
        ("Lists", "newArrayList") if !matches!(a, [one] if !matches!(one, V::Str(_) | V::Int(_) | V::Obj(_))) => Ok(list(a.to_vec())),
        ("ImmutableList" | "Lists", "copyOf" | "newArrayList" | "newLinkedList") => items(0).map(list),
        ("ImmutableList", "builder") => Ok(builder("list")),
        // `ImmutableSet.of(e1, ..., e6, E... others)`.
        ("ImmutableSet", "of") if a.len() >= 7 && matches!(a.last(), Some(V::Arr(_))) => {
            let mut all = a[..a.len() - 1].to_vec();
            all.extend(a[a.len() - 1].items().unwrap_or_default());
            Ok(into_set(&all))
        }
        ("ImmutableSet", "of") => Ok(into_set(a)),
        // Guava's `Sets.immutableEnumSet(a, b...)` / `(iterable)`: a set of them, in their order.
        ("Sets", "immutableEnumSet") => {
            let items = match a {
                [one] if !matches!(one, V::Str(_) | V::Obj(_)) => one.items().unwrap_or_else(|_| vec![one.clone()]),
                many => many.iter().flat_map(|x| if matches!(x, V::Arr(_)) { x.items().unwrap_or_default() } else { vec![x.clone()] }).collect(),
            };
            Ok(into_set(&items))
        }
        ("Sets", "newHashSet") if a.len() == 1 && !matches!(a[0], V::Str(_) | V::Int(_) | V::Obj(_)) => {
            items(0).map(|i| crate::new_builtin("HashSet", &[list(i)]).and_then(Result::ok).unwrap_or(V::Null))
        }
        ("ImmutableSet" | "Sets", "copyOf" | "newHashSet" | "newLinkedHashSet") if a.len() == 1 && !matches!(a[0], V::Str(_) | V::Int(_) | V::Obj(_)) => items(0).map(|i| into_set(&i)),
        ("Sets", "newHashSet" | "newLinkedHashSet" | "newConcurrentHashSet") => Ok(into_set(a)),
        ("ImmutableSet", "builder") => Ok(builder("set")),
        ("ImmutableMap", "of") => Ok(into_map(&a.chunks(2).map(|p| V::Entry(Arc::new((p[0].clone(), p[1].clone())))).collect::<Vec<_>>())),
        ("ImmutableMap", "copyOf") => call(&a[0], "entrySet", &[]).and_then(|e| e.items()).map(|i| into_map(&i)),
        ("ImmutableMap", "builder") => Ok(builder("map")),
        ("Maps", "immutableEntry") => Ok(V::Entry(Arc::new((a[0].clone(), a[1].clone())))),
        ("Maps", "newHashMap" | "newConcurrentMap") => Ok(V::Map(Arc::new(Mutex::new(JMap::with_order(crate::Order::Hash))))),
        ("Maps", "newTreeMap") => Ok(V::Map(Arc::new(Mutex::new(JMap::with_order(crate::Order::Sorted))))),
        ("Maps", "newLinkedHashMap") => Ok(V::Map(Arc::new(Mutex::new(JMap::new())))),
        ("Preconditions", "checkNotNull") if a[0].is_null() => Err(exc("NullPointerException", a.get(1).map_or_else(String::new, V::text))),
        ("Preconditions", "checkNotNull") => Ok(a[0].clone()),
        ("Preconditions", "checkArgument") if !a[0].truth() => Err(exc("IllegalArgumentException", a.get(1).map_or_else(String::new, V::text))),
        ("Preconditions", "checkState") if !a[0].truth() => Err(exc("IllegalStateException", a.get(1).map_or_else(String::new, V::text))),
        ("Preconditions", "checkArgument" | "checkState") => Ok(V::Null),
        ("Splitter", "onPattern") if a.len() == 1 => Ok(splitter(JSplitter { on: a[0].text(), any: false, limit: None, omit: false, trim: false, pair: None, pattern: true })),
        ("Splitter", "on") => Ok(splitter(JSplitter { on: a[0].native::<JCharMatcher>().map_or_else(|| a[0].text(), |m| m.0.clone()), any: a[0].native::<JCharMatcher>().is_some(), limit: None, omit: false, trim: false, pair: None, pattern: false })),
        ("CharMatcher", "anyOf") => Ok(native(JCharMatcher(a[0].text()))),
        ("Iterables", "getFirst") => items(0).map(|i| i.first().cloned().unwrap_or_else(|| a[1].clone())),
        ("Iterables", "getLast") => items(0).and_then(|i| i.last().cloned().or_else(|| a.get(1).cloned()).ok_or_else(|| exc("NoSuchElementException", "empty"))),
        ("Iterables", "size") => items(0).map(|i| V::Int(i.len() as i32)),
        ("Iterables" | "Iterators", "limit") => items(0).map(|i| list(i.into_iter().take(a[1].int().max(0) as usize).collect())),
        ("Iterables", "isEmpty") => items(0).map(|i| V::Bool(i.is_empty())),
        ("Strings", "isNullOrEmpty") => Ok(V::Bool(a[0].is_null() || a[0].text().is_empty())),
        ("Strings", "nullToEmpty") => Ok(if a[0].is_null() { s("") } else { a[0].clone() }),
        ("Strings", "repeat") => Ok(s(&a[0].text().repeat(a[1].int().max(0) as usize))),
        ("EnumSet", "of") => Ok(into_set(a)),
        ("EnumSet", "noneOf") => Ok(into_set(&[])),
        ("EnumSet", "copyOf") => items(0).map(|i| into_set(&i)),
        ("JsonParser", "parseString" | "parseReader") => gson_object_method("fromJson", a, false)?,
        ("Function", "identity") | ("UnaryOperator", "identity") => Ok(func(|x| Ok(x[0].clone()))),
        ("ThreadLocalRandom", "current") => crate::stdlib::new_builtin("Random", &[])?,
        ("Long", "hashCode") => Ok(V::Int((a[0].long() ^ (a[0].long() >> 32)) as i32)),
        ("Boolean", "compare") => Ok(V::Int(a[0].truth().cmp(&a[1].truth()) as i32)),
        ("OptionalInt", "empty") => Ok(V::Opt(Arc::new(None))),
        ("OptionalInt", "of") => Ok(V::Opt(Arc::new(Some(a[0].clone())))),
        ("ConcurrentHashMap", "newKeySet") | ("Collections", "newSetFromMap") => Ok(into_set(&[])),
        ("Collections", "synchronizedSet" | "synchronizedList" | "synchronizedMap" | "synchronizedCollection" | "unmodifiableSortedSet" | "unmodifiableSortedMap") => Ok(a[0].clone()),
        ("Comparator", "reverseOrder") => Ok(func(|x| Ok(V::Int(cmp_values(&x[1], &x[0]))))),
        ("Comparator", "naturalOrder") => Ok(func(|x| Ok(V::Int(cmp_values(&x[0], &x[1]))))),
        _ => return None,
    })
}

pub struct JRuntime;
pub struct JRuntimeVersion;
/// `java.nio.ByteBuffer`, big-endian: the bytes, the position and the limit. `array()` is a copy of the bytes.
pub struct JByteBuffer(Mutex<(Vec<u8>, usize, usize)>);

fn byte_buffer_method(this: &V, b: &JByteBuffer, name: &str, a: &[V]) -> Option<R> {
    let mut st = lock(&b.0);
    let (bytes, pos, limit) = &mut *st;
    let width = |name: &str| match name {
        "put" | "get" => 1,
        "putShort" | "getShort" | "putChar" | "getChar" => 2,
        "putInt" | "getInt" | "putFloat" | "getFloat" => 4,
        _ => 8,
    };
    let overflow = |what| exc(what, "buffer");
    Some(Ok(match (name, a.len()) {
        ("array", 0) => crate::javaio::to_byte_array(bytes),
        ("position", 0) => V::Int(*pos as i32),
        ("position", 1) => {
            *pos = a[0].int().max(0) as usize;
            this.clone()
        }
        ("limit", 0) => V::Int(*limit as i32),
        ("limit", 1) => {
            *limit = (a[0].int().max(0) as usize).min(bytes.len());
            this.clone()
        }
        ("capacity", 0) => V::Int(bytes.len() as i32),
        ("remaining", 0) => V::Int(limit.saturating_sub(*pos) as i32),
        ("hasRemaining", 0) => V::Bool(*pos < *limit),
        ("flip", 0) => {
            *limit = *pos;
            *pos = 0;
            this.clone()
        }
        ("rewind", 0) => {
            *pos = 0;
            this.clone()
        }
        ("clear", 0) => {
            *pos = 0;
            *limit = bytes.len();
            this.clone()
        }
        ("order" | "mark" | "reset" | "duplicate" | "asReadOnlyBuffer", _) => this.clone(),
        ("put", 1) if matches!(a[0], V::Arr(_)) => {
            let more = crate::javaio::bytes_of(&a[0]).unwrap_or_default();
            if *pos + more.len() > *limit {
                return Some(Err(overflow("BufferOverflowException")));
            }
            bytes[*pos..*pos + more.len()].copy_from_slice(&more);
            *pos += more.len();
            this.clone()
        }
        ("put" | "putShort" | "putChar" | "putInt" | "putLong" | "putFloat" | "putDouble", 1 | 2) => {
            let n = width(name);
            let value = &a[a.len() - 1];
            let raw = match name {
                "putFloat" => u64::from((value.double() as f32).to_bits()),
                "putDouble" => value.double().to_bits(),
                _ => value.long() as u64,
            };
            let at = if a.len() == 2 { a[0].int().max(0) as usize } else { *pos };
            if at + n > *limit {
                return Some(Err(overflow("BufferOverflowException")));
            }
            for i in 0..n {
                bytes[at + i] = (raw >> (8 * (n - 1 - i))) as u8;
            }
            if a.len() == 1 {
                *pos += n;
            }
            this.clone()
        }
        ("get" | "getShort" | "getChar" | "getInt" | "getLong" | "getFloat" | "getDouble", 0 | 1) if !matches!(a.first(), Some(V::Arr(_))) => {
            let n = width(name);
            let at = if a.len() == 1 { a[0].int().max(0) as usize } else { *pos };
            if at + n > *limit {
                return Some(Err(overflow("BufferUnderflowException")));
            }
            let raw = (0..n).fold(0u64, |acc, i| (acc << 8) | u64::from(bytes[at + i]));
            if a.is_empty() {
                *pos += n;
            }
            match name {
                "get" => V::Int(i32::from(raw as u8 as i8)),
                "getShort" => V::Int(i32::from(raw as u16 as i16)),
                "getChar" => V::Char(char::from_u32(raw as u32).unwrap_or(' ')),
                "getInt" => V::Int(raw as u32 as i32),
                "getFloat" => V::Double(f64::from(f32::from_bits(raw as u32))),
                "getDouble" => V::Double(f64::from_bits(raw)),
                _ => V::Long(raw as i64),
            }
        }
        _ => return None,
    }))
}

fn hash_of(value: &V) -> i32 {
    call(value, "hashCode", &[]).map_or(0, |h| h.int())
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = u32::from(chunk[0]) << 16 | u32::from(*chunk.get(1).unwrap_or(&0)) << 8 | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            out.push(if i <= chunk.len() { BASE64[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// `Base64.getEncoder()` and its kin: `getUrlEncoder` (`-_` for `+/`), `withoutPadding()` (no `=`).
pub struct JB64 {
    url: bool,
    pad: bool,
}

fn b64_encode(coder: &JB64, bytes: &[u8]) -> String {
    let mut out = base64_encode(bytes);
    if coder.url {
        out = out.replace('+', "-").replace('/', "_");
    }
    if !coder.pad {
        out.truncate(out.trim_end_matches('=').len());
    }
    out
}

fn b64_method(coder: &JB64, name: &str, a: &[V]) -> Option<R> {
    let bytes = |v: &V| crate::javaio::bytes_of(v).unwrap_or_else(|| v.text().into_bytes());
    Some(Ok(match name {
        "encodeToString" => s(&b64_encode(coder, &bytes(&a[0]))),
        "encode" => crate::javaio::to_byte_array(b64_encode(coder, &bytes(&a[0])).as_bytes()),
        "decode" => crate::javaio::to_byte_array(&base64_decode(&String::from_utf8_lossy(&bytes(&a[0])))),
        "withoutPadding" => native(JB64 { url: coder.url, pad: false }),
        _ => return None,
    }))
}

fn base64_decode(text: &str) -> Vec<u8> {
    let (mut out, mut buffer, mut bits) = (Vec::new(), 0u32, 0);
    for byte in text.bytes() {
        // The URL alphabet too: `-` and `_` are `+` and `/`.
        let byte = match byte {
            b'-' => b'+',
            b'_' => b'/',
            other => other,
        };
        let Some(at) = BASE64.iter().position(|b| *b == byte) else { continue };
        buffer = buffer << 6 | at as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

fn value_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    match (this, name, a.len()) {
        (V::Int(_) | V::Long(_) | V::Double(_), "shortValue", 0) => Some(Ok(V::Int(i32::from(this.long() as i16)))),
        (V::Int(_) | V::Long(_) | V::Double(_), "byteValue", 0) => Some(Ok(V::Int(i32::from(this.long() as i8)))),
        (V::Str(unit), "toNanos" | "toMicros" | "toMillis" | "toSeconds" | "toMinutes" | "toHours" | "toDays", 1) => {
            let per = |u: &str| -> i128 {
                match u {
                    "NANOSECONDS" => 1,
                    "MICROSECONDS" => 1_000,
                    "MILLISECONDS" => 1_000_000,
                    "SECONDS" => 1_000_000_000,
                    "MINUTES" => 60_000_000_000,
                    "HOURS" => 3_600_000_000_000,
                    "DAYS" => 86_400_000_000_000,
                    _ => 0,
                }
            };
            let to = match name {
                "toNanos" => "NANOSECONDS",
                "toMicros" => "MICROSECONDS",
                "toMillis" => "MILLISECONDS",
                "toSeconds" => "SECONDS",
                "toMinutes" => "MINUTES",
                "toHours" => "HOURS",
                _ => "DAYS",
            };
            (per(unit) > 0).then(|| Ok(V::Long((i128::from(a[0].long()) * per(unit) / per(to)) as i64)))
        }
        // `unit.convert(amount, sourceUnit)`: the amount in this unit.
        (V::Str(unit), "convert", 2) if matches!(a[1], V::Str(_)) => {
            let source = a[1].text();
            let name = match &**unit {
                "NANOSECONDS" => "toNanos",
                "MICROSECONDS" => "toMicros",
                "MILLISECONDS" => "toMillis",
                "SECONDS" => "toSeconds",
                "MINUTES" => "toMinutes",
                "HOURS" => "toHours",
                "DAYS" => "toDays",
                _ => return None,
            };
            value_method(&s(&source), name, &a[..1])
        }
        (V::Str(_), "name", 0) => Some(Ok(this.clone())),
        (V::Str(unit), "getDuration", 0) => {
            let ms: i64 = match &**unit {
                "MILLIS" => 1,
                "SECONDS" => 1_000,
                "MINUTES" => 60_000,
                "HOURS" => 3_600_000,
                "DAYS" => 86_400_000,
                "WEEKS" => 604_800_000,
                "MONTHS" => 2_629_746_000,
                "YEARS" => 31_556_952_000,
                _ => return None,
            };
            Some(Ok(native(JDuration(ms))))
        }
        (V::Str(_) | V::Sb(_), "chars", 0) => Some(Ok(V::Stream(Arc::new(Mutex::new(this.text().chars().map(|c| V::Int(c as i32)).collect()))))),
        (V::Sb(text), "setCharAt", 2) => {
            let mut chars: Vec<char> = lock(text).chars().collect();
            if let Some(slot) = usize::try_from(a[0].int()).ok().and_then(|i| chars.get_mut(i)) {
                *slot = a[1].text().chars().next().unwrap_or(' ');
            }
            *lock(text) = chars.into_iter().collect();
            Some(Ok(V::Null))
        }
        (V::List(_) | V::Set(_), "retainAll", 1) => {
            let items = match this.items() {
                Ok(items) => items,
                Err(error) => return Some(Err(error)),
            };
            let mut changed = false;
            for item in items {
                let keep = call(&a[0], "contains", std::slice::from_ref(&item)).is_ok_and(|c| c.truth());
                if !keep {
                    changed = true;
                    if let Err(error) = call(this, "remove", std::slice::from_ref(&item)) {
                        return Some(Err(error));
                    }
                }
            }
            Some(Ok(V::Bool(changed)))
        }
        // `set.removeAll(other)` / `list.removeAll(other)`: every element the other collection has goes.
        (V::List(_) | V::Set(_), "removeAll", 1) => {
            let gone = match a[0].items() {
                Ok(gone) => gone,
                Err(error) => return Some(Err(error)),
            };
            let changed = match this {
                V::List(list_items) => {
                    let mut g = lock(list_items);
                    let before = g.len();
                    g.retain(|x| !gone.iter().any(|y| same(x, y)));
                    g.len() != before
                }
                V::Set(map) => {
                    let mut g = lock(map);
                    gone.iter().fold(false, |changed, y| g.remove(&y.key()).is_some() || changed)
                }
                _ => false,
            };
            Some(Ok(V::Bool(changed)))
        }
        (V::Stream(items), "reduce", 1 | 2) => {
            let (first, step) = if a.len() == 2 { (Some(a[0].clone()), &a[1]) } else { (None, &a[0]) };
            let mut total = first.clone();
            for item in lock(items).clone() {
                total = Some(match total {
                    Some(value) => match call(step, "$invoke", &[value, item]) {
                        Ok(next) => next,
                        Err(error) => return Some(Err(error)),
                    },
                    None => item,
                });
            }
            Some(Ok(if first.is_some() { total.unwrap_or(V::Null) } else { V::Opt(Arc::new(total)) }))
        }
        _ => None,
    }
}

fn misc_method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(found) = value_method(this, name, a) {
        return Some(found);
    }
    if let Some(b) = this.native::<JByteBuffer>() {
        return byte_buffer_method(this, b, name, a);
    }
    if this.native::<JRuntimeVersion>().is_some() {
        return Some(Ok(match name {
            "feature" | "major" => V::Int(21),
            "interim" | "minor" | "update" | "security" | "patch" => V::Int(0),
            "toString" => s("21"),
            _ => return None,
        }));
    }
    if this.native::<JRuntime>().is_some() {
        return Some(Ok(match name {
            "availableProcessors" => V::Int(std::thread::available_parallelism().map_or(1, |n| n.get() as i32)),
            "totalMemory" | "maxMemory" => V::Long(4_294_967_296),
            "freeMemory" => V::Long(2_147_483_648),
            "gc" | "addShutdownHook" => V::Null,
            _ => return None,
        }));
    }
    if let Some(coder) = this.native::<JB64>() {
        return b64_method(coder, name, a);
    }
    if this.native::<JCodeSource>().is_some() {
        return match name {
            "getCodeSource" => Some(Ok(this.clone())),
            // Where the classes came from: the server's plugins folder, as a `file:` URL of a jar there.
            "getLocation" => Some(Ok(s(&plugin_jar_url()))),
            "getCertificates" | "getCodeSigners" => Some(Ok(V::Null)),
            "getClassLoader" => Some(Ok(native(JPlain))),
            "getPermissions" => Some(Ok(V::Null)),
            "implies" => Some(Ok(V::Bool(true))),
            "toString" => Some(Ok(s("ProtectionDomain"))),
            _ => None,
        };
    }
    if this.native::<JPlain>().is_some() {
        return match name {
            "getResourceAsStream" | "getResource" => Some(Ok(V::Null)),
            // No jars on a class path here: an empty `Enumeration<URL>` (FAWE's PackageChecker walks them).
            "getResources" => Some(call(&crate::list(Vec::new()), "iterator", &[])),
            // One class loader for everything here: the plugin's is the system's (`loader != getSystemClassLoader()`
            // is false), which `==` reads through this text.
            "toString" => Some(Ok(s("ClassLoader"))),
            // `getClassLoader().loadClass(name)`: as `Class.forName`.
            "loadClass" if !a.is_empty() => Some(call_static("Class", "forName", &a[..1])),
            "hashCode" => Some(Ok(V::Int(0))),
            _ => None,
        };
    }
    if name == "parallelStream" {
        return Some(call(this, "stream", &[]));
    }
    if let V::Str(text) = this {
        return match (name, a.len()) {
            ("toLanguageTag" | "getCountry" | "getDisplayLanguage" | "getDisplayCountry", 0) => Some(Ok(this.clone())),
            ("regionMatches", 5) => {
                let (at, other, from, count) = (a[1].int() as usize, a[2].text(), a[3].int() as usize, a[4].int() as usize);
                let mine: String = text.chars().skip(at).take(count).collect();
                let theirs: String = other.chars().skip(from).take(count).collect();
                Some(Ok(V::Bool(if a[0].truth() { mine.eq_ignore_ascii_case(&theirs) } else { mine == theirs })))
            }
            ("regionMatches", 4) => {
                let (at, other, from, count) = (a[0].int() as usize, a[1].text(), a[2].int() as usize, a[3].int() as usize);
                Some(Ok(V::Bool(text.chars().skip(at).take(count).eq(other.chars().skip(from).take(count)))))
            }
            _ => None,
        };
    }
    if let V::List(items) = this {
        let mut items = lock(items);
        return Some(Ok(match name {
            "reverse" => list(items.iter().rev().cloned().collect()),
            "poll" | "pollFirst" | "pollLast" | "peekLast" | "peek" | "peekFirst" | "offer" | "offerLast" | "offerFirst" | "push" | "pop" | "element" => match name {
                "poll" | "pollFirst" | "pop" => {
                    if items.is_empty() {
                        if name == "pop" {
                            return Some(Err(exc("NoSuchElementException", "empty")));
                        }
                        V::Null
                    } else {
                        items.remove(0)
                    }
                }
                "pollLast" => items.pop().unwrap_or(V::Null),
                "peek" | "peekFirst" | "element" => items.first().cloned().unwrap_or(V::Null),
                "peekLast" => items.last().cloned().unwrap_or(V::Null),
                "offer" | "offerLast" => {
                    items.push(a[0].clone());
                    V::Bool(true)
                }
                _ => {
                    items.insert(0, a[0].clone());
                    V::Bool(true)
                }
            },
            _ => return None,
        }));
    }
    None
}

fn cmp_values(a: &V, b: &V) -> i32 {
    match (a, b) {
        (V::Str(x), V::Str(y)) => x.cmp(y) as i32,
        (V::Int(_) | V::Long(_) | V::Double(_) | V::Char(_), _) => crate::cmp(a, b) as i32,
        _ => call(a, "compareTo", std::slice::from_ref(b)).map_or(0, |c| c.int()),
    }
}

fn jvm_properties() -> std::sync::MutexGuard<'static, std::collections::BTreeMap<String, String>> {
    static PROPERTIES: Mutex<std::collections::BTreeMap<String, String>> = Mutex::new(std::collections::BTreeMap::new());
    lock(&PROPERTIES)
}
