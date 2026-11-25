//! What the embedding app supplies: where a program's views go ([`ViewPlacement`]), the screens'
//! refreshes ([`VsyncWaiter`]), the system's compositor, which makes the layers a program places
//! beneath its drawing ([`ShellCompositor`], each layer a [`ShellCompositionLayer`], answering what
//! it shows now as a [`LayerPicture`]), what asks the
//! user for what a program may use ([`Prompts`], with [`NoPrompts`] for a shell with nobody to
//! ask), the shell's side of each WIT resource minus what the engine keeps ([`ShellView`],
//! [`ShellTextInput`], [`ShellWindow`], [`ShellWindowSizeLimits`], [`ShellClipboard`],
//! [`ShellClipboardItem`],
//! [`ShellMenuBar`], [`ShellContextMenus`], [`ShellTrayIcons`], [`ShellTrayIcon`],
//! [`ShellAttachedMenu`], [`ShellMenuAtIcon`], [`ShellFileDialogs`], [`ShellOpener`],
//! [`ShellNotifications`], [`ShellNotificationScheduling`], [`ShellSystemPermissions`],
//! [`ShellMediaPlayers`], [`ShellMediaPlayer`], [`ShellMediaAssets`], [`ShellMediaAsset`],
//! [`ShellImageCodecs`], [`ShellImageSource`], [`ShellHardwareImageCodecs`], with what the engine
//! keeps of an image as it opens, [`OpenedImage`], [`ShellVideoFrameReaders`],
//! [`ShellVideoFrameReader`], [`ShellCameras`], [`ShellCamera`], [`ShellCaptureSession`], with what
//! the engine keeps of a camera as it opens and of a session as it starts, [`OpenedCamera`] and
//! [`StartedSession`]), the picker of a sound's output ([`ShellAudioOutputPicker`]) and what shows
//! which programs record, and with what ([`ShellRecordingIndicator`], each device a
//! [`RecordingDevice`]), the picker of a machine for a program ([`ShellMachinePicker`]), a view as
//! it is when the shell adds it ([`PlacedView`]), what a program writes to the clipboard as the
//! shell takes it ([`CopiedItem`]), and where a player's picture goes ([`VideoOutput`]). One trait
//! per job, each replaceable on its own.
//!
//! The invariant: the engine keeps every copy and every rule, and asks the shell only for what
//! needs the platform. Every call here may come from any thread, the program's among them, and
//! nothing here blocks on the shell's thread: a method is a request the shell carries out on its
//! own loop, an answer that needs that loop is a future, and what it reports back comes through the
//! view's `ProgramView`, a program's `RunningProgram`, a tray icon's `TrayIconReports`, a player's
//! `MediaPlayerReports` or a capture session's `CaptureSessionReports`, or for a screen's refresh
//! through the `OnRefresh` the engine handed over with its ask. No GPU object crosses here: a hardware
//! buffer and a video's frame are the platform's own types, an image the hardware codecs decoded
//! a hardware buffer. A composition layer and a frame
//! reader are the shell's own values, which the engine holds and hands back and never looks into.

use std::any::Any;
use std::future::{Future, ready};
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use futures::future::BoxFuture;
use futures::stream::BoxStream;

use waos_capabilities::{Answer, AudioOutput, MachineChoice, Question, Role, Switch};

use crate::bindings::wasdk::media::cameras::{CameraError, CameraFormat, CameraInfo, Photo};
use crate::bindings::wasdk::media::image_codecs::{ImageMetadata, ImageOrientation, ImagePictures};
use crate::bindings::wasdk::media::media_players::{
    MediaMetadata, MediaTrack, PlayerError, SeekTolerance,
};
use crate::bindings::wasdk::media::video_decoders::{
    DecoderError, VideoDecodeSupport, VideoDecoderOptions, VideoStreamDescription,
};
use crate::bindings::wasdk::media::video_frames::{HdrMetadata, PixelAspect, VideoColor};
use crate::bindings::wasdk::media::video_streams::{CodecKind, EncodedChunk, VideoStreamSetup};
use crate::bindings::wasdk::platform::clipboard::ClipboardError;
use crate::bindings::wasdk::platform::file_dialogs::DialogError;
use crate::bindings::wasdk::platform::hardware_buffers::{ImageError, PixelBuffer, PixelFormat};
use crate::bindings::wasdk::platform::menus::{Image, MenuEntry, TrayIconError};
use crate::bindings::wasdk::platform::notifications::NotificationError;
use crate::bindings::wasdk::platform::opener::OpenError;
use crate::bindings::wasdk::platform::screens::ScreenId;
use crate::bindings::wasdk::platform::views::{
    Cursor, Metrics, OverlayStyle, PixelRect, Placement, RequestId, SystemBarsBehavior,
    TextEditingValue, TextInputConfiguration, ViewError, ViewFocusDirection, ViewFocusState,
    ViewId,
};
use crate::bindings::wasdk::platform::windows::{Rect, Size, TitleBarAction};
use crate::cameras::{CameraOutputs, CaptureSessionReports};
use crate::clipboard::CopyOptions;
use crate::file_dialogs::{FolderOptions, OpenOptions, SaveOptions};
use crate::lifecycle::Presence;
use crate::media_players::MediaPlayerReports;
use crate::menus::TrayIconReports;
use crate::notifications::Notification;
use crate::running::RunningProgram;
use crate::windows::WindowConfig;

/// The embedding app's parts, one per job.
pub struct Shell {
    /// Where a program's views go.
    pub views: Arc<dyn ViewPlacement>,
    /// The screens' refreshes, which serve the views' frames.
    pub vsync: Arc<dyn VsyncWaiter>,
    /// The system's compositor, which makes the layers a program places beneath its drawing;
    /// none for a shell without one, and then a program's `get-compositor` answers none.
    pub compositor: Option<Arc<dyn ShellCompositor>>,
    /// What asks the user for what a program may use.
    pub prompts: Arc<dyn Prompts>,
    /// What the system itself allows of what a program's user allowed it; none for a shell whose
    /// system guards nothing beyond what the user allowed in WaOS.
    pub system_permissions: Option<Arc<dyn ShellSystemPermissions>>,
    /// The platform's clipboard; none for a shell without one, and then a program's
    /// `get-clipboard` answers none.
    pub clipboard: Option<Arc<dyn ShellClipboard>>,
    /// The platform's menu bar; none for a shell without one, and then a program's
    /// `get-menu-bar` answers none.
    pub menu_bar: Option<Arc<dyn ShellMenuBar>>,
    /// The platform's context menus; none for a shell without them, and then a program's
    /// `get-context-menus` answers none.
    pub context_menus: Option<Arc<dyn ShellContextMenus>>,
    /// The platform's tray; none for a shell without one, and then a program's `get-tray-icons`
    /// answers none.
    pub tray_icons: Option<Arc<dyn ShellTrayIcons>>,
    /// The picker of a sound's output; none for a shell without one, and then a program's
    /// `select-audio-output` answers `not-supported`, and its sound plays on the default output.
    pub audio_output_picker: Option<Arc<dyn ShellAudioOutputPicker>>,
    /// What shows the user which programs record, and with what; none for a shell that cannot,
    /// and then a program's `open-microphone` answers `not-supported`, and its `get-cameras` none: a
    /// program records only where the user can see it does (`2026-10-29-sound.md`: "Recording
    /// waits for what shows it"; `2026-10-29-media.md`, as settled before it: WaOS shows which
    /// program uses the camera).
    pub recording_indicator: Option<Arc<dyn ShellRecordingIndicator>>,
    /// The platform's file dialogs; none for a shell without them, and then a program's
    /// `get-file-dialogs` answers none.
    pub file_dialogs: Option<Arc<dyn ShellFileDialogs>>,
    /// The system's opener; none for a shell without one, and then a program's `get-opener`
    /// answers none.
    pub opener: Option<Arc<dyn ShellOpener>>,
    /// The system's notifications; none for a shell without them, and then a program's
    /// `get-notifications` answers none.
    pub notifications: Option<Arc<dyn ShellNotifications>>,
    /// The system's players; none for a shell without them, and then a program's
    /// `get-media-players` answers none.
    pub media_players: Option<Arc<dyn ShellMediaPlayers>>,
    /// The system's reading of media without playing it; none for a shell without it, and then a
    /// program's `get-media-assets` answers none.
    pub media_assets: Option<Arc<dyn ShellMediaAssets>>,
    /// The system's image codecs; none for a shell without them, and then a program's
    /// `get-image-codecs` answers none.
    pub image_codecs: Option<Arc<dyn ShellImageCodecs>>,
    /// The system's frame readers; none for a shell whose system hands out no frames, and then a
    /// program's `get-video-frame-readers` answers none.
    pub video_frame_readers: Option<Arc<dyn ShellVideoFrameReaders>>,
    /// The system's video decoders; none for a shell without them, and then a program's
    /// `get-video-decoders` answers none.
    pub video_decoders: Option<Arc<dyn ShellVideoDecoders>>,
    /// The system's cameras; none for a shell without them, and then a program's `get-cameras`
    /// answers none.
    pub cameras: Option<Arc<dyn ShellCameras>>,
    /// Cameras of WaOS's own, a test pattern, which a program whose camera answer is simulated
    /// lists and opens in place of the system's; none for a shell without them, and then such a
    /// program lists no camera.
    pub simulated_cameras: Option<Arc<dyn ShellCameras>>,
    /// The picker of a machine for a program; none for a shell without one, and then a
    /// program's `pick` answers none, as when the user cancels.
    pub machine_picker: Option<Arc<dyn ShellMachinePicker>>,
}

