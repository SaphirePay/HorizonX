//! `WebhookDispatcher`: at most one message per drain, and an exponential wait after a 429.
//! `WrappedWebhookClient`'s job is the `Sender` it's given.
use crate::client::model::Mentions;
use crate::client::pending::{Completer, Pending};
use crate::client::rest::RestError;
use crate::host::{self, Host, Level};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const DRAIN_INTERVAL: Duration = Duration::from_millis(500);
pub const DEFAULT_QUEUE_CAPACITY: usize = 30;
const INITIAL_BACKOFF: Duration = Duration::from_millis(4000);
const MAX_BACKOFF: Duration = Duration::from_millis(30000);

/// `WebhookMessage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebhookMessage {
    pub content: String,
    pub username: String,
    pub avatar_url: Option<String>,
    pub mentions: Mentions,
}

impl WebhookMessage {
    pub fn to_json(&self) -> Value {
        let mut body = json!({ "content": self.content, "username": self.username, "allowed_mentions": self.mentions.to_json() });
        if let Some(avatar) = &self.avatar_url {
            body["avatar_url"] = json!(avatar);
        }
        body
    }
}

/// One try at sending: the webhook's id and token are in here, and go nowhere else.
pub type Sender = Box<dyn Fn(&WebhookMessage) -> Result<(), RestError> + Send + Sync>;

pub type Sent = Result<(), RestError>;

struct PendingMessage {
    message: WebhookMessage,
    done: Completer<Sent>,
}

struct Shared {
    send: Sender,
    host: Arc<dyn Host>,
    capacity: usize,
    queue: Mutex<VecDeque<PendingMessage>>,
    wake: Condvar,
    shutdown: AtomicBool,
    dropped: AtomicU32,
    limited: AtomicU32,
}

impl Shared {
    fn queue(&self) -> MutexGuard<'_, VecDeque<PendingMessage>> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `drain`, on the thread, until `close`.
    fn run(&self, interval: Duration) {
        let mut backoff_until: Option<Instant> = None;
        let mut current_backoff = INITIAL_BACKOFF;
        loop {
            let mut queue = self.queue();
            let until = Instant::now() + interval;
            while !self.shutdown.load(Ordering::SeqCst) {
                let Some(left) = until.checked_duration_since(Instant::now()) else {
                    break;
                };
                queue = self.wake.wait_timeout(queue, left).unwrap_or_else(|e| e.into_inner()).0;
            }
            if self.shutdown.load(Ordering::SeqCst) {
                return;
            }
            if backoff_until.is_some_and(|at| Instant::now() < at) {
                continue;
            }
            let Some(pending) = queue.pop_front() else { continue };
            drop(queue);
            match (self.send)(&pending.message) {
                Ok(()) => {
                    current_backoff = INITIAL_BACKOFF;
                    pending.done.complete(Ok(()));
                }
                Err(RestError::RateLimited { retry_after }) => {
                    let count = self.limited.fetch_add(1, Ordering::SeqCst) + 1;
                    let wait = current_backoff.max(retry_after);
                    backoff_until = Some(Instant::now() + wait);
                    let mut queue = self.queue();
                    host::log(&self.host, Level::Warning, &format!("Webhook rate limited (429). Backing off {}ms. Queue: {}, 429 count: {count}, dropped: {}", wait.as_millis(), queue.len(), self.dropped.load(Ordering::SeqCst)));
                    current_backoff = (current_backoff * 2).min(MAX_BACKOFF);
                    // The library behind the Java tries a limited message again by itself.
                    queue.push_front(pending);
                }
                Err(error) => {
                    host::log(&self.host, Level::Warning, &format!("Webhook send failed: {error}"));
                    pending.done.complete(Err(error));
                }
            }
        }
    }
}

pub struct WebhookDispatcher {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl WebhookDispatcher {
    pub fn new(id: u64, send: Sender, capacity: usize, host: Arc<dyn Host>) -> Self {
        Self::with_interval(id, send, capacity, host, DRAIN_INTERVAL)
    }
    pub fn with_interval(id: u64, send: Sender, capacity: usize, host: Arc<dyn Host>, interval: Duration) -> Self {
        let shared = Arc::new(Shared { send, host, capacity, queue: Mutex::new(VecDeque::new()), wake: Condvar::new(), shutdown: AtomicBool::new(false), dropped: AtomicU32::new(0), limited: AtomicU32::new(0) });
        let worker = shared.clone();
        let thread = std::thread::Builder::new().name(format!("EssX-WebhookDispatcher-{id}")).spawn(move || worker.run(interval)).expect("thread");
        Self { shared, thread: Mutex::new(Some(thread)) }
    }
    /// `send`: with the queue full, the oldest goes.
    pub fn send(&self, message: WebhookMessage) -> Pending<Sent> {
        if self.is_shutdown() {
            return Pending::ready(Ok(()));
        }
        let (pending, done) = Pending::channel();
        let dropped = {
            let mut queue = self.shared.queue();
            let dropped = (queue.len() >= self.shared.capacity).then(|| queue.pop_front()).flatten();
            queue.push_back(PendingMessage { message, done });
            dropped
        };
        if let Some(dropped) = dropped {
            self.shared.dropped.fetch_add(1, Ordering::SeqCst);
            dropped.done.complete(Ok(()));
        }
        pending
    }
    /// `close`: the thread is gone when this returns.
    pub fn close(&self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        self.shared.wake.notify_all();
        let thread = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
        let left: Vec<PendingMessage> = self.shared.queue().drain(..).collect();
        for pending in left {
            pending.done.complete(Ok(()));
        }
    }
    pub fn is_shutdown(&self) -> bool {
        self.shared.shutdown.load(Ordering::SeqCst)
    }
    pub fn get_queue_size(&self) -> usize {
        self.shared.queue().len()
    }
    pub fn get_total_dropped(&self) -> u32 {
        self.shared.dropped.load(Ordering::SeqCst)
    }
    pub fn get_total_429s(&self) -> u32 {
        self.shared.limited.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Feedback, Relay, Task};

