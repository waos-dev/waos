//! Opening an installed program: its component, from the store, its code waited for while it is
//! compiled (`installed_code`), as at its install, or compiled where the cache holds none current,
//! then loaded off the caller's thread and run in WaOS as `waos run` runs what it sends,
//! with this computer's capabilities, the grants of its own settings and its own folders, and the
//! screen reserved for its first request where WaOS shows one already, a tab of the deck or a card
//! Android restored, where its views go set before it starts (`program_places`). It is named by its
//! id, which names it among the runs, so a quit asks it while it runs, in WaOS's log, where its
//! output goes under the `program` target, and to the questions it puts to its user. A program
//! running already is left as it is, and the screen reserved for it goes unused.
//!
//! The invariant: a run started here is taken out of the runs once it ends, by a task of the
//! runtime it was started on, so the runs never hold an ended program of the library's.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use waos::{PlacedView, ReservedView, RunningProgram, WaOS};
use waos_package::ProgramId;

use super::environments::ProgramEnvironments;
use super::installed_code::InstalledCode;
use super::program_places::{ProgramPlace, ProgramPlaces};
use super::runs::{Runs, Started};

/// What opening an installed program needs of the app.
#[derive(Clone)]
pub(crate) struct InstalledRuns {
    pub(crate) engine: Arc<WaOS>,
    /// The installed programs' code, which an open waits for while it is compiled.
    pub(crate) code: Arc<InstalledCode>,
    /// What each program runs with.
    pub(crate) environments: Arc<ProgramEnvironments>,
    pub(crate) runs: Arc<Runs>,
    /// Where each program's views go, which the engine's placement reads.
    pub(crate) places: ProgramPlaces,
}

impl InstalledRuns {
    /// Opening installed programs on `engine`, each run with what `environments` gives it, among
    /// `runs`, its views going where `places` says.
    pub(crate) fn new(
        engine: Arc<WaOS>,
        environments: Arc<ProgramEnvironments>,
        runs: Arc<Runs>,
        places: ProgramPlaces,
    ) -> InstalledRuns {
        InstalledRuns {
            code: Arc::new(InstalledCode::new(Arc::clone(&engine))),
            engine,
            environments,
            runs,
            places,
        }
    }

    /// Runs the installed program `id`, whose component is at `component`, with `reserved` kept for
    /// its first request where WaOS shows a screen for it already, and its views going to `place`,
    /// unless it runs already, which keeps its place; answers the program once it has started,
    /// with what resolves with the view `reserved` becomes, or the one running, with none, or why
    /// it could not start. Its end is waited for on a task of the runtime this runs on.
    pub(crate) async fn open(
        &self,
        id: &ProgramId,
        component: PathBuf,
        reserved: Option<PlacedView>,
        place: ProgramPlace,
    ) -> Result<(RunningProgram, Option<ReservedView>)> {
        let name = id.as_str();
        self.code_ready(id, &component)
            .await
            .with_context(|| format!("{id} could not be opened"))?;
        let mut waiting = None;
        let started = self
            .runs
            .start_unless_running(id, async || {
                let engine = Arc::clone(&self.engine);
                let loading = tokio::task::spawn_blocking(move || engine.load(&component));
                let program = loading.await.context("the load's thread")??.named(name);
                let environment = self.environments.of(id)?;
                self.places.set(id, place);
                Ok(match reserved {
                    Some(reserved) => {
                        let (running, taken) =
                            self.engine.run_reserving(&program, environment, reserved)?;
                        waiting = Some(taken);
                        running
                    }
                    None => self.engine.run(&program, environment)?,
                })
            })
            .await
            .with_context(|| format!("{id} could not be opened"))?;
        let run = match started {
            Started::New(run) => run,
            Started::Running(program) => {
                tracing::info!(%id, "opened again while it runs: left as it is");
                return Ok((program, None));
            }
        };
        tracing::info!(%id, "opened from the library");
        let program = run.program.clone();
        self.runs.take_out_once_ended(run);
        Ok((program, waiting))
    }

    /// Resolves once the code of the program `id`, whose component is at `component`, is in the
    /// cache, or at once while the program runs. Waited for before the runs' lock is taken, which
    /// every start and every quit takes, so a compile holds back no other program's open.
    async fn code_ready(&self, id: &ProgramId, component: &Path) -> Result<()> {
        if self.runs.running(id).await.is_some() {
            return Ok(());
        }
        self.code.compile(id, component).await
    }
}