/// What asks the user for what a program may use: the question put at its first use, the
/// question put at its first link of a scheme of its own, and a switch's page in the app's
/// settings, opened for the user, as an iOS app sends its user to its page in Settings. The engine
/// keeps what the user answers through the program's grants.
pub trait Prompts: Send + Sync {
    /// Puts `question` to the user for `program`, and resolves to the answer; to none when nobody
    /// answered, as when the program ended first. The engine puts one question to the user at a
    /// time for a program, however often the program asks. Called from the program's thread.
    fn ask(&self, program: &RunningProgram, question: Question) -> AnswerFuture;

    /// Asks the user whether `program` may open links of `scheme`, any but `http`, `https` and
    /// `mailto`, which open freely, in lower case (`zoommtg`), and resolves to the answer; to none
    /// when nobody answered. The engine asks once at a time for a scheme of a program's, however
    /// often the program opens its links. Called from the program's thread.
    fn ask_to_open_links(&self, program: &RunningProgram, scheme: &str) -> AnswerFuture;

    /// Asks the user whether `program` may hold `role`, which it asks for by importing the
    /// role's interfaces, saying what the role sees, and resolves to the answer, Allow or Don't
    /// allow; to none when nobody answered. Asked at the program's start, before any of it runs,
    /// while the user has not answered. Called from the program's thread.
    fn ask_for_role(&self, program: &RunningProgram, role: Role) -> AnswerFuture;

    /// Opens `switch`'s page in `program`'s settings for the user, and resolves once the user
    /// leaves it. Called from the program's thread.
    fn open_settings(&self, program: &RunningProgram, switch: Switch) -> SettingsClosedFuture;
}

/// What [`Prompts::ask`] answers.
pub type AnswerFuture = Pin<Box<dyn Future<Output = Option<Answer>> + Send>>;

/// What [`Prompts::open_settings`] answers.
pub type SettingsClosedFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Prompts for a shell with nobody to ask, as a headless runner has: every question is refused,
/// links of every scheme and every role asked about with it, and no settings open, so asking for a switch
/// answers at once.
pub struct NoPrompts;

impl Prompts for NoPrompts {
    fn ask(&self, _program: &RunningProgram, _question: Question) -> AnswerFuture {
        Box::pin(ready(Some(Answer::Denied)))
    }

    fn ask_to_open_links(&self, _program: &RunningProgram, _scheme: &str) -> AnswerFuture {
        Box::pin(ready(Some(Answer::Denied)))
    }

    fn ask_for_role(&self, _program: &RunningProgram, _role: Role) -> AnswerFuture {
        Box::pin(ready(Some(Answer::Denied)))
    }

    fn open_settings(&self, _program: &RunningProgram, _switch: Switch) -> SettingsClosedFuture {
        Box::pin(ready(()))
    }
}

/// The screens' refreshes: Flutter's `VsyncWaiter`, for any screen. The engine decides which
/// views a refresh serves; the shell only says when it comes.
pub trait VsyncWaiter: Send + Sync {
    /// Calls `on_refresh` once, at the next refresh of `screen`, with when it began and when the
    /// picture drawn for it is expected on screen, whether or not anything on it is in sight, and
    /// never inside this call: a refresh may ask for the next. The engine asks again for each
    /// refresh it wants, and never twice before the one asked for comes. Called from any thread.
    fn next_vsync(&self, screen: ScreenId, on_refresh: OnRefresh);
}

/// What a shell calls at a screen's refresh it was asked for: when the refresh began, and when the
/// picture drawn for it is expected on screen. As a browser's `requestAnimationFrame` takes its
/// callback, the engine hands it over with each ask, so a shell needs no way back to the engine.
pub type OnRefresh = Box<dyn FnOnce(Instant, Instant) + Send>;

/// Where a program's views go: what answers a program's `request-view`. A desktop's shell makes
/// a window for each request, and Android a task; a shell that showed a screen for the program
/// before it asked, such as a phone's tab, answers its first request with that screen.
pub trait ViewPlacement: Send + Sync {
    /// `program` asks for a view as `request` says. `Ok` promises exactly one later answer:
    /// `program.add_view` with `request.id`, or `program.view_request_failed` with it. `Err` refuses at once, and nothing follows. Called from the program's thread.
    fn view_requested(
        &self,
        program: &RunningProgram,
        request: ViewRequest,
    ) -> Result<(), ViewError>;
}

/// One request for a view, made with `views.request-view`.
pub struct ViewRequest {
    /// What the view that arrives for it, or the failure, carries.
    pub id: RequestId,
    /// What the program set on its `window-config`; the engine keeps the request's initial data.
    pub config: WindowConfig,
}

/// A view as it is when the shell adds it: a starting value for everything the engine copies.
/// Every change after comes through the view's [`ProgramView`](crate::views::ProgramView).
pub struct PlacedView {
    /// What the engine asks of the platform for this view.
    pub view: Arc<dyn ShellView>,
    /// How the platform's input methods edit text in the view; none where it has no text input
    /// for it, and then the program's `view.text-input` answers none.
    pub text_input: Option<Arc<dyn ShellTextInput>>,
    /// The system's bars over the view; none where the platform draws none over it, as over a
    /// desktop's window, and then the program's `view.system-bars` answers none.
    pub system_bars: Option<Arc<dyn ShellSystemBars>>,
    /// The window the view is in; none where the platform shows it without one, and then the
    /// program's `view.window` answers none.
    pub window: Option<PlacedWindow>,
    pub metrics: Metrics,
    /// The screen the view is on; none while on no screen.
    pub screen: Option<ScreenId>,
    pub presence: Presence,
}

/// A window as it is when its view is added: a starting value for everything the engine copies.
/// Every change after comes through the view's [`ProgramView`](crate::views::ProgramView).
pub struct PlacedWindow {
    pub window: Arc<dyn ShellWindow>,
    /// Where it is on the desktop now, in logical points from the top-left of the main screen,
    /// y down.
    pub frame: Rect,
    /// Whether it is maximized now; on macOS, zoomed.
    pub maximized: bool,
    /// Whether it is full screen now.
    pub fullscreen: bool,
    /// What the system's settings say a double-click on a title bar does now; none where the
    /// system has no such setting.
    pub title_bar_double_click_action: Option<TitleBarAction>,
}

