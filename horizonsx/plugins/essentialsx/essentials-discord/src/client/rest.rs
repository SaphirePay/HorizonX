//! Discord's REST API over `ureq`: requests queue by route and run on a few threads of their own, each
//! route in order, waiting out what the rate limit headers and any 429 say. A URL can hold a webhook's
//! or an interaction's token, so no error here carries one: errors name the route's template.
use essentials::prelude::*;
use super::pending::{Completer, Pending};
use super::secret::Secret;
use crate::host::{self, Host, Level};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const WORKERS: usize = 3;
/// How long one request may take. `shutdown` waits for what's running, so this bounds it too.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// JDA tries a failed connection or a 5xx three times.
const MAX_TRIES: u32 = 3;
const MAX_RATE_LIMITS: u32 = 16;
const RETRY_DELAY: Duration = Duration::from_millis(500);

/// A request's place in the rate limits: its method and template, and the id Discord keeps apart.
#[derive(Clone, Copy, Debug)]
pub struct Route {
    pub method: &'static str,
    pub template: &'static str,
    pub major: u64,
}

impl Route {
    pub fn new(method: &'static str, template: &'static str, major: u64) -> Self {
        Self { method, template, major }
    }
    fn endpoint(&self) -> String {
        format!("{} {}", self.method, self.template)
    }
    fn key(&self) -> String {
        format!("{} {}", self.endpoint(), self.major)
    }
}

pub struct Call {
    pub route: Route,
    /// With the ids and tokens in it. Never logged.
    pub path: String,
    pub body: Option<Value>,
    /// False for webhooks and interactions, which their token authorises.
    pub auth: bool,
}

impl Call {
    pub fn new(route: Route, path: String) -> Self {
        Self { route, path, body: None, auth: true }
    }
    pub fn body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }
    pub fn no_auth(mut self) -> Self {
        self.auth = false;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RestError {
    /// Discord said no. `code` is its JSON error code, 0 if there was none.
    Status { route: String, status: u16, code: i64, message: String },
    /// Only from `call_now`: the caller paces itself.
    RateLimited { retry_after: Duration },
    Transport { route: String, kind: String },
    Shutdown,
}

impl RestError {
    /// `ErrorResponse.MISSING_ACCESS`.
    pub fn is_missing_access(&self) -> bool {
        matches!(self, Self::Status { code: 50001, .. })
    }
    /// `ErrorResponse.UNKNOWN_MEMBER` and such: the thing isn't there.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Status { status: 404, .. })
    }
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Status { status: 401, .. })
    }
}

impl std::fmt::Display for RestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Status { route, status, code, message } => write!(f, "HTTP {status} from {route}: {message} ({code})"),
            Self::RateLimited { retry_after } => write!(f, "rate limited (429) for {}ms", retry_after.as_millis()),
            Self::Transport { route, kind } => write!(f, "{kind} on {route}"),
            Self::Shutdown => f.write_str("the client has shut down"),
        }
    }
}

pub type Answer = Result<Value, RestError>;

struct Job {
    call: Call,
    done: Completer<Answer>,
    tries: u32,
    limited: u32,
}

#[derive(Default)]
struct RouteQueue {
    jobs: VecDeque<Job>,
    busy: bool,
    not_before: Option<Instant>,
}

#[derive(Clone, Copy)]
struct Bucket {
    remaining: u64,
    reset_at: Instant,
}

#[derive(Default)]
struct State {
    routes: HashMap<String, RouteQueue>,
    /// `X-RateLimit-Bucket` of each endpoint: endpoints can share one.
    hashes: HashMap<String, String>,
    buckets: HashMap<String, Bucket>,
    global_until: Option<Instant>,
    /// Finish what's queued, until then: then stop.
    closing_until: Option<Instant>,
}

struct Core {
    agent: ureq::Agent,
    base: String,
    token: Secret,
    host: Arc<dyn Host>,
    state: Mutex<State>,
    wake: Condvar,
    stop: AtomicBool,
}

/// One request's result, before deciding what comes of it.
enum Outcome {
    Done(Value),
    RateLimited(Duration),
    Retry(RestError),
    Failed(RestError),
}

fn bucket_key(hash: &str, route: &Route) -> String {
    format!("{hash}:{}", route.major)
}

