//! Where this backend's callbacks run.
//!
//! wgpu answers a buffer's mapping and a queue's submitted work through callbacks. The request
//! reaches the engine at the call: `map-async` and `on-submitted-work-done` ask wgpu-core then, in
//! order with the calls around them, and answer a future the engine resolves once the GPU has
//! done the work, so an `unmap` after a mapping aborts it, and work submitted after
//! `on_submitted_work_done` is not waited for. Only the answer comes later. A device keeps each
//! future with its callback, in the order asked ([`PendingCallbacks`]), and runs a callback once
//! its future has resolved, at whichever comes first: the device's `poll`, which runs those whose
//! future has resolved by the time the engine's poll returns, as native wgpu runs its callbacks
//! inside `Device::poll`, so a program that asks, polls and reads works; or the program's next
//! `.await`, where a task of the device's awaits them, as the web resolves its promises, so a
//! program that never polls works too. An `unmap` runs its buffer's aborted mapping at once, as
//! wgpu-core does.
//!
//! The device-lost and uncaptured-error handlers are no answer to a request: each is a task
//! spawned onto the program's executor, which runs while the program awaits ([`spawn`]).
//!
//! The invariant: a callback runs exactly once, taken from the list by whichever finds its
//! future resolved first, and nothing is held while it runs, so a callback may ask for another. A
//! future read by `poll` without waiting is read again by the device's task afterwards, which
//! `poll` wakes for it.

use std::collections::VecDeque;
use std::fmt;
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::task::{Context, Poll, Waker};

use wasdk::FutureReader;
use wasdk::gpu::Buffer;
use wgpu::custom::{BoxSubmittedWorkDoneCallback, BufferMapCallback};

pub(crate) fn spawn(task: impl Future<Output = ()> + 'static) {
    wasdk::spawn(task);
}

/// The callbacks one device waits to run, in the order asked, shared by the device, its queue
/// and its buffers. Every clone is the same list.
#[derive(Clone, Default)]
pub(crate) struct PendingCallbacks(Arc<Shared>);

#[derive(Default)]
struct Shared {
    // Shared by what asks (a buffer, the queue) and what runs the callbacks (the device's poll,
    // a buffer's unmap, the device's task), none of which owns the others. Never held while a
    // callback runs.
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    callbacks: VecDeque<PendingCallback>,
    /// The device's task, woken for each callback added and each future `poll` read.
    task: Option<Waker>,
}

impl Drop for Shared {
    /// The device, its queue and its buffers are gone: the task ends.
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap_or_else(PoisonError::into_inner);
        if let Some(task) = state.task.take() {
            task.wake();
        }
    }
}

impl fmt::Debug for PendingCallbacks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingCallbacks")
            .field("callbacks", &self.state().callbacks.len())
            .finish()
    }
}

impl PendingCallbacks {
    /// A device's callbacks, with the task that runs them as the program yields started on the
    /// executor running now: started inside `request_device`'s future, as the uncaptured errors'
    /// listener is (`uncaptured_errors`), it ends once the device, its queue and its buffers are
    /// gone.
    pub(crate) fn start() -> PendingCallbacks {
        let callbacks = PendingCallbacks::default();
        let shared = Arc::downgrade(&callbacks.0);
        spawn(std::future::poll_fn(move |cx| {
            run_as_the_program_yields(&shared, cx)
        }));
        callbacks
    }

    /// Waits for `answer`, the future of `buffer`'s mapping, to run `callback`.
    pub(crate) fn add_mapping(
        &self,
        buffer: &Arc<Buffer>,
        answer: FutureReader<Result<(), String>>,
        callback: BufferMapCallback,
    ) {
        self.add(PendingCallback::Mapping {
            buffer: Arc::downgrade(buffer),
            answer: Answer::Unread(answer),
            callback,
        });
    }

    /// Waits for `answer`, the future of a queue's submitted work, to run `callback`.
    pub(crate) fn add_submitted_work_done(
        &self,
        answer: FutureReader<()>,
        callback: BoxSubmittedWorkDoneCallback,
    ) {
        self.add(PendingCallback::SubmittedWorkDone {
            answer: Answer::Unread(answer),
            callback,
        });
    }

    /// Runs, in the order asked, every callback whose future has resolved by now: what the
    /// device's `poll` runs once the engine's poll returns.
    pub(crate) fn run_resolved(&self) {
        self.run(&mut Reading::Now, |_| true);
    }

