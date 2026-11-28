//! What runs in WaOS: at most one instance of each installed app, by the app's id, as Windows and
//! Linux call an app single-instance, and every way of opening one, as macOS's `NSWorkspace` opens
//! an app and Chromium's App Service keeps its instance registry beside the registry of installed
//! apps. The user opening an app from an icon (the library's, the launcher's, the Dock's, a
//! shortcut on a phone's home screen, a card brought back from Recents), WaOS opening it to hand it
//! files or a click on one of its notifications, `waos launch` with its arguments and variables,
//! and an inspector, which opens it held at its first instruction: each is an
//! [`OpenConfiguration`], and each starts the app's instance here, or finds the one running and
//! leaves it as it is, for the caller to bring forward. The opens under way are told with their
//! stage ([`AppInstances::opening`]), and the apps running ([`AppInstances::running`]).
//!
//! An open loads the app's program under its lock from the package manager, then starts its
//! instance with this computer's capabilities, the grants of its own settings and its own folders,
//! its output to WaOS's log and to whoever attaches (`launch_outputs`), its views going where its
//! place says, set before it starts (`program_places`): an open from an icon or for an inspector
//! sets it, a launch leaves it as it was. An instance runs under its app's id, which names it in
//! WaOS's log, where its output goes under the `program` target, and in what it shows. A tool, an
//! app its manifest declares has no UI, opened by the user from an icon is refused: no opening here
//! has a terminal for its input and output, and WaOS shows none of its own yet. One opened to be
//! handed files or a click on its notification runs as any app does, which it may answer with no
//! window. A launch, or an open an inspector asks for, stops the instance running first, which
//! ends `Replaced`.
//!
//! The package manager stops an app's instance before it changes the app, as a system ends an app
//! it uninstalls or updates, so no instance outlives the package it came from or the folders it was
//! given. As Android's package side asks its process side to stop an app, it puts "stop this app,
//! and say when it has stopped" in a mailbox ([`stop_mailbox`]), made first in startup, whose
//! reading end the app instances hold and answer on a task of their own: neither has to exist
//! before the other. Every instance running is its user's to read and drive through the dev
//! socket, through its own mouse and the keyboard, as `driven` finds it, which a phone, where
//! `waos` reaches no WaOS yet, does not compile.
//!
//! The invariants: nothing starts an instance but [`AppInstances::open`]. An instance is taken out
//! of the list once it ends, by a task of the app instances' runtime, its output told how it ended
//! first, so the list never holds an ended instance and every attach hears its end; every instance
//! writes to an output of its own, so any app running is attached to; nothing of an instance (its
//! output, its place) is made for an app left running; one open or stop of an app at a time
//! (`list`); and the program loaded keeps its app's lock until its instance is in the list, so no
//! removal or update lands between the two, and nothing compiles in an app's turn.

#[cfg(desktop)]
mod driven;
mod list;
mod opens;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::{FutureExt as _, StreamExt as _};
use tokio::sync::watch;
use tracing::Instrument;
use waos::{HeldProgram, Program, ProgramEnvironment, ProgramOutput, Role, RunningProgram, WaOS};
use waos_capabilities::InspectedApps;
use waos_package::{ProgramId, ProgramKind};
use waos_services::app_storage::AppStorage;
use waos_services::packages::{PackageChange, PackageManager, StopApp};
use waos_services::permissions::Permissions;

#[cfg(desktop)]
pub(crate) use driven::DrivenProgram;
pub(crate) use opens::OpenStage;

use super::environments::ProgramEnvironments;
use super::held_starts::HeldStarts;
use super::inspected_programs::ProgramsOnThisComputer;
use super::launch_outputs::{LaunchOutput, LaunchOutputs};
use super::program_places::{ProgramPlace, ProgramPlaces};
use super::startup::Domains;
use list::{InstanceList, Run};
use opens::{Open, Opens};

