//! Where this backend's callbacks run: at the program's calls into wgpu, as native wgpu runs
//! them, and never on a task of their own.
//!
//! wgpu answers a buffer's mapping and a queue's submitted work through callbacks. The request
//! reaches the engine at the call: `map-async` and `on-submitted-work-done` ask wgpu-core then, in
//! order with the calls around them, and answer a future the engine resolves once the GPU has
//! done the work, so an `unmap` after a mapping aborts it, and work submitted after
//! `on_submitted_work_done` is not waited for. Only the answer comes later. A device keeps each
//! future with its callback, in the order asked ([`PendingCallbacks`]), and runs the callbacks
//! whose futures have resolved at `Device::poll` (once the engine's poll returns), at
//! `Queue::submit` and at `Instance::poll_all`, where wgpu-core fires its callbacks, then the
//! device's uncaptured errors and its loss ([`DeviceCallbacks`]). A program that awaits a mapping
//! without ever polling or submitting is never answered, as with native wgpu. An `unmap` runs
//! its buffer's aborted mapping at once, as wgpu-core does.
//!
//! The invariant: nothing here is spawned, since a task started while a device opens inside a
//! `wasdk::block_on` keeps that wait from returning. Each future or stream is read without
//! waiting: polled once with a waker that does nothing, and the read cancelled back into its
//! reader where it has no answer yet. A callback runs once, and nothing is held while it runs, so
//! a callback may ask for another or call into wgpu.

use std::collections::VecDeque;
use std::fmt;
use std::future::{Future, IntoFuture};
use std::pin::pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::task::{Context, Poll, Waker};

use wasdk::FutureReader;
use wasdk::gpu::Buffer;
use wgpu::custom::{BoxSubmittedWorkDoneCallback, BufferMapCallback};

use crate::device_lost::DeviceLostCallback;
use crate::uncaptured_errors::UncapturedErrors;

/// Everything one device answers the program through, shared by the device and its queue, which
/// run it at the calls wgpu-core fires its callbacks at.
#[derive(Debug)]
pub(crate) struct DeviceCallbacks {
    /// Its buffers' mappings and its queue's submitted work.
    pub(crate) pending: PendingCallbacks,
    pub(crate) uncaptured_errors: UncapturedErrors,
    pub(crate) device_lost: DeviceLostCallback,
}

impl DeviceCallbacks {
    /// The callbacks of `device`, its uncaptured errors and its loss read from now on.
    pub(crate) fn of(device: &wasdk::gpu::Device) -> DeviceCallbacks {
        DeviceCallbacks {
            pending: PendingCallbacks::default(),
            uncaptured_errors: UncapturedErrors::listen(device),
            device_lost: DeviceLostCallback::listen(device),
        }
    }

    /// Runs, without waiting, every callback whose answer has come: the mappings and submitted
    /// work done, in the order asked, then the errors no scope caught, then the device's loss, as
    /// wgpu-core fires its callbacks.
    pub(crate) fn run_ready(&self) {
        self.pending.run_resolved();
        self.uncaptured_errors.handle_reported();
        self.device_lost.run_if_lost();
    }
}

/// The callbacks one device waits to run, in the order asked, shared by the device, its queue
/// and its buffers. Every clone is the same list.
#[derive(Clone, Default)]
pub(crate) struct PendingCallbacks(Arc<Mutex<VecDeque<PendingCallback>>>);

impl fmt::Debug for PendingCallbacks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingCallbacks")
            .field("callbacks", &self.callbacks().len())
            .finish()
    }
}

impl PendingCallbacks {
    /// Waits for `answer`, the future of `buffer`'s mapping, to run `callback`.
    pub(crate) fn add_mapping(
        &self,
        buffer: &Arc<Buffer>,
        answer: FutureReader<Result<(), String>>,
        callback: BufferMapCallback,
    ) {
        self.callbacks().push_back(PendingCallback::Mapping {
            buffer: Arc::downgrade(buffer),
            answer,
            callback,
        });
    }