    /// Runs the callback of `buffer`'s mapping where its future has resolved: an `unmap`
    /// resolves the future of a mapping it aborts as it is made.
    pub(crate) fn run_resolved_mapping(&self, buffer: &Arc<Buffer>) {
        self.run(&mut Reading::Now, |callback| callback.is_mapping_of(buffer));
    }

    fn add(&self, callback: PendingCallback) {
        let mut state = self.state();
        state.callbacks.push_back(callback);
        if let Some(task) = &state.task {
            task.wake_by_ref();
        }
    }

    /// Runs, in the order asked, each callback `chosen` picks whose future `reading` finds
    /// resolved, taken from the list first; the rest stay, in order.
    fn run(&self, reading: &mut Reading<'_, '_>, chosen: impl Fn(&PendingCallback) -> bool) {
        let resolved = {
            let mut state = self.state();
            if let Reading::ByTask(cx) = reading {
                state.task = Some(cx.waker().clone());
            }
            let mut resolved = Vec::new();
            for callback in std::mem::take(&mut state.callbacks) {
                if !chosen(&callback) {
                    state.callbacks.push_back(callback);
                    continue;
                }
                match callback.read(reading) {
                    Ok(callback) => resolved.push(callback),
                    Err(callback) => state.callbacks.push_back(callback),
                }
            }
            // A future read without waiting is the task's to wait for again.
            if matches!(reading, Reading::Now)
                && !state.callbacks.is_empty()
                && let Some(task) = &state.task
            {
                task.wake_by_ref();
            }
            resolved
        };
        for callback in resolved {
            callback.run();
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The device's task: runs the callbacks whose futures resolve, woken by each, until the device,
/// its queue and its buffers are gone.
fn run_as_the_program_yields(shared: &Weak<Shared>, cx: &mut Context<'_>) -> Poll<()> {
    let Some(shared) = shared.upgrade() else {
        return Poll::Ready(());
    };
    PendingCallbacks(shared).run(&mut Reading::ByTask(cx), |_| true);
    Poll::Pending
}

/// How a callback's future is read: by the device's task, woken once it resolves, or at once,
/// without waiting.
enum Reading<'a, 'b> {
    ByTask(&'a mut Context<'b>),
    Now,
}

/// A callback waiting for the future of the request it was asked with.
enum PendingCallback {
    /// A buffer's range mapped, or why not.
    Mapping {
        buffer: Weak<Buffer>,
        answer: Answer<Result<(), String>>,
        callback: BufferMapCallback,
    },
    /// The work submitted to the queue before it was asked done.
    SubmittedWorkDone {
        answer: Answer<()>,
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

    /// The callback with its answer where `reading` finds its future resolved; itself where not.
    fn read(self, reading: &mut Reading<'_, '_>) -> Result<ResolvedCallback, PendingCallback> {
        match self {
            PendingCallback::Mapping {
                buffer,
                answer,
                callback,
            } => match answer.read(reading) {
                Ok(mapped) => Ok(ResolvedCallback::Mapping(callback, mapped)),
                Err(answer) => Err(PendingCallback::Mapping {
                    buffer,
                    answer,
                    callback,
                }),
            },
            PendingCallback::SubmittedWorkDone { answer, callback } => match answer.read(reading) {
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

/// A read of `FutureReader<T>`, in progress.
type FutureRead<T> = Pin<Box<<FutureReader<T> as IntoFuture>::IntoFuture>>;

/// The engine's future for a request, as the program reads it: not being read, or being read by
/// the device's task.
enum Answer<T: 'static> {
    Unread(FutureReader<T>),
    Reading(FutureRead<T>),
}

impl<T: 'static> Answer<T> {
    /// What the future resolved to, where `reading` finds it resolved; the future, read on or
    /// given back unread, where not.
    fn read(self, reading: &mut Reading<'_, '_>) -> Result<T, Answer<T>> {
        let mut read = match self {
            Answer::Unread(reader) => Box::pin(reader.into_future()),
            Answer::Reading(read) => read,
        };
        let cx = match reading {
            Reading::ByTask(cx) => cx,
            // A read that does not resolve as it starts is cancelled, which asks the engine once
            // more, and given back unread.
            Reading::Now => {
                if let Poll::Ready(value) =
                    read.as_mut().poll(&mut Context::from_waker(Waker::noop()))
                {
                    return Ok(value);
                }
                return read.as_mut().cancel().map_err(Answer::Unread);
            }
        };
        match read.as_mut().poll(cx) {
            Poll::Ready(value) => Ok(value),
            Poll::Pending => Err(Answer::Reading(read)),
        }
    }
}