/// The shell's side of `views.view`, minus what the engine keeps: what shows hardware buffers with
/// the program's layers beneath them, owns the cursor and the focus, and does its own with the
/// user's Back while the program does not handle it.
pub trait ShellView: Send + Sync {
    /// Shows `buffer` from the compositor's next frame on, with `layers` beneath it, the
    /// bottom-most first, each where its placement says in the buffer's pixels, in one step: the
    /// layers the view held as the program presented, made by the shell's [`ShellCompositor`],
    /// every transform among their mutators a 4×4 matrix of 16 values, as the engine checks. A
    /// layer the last present showed and this one does not name is let go. Calls `released` once, from any
    /// thread, once the compositor is done with `buffer`, which it may learn only at the view's
    /// next present: the engine never waits for it, and draws a frame into another hardware
    /// buffer meanwhile.
    fn present(
        &self,
        buffer: HardwareBuffer,
        layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
        released: Box<dyn FnOnce() + Send + Sync>,
    );

    /// Shows `cursor` while a pointer is over the view.
    fn set_cursor(&self, cursor: Cursor);

    /// Gives the view the input focus, or takes it away, moving it in `direction`. A platform
    /// that cannot move its focus does nothing.
    fn request_focus_change(&self, state: ViewFocusState, direction: ViewFocusDirection);

    /// The program goes back within itself when the user asks from now on, or no longer: while it
    /// does, the shell sends the user's Back to the view (`ProgramView::pop_route`), and while it
    /// does not, does its own with the press. Called only where it changed, from the program's
    /// thread; a view starts not handling Back, and [`ProgramView::handles_back`] answers it at any
    /// time.
    ///
    /// [`ProgramView::handles_back`]: crate::views::ProgramView::handles_back
    fn set_handles_back(&self, handles: bool);
}

/// The shell's side of `views.system-bars`: the system's bars over one view, the status bar and
/// the navigation bar or the home indicator, on Android over the window holding the view, or
/// whatever shows it, while it does. Each is called from the program's thread each time the
/// program says it; the engine keeps no copy.
pub trait ShellSystemBars: Send + Sync {
    /// Styles the bars as `style` says, a field left out keeping what they have.
    fn set_overlay_style(&self, style: OverlayStyle);

    /// Hides the bars until [`show`](Self::show), a swipe from a screen's edge acting as
    /// `behavior` says meanwhile.
    fn hide(&self, behavior: SystemBarsBehavior);

    /// Shows the bars again, drawn over the view, as a view starts.
    fn show(&self);
}

/// The shell's side of `views.compositor`: the system's compositor, as Waft's `Compositor` is.
pub trait ShellCompositor: Send + Sync {
    /// A layer placed nowhere yet, `opaque` when nothing beneath it will show through it. It is
    /// shown wherever a view presents while holding it (`ShellView::present`), and goes once the
    /// engine and every present showing it have let go of it. Called from the program's thread.
    fn make_layer(&self, opaque: bool) -> Arc<dyn ShellCompositionLayer>;

    /// The buffers its layers show, as Waft's `Compositor::accepted_buffers` answers: read once,
    /// as the engine starts, to choose the GPU programs draw on and the memory their pictures are
    /// kept in.
    fn accepted_buffers(&self) -> AcceptedBuffers;
}

/// What a shell's compositor shows a layer's pictures from, as Waft's `AcceptedBuffers` says it:
/// GPU memory it imports, one entry per GPU in its order of preference, the first the GPU it
/// composites on; and whether it maps shared memory, which the CPU writes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AcceptedBuffers {
    pub gpu_memory: Vec<GpuMemory>,
    pub shared_memory: bool,
}

impl AcceptedBuffers {
    /// The GPU it composites on, where its system names it.
    pub fn gpu(&self) -> Option<GpuId> {
        self.gpu_memory.first().and_then(|memory| memory.gpu)
    }
}

/// The formats a compositor imports from one GPU's memory: `gpu` none where the system names no
/// GPU, as Apple's systems and Android do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuMemory {
    pub gpu: Option<GpuId>,
    pub formats: Vec<AcceptedFormat>,
}

/// One format a compositor imports, and on Linux the DRM format modifiers it imports it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedFormat {
    pub format: PixelFormat,
    pub modifiers: Vec<u64>,
}

/// A GPU as its system names it, as Waft's `GpuId` does: two are the same GPU exactly when equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GpuId {
    /// Windows: the adapter's locally unique id.
    Luid { low: u32, high: i32 },
    /// Linux: the device number of the GPU's DRM render node.
    DrmRenderNode { major: u32, minor: u32 },
}

/// One layer a [`ShellCompositor`] made: the shell's own value, which the engine holds while the
/// program or one of its views does and hands back with each present of a view holding it, and
/// never looks into. The shell gets its own type back by downcasting through `Any`, as Waft's hosts
/// get their layers back.
pub trait ShellCompositionLayer: Any + Send + Sync {
    /// Shows `buffer`, a hardware buffer the program drew, filling the layer, from the
    /// compositor's next frame on, in place of what the layer showed: the buffers presented before,
    /// or a player's or a camera's picture, which then goes nowhere, as Waft's
    /// `CompositionLayer::present` shows one. The engine hands it over once the GPU has drawn it,
    /// from a thread of the layer's own, in the order the program presented. Calls `released` once,
    /// from any thread, once the compositor is done with `buffer`, which it may learn only at the
    /// layer's next present, or as the layer goes.
    fn present(&self, buffer: HardwareBuffer, released: Box<dyn FnOnce() + Send + Sync>);

    /// What the layer shows now, as Waft's `CompositionLayer::current_picture` answers it, case
    /// for case: the buffer presented last, or the current frame of the player's video or the
    /// camera's picture on it; none while it shows nothing. What a tool's picture of the program's
    /// window draws beneath the program's own (the design's decision 62); programs never see it.
    /// Called from any thread.
    fn current_picture(&self) -> Option<LayerPicture>;
}

/// What a composition layer shows now, as the shell hands it over, raw or lent as Waft's
/// `LayerPicture` is: the platform's own types, so a video's frame stays held from its decoder's
/// or camera's pool while the engine keeps it, since a pool hands a frame's surface out again once
/// nothing holds the frame (a pool decides by the surface's use count, not its retain count).
pub enum LayerPicture {
    /// The raw buffer presented to the layer last, which fills it.
    Buffer(HardwareBuffer),
    /// The lent frame of the player's video or the camera's picture on the layer, which the
    /// system shows whole, keeping its shape, centred in the layer.
    Frame(VideoFrame),
}

/// The shell's side of `views.text-input`: the platform's input methods, which the program hands
/// each change of its text to, so they always have a copy to read, through Flutter's text input
/// calls, one method each. What they do with it comes back among the view's events.
pub trait ShellTextInput: Send + Sync {
    /// A new client, configured with `configuration`, has the connection from now on: Flutter's
    /// `TextInput.setClient`. The on-screen keyboard shows only at [`show`](Self::show).
    fn set_client(&self, configuration: TextInputConfiguration);

    /// The input method shows for the current client: Flutter's `TextInput.show`.
    fn show(&self);

    /// The current client's configuration changed: Flutter's `TextInput.updateConfig`.
    fn update_config(&self, configuration: TextInputConfiguration);

    /// The value the program holds now, which the input methods edit from here on.
    fn set_editing_state(&self, value: TextEditingValue);

    /// Where the caret is, in the view's physical pixels, for the input method's candidate
    /// window.
    fn set_caret_rect(&self, rect: PixelRect);

    /// Where the text being composed is, in the view's physical pixels.
    fn set_composing_rect(&self, rect: PixelRect);

    /// The current client gave the connection up, and the input method drops what it was
    /// composing: Flutter's `TextInput.clearClient`. The on-screen keyboard stays until
    /// [`hide`](Self::hide).
    fn clear_client(&self);

    /// The input method goes, the on-screen keyboard with it: Flutter's `TextInput.hide`.
    fn hide(&self);
}

/// The shell's side of `windows.window`, minus its frame, whether it is maximized or full screen
/// and what a double-click on a title bar does, which the engine keeps from the shell's reports.
pub trait ShellWindow: Send + Sync {
    /// Moves and sizes the window, over `animate` where the platform can animate the change.
    fn set_frame(&self, frame: Rect, animate: Option<Duration>);

