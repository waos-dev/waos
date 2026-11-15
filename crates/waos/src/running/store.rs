//! A program's store, one per instance of its code: what its imports read and write while it
//! runs, and the run itself, from linking and instantiation to the return of `wasi:cli/run` in
//! the main instance, and to the end of everything `worker-entry.run` started in a worker.
//!
//! The invariant: every interface is served from the store's data through the view trait of
//! the module that serves it, and the store's own business (its limits, its fuel, the epoch
//! tick that reads the fuel) is here and nowhere else.

use std::future;
use std::sync::Arc;

use waos_webgpu_host::{WasiWebGpuCtx, WasiWebGpuCtxView};
use wasmtime::component::{HasData, Linker, ResourceTable};
use wasmtime::{AsContext, Engine, Store, StoreLimitsBuilder, UpdateDeadline};
use wasmtime_wasi::{I32Exit, WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

use super::instances::{ProgramInstances, UserSide};
use super::limits::Limits;
use super::worker::WorkerRun;
use super::{ProgramEnd, ProgramStart, RunningProgram};
use crate::access::ProgramAccess;
use crate::audio::ProgramAudio;
use crate::audio::host::{AudioCtx, AudioCtxView};
use crate::bindings::wasdk::platform::workers::WorkerError;
use crate::cameras::host::{CamerasCtx, CamerasCtxView};
use crate::cameras::{CameraShells, ProgramCameras};
use crate::caption_settings::CaptionSettings;
use crate::caption_settings::host::{CaptionSettingsCtx, CaptionSettingsCtxView};
use crate::clipboard::ProgramClipboard;
use crate::clipboard::host::{ClipboardCtx, ClipboardCtxView};
use crate::clock::Clock;
use crate::file_dialogs::host::{FileDialogsCtx, FileDialogsCtxView};
use crate::file_watching::host::{FileWatchingCtx, FileWatchingCtxView};
use crate::files::{FilesCtx, FilesCtxView};
use crate::files::{Picks, ProgramFiles};
use crate::fonts::ProgramFonts;
use crate::fonts::host::{FontsCtx, FontsCtxView};
use crate::gpu::{self, GpuInstance, OpenedDevices};
use crate::gpu_interop::host::{GpuInteropCtx, GpuInteropCtxView};
use crate::hardware_buffers::Canvases;
use crate::hardware_buffers::host::{HardwareBuffersCtx, HardwareBuffersCtxView};
use crate::image_codecs::host::{ImageCodecsCtx, ImageCodecsCtxView};
use crate::lifecycle::host::{LifecycleCtx, LifecycleCtxView};
use crate::machines::ProgramMachines;
use crate::machines::host::{MachinesCtx, MachinesCtxView};
use crate::media_assets::host::{MediaAssetsCtx, MediaAssetsCtxView};
use crate::media_players::host::{MediaPlayersCtx, MediaPlayersCtxView};
use crate::menus::MenuPlaces;
use crate::menus::host::{MenusCtx, MenusCtxView};
use crate::network::host::{NetworkCtx, NetworkCtxView};
use crate::network::{ProgramHttp, ProgramNetwork};
use crate::notifications::host::{NotificationsCtx, NotificationsCtxView};
use crate::opener::LinkSchemes;
use crate::opener::host::{OpenerCtx, OpenerCtxView};
use crate::permissions::ProgramPermissions;
use crate::permissions::host::{PermissionsCtx, PermissionsCtxView};
use crate::screens::Screens;
use crate::screens::host::{ScreensCtx, ScreensCtxView};
use crate::shell::{
    ShellCompositor, ShellFileDialogs, ShellImageCodecs, ShellMediaAssets, ShellMediaPlayers,
    ShellNotifications, ShellOpener, ShellVideoFrameReaders, ViewPlacement,
};
use crate::stream::{Forwarded, stream_of};
use crate::system::SystemSettings;
use crate::system::host::{SystemCtx, SystemCtxView};
use crate::targets;
use crate::trust::host::{TrustCtx, TrustCtxView};
use crate::video_frames::host::{VideoFramesCtx, VideoFramesCtxView};
use crate::views::ViewLayers;
use crate::views::host::{ViewsCtx, ViewsCtxView};
use crate::wasi;
use crate::windows::host::{WindowsCtx, WindowsCtxView};
use crate::workers::WorkerEnd;
use crate::workers::host::{WorkersCtx, WorkersCtxView};

/// How many instances and how many memories one store may have. Its memory grows as far as
/// wasm lets it.
const INSTANCE_LIMIT: usize = 32;
const MEMORY_COUNT_LIMIT: usize = 32;

/// How often a program yields to its runtime, in fuel: where a stop takes it. A yield costs
/// far more than the fuel between two of them at a small interval: at 100,000 a program's own
/// compute ran six times slower than with none, and at this one within a tenth of it, a stop
/// still taken within a millisecond (`../experiments/2026-11-13-fuel-yield-interval`).
const YIELD_INTERVAL: u64 = 10_000_000;

/// Runs the program to its end: instantiates it in a store of its own and calls its
/// `wasi:cli/run`.
pub(super) async fn run(start: ProgramStart, program: RunningProgram, clock: Clock) -> ProgramEnd {
    match run_in_store(start, program, clock).await {
        Ok(Ok(returned)) => ProgramEnd::Returned(returned),
        Ok(Err(trap)) | Err(trap) => end_of_error(trap),
    }
}

async fn run_in_store(
    start: ProgramStart,
    program: RunningProgram,
    clock: Clock,
) -> wasmtime::Result<wasmtime::Result<Result<(), ()>>> {
    let linker = linker(&start.engine)?;
    let (instances, user_side) = ProgramInstances::link(start, &linker, program, clock)?;
    let instances = Arc::new(instances);
    let mut store = store(&instances, user_side, InstanceRole::Main);
    arm(&mut store)?;
    let instance = instances.linked.instantiate_async(&mut store).await?;
    let entry = instances.entry.load(&mut store, &instance)?;
    store
        .run_concurrent(async move |store| entry.call_run(store).await)
        .await
}

/// Runs a worker to its end: instantiates the program's code in a store of its own, without the
/// user's side, and calls its `worker-entry.run`, which says whether it takes the entry; then
/// goes on until everything the call started has finished. What `run` answered, the worker's
/// messages, crosses to the instance that started it as it is written.
pub(super) async fn run_worker(instances: &Arc<ProgramInstances>, run: WorkerRun) -> WorkerEnd {
    let entry = run.entry.clone();
    match run_worker_in_store(instances, run).await {
        Ok(()) => Ok(()),
        Err(error) => worker_end_of_error(&entry, error),
    }
}

async fn run_worker_in_store(
    instances: &Arc<ProgramInstances>,
    run: WorkerRun,
) -> wasmtime::Result<()> {
    let WorkerRun {
        entry,
        inbox,
        outbox,
        accept,
    } = run;
    let Some(worker_entry) = &instances.worker_entry else {
        let refused = WorkerError::Refused("the program's code has no workers".to_owned());
        let _ = accept.send(Err(refused));
        return Ok(());
    };
    let mut store = store(instances, UserSide::none(), InstanceRole::Worker);
    arm(&mut store)?;
    let instance = instances.linked.instantiate_async(&mut store).await?;
    let worker_entry = worker_entry.load(&mut store, &instance)?;
    store
        .run_concurrent(async move |accessor| -> wasmtime::Result<()> {
            let inbox = accessor.with(|access| {
                stream_of(access, inbox, |_: &mut StoreData, message| Ok(message))
            })?;
            match worker_entry.call_run(accessor, entry, inbox).await? {
                Err(refused) => {
                    let _ = accept.send(Err(refused));
                }
                Ok(messages) => {
                    accessor
                        .with(|mut access| messages.pipe(&mut access, Forwarded::new(outbox)))?;
                    let _ = accept.send(Ok(()));
                    future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
                }
            }
            Ok(())
        })
        .await?
}

/// A worker that exited with 0 ended well, one that exited with another code or trapped
/// failed, said in the log with its backtrace.
fn worker_end_of_error(entry: &str, error: wasmtime::Error) -> WorkerEnd {
    match error.downcast_ref::<I32Exit>() {
        Some(I32Exit(0)) => Ok(()),
        Some(I32Exit(code)) => Err(format!("the worker exited with {code}")),
        None => {
            tracing::warn!(target: targets::PROGRAM, "the worker at {entry} failed: {error:?}");
            Err(format!("{error:?}"))
        }
    }
}

/// A program that exited with a code returned, with 0 for success; anything else failed.
fn end_of_error(error: wasmtime::Error) -> ProgramEnd {
    match error.downcast_ref::<I32Exit>() {
        Some(I32Exit(0)) => ProgramEnd::Returned(Ok(())),
        Some(I32Exit(_)) => ProgramEnd::Returned(Err(())),
        None => ProgramEnd::Failed(Arc::new(error.into())),
    }
}

/// Every interface a program may import, each served by its module.
fn linker(engine: &Engine) -> wasmtime::Result<Linker<StoreData>> {
    let mut linker = Linker::<StoreData>::new(engine);
    wasi::add_to_linker(&mut linker)?;
    crate::files::p3::add_to_linker(&mut linker)?;
    crate::network::host::add_to_linker(&mut linker)?;
    wasmtime_wasi_http::p3::add_to_linker(&mut linker)?;
    waos_webgpu_host::add_to_linker(&mut linker)?;
    crate::views::host::add_to_linker(&mut linker)?;
    crate::windows::host::add_to_linker(&mut linker)?;
    crate::screens::host::add_to_linker(&mut linker)?;
    crate::system::host::add_to_linker(&mut linker)?;
    crate::caption_settings::host::add_to_linker(&mut linker)?;
    crate::lifecycle::host::add_to_linker(&mut linker)?;
    crate::fonts::host::add_to_linker(&mut linker)?;
    crate::permissions::host::add_to_linker(&mut linker)?;
    crate::trust::host::add_to_linker(&mut linker)?;
    crate::clipboard::host::add_to_linker(&mut linker)?;
    crate::menus::host::add_to_linker(&mut linker)?;
    crate::audio::host::add_to_linker(&mut linker)?;
    crate::file_dialogs::host::add_to_linker(&mut linker)?;
    crate::opener::host::add_to_linker(&mut linker)?;
    crate::notifications::host::add_to_linker(&mut linker)?;
    crate::machines::host::add_to_linker(&mut linker)?;
    crate::file_watching::host::add_to_linker(&mut linker)?;
    crate::hardware_buffers::host::add_to_linker(&mut linker)?;
    crate::gpu_interop::host::add_to_linker(&mut linker)?;
    crate::video_frames::host::add_to_linker(&mut linker)?;
    crate::media_players::host::add_to_linker(&mut linker)?;
    crate::media_assets::host::add_to_linker(&mut linker)?;
    crate::image_codecs::host::add_to_linker(&mut linker)?;
    crate::cameras::host::add_to_linker(&mut linker)?;
    crate::workers::host::add_to_linker(&mut linker)?;
    Ok(linker)
}

/// Which of its program's instances a store is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InstanceRole {
    /// The one `wasi:cli/run` runs in, which hears the platform's questions to the program.
    Main,
    /// One `worker-entry.run` runs in.
    Worker,
}