impl State {
    /// When this route may next run, by what's known of it and of its bucket.
    fn ready_at(&self, route: &Route, queue: &RouteQueue) -> Option<Instant> {
        let bucket = self.hashes.get(&route.endpoint()).and_then(|hash| self.buckets.get(&bucket_key(hash, route))).filter(|bucket| bucket.remaining == 0).map(|bucket| bucket.reset_at);
        [queue.not_before, bucket, self.global_until].into_iter().flatten().max()
    }
    /// The next job that may run, or when to look again.
    fn take(&mut self, now: Instant) -> Result<(String, Job), Option<Instant>> {
        let mut soonest: Option<Instant> = None;
        let mut chosen = None;
        for (key, queue) in &self.routes {
            let Some(job) = queue.jobs.front().filter(|_| !queue.busy) else { continue };
            match self.ready_at(&job.call.route, queue) {
                Some(at) if at > now => soonest = Some(soonest.map_or(at, |known: Instant| known.min(at))),
                _ => {
                    chosen = Some(key.clone());
                    break;
                }
            }
        }
        let Some(key) = chosen else { return Err(soonest) };
        let queue = self.routes.get_mut(&key).expect("just found");
        queue.busy = true;
        let job = queue.jobs.pop_front().expect("just found");
        Ok((key, job))
    }
    fn is_idle(&self) -> bool {
        self.routes.values().all(|queue| queue.jobs.is_empty() && !queue.busy)
    }
}

fn seconds(text: &str) -> Option<Duration> {
    text.parse::<f64>().ok().filter(|value| value.is_finite() && *value >= 0.0).map(Duration::from_secs_f64)
}

