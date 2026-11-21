//! One program as it runs: its main instance on a thread of its own, with a single-threaded
//! tokio runtime and one store, and each of its workers likewise (`worker`), from the moment `run`
//! starts it to the moment it ends. [`RunningProgram`] is the handle the shell and the engine hold
//! on it; [`ProgramEnd`] is how it ended.
//!
//! The invariant: each store lives on its instance's thread alone, and nothing of the program is
//! touched from another thread but through its state (its views, its lifecycle, its stats, its
//! workers, what the system says of its notifications) and the one stop. Dropping every handle
//! stops nothing: the main thread owns the program, and ends it when its `wasi:cli/run` returns,
//! when it traps, or when `stop` is called, and every worker with it before the end is told. All
//! of a program's instances run under its one span, read its one clock and are frozen by its one
//! lifecycle. Every program joins [`Programs`] before its code runs, for what the engine does to
//! all of them at once.

mod activity;
mod held;
mod incompatible;
mod instances;
mod limits;
mod programs;
mod store;
mod worker;

use std::future::{self, Future};
use std::path::PathBuf;
use std::pin::pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use anyhow::{Context as _, Result};
use futures::FutureExt;
use futures::channel::oneshot::{self, Canceled};
use futures::future::{Map, Shared};
use waos_capabilities::ProgramEnvironment;
use wasmtime::Engine;
use wasmtime::component::Component;

use crate::HardwareBufferMode;
use crate::access::{Days, ProgramAccess, TraceKind, TraceStream, WeakAccess};
use crate::bindings::wasdk::platform::lifecycle::AppExitResponse;
use crate::bindings::wasdk::platform::menus::MenuEntry;
use crate::bindings::wasdk::platform::notifications::Activation;
use crate::bindings::wasdk::platform::views::{RequestId, ViewError, ViewId};
use crate::caption_settings::CaptionSettings;
use crate::clipboard::{PlatformClipboard, ProgramSources};
use crate::clock::Clock;
use crate::engine_time::EngineClock;
use crate::files::HandedFiles;
use crate::gpu::{GpuInstance, InFlight, OpenedDevices};
use crate::hardware_buffers::UnaskedRead;
use crate::inspection::ProgramStorage;
use crate::lifecycle::{FreezePolicy, Lifecycle, OpenRequest};
use crate::menus::{MenuPlaces, ProgramMenus};
use crate::network::HttpProxy;
use crate::notifications::ProgramNotifications;
use crate::permissions::{NothingGranted, SystemRefusal};
use crate::screens::Screens;
use crate::screenshots::Screenshots;
use crate::shell::{
    PlacedView, Prompts, ShellAudioOutputPicker, ShellCameras, ShellCompositor, ShellFileDialogs,
    ShellImageCodecs, ShellMachinePicker, ShellMediaAssets, ShellMediaPlayers, ShellNotifications,
    ShellOpener, ShellRecordingIndicator, ShellVideoFrameReaders, ViewPlacement,
};
use crate::source_lines::ComponentFile;
use crate::stats::Stats;
use crate::sync::locked;
use crate::system::SystemSettings;
use crate::targets;
use crate::temporary_folders::TemporaryFolder;
use crate::views::{ProgramView, ProgramViews, ScreenRefreshes, ViewLayers, Watching};
use crate::workers::ProgramWorkers;
use activity::Activity;
pub use held::HeldProgram;
pub(crate) use held::{Hold, hold};
pub use incompatible::IncompatibleProgram;
pub(crate) use instances::ProgramInstances;
pub(crate) use programs::Programs;
pub(crate) use worker::start_worker;

/// How a program ended.
#[derive(Clone, Debug)]
pub enum ProgramEnd {
    /// Its `main` returned: `Err` for a run that reported failure, by its result or by exiting
    /// with a code other than 0.
    Returned(Result<(), ()>),
    /// It trapped, or could not run: with an [`IncompatibleProgram`] in the error's chain when it
    /// was built for another version of the interface, and a
    /// [`RoleNotGranted`](crate::RoleNotGranted) when it asks for a role its user did not grant.
    Failed(Arc<anyhow::Error>),
    /// `stop` ended it.
    Stopped,
}