/// A store of `instances`' program, with its limits in force, its user's side `user_side`.
pub(super) fn store(
    instances: &Arc<ProgramInstances>,
    user_side: UserSide,
    role: InstanceRole,
) -> Store<StoreData> {
    let mut store = Store::new(&instances.engine, store_data(instances, user_side, role));
    store.limiter(|data| &mut data.limits);
    store
}

/// What one instance's imports read and write: the program's state, shared with its other
/// instances, the user's side given it, and its limits counted into the program's stats.
fn store_data(
    instances: &Arc<ProgramInstances>,
    user_side: UserSide,
    role: InstanceRole,
) -> StoreData {
    let program = &instances.program;
    let environment = &instances.environment;
    let limits = StoreLimitsBuilder::new()
        .instances(INSTANCE_LIMIT)
        .memories(MEMORY_COUNT_LIMIT)
        .build();
    let access = &instances.access_of(role);
    let handed = instances.handed.clone();
    let picks = Picks::new(handed.clone(), access.clone());
    let files = ProgramFiles::new(
        environment.folders.as_ref(),
        instances.temporary_folder.as_deref(),
        access.clone(),
        handed.clone(),
    );
    let network = ProgramNetwork::new(access.clone());
    let clipboard = ProgramClipboard::new(
        user_side.clipboard,
        access.clone(),
        program.program_views().clone(),
        program.sources().clone(),
        handed,
    );
    let links = LinkSchemes::new(
        access.clone(),
        Arc::clone(&instances.prompts),
        program.clone(),
    );
    let machines = ProgramMachines::new(
        environment.machines.clone(),
        access.clone(),
        user_side.machine_picker,
        (program.clone(), instances.commands.clone()),
    );
    let permissions = ProgramPermissions::new(
        access.clone(),
        Arc::clone(&instances.prompts),
        user_side.notifications.clone(),
        program.clone(),
    );
    let recordings_shown = user_side.recordings_shown;
    let audio = ProgramAudio::new(
        user_side.audio,
        user_side.audio_output_picker,
        access.clone(),
        recordings_shown,
        program,
    );
    let cameras = ProgramCameras::new(
        CameraShells {
            system: user_side.cameras,
            simulated: user_side.simulated_cameras,
        },
        access.clone(),
        recordings_shown,
        program.lifecycle().clone(),
    );
    StoreData {
        table: ResourceTable::new(),
        wasi: wasi::context(
            &instances.name,
            instances.clock,
            environment.output.clone(),
            &network,
            instances.temporary_folder.is_some(),
        ),
        gpu: Arc::clone(&instances.gpu),
        canvases: Canvases::default(),
        view_layers: user_side.view_layers,
        devices: Arc::clone(program.devices()),
        limits: Limits::new(limits, program.shared_stats()),
        program: program.clone(),
        role,
        placement: user_side.placement,
        compositor: user_side.compositor,
        screens: instances.screens.clone(),
        settings: instances.settings.clone(),
        caption_settings: instances.caption_settings.clone(),
        fonts: ProgramFonts::new(environment.fonts.clone()),
        files,
        http: ProgramHttp::new(network.clone(), instances.http_proxy),
        http_ctx: WasiHttpCtx::new(),
        network,
        permissions,
        clipboard,
        menus: user_side.menus,
        audio,
        file_dialogs: user_side.file_dialogs,
        picks,
        opener: user_side.opener,
        links,
        notifications: user_side.notifications,
        media_players: user_side.media_players,
        media_assets: instances.media_assets.clone(),
        image_codecs: instances.image_codecs.clone(),
        video_frame_readers: user_side.video_frame_readers,
        cameras,
        machines,
        access: access.clone(),
        instances: Arc::clone(instances),
    }
}