/// Why a tool the user opens does not open: there is nowhere for its input and output to go.
const NO_TERMINAL: &str = "it is a command-line program, with no window of its own, and WaOS cannot show its terminal yet";

/// What an app is opened for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenedFor {
    /// The user opened it from an icon: the library's, the launcher's, the Dock's, a shortcut on
    /// a phone's home screen, or a card brought back from Recents.
    User,
    /// To hand it what the user did outside it: files opened with it, or a click on one of its
    /// notifications.
    HandOver,
    /// A developer opened it, with `waos launch` or from an inspector.
    Development,
}

/// Where an open puts the app's views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenPlace {
    /// Where they went before, or where apps usually go for an app not placed yet: a launch's.
    AsItWas,
    /// Where apps usually go.
    Usual,
    /// Where the configuration names.
    Named(ProgramPlace),
}

/// How an app is opened, as macOS's `NSWorkspace.OpenConfiguration` says it.
pub(crate) struct OpenConfiguration {
    pub(crate) opened_for: OpenedFor,
    /// Whether the instance running is stopped, ending `Replaced`, and the app started again, as
    /// `waos launch` does; otherwise the one running is left as it is.
    pub(crate) replaces_running: bool,
    pub(crate) place: OpenPlace,
    pub(crate) arguments: Vec<String>,
    /// The variables it alone adds to its environment.
    pub(crate) variables: Vec<(String, String)>,
    /// Whether it is held before its first instruction, for an inspector to listen from its start.
    pub(crate) held: bool,
}

impl OpenConfiguration {
    /// An open for `opened_for`, the app's views going where apps usually go; the instance running
    /// left as it is.
    pub(crate) fn new(opened_for: OpenedFor) -> OpenConfiguration {
        OpenConfiguration {
            opened_for,
            replaces_running: false,
            place: OpenPlace::Usual,
            arguments: Vec::new(),
            variables: Vec::new(),
            held: false,
        }
    }
}

/// What an open came to.
pub(crate) enum Opened {
    /// Its instance started now; held, where the configuration asked, until `held` is dropped.
    Started {
        instance: RunningProgram,
        held: Option<HeldProgram>,
    },
    /// Its instance was running already, and is left as it is.
    RunningAlready(RunningProgram),
}

impl Opened {
    pub(crate) fn instance(&self) -> &RunningProgram {
        match self {
            Opened::Started { instance, .. } | Opened::RunningAlready(instance) => instance,
        }
    }

    /// What holds the instance before its first instruction, where it started held.
    pub(crate) fn held(self) -> Option<HeldProgram> {
        match self {
            Opened::Started { held, .. } => held,
            Opened::RunningAlready(_) => None,
        }
    }
}

/// Why an app did not open.
#[derive(Debug)]
pub(crate) enum OpenError {
    /// It is a tool, which the user opened from an icon: there is nowhere for its input and output
    /// to go.
    Tool(ProgramId),
    /// Its program did not load, or its instance did not start, for this.
    Failed(anyhow::Error),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Tool(id) => write!(f, "{id} could not be opened: {NO_TERMINAL}"),
            OpenError::Failed(error) => write!(f, "{error:#}"),
        }
    }
}

impl std::error::Error for OpenError {}

/// The package manager's end of the mailbox its stops go through: each stop is a request in it,
/// answered once the app's instance has stopped; one answered at once once nobody reads the
/// mailbox, which leaves nothing running.
pub(crate) struct StopMailbox {
    requests: UnboundedSender<StopRequest>,
}

/// The app instances' end of the mailbox the package manager's stops go through.
pub(crate) struct StopRequests(UnboundedReceiver<StopRequest>);

/// "Stop this app, for this change, and say when it has stopped."
struct StopRequest {
    app: ProgramId,
    change: PackageChange,
    stopped: oneshot::Sender<()>,
}

/// The mailbox the package manager's stops go through: its end, and the app instances'.
pub(crate) fn stop_mailbox() -> (StopMailbox, StopRequests) {
    let (requests, received) = mpsc::unbounded();
    (StopMailbox { requests }, StopRequests(received))
}