/// One program as it runs, shared by the shell and the engine. Every clone is the same program,
/// and equal to it; two runs of one component are two programs.
#[derive(Clone)]
pub struct RunningProgram(Arc<Running>);

impl PartialEq for RunningProgram {
    fn eq(&self, other: &RunningProgram) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for RunningProgram {}

struct Running {
    name: Arc<str>,
    views: ProgramViews,
    lifecycle: Lifecycle,
    /// Its menu bar, as it set it last.
    menus: ProgramMenus,
    /// What the user does to its notifications.
    notifications: ProgramNotifications,
    /// Whether the system refuses it its notifications, as the shell last said.
    notifications_refused: SystemRefusal,
    /// The engine's time, which its views' hardware buffers read.
    clock: EngineClock,
    stats: Arc<Stats>,
    /// Whether any of its instances is in its code now.
    activity: Arc<Activity>,
    /// The workers it runs now.
    workers: ProgramWorkers,
    /// What its every instance runs under.
    span: tracing::Span,
    /// The devices it opened, which the store tells of.
    devices: Arc<OpenedDevices>,
    /// What it put on the clipboard, asked for its promised forms before the engine ends it.
    sources: ProgramSources,
    /// What of its GPU work is in flight.
    in_flight: Arc<InFlight>,
    /// The resolved hardware buffer backing mode for the engine this program runs on.
    hardware_buffers: HardwareBufferMode,
    /// Whether it has been warned of reading a hardware buffer it did not ask to read.
    unasked_read: UnaskedRead,
    /// Taken by the first `stop`.
    stop: Mutex<Option<oneshot::Sender<()>>>,
    ended: Ended,
    /// Its access, once its code is linked, held without keeping it: the access holds the
    /// program in turn.
    access: OnceLock<WeakAccess>,
    /// The files the user hands it while it runs, mounted for every instance and for an
    /// inspector.
    handed: HandedFiles,
    /// What it sees of this computer's files, as its settings, grants and handed files make them
    /// now, for an inspector.
    storage: ProgramStorage,
}

/// The program's end, as every `ended` resolves with it.
type Ended =
    Shared<Map<oneshot::Receiver<ProgramEnd>, fn(Result<ProgramEnd, Canceled>) -> ProgramEnd>>;

/// What a program is started with, besides its views.
pub(crate) struct ProgramStart {
    pub(crate) engine: Engine,
    pub(crate) gpu: GpuInstance,
    /// The resolved hardware buffer backing mode.
    pub(crate) hardware_buffers: HardwareBufferMode,
    /// What draws the screenshots of the program's views.
    pub(crate) screenshots: Arc<Screenshots>,
    pub(crate) component: Component,
    pub(crate) name: String,
    /// The file its component was loaded from, which a failure is located in.
    pub(crate) file: ComponentFile,
    pub(crate) placement: Arc<dyn ViewPlacement>,
    /// The system's compositor, which makes the program's composition layers, where the shell
    /// has one.
    pub(crate) compositor: Option<Arc<dyn ShellCompositor>>,
    /// What asks the program's user for what it may use.
    pub(crate) prompts: Arc<dyn Prompts>,
    pub(crate) screens: Screens,
    /// The screens' refreshes the engine waits for, which the program's frames wait on.
    pub(crate) refreshes: ScreenRefreshes,
    pub(crate) settings: SystemSettings,
    /// The user's caption settings, where the system keeps them.
    pub(crate) caption_settings: Option<CaptionSettings>,
    /// The platform's clipboard.
    pub(crate) clipboard: PlatformClipboard,
    /// The shell's places for menus.
    pub(crate) menus: MenuPlaces,
    /// The shell's picker of a sound's output, where it has one.
    pub(crate) audio_output_picker: Option<Arc<dyn ShellAudioOutputPicker>>,
    /// What shows who records, and with what, where the shell has it.
    pub(crate) recording_indicator: Option<Arc<dyn ShellRecordingIndicator>>,
    /// The shell's file dialogs, where it has them.
    pub(crate) file_dialogs: Option<Arc<dyn ShellFileDialogs>>,
    /// The system's opener, where the shell has one.
    pub(crate) opener: Option<Arc<dyn ShellOpener>>,
    /// The system's notifications, where the shell has them.
    pub(crate) notifications: Option<Arc<dyn ShellNotifications>>,
    /// The system's players, where the shell has them.
    pub(crate) media_players: Option<Arc<dyn ShellMediaPlayers>>,
    /// The system's reading of media without playing it, where the shell has one.
    pub(crate) media_assets: Option<Arc<dyn ShellMediaAssets>>,
    /// The system's image codecs, where the shell has them.
    pub(crate) image_codecs: Option<Arc<dyn ShellImageCodecs>>,
    /// The system's frame readers, where the shell has them.
    pub(crate) video_frame_readers: Option<Arc<dyn ShellVideoFrameReaders>>,
    /// The system's cameras, where the shell has them.
    pub(crate) cameras: Option<Arc<dyn ShellCameras>>,
    /// The cameras of WaOS's own, where the shell has them.
    pub(crate) simulated_cameras: Option<Arc<dyn ShellCameras>>,
    /// The picker of a machine, where the shell has one.
    pub(crate) machine_picker: Option<Arc<dyn ShellMachinePicker>>,
    pub(crate) environment: ProgramEnvironment,
    /// Where its `/tmp` is on the machine, where it has one.
    pub(crate) temporary_folder: Option<PathBuf>,
    /// The days of what programs did, which its summary counts into, where the engine keeps
    /// them.
    pub(crate) days: Option<Days>,
    /// What it sees of the programs while it holds the inspector role, where the embedder gives
    /// it.
    pub(crate) inspected_programs: Option<Arc<dyn crate::inspection::InspectedPrograms>>,
    pub(crate) freeze: FreezePolicy,
    /// The engine's time, which its freeze and its hardware buffers read.
    pub(crate) clock: EngineClock,
    /// The proxy its HTTP goes through.
    pub(crate) http_proxy: HttpProxy,
    /// The programs it joins.
    pub(crate) programs: Programs,
    /// The layers its views will hold, which its store keeps and its views let go of as they go.
    pub(crate) view_layers: Arc<ViewLayers>,
    /// What holds it before its first instruction, for a program started held.
    pub(crate) hold: Option<Hold>,
}

impl RunningProgram {
    /// Starts the program on a thread of its own; `temporary`, its `/tmp`, goes once it and its
    /// workers have ended, before its end is told.
    pub(crate) fn start(
        start: ProgramStart,
        temporary: Option<TemporaryFolder>,
    ) -> Result<RunningProgram> {
        let clock = Clock::start();
        let lifecycle = Lifecycle::new(start.freeze, start.clock.clone());
        let program_views = ProgramViews::new(
            clock,
            lifecycle.clone(),
            start.refreshes.clone(),
            start.clipboard.clone(),
            Arc::clone(&start.screenshots),
            Arc::downgrade(&start.view_layers),
        );
        let (stop, stopped) = oneshot::channel();
        let (end, ended) = oneshot::channel();
        let stats = Arc::new(Stats::default());
        let handed = HandedFiles::default();
        let storage = ProgramStorage::with_handed(
            start.environment.folders.clone(),
            start.temporary_folder.clone(),
            (start.environment.grants.clone()).unwrap_or_else(|| Arc::new(NothingGranted)),
            handed.clone(),
        );
        let program = RunningProgram(Arc::new(Running {
            name: Arc::from(start.name.as_str()),
            views: program_views,
            menus: ProgramMenus::new(lifecycle.clone()),
            notifications: ProgramNotifications::new(lifecycle.clone()),
            notifications_refused: SystemRefusal::default(),
            lifecycle,
            clock: start.clock.clone(),
            workers: ProgramWorkers::new(Arc::clone(&stats)),
            stats,
            activity: Arc::new(Activity::new()),
            span: tracing::info_span!(target: targets::PROGRAM, "program", name = %start.name),
            devices: OpenedDevices::new(Arc::clone(&start.gpu)),
            sources: ProgramSources::default(),
            in_flight: InFlight::new(),
            hardware_buffers: start.hardware_buffers,
            unasked_read: UnaskedRead::default(),
            stop: Mutex::new(Some(stop)),
            ended: ended.map(end_of_thread as fn(_) -> _).shared(),
            access: OnceLock::new(),
            handed,
            storage,
        }));
        start.programs.add(&program);
        let on_thread = program.clone();
        let thread_name = format!("program {}", start.name);
        let days = start.days.clone().map(|days| (days, start.name.clone()));
        thread::Builder::new()
            .name(thread_name.clone())
            .spawn(move || {
                let ending = run_on_this_thread(start, on_thread, clock, stopped);
                // Its day is kept at its end, by the engine's writer.
                if let Some((days, name)) = days {
                    days.keep_now(&name);
                }
                drop(temporary);
                // Nobody waiting for the end is not an error.
                let _ = end.send(ending);
            })
            .with_context(|| format!("a thread for {thread_name}"))?;
        Ok(program)
    }