/// Arms the store's fuel and its epoch tick, at which the fuel it burned since the last tick is
/// counted into the program's stats and a frozen program stops until its thaw.
pub(super) fn arm(store: &mut Store<StoreData>) -> wasmtime::Result<()> {
    let stats = store.data().program.shared_stats();
    let lifecycle = store.data().program.lifecycle().clone();
    store.set_fuel(u64::MAX)?;
    store.fuel_async_yield_interval(Some(YIELD_INTERVAL))?;
    // A tick is where the fuel is read and where a freeze stops the program, not a deadline it
    // can miss: the next one is armed each time.
    store.set_epoch_deadline(1);
    let mut counted = 0;
    store.epoch_deadline_callback(move |store| {
        let burned = u64::MAX - store.as_context().get_fuel()?;
        stats.fuel_burned(burned - counted);
        counted = burned;
        Ok(match lifecycle.at_tick() {
            None => UpdateDeadline::Continue(1),
            Some(until_thawed) => UpdateDeadline::YieldCustom(1, Box::pin(until_thawed)),
        })
    });
    Ok(())
}

/// Everything one instance's imports read and write.
pub(crate) struct StoreData {
    table: ResourceTable,
    wasi: WasiCtx,
    /// The instance the program's devices are opened on: the engine's one.
    gpu: GpuInstance,
    /// The canvases its views are drawn into, one per view the program took a context on.
    canvases: Canvases,
    /// The layers each of its views holds beneath its picture, as it set them last: the one
    /// strong hold on them, so they go with the store.
    view_layers: Arc<ViewLayers>,
    /// The devices the program opened, told of each, shared with the program's handle for the
    /// wait on them all.
    devices: Arc<OpenedDevices>,
    limits: Limits,
    program: RunningProgram,
    role: InstanceRole,
    /// What places the program's views; none in an instance that has none.
    placement: Option<Arc<dyn ViewPlacement>>,
    /// The system's compositor, which makes its composition layers, where the shell has one.
    compositor: Option<Arc<dyn ShellCompositor>>,
    screens: Screens,
    settings: SystemSettings,
    /// The user's caption settings, where the system keeps them.
    caption_settings: Option<CaptionSettings>,
    /// The fonts it was given, with the ids of the faces it was handed and the glyphs it drew.
    fonts: ProgramFonts,
    /// Its files: its own folders and its grants' mounts.
    files: ProgramFiles,
    /// What it may reach of the network, and the addresses its lookups answered; the same one
    /// checks its sockets, in `wasi`.
    network: ProgramNetwork,
    /// Its HTTP requests, checked by the same network; and `wasi:http`'s own state.
    http: ProgramHttp,
    http_ctx: WasiHttpCtx,
    /// What the user lets it use, and what asks the user.
    permissions: ProgramPermissions,
    /// The platform's clipboard, as the program reaches it.
    clipboard: ProgramClipboard,
    /// The shell's places for menus.
    menus: MenuPlaces,
    /// Its sound: the devices it was given, the outputs picked for it and its microphones.
    audio: ProgramAudio,
    /// The shell's file dialogs, where it has them; and where their picks are mounted, and the
    /// files the user opens with the program.
    file_dialogs: Option<Arc<dyn ShellFileDialogs>>,
    picks: Picks,
    /// The system's opener, where the shell has one, and which links the program may open.
    opener: Option<Arc<dyn ShellOpener>>,
    links: LinkSchemes,
    /// The system's notifications, where the shell has them.
    notifications: Option<Arc<dyn ShellNotifications>>,
    /// The system's players, where the shell has them.
    media_players: Option<Arc<dyn ShellMediaPlayers>>,
    /// The system's reading of media without playing it, where the shell has one.
    media_assets: Option<Arc<dyn ShellMediaAssets>>,
    /// The system's image codecs, where the shell has them.
    image_codecs: Option<Arc<dyn ShellImageCodecs>>,
    /// The system's frame readers, where the shell has them.
    video_frame_readers: Option<Arc<dyn ShellVideoFrameReaders>>,
    /// The system's cameras, where the shell has them and can show them used, and the sessions it
    /// holds.
    cameras: ProgramCameras,
    /// The machines its user picked for it, and what picks another.
    machines: ProgramMachines,
    /// What every instance of the program is made from, which holds its capabilities for the
    /// run: each is already where it is served from, its output in `wasi`, its fonts in `fonts`,
    /// its folders in `files`, its grants in `files` and `network` and, for every other
    /// capability, its access, here, its sound devices in `audio`, its machines' providers in
    /// `machines`. A new worker is made from it too.
    /// Where every guarded use this instance makes is decided and told, as its program's.
    access: ProgramAccess,
    instances: Arc<ProgramInstances>,
}

