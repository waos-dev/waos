//! The programs running in WaOS, by the id of the program each is, whether `cargo waos` sent it or
//! the library opened it (decision 11 of the permissions proposal). One per id: a later run
//! `cargo waos` sends takes the place of the one before, which is stopped first and ends
//! `Replaced` (the design's decision 29), as does a start an inspector asks for, while a program
//! the library opens again is left running as it is. Every run of an id is ended before its program is removed or updated
//! ([`Runs::stop`]), as a system ends an app it uninstalls or updates, so no run outlives the
//! package it came from or the folders it was given. A run `cargo waos` sent is its own to read and drive, with a mouse of its own
//! over the program's windows; one the library opened is no tool's (the design's decision 60).
//! What a sent run has beside its program is `sent`'s, which a phone, where `cargo waos` reaches
//! no WaOS yet, does not compile.
//!
//! The invariants: one start or stop of an id at a time. Each holds the id's turn from the moment
//! it looks for the run before to the moment the new one is in the list, so two runs of one id
//! arriving together cannot both start, and an id never names two programs. The list's own lock
//! is held only to read or change the list, never while a program starts or ends, so a program
//! slow to end holds back no screenshot, input, open or quit, nor another program's start; the ids
//! running, which [`Runs::running_ids`] tells, change with the list under that lock. An id being
//! started is among those starting from before its turn is taken until the start is done, so
//! [`Runs::runs_or_starts`] misses no run about to be listed. And nothing compiles in a turn: a
//! caller has the program's code compiled before it starts a run.

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod sent;

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use anyhow::Result;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use futures::future::join_all;
use tokio::sync::{OwnedMutexGuard, watch};
use tracing::Instrument;
use waos::RunningProgram;
use waos_package::ProgramId;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) use sent::{DevRun, SentRun};