    /// Waits for `answer`, the future of a queue's submitted work, to run `callback`.
    pub(crate) fn add_submitted_work_done(
        &self,
        answer: FutureReader<()>,
        callback: BoxSubmittedWorkDoneCallback,
    ) {
        self.callbacks()
            .push_back(PendingCallback::SubmittedWorkDone { answer, callback });
    }

    /// Runs, in the order asked, every callback whose future has resolved by now.
    pub(crate) fn run_resolved(&self) {
        self.run(|_| true);
    }

    /// Runs the callback of `buffer`'s mapping where its future has resolved: an `unmap`
    /// resolves the future of a mapping it aborts as it is made.
    pub(crate) fn run_resolved_mapping(&self, buffer: &Arc<Buffer>) {
        self.run(|callback| callback.is_mapping_of(buffer));
    }

    /// Runs, in the order asked, each callback `chosen` picks whose future has resolved, taken
    /// from the list first; the rest stay, in order.
    fn run(&self, chosen: impl Fn(&PendingCallback) -> bool) {
        let resolved = {
            let mut callbacks = self.callbacks();
            let mut resolved = Vec::new();
            for callback in std::mem::take(&mut *callbacks) {
                if !chosen(&callback) {
                    callbacks.push_back(callback);
                    continue;
                }
                match callback.read() {
                    Ok(callback) => resolved.push(callback),
                    Err(callback) => callbacks.push_back(callback),
                }
            }
            resolved
        };
        for callback in resolved {
            callback.run();
        }
    }

    fn callbacks(&self) -> MutexGuard<'_, VecDeque<PendingCallback>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A callback waiting for the future of the request it was asked with.
enum PendingCallback {
    /// A buffer's range mapped, or why not.
    Mapping {
        buffer: Weak<Buffer>,
        answer: FutureReader<Result<(), String>>,
        callback: BufferMapCallback,
    },
    /// The work submitted to the queue before it was asked done.
    SubmittedWorkDone {
        answer: FutureReader<()>,
        callback: BoxSubmittedWorkDoneCallback,
    },
}

/// A callback whose future has resolved, with what it resolved to.
enum ResolvedCallback {
    Mapping(BufferMapCallback, Result<(), String>),
    SubmittedWorkDone(BoxSubmittedWorkDoneCallback),
}

impl PendingCallback {
    fn is_mapping_of(&self, buffer: &Arc<Buffer>) -> bool {
        matches!(self, PendingCallback::Mapping { buffer: of, .. } if Weak::as_ptr(of) == Arc::as_ptr(buffer))
    }

    /// The callback with its answer where its future has resolved; itself where not.
    fn read(self) -> Result<ResolvedCallback, PendingCallback> {
        match self {
            PendingCallback::Mapping {
                buffer,
                answer,
                callback,
            } => match read_now(answer) {
                Ok(mapped) => Ok(ResolvedCallback::Mapping(callback, mapped)),
                Err(answer) => Err(PendingCallback::Mapping {
                    buffer,
                    answer,
                    callback,
                }),
            },
            PendingCallback::SubmittedWorkDone { answer, callback } => match read_now(answer) {
                Ok(()) => Ok(ResolvedCallback::SubmittedWorkDone(callback)),
                Err(answer) => Err(PendingCallback::SubmittedWorkDone { answer, callback }),
            },
        }
    }
}

impl ResolvedCallback {
    fn run(self) {
        match self {
            ResolvedCallback::Mapping(callback, mapped) => {
                callback(mapped.map_err(|_| wgpu::BufferAsyncError));
            }
            ResolvedCallback::SubmittedWorkDone(callback) => callback(),
        }
    }
}

/// What `reader`'s future resolved to, where it has; the reader where not, its read cancelled,
/// which asks the engine once more, so it is read again next time.
pub(crate) fn read_now<T: 'static>(reader: FutureReader<T>) -> Result<T, FutureReader<T>> {
    let mut read = pin!(reader.into_future());
    if let Poll::Ready(value) = read.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        return Ok(value);
    }
    read.as_mut().cancel()
}