impl HasData for StoreData {
    type Data<'a> = &'a mut StoreData;
}

impl WasiView for StoreData {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl WasiWebGpuCtxView for StoreData {
    fn webgpu_ctx(&mut self) -> WasiWebGpuCtx<'_> {
        WasiWebGpuCtx {
            instance: &self.gpu,
            table: &mut self.table,
            device_opener: gpu::device_opener(),
            submissions: Some(self.program.lifecycle()),
            devices: Some(self.devices.as_ref()),
        }
    }
}

impl ViewsCtxView for StoreData {
    fn views_ctx(&mut self) -> ViewsCtx<'_> {
        ViewsCtx {
            table: &mut self.table,
            program: &self.program,
            screens: &self.screens,
            placement: self.placement.as_deref(),
            compositor: self.compositor.as_ref(),
            gpu: &self.gpu,
            canvases: &self.canvases,
            layers: &self.view_layers,
        }
    }
}

impl WindowsCtxView for StoreData {
    fn windows_ctx(&mut self) -> WindowsCtx<'_> {
        WindowsCtx {
            table: &mut self.table,
        }
    }
}

impl ScreensCtxView for StoreData {
    fn screens_ctx(&mut self) -> ScreensCtx<'_> {
        ScreensCtx {
            table: &mut self.table,
            screens: &self.screens,
        }
    }
}