    fn set_title(&self, title: &str);

    fn show(&self);

    fn hide(&self);

    /// Closes the window; its view goes away with it, which the shell reports with
    /// `remove_view`.
    fn close(&self);

    /// The smallest and largest size the user may resize the window to; none where users do not
    /// resize windows. The same answer for as long as the window is open.
    fn size_limits(&self) -> Option<&dyn ShellWindowSizeLimits>;

    /// Starts the system's own move of the window from the press under way, as a press on its
    /// title bar does; outside a press it does nothing. The program calls it as it hears the
    /// press, which the shell reported, so the press may have moved on or ended by now.
    fn drag(&self);

    /// Maximizes the window, or returns it to its size from before; on macOS, zooms it. The shell
    /// reports the change with `ProgramView::window_maximized_changed`.
    fn set_maximized(&self, maximized: bool);

    /// Minimizes the window, or restores it. The shell reports the change as the view's
    /// presence.
    fn set_minimized(&self, minimized: bool);

    /// Has the window fill the screen `screen` names, the window's own where none, or returns it
    /// to how it was before, when `screen` is not read. A shell whose windows have no such state
    /// does nothing. The shell reports the change with
    /// `ProgramView::window_fullscreen_changed`.
    fn set_fullscreen(&self, fullscreen: bool, screen: Option<ScreenId>);
}

/// The shell's side of `windows.size-limits`: the limits a window its user resizes keeps its
/// content within, the view's size without the system's title bar and border, in logical points.
/// A limit holds until it is set again, and none takes it away.
pub trait ShellWindowSizeLimits: Send + Sync {
    fn set_min_size(&self, size: Option<Size>);

    fn set_max_size(&self, size: Option<Size>);
}

/// The shell's side of `clipboard.clipboard`, minus what the engine keeps (the focus, the
/// switches, and what programs are told changed): listing and writing the platform's clipboard.
/// What another writer put on it the shell also sets on the engine each time it hears of the
/// change (`WaOS::set_clipboard`), which is when programs are told; a listing waits for no such
/// word.
pub trait ShellClipboard: Send + Sync {
    /// What is on the platform's clipboard at this call, as the shell lists it: the platform is
    /// asked as the call is made, not as the answer is first awaited, so a paste is the
    /// clipboard as it was when the user pasted. Called from the program's thread as it lists
    /// the clipboard, and from the thread the shell reports the user's paste on.
    fn items(&self) -> ClipboardItemsFuture;

    /// Replaces the platform's clipboard with `items`, each in its forms, treated as `options`
    /// says, and resolves with what is on it then, as the shell lists it: what programs are told
    /// it changed to, as `WaOS::set_clipboard` sets one. The shell reports no change for this
    /// write: a change it reports after it is another writer's, which replaces the program's
    /// copy. Called from the program's thread.
    fn write(&self, items: Vec<CopiedItem>, options: CopyOptions) -> ClipboardWriteFuture;
}

/// What [`ShellClipboard::items`] answers: what is on the clipboard.
pub type ClipboardItemsFuture =
    Pin<Box<dyn Future<Output = Vec<Arc<dyn ShellClipboardItem>>> + Send>>;

/// What [`ShellClipboard::write`] answers: what is on the clipboard after the write.
pub type ClipboardWriteFuture =
    Pin<Box<dyn Future<Output = Result<Vec<Arc<dyn ShellClipboardItem>>, ClipboardError>> + Send>>;

/// The shell's side of `clipboard.clipboard-item`: one item on the platform's clipboard as the
/// shell listed it.
pub trait ShellClipboardItem: Send + Sync {
    /// The forms it is offered in, by MIME type, the most faithful first; a file as
    /// `text/uri-list`.
    fn types(&self) -> &[String];

    /// Its content in `mime`, one of its types: a file as its `file:` URL. Fails with `changed`
    /// once the platform's clipboard has changed since the shell listed it, which is all that
    /// keeps a read from landing on a later copy: the engine does not check again. Called from
    /// the program's thread.
    fn read(&self, mime: &str) -> ClipboardReadFuture;
}

/// What [`ShellClipboardItem::read`] answers.
pub type ClipboardReadFuture =
    Pin<Box<dyn Future<Output = Result<Vec<u8>, ClipboardError>> + Send>>;

/// One item a program writes, as the shell writes it to the platform's clipboard: the same thing
/// in several forms, each keyed by its MIME type, the most faithful first.
pub struct CopiedItem {
    pub representations: Vec<(String, Content)>,
}

/// One form's content, as the shell writes it: what the program gave, with its files by their
/// paths on the machine, and its promises produced by the program when someone pastes them.
pub enum Content {
    /// The bytes, given at once.
    Data(Vec<u8>),
    /// Produced when someone pastes it, each time; none when the program gave nothing in time.
    Promise(Arc<dyn Fn() -> ContentFuture + Send + Sync>),
    /// A file, by its path on the machine.
    File(PathBuf),
    /// A file that exists only once pasted: its name, and what produces its bytes into the sink
    /// the shell hands it, each time someone pastes it. The file is whole when the future
    /// resolves `Ok`; an `Err` fails the paste, and the shell removes what it wrote.
    FilePromise {
        name: String,
        produce: Arc<dyn Fn(Box<dyn FileSink>) -> FileWriteFuture + Send + Sync>,
    },
}

/// Where a promised file's bytes go, in order: the shell's, which writes them into the file where
/// the receiver chose, as Waft's `FileSink` is the host's.
pub trait FileSink: Send {
    /// Appends `bytes`; resolves once the shell has taken them.
    fn write<'a>(
        &'a mut self,
        bytes: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'a>>;
}

/// A promised form being produced.
pub type ContentFuture = Pin<Box<dyn Future<Output = Option<Vec<u8>>> + Send>>;

/// A promised file's bytes being produced into its sink: whole once it resolves `Ok`.
pub type FileWriteFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

/// The shell's side of `menus.menu-bar`, minus what the engine keeps (each program's bar as it
/// set it last, and the streams of its choices): the platform's menu bar. Which program's bar it
/// shows is the app's to choose, reading the bar with `RunningProgram::menu_bar`; a choice in it
/// reaches the program through `RunningProgram::menu_chosen`.
pub trait ShellMenuBar: Send + Sync {
    /// `program` set its menu bar to `entries`, whole, as `RunningProgram::menu_bar` answers from
    /// now on: shown in place of the one before wherever the shell shows the program's. Called
    /// from the program's thread.
    fn set(&self, program: &RunningProgram, entries: &[MenuEntry]);
}

/// The shell's side of `menus.context-menus`: the platform's menus opened at a point of a view.
pub trait ShellContextMenus: Send + Sync {
    /// Opens `entries` at (`x`, `y`) in `program`'s view `view`, in the view's physical pixels,
    /// and resolves to the id of the item the user chose; to none when the menu closed without a
    /// choice, or the view is gone. Called from the program's thread.
    fn show(
        &self,
        program: &RunningProgram,
        view: ViewId,
        x: f64,
        y: f64,
        entries: &[MenuEntry],
    ) -> MenuChosenFuture;
}

/// What a menu opened answers: the id of the item the user chose, or none.
pub type MenuChosenFuture = Pin<Box<dyn Future<Output = Option<u32>> + Send>>;

/// The shell's side of `menus.tray-icons`: the platform's tray, where a program adds its icons.
pub trait ShellTrayIcons: Send + Sync {
    /// Adds an icon of `program`'s showing `image`, with `tooltip` shown over it, and resolves to
    /// it once the platform has made it, or to why the platform did not show it. What the user
    /// does to it, and chooses in its attached menu, the shell reports into `reports`, from any
    /// thread. Called from the program's thread.
    fn add(
        &self,
        program: &RunningProgram,
        image: &Image,
        tooltip: Option<&str>,
        reports: TrayIconReports,
    ) -> TrayIconAddFuture;
}

/// What [`ShellTrayIcons::add`] answers: the icon the platform made, or why it did not.
pub type TrayIconAddFuture =
    Pin<Box<dyn Future<Output = Result<Box<dyn ShellTrayIcon>, TrayIconError>> + Send>>;

