//! The programs running in WaOS, by the id of the program each is, whether `cargo waos` sent it or
//! the library opened it (decision 11 of the permissions proposal). One per id: a later run
//! `cargo waos` sends takes the place of the one before, which is stopped first and ends
//! `Replaced` (the design's decision 29), while a program the library opens again is left
//! running as it is. Every run of an id is ended before its program is removed or updated
//! ([`Runs::stop`]), as a system ends an app it uninstalls or updates, so no run outlives the
//! package it came from or the folders it was given. A run `cargo waos` sent is its own to read and drive, with a mouse of its own
//! over the program's windows; one the library opened is no tool's (the design's decision 60).
//!
//! The invariants: one run starts at a time. The lock is held from the moment the run before is
//! stopped, or found running, to the moment the new one is in the list, so two runs of one id
//! arriving together cannot both start, and an id never names two programs; and the ids running,
//! which [`Runs::running_ids`] tells, change with the list under the same lock; an id being
//! started is among those starting from before the lock is taken until the start is done, so
//! [`Runs::runs_or_starts`] misses no run about to be listed. And nothing
//! compiles under the lock, which every screenshot, input, open and quit waits for: a caller
//! has the program's code compiled before it starts a run.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, PoisonError};

use anyhow::Result;
use futures::future::join_all;
use tokio::sync::{Mutex, watch};
use waos::{ProgramEnd, ProgramView, RunningProgram};
use waos_dev_protocol::{RunEnd, Unreached};
use waos_package::ProgramId;

use super::dev_input::DevMouse;

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
    /// Set before the program is stopped for a later run of its id.
    replaced: Arc<AtomicBool>,
    /// The mouse `cargo waos`'s input moves over the program's windows, for a run it sent; none for
    /// one the library opened, which no tool reads or drives.
    mouse: Option<Arc<DevMouse>>,
}

/// A program a run of `cargo waos run` runs, which `cargo waos`'s screenshots and input read and
/// drive: the program, and the mouse that input moves over its windows.
#[derive(Clone)]
pub(crate) struct DevRun {
    pub(crate) program: RunningProgram,
    pub(crate) mouse: Arc<DevMouse>,
}

impl DevRun {
    /// The program's window numbered `number`, from 1 in the order its views arrived, as `cargo
    /// waos screenshot` lists them.
    pub(crate) fn window(&self, number: u32) -> Result<ProgramView, Unreached> {
        let views = self.program.views();
        let no_window = Unreached::NoWindow {
            window: number,
            windows: u32::try_from(views.len()).unwrap_or(u32::MAX),
        };
        let index = usize::try_from(number.checked_sub(1).ok_or(no_window.clone())?)
            .map_err(|_| no_window.clone())?;
        views.into_iter().nth(index).ok_or(no_window)
    }
}

/// What [`Runs::start_unless_running`] found: a run it started, or the program running already.
pub(crate) enum Started {
    New(Run),
    Running(RunningProgram),
}

/// One run, as its connection holds it.
pub(crate) struct Run {
    id: ProgramId,
    serial: u64,
    pub(crate) program: RunningProgram,
    replaced: Arc<AtomicBool>,
}

impl Runs {
    /// Starts a run `cargo waos` sent of the program `id`: the one running, if any, is stopped
    /// first and ends `Replaced`; then `start` starts the program, and the run is in the list,
    /// with a mouse of its own for `cargo waos`'s input. An error from `start` leaves the id free.
    /// `start` waits for nothing: the program it starts is loaded already.
    pub(crate) async fn start(
        &self,
        id: &ProgramId,
        start: impl FnOnce() -> Result<RunningProgram>,
    ) -> Result<Run> {
        let mut running = self.running.lock().await;
        if let Some(before) = running.remove(id) {
            self.ids.send_replace(running.keys().cloned().collect());
            tracing::info!(%id, "a later run of the program: stopping the one before");
            before.replaced.store(true, Ordering::SeqCst);
            before.program.stop();
            before.program.ended().await;
        }
        let program = start()?;
        let mouse = Arc::default();
        Ok(self.list(&mut running, id, program, Some(mouse)))
    }

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
        Ok(Started::New(self.list(&mut running, id, program, None)))
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

    /// The program `id` running now, if `cargo waos run` sent it; none for one the library
    /// opened.
    pub(crate) async fn dev_run(&self, id: &ProgramId) -> Option<DevRun> {
        let running = self.running.lock().await;
        let current = running.get(id)?;
        Some(DevRun {
            program: current.program.clone(),
            mouse: Arc::clone(current.mouse.as_ref()?),
        })
    }

    /// Puts `program` in the list as the program `id`, as a new run, with the mouse `cargo
    /// waos`'s input moves where it sent the run.
    fn list(
        &self,
        running: &mut HashMap<ProgramId, Running>,
        id: &ProgramId,
        program: RunningProgram,
        mouse: Option<Arc<DevMouse>>,
    ) -> Run {
        let serial = self.serials.fetch_add(1, Ordering::SeqCst);
        let replaced = Arc::new(AtomicBool::new(false));
        running.insert(
            id.clone(),
            Running {
                serial,
                program: program.clone(),
                replaced: Arc::clone(&replaced),
                mouse,
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

    /// The programs running now, in no order: what a quit asks.
    pub(crate) async fn programs(&self) -> Vec<RunningProgram> {
        let running = self.running.lock().await;
        running
            .values()
            .map(|current| current.program.clone())
            .collect()
    }

    /// Stops every run and waits until each program has ended.
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

impl Run {
    /// How the run ended, as its connection hears it: a stop for a later run of the id is
    /// `Replaced`.
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