impl CaptionSettingsCtxView for StoreData {
    fn caption_settings_ctx(&mut self) -> CaptionSettingsCtx<'_> {
        CaptionSettingsCtx {
            table: &mut self.table,
            settings: self.caption_settings.as_ref(),
        }
    }
}

impl SystemCtxView for StoreData {
    fn system_ctx(&mut self) -> SystemCtx<'_> {
        SystemCtx {
            settings: &self.settings,
        }
    }
}

impl FontsCtxView for StoreData {
    fn fonts_ctx(&mut self) -> FontsCtx<'_> {
        FontsCtx {
            table: &mut self.table,
            fonts: &mut self.fonts,
        }
    }
}

impl FilesCtxView for StoreData {
    fn files_ctx(&mut self) -> FilesCtx<'_> {
        FilesCtx {
            table: &mut self.table,
            files: &mut self.files,
        }
    }
}

impl WasiHttpView for StoreData {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http_ctx,
            table: &mut self.table,
            hooks: &mut self.http,
        }
    }
}

impl NetworkCtxView for StoreData {
    fn network_ctx(&mut self) -> NetworkCtx<'_> {
        NetworkCtx {
            network: &self.network,
        }
    }
}

impl PermissionsCtxView for StoreData {
    fn permissions_ctx(&mut self) -> PermissionsCtx<'_> {
        PermissionsCtx {
            table: &mut self.table,
            permissions: &self.permissions,
        }
    }
}

