//! Opening an installed program: its component, from the store, its code waited for while it is
//! compiled (`installed_code`), as at its install, or compiled where the cache holds none current,
//! then loaded off the caller's thread and run in WaOS as `waos run` runs what it sends,
//! with this computer's capabilities, the grants of its own settings and its own folders, and the
//! screen reserved for its first request where WaOS shows one already, a tab of the deck or a card
//! Android restored, where its views go set before it starts (`program_places`). It is named by its
//! id, which names it among the runs, so a quit asks it while it runs, in WaOS's log, where its
//! output goes under the `program` target, and to the questions it puts to its user. A program
//! running already is left as it is, and the screen reserved for it goes unused. A tool, a program
//! its manifest declares has no UI, opened by the user from an icon is refused: no opening here has
//! a terminal for its input and output, and WaOS shows none of its own yet (decisions 3 and 7 of
//! `docs/proposals/2026-11-21-views-a-program-asks-for.md`). One started to be handed files or a
//! click on its notification runs as any program does, which it may answer with no window.
//!
//! The invariant: a run started here is taken out of the runs once it ends, by a task of the
//! runtime it was started on, so the runs never hold an ended program of the library's.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use waos::{PlacedView, ReservedView, RunningProgram, WaOS};
use waos_package::{ProgramId, ProgramKind};

use super::environments::ProgramEnvironments;
use super::installed_code::InstalledCode;
use super::program_places::{ProgramPlace, ProgramPlaces};
use super::runs::{Runs, Started};

/// Why a tool the user opens does not open: there is nowhere for its input and output to go.
const NO_TERMINAL: &str = "it is a command-line program, with no window of its own, and WaOS cannot show its terminal yet";

/// Why an installed program is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opening {
    /// The user opened it from an icon: the library's, the launcher's, the Dock's, a shortcut
    /// on a phone's home screen, or a card brought back from Recents.
    ByTheUser,
    /// To hand it what the user did outside it: files opened with it, or a click on one of its
    /// notifications.
    ToHandOver,
}

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
    /// unless it runs already, which keeps its place, for `opening`, which refuses a tool the user
    /// opened; answers the program once it has started,
    /// with what resolves with the view `reserved` becomes, or the one running, with none, or why
    /// it could not start. Its end is waited for on a task of the runtime this runs on.
    pub(crate) async fn open(
        &self,
        id: &ProgramId,
        component: PathBuf,
        reserved: Option<PlacedView>,
        place: ProgramPlace,
        opening: Opening,
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
                ensure!(
                    opening != Opening::ByTheUser
                        || self.environments.kind_of(id) == ProgramKind::App,
                    NO_TERMINAL
                );
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
