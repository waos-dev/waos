//! What every instance of one program is made from: its code, linked once, and what serves its
//! IO side, which its main instance and each of its workers share ([`ProgramInstances`]); and its
//! user's side, the shell's parts that show the program, hear its user or ask its user, which its
//! main instance alone reaches ([`UserSide`]).
//!
//! The invariant: what one instance's store holds of the program's state is the program's own,
//! shared by every instance (its access, through which its grants are read, the files handed to
//! it, its clock, its lifecycle and stats through `program`), so a worker reaches what its program
//! reaches; and a store made without the user's side answers each interface of it as a platform
//! without it does.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use waos_capabilities::{AudioDevices, ProgramEnvironment};
use wasmtime::Engine;
use wasmtime::component::{Component, InstancePre};

use super::{IncompatibleProgram, ProgramStart, RunningProgram};
use crate::access::{KeptIn, ProgramAccess, ShownRecordings};
use crate::bindings::exports::wasdk::platform::worker_entry;
use crate::bindings::exports::wasi::cli::run;
use crate::caption_settings::CaptionSettings;
use crate::clipboard::PlatformClipboard;
use crate::clock::Clock;
use crate::files::HandedFiles;
use crate::gpu::GpuInstance;
use crate::inspection::host::InspectorReach;
use crate::machines::StartedCommands;
use crate::menus::MenuPlaces;
use crate::network::HttpProxy;
use crate::permissions::NothingGranted;
use crate::screens::Screens;
use crate::shell::{
    Prompts, ShellAudioOutputPicker, ShellCameras, ShellFileDialogs, ShellGpus, ShellImageCodecs,
    ShellMachinePicker, ShellMediaAssets, ShellMediaPlayers, ShellNotifications, ShellOpener,
    ShellSystemPermissions, ShellVideoFrameReaders, ViewPlacement,
};
use crate::source_lines::ComponentFile;
use crate::system::SystemSettings;
use crate::views::ViewLayers;
use crate::views::host::Compositor;

/// One program's instances: its code, linked once to every interface, and what each instance's
/// IO side is served from.
pub(crate) struct ProgramInstances {
    pub(super) engine: Engine,
    pub(super) linked: InstancePre<super::store::StoreData>,
    /// Where the program's `wasi:cli/run` is.
    pub(super) entry: run::GuestIndices,
    /// Where its `worker-entry` is; none where its code exports none, and has no workers.
    pub(super) worker_entry: Option<worker_entry::GuestIndices>,
    pub(super) gpu: GpuInstance,
    pub(super) name: String,
    /// The file its component was loaded from, which a worker's failure is located in.
    pub(super) file: ComponentFile,
    pub(super) prompts: Arc<dyn Prompts>,
    pub(super) system_permissions: Option<Arc<dyn ShellSystemPermissions>>,
    pub(super) screens: Screens,
    pub(super) settings: SystemSettings,
    pub(super) caption_settings: Option<CaptionSettings>,
    pub(super) media_assets: Option<Arc<dyn ShellMediaAssets>>,
    pub(super) image_codecs: Option<Arc<dyn ShellImageCodecs>>,
    pub(super) environment: ProgramEnvironment,
    /// Every guarded use of the program, whichever instance makes it: its grants, or nothing
    /// granted where its environment gives none.
    pub(super) access: ProgramAccess,
    /// What it sees of the programs while it holds the inspector role.
    pub(super) inspector_reach: InspectorReach,
    pub(super) http_proxy: HttpProxy,
    /// The files the user pastes, picks and opens the program with during its run, mounted for
    /// every instance: those picked through its grants where they keep them.
    pub(super) handed: HandedFiles,
    /// The commands every instance started, which end as the program does.
    pub(super) commands: StartedCommands,
    /// How many workers have started, which numbers each as its instance.
    workers_started: AtomicU32,
    /// Where its `/tmp` is on the machine, where it has one.
    pub(super) temporary_folder: Option<PathBuf>,
    pub(super) clock: Clock,
    pub(super) program: RunningProgram,
}

impl ProgramInstances {
    /// The program `start` names, its code linked by `linker`, its uses decided through
    /// `access`, and its user's side apart: an [`IncompatibleProgram`] when one of its imports is
    /// not there in that shape.
    pub(super) fn link(
        start: ProgramStart,
        linker: &wasmtime::component::Linker<super::store::StoreData>,
        program: RunningProgram,
        access: ProgramAccess,
        clock: Clock,
    ) -> wasmtime::Result<(ProgramInstances, UserSide)> {
        let linked = link(linker, &start.component)?;
        let entry = run::GuestIndices::new(&linked)?;
        let worker_entry = worker_entry::GuestIndices::new(&linked).ok();
        let inspector_reach = InspectorReach {
            programs: start.inspected_programs.clone(),
            running: start.programs.clone(),
            days: start.days.clone(),
            access: access.clone(),
        };
        let user_side = UserSide {
            placement: Some(start.placement),
            compositor: start.compositor,
            clipboard: start.clipboard,
            menus: start.menus,
            audio: start.environment.audio.clone(),
            audio_output_picker: start.audio_output_picker,
            recordings_shown: start.recording_indicator.is_some(),
            file_dialogs: start.file_dialogs,
            opener: start.opener,
            notifications: start.notifications,
            media_players: start.media_players,
            video_frame_readers: start.video_frame_readers,
            gpus: start.gpus,
            cameras: start.cameras,
            simulated_cameras: start.simulated_cameras,
            machine_picker: start.machine_picker,
            view_layers: start.view_layers,
        };
        let instances = ProgramInstances {
            engine: start.engine,
            linked,
            entry,
            worker_entry,
            gpu: start.gpu,
            name: start.name,
            file: start.file,
            prompts: start.prompts,
            system_permissions: start.system_permissions,
            screens: start.screens,
            settings: start.settings,
            caption_settings: start.caption_settings,
            media_assets: start.media_assets,
            image_codecs: start.image_codecs,
            environment: start.environment,
            access,
            inspector_reach,
            http_proxy: start.http_proxy,
            handed: program.handed(),
            commands: StartedCommands::default(),
            workers_started: AtomicU32::new(0),
            temporary_folder: start.temporary_folder,
            clock,
            program,
        };
        Ok((instances, user_side))
    }

