//! A handle any thread can hold to a value that lives on the App's thread, as objc2's
//! `MainThreadBound` and Chromium's `SequenceBound<T>` are: the engine calls the shell's services
//! from programs' threads, and each service's Waft objects live in one such value, which a call
//! posted through the handle reaches.
//!
//! The invariant: the calls posted through a value's handles run on the App's thread, one at a
//! time, in the order posted; and a call that can no longer run, as WaOS quits, says so to the
//! thread that posts it, and an asked call's answer resolves to that once it cannot come, so a
//! caller that must answer someone is never left waiting.

use std::future::Future;

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use futures::{FutureExt, StreamExt};
use waft::{App, AsyncApp};

/// One call of a value on the App's thread, with the App's handle to run what it starts.
type Call<T> = Box<dyn FnOnce(&mut T, &AsyncApp) + Send>;

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
        self.send(Box::new(|value, _| call(value)))
    }

    /// Calls the value on the App's thread, in the order posted, and answers what the future the
    /// call starts there resolves to.
    pub(crate) fn ask<A: Send + 'static, F: Future<Output = A> + 'static>(
        &self,
        call: impl FnOnce(&mut T) -> F + Send + 'static,
    ) -> impl Future<Output = Result<A, AppEnded>> + Send + 'static {
        let (answer, answered) = oneshot::channel();
        let _ = self.send(Box::new(|value, app| {
            let answering = call(value);
            app.update(|app| {
                app.spawn(async move |_| {
                    // A caller that went meanwhile hears nothing.
                    let _ = answer.send(answering.await);
                })
            });
        }));
        answered.map(|answer| answer.map_err(|_| AppEnded))
    }

    fn send(&self, call: Call<T>) -> Result<(), AppEnded> {
        self.calls.unbounded_send(call).map_err(|_| {
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
        app.spawn(async move |cx| {
            let mut value = value;
            while let Some(call) = arriving.next().await {
                call(&mut value, cx);
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
        let answer = futures::executor::block_on(value.ask(|_| async { 1 }));
        assert!(answer.is_err());
    }
}
