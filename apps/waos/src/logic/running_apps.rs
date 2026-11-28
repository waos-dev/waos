//! The apps running in WaOS, one per app, by its id, whoever started it: the library opening it,
//! `waos launch`, or an inspector. A start says what to do when the app runs already
//! ([`IfRunning`]): opening it again leaves it as it is, while a launch, or a start an inspector
//! asks for, replaces it, and the one before ends `Replaced`. An app is stopped before it is
//! removed or updated ([`RunningApps::stop`]), as a system ends an app it uninstalls or updates,
//! so no run outlives the package it came from or the folders it was given. This knows nothing of
//! packages: whoever starts an app hands it the app's program, loaded. Every app running is its
//! user's to read and drive through the dev socket, through its program's own mouse and the
//! keyboard, as `driven` finds it, which a phone, where `waos` reaches no WaOS yet, does not
//! compile. Each run knows whether a later one took its place, so its end is told as `Replaced`.
//!
//! The invariants: one start or stop of an app at a time. Each holds the app's turn from the moment
//! it looks for the run before to the moment the new one is in the list, so two starts of one app
//! arriving together cannot both start, and an id never names two programs. The list's own lock
//! is held only to read or change the list, never while a program starts or ends, so a program
//! slow to end holds back no screenshot, input, open or quit, nor another app's start; the ids
//! running, which [`RunningApps::running_ids`] tells, change with the list under that lock. And
//! nothing compiles in a turn: a caller loads the program, through the package manager, before it
//! starts the app. The package manager stops an app here before it changes (`StopApp`).

#[cfg(desktop)]
mod driven;

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use anyhow::Result;
use futures::FutureExt as _;
use futures::future::BoxFuture;
#[cfg(desktop)]
use futures::future::join_all;
use tokio::sync::{OwnedMutexGuard, watch};
use tracing::Instrument;
use waos::{ProgramEnd, RunningProgram};
use waos_dev_protocol::RunEnd;
use waos_package::ProgramId;
use waos_services::packages::{PackageChange, StopApp};

#[cfg(desktop)]
pub(crate) use driven::DrivenProgram;

/// The apps running, by id.
#[derive(Default)]
pub(crate) struct RunningApps {
    /// The programs in the list, held only to read or change it.
    running: Mutex<HashMap<ProgramId, Running>>,
    /// Each app's turn, one start or stop of the app at a time.
    turns: Mutex<HashMap<ProgramId, Arc<tokio::sync::Mutex<()>>>>,
    /// Tells one run of an id from the next.
    serials: AtomicU64,
    /// The ids in the list, for whoever shows which programs run.
    ids: watch::Sender<BTreeSet<ProgramId>>,
}

/// One program in the list.
struct Running {
    serial: u64,
    program: RunningProgram,
    /// Set before the program is stopped for a later run of its id, or a new build of it.
    replaced: Arc<AtomicBool>,
}

/// What to do when an app asked to start runs already.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IfRunning {
    /// Leave it running as it is, as opening an app again does.
    Leave,
    /// Stop it first, so it ends `Replaced`, and start the app again, as `waos launch` does.
    Replace,
}

/// What [`RunningApps::start`] came to: a run it started, or the program running already, left as
/// it is.
pub(crate) enum Started {
    New(Run),
    Running(RunningProgram),
}

/// One run, as whoever started it holds it.
pub(crate) struct Run {
    id: ProgramId,
    serial: u64,
    pub(crate) program: RunningProgram,
    replaced: Arc<AtomicBool>,
}

impl Run {
    /// How the run ended, as an attach hears it: a stop for a later run of the id, or a new build
    /// of it, is `Replaced`.
    pub(crate) fn end(&self, end: &ProgramEnd) -> RunEnd {
        match end {
            ProgramEnd::Returned(Ok(())) => RunEnd::Returned { success: true },
            ProgramEnd::Returned(Err(())) => RunEnd::Returned { success: false },
            ProgramEnd::Failed(error) => RunEnd::Failed(format!("{error:#}")),
            ProgramEnd::Stopped if self.replaced.load(Ordering::SeqCst) => RunEnd::Replaced,
            ProgramEnd::Stopped => RunEnd::Stopped,
        }
    }
}

impl RunningApps {
    /// Starts the app `id`: unless it runs already and `if_running` leaves it, `run` starts its
    /// program, which is in the list once it has started; one replaced is stopped first and ends
    /// `Replaced`. Answers the run started, or the program running. An error from `run` leaves the
    /// app not running. `run` waits for nothing: the program it starts is loaded already.
    pub(crate) async fn start(
        &self,
        id: &ProgramId,
        if_running: IfRunning,
        run: impl FnOnce() -> Result<RunningProgram>,
    ) -> Result<Started> {
        let _turn = self.turn_of(id).await;
        match if_running {
            IfRunning::Leave => {
                if let Some(current) = self.running(id) {
                    return Ok(Started::Running(current));
                }
            }
            IfRunning::Replace => self.end_the_one_before(id).await,
        }
        let program = run()?;
        Ok(Started::New(self.list(id, program)))
    }

    /// Stops the program running as `id`, if any, and waits until it has ended, out of the list
    /// first; it ends `Replaced`. The caller holds the id's turn.
    async fn end_the_one_before(&self, id: &ProgramId) {
        let Some(before) = self.take_out(id) else {
            return;
        };
        tracing::info!(%id, "a later run of the program: stopping the one before");
        before.replaced.store(true, Ordering::SeqCst);
        before.program.stop();
        before.program.ended().await;
    }

