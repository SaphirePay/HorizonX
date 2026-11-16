//! The part of `CompletableFuture<Boolean>` the teleports lean on. Its order matters: a `then_accept`
//! on a finished future runs at once, and those waiting run last added first, as Java's stack of
//! dependents does. So `warpingTo` and `teleportHome` land where they land in Java.
use crate::error::EssError;
use std::sync::{Arc, Mutex};

type OnDone = Box<dyn FnOnce(bool) + Send>;
type OnFail = Box<dyn FnOnce(&EssError) + Send>;

enum Waiter {
    Done(OnDone),
    Fail(OnFail),
}

#[derive(Default)]
struct State {
    finished: Option<Result<bool, EssError>>,
    waiting: Vec<Waiter>,
}

#[derive(Clone, Default)]
pub struct EssFuture(Arc<Mutex<State>>);

impl EssFuture {
    pub fn new() -> Self {
        Self::default()
    }
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// `thenAccept`.
    pub fn then_accept(&self, then: impl FnOnce(bool) + Send + 'static) {
        let finished = {
            let mut state = self.state();
            match &state.finished {
                Some(result) => result.clone(),
                None => {
                    state.waiting.push(Waiter::Done(Box::new(then)));
                    return;
                }
            }
        };
        if let Ok(success) = finished {
            then(success);
        }
    }
    /// `exceptionally`.
    pub fn exceptionally(&self, then: impl FnOnce(&EssError) + Send + 'static) {
        let finished = {
            let mut state = self.state();
            match &state.finished {
                Some(result) => result.clone(),
                None => {
                    state.waiting.push(Waiter::Fail(Box::new(then)));
                    return;
                }
            }
        };
        if let Err(error) = finished {
            then(&error);
        }
    }
    fn finish(&self, result: Result<bool, EssError>) -> bool {
        let waiting = {
            let mut state = self.state();
            if state.finished.is_some() {
                return false;
            }
            state.finished = Some(result.clone());
            std::mem::take(&mut state.waiting)
        };
        for waiter in waiting.into_iter().rev() {
            match (waiter, &result) {
                (Waiter::Done(then), Ok(success)) => then(*success),
                (Waiter::Fail(then), Err(error)) => then(error),
                _ => {}
            }
        }
        true
    }
    /// `complete`: false if it had finished already.
    pub fn complete(&self, success: bool) -> bool {
        self.finish(Ok(success))
    }
    /// `completeExceptionally`.
    pub fn complete_exceptionally(&self, error: EssError) -> bool {
        self.finish(Err(error))
    }
    /// `isCompletedExceptionally`.
    pub fn is_completed_exceptionally(&self) -> bool {
        matches!(self.state().finished, Some(Err(_)))
    }
    /// What `future.get()` would throw, for the blocking overloads.
    pub fn failure(&self) -> Option<EssError> {
        match &self.state().finished {
            Some(Err(error)) => Some(error.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiters_run_last_first_and_first_completion_wins() {
        let future = EssFuture::new();
        let log = Arc::new(Mutex::new(Vec::new()));
        for name in ["first", "second"] {
            let log = log.clone();
            future.then_accept(move |_| log.lock().unwrap().push(name));
        }
        assert!(future.complete(true));
        assert!(!future.complete(false));
        assert_eq!(*log.lock().unwrap(), ["second", "first"]);
    }

    #[test]
    fn failure_skips_then_accept() {
        let future = EssFuture::new();
        future.then_accept(|_| panic!("not on failure"));
        assert!(future.complete_exceptionally(EssError::NoCharge));
        assert!(future.is_completed_exceptionally());
    }
}