/// The package manager stops an app's instance before it changes the app: one an install replaces
/// ends `Replaced`, one removed `Stopped`.
impl StopApp for StopMailbox {
    fn stop<'a>(&'a self, id: &'a ProgramId, change: PackageChange) -> BoxFuture<'a, ()> {
        let (stopped, answered) = oneshot::channel();
        let request = StopRequest {
            app: id.clone(),
            change,
            stopped,
        };
        let sent = self.requests.unbounded_send(request).is_ok();
        async move {
            if sent {
                // Nobody left to answer stopped nothing, as nothing runs.
                let _ = answered.await;
            }
        }
        .boxed()
    }
}

/// The instances running, at most one per installed app, and every way of opening one.
pub(crate) struct AppInstances {
    list: Arc<InstanceList>,
    opens: Arc<Opens>,
    packages: Arc<PackageManager>,
    /// What the user lets each app use, which an inspector reads of an app that does not run.
    permissions: Permissions,
    /// Each app's own folders, which an inspector reads of an app that does not run.
    storage: AppStorage,
    engine: Arc<WaOS>,
    environments: Arc<ProgramEnvironments>,
    places: ProgramPlaces,
    outputs: Arc<LaunchOutputs>,
    /// Where the instances' ends are waited for, the package manager's stops answered, and an
    /// inspector's opens and ends carried out.
    runtime: tokio::runtime::Handle,
}

impl AppInstances {
    /// The instances of the apps `domains`' package manager loads, on `engine`, each with what
    /// `environments` gives it, its views going where `places` says, and an inspector with the
    /// apps it sees, what `domains`' permissions let each and its own folders among them; the
    /// package manager's stops read from `stops`, and every task of theirs run on `runtime`.
    pub(crate) fn new(
        domains: &Domains,
        engine: Arc<WaOS>,
        environments: Arc<ProgramEnvironments>,
        places: ProgramPlaces,
        stops: StopRequests,
        runtime: tokio::runtime::Handle,
    ) -> Arc<AppInstances> {
        let list = Arc::new(InstanceList::default());
        answer_stops(&runtime, Arc::clone(&list), stops);
        Arc::new(AppInstances {
            list,
            opens: Arc::default(),
            packages: Arc::clone(&domains.packages),
            permissions: domains.permissions.clone(),
            storage: domains.storage.clone(),
            engine,
            environments,
            places,
            outputs: Arc::default(),
            runtime,
        })
    }

    /// Opens the installed app `id` as `configuration` asks; answers its instance once it has
    /// started, or the one running where the configuration leaves it, or why it did not open. Its
    /// end is waited for on a task of the app instances' runtime.
    pub(crate) async fn open(
        self: &Arc<Self>,
        id: &ProgramId,
        configuration: OpenConfiguration,
    ) -> Result<Opened, OpenError> {
        let open = Open::begin(&self.opens, id);
        let opened_for = configuration.opened_for;
        let opened = self.open_now(id, configuration, &open).await;
        // A developer's tool says what it asked for itself; the library says what the user did.
        let opened = opened.map_err(|error| match (error, opened_for) {
            (OpenError::Failed(error), OpenedFor::User | OpenedFor::HandOver) => {
                OpenError::Failed(error.context(format!("{id} could not be opened")))
            }
            (error, _) => error,
        })?;
        match &opened {
            Opened::Started { .. } => tracing::info!(%id, "started"),
            Opened::RunningAlready(_) => {
                tracing::info!(%id, "asked to open while it runs: left as it is");
            }
        }
        Ok(opened)
    }

