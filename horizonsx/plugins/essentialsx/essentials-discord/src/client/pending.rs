//! `CompletableFuture`, as far as this module uses it: an answer that comes on another thread.
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

type Callback<T> = Box<dyn FnOnce(T) + Send>;

enum Slot<T> {
    Waiting(Option<Callback<T>>),
    Done(T),
    Taken,
}

struct Shared<T> {
    slot: Mutex<Slot<T>>,
    changed: Condvar,
}

/// The reading end.
pub struct Pending<T>(Arc<Shared<T>>);

/// The writing end.
pub struct Completer<T>(Arc<Shared<T>>);

impl<T: Send + 'static> Pending<T> {
    pub fn channel() -> (Self, Completer<T>) {
        let shared = Arc::new(Shared { slot: Mutex::new(Slot::Waiting(None)), changed: Condvar::new() });
        (Self(shared.clone()), Completer(shared))
    }
    /// `CompletableFuture.completedFuture`.
    pub fn ready(value: T) -> Self {
        Self(Arc::new(Shared { slot: Mutex::new(Slot::Done(value)), changed: Condvar::new() }))
    }
    /// `join`, for at most `limit`: `None` if the answer didn't come.
    pub fn wait(self, limit: Duration) -> Option<T> {
        let until = Instant::now() + limit;
        let mut slot = self.0.slot.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match std::mem::replace(&mut *slot, Slot::Taken) {
                Slot::Done(value) => return Some(value),
                waiting @ Slot::Waiting(_) => {
                    *slot = waiting;
                    let left = until.checked_duration_since(Instant::now())?;
                    slot = self.0.changed.wait_timeout(slot, left).unwrap_or_else(|e| e.into_inner()).0;
                }
                Slot::Taken => return None,
            }
        }
    }
    /// `thenAccept`: on whichever thread completes it, or here if it's done already.
    pub fn then(self, callback: impl FnOnce(T) + Send + 'static) {
        let mut slot = self.0.slot.lock().unwrap_or_else(|e| e.into_inner());
        match std::mem::replace(&mut *slot, Slot::Taken) {
            Slot::Done(value) => {
                drop(slot);
                callback(value);
            }
            Slot::Waiting(_) => *slot = Slot::Waiting(Some(Box::new(callback))),
            Slot::Taken => {}
        }
    }
    /// `thenApply`.
    pub fn map<U: Send + 'static>(self, change: impl FnOnce(T) -> U + Send + 'static) -> Pending<U> {
        let (next, completer) = Pending::channel();
        self.then(move |value| completer.complete(change(value)));
        next
    }
}

impl<T: Send + 'static> Completer<T> {
    pub fn complete(self, value: T) {
        let mut slot = self.0.slot.lock().unwrap_or_else(|e| e.into_inner());
        match std::mem::replace(&mut *slot, Slot::Taken) {
            Slot::Waiting(Some(callback)) => {
                drop(slot);
                callback(value);
            }
            Slot::Waiting(None) => {
                *slot = Slot::Done(value);
                self.0.changed.notify_all();
            }
            other => *slot = other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_sees_a_late_answer() {
        let (pending, completer) = Pending::channel();
        std::thread::spawn(move || completer.complete(7));
        assert_eq!(pending.wait(Duration::from_secs(5)), Some(7));
    }

    #[test]
    fn wait_gives_up() {
        let (pending, _completer) = Pending::<u8>::channel();
        assert_eq!(pending.wait(Duration::from_millis(5)), None);
    }

    #[test]
    fn then_runs_either_way() {
        let (tell, seen) = std::sync::mpsc::channel();
        let tell_again = tell.clone();
        Pending::ready(1).then(move |value| tell.send(value).unwrap());
        let (pending, completer) = Pending::channel();
        pending.map(|value: i32| value + 1).then(move |value| tell_again.send(value).unwrap());
        completer.complete(2);
        assert_eq!(seen.recv().unwrap(), 1);
        assert_eq!(seen.recv().unwrap(), 3);
    }
}
