//! A program's workers: other instances of its code, each in a store of its own on a thread of its
//! own (`running::worker`), sharing nothing with the instance that started it and exchanging
//! messages, each a list of bytes, over a stream each way.
//!
//! A stream is a handle in one store's table and cannot be handed to another store, so each
//! instance's side of a worker's stream is the engine's own: what one instance writes ends at a
//! consumer that sends each message on a bounded channel (`stream::Forwarded`), and the other
//! instance reads the channel's messages as a stream the engine produces (`stream::stream_of`). A
//! message is copied twice, out of one memory and into the other, which
//! `../experiments/2026-11-13-workers` measured at about 21 GiB/s.
//!
//! The invariants: a message is taken from its writer only once the channel has room for it, so a
//! reader that stops reading stops the writer, as one stream would on its own; a [`Worker`]
//! dropped ends its instance; and every instance of a program ends before the program is told
//! ended ([`ProgramWorkers::end_all`]), the workers' workers among them.

pub(crate) mod host;

use std::sync::Arc;

use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, Shared};
use tokio::sync::watch;

use crate::bindings::wasdk::platform::workers::WorkerError;
use crate::stats::Stats;

/// How many workers one program may run at once, its workers' workers among them: about four
/// times the threads of a large machine, so a program that keeps every thread busy has room to
/// spare, and one that starts workers without end is refused before it runs the machine out of
/// threads.
pub(crate) const WORKER_LIMIT: usize = 64;

/// How many messages wait between a writer and its reader beyond the one each writer may always
/// send: none, so a writer waits for its reader, as on one stream.
const WAITING_MESSAGES: usize = 0;

/// The channel a worker's messages cross one way on.
pub(crate) fn message_channel() -> (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
    mpsc::channel(WAITING_MESSAGES)
}

/// `workers.workers`: the program's workers, as an instance reaches them.
pub struct Workers;

/// How a worker ended: `Err` with why it failed.
pub(crate) type WorkerEnd = Result<(), String>;

/// When a worker ended, as every `finished` resolves with it.
pub(crate) type Finished = Shared<BoxFuture<'static, WorkerEnd>>;

/// `workers.worker`: a running worker, as the instance that started it holds it. Dropping it
/// ends the worker.
pub struct Worker {
    /// What the worker sends, until the program takes it.
    messages: Option<mpsc::Receiver<Vec<u8>>>,
    finished: Finished,
    /// Ends the worker as it is dropped.
    stop: Option<oneshot::Sender<()>>,
}

impl Worker {
    pub(crate) fn new(
        messages: mpsc::Receiver<Vec<u8>>,
        finished: Finished,
        stop: oneshot::Sender<()>,
    ) -> Worker {
        Worker {
            messages: Some(messages),
            finished,
            stop: Some(stop),
        }
    }

    /// What the worker sends: the first time asked, and none after.
    fn take_messages(&mut self) -> Option<mpsc::Receiver<Vec<u8>>> {
        self.messages.take()
    }

    pub(crate) fn finished(&self) -> Finished {
        self.finished.clone()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            // A worker that has ended has nothing to stop.
            let _ = stop.send(());
        }
    }
}

/// Every worker one program runs now: how many, and whether the program is ending, which ends
/// them all and starts no more. Every clone is the same.
#[derive(Clone)]
pub(crate) struct ProgramWorkers(Arc<Roster>);

struct Roster {
    state: watch::Sender<RosterState>,
    stats: Arc<Stats>,
}

#[derive(Clone, Copy, Default)]
struct RosterState {
    running: usize,
    ending: bool,
}

impl ProgramWorkers {
    pub(crate) fn new(stats: Arc<Stats>) -> ProgramWorkers {
        ProgramWorkers(Arc::new(Roster {
            state: watch::Sender::new(RosterState::default()),
            stats,
        }))
    }

    /// A place for one more worker, held until the worker has ended; `refused` while the
    /// program runs as many as it may, or is ending.
    pub(crate) fn admit(&self) -> Result<WorkerPlace, WorkerError> {
        let mut refusal = None;
        self.0.state.send_if_modified(|state| {
            if state.ending {
                refusal = Some("the program is ending".to_owned());
                return false;
            }
            if state.running >= WORKER_LIMIT {
                refusal = Some(format!(
                    "a program runs at most {WORKER_LIMIT} workers at once"
                ));
                return false;
            }
            state.running += 1;
            true
        });
        if let Some(why) = refusal {
            return Err(WorkerError::Refused(why));
        }
        self.0.stats.worker_started();
        Ok(WorkerPlace(self.clone()))
    }

    /// Resolves once the program is ending: where every worker ends.
    pub(crate) fn ending(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let mut state = self.0.state.subscribe();
        async move {
            // The sender is the roster's, here as long as any worker's place.
            let _ = state.wait_for(|state| state.ending).await;
        }
    }

    /// Ends every worker, starts no more, and resolves once each has ended.
    pub(crate) fn end_all(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.0.state.send_modify(|state| state.ending = true);
        let mut state = self.0.state.subscribe();
        async move {
            let _ = state.wait_for(|state| state.running == 0).await;
        }
    }
}

/// One worker's place among its program's, given back as it is dropped, once its instance has
/// ended.
pub(crate) struct WorkerPlace(ProgramWorkers);

impl Drop for WorkerPlace {
    fn drop(&mut self) {
        let roster = &(self.0).0;
        // Counted before the roster wakes `end_all`, so a program told ended counts no worker.
        roster.stats.worker_ended();
        roster.state.send_modify(|state| state.running -= 1);
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use super::*;

    fn workers() -> (ProgramWorkers, Arc<Stats>) {
        let stats = Arc::new(Stats::default());
        (ProgramWorkers::new(Arc::clone(&stats)), stats)
    }

    #[test]
    fn a_program_runs_as_many_workers_as_it_may_and_no_more() {
        let (workers, stats) = workers();
        let places: Vec<WorkerPlace> = (0..WORKER_LIMIT)
            .map(|_| workers.admit().expect("room for it"))
            .collect();
        assert_eq!(stats.workers(), WORKER_LIMIT as u64);
        assert!(matches!(workers.admit(), Err(WorkerError::Refused(_))));
        drop(places);
        assert_eq!(stats.workers(), 0);
        assert!(workers.admit().is_ok(), "a place given back is free again");
    }

    #[test]
    fn ending_resolves_once_every_worker_has_ended_and_refuses_another() {
        let (workers, _stats) = workers();
        let place = workers.admit().unwrap();
        let mut ending = workers.ending().boxed();
        assert_eq!((&mut ending).now_or_never(), None);
        let mut ended = workers.end_all().boxed();
        assert_eq!(ending.now_or_never(), Some(()), "every worker hears it");
        assert_eq!((&mut ended).now_or_never(), None, "one still runs");
        assert!(matches!(workers.admit(), Err(WorkerError::Refused(_))));
        drop(place);
        assert_eq!(ended.now_or_never(), Some(()));
    }
}