    /// [`open`](Self::open), told as `open`.
    async fn open_now(
        self: &Arc<Self>,
        id: &ProgramId,
        configuration: OpenConfiguration,
        open: &Open,
    ) -> Result<Opened, OpenError> {
        if !configuration.replaces_running
            && let Some(instance) = self.list.instance(id)
        {
            return Ok(Opened::RunningAlready(instance));
        }
        let optimizing = open.set_later(OpenStage::Optimizing);
        let lock = self.packages.lock(id).await;
        let (engine, name) = (Arc::clone(&self.engine), id.clone());
        let loaded = lock
            .open(optimizing, move |version| {
                let program = engine.load(&version.component(), &version.code())?;
                Ok(program.named(name.as_str()))
            })
            .await;
        open.set(OpenStage::Starting);
        let program = loaded.map_err(OpenError::Failed)?;
        let turn = self.list.turn_of(id).await;
        if configuration.replaces_running {
            if self.list.end(id, true).await {
                tracing::info!(%id, "a later instance of the app: the one before stopped");
            }
        } else if let Some(instance) = self.list.instance(id) {
            return Ok(Opened::RunningAlready(instance));
        }
        let (instance, held, output) = self.start(id, &configuration, &program)?;
        let run = self.list.list(id, instance.clone());
        drop(turn);
        drop(lock);
        self.take_out_once_ended(run, output);
        Ok(Opened::Started { instance, held })
    }

    /// Starts `program`, the installed app `id`'s, as `configuration` asks: its views going where
    /// it says, held where it asks, writing to an output of its own. A tool the user opened from
    /// an icon is refused.
    fn start(
        self: &Arc<Self>,
        id: &ProgramId,
        configuration: &OpenConfiguration,
        program: &Program,
    ) -> Result<(RunningProgram, Option<HeldProgram>, Arc<LaunchOutput>), OpenError> {
        if configuration.opened_for == OpenedFor::User && self.kind_of(id) != ProgramKind::App {
            return Err(OpenError::Tool(id.clone()));
        }
        let (environment, output) = self
            .environment_of(id, configuration, program)
            .map_err(OpenError::Failed)?;
        match configuration.place {
            OpenPlace::AsItWas => {}
            OpenPlace::Usual => self.places.set(id, self.places.usual()),
            OpenPlace::Named(place) => self.places.set(id, place),
        }
        let (instance, held) = if configuration.held {
            let (instance, held) =
                (self.engine.run_held(program, environment)).map_err(OpenError::Failed)?;
            (instance, Some(held))
        } else {
            let instance = (self.engine.run(program, environment)).map_err(OpenError::Failed)?;
            (instance, None)
        };
        Ok((instance, held, output))
    }

    /// Stops the instance of the app `id`, if any, once an open under way is done, and waits until
    /// it has ended: it is out of the list first, so nothing finds it running meanwhile. Answers
    /// whether one ran.
    pub(crate) async fn stop(&self, id: &ProgramId) -> bool {
        self.list.stop(id, false).await
    }

    /// The ids of the apps running now, and each change of them from now on.
    pub(crate) fn running(&self) -> watch::Receiver<BTreeSet<ProgramId>> {
        self.list.ids()
    }

    /// The opens under way now, each with its stage, and each change of them from now on.
    pub(crate) fn opening(&self) -> watch::Receiver<BTreeMap<ProgramId, OpenStage>> {
        self.opens.stages()
    }

    /// The instance of the app `id` running now, if one is.
    pub(crate) fn instance(&self, id: &ProgramId) -> Option<RunningProgram> {
        self.list.instance(id)
    }

    /// What each instance writes, which an attach reads; a phone has no dev server yet, but a
    /// test attaches there too.
    #[cfg(any(desktop, test))]
    pub(crate) fn outputs(&self) -> &Arc<LaunchOutputs> {
        &self.outputs
    }

    /// The instances running now, in no order: what a quit asks, which a phone's system never
    /// does.
    #[cfg(desktop)]
    pub(crate) fn instances(&self) -> Vec<RunningProgram> {
        self.list.instances()
    }

    /// Stops every instance and waits until each has ended: once a quit is agreed to.
    #[cfg(desktop)]
    pub(crate) async fn stop_all(&self) {
        let ends: Vec<_> = (self.list.instances().into_iter())
            .map(|instance| {
                instance.stop();
                instance.ended()
            })
            .collect();
        futures::future::join_all(ends).await;
    }