/// The runs by id.
#[derive(Default)]
pub(crate) struct Runs {
    /// The programs in the list, held only to read or change it.
    running: Mutex<HashMap<ProgramId, Running>>,
    /// Each id's turn, one start or stop of the id at a time.
    turns: Mutex<HashMap<ProgramId, Arc<tokio::sync::Mutex<()>>>>,
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
        let _turn = self.turn_of(id).await;
        if let Some(current) = self.listed().get(id) {
            return Ok(Started::Running(current.program.clone()));
        }
        let program = start().await?;
        Ok(Started::New(self.list(id, program)))
    }

    /// Starts a run of the program `id` in place of the one running, if any, which is stopped
    /// first, as a later run `cargo waos` sends takes the place of the one before (decision 18 of
    /// `docs/proposals/2026-11-15-inspector-roles-and-compatibility.md`); then `start` starts the
    /// program, and the run is in the list as one the library opened. An error from `start`
    /// leaves the id free. `start` waits for nothing: the program it starts is loaded already.
    pub(crate) async fn start_in_place(
        &self,
        id: &ProgramId,
        start: impl FnOnce() -> Result<RunningProgram>,
    ) -> Result<Run> {
        let _starting = Starting::of(self, id);
        let _turn = self.turn_of(id).await;
        self.end_the_one_before(id).await;
        let program = start()?;
        Ok(self.list(id, program))
    }

    /// Stops the program running as `id`, if any, and waits until it has ended, out of the list
    /// first; one `cargo waos` sent ends `Replaced`. The caller holds the id's turn.
    async fn end_the_one_before(&self, id: &ProgramId) {
        let Some(before) = self.take_out(id) else {
            return;
        };
        tracing::info!(%id, "a later run of the program: stopping the one before");
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        if let Some(sent) = &before.sent {
            sent.replaced();
        }
        before.program.stop();
        before.program.ended().await;
    }

    /// Takes the run of `id` out of the list, so nothing finds it running while it ends.
    fn take_out(&self, id: &ProgramId) -> Option<Running> {
        let mut running = self.listed();
        let current = running.remove(id)?;
        self.ids.send_replace(running.keys().cloned().collect());
        Some(current)
    }

    /// Takes `run` out of the list once its program ends, on a task of the runtime this runs on,
    /// telling WaOS's log how long it ran.
    pub(crate) fn take_out_once_ended(self: &Arc<Self>, run: Run) {
        let runs = Arc::clone(self);
        let started_at = Instant::now();
        let span = tracing::info_span!("run", name = %run.id);
        tokio::spawn(
            async move {
                let end = run.program.ended().await;
                tracing::info!(
                    ?end,
                    seconds = started_at.elapsed().as_secs_f64(),
                    frames_presented = run.program.stats().frames_presented(),
                    "ended"
                );
                runs.ended(&run).await;
            }
            .instrument(span),
        );
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
        let _turn = self.turn_of(id).await;
        let Some(current) = self.take_out(id) else {
            return;
        };
        tracing::info!(%id, "its run is stopped");
        current.program.stop();
        current.program.ended().await;
    }

    /// The ids running now, and each change of them from now on.
    pub(crate) fn running_ids(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        self.ids.subscribe()
    }

    /// The program `id` running now, if one is.
    pub(crate) async fn running(&self, id: &ProgramId) -> Option<RunningProgram> {
        self.running_now(id)
    }

    /// The program `id` running now, if one is, without waiting.
    pub(crate) fn running_now(&self, id: &ProgramId) -> Option<RunningProgram> {
        self.listed().get(id).map(|current| current.program.clone())
    }

    /// Puts `program` in the list as the program `id`, as a new run, which nothing sent.
    fn list(&self, id: &ProgramId, program: RunningProgram) -> Run {
        let mut running = self.listed();
        self.list_in(&mut running, id, program)
    }

    /// Puts `program` in `running`, the list, as the program `id`.
    fn list_in(
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
        let mut running = self.listed();
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
        self.listed().keys().map(ToString::to_string).collect()
    }

    /// The programs running now, in no order: what a quit asks, which a phone's system never
    /// does.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn programs(&self) -> Vec<RunningProgram> {
        let running = self.listed();
        running
            .values()
            .map(|current| current.program.clone())
            .collect()
    }

    /// Stops every run and waits until each program has ended: once a quit is agreed to.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn stop_all(&self) {
        let ends: Vec<_> = self
            .listed()
            .values()
            .map(|current| {
                current.program.stop();
                current.program.ended()
            })
            .collect();
        join_all(ends).await;
    }

    /// The list, for a moment: never held across an await.
    fn listed(&self) -> MutexGuard<'_, HashMap<ProgramId, Running>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The turn of the id `id`, once no other start or stop of it holds it.
    async fn turn_of(&self, id: &ProgramId) -> OwnedMutexGuard<()> {
        let turn = {
            let mut turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(turns.entry(id.clone()).or_default())
        };
        turn.lock_owned().await
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use futures::channel::oneshot;
    use waos::ProgramEnvironment;

    use super::*;
    use crate::logic::test_engine::{self, NoWindows};

    #[tokio::test(flavor = "multi_thread")]
    async fn the_list_is_read_and_another_program_started_while_a_start_waits() {
        let engine = test_engine::engine(
            Arc::new(NoWindows),
            Path::new(waos_test_programs::CODE_CACHE),
        );
        let run = |name: &'static str| {
            let program = engine
                .load(Path::new(waos_test_programs::FOREVER))
                .unwrap()
                .named(name);
            let (running, _views) = engine
                .run(&program, ProgramEnvironment::default(), Vec::new())
                .unwrap();
            running
        };
        let runs = Arc::new(Runs::default());
        let slow: ProgramId = "dev.example.slow".parse().unwrap();
        let other: ProgramId = "dev.example.other".parse().unwrap();
        let (go_on, going_on) = oneshot::channel::<()>();
        let slow_start = {
            let (runs, slow) = (Arc::clone(&runs), slow.clone());
            let program = run("dev.example.slow");
            tokio::spawn(async move {
                runs.start_unless_running(&slow, async || {
                    going_on.await.unwrap();
                    Ok(program)
                })
                .await
                .map(|_| ())
            })
        };
        let under_way = async {
            while !runs.runs_or_starts(&slow) {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), under_way)
            .await
            .expect("its start is under way");
        let at_once = std::time::Duration::from_secs(1);
        let names = tokio::time::timeout(at_once, runs.names()).await;
        assert!(names.expect("the list is read meanwhile").is_empty());
        let other_program = run("dev.example.other");
        let started = tokio::time::timeout(
            at_once,
            runs.start_in_place(&other, || Ok(other_program.clone())),
        )
        .await
        .expect("and another program starts meanwhile")
        .unwrap();
        assert_eq!(runs.names().await, ["dev.example.other"]);
        go_on.send(()).unwrap();
        slow_start.await.unwrap().unwrap();
        assert!(runs.running(&slow).await.is_some());

        runs.stop_all().await;
        drop(started);
    }
}