/// The shell's side of `menus.tray-icon`, minus what the engine keeps (the streams of its events
/// and of its attached menu's choices): one icon in the platform's tray, which says which menus it
/// offers, removed when dropped, as `NSStatusBar.removeStatusItem` or
/// `Shell_NotifyIcon(NIM_DELETE)` removes one.
pub trait ShellTrayIcon: Send + Sync {
    fn set_image(&self, image: &Image);

    fn set_tooltip(&self, tooltip: Option<&str>);

    /// The menu the platform opens by itself at the icon: macOS's on a click, Linux's when the
    /// desktop decides; none where it opens none, as on Windows.
    fn attached_menu(&self) -> Option<&dyn ShellAttachedMenu>;

    /// What opens a menu at the icon now: macOS's and Windows'; none on Linux.
    fn menu_at_icon(&self) -> Option<&dyn ShellMenuAtIcon>;
}

/// The menu the platform opens by itself at a tray icon.
pub trait ShellAttachedMenu: Send + Sync {
    /// Replaces the menu with `entries`, whole. A choice in it the shell reports with
    /// `TrayIconReports::attached_menu_chosen`. Called from the program's thread.
    fn set(&self, entries: &[MenuEntry]);
}

/// What opens a menu at a tray icon.
pub trait ShellMenuAtIcon: Send + Sync {
    /// Opens `entries` at the icon now, and resolves to the id of the item the user chose; to
    /// none when the menu closed without a choice. Called from the program's thread.
    fn show(&self, entries: &[MenuEntry]) -> MenuChosenFuture;
}

/// The shell's side of `audio.select-audio-output`: the platform's picker, where the user picks
/// the output a program's sound plays on, which the program then sees alone, as the web's
/// `selectAudioOutput` shows its own.
pub trait ShellAudioOutputPicker: Send + Sync {
    /// Shows `outputs` to the user for `program`, and resolves to the id of the one picked; to
    /// none when the user dismissed the picker, or the program ended first. Called from the
    /// program's thread.
    fn pick(&self, program: &RunningProgram, outputs: Vec<AudioOutput>) -> AudioOutputPickedFuture;
}

/// What [`ShellAudioOutputPicker::pick`] answers: the id of the output picked, or none.
pub type AudioOutputPickedFuture = Pin<Box<dyn Future<Output = Option<String>> + Send>>;

/// The shell's side of `machines.pick`: the platform's sheet where the user picks a machine for a
/// program, as a photo picker shows its photos, the pick being the program's grant (the processes
/// proposal's decision 3). It shows the machine as the program can act on it: as the user.
pub trait ShellMachinePicker: Send + Sync {
    /// Shows `machines` to the user for `program`, and resolves to the id of the one picked; to
    /// none when the user cancelled, or the program ended first. Called from the program's
    /// thread.
    fn pick(&self, program: &RunningProgram, machines: Vec<MachineChoice>) -> MachinePickedFuture;
}

/// What [`ShellMachinePicker::pick`] answers: the id of the machine picked, or none.
pub type MachinePickedFuture = Pin<Box<dyn Future<Output = Option<String>> + Send>>;

/// What shows the user which programs record, and with what, as a system shows its own indicator
/// while an app uses the microphone or the camera.
pub trait ShellRecordingIndicator: Send + Sync {
    /// `program` records with `device` from now on, or no longer does: told once as its first
    /// microphone opens, or its first capture session starts, and once as its last is let go, the
    /// program ending among the ways. Called from any thread.
    fn recording_changed(&self, program: &RunningProgram, device: RecordingDevice, recording: bool);
}

/// What a program records with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecordingDevice {
    /// From its first microphone open until its last is let go.
    Microphone,
    /// From its first capture session started until its last is let go.
    Camera,
    /// A microphone of WaOS's own, for a program whose microphone answer is simulated: shown
    /// apart, so nobody takes it for the machine's.
    SimulatedMicrophone,
    /// A camera of WaOS's own, for a program whose camera answer is simulated.
    SimulatedCamera,
}

/// The shell's side of `file-dialogs.file-dialogs`, minus what the engine keeps (each pick's
/// mount): the platform's dialogs, a function per kind, each shown for `program`, as a sheet on
/// the window of its view `parent` where one is given. Each resolves once the user has chosen or
/// cancelled, to the paths on the machine chosen, which the engine mounts. Called from the
/// program's thread.
pub trait ShellFileDialogs: Send + Sync {
    /// Asks for one file to open; none if the user cancelled.
    fn open_file(
        &self,
        program: &RunningProgram,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Option<PathBuf>>;

    /// Asks for files to open; none if the user cancelled.
    fn open_files(
        &self,
        program: &RunningProgram,
        parent: Option<ViewId>,
        options: OpenOptions,
    ) -> FileDialogFuture<Vec<PathBuf>>;

    /// Asks for a folder; none if the user cancelled.
    fn open_folder(
        &self,
        program: &RunningProgram,
        parent: Option<ViewId>,
        options: FolderOptions,
    ) -> FileDialogFuture<Option<PathBuf>>;

    /// Asks where to save a file, which may not exist yet; none if the user cancelled.
    fn save_file(
        &self,
        program: &RunningProgram,
        parent: Option<ViewId>,
        options: SaveOptions,
    ) -> FileDialogFuture<Option<PathBuf>>;
}

/// What a [`ShellFileDialogs`] dialog answers, once the user has chosen or cancelled.
pub type FileDialogFuture<T> = Pin<Box<dyn Future<Output = Result<T, DialogError>> + Send>>;

/// The shell's side of `opener.opener`, minus what the engine keeps (which schemes of links a
/// program may open, and where on the machine a file it names is): the system's opener, as
/// Waft's `Opener` is, each call made for `program` and resolving once the system has handed what
/// it names to the app that opens it, or to why not: `no-app` or `failed`, never `denied`, which
/// is the engine's. Called from the program's thread.
pub trait ShellOpener: Send + Sync {
    /// Opens `link`, which the program may open, in the app the system picks for its scheme.
    fn open_link(&self, program: &RunningProgram, link: &str) -> OpenFuture;

    /// Opens the file at `path` on the machine in the app the system picks for its type.
    fn open_file(&self, program: &RunningProgram, path: &Path) -> OpenFuture;

    /// Shows the file at `path` on the machine in the system's file manager, selected.
    fn reveal_file(&self, program: &RunningProgram, path: &Path) -> OpenFuture;
}

/// What a [`ShellOpener`] call answers.
pub type OpenFuture = Pin<Box<dyn Future<Output = Result<(), OpenError>> + Send>>;

/// The shell's side of `notifications.notifications`, minus what the engine keeps (each
/// notification as the program builds it, whether the program may post, and the streams of its
/// activations): the system's notifications, as Waft's `Notifications` is. Every call names the
/// program the notification is of, whose ids are its own: the shell keeps two programs' same id
/// apart, by the program's name (`RunningProgram::name`, the id an embedder names it by), which
/// also finds a program's notifications, shown and scheduled, when the app removes the program.
/// Until a program has an identity of its own with the system, the shell posts under the app's and
/// puts the program's name in the notification. Where it has one, as each program's app of its own
/// on macOS, the system keeps a permission of its own for the program
/// ([`ShellSystemPermissions`]), which may refuse it its notifications whatever its user said in
/// WaOS. What the user does to a notification, at launch or while the
/// program runs, the shell reports with `RunningProgram::notification_activated`, starting the
/// program first when it is not running. Called from the program's thread.
pub trait ShellNotifications: Send + Sync {
    /// Posts `notification` of `program`'s, in place of one of its own under the same id, and
    /// resolves once the system has it, or to why not: `failed`, or `denied` where the system
    /// refuses the program's notifications, never for the program's own permission, which is the
    /// engine's.
    fn post(&self, program: &RunningProgram, notification: &Notification) -> NotificationFuture;

    /// Takes `program`'s notification `id` away, shown or scheduled; nothing for an id it has
    /// none of.
    fn remove(&self, program: &RunningProgram, id: &str);