impl TrustCtxView for StoreData {
    fn trust_ctx(&mut self) -> TrustCtx<'_> {
        TrustCtx {
            table: &mut self.table,
        }
    }
}

impl ClipboardCtxView for StoreData {
    fn clipboard_ctx(&mut self) -> ClipboardCtx<'_> {
        ClipboardCtx {
            table: &mut self.table,
            clipboard: &self.clipboard,
            files: &mut self.files,
            lifecycle: self.program.lifecycle(),
        }
    }
}

impl MenusCtxView for StoreData {
    fn menus_ctx(&mut self) -> MenusCtx<'_> {
        MenusCtx {
            table: &mut self.table,
            program: &self.program,
            places: &self.menus,
        }
    }
}

impl AudioCtxView for StoreData {
    fn audio_ctx(&mut self) -> AudioCtx<'_> {
        AudioCtx {
            table: &mut self.table,
            audio: &self.audio,
            permissions: &self.permissions,
        }
    }
}

impl FileDialogsCtxView for StoreData {
    fn file_dialogs_ctx(&mut self) -> FileDialogsCtx<'_> {
        FileDialogsCtx {
            table: &mut self.table,
            program: &self.program,
            dialogs: self.file_dialogs.as_ref(),
            files: &mut self.files,
            picks: &self.picks,
        }
    }
}