    /// Adds the view that arrives for `request`: the program has it from now on, and hears of it.
    pub fn add_view(&self, request: RequestId, view: PlacedView) -> ProgramView {
        self.0.views.add(request, view)
    }

    /// The shell took the view away: its events end, and the program hears of it.
    pub fn remove_view(&self, view: ViewId) {
        self.0.views.remove(view);
    }

    /// The shell could not place the view asked for with `request`: the program hears why.
    pub fn view_request_failed(&self, request: RequestId, error: ViewError) {
        self.0.views.request_failed(request, error);
    }

    /// A tool watches the program, as `waos run` does while it is attached (the design's
    /// decision 63), until the answer is dropped: the program's views count as in sight though
    /// other windows cover them, so their frames are served and the program is neither hidden nor
    /// frozen for being covered, while a minimized or hidden window stays out of sight. Nothing is
    /// forced: a frame a view waits for is asked for, and the program hears its state change only
    /// as its views' standing changes it.
    pub fn watch(&self) -> Watching {
        self.0.views.watch()
    }

    /// Asks the program whether it may end: resolves once every stream it opened has answered,
    /// at once when none is open. Cancel if any cancelled, exit otherwise.
    pub fn request_exit(&self) -> impl Future<Output = AppExitResponse> + Send + 'static + use<> {
        self.0.lifecycle.request_exit()
    }