    struct Quiet;

    impl Host for Quiet {
        fn run_main(&self, task: Task) {
            task();
        }
        fn is_main(&self) -> bool {
            true
        }
        fn write_log(&self, _level: Level, _text: &str) {}
        fn is_debug(&self) -> bool {
            false
        }
        fn deliver(&self, _relay: Relay) {}
        fn dispatch_command(&self, _line: &str, _feedback: Feedback) {}
    }

    fn message(content: &str) -> WebhookMessage {
        WebhookMessage { content: content.to_owned(), username: "bot".to_owned(), avatar_url: None, mentions: Mentions::NoGroups }
    }

    #[test]
    fn the_message_goes_out_as_discord_takes_it() {
        let mut with_avatar = message("hi");
        assert_eq!(with_avatar.to_json(), json!({ "content": "hi", "username": "bot", "allowed_mentions": { "parse": ["users"] } }));
        with_avatar.avatar_url = Some("http://a/b.png".to_owned());
        assert_eq!(with_avatar.to_json()["avatar_url"], "http://a/b.png");
    }

    #[test]
    fn one_message_goes_each_drain_in_order() {
        let (tell, seen) = std::sync::mpsc::channel();
        let tell = Mutex::new(tell);
        let send: Sender = Box::new(move |message| {
            tell.lock().unwrap().send(message.content.clone()).unwrap();
            Ok(())
        });
        let dispatcher = WebhookDispatcher::with_interval(1, send, 30, Arc::new(Quiet), Duration::from_millis(5));
        dispatcher.send(message("a"));
        dispatcher.send(message("b"));
        assert_eq!(seen.recv_timeout(Duration::from_secs(5)).unwrap(), "a");
        assert_eq!(seen.recv_timeout(Duration::from_secs(5)).unwrap(), "b");
        dispatcher.close();
    }

    #[test]
    fn a_full_queue_drops_the_oldest() {
        let send: Sender = Box::new(|_| Ok(()));
        let dispatcher = WebhookDispatcher::with_interval(1, send, 2, Arc::new(Quiet), Duration::from_secs(60));
        let first = dispatcher.send(message("a"));
        dispatcher.send(message("b"));
        dispatcher.send(message("c"));
        assert_eq!(dispatcher.get_total_dropped(), 1);
        assert_eq!(dispatcher.get_queue_size(), 2);
        assert_eq!(first.wait(Duration::from_secs(1)), Some(Ok(())));
        dispatcher.close();
    }

    #[test]
    fn a_429_keeps_the_message_and_counts() {
        let tries = Arc::new(AtomicU32::new(0));
        let counted = tries.clone();
        let send: Sender = Box::new(move |_| {
            if counted.fetch_add(1, Ordering::SeqCst) == 0 { Err(RestError::RateLimited { retry_after: Duration::from_millis(1) }) } else { Ok(()) }
        });
        let dispatcher = WebhookDispatcher::with_interval(1, send, 5, Arc::new(Quiet), Duration::from_millis(1));
        dispatcher.send(message("a"));
        let started = Instant::now();
        while dispatcher.get_total_429s() == 0 && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(dispatcher.get_total_429s(), 1);
        assert_eq!(dispatcher.get_queue_size(), 1);
        dispatcher.close();
    }

    #[test]
    fn close_ends_the_thread_and_answers_what_waits() {
        let send: Sender = Box::new(|_| Ok(()));
        let dispatcher = WebhookDispatcher::with_interval(1, send, 5, Arc::new(Quiet), Duration::from_secs(60));
        let waiting = dispatcher.send(message("a"));
        dispatcher.close();
        assert!(dispatcher.is_shutdown());
        assert_eq!(waiting.wait(Duration::from_secs(1)), Some(Ok(())));
        assert_eq!(dispatcher.send(message("b")).wait(Duration::from_secs(1)), Some(Ok(())));
    }
}
