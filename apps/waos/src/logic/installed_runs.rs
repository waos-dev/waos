//! Starting a run of an installed program, however it is asked for: opened by the user or to be
//! handed something (`open`), launched by `waos launch` with its arguments and variables
//! (`launch`), or held for an inspector (`start_held`). Each loads the program through the package
//! manager (`package_manager`), which compiles its code first where it is missing, then runs it in
//! WaOS with this computer's capabilities, the grants of its own settings and its own folders, its
//! output to WaOS's log and to whoever attaches (`launch_outputs`), its views going where its
//! place says, set before it starts (`program_places`). It is named by its id, which names it
//! among the runs, so a quit asks it while it runs, in WaOS's log, where its output goes under the
//! `program` target, and to the questions it puts to its user. An open leaves a program running
//! already as it is; a launch and a held start take the place of the run before, which ends
//! `Replaced`. A tool, a program its manifest declares has no UI, opened by the user from an icon
//! is refused: no opening here has a terminal for its input and output, and WaOS shows none of
//! its own yet. One
//! started to be handed files or a click on its notification runs as any program does, which it
//! may answer with no window.
//!
//! The invariant: a run started here is taken out of the runs once it ends, by a task of the
//! runtime it was started on, its output told how it ended first, so the runs never hold an ended
//! program and every attach hears its end; every run of every program writes to an output of
//! `outputs`, so any program running is attached to; and the program loaded keeps its app's turn
//! until its run is among the runs, so no removal or update lands between the two.

use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use waos::{HeldProgram, ProgramEnvironment, ProgramOutput, RunningProgram, WaOS};
use waos_package::{ProgramId, ProgramKind};
use waos_store::Store;

use super::environments::ProgramEnvironments;
use super::launch_outputs::{LaunchOutput, LaunchOutputs};
use super::package_manager::PackageManager;
use super::program_places::{ProgramPlace, ProgramPlaces};
use super::running_apps::{IfRunning, Run, RunningApps, Started};

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
    /// What installs, removes and loads the programs.
    pub(crate) packages: Arc<PackageManager>,
    /// What each program runs with.
    pub(crate) environments: Arc<ProgramEnvironments>,
    pub(crate) runs: Arc<RunningApps>,
    /// Where each program's views go, which the engine's placement reads.
    pub(crate) places: ProgramPlaces,
    /// What each program running writes, for the attaches to it.
    pub(crate) outputs: Arc<LaunchOutputs>,
}

impl InstalledRuns {
    /// Running the programs installed in `store` on `engine`, each run with what `environments`
    /// gives it, among `runs`, its views going where `places` says.
    pub(crate) fn new(
        engine: Arc<WaOS>,
        store: Store,
        environments: Arc<ProgramEnvironments>,
        runs: Arc<RunningApps>,
        places: ProgramPlaces,
    ) -> InstalledRuns {
        InstalledRuns {
            packages: Arc::new(PackageManager::new(
                store,
                Arc::clone(&engine),
                Arc::clone(&runs),
            )),
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

    /// Runs the installed program `id`, its views going to `place`, unless it runs already,
    /// which keeps its place, for `opening`, which refuses a tool the user opened; answers the
    /// program once it has started, or the one running, or why it could not start. Its end is
    /// waited for on a task of the runtime this runs on.
    pub(crate) async fn open(
        &self,
        id: &ProgramId,
        place: ProgramPlace,
        opening: Opening,
    ) -> Result<OpenedRun> {
        if let Some(program) = self.runs.running(id) {
            tracing::info!(%id, "opened again while it runs: left as it is");
            return Ok(OpenedRun::RunningAlready(program));
        }
        let mut output = None;
        let started = async {
            let loaded = self.packages.load(id).await?;
            self.runs
                .start(id, IfRunning::Leave, || {
                    ensure!(
                        opening != Opening::ByTheUser
                            || self.environments.kind_of(id) == ProgramKind::App,
                        NO_TERMINAL
                    );
                    let (environment, its_output) = self.with_output(id, self.environments.of(id)?);
                    output = Some(its_output);
                    self.places.set(id, place);
                    self.engine.run(&loaded.program, environment)
                })
                .await
        }
        .await
        .with_context(|| format!("{id} could not be opened"))?;
        let run = match started {
            Started::New(run) => run,
            Started::Running(program) => {
                tracing::info!(%id, "opened again while it runs: left as it is");
                return Ok(OpenedRun::RunningAlready(program));
            }
        };
        tracing::info!(%id, "opened");
        let program = run.program.clone();
        let output = output.context("a run that started writes to its output")?;
        self.take_out_once_ended(id, run, output);
        Ok(OpenedRun::Started(program))
    }

    /// Runs the installed program `id` with `arguments` and the `variables` it alone adds
    ///, in place of its run if it runs, as `waos launch` asks; answers once it has
    /// started. Its end is waited for on a task of the runtime this runs on.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn launch(
        &self,
        id: &ProgramId,
        arguments: Vec<String>,
        variables: Vec<(String, String)>,
    ) -> Result<()> {
        let environment = self.environments.of(id)?;
        let loaded = self.packages.load(id).await?;
        let (environment, output) = self.with_output(id, environment);
        let environment = ProgramEnvironment {
            variables,
            arguments,
            ..environment
        };
        let Started::New(run) = self
            .runs
            .start(id, IfRunning::Replace, || {
                self.engine.run(&loaded.program, environment)
            })
            .await?
        else {
            unreachable!("a start that replaces starts a run")
        };
        tracing::info!(%id, "launched");
        self.take_out_once_ended(id, run, output);
        Ok(())
    }

    /// Starts the installed program `id` held for an inspector, in place of its run if it runs,
    /// its views where programs usually go; answers it held.
    pub(crate) async fn start_held(&self, id: &ProgramId) -> Result<HeldProgram> {
        let environment = self.environments.of(id)?;
        let loaded = self.packages.load(id).await?;
        let (environment, output) = self.with_output(id, environment);
        self.places.set(id, self.places.usual());
        let mut held = None;
        let Started::New(run) = self
            .runs
            .start(id, IfRunning::Replace, || {
                let (running, hold) = self.engine.run_held(&loaded.program, environment)?;
                held = Some(hold);
                Ok(running)
            })
            .await?
        else {
            unreachable!("a start that replaces starts a run")
        };
        tracing::info!(%id, "started held for an inspector");
        self.take_out_once_ended(id, run, output);
        held.context("a run that started is held")
    }
}