    /// Stops the program running as `id`, if any, for a new build of it installed in its place,
    /// and waits until it has ended: it ends `Replaced`. Answers whether one ran.
    pub(crate) async fn stop_replaced(&self, id: &ProgramId) -> bool {
        let _turn = self.turn_of(id).await;
        let ran = self.runs_now(id);
        self.end_the_one_before(id).await;
        ran
    }

    /// Whether a run of `id` is in the list now.
    fn runs_now(&self, id: &ProgramId) -> bool {
        self.listed().contains_key(id)
    }

    /// Takes the run of `id` out of the list, so nothing finds it running while it ends.
    fn take_out(&self, id: &ProgramId) -> Option<Running> {
        let mut running = self.listed();
        let current = running.remove(id)?;
        self.ids.send_replace(running.keys().cloned().collect());
        Some(current)
    }

    /// Takes `run` out of the list once its program ends, on a task of the runtime this runs on,
    /// telling WaOS's log how long it ran and `heard` how it ended, first.
    pub(crate) fn take_out_once_ended(
        self: &Arc<Self>,
        run: Run,
        heard: impl FnOnce(RunEnd) + Send + 'static,
    ) {
        let apps = Arc::clone(self);
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
                heard(run.end(&end));
                apps.ended(&run).await;
            }
            .instrument(span),
        );
    }

    /// Ends the run of the program `id`, if any, once a start under way is done, and waits until
    /// it has ended: it is out of the list first, so nothing finds it running meanwhile. Answers
    /// whether one ran.
    pub(crate) async fn stop(&self, id: &ProgramId) -> bool {
        let _turn = self.turn_of(id).await;
        let Some(current) = self.take_out(id) else {
            return false;
        };
        tracing::info!(%id, "its run is stopped");
        current.program.stop();
        current.program.ended().await;
        true
    }

    /// The ids running now, and each change of them from now on.
    pub(crate) fn running_ids(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        self.ids.subscribe()
    }

    /// The program `id` running now, if one is.
    pub(crate) fn running(&self, id: &ProgramId) -> Option<RunningProgram> {
        self.listed().get(id).map(|current| current.program.clone())
    }

    /// Puts `program` in the list as the program `id`, as a new run.
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
        let replaced = Arc::new(AtomicBool::new(false));
        running.insert(
            id.clone(),
            Running {
                serial,
                program: program.clone(),
                replaced: Arc::clone(&replaced),
            },
        );
        self.ids.send_replace(running.keys().cloned().collect());
        Run {
            id: id.clone(),
            serial,
            program,
            replaced,
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
    #[cfg(desktop)]
    pub(crate) async fn programs(&self) -> Vec<RunningProgram> {
        let running = self.listed();
        running
            .values()
            .map(|current| current.program.clone())
            .collect()
    }

    /// Stops every run and waits until each program has ended: once a quit is agreed to.
    #[cfg(desktop)]
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

/// The package manager stops an app among these before it changes: one an install replaces ends
/// `Replaced`, one removed `Stopped`.
impl StopApp for RunningApps {
    fn stop<'a>(&'a self, id: &'a ProgramId, change: PackageChange) -> BoxFuture<'a, ()> {
        async move {
            match change {
                PackageChange::Install => self.stop_replaced(id).await,
                PackageChange::Removal => RunningApps::stop(self, id).await,
            };
        }
        .boxed()
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
        let engine = test_engine::engine(Arc::new(NoWindows));
        let run = |name: &'static str| {
            let program = engine
                .load(
                    Path::new(waos_test_programs::FOREVER),
                    &waos_test_programs::code_of(waos_test_programs::FOREVER),
                )
                .unwrap()
                .named(name);

            engine.run(&program, ProgramEnvironment::default()).unwrap()
        };
        let apps = Arc::new(RunningApps::default());
        let slow: ProgramId = "dev.example.slow".parse().unwrap();
        let other: ProgramId = "dev.example.other".parse().unwrap();
        let (go_on, going_on) = std::sync::mpsc::channel::<()>();
        let (entered, under_way) = oneshot::channel::<()>();
        let slow_start = {
            let (apps, slow) = (Arc::clone(&apps), slow.clone());
            let program = run("dev.example.slow");
            tokio::spawn(async move {
                apps.start(&slow, IfRunning::Leave, move || {
                    entered.send(()).unwrap();
                    // Blocks this worker alone, as a program slow to start would.
                    going_on.recv().unwrap();
                    Ok(program)
                })
                .await
                .map(|_| ())
            })
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), under_way)
            .await
            .expect("its start is under way")
            .unwrap();
        let at_once = std::time::Duration::from_secs(1);
        let names = tokio::time::timeout(at_once, apps.names()).await;
        assert!(names.expect("the list is read meanwhile").is_empty());
        let other_program = run("dev.example.other");
        let started = tokio::time::timeout(
            at_once,
            apps.start(&other, IfRunning::Replace, || Ok(other_program.clone())),
        )
        .await
        .expect("and another app starts meanwhile")
        .unwrap();
        assert_eq!(apps.names().await, ["dev.example.other"]);
        go_on.send(()).unwrap();
        slow_start.await.unwrap().unwrap();
        assert!(apps.running(&slow).is_some());

        apps.stop_all().await;
        drop(started);
    }
}