    /// Whether a notification may carry an action the user answers by typing; false where the
    /// system has none.
    fn offers_replies(&self) -> bool;

    /// Delivery at a later time, which the system holds whether or not WaOS runs; none where it
    /// has none.
    fn scheduling(&self) -> Option<&dyn ShellNotificationScheduling>;
}

/// Something a program's user allowed it in WaOS which the system may guard as well, with a
/// permission of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemPermission<'a> {
    /// The program's notifications, where the system keeps a permission for each program, as macOS
    /// does for each program's app.
    Notifications,
    /// Reading and writing the file or folder at this path on the machine, which the user picked
    /// for the program, where the system lets an app reach it only with a permission of its own,
    /// as Android does the phone's shared storage.
    Path(&'a Path),
}

/// What the system says of a [`SystemPermission`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemPermissionState {
    /// The system has not been asked, and would ask the user.
    NotAsked,
    Allowed,
    /// The user, or the system's settings, said no.
    Denied,
}

/// The system's own permissions, beside what a program's user allowed it in WaOS: the user's
/// answer in WaOS is one layer, the system's another, and a program has what both allow. Each
/// system has its own way to be asked, which is the shell's: macOS keeps a permission for each
/// program's notifications, asks its user for it once, and lets them change it later in System
/// Settings; Android lets an app reach the phone's shared storage only once its user turned that
/// on for the app in the system's settings, where the shell sends them. The engine reads [`state`](Self::state) each time it is asked where a permission
/// stands, keeping no copy, and asks through [`request`](Self::request) once the program's user
/// allowed the thing in WaOS and the system does not allow it yet. No system tells an app that its
/// user changed a permission in its settings, so nothing here does: the shell reads again as it
/// can, as when WaOS comes to the front, and the next `state` answers what it read.
pub trait ShellSystemPermissions: Send + Sync {
    /// What the system says of `permission` for `program` now, as the shell last read it, without
    /// asking the user: allowed where the system guards nothing of it. Called from any thread.
    fn state(
        &self,
        permission: SystemPermission<'_>,
        program: &RunningProgram,
    ) -> SystemPermissionState;

    /// Asks the system for `permission` for `program` in the system's own way: its prompt, which
    /// asks the user only if they were never asked, or its settings page and back, telling the
    /// user where it is still off. Resolves to what the system says once that is done; to what it
    /// said before where it could not be asked. Called from the program's thread.
    fn request(
        &self,
        permission: SystemPermission<'_>,
        program: &RunningProgram,
    ) -> SystemPermissionFuture;
}

/// The system's delivery of a notification at a later time.
pub trait ShellNotificationScheduling: Send + Sync {
    /// Has the system post `notification` of `program`'s at `at`, in place of one of its own
    /// under the same id, and resolves once the system holds it, or to why not, as a post does.
    /// Called from the program's thread.
    fn schedule(
        &self,
        program: &RunningProgram,
        notification: &Notification,
        at: SystemTime,
    ) -> NotificationFuture;
}

/// What a [`ShellNotifications`] post or a schedule answers.
pub type NotificationFuture = Pin<Box<dyn Future<Output = Result<(), NotificationError>> + Send>>;

/// What [`ShellSystemPermissions::request`] answers.
pub type SystemPermissionFuture = Pin<Box<dyn Future<Output = SystemPermissionState> + Send>>;

/// The shell's side of `media-players.media-players`: the system's players, as Waft's `Players`
/// is.
pub trait ShellMediaPlayers: Send + Sync {
    /// A player of the media at `url` for `program`, which loads by itself: an `http` or `https`
    /// link the program may reach, or the `file:` URL of a file on the machine the program can
    /// read, as the engine checked. The engine holds it loading, with no metadata and no cues,
    /// until the shell reports otherwise into `reports`, from any thread, at each change of its
    /// state, its metadata and its cues, the cues as the media styles them, and where the player is
    /// at each of its changes, a jump of its position among them. Called from any thread.
    fn open(
        &self,
        program: &RunningProgram,
        url: &str,
        reports: MediaPlayerReports,
    ) -> Box<dyn ShellMediaPlayer>;
}

/// The shell's side of `media-players.media-player`, minus what the engine keeps (its state as the
/// shell reported it, and the streams of its changes): one of the system's players, which stops
/// when dropped, its picture leaving where it went.
pub trait ShellMediaPlayer: Send + Sync {
    fn play(&self);

    fn pause(&self);

    /// Moves to `position`, or as near it as `tolerance` lets the system land, playing on from
    /// there if it was playing.
    fn seek(&self, position: Duration, tolerance: SeekTolerance);

    fn set_rate(&self, rate: f64);

    fn set_volume(&self, volume: f32);

    fn set_muted(&self, muted: bool);

    fn set_looping(&self, looping: bool);

    /// Sends the picture to `output` from now on, and nowhere it went before; none sends it
    /// nowhere.
    fn set_video_output(&self, output: Option<VideoOutput>);

    /// Plays the audio track `id`, one of the state's tracks, in place of the one playing.
    fn select_audio_track(&self, id: &str);

    /// Shows the text track `id`, one of the state's tracks, or none: subtitles off. Its cues
    /// the shell reports with `MediaPlayerReports::cues_changed`.
    fn select_text_track(&self, id: Option<&str>);
}

/// The shell's side of `media-assets.media-assets`: the system's reading of media without playing
/// it, as Waft's `MediaAssets` is.
pub trait ShellMediaAssets: Send + Sync {
    /// The media at `url` for `program`, read as it is asked: an `http` or `https` link the
    /// program may reach, or the `file:` URL of a file on the machine the program can read, as the
    /// engine checked, as a player is opened. Called from any thread.
    fn open(&self, program: &RunningProgram, url: &str) -> Box<dyn ShellMediaAsset>;
}

/// The shell's side of `media-assets.media-asset`: one piece of media the system reads. Each
/// answer resolves once the system has read what it asks, or failed to, whether or not the engine
/// still holds the asset. Called from the program's thread.
pub trait ShellMediaAsset: Send + Sync {
    /// How long it is; none for a live stream.
    fn duration(&self) -> MediaAssetFuture<Option<Duration>>;

    /// Its tracks as a player lists them, each selected that a player would play unless asked
    /// otherwise.
    fn tracks(&self) -> MediaAssetFuture<Vec<MediaTrack>>;

    fn metadata(&self) -> MediaAssetFuture<MediaMetadata>;

    /// Its picture at `time`, or as near it as `tolerance` lets the system land, presented as a
    /// player presents it, scaled down to fit `max_size`, in pixels, where given: a frame as a
    /// reader's frames are.
    fn frame_at(
        &self,
        time: Duration,
        tolerance: SeekTolerance,
        max_size: Option<Size>,
    ) -> MediaAssetFuture<VideoFrame>;
}

/// What a [`ShellMediaAsset`] answers.
pub type MediaAssetFuture<T> = Pin<Box<dyn Future<Output = Result<T, PlayerError>> + Send>>;

/// The shell's side of `image-codecs.image-codecs`: the system's image codecs, as Waft's
/// `ImageCodecs` are. Called from the program's thread; each future resolves once, whether or not
/// the engine still holds what it asked of.
pub trait ShellImageCodecs: Send + Sync {
    /// The formats it decodes, by MIME type: the same for as long as the shell runs.
    fn decoded_formats(&self) -> Vec<String>;

    /// The formats it encodes, by MIME type: the same for as long as the shell runs.
    fn encoded_formats(&self) -> Vec<String>;

    /// The image `data` encodes, for `program`, with what it says of itself.
    fn open(&self, program: &RunningProgram, data: Vec<u8>) -> ImageFuture<OpenedImage>;

    /// The image in the file at `path` on the machine, for `program`: a file inside the program's
    /// mounts, as the engine checked.
    fn open_file(&self, program: &RunningProgram, path: &Path) -> ImageFuture<OpenedImage>;