    /// The ids running now, as text, in no order: the tests' window.
    #[cfg(test)]
    pub(crate) async fn names(&self) -> Vec<String> {
        self.list
            .ids()
            .borrow()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// What the installed app `id` declares it is, an app or a tool; an app where its manifest
    /// cannot be read.
    fn kind_of(&self, id: &ProgramId) -> ProgramKind {
        self.packages
            .installed_program(id)
            .map_or(ProgramKind::App, |installed| installed.manifest.kind())
    }

    /// What the app `id` runs with, the arguments and variables `configuration` gives among it,
    /// with its output to a new output of `outputs`, the log's and the attaches', and the apps on
    /// this computer where its `program` asks for the inspector role: made as its instance starts,
    /// never for an app that runs already, whose output it would take the place of.
    fn environment_of(
        self: &Arc<Self>,
        id: &ProgramId,
        configuration: &OpenConfiguration,
        program: &Program,
    ) -> anyhow::Result<(ProgramEnvironment, Arc<LaunchOutput>)> {
        let environment = self.environments.of(id)?;
        let output = self.outputs.launched(id);
        let inspected = program
            .roles()
            .contains(&Role::Inspector)
            .then(|| self.inspected_apps());
        let environment = ProgramEnvironment {
            output: Some(Arc::clone(&output) as Arc<dyn ProgramOutput>),
            arguments: configuration.arguments.clone(),
            variables: configuration.variables.clone(),
            inspected,
            ..environment
        };
        Ok((environment, output))
    }

    /// The apps on this computer as an inspector sees them, opened held here and ended on the
    /// app instances' runtime.
    fn inspected_apps(self: &Arc<Self>) -> Arc<dyn InspectedApps> {
        let starts = HeldStarts {
            instances: Arc::clone(self),
            packages: Arc::clone(&self.packages),
        };
        Arc::new(ProgramsOnThisComputer::new(
            self.storage.clone(),
            self.permissions.clone(),
            starts,
            self.runtime.clone(),
        ))
    }

    /// Takes `run` out of the list once its instance ends, on a task of the app instances'
    /// runtime, telling WaOS's log how long it ran and `output` how it ended first, and letting
    /// go of the output once an attach heard.
    fn take_out_once_ended(&self, run: Run, output: Arc<LaunchOutput>) {
        let (list, outputs) = (Arc::clone(&self.list), Arc::clone(&self.outputs));
        let started_at = Instant::now();
        let span = tracing::info_span!("run", name = %run.app);
        self.runtime.spawn(
            async move {
                let end = run.instance.ended().await;
                tracing::info!(
                    ?end,
                    seconds = started_at.elapsed().as_secs_f64(),
                    frames_presented = run.instance.stats().frames_presented(),
                    "ended"
                );
                if output.ended(run.end(&end)) {
                    outputs.heard(&run.app, &output);
                }
                list.ended(&run);
            }
            .instrument(span),
        );
    }
}

/// Answers each stop the package manager puts in the mailbox, read from `stops` on `runtime`,
/// once the app's instance in `list` has stopped; each on a task of its own, so an instance slow
/// to end holds back no other app's change.
fn answer_stops(runtime: &tokio::runtime::Handle, list: Arc<InstanceList>, stops: StopRequests) {
    let StopRequests(mut requests) = stops;
    let each = runtime.clone();
    runtime.spawn(async move {
        while let Some(StopRequest {
            app,
            change,
            stopped,
        }) = requests.next().await
        {
            let list = Arc::clone(&list);
            let replaced = matches!(change, PackageChange::Install);
            each.spawn(async move {
                list.stop(&app, replaced).await;
                // A package manager that stopped waiting has nobody to tell.
                let _ = stopped.send(());
            });
        }
    });
}

#[cfg(test)]
mod tests;