    /// The program's access for an instance of `role`: the main one's, 0, or the next worker's,
    /// numbered from 1 as they start.
    pub(super) fn access_of(&self, role: super::store::InstanceRole) -> ProgramAccess {
        match role {
            super::store::InstanceRole::Main => self.access.for_instance(0),
            super::store::InstanceRole::Worker => {
                let worker = self.workers_started.fetch_add(1, Ordering::Relaxed) + 1;
                self.access.for_instance(worker)
            }
        }
    }

    /// Whether the program's code has workers: whether it exports `worker-entry`.
    pub(crate) fn has_workers(&self) -> bool {
        self.worker_entry.is_some()
    }
}

/// The access of the program `start` names: its grants, or nothing granted where its environment
/// gives none, its recordings shown by the shell's indicator, where the shell has one, and its
/// days kept by the engine's audit log under its name, where the engine has one. Made before the
/// program is linked, so what its start asks is told.
pub(super) fn access_of(start: &ProgramStart, program: &RunningProgram) -> ProgramAccess {
    let grants = start
        .environment
        .grants
        .clone()
        .unwrap_or_else(|| Arc::new(NothingGranted));
    let shown = start
        .recording_indicator
        .as_ref()
        .map(|indicator| ShownRecordings {
            indicator: Arc::clone(indicator),
            program: program.clone(),
        });
    let kept = start.days.as_ref().map(|days| KeptIn {
        days: days.clone(),
        program: program.name().to_owned(),
    });
    ProgramAccess::of(grants, shown, kept)
}

/// `component` linked to what `linker` serves, before anything of it runs: an
/// [`IncompatibleProgram`] when one of its imports is not there in that shape.
fn link(
    linker: &wasmtime::component::Linker<super::store::StoreData>,
    component: &Component,
) -> wasmtime::Result<InstancePre<super::store::StoreData>> {
    linker
        .instantiate_pre(component)
        .map_err(|link_error| wasmtime::Error::new(IncompatibleProgram::new(&link_error)))
}

/// What a program's main instance alone reaches: the shell's parts that show the program, hear
/// its user or ask its user. Each is none where the shell has none, and all are none in a worker.
pub(crate) struct UserSide {
    /// What places the program's views, and so whether the instance has them.
    pub(super) placement: Option<Arc<dyn ViewPlacement>>,
    pub(super) compositor: Option<Compositor>,
    pub(super) clipboard: PlatformClipboard,
    pub(super) menus: MenuPlaces,
    /// The sound devices its graph plays on and records from.
    pub(super) audio: Option<Arc<dyn AudioDevices>>,
    pub(super) audio_output_picker: Option<Arc<dyn ShellAudioOutputPicker>>,
    /// Whether the shell can show that the program records, without which it records nothing.
    pub(super) recordings_shown: bool,
    pub(super) file_dialogs: Option<Arc<dyn ShellFileDialogs>>,
    pub(super) opener: Option<Arc<dyn ShellOpener>>,
    pub(super) notifications: Option<Arc<dyn ShellNotifications>>,
    pub(super) media_players: Option<Arc<dyn ShellMediaPlayers>>,
    pub(super) video_frame_readers: Option<Arc<dyn ShellVideoFrameReaders>>,
    pub(super) gpus: Option<Arc<dyn ShellGpus>>,
    pub(super) cameras: Option<Arc<dyn ShellCameras>>,
    pub(super) simulated_cameras: Option<Arc<dyn ShellCameras>>,
    pub(super) machine_picker: Option<Arc<dyn ShellMachinePicker>>,
    /// The layers its views will hold, which its store keeps and its views let go of as they go.
    pub(super) view_layers: Arc<ViewLayers>,
}

impl UserSide {
    /// The user's side of a worker: none of it, each part as a shell without it.
    pub(super) fn none() -> UserSide {
        UserSide {
            placement: None,
            compositor: None,
            clipboard: PlatformClipboard::new(None),
            menus: MenuPlaces::none(),
            audio: None,
            audio_output_picker: None,
            recordings_shown: false,
            file_dialogs: None,
            opener: None,
            notifications: None,
            media_players: None,
            video_frame_readers: None,
            gpus: None,
            cameras: None,
            simulated_cameras: None,
            machine_picker: None,
            view_layers: Arc::default(),
        }
    }
}