impl OpenerCtxView for StoreData {
    fn opener_ctx(&mut self) -> OpenerCtx<'_> {
        OpenerCtx {
            table: &mut self.table,
            program: &self.program,
            opener: self.opener.as_ref(),
            files: &mut self.files,
            links: &self.links,
        }
    }
}

impl NotificationsCtxView for StoreData {
    fn notifications_ctx(&mut self) -> NotificationsCtx<'_> {
        NotificationsCtx {
            table: &mut self.table,
            program: &self.program,
            notifications: self.notifications.as_ref(),
            access: &self.access,
        }
    }
}

impl MachinesCtxView for StoreData {
    fn machines_ctx(&mut self) -> MachinesCtx<'_> {
        MachinesCtx {
            table: &mut self.table,
            machines: &self.machines,
        }
    }
}

impl FileWatchingCtxView for StoreData {
    fn file_watching_ctx(&mut self) -> FileWatchingCtx<'_> {
        FileWatchingCtx {
            table: &mut self.table,
            access: &self.access,
        }
    }
}

impl MediaPlayersCtxView for StoreData {
    fn media_players_ctx(&mut self) -> MediaPlayersCtx<'_> {
        MediaPlayersCtx {
            table: &mut self.table,
            program: &self.program,
            media_players: self.media_players.as_ref(),
            files: &mut self.files,
            network: &self.network,
        }
    }
}

impl MediaAssetsCtxView for StoreData {
    fn media_assets_ctx(&mut self) -> MediaAssetsCtx<'_> {
        MediaAssetsCtx {
            table: &mut self.table,
            program: &self.program,
            media_assets: self.media_assets.as_ref(),
            files: &mut self.files,
            network: &self.network,
        }
    }
}

impl ImageCodecsCtxView for StoreData {
    fn image_codecs_ctx(&mut self) -> ImageCodecsCtx<'_> {
        ImageCodecsCtx {
            table: &mut self.table,
            program: &self.program,
            image_codecs: self.image_codecs.as_ref(),
            files: &mut self.files,
            gpu: &self.gpu,
        }
    }
}

impl CamerasCtxView for StoreData {
    fn cameras_ctx(&mut self) -> CamerasCtx<'_> {
        CamerasCtx {
            table: &mut self.table,
            program: &self.program,
            cameras: &self.cameras,
            permissions: &self.permissions,
        }
    }
}

impl VideoFramesCtxView for StoreData {
    fn video_frames_ctx(&mut self) -> VideoFramesCtx<'_> {
        VideoFramesCtx {
            table: &mut self.table,
            readers: self.video_frame_readers.as_ref(),
            clock: self.program.program_views().clock(),
        }
    }
}

impl HardwareBuffersCtxView for StoreData {
    fn hardware_buffers_ctx(&mut self) -> HardwareBuffersCtx<'_> {
        HardwareBuffersCtx {
            table: &mut self.table,
            gpu: &self.gpu,
            program: &self.program,
        }
    }
}

impl GpuInteropCtxView for StoreData {
    fn gpu_interop_ctx(&mut self) -> GpuInteropCtx<'_> {
        GpuInteropCtx {
            table: &mut self.table,
            gpu: &self.gpu,
        }
    }
}

impl WorkersCtxView for StoreData {
    fn workers_ctx(&mut self) -> WorkersCtx<'_> {
        WorkersCtx {
            table: &mut self.table,
            instances: &self.instances,
        }
    }
}

impl LifecycleCtxView for StoreData {
    fn lifecycle_ctx(&mut self) -> LifecycleCtx<'_> {
        LifecycleCtx {
            table: &mut self.table,
            lifecycle: self.program.lifecycle(),
            picks: &self.picks,
            hears_requests: self.role == InstanceRole::Main,
        }
    }
}
