//! The programs running in WaOS, by the id of the program each is, whether `cargo waos` sent it or
//! the library opened it (decision 11 of the permissions proposal). One per id: a later run
//! `cargo waos` sends takes the place of the one before, which is stopped first and ends
//! `Replaced` (the design's decision 29), while a program the library opens again is left
//! running as it is. Every run of an id is ended before its program is removed or updated
//! ([`Runs::stop`]), as a system ends an app it uninstalls or updates, so no run outlives the
//! package it came from or the folders it was given. A run `cargo waos` sent is its own to read and drive, with a mouse of its own
//! over the program's windows; one the library opened is no tool's (the design's decision 60).
//! What a sent run has beside its program is `sent`'s, which a phone, where `cargo waos` reaches
//! no WaOS yet, does not compile.
//!
//! The invariants: one run starts at a time. The lock is held from the moment the run before is
//! stopped, or found running, to the moment the new one is in the list, so two runs of one id
//! arriving together cannot both start, and an id never names two programs; and the ids running,
//! which [`Runs::running_ids`] tells, change with the list under the same lock; an id being
//! started is among those starting from before the lock is taken until the start is done, so
//! [`Runs::runs_or_starts`] misses no run about to be listed. And nothing
//! compiles under the lock, which every screenshot, input, open and quit waits for: a caller
//! has the program's code compiled before it starts a run.

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod sent;

use std::collections::{BTreeSet, HashMap};
use std::sync::PoisonError;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use futures::future::join_all;
use tokio::sync::{Mutex, watch};
use waos::RunningProgram;
use waos_package::ProgramId;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) use sent::{DevRun, SentRun};

/// The runs by id.
#[derive(Default)]
pub(crate) struct Runs {
    running: Mutex<HashMap<ProgramId, Running>>,
    /// Tells one run of an id from the next.
    serials: AtomicU64,
    /// The ids in the list, for whoever shows which programs run.
    ids: watch::Sender<BTreeSet<ProgramId>>,
    /// The ids the library is starting a run of now, each as many times as it is.
    starting: std::sync::Mutex<Vec<ProgramId>>,
}

/// One program in the list.
struct Running {
    serial: u64,
    program: RunningProgram,
    /// What a run `cargo waos` sent has beside its program; none for one the library opened, which
    /// no tool reads or drives.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    sent: Option<sent::Sent>,
}

/// What [`Runs::start_unless_running`] found: a run it started, or the program running already.
pub(crate) enum Started {
    New(Run),
    Running(RunningProgram),
}

/// One run, as whoever started it holds it.
pub(crate) struct Run {
    id: ProgramId,
    serial: u64,
    pub(crate) program: RunningProgram,
}

impl Runs {
    /// Starts a run of the program `id` unless one runs already, which is left as it is; then
    /// `start` starts the program, and the run is in the list. Answers the run started, or the
    /// program running. An error from `start` leaves the id free.
    pub(crate) async fn start_unless_running(
        &self,
        id: &ProgramId,
        start: impl AsyncFnOnce() -> Result<RunningProgram>,
    ) -> Result<Started> {
        let _starting = Starting::of(self, id);
        let mut running = self.running.lock().await;
        if let Some(current) = running.get(id) {
            return Ok(Started::Running(current.program.clone()));
        }
        let program = start().await?;
        Ok(Started::New(self.list(&mut running, id, program)))
    }

    /// Whether a run of the program `id` is in the list, or being started: what an end of its
    /// runs has to wait for.
    pub(crate) fn runs_or_starts(&self, id: &ProgramId) -> bool {
        self.ids.borrow().contains(id)
            || self
                .starting
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(id)
    }

    /// Ends the run of the program `id`, if any, once a start under way is done, and waits until
    /// it has ended: it is out of the list first, so nothing finds it running meanwhile.
    pub(crate) async fn stop(&self, id: &ProgramId) {
        let mut running = self.running.lock().await;
        let Some(current) = running.remove(id) else {
            return;
        };
        self.ids.send_replace(running.keys().cloned().collect());
        tracing::info!(%id, "the program is removed or updated: its run ends");
        current.program.stop();
        current.program.ended().await;
    }

    /// The ids running now, and each change of them from now on.
    pub(crate) fn running_ids(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        self.ids.subscribe()
    }

    /// The program `id` running now, if one is.
    pub(crate) async fn running(&self, id: &ProgramId) -> Option<RunningProgram> {
        let running = self.running.lock().await;
        running.get(id).map(|current| current.program.clone())
    }

    /// Puts `program` in the list as the program `id`, as a new run, which nothing sent.
    fn list(
        &self,
        running: &mut HashMap<ProgramId, Running>,
        id: &ProgramId,
        program: RunningProgram,
    ) -> Run {
        let serial = self.serials.fetch_add(1, Ordering::SeqCst);
        running.insert(
            id.clone(),
            Running {
                serial,
                program: program.clone(),
                #[cfg(not(any(target_os = "android", target_os = "ios")))]
                sent: None,
            },
        );
        self.ids.send_replace(running.keys().cloned().collect());
        Run {
            id: id.clone(),
            serial,
            program,
        }
    }

    /// Takes `run` out of the list once it has ended, unless a later run of its id took its place
    /// already.
    pub(crate) async fn ended(&self, run: &Run) {
        let mut running = self.running.lock().await;
        if running
            .get(&run.id)
            .is_some_and(|current| current.serial == run.serial)
        {
            running.remove(&run.id);
            self.ids.send_replace(running.keys().cloned().collect());
        }
    }

    /// The ids running now, as text, in no order: the tests' window.
    #[cfg(test)]
    pub(crate) async fn names(&self) -> Vec<String> {
        let running = self.running.lock().await;
        running.keys().map(ToString::to_string).collect()
    }

    /// The programs running now, in no order: what a quit asks, which a phone's system never
    /// does.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn programs(&self) -> Vec<RunningProgram> {
        let running = self.running.lock().await;
        running
            .values()
            .map(|current| current.program.clone())
            .collect()
    }

    /// Stops every run and waits until each program has ended: once a quit is agreed to.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn stop_all(&self) {
        let ends: Vec<_> = {
            let running = self.running.lock().await;
            running
                .values()
                .map(|current| {
                    current.program.stop();
                    current.program.ended()
                })
                .collect()
        };
        join_all(ends).await;
    }
}

/// An id among those being started, for as long as this is held.
struct Starting<'a> {
    runs: &'a Runs,
    id: ProgramId,
}

impl<'a> Starting<'a> {
    fn of(runs: &'a Runs, id: &ProgramId) -> Starting<'a> {
        runs.starting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(id.clone());
        Starting {
            runs,
            id: id.clone(),
        }
    }
}

impl Drop for Starting<'_> {
    fn drop(&mut self) {
        let mut starting = self
            .runs
            .starting
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(index) = starting.iter().position(|id| *id == self.id) {
            starting.remove(index);
        }
    }
}
