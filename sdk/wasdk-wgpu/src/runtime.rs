//! Where this backend's callbacks run.
//!
//! wgpu answers a buffer's mapping and a queue's submitted work through callbacks, and promises
//! what its own backends do: the callback runs once the GPU has done the work, during a
//! `Device::poll`, and `poll(Wait)` does not return before it has. A program written for that
//! asks, polls, and reads the answer at once. A program written for the web never polls: its
//! callbacks run when it yields. So a device keeps the callbacks it owes, in the order asked
//! ([`OwedCallbacks`]), and delivers them at whichever comes first: its `poll(Wait)`, or the
//! program's next `.await`. Each waits on its host call in place, a plain blocking call, which the
//! host answers once the GPU has done the work.
//!
//! The device-lost and uncaptured-error handlers are no answer to a request: each is a task
//! spawned onto the program's executor, which runs while the program awaits ([`spawn`]).
//!
//! The invariant: a callback owed is delivered exactly once, by whichever takes it from the list
//! first, and nothing is held while it runs, so a callback may ask for another.

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use wasdk::webgpu::{GpuBuffer, GpuMapMode, GpuQueue};
use wgpu::custom::{BoxSubmittedWorkDoneCallback, BufferMapCallback};

pub(crate) fn spawn(task: impl Future<Output = ()> + 'static) {
    wasdk::spawn(task);
}

/// A callback a device owes the program once the GPU has done something.
pub(crate) enum Owed {
    /// A buffer's range mapped, or why not.
    Mapping {
        buffer: Arc<GpuBuffer>,
        mode: GpuMapMode,
        offset: u64,
        size: u64,
        callback: BufferMapCallback,
    },
    /// Everything submitted to the queue so far done.
    SubmittedWorkDone {
        queue: Arc<GpuQueue>,
        callback: BoxSubmittedWorkDoneCallback,
    },
}

impl Owed {
    /// Waits on the host for what the callback answers, then runs it.
    fn deliver(self) {
        match self {
            Owed::Mapping {
                buffer,
                mode,
                offset,
                size,
                callback,
            } => {
                let mapped = buffer.map_async(mode, Some(offset), Some(size));
                callback(mapped.map_err(|_| wgpu::BufferAsyncError));
            }
            Owed::SubmittedWorkDone { queue, callback } => {
                queue.on_submitted_work_done();
                callback();
            }
        }
    }
}

/// The callbacks one device owes, in the order asked, shared by the device, its queue and its
/// buffers. Every clone is the same list.
#[derive(Clone, Default)]
pub(crate) struct OwedCallbacks(Arc<Shared>);

#[derive(Default)]
struct Shared {
    // Shared by what asks (a buffer, the queue) and what delivers (the device's poll, the task
    // run as the program yields), none of which owns the others. Never held while a callback
    // runs.
    owed: Mutex<VecDeque<Owed>>,
    /// Whether a task that delivers as the program yields is spawned and has not run yet.
    delivery_spawned: AtomicBool,
}

impl fmt::Debug for OwedCallbacks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwedCallbacks")
            .field("owed", &self.owed().len())
            .finish()
    }
}

impl OwedCallbacks {
    /// Owes `callback`: delivered at the device's next `poll(Wait)`, or as the program next
    /// yields, whichever comes first.
    pub(crate) fn owe(&self, callback: Owed) {
        self.owed().push_back(callback);
        if !self.0.delivery_spawned.swap(true, Ordering::Relaxed) {
            let owed = self.clone();
            spawn(async move {
                owed.0.delivery_spawned.store(false, Ordering::Relaxed);
                owed.deliver();
            });
        }
    }

    /// Delivers every callback owed, in order, each once the host has answered it; those a
    /// callback asks for meanwhile too.
    pub(crate) fn deliver(&self) {
        loop {
            let next = self.owed().pop_front();
            let Some(next) = next else {
                return;
            };
            next.deliver();
        }
    }

    fn owed(&self) -> MutexGuard<'_, VecDeque<Owed>> {
        self.0.owed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
