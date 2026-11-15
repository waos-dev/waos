//! A callback GPUI registers, kept so that calling it never holds a borrow.
//!
//! GPUI's callbacks re-enter the platform: a frame callback draws, an input callback sets the
//! input handler, and either may register callbacks anew. So a slot lends its callback out for
//! the call and takes it back after, unless GPUI registered a new one meanwhile, which then stays.

use std::cell::Cell;

pub(crate) struct Slot<F>(Cell<Option<F>>);

impl<F> Default for Slot<F> {
    fn default() -> Self {
        Slot(Cell::new(None))
    }
}

impl<F> Slot<F> {
    pub(crate) fn set(&self, callback: F) {
        self.0.set(Some(callback));
    }

    /// Calls the callback with `call`, or answers none when there is none, or it is being called
    /// already.
    pub(crate) fn call<R>(&self, call: impl FnOnce(&mut F) -> R) -> Option<R> {
        let mut callback = self.0.take()?;
        let answer = call(&mut callback);
        let registered_meanwhile = self.0.take();
        self.0.set(registered_meanwhile.or(Some(callback)));
        Some(answer)
    }

    /// The callback, to be called once.
    pub(crate) fn take(&self) -> Option<F> {
        self.0.take()
    }
}