    /// `pixels` encoded in `format`, a MIME type, at `quality` from 0 to 1 where given; pixels
    /// whose bytes hold their rows and a quality from 0 to 1, as the engine checked.
    fn encode_pixels(
        &self,
        pixels: PixelBuffer,
        format: &str,
        quality: Option<f32>,
    ) -> ImageFuture<Vec<u8>>;

    /// The system's hardware codecs; none for a system without them. The same answer for as long
    /// as the shell runs.
    fn hardware_image_codecs(&self) -> Option<Arc<dyn ShellHardwareImageCodecs>>;
}

/// An image the shell's codecs opened, with what it says of itself, which the engine keeps with
/// it, as Waft's `image_codecs::ImageSource` answers it.
pub struct OpenedImage {
    pub image: Box<dyn ShellImageSource>,
    /// Its format's MIME type.
    pub mime_type: String,
    /// Its size in pixels, upright.
    pub size: Size,
    /// How its pictures go: one still picture, the frames of an animation or the pages of a
    /// document.
    pub pictures: ImagePictures,
    pub orientation: ImageOrientation,
    pub metadata: ImageMetadata,
}

/// The shell's side of `image-codecs.image-source`: an image the shell's codecs opened, which the
/// shell gets its own type back from by downcasting through `Any`, as its hardware codecs are
/// handed it.
pub trait ShellImageSource: Any + Send + Sync {
    /// Picture `index`, one of the image's, upright, scaled down to fit `max_size`, in pixels,
    /// keeping its shape, where given, in the program's memory.
    fn decode_pixels(&self, index: u32, max_size: Option<Size>) -> ImageFuture<PixelBuffer>;
}

/// The shell's side of `image-codecs.hardware-image-codecs`: the system's codecs between images and
/// hardware buffers, which the engine holds as a program's `hardware-buffer`s.
pub trait ShellHardwareImageCodecs: Send + Sync {
    /// Picture `index` of `image`, an image the shell's codecs opened, in a hardware buffer of the
    /// program's own, which no pool waits for: on Apple's systems an `IOSurface` in 32-bit BGRA.
    fn decode(
        &self,
        image: &dyn ShellImageSource,
        index: u32,
        max_size: Option<Size>,
    ) -> ImageFuture<HardwareBuffer>;

    /// `buffer`, a hardware buffer of the program's, one it drew, a decoded image or a video
    /// frame's, encoded in `format` at `quality`, as [`ShellImageCodecs::encode_pixels`] takes
    /// them. The engine hands it over once the GPU has finished drawing into it, and holds it, and
    /// the frame it is the memory of, until the answer comes.
    fn encode(
        &self,
        buffer: &HardwareBuffer,
        format: &str,
        quality: Option<f32>,
    ) -> ImageFuture<Vec<u8>>;
}

/// What the shell's image codecs answer.
pub type ImageFuture<T> = Pin<Box<dyn Future<Output = Result<T, ImageError>> + Send>>;

/// Where a player's picture goes, as the shell's player is given it: `video-frames.video-output`,
/// with the shell's own layer or reader.
pub enum VideoOutput {
    /// The system draws the video on this layer, which the shell's [`ShellCompositor`] made.
    Layer(Arc<dyn ShellCompositionLayer>),
    /// The program draws the frames this reader reads, which the shell's [`ShellVideoFrameReaders`]
    /// made.
    Frames(Arc<dyn ShellVideoFrameReader>),
}

/// The shell's side of `video-frames.video-frame-readers`: the system's frame readers, as Waft's
/// `VideoFrameReaders` are.
pub trait ShellVideoFrameReaders: Send + Sync {
    /// A reader fed by nothing yet, which reads the frames of the player it is given to. It goes
    /// once the engine and its source have let go of it. Called from the program's thread.
    fn make_reader(&self) -> Arc<dyn ShellVideoFrameReader>;
}

/// One reader a [`ShellVideoFrameReaders`] made: the shell's own value, which the engine holds
/// while the program does and hands back with a player's [`VideoOutput::Frames`]. The shell gets
/// its own type back by downcasting through `Any`, as Waft's hosts get their readers back.
pub trait ShellVideoFrameReader: Any + Send + Sync {
    /// The frame to show at `display_time`: its source's frame for that moment of the video. None
    /// when it is the frame answered last, or none came. Called from the program's thread.
    fn frame_for(&self, display_time: Instant) -> Option<VideoFrame>;
}

/// The shell's side of `video-decoders.video-decoders`: the system's video decoders. Called from
/// the program's thread.
pub trait ShellVideoDecoders: Send + Sync {
    /// What the system can tell, without trying, about decoding a stream like `description`.
    fn support(&self, description: VideoStreamDescription) -> VideoDecodeSupportFuture;

    /// Opens a decoder for the stream `setup` begins, as `options` ask, with a pool that lends
    /// at least their `frame_limit` frames at once: `unsupported` where the system has none of
    /// the kind chosen for it, `busy` where every such decoder is in use.
    fn open(&self, setup: VideoStreamSetup, options: VideoDecoderOptions)
    -> VideoDecoderOpenFuture;
}

pub type VideoDecodeSupportFuture = Pin<Box<dyn Future<Output = VideoDecodeSupport> + Send>>;

pub type VideoDecoderOpenFuture =
    Pin<Box<dyn Future<Output = Result<OpenedVideoDecoder, DecoderError>> + Send>>;

/// A decoder the shell opened, with what comes out of it.
pub struct OpenedVideoDecoder {
    pub decoder: Arc<dyn ShellVideoDecoder>,
    /// Where it runs.
    pub kind: CodecKind,
    /// The decoded pictures in the order they are shown, each taken as the engine asks for the
    /// next, which it does only while the program holds fewer than it may; ends once the decoder
    /// has.
    pub frames: BoxStream<'static, DecodedFrame>,
    /// Why the decoder ended by itself, as the system taking it away; never resolves for one
    /// that ends only as the engine drops it.
    pub ended: BoxFuture<'static, DecoderError>,
}

/// The shell's side of `video-decoders.video-decoder`, minus the rules the engine keeps: that a
/// key frame begins a stream and follows a drain, a reset and a damaged chunk, and how many frames
/// the program holds at once. Dropped once the engine lets go of it, which ends its frames.
/// Called from the program's thread.
pub trait ShellVideoDecoder: Send + Sync {
    /// Hands `chunk` to the decoder, resolving once it has room for the next; `bad-data` for a
    /// chunk it cannot decode, after which it drops what comes until a key frame.
    fn decode(&self, chunk: EncodedChunk) -> VideoDecoderFuture;

    /// Resolves once every chunk given has come out as a picture.
    fn drain(&self) -> VideoDecoderFuture;

    /// Drops every chunk and picture the decoder holds.
    fn reset(&self);

    /// The stream's setup from the next chunk on, of the same codec.
    fn configure(&self, setup: VideoStreamSetup) -> VideoDecoderFuture;
}

pub type VideoDecoderFuture = Pin<Box<dyn Future<Output = Result<(), DecoderError>> + Send>>;

/// A picture a decoder put out, with what its stream says of it.
pub struct DecodedFrame {
    pub frame: VideoFrame,
    /// The timestamp of the chunk it came from, in nanoseconds on the stream's own timeline.
    pub timestamp: i64,
    /// How long it is shown, in nanoseconds, where the stream says.
    pub duration: Option<u64>,
    /// The part of its buffer that is picture, in the buffer's pixels.
    pub visible_rect: crate::bindings::wasdk::media::video_frames::PixelRect,
    pub pixel_aspect: PixelAspect,
    pub color: VideoColor,
    pub hdr_metadata: Option<HdrMetadata>,
}

/// The shell's side of `cameras.cameras`: the system's cameras, as Waft's `Cameras` are.
pub trait ShellCameras: Send + Sync {
    /// The cameras attached now, in the system's order; asks the user nothing. Called from the
    /// program's thread.
    fn list(&self) -> CameraListFuture;

    /// Resolves once [`list`](Self::list) may answer something else than when this was called: a
    /// camera plugged in or out, as Waft's `Cameras::changed`; never once the shell hears of no
    /// more changes, as when the app quits. Called from the program's thread.
    fn changed(&self) -> CameraChangeFuture;

