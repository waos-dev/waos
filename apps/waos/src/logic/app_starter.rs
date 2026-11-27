//! Starting an installed app, by one path whoever asks: the user opening it from an icon, WaOS
//! opening it to hand it files or a click on its notification, `waos launch` with its arguments
//! and variables, or an inspector, which starts it held. How a start was asked is a value
//! ([`StartRequest`]), and the starts under way are told with their stage
//! ([`AppStarter::starting`]), as Android's `ActivityStarter` sits beside its list of processes.
//! Each start loads the app's program through the package manager (`package_manager`), then
//! starts it among the running apps (`running_apps`) with this computer's capabilities, the grants
//! of its own settings and its own folders, its output to WaOS's log and to whoever attaches
//! (`launch_outputs`), its views going where its place says, set before it starts
//! (`program_places`). It is named by its id, which names it among the running apps, so a quit
//! asks it while it runs, in WaOS's log, where its output goes under the `program` target, and
//! to the questions it puts to its user. A tool, an app its manifest declares has no UI, opened by
//! the user from an icon is refused: no opening here has a terminal for its input and output, and
//! WaOS shows none of its own yet. One started to be handed files or a click on its notification
//! runs as any app does, which it may answer with no window.
//!
//! The invariant: a run started here is taken out of the running apps once it ends, by a task of
//! the runtime it was started on, its output told how it ended first, so the running apps never
//! hold an ended program and every attach hears its end; every run writes to an output of
//! `outputs`, so any app running is attached to; nothing of a run (its output, its place) is made
//! for an app left running; and the program loaded keeps its app's lock until its run is among
//! the running apps, so no removal or update lands between the two.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result, ensure};
use tokio::sync::watch;
use waos::{HeldProgram, ProgramEnvironment, ProgramOutput, RunningProgram, WaOS};
use waos_package::{ProgramId, ProgramKind};

use super::environments::ProgramEnvironments;
use super::launch_outputs::{LaunchOutput, LaunchOutputs};
use super::package_manager::PackageManager;
use super::program_places::{ProgramPlace, ProgramPlaces};
use super::running_apps::{IfRunning, Run, RunningApps, Started};

/// Why a tool the user opens does not open: there is nowhere for its input and output to go.
const NO_TERMINAL: &str = "it is a command-line program, with no window of its own, and WaOS cannot show its terminal yet";

/// Why an installed app is started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opening {
    /// The user opened it from an icon: the library's, the launcher's, the Dock's, a shortcut
    /// on a phone's home screen, or a card brought back from Recents.
    ByTheUser,
    /// To hand it what the user did outside it: files opened with it, or a click on one of its
    /// notifications.
    ToHandOver,
    /// A developer started it, with `waos launch` or from an inspector.
    ByTheDeveloper,
}

/// How a start of an installed app was asked for.
pub(crate) struct StartRequest {
    pub(crate) id: ProgramId,
    pub(crate) opening: Opening,
    pub(crate) if_running: IfRunning,
    /// Where its views go; none for where apps usually go.
    pub(crate) place: Option<ProgramPlace>,
    pub(crate) arguments: Vec<String>,
    /// The variables it alone adds to its environment.
    pub(crate) variables: Vec<(String, String)>,
    /// Whether it is held before its first instruction, for an inspector to listen from its start.
    pub(crate) held: bool,
}

impl StartRequest {
    /// The app `id` opened for `opening`, its views going where apps usually go; left as it is
    /// where it runs already.
    pub(crate) fn open(id: ProgramId, opening: Opening) -> StartRequest {
        StartRequest {
            id,
            opening,
            if_running: IfRunning::Leave,
            place: None,
            arguments: Vec::new(),
            variables: Vec::new(),
            held: false,
        }
    }
}

/// What a start came to.
pub(crate) enum StartAnswer {
    /// It started now; held, where the request asked, until `held` is dropped.
    Started {
        program: RunningProgram,
        held: Option<HeldProgram>,
    },
    /// It was running already, and is left as it is.
    RunningAlready(RunningProgram),
}

impl StartAnswer {
    pub(crate) fn program(&self) -> &RunningProgram {
        match self {
            StartAnswer::Started { program, .. } | StartAnswer::RunningAlready(program) => program,
        }
    }

    /// What holds the app before its first instruction, where it started held.
    pub(crate) fn held(self) -> Option<HeldProgram> {
        match self {
            StartAnswer::Started { held, .. } => held,
            StartAnswer::RunningAlready(_) => None,
        }
    }
}

/// Where a start under way is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartStage {
    /// Its program is loaded and started.
    Starting,
}

/// Starts installed apps, however asked, and tells the starts under way.
pub(crate) struct AppStarter {
    running: Arc<RunningApps>,
    packages: Arc<PackageManager>,
    engine: Arc<WaOS>,
    environments: Arc<ProgramEnvironments>,
    places: ProgramPlaces,
    outputs: Arc<LaunchOutputs>,
    starts: Arc<Starts>,
}

impl AppStarter {
    /// Starting the apps `packages` loads among `running`, on `engine`, each with what
    /// `environments` gives it, its views going where `places` says and its output to `outputs`.
    pub(crate) fn new(
        running: Arc<RunningApps>,
        packages: Arc<PackageManager>,
        engine: Arc<WaOS>,
        environments: Arc<ProgramEnvironments>,
        places: ProgramPlaces,
        outputs: Arc<LaunchOutputs>,
    ) -> AppStarter {
        AppStarter {
            running,
            packages,
            engine,
            environments,
            places,
            outputs,
            starts: Arc::default(),
        }
    }

