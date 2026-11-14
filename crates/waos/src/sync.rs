//! Locks this crate takes: each is held for a read or a write of one value, with nothing
//! between the lock and its drop that can panic, so a poisoned lock is a bug, not a state to
//! recover from.

use std::sync::{Mutex, MutexGuard};

const NEVER_POISONED: &str = "a lock in waos is never held across a panic";

pub(crate) fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect(NEVER_POISONED)
}
