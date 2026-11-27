//! A handle any thread can hold to a value that lives on the App's thread, as objc2's
//! `MainThreadBound` and Chromium's `SequenceBound<T>` are: the engine calls the shell's services
//! from programs' threads, and each service's Waft objects live in one such value, which a call
//! posted through the handle reaches.
//!
//! The invariant: the calls posted through a value's handles run on the App's thread, one at a
//! time, in the order posted; and a call that can no longer run, as WaOS quits, says so to the
//! thread that posts it, so a caller that must answer someone answers at once.

use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use waft::App;

/// One call of a value on the App's thread.
type Call<T> = Box<dyn FnOnce(&mut T) + Send>;

/// A handle any thread can hold to a value that lives on the App's thread.
pub(crate) struct AppThreadBound<T> {
    calls: UnboundedSender<Call<T>>,
}

/// The value's calls, still to be run on the App's thread by [`serve`](Self::serve).
pub(crate) struct AppThreadCalls<T> {
    arriving: UnboundedReceiver<Call<T>>,
}

/// The App's thread runs no more calls: WaOS is quitting.
#[derive(Debug)]
pub(crate) struct AppEnded;

impl<T> AppThreadBound<T> {
    /// A handle, and the calls it posts, to serve once the value is made on the App's thread.
    pub(crate) fn new() -> (AppThreadBound<T>, AppThreadCalls<T>) {
        let (calls, arriving) = mpsc::unbounded();
        (AppThreadBound { calls }, AppThreadCalls { arriving })
    }

    /// Calls the value on the App's thread, in the order posted.
    pub(crate) fn post(&self, call: impl FnOnce(&mut T) + Send + 'static) -> Result<(), AppEnded> {
        self.calls.unbounded_send(Box::new(call)).map_err(|_| {
            tracing::info!("WaOS is quitting: a call to the App's thread goes nowhere");
            AppEnded
        })
    }
}

impl<T> Clone for AppThreadBound<T> {
    fn clone(&self) -> Self {
        AppThreadBound {
            calls: self.calls.clone(),
        }
    }
}

impl<T: 'static> AppThreadCalls<T> {
    /// From now on, runs each call posted on `value`, on the App's thread, until every handle is
    /// gone; `value` is dropped there then.
    pub(crate) fn serve(self, app: &App, value: T) {
        let mut arriving = self.arriving;
        app.spawn(async move |_cx| {
            let mut value = value;
            while let Some(call) = arriving.next().await {
                call(&mut value);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_posted_once_nothing_serves_the_value_says_so() {
        let (value, calls) = AppThreadBound::<u32>::new();
        drop(calls);
        assert!(value.post(|_| {}).is_err());
    }
}