    /// Starts the installed app `request` names as it asks; answers the program once it has
    /// started, or the one running where the request leaves it, or why it could not start. Its
    /// end is waited for on a task of the runtime this runs on.
    pub(crate) async fn start(&self, request: StartRequest) -> Result<StartAnswer> {
        let id = request.id.clone();
        let _start = Start::begin(&self.starts, &id);
        let opening = request.opening;
        let answer = self.start_now(request).await;
        // A developer's tool says what it asked for itself; the library says what the user did.
        let answer = match opening {
            Opening::ByTheDeveloper => answer,
            Opening::ByTheUser | Opening::ToHandOver => {
                answer.with_context(|| format!("{id} could not be opened"))
            }
        }?;
        match &answer {
            StartAnswer::Started { .. } => tracing::info!(%id, "started"),
            StartAnswer::RunningAlready(_) => {
                tracing::info!(%id, "asked to start while it runs: left as it is")
            }
        }
        Ok(answer)
    }

    /// The starts under way now, each with its stage, and each change of them from now on.
    pub(crate) fn starting(&self) -> watch::Receiver<BTreeMap<ProgramId, StartStage>> {
        self.starts.stages.subscribe()
    }

    /// [`start`](Self::start), told already.
    async fn start_now(&self, request: StartRequest) -> Result<StartAnswer> {
        let id = &request.id;
        if request.if_running == IfRunning::Leave
            && let Some(program) = self.running.running(id)
        {
            return Ok(StartAnswer::RunningAlready(program));
        }
        let loaded = self.packages.load(id).await?;
        let mut held = None;
        let mut output = None;
        let started = self
            .running
            .start(id, request.if_running, || {
                ensure!(
                    request.opening != Opening::ByTheUser
                        || self.environments.kind_of(id) == ProgramKind::App,
                    NO_TERMINAL
                );
                let (environment, its_output) = self.environment_of(&request)?;
                output = Some(its_output);
                self.places
                    .set(id, request.place.unwrap_or(self.places.usual()));
                if !request.held {
                    return self.engine.run(&loaded.program, environment);
                }
                let (running, hold) = self.engine.run_held(&loaded.program, environment)?;
                held = Some(hold);
                Ok(running)
            })
            .await?;
        drop(loaded);
        let run = match started {
            Started::New(run) => run,
            Started::Running(program) => return Ok(StartAnswer::RunningAlready(program)),
        };
        let program = run.program.clone();
        let output = output.context("a run that started writes to its output")?;
        self.take_out_once_ended(id, run, output);
        Ok(StartAnswer::Started { program, held })
    }

    /// What the app `request` names runs with, its arguments and variables among it, with its
    /// output to a new output of `outputs`, the log's and the attaches': made as the run starts,
    /// never for an app that runs already, whose output it would take the place of.
    fn environment_of(
        &self,
        request: &StartRequest,
    ) -> Result<(ProgramEnvironment, Arc<LaunchOutput>)> {
        let environment = self.environments.of(&request.id)?;
        let output = self.outputs.launched(&request.id);
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(&output) as Arc<dyn ProgramOutput>),
            arguments: request.arguments.clone(),
            variables: request.variables.clone(),
            ..environment
        };
        Ok((environment, output))
    }

    /// Takes `run`, of the app `id`, out of the running apps once it ends, on a task of the
    /// runtime this runs on, telling `output` how it ended first, and letting go of it once an
    /// attach heard.
    fn take_out_once_ended(&self, id: &ProgramId, run: Run, output: Arc<LaunchOutput>) {
        let (outputs, id) = (Arc::clone(&self.outputs), id.clone());
        self.running.take_out_once_ended(run, move |end| {
            if output.ended(end) {
                outputs.heard(&id, &output);
            }
        });
    }
}

/// The starts under way, by app, as their watchers hear them.
#[derive(Default)]
struct Starts {
    stages: watch::Sender<BTreeMap<ProgramId, StartStage>>,
    /// How many starts of each app are under way: two taps start one app twice, the second
    /// waiting for the first.
    counts: Mutex<HashMap<ProgramId, usize>>,
}

/// A start of one app, told to the watchers of the starts from its beginning until it is dropped.
struct Start {
    starts: Arc<Starts>,
    id: ProgramId,
}

impl Start {
    fn begin(starts: &Arc<Starts>, id: &ProgramId) -> Start {
        let mut counts = starts.counts.lock().unwrap_or_else(PoisonError::into_inner);
        let count = counts.entry(id.clone()).or_default();
        *count += 1;
        if *count == 1 {
            starts.stages.send_modify(|stages| {
                stages.insert(id.clone(), StartStage::Starting);
            });
        }
        Start {
            starts: Arc::clone(starts),
            id: id.clone(),
        }
    }
}

impl Drop for Start {
    fn drop(&mut self) {
        // Every change leaves the counts whole, so one that panicked leaves nothing half-changed.
        let mut counts = self
            .starts
            .counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(count) = counts.get_mut(&self.id) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            counts.remove(&self.id);
            self.starts.stages.send_modify(|stages| {
                stages.remove(&self.id);
            });
        }
    }
}