impl Core {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// The bucket headers of an answer.
    fn note_limits(&self, route: &Route, response: &ureq::Response) {
        let Some(hash) = response.header("X-RateLimit-Bucket").map(str::to_owned) else { return };
        let mut state = self.state();
        if let (Some(remaining), Some(reset)) = (response.header("X-RateLimit-Remaining").and_then(|text| text.parse().ok()), response.header("X-RateLimit-Reset-After").and_then(seconds)) {
            state.buckets.insert(bucket_key(&hash, route), Bucket { remaining, reset_at: Instant::now() + reset });
        }
        state.hashes.insert(route.endpoint(), hash);
    }
    /// One try, with nothing queued or retried.
    fn send(&self, call: &Call) -> Outcome {
        let route = call.route;
        let url = format!("{}{}", self.base, call.path);
        let mut request = self.agent.request(route.method, &url).set("User-Agent", concat!("DiscordBot (https://essentialsx.net/, ", env!("CARGO_PKG_VERSION"), ")"));
        if call.auth {
            request = request.set("Authorization", &format!("Bot {}", self.token.expose()));
        }
        let result = match &call.body {
            Some(body) => request.set("Content-Type", "application/json").send_string(&body.to_string()),
            None => request.call(),
        };
        match result {
            Ok(response) => {
                self.note_limits(&route, &response);
                let text = response.into_string().unwrap_or_default();
                Outcome::Done(serde_json::from_str(&text).unwrap_or(Value::Null))
            }
            Err(ureq::Error::Status(429, response)) => {
                self.note_limits(&route, &response);
                let header = response.header("Retry-After").and_then(seconds);
                let global = response.header("X-RateLimit-Global").is_some();
                let body: Value = serde_json::from_str(&response.into_string().unwrap_or_default()).unwrap_or(Value::Null);
                let wait = body["retry_after"].as_f64().filter(|value| value.is_finite() && *value >= 0.0).map(Duration::from_secs_f64).or(header).unwrap_or(Duration::from_secs(1));
                if global || body["global"].as_bool() == Some(true) {
                    self.state().global_until = Some(Instant::now() + wait);
                }
                Outcome::RateLimited(wait)
            }
            Err(ureq::Error::Status(status, response)) => {
                self.note_limits(&route, &response);
                let body: Value = serde_json::from_str(&response.into_string().unwrap_or_default()).unwrap_or(Value::Null);
                let error = RestError::Status { route: route.endpoint(), status, code: body["code"].as_i64().unwrap_or(0), message: body["message"].as_str().unwrap_or_default().to_owned() };
                if matches!(status, 500 | 502 | 503 | 504) { Outcome::Retry(error) } else { Outcome::Failed(error) }
            }
            // `Transport`'s text holds the URL, and with it any token: only its kind goes further.
            Err(ureq::Error::Transport(transport)) => Outcome::Retry(RestError::Transport { route: route.endpoint(), kind: format!("{:?}", transport.kind()) }),
        }
    }
    fn finish(&self, key: &str, not_before: Option<Instant>, requeue: Option<Job>) {
        let mut state = self.state();
        if let Some(queue) = state.routes.get_mut(key) {
            queue.busy = false;
            queue.not_before = not_before;
            if let Some(job) = requeue {
                queue.jobs.push_front(job);
            }
        }
        drop(state);
        self.wake.notify_all();
    }
    fn run(&self, key: &str, mut job: Job) {
        match self.send(&job.call) {
            Outcome::Done(value) => {
                self.finish(key, None, None);
                job.done.complete(Ok(value));
            }
            Outcome::RateLimited(wait) => {
                job.limited += 1;
                host::log(&self.host, Level::Warning, &format!("Discord rate limited {} for {}ms.", job.call.route.endpoint(), wait.as_millis()));
                if job.limited >= MAX_RATE_LIMITS {
                    self.finish(key, Some(Instant::now() + wait), None);
                    job.done.complete(Err(RestError::RateLimited { retry_after: wait }));
                } else {
                    self.finish(key, Some(Instant::now() + wait), Some(job));
                }
            }
            Outcome::Retry(error) => {
                job.tries += 1;
                if job.tries >= MAX_TRIES {
                    self.finish(key, None, None);
                    job.done.complete(Err(error));
                } else {
                    self.finish(key, Some(Instant::now() + RETRY_DELAY), Some(job));
                }
            }
            Outcome::Failed(error) => {
                self.finish(key, None, None);
                job.done.complete(Err(error));
            }
        }
    }
    fn worker(&self) {
        loop {
            let mut state = self.state();
            loop {
                let now = Instant::now();
                if self.stop.load(Ordering::SeqCst) || state.closing_until.is_some_and(|until| now >= until || state.is_idle()) {
                    self.wake.notify_all();
                    return;
                }
                match state.take(now) {
                    Ok((key, job)) => {
                        drop(state);
                        self.run(&key, job);
                        break;
                    }
                    Err(soonest) => {
                        let limit = soonest.map_or(Duration::from_secs(1), |at| at.saturating_duration_since(now)).min(Duration::from_secs(1));
                        state = self.wake.wait_timeout(state, limit.max(Duration::from_millis(1))).unwrap_or_else(|e| e.into_inner()).0;
                    }
                }
            }
        }
    }
}

