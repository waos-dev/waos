//! A worker as it runs: another instance of its program's code, in a store of its own, on a
//! thread of its own with a single-threaded tokio runtime, as the program's main instance runs,
//! from its start to its end.
//!
//! The invariant: the worker's store lives on its thread alone, and the thread gives its place
//! among the program's workers back only once the store is gone, so a program told it has no
//! workers left has none running. The thread ends the worker when its work is done, when its
//! `Worker` is dropped, and when its program ends.

use std::sync::Arc;
use std::thread;

use futures::FutureExt;
use futures::channel::mpsc;
use futures::channel::oneshot::{self, Canceled};

use super::instances::ProgramInstances;
use super::store;
use crate::bindings::wasdk::platform::workers::WorkerError;
use crate::targets;
use crate::workers::{Finished, Worker, WorkerEnd, WorkerPlace, message_channel};

/// Whether the new instance's `run` took its entry.
pub(super) type Acceptance = oneshot::Sender<Result<(), WorkerError>>;

/// Starts a worker of `instances`' program at `entry`, reading `inbox`, on a thread of its own;
/// resolves once its `run` has accepted the entry, or with why it did not start.
pub(crate) fn start_worker(
    instances: &Arc<ProgramInstances>,
    entry: String,
    inbox: mpsc::Receiver<Vec<u8>>,
) -> impl Future<Output = Result<Worker, WorkerError>> + Send + 'static + use<> {
    let spawned = spawn(instances, entry, inbox);
    async move {
        let (accepted, worker) = spawned?;
        match accepted.await {
            Ok(Ok(())) => Ok(worker),
            Ok(Err(refused)) => Err(refused),
            Err(Canceled) => Err(WorkerError::Refused(why_it_ended(worker.finished()).await)),
        }
    }
}

/// Why a worker that never answered ended.
async fn why_it_ended(finished: Finished) -> String {
    match finished.await {
        Err(why) => why,
        Ok(()) => "the worker ended before it answered".to_owned(),
    }
}

/// The worker's thread, started: what says whether its `run` accepted the entry, and the
/// worker.
fn spawn(
    instances: &Arc<ProgramInstances>,
    entry: String,
    inbox: mpsc::Receiver<Vec<u8>>,
) -> Result<(oneshot::Receiver<Result<(), WorkerError>>, Worker), WorkerError> {
    let place = instances.program.0.workers.admit()?;
    let (outbox, messages) = message_channel();
    let (accept, accepted) = oneshot::channel();
    let (stop, stopped) = oneshot::channel();
    let (end, ended) = oneshot::channel();
    let finished = ended.map(end_of_thread).boxed().shared();
    let thread_name = format!("worker {entry} of {}", instances.name);
    let on_thread = Arc::clone(instances);
    thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let run = WorkerRun {
                entry,
                inbox,
                outbox,
                accept,
            };
            // Nobody waiting for the end is not an error.
            let _ = end.send(run_on_this_thread(&on_thread, run, stopped, place));
        })
        .map_err(|error| WorkerError::Refused(format!("no thread for the worker: {error}")))?;
    Ok((accepted, Worker::new(messages, finished, stop)))
}

/// The worker's thread ended without saying how: it panicked.
fn end_of_thread(end: Result<WorkerEnd, Canceled>) -> WorkerEnd {
    end.unwrap_or_else(|Canceled| Err("the worker's thread ended without saying how".to_owned()))
}

/// What one worker's run is given.
pub(super) struct WorkerRun {
    pub(super) entry: String,
    pub(super) inbox: mpsc::Receiver<Vec<u8>>,
    pub(super) outbox: mpsc::Sender<Vec<u8>>,
    pub(super) accept: Acceptance,
}

/// Runs the worker to its end on this thread, under its program's span, reporting into the
/// program's activity while it is polled, until its work is done, it is dropped (`stopped`) or
/// its program ends; then gives `place` back, the store gone.
fn run_on_this_thread(
    instances: &Arc<ProgramInstances>,
    run: WorkerRun,
    stopped: oneshot::Receiver<()>,
    place: WorkerPlace,
) -> WorkerEnd {
    let program = &instances.program;
    let span = tracing::info_span!(
        target: targets::PROGRAM,
        parent: &program.0.span,
        "worker",
        entry = %run.entry
    );
    let _entered = span.enter();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return Err(format!("no runtime for the worker: {error}")),
    };
    let ending = program.0.workers.ending();
    let end = runtime.block_on(async {
        let run = program.0.activity.watch(store::run_worker(instances, run));
        tokio::select! {
            end = run => end,
            // Dropped, or never handed out: either way nobody holds the worker.
            _ = stopped => Ok(()),
            () = ending => Ok(()),
        }
    });
    drop(place);
    end
}