    /// Ends the program, from any thread, its store with it: first it is asked for every form its
    /// copy on the clipboard still promises, files aside, which is kept for later pastes, as a
    /// flush asks, and it ends where it stands once each is given or has timed out. Nothing after
    /// the first call.
    pub fn stop(&self) {
        if let Some(stop) = locked(&self.0.stop).take() {
            // A program that ended already has nobody to stop.
            let _ = stop.send(());
        }
    }

    /// Whether the program has ended.
    pub(crate) fn has_ended(&self) -> bool {
        self.0.ended.peek().is_some()
    }

    /// How the program ended, once it has. Any number of times; each resolves with the same end.
    /// Dropping it stops nothing.
    pub fn ended(&self) -> impl Future<Output = ProgramEnd> + Send + 'static + use<> {
        self.0.ended.clone()
    }

    /// The user opened `request`'s files and links with the program: every stream of its open
    /// requests hears it, or, while none is open, as before the program has started listening,
    /// the next one it opens; the program thawed until it has read it. Each file is mounted
    /// writable at its own path as the program reads the request, and kept among its mounts as a
    /// pick is.
    pub fn open_requested(&self, request: OpenRequest) {
        self.0.lifecycle.open_requested(request);
    }

    /// The user did `activation` to one of the program's notifications, at launch or while it
    /// runs: every stream of its activations hears it, or, while none is open, as before the
    /// program has started listening, the next one it opens; the program thawed until it has read
    /// it.
    pub fn notification_activated(&self, activation: Activation) {
        self.0.notifications.activated(activation);
    }

    /// The system says whether it `refused` the program its notifications, where it keeps a
    /// permission of its own for each program, as macOS does for each program's app
    /// (`ShellSystemPermission`): the shell says so whenever it learns it, as the app connects, as
    /// the system's question is answered, and as a post or a schedule is refused or taken. The
    /// program's permission reads denied while the system refuses, whatever its user answered in
    /// WaOS, the user's answer kept, and every stream of its changes hears it.
    pub fn system_notifications_refused(&self, refused: bool) {
        self.0.notifications_refused.set(refused);
    }

    /// The menu bar the program set last, whole; none while it has set none, and then the
    /// platform's standard one is its own. What the shell shows where it shows this program's.
    pub fn menu_bar(&self) -> Option<Arc<[MenuEntry]>> {
        self.0.menus.bar()
    }

    /// The user chose the item `id` in the program's menu bar: every stream of its choices hears
    /// it, the program thawed until it has handled it.
    pub fn menu_chosen(&self, id: u32) {
        self.0.menus.chosen(id);
    }

    /// What the program has cost so far, its workers' cost among it.
    pub fn stats(&self) -> &Stats {
        &self.0.stats
    }

    pub fn name(&self) -> &str {
        &self.0.name
    }

    /// What the program does with `kind` from now on, live, for as long as the stream is held:
    /// none before its code is linked and once it and every use it held have ended.
    pub fn trace(&self, kind: TraceKind) -> Option<TraceStream> {
        let access = self.0.access.get()?.upgrade()?;
        Some(access.subscribe(kind))
    }

    /// What the program uses now, as `wasdk:inspect` says it, and each change from then on,
    /// until it and every use it made have ended; none before its code is linked and once that
    /// has happened.
    pub fn active_uses(
        &self,
    ) -> Option<(
        Vec<crate::inspection::bindings::wasdk::inspect::active_uses::ActiveUse>,
        futures::stream::BoxStream<
            'static,
            crate::inspection::bindings::wasdk::inspect::active_uses::ActiveUseChange,
        >,
    )> {
        use futures::StreamExt;
        let access = self.0.access.get()?.upgrade()?;
        let (now, changes) = access.subscribe_active_uses();
        let now = now
            .into_iter()
            .map(crate::inspection::active_use_of)
            .collect();
        Some((
            now,
            changes.map(crate::inspection::active_use_change_of).boxed(),
        ))
    }

    /// The resolved hardware buffer backing mode for the engine this program runs on.
    pub(crate) fn hardware_buffers(&self) -> HardwareBufferMode {
        self.0.hardware_buffers
    }

    /// Its access, from its instances' link on.
    pub(crate) fn attach_access(&self, access: &ProgramAccess) {
        // Linked once; a second link would be the same program's.
        let _ = self.0.access.set(access.downgrade());
    }

    /// The files the user handed it, which every instance mounts.
    pub(crate) fn handed(&self) -> HandedFiles {
        self.0.handed.clone()
    }

    /// What it sees of this computer's files now, as its settings, grants and handed files make
    /// them, for an inspector.
    pub fn storage(&self) -> ProgramStorage {
        self.0.storage.clone()
    }

    /// Now on the program's clock, in nanoseconds: what the program's `wasi:clocks` read, and what
    /// its views' pointer and key events are stamped on, as `ProgramView::now` reads it.
    pub fn now(&self) -> u64 {
        self.0.views.now()
    }

    /// The views the program has now, in the order they arrived: what a tool numbers its windows
    /// by (the design's decision 60).
    pub fn views(&self) -> Vec<ProgramView> {
        self.0.views.views()
    }

    pub(crate) fn program_views(&self) -> &ProgramViews {
        &self.0.views
    }

    pub(crate) fn lifecycle(&self) -> &Lifecycle {
        &self.0.lifecycle
    }

    pub(crate) fn menus(&self) -> &ProgramMenus {
        &self.0.menus
    }

    pub(crate) fn notifications(&self) -> &ProgramNotifications {
        &self.0.notifications
    }

    pub(crate) fn notifications_refused(&self) -> &SystemRefusal {
        &self.0.notifications_refused
    }

    pub(crate) fn engine_clock(&self) -> &EngineClock {
        &self.0.clock
    }

    pub(crate) fn shared_stats(&self) -> Arc<Stats> {
        Arc::clone(&self.0.stats)
    }

    pub(crate) fn sources(&self) -> &ProgramSources {
        &self.0.sources
    }

    pub(crate) fn devices(&self) -> &Arc<OpenedDevices> {
        &self.0.devices
    }

    /// What warns it, once, of reading a hardware buffer it did not ask to read.
    pub(crate) fn unasked_read(&self) -> &UnaskedRead {
        &self.0.unasked_read
    }

    pub(crate) fn in_flight(&self) -> &Arc<InFlight> {
        &self.0.in_flight
    }

    /// Resolves once the program is frozen, at once while it is, whether or not it has stopped
    /// yet ([`stopped_at_freeze`](Self::stopped_at_freeze)). What a test waits on to see the rule
    /// freeze a program, as it does once the program has answered what thawed it.
    pub fn frozen(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.0.lifecycle.until_frozen()
    }

    /// Resolves once the program stands at its freeze: one of its instances, the main one or a
    /// worker, stopped by it at an epoch check, until the thaw. What a test waits on to see a
    /// frozen program has stopped, where [`idle`](Self::idle) also resolves at a moment a running
    /// program yields; it never resolves for a program that is not frozen, or one whose every
    /// instance was frozen while it waits in a host call, until something wakes one.
    pub fn stopped_at_freeze(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.0.lifecycle.stopped_at_freeze()
    }

    /// Resolves once the program is not running its code at this moment, each of its instances
    /// standing at its freeze, waiting in a host call or ended, and none of its GPU work is in
    /// flight. It says
    /// nothing of the next moment: what keeps a program from running again is its freeze. What
    /// the app's suspending awaits for each program, and what a test waits on to see a frozen
    /// program has stopped, rather than watching it for a while. Once a frozen program stands
    /// still nothing more is submitted, so what is on the GPU then is all there is: every device
    /// the program opened is waited for, and that wait is over with the frames it presented.
    pub fn idle(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        let idle = self.0.activity.idle();
        let devices = Arc::clone(&self.0.devices);
        let in_flight = Arc::clone(&self.0.in_flight);
        async move {
            idle.await;
            devices.wait_for_all(&in_flight);
            in_flight.none().await;
        }
    }
}