/// The REST side of the client.
pub struct Rest {
    core: Arc<Core>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl Rest {
    pub fn new(base: &str, token: Secret, host: Arc<dyn Host>) -> Self {
        let agent = ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).timeout_connect(Duration::from_secs(10)).redirects(0).build();
        let core = Arc::new(Core { agent, base: base.trim_end_matches('/').to_owned(), token, host, state: Mutex::new(State::default()), wake: Condvar::new(), stop: AtomicBool::new(false) });
        let workers = (0..WORKERS)
            .map(|index| {
                let core = core.clone();
                std::thread::Builder::new().name(format!("EssX-Discord-REST-{index}")).spawn(move || core.worker()).expect("thread")
            })
            .collect();
        Self { core, workers: Mutex::new(workers) }
    }
    /// `RestAction.queue`: in order with the others on this route.
    pub fn queue(&self, call: Call) -> Pending<Answer> {
        let (pending, done) = Pending::channel();
        let mut state = self.core.state();
        if self.core.stop.load(Ordering::SeqCst) || state.closing_until.is_some() {
            drop(state);
            done.complete(Err(RestError::Shutdown));
            return pending;
        }
        state.routes.entry(call.route.key()).or_default().jobs.push_back(Job { call, done, tries: 0, limited: 0 });
        drop(state);
        self.core.wake.notify_all();
        pending
    }
    /// `RestAction.complete`.
    pub fn complete(&self, call: Call) -> Answer {
        self.queue(call).wait(REQUEST_TIMEOUT * (MAX_TRIES + 1)).unwrap_or(Err(RestError::Shutdown))
    }
    /// One try, here, past the queue. A 429 comes back to the caller, which paces itself: the webhooks do.
    pub fn call_now(&self, call: &Call) -> Answer {
        fail_if!(self.core.stop.load(Ordering::SeqCst), RestError::Shutdown);
        match self.core.send(call) {
            Outcome::Done(value) => Ok(value),
            Outcome::RateLimited(retry_after) => Err(RestError::RateLimited { retry_after }),
            Outcome::Retry(error) | Outcome::Failed(error) => Err(error),
        }
    }
    /// JDA's `shutdown`: what's queued has `grace` to go out, what's left fails, and every thread is
    /// gone when this returns. What's running ends by `REQUEST_TIMEOUT`, so that bounds the wait.
    pub fn shutdown(&self, grace: Duration) {
        self.core.state().closing_until = Some(Instant::now() + grace);
        self.core.wake.notify_all();
        let workers = std::mem::take(&mut *self.core_workers());
        for worker in workers {
            let _ = worker.join();
        }
        self.core.stop.store(true, Ordering::SeqCst);
        let left: Vec<Job> = self.core.state().routes.values_mut().flat_map(|queue| queue.jobs.drain(..)).collect();
        for job in left {
            job.done.complete(Err(RestError::Shutdown));
        }
    }
    fn core_workers(&self) -> MutexGuard<'_, Vec<JoinHandle<()>>> {
        self.workers.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(route: Route) -> Job {
        let (_pending, done) = Pending::channel();
        Job { call: Call::new(route, "/x".to_owned()), done, tries: 0, limited: 0 }
    }

    #[test]
    fn routes_with_one_major_share_a_queue_and_others_do_not() {
        let one = Route::new("POST", "/channels/{}/messages", 1);
        assert_eq!(one.key(), Route::new("POST", "/channels/{}/messages", 1).key());
        assert_ne!(one.key(), Route::new("POST", "/channels/{}/messages", 2).key());
        assert_ne!(one.key(), Route::new("PATCH", "/channels/{}/messages", 1).key());
    }

    #[test]
    fn an_empty_bucket_holds_its_route_back() {
        let route = Route::new("POST", "/channels/{}/messages", 1);
        let mut state = State::default();
        state.routes.entry(route.key()).or_default().jobs.push_back(job(route));
        let now = Instant::now();
        state.hashes.insert(route.endpoint(), "abc".to_owned());
        state.buckets.insert(bucket_key("abc", &route), Bucket { remaining: 0, reset_at: now + Duration::from_secs(5) });
        assert!(state.take(now).is_err_and(|soonest| soonest == Some(now + Duration::from_secs(5))));
        assert!(state.take(now + Duration::from_secs(6)).is_ok());
    }

    #[test]
    fn a_busy_route_keeps_its_order() {
        let route = Route::new("POST", "/channels/{}/messages", 1);
        let mut state = State::default();
        let queue = state.routes.entry(route.key()).or_default();
        queue.jobs.push_back(job(route));
        queue.jobs.push_back(job(route));
        let now = Instant::now();
        assert!(state.take(now).is_ok());
        assert!(state.take(now).is_err());
    }

    #[test]
    fn a_global_limit_holds_everything() {
        let route = Route::new("GET", "/users/@me", 0);
        let now = Instant::now();
        let mut state = State { global_until: Some(now + Duration::from_secs(1)), ..State::default() };
        state.routes.entry(route.key()).or_default().jobs.push_back(job(route));
        assert!(state.take(now).is_err());
    }

    #[test]
    fn errors_name_the_template() {
        let error = RestError::Status { route: "POST /webhooks/{}/{}".to_owned(), status: 404, code: 10015, message: "Unknown Webhook".to_owned() };
        assert_eq!(error.to_string(), "HTTP 404 from POST /webhooks/{}/{}: Unknown Webhook (10015)");
        assert!(error.is_not_found());
    }

    #[test]
    fn seconds_refuses_nonsense() {
        assert_eq!(seconds("1.5"), Some(Duration::from_millis(1500)));
        assert_eq!(seconds("-1"), None);
        assert_eq!(seconds("soon"), None);
    }
}
