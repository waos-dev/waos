//! Opening an installed program: its component, from the store, its code waited for while it is
//! compiled (`installed_code`), as at its install, or compiled where the cache holds none current,
//! then loaded off the caller's thread and run in WaOS as `waos launch` runs one, its output to
//! WaOS's log and to whoever attaches (`launch_outputs`),
//! with this computer's capabilities, the grants of its own settings and its own folders, its views
//! going where its place says, set before it starts (`program_places`). It is named by its id,
//! which names it among the runs, so a quit asks it while it runs, in WaOS's log, where its output
//! goes under the `program` target, and to the questions it puts to its user. A program running
//! already is left as it is. A tool, a program
//! its manifest declares has no UI, opened by the user from an icon is refused: no opening here has
//! a terminal for its input and output, and WaOS shows none of its own yet (decisions 3 and 7 of
//! `docs/proposals/2026-11-21-views-a-program-asks-for.md`). One started to be handed files or a
//! click on its notification runs as any program does, which it may answer with no window.
//!
//! The invariant: a run started here is taken out of the runs once it ends, by a task of the
//! runtime it was started on, its output told how it ended first, so the runs never hold an ended
//! program of the library's and every attach hears its end; and every run of every program, the
//! library's, `waos launch`'s and an inspector's, writes to an output of `outputs`, so any program
//! running is attached to.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use waos::{ProgramEnvironment, ProgramOutput, RunningProgram, WaOS};
use waos_package::{ProgramId, ProgramKind};

use super::environments::ProgramEnvironments;
use super::installed_code::InstalledCode;
use super::launch_outputs::{LaunchOutput, LaunchOutputs};
use super::program_places::{ProgramPlace, ProgramPlaces};
use super::runs::{Run, Runs, Started};

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

/// What opening an installed program came to.
pub(crate) enum OpenedRun {
    /// It started now.
    Started(RunningProgram),
    /// It was running already, and is left as it is.
    RunningAlready(RunningProgram),
}

impl OpenedRun {
    pub(crate) fn program(&self) -> &RunningProgram {
        match self {
            OpenedRun::Started(program) | OpenedRun::RunningAlready(program) => program,
        }
    }
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
    /// What each program running writes, for the attaches to it.
    pub(crate) outputs: Arc<LaunchOutputs>,
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
            outputs: Arc::default(),
        }
    }

    /// What the program `id` runs with, `environment`, with its output to a new output of
    /// `outputs`, the log's and the attaches': call it as the run starts, never for a program that
    /// runs already, whose output it would take the place of.
    pub(crate) fn with_output(
        &self,
        id: &ProgramId,
        environment: ProgramEnvironment,
    ) -> (ProgramEnvironment, Arc<LaunchOutput>) {
        let output = self.outputs.launched(id);
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(&output) as Arc<dyn ProgramOutput>),
            ..environment
        };
        (environment, output)
    }

    /// Takes `run`, of the program `id`, out of the runs once it ends, on a task of the runtime
    /// this runs on, telling `output` how it ended first, and letting go of it once an attach heard.
    pub(crate) fn take_out_once_ended(&self, id: &ProgramId, run: Run, output: Arc<LaunchOutput>) {
        let (outputs, id) = (Arc::clone(&self.outputs), id.clone());
        self.runs.take_out_once_ended(run, move |end| {
            if output.ended(end) {
                outputs.heard(&id, &output);
            }
        });
    }

    /// Runs the installed program `id`, whose component is at `component`, its views going to
    /// `place`, unless it runs already, which keeps its place, for `opening`, which refuses a tool
    /// the user opened; answers the program once it has started, or the one running, or why it
    /// could not start. Its end is waited for on a task of the runtime this runs on.
    pub(crate) async fn open(
        &self,
        id: &ProgramId,
        component: PathBuf,
        place: ProgramPlace,
        opening: Opening,
    ) -> Result<OpenedRun> {
        let name = id.as_str();
        let mut output = None;
        self.code_ready(id, &component)
            .await
            .with_context(|| format!("{id} could not be opened"))?;
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
                let (environment, its_output) = self.with_output(id, self.environments.of(id)?);
                output = Some(its_output);
                self.places.set(id, place);
                self.engine.run(&program, environment)
            })
            .await
            .with_context(|| format!("{id} could not be opened"))?;
        let run = match started {
            Started::New(run) => run,
            Started::Running(program) => {
                tracing::info!(%id, "opened again while it runs: left as it is");
                return Ok(OpenedRun::RunningAlready(program));
            }
        };
        tracing::info!(%id, "opened from the library");
        let program = run.program.clone();
        let output = output.context("a run that started writes to its output")?;
        self.take_out_once_ended(id, run, output);
        Ok(OpenedRun::Started(program))
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
