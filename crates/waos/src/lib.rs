//! The engine: runs programs and serves `wasdk:platform` and `wasdk:media` to them. Each interface's module holds
//! its state beside the code that serves it; the runtime, the GPU side, the shell's traits and
//! the public API are modules of their own.
//!
//! The invariant: the engine reaches nothing outside itself. Every service a program uses is a
//! capability handed to it in its `ProgramEnvironment`; everything the person sees or decides
//! is the shell's, reached through its traits. No layer, window or UI toolkit type comes in (a
//! composition layer is the shell's own value, which the engine holds and never looks into, asking
//! it only what it shows for a screenshot), and no GPU object goes out: the one handoff is the
//! shell's `ShellView`, which shows the hardware buffers this crate makes.
//!
//! Within it, an interface's state names no wasmtime runtime type and no GPU type, only the types
//! wasmtime generates from the WIT ([`bindings`]), so its tests need no engine; the code serving
//! the interface, its `host` module, is where wasmtime's resources and streams are made. Built so
//! far, steps 4a to 4c of the design: [`WaOS`] loads a program through its code cache and runs it
//! on a thread of its own, with the views the shell gives it, until it ends or is stopped, when
//! every GPU object it held goes back with its store, freezing it out of sight by [`FreezePolicy`]
//! and while the app is suspended; the bindings for views, windows, screens, the system and the
//! lifecycle, the fonts its environment gives it, the permissions its grants give it and the
//! shell's [`Prompts`] ask its user for, the files its mounts give it, the network its grants allow
//! and the system's check of a server's certificate, the platform's clipboard, the menus the shell
//! has places for, the sound its devices play and record, the file dialogs, the system's opener and
//! its notifications, the machines its user picked for it, the system's players, which play its
//! media on the composition layers it makes or into the frame readers it makes, a link only once
//! its host passed the network rules and a path where its mount puts it, the system's reading of
//! its media without playing it, by the same rules, its length, tracks, tags and pictures, the
//! system's codecs, which decode and encode its images, into its memory and out of it and into
//! hardware buffers and out of them, an image's file where its mount puts it, the
//! frames those readers answer for a frame's display time, sampled through textures it imports on
//! its own device over their hardware buffers (`gpu-interop`), whose pixels it reads back into its
//! memory, the system's cameras, opened once its user allows it, run in a format they
//! list with their preview on its layers, their frames in its readers and their photos, and shown
//! by the shell's indicator while it holds a session, the WASI a program's `std` imports, and
//! `wasi:webgpu` on one
//! GPU instance; and, on Apple's systems, the hardware buffers a view's frames
//! are drawn into, of which a frozen view keeps only the one on screen, with the GPU idle before the
//! app's suspending resolves, each handed to the shell through its [`ShellView`] with the
//! composition layers the program made on the shell's [`ShellCompositor`] and set on the view
//! beneath it; and a view's screenshot, its last picture over what each of those layers shows,
//! drawn on a GPU device of the engine's own (`screenshots`).

// Proving that a store's data crosses threads walks wgpu-core's registries, deeper than the
// default limit allows.
#![recursion_limit = "256"]

mod access;
mod asking;
mod audio;
pub mod bindings;
mod cameras;
mod caption_settings;
mod channel;
mod clipboard;
mod clock;
mod code_cache;
mod engine;
mod engine_time;
mod epoch;
mod file_dialogs;
mod file_watching;
mod files;
mod fonts;
mod gpu;
mod gpu_interop;
mod hardware_buffers;
mod image_codecs;
mod lifecycle;
mod machines;
mod media_assets;
mod media_players;
mod media_sources;
mod menus;
mod network;
mod newest;
mod news;
mod notifications;
mod opener;
mod os;
mod permissions;
mod program;
mod recordings;
mod running;
mod screens;
mod screenshots;
mod shell;
mod stats;
mod stream;
mod sync;
mod system;
pub mod targets;
mod temporary_folders;
mod trust;
mod video_frames;
mod views;
mod wasi;
mod windows;
mod workers;

pub use cameras::{CameraOutputs, CaptureSessionReports};
pub use clipboard::CopyOptions;
pub use engine::{PlatformState, WaOS, WaOSConfig};
pub use engine_time::{EngineTime, ManualTime};
pub use file_dialogs::set_answers::refusal as file_dialog_answer_refusal;
pub use file_dialogs::{FileType, FolderOptions, OpenOptions, SaveOptions};
pub use files::machine_paths;
pub use lifecycle::{FreezePolicy, OpenRequest, Presence};
pub use media_players::MediaPlayerReports;
pub use menus::TrayIconReports;
pub use network::HttpProxy;
pub use notifications::{Notification, NotificationAction};
pub use program::Program;
pub use running::{IncompatibleProgram, ProgramEnd, RunningProgram};
pub use screens::Screen;
pub use shell::{
    AnswerFuture, AudioOutputPickedFuture, CameraChangeFuture, CameraListFuture, CameraOpenFuture,
    CaptureStartFuture, ClipboardReadFuture, ClipboardWriteFuture, Content, ContentFuture,
    CopiedItem, FileDialogFuture, FileSink, FileWriteFuture, HardwareBuffer, ImageFuture,
    LayerPicture, MachinePickedFuture, MediaAssetFuture, MenuChosenFuture, NoPrompts,
    NotificationFuture, OpenFuture, OpenedCamera, OpenedImage, PhotoFuture, PlacedView,
    PlacedWindow, Prompts, RecordingDevice, SettingsClosedFuture, Shell, ShellAttachedMenu,
    ShellAudioOutputPicker, ShellCamera, ShellCameras, ShellCaptureSession, ShellClipboard,
    ShellClipboardItem, ShellCompositionLayer, ShellCompositor, ShellContextMenus,
    ShellFileDialogs, ShellHardwareImageCodecs, ShellImageCodecs, ShellImageSource,
    ShellMachinePicker, ShellMediaAsset, ShellMediaAssets, ShellMediaPlayer, ShellMediaPlayers,
    ShellMenuAtIcon, ShellMenuBar, ShellNotificationScheduling, ShellNotifications, ShellOpener,
    ShellRecordingIndicator, ShellSystemPermission, ShellTextInput, ShellTrayIcon, ShellTrayIcons,
    ShellVideoFrameReader, ShellVideoFrameReaders, ShellView, ShellWindow, ShellWindowSizeLimits,
    StartedSession, SystemPermissionFuture, TrayIconAddFuture, VideoFrame, VideoOutput,
    VideoPicture, ViewPlacement, ViewRequest, VsyncWaiter,
};
pub use stats::Stats;
pub use views::{HeldModifiers, KeyHandledFuture, ProgramView, Watching};
/// A machine as the shell's picker shows it.
pub use waos_capabilities::MachineChoice;
/// What the user lets a program use, as its environment's grants give it and the shell's
/// prompts ask for it.
pub use waos_capabilities::{Answer, FileDialog, ProgramGrants, Question, SimulatedSound, Switch};
/// A program's sound devices, and an output among them as the shell's picker shows it.
pub use waos_capabilities::{AudioDeviceError, AudioDevices, AudioOutput};
pub use waos_capabilities::{OutputStream, ProgramEnvironment, ProgramOutput};
pub use windows::WindowConfig;