    /// Opens the camera `id` for `program`, whose user allowed it the cameras, as the engine
    /// checked first; `unplugged` for a camera no longer attached. Called from the program's
    /// thread.
    fn open(&self, program: &RunningProgram, id: &str) -> CameraOpenFuture;
}

/// What [`ShellCameras::list`] answers.
pub type CameraListFuture = Pin<Box<dyn Future<Output = Vec<CameraInfo>> + Send>>;

/// What [`ShellCameras::changed`] answers.
pub type CameraChangeFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// What [`ShellCameras::open`] answers: the camera opened, or why it was not.
pub type CameraOpenFuture = Pin<Box<dyn Future<Output = Result<OpenedCamera, CameraError>> + Send>>;

/// A camera the shell opened, with what the engine keeps of it.
pub struct OpenedCamera {
    pub camera: Box<dyn ShellCamera>,
    /// The formats it runs in, in the system's order, which stay the same while it is open.
    pub formats: Vec<CameraFormat>,
}

/// The shell's side of `cameras.camera`, minus what the engine keeps (its formats): a camera
/// opened for a program, as Waft's `CameraDevice` is. A session started on it runs on after it is
/// dropped.
pub trait ShellCamera: Send + Sync {
    /// Starts the camera in `format`, one of its formats, as the engine checked, feeding
    /// `outputs`, which the system configures together; where the camera runs for another session
    /// in one format at a time, the session joins that format, which the answer says. Fails for
    /// what can happen at any time: the camera in use, unplugged, or not allowed. Once started,
    /// the shell reports the session's state into `reports`, from any thread, at each change; the
    /// engine holds it running until then. Called from the program's thread.
    fn start(
        &self,
        format: CameraFormat,
        outputs: CameraOutputs,
        reports: CaptureSessionReports,
    ) -> CaptureStartFuture;
}

/// What [`ShellCamera::start`] answers: the session started, or why it was not.
pub type CaptureStartFuture =
    Pin<Box<dyn Future<Output = Result<StartedSession, CameraError>> + Send>>;

/// A session the shell started, with what the engine keeps of it.
pub struct StartedSession {
    pub session: Box<dyn ShellCaptureSession>,
    /// The format it runs in: the one picked, unless it joined the format the camera ran in for
    /// another session. It stays the same while it runs, since changing a shared camera's format
    /// comes later (decision 9 of `docs/proposals/2026-11-02-program-media.md`).
    pub format: CameraFormat,
}

/// The shell's side of `cameras.capture-session`, minus what the engine keeps (its format, its
/// state as the shell reported it, and the streams of its changes): a camera running for a
/// program, as Waft's `CaptureSession` is, which stops for the program when dropped, its preview
/// leaving its layer and its reader reading nothing more.
pub trait ShellCaptureSession: Send + Sync {
    /// A photo at the size among its outputs, encoded by the system; the engine asks one of a
    /// session started with photos. `failed` while the session does not run. Called from the
    /// program's thread.
    fn take_photo(&self) -> PhotoFuture;
}

/// What [`ShellCaptureSession::take_photo`] answers.
pub type PhotoFuture = Pin<Box<dyn Future<Output = Result<Photo, CameraError>> + Send>>;

/// GPU memory the system's compositor can show: the platform's own type, as Waft's `Buffer` is.
#[cfg(target_vendor = "apple")]
pub type HardwareBuffer = objc2_core_foundation::CFRetained<objc2_io_surface::IOSurfaceRef>;

/// GPU memory the system's compositor can show: on Android an `AHardwareBuffer`, as Waft's
/// `AndroidHardwareBuffer` holds it; on Linux a dmabuf, as Waft's `DmabufBuffer` holds it; on
/// Windows a D3D12 texture's NT handle, as Waft's `SharedTexture` holds it.
#[cfg(any(target_os = "android", target_os = "linux", windows))]
pub use crate::os::HardwareBuffer;

/// A platform whose hardware buffers are not made yet: none can exist, so [`ShellView::present`]
/// cannot be called.
#[cfg(not(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "linux",
    windows
)))]
#[derive(Clone)]
pub enum HardwareBuffer {}

/// A video's frame as the system hands it out: the platform's own picture, and how it is shown,
/// as Waft's `VideoFrame` and WebCodecs' say it. Cloning it is a reference more on the picture.
#[derive(Clone)]
pub struct VideoFrame {
    pub picture: VideoPicture,
    /// The turn applied when it is shown, in degrees clockwise, 0, 90, 180 or 270, before `flip`.
    pub rotation: u32,
    /// Whether it is shown flipped horizontally, after `rotation`.
    pub flip: bool,
}

impl VideoFrame {
    /// `picture`, shown as it is.
    pub fn upright(picture: VideoPicture) -> VideoFrame {
        VideoFrame {
            picture,
            rotation: 0,
            flip: false,
        }
    }
}

/// A video's picture as the system hands it out: the platform's own type. On Apple's systems a
/// `CVPixelBuffer` on an `IOSurface`, in 32-bit BGRA.
#[cfg(target_vendor = "apple")]
pub type VideoPicture = objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer>;

/// A video's picture as the system hands it out: on Android an RGBA8 `AHardwareBuffer`, as Waft's
/// `AndroidVideoFrame` holds it, the frame's own memory, which a reference holds.
#[cfg(target_os = "android")]
pub type VideoPicture = crate::os::HardwareBuffer;

/// A platform whose frames are not handed out yet: none can exist, so a reader never answers one.
#[cfg(not(any(target_vendor = "apple", target_os = "android")))]
#[derive(Clone)]
pub enum VideoPicture {}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::*;

    /// A shell that shows nothing: every request is carried out by doing nothing, and the
    /// screens' refreshes asked for, what it was told of Back and the frames its windows were
    /// given are recorded, none of the refreshes coming.
    #[derive(Default)]
    pub(crate) struct Headless {
        refreshes_asked: Mutex<Vec<ScreenId>>,
        handles_back: Mutex<Vec<bool>>,
        frames_set: Mutex<Vec<Rect>>,
    }

    impl Headless {
        /// The screens whose next refresh was asked for, in order.
        pub(crate) fn refreshes_asked(&self) -> Vec<ScreenId> {
            self.refreshes_asked.lock().unwrap().clone()
        }

        /// Each time it was told whether a view's program handles Back, in order.
        pub(crate) fn handles_back(&self) -> Vec<bool> {
            self.handles_back.lock().unwrap().clone()
        }

        /// The frames a window of its was given, in order.
        pub(crate) fn frames_set(&self) -> Vec<Rect> {
            self.frames_set.lock().unwrap().clone()
        }
    }

    impl VsyncWaiter for Headless {
        fn next_vsync(&self, screen: ScreenId, _on_refresh: OnRefresh) {
            self.refreshes_asked.lock().unwrap().push(screen);
        }
    }

    impl ShellView for Headless {
        fn present(
            &self,
            _hardware_buffer: HardwareBuffer,
            _layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>,
            _released: Box<dyn FnOnce() + Send + Sync>,
        ) {
        }

        fn set_cursor(&self, _cursor: Cursor) {}

        fn request_focus_change(&self, _state: ViewFocusState, _direction: ViewFocusDirection) {}

        fn set_handles_back(&self, handles: bool) {
            self.handles_back.lock().unwrap().push(handles);
        }
    }

    impl ShellWindow for Headless {
        fn set_frame(&self, frame: Rect, _animate: Option<Duration>) {
            self.frames_set.lock().unwrap().push(frame);
        }

        fn set_title(&self, _title: &str) {}

        fn show(&self) {}

        fn hide(&self) {}

        fn close(&self) {}

        fn size_limits(&self) -> Option<&dyn ShellWindowSizeLimits> {
            None
        }

        fn drag(&self) {}

        fn set_maximized(&self, _maximized: bool) {}

        fn set_minimized(&self, _minimized: bool) {}

        fn set_fullscreen(&self, _fullscreen: bool, _screen: Option<ScreenId>) {}
    }
}