/// The program's thread ended without saying how: it panicked.
fn end_of_thread(end: Result<ProgramEnd, Canceled>) -> ProgramEnd {
    end.unwrap_or_else(|Canceled| {
        ProgramEnd::Failed(Arc::new(anyhow::anyhow!(
            "the program's thread ended without saying how"
        )))
    })
}

/// Runs the program to its end on this thread: on a single-threaded runtime of its own, so a
/// program that blocks stalls itself and nobody else, until it ends, or `stopped` fires and it has
/// given, or failed to give in time, the forms its copy on the clipboard promises, files aside;
/// then ends its workers, and answers once they have ended. The run reports into the program's
/// activity while it is polled.
fn run_on_this_thread(
    start: ProgramStart,
    program: RunningProgram,
    clock: Clock,
    stopped: oneshot::Receiver<()>,
) -> ProgramEnd {
    let _entered = program.0.span.enter();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return ProgramEnd::Failed(Arc::new(error.into())),
    };
    let workers = program.0.workers.clone();
    runtime.block_on(async {
        let end = run_until_stopped(start, program.clone(), clock, stopped).await;
        workers.end_all().await;
        end
    })
}

/// Runs the main instance until it ends, or `stopped` fires and it has given what it promised.
async fn run_until_stopped(
    start: ProgramStart,
    program: RunningProgram,
    clock: Clock,
    stopped: oneshot::Receiver<()>,
) -> ProgramEnd {
    let activity = Arc::clone(&program.0.activity);
    let sources = program.0.sources.clone();
    let mut run = pin!(activity.watch(store::run(start, program, clock)));
    tokio::select! {
        end = &mut run => return end,
        () = stop_asked(stopped) => {}
    }
    // The program runs on while it gives what it promised.
    tokio::select! {
        end = run => end,
        () = sources.keep_promised() => ProgramEnd::Stopped,
    }
}

/// Resolves when `stop` is called, and never when every handle is dropped instead.
async fn stop_asked(stopped: oneshot::Receiver<()>) {
    if stopped.await.is_err() {
        future::pending::<()>().await;
    }
}
