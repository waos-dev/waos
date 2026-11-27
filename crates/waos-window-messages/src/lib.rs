//! What WaOS and a program's window process say to each other, as values: [`ToWindow`], what the
//! program asks of its windows, and [`FromWindow`], what its windows tell it. A message carries the
//! program's own records, as wasmtime generates them from the WIT (`records`), and the program's
//! calls on a view as the [`ViewCall`]s they are in WaOS's own windows, so the engine's shell calls
//! cross as the values they already are: among them the program's menu bar, whole, and its context
//! menus, which its app shows, its file dialogs, which its app opens and answers with the paths
//! on the machine the user picked, and its notifications, which its app posts under its bundle,
//! a click on one coming back as the program hears it, and macOS's own question for them, which
//! its app puts, telling WaOS whether macOS refuses them as it answers and whenever else it reads
//! it, as when WaOS asks it to read again. WaOS's prompts for the program, each shown
//! in a dialog window of its own in the program's app, cross as the values Waft's host gives a
//! view and takes from it (`prompts`).
//!
//! On macOS the messages travel over Mach (`os::macos`): a WaOS checks in a bootstrap name made of
//! its data folder ([`bootstrap_name`]), a window process looks the name up and says [`Hello`] with
//! a port of its own, and WaOS, once it has checked who sent it, answers [`Welcome`] with a port of
//! the connection's. The protocol's number ([`PROTOCOL`]) is every Mach message's id.
//!
//! The invariant a later editor will break: nothing here is a question WaOS waits on, and a window
//! process waits on one alone. Every message is one-way; a key is answered by a later
//! [`ToWindow::KeyReplied`] naming it, a window request by a later [`FromWindow::WindowOpened`] or
//! [`FromWindow::WindowRefused`] naming the request, a context menu by a later
//! [`FromWindow::ContextMenuClosed`] naming it, a quit by a later [`ToWindow::QuitAnswered`], a
//! file dialog by a later [`FromWindow::FileDialogClosed`] naming it, a notification's post or
//! schedule by a later [`FromWindow::NotificationPosted`] naming it, macOS's question for the
//! program's notifications by a later [`FromWindow::NotificationPermissionAnswered`] naming it, and
//! a reading of macOS's permission for them by a later [`FromWindow::NotificationPermission`]. The
//! one wait is a window's, in a resize: a view's new size sent with a fence is answered by a later
//! [`ToWindow::PictureShown`] of that size, which the window process waits for inside the resize,
//! so a fence comes only with a size the window waits at; but neither side waits while the program
//! awaits the window process's answer to a request WaOS made of it, which the window process gives
//! from the thread that waits: the window's wait ends as such a request arrives, and WaOS lets the
//! fence go as it makes it.

pub mod bundle;
mod prompts;
mod records;
mod view_calls;

#[cfg(target_os = "macos")]
pub mod os;

use std::fmt::{self, Debug};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use waos::bindings::wasdk::platform::file_dialogs::DialogError;
use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos::bindings::wasdk::platform::menus::MenuEntry;
use waos::bindings::wasdk::platform::notifications::{Activation, NotificationError};
use waos::bindings::wasdk::platform::screens::ScreenId;
use waos::bindings::wasdk::platform::views::{
    EditCommand, KeyEvent, Metrics, PointerEvent, RequestId, TextEditingValue, TextInputAction,
    ViewError, ViewFocusEvent,
};
use waos::bindings::wasdk::platform::windows::{Rect, TitleBarAction};
use waos::{
    FolderOptions, Notification, OpenOptions, OpenRequest, Presence, SaveOptions,
    SystemPermissionState, WindowConfig,
};
use waos_package::ProgramId;

pub use prompts::{PromptCall, PromptId, PromptReport};
use records::{
    ActivationDef, AppExitResponseDef, EditCommandDef, FolderOptionsDef, KeyEventDef, MetricsDef,
    NotificationDef, OpenOptionsDef, OpenRequestDef, PointerEventDef, PresenceDef, RectDef,
    SaveOptionsDef, SystemPermissionStateDef, TextEditingValueDef, TextInputActionDef,
    ViewErrorDef, ViewFocusEventDef, WindowConfigDef, menu_entries, option_title_bar_action,
    picked_paths, posted,
};
pub use view_calls::{SystemBarsCall, TextInputCall, ViewCall, WindowCall};

/// The protocol's number, every message's Mach id, which a receiver reads before the message: a
/// change to a message adds at the end of it, and anything else raises this, so a window process
/// and a WaOS of different builds refuse each other, each saying both numbers, rather than misread.
/// The handshake's bytes are pinned to it by a test, which fails when [`Hello`] or [`Welcome`]
/// changes and this does not.
pub const PROTOCOL: u32 = 7;

/// Names one key a window process sent, which WaOS's reply names back.
pub type KeyId = u64;

/// Names one context menu WaOS asked a window process to open, which its answer names back.
pub type ContextMenuId = u64;

/// Names one file dialog WaOS asked a window process to open, which its answer names back.
pub type FileDialogId = u64;

/// Names one notification WaOS asked a window process to post or schedule, which its answer names
/// back.
pub type PostId = u64;

/// Names one question of macOS's for the program's notifications WaOS asked a window process to
/// put, which its answer names back.
pub type PermissionRequestId = u64;

/// WaOS → a program's window process.
#[derive(Debug, Serialize, Deserialize)]
pub enum ToWindow {
    /// WaOS took the window process as its program's; the connection's port comes with it.
    Welcome(Welcome),
    /// WaOS refused the window process, saying why; it ends.
    Refused(String),
    /// The program asked for a window made as `config` says, for its request `window`, which
    /// names the window from then on; the window shows the view's layers, which stay in WaOS,
    /// where `layers` says.
    OpenWindow {
        window: RequestId,
        #[serde(with = "WindowConfigDef")]
        config: WindowConfig,
        layers: HostedLayers,
    },
    /// The program's call on the view in the window made for `window`.
    View { window: RequestId, call: ViewCall },
    /// Whether the program handled the key the window process sent as `key`.
    KeyReplied { key: KeyId, handled: bool },
    /// The program ended: the window process closes its windows and ends.
    ProgramEnded,
    /// Show the prompt `prompt` in a dialog window of its own in the program's app, `size` logical
    /// points large, centred on the screen of the program's front window, which shows what the
    /// layer context `layers` names, drawn in WaOS.
    ShowPrompt {
        prompt: PromptId,
        size: [f64; 2],
        layers: HostedLayers,
    },
    /// WaOS's view of the prompt `prompt` asks its dialog for something.
    Prompt { prompt: PromptId, call: PromptCall },
    /// Take the dialog of the prompt `prompt` away: it was answered or put away.
    HidePrompt { prompt: PromptId },
    /// The program's menu bar, whole, in place of the one before, which the app shows while it
    /// is in front.
    SetMenuBar(#[serde(with = "menu_entries")] Vec<MenuEntry>),
    /// Open `entries` at `at`, in the physical pixels of the view in the window made for
    /// `window`; the window process answers [`FromWindow::ContextMenuClosed`] with `menu` once it
    /// closes.
    ShowContextMenu {
        menu: ContextMenuId,
        window: RequestId,
        at: (f64, f64),
        #[serde(with = "menu_entries")]
        entries: Vec<MenuEntry>,
    },
    /// The program's answer to the user's quitting its app, which the app waits on: exit, and the
    /// program ends with its app; cancel, and both go on.
    QuitAnswered(#[serde(with = "AppExitResponseDef")] AppExitResponse),
    /// Open the file dialog `kind`, a sheet on the window made for `parent` when one is named and
    /// on its own otherwise; the window process answers [`FromWindow::FileDialogClosed`] with
    /// `dialog` once the user has chosen or cancelled.
    OpenFileDialog {
        dialog: FileDialogId,
        parent: Option<RequestId>,
        kind: FileDialog,
    },
    /// Post the program's `notification` under the app's bundle, in place of one of the program's
    /// under the same id: now, or at `at`, the system holding it until then; the window process
    /// answers [`FromWindow::NotificationPosted`] with `post` once the system has it, or why not.
    PostNotification {
        post: PostId,
        #[serde(with = "NotificationDef")]
        notification: Notification,
        at: Option<SystemTime>,
    },
    /// Take the program's notification `id` away, shown or scheduled.
    RemoveNotification { id: String },
    /// Put macOS's own question for the program's notifications, which asks the user only if they
    /// were never asked; the window process answers [`FromWindow::NotificationPermissionAnswered`]
    /// with `request` once macOS has answered.
    RequestNotificationPermission { request: PermissionRequestId },
    /// Read macOS's permission for the program's notifications again, without asking: WaOS came
    /// to the front, the user having perhaps changed it in System Settings, where WaOS's settings
    /// page sent them. The window process tells it as it tells every reading,
    /// [`FromWindow::NotificationPermission`].
    ReadNotificationPermission,
    /// The view in the window made for `window` shows the program's picture at `size`, its width
    /// and height in physical pixels: the view's first picture, after which its window waits in
    /// each resize, or the picture a resize waits for, which WaOS showed with the view's layers
    /// laid out at that size under the fence the new size came with.
    PictureShown { window: RequestId, size: [u32; 2] },
    /// Take the file dialog `dialog` away: WaOS no longer awaits its answer.
    CloseFileDialog { dialog: FileDialogId },
}

/// A program's file dialog of each kind, with what it offers, as the engine's shell takes it.
#[derive(Debug, Serialize, Deserialize)]
pub enum FileDialog {
    OpenFile(#[serde(with = "OpenOptionsDef")] OpenOptions),
    OpenFiles(#[serde(with = "OpenOptionsDef")] OpenOptions),
    OpenFolder(#[serde(with = "FolderOptionsDef")] FolderOptions),
    SaveFile(#[serde(with = "SaveOptionsDef")] SaveOptions),
}

/// Where WaOS keeps a view's layers for its window to show: the program's hardware buffers and its
/// composition layers beneath them, which stay in WaOS. On macOS, the id of the `CAContext` whose
/// root layer holds them, which the window shows in a `CALayerHost`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedLayers {
    pub context_id: u32,
}

/// WaOS's answer to a [`Hello`] it took.
#[derive(Debug, Serialize, Deserialize)]
pub struct Welcome {
    /// The program's name as its user knows it, which a window it left untitled carries.
    pub name: String,
}

/// A program's window process → WaOS.
#[derive(Serialize, Deserialize)]
pub enum FromWindow {
    /// The first message, with the window process's own port.
    Hello(Hello),
    /// The window made for `window` is open, and its view starts as `opened` says.
    WindowOpened {
        window: RequestId,
        opened: OpenedWindow,
    },
    /// No window was made for `window`, and why.
    WindowRefused {
        window: RequestId,
        #[serde(with = "ViewErrorDef")]
        error: ViewError,
    },
    /// What happened to the view in the window made for `window`.
    View {
        window: RequestId,
        report: ViewReport,
    },
    /// A key, while the view in the window made for `window` has the keyboard's focus; WaOS
    /// answers [`ToWindow::KeyReplied`] with `key`.
    Key {
        window: RequestId,
        key: KeyId,
        #[serde(with = "KeyEventDef")]
        event: KeyEvent,
    },
    /// The window made for `window` is gone, and its view with it.
    WindowClosed { window: RequestId },
    /// The system opened the program's app with files or links, after its hello.
    Opened(#[serde(with = "OpenRequestDef")] OpenRequest),
    /// What happened to the dialog of the prompt `prompt`.
    Prompt {
        prompt: PromptId,
        report: PromptReport,
    },
    /// The user chose the program's own item of this id in its menu bar.
    MenuBarChosen(u32),
    /// The context menu `menu` closed, with the id of the item the user chose; none when the user
    /// chose none, or it could not open.
    ContextMenuClosed {
        menu: ContextMenuId,
        chosen: Option<u32>,
    },
    /// The user quit the program's app, from its menu, the Dock or by logging out: WaOS asks the
    /// program, and answers [`ToWindow::QuitAnswered`].
    QuitRequested,
    /// The file dialog `dialog` closed, with the paths on the machine the user picked, none when
    /// the user cancelled; or why it answered nothing. WaOS mounts the paths for the program as
    /// they are: the window process is WaOS's own code, and the program names no path.
    FileDialogClosed {
        dialog: FileDialogId,
        #[serde(with = "picked_paths")]
        picked: Result<Vec<PathBuf>, DialogError>,
    },
    /// The system has the notification the post or schedule `post` asked for, or why not.
    NotificationPosted {
        post: PostId,
        #[serde(with = "posted")]
        posted: Result<(), NotificationError>,
    },
    /// The user clicked one of the program's notifications, or one of its actions, or replied,
    /// while the app ran, after its hello.
    NotificationActivated(#[serde(with = "ActivationDef")] Activation),
    /// What macOS says of the program's notifications, as the app read it: as WaOS welcomed it, as
    /// macOS's question before its first post was answered, each time it comes to the front, and
    /// each time WaOS asks ([`ToWindow::ReadNotificationPermission`]), the user having perhaps
    /// changed it in System Settings meanwhile.
    NotificationPermission {
        #[serde(with = "SystemPermissionStateDef")]
        state: SystemPermissionState,
    },
    /// macOS answered the question `request` asked for: what it says of the program's
    /// notifications now.
    NotificationPermissionAnswered {
        request: PermissionRequestId,
        #[serde(with = "SystemPermissionStateDef")]
        state: SystemPermissionState,
    },
}

/// Which program a window process is, why it was launched, and what it was opened with.
#[derive(Debug, Serialize, Deserialize)]
pub struct Hello {
    pub program: ProgramId,
    pub launched_for: LaunchReason,
    /// The files and links the system opened the program's app with before it said hello; any
    /// later arrive as [`FromWindow::Opened`].
    #[serde(with = "OpenRequestDef")]
    pub opened: OpenRequest,
}

/// Why a program's window process was launched.
#[derive(Serialize, Deserialize)]
pub enum LaunchReason {
    /// WaOS launched it for the program, which runs there.
    ProgramRunning,
    /// The user opened the program's app: from the Dock, the Finder, Spotlight or Launchpad, or
    /// by opening a file or a link with it. WaOS runs the program as its library opens it.
    UserOpened,
    /// The user clicked one of the program's notifications, or one of its actions, or replied,
    /// while the app did not run, and macOS launched the app for it: WaOS runs the program as its
    /// library opens it, and hands it the click.
    NotificationActivated(#[serde(with = "ActivationDef")] Activation),
}

/// A view as its window opens, which WaOS adds to the program.
#[derive(Debug, Serialize, Deserialize)]
pub struct OpenedWindow {
    #[serde(with = "MetricsDef")]
    pub metrics: Metrics,
    /// The screen the view is on; none while on no screen.
    pub screen: Option<ScreenId>,
    #[serde(with = "PresenceDef")]
    pub presence: Presence,
    /// Where the window is on the desktop, in logical points from the top-left of the main
    /// screen, y down.
    #[serde(with = "RectDef")]
    pub frame: Rect,
    /// Whether the window has size limits, which the program's calls on it then set: not where
    /// users do not resize windows. The same while the window is open.
    pub size_limits: bool,
    /// Whether the window is maximized; on macOS, zoomed.
    pub maximized: bool,
    /// Whether the window is full screen.
    pub fullscreen: bool,
    /// What the system's settings say a double-click on a title bar does; none where the system
    /// has no such setting.
    #[serde(with = "option_title_bar_action")]
    pub title_bar_double_click_action: Option<TitleBarAction>,
}

/// What a window reports of its view: a method of the engine's `ProgramView` the shell reports
/// through, as a value, but for a key, which is [`FromWindow::Key`].
#[derive(Serialize, Deserialize)]
pub enum ViewReport {
    /// A pointer event, with the keys the keyboard holds as it happens: each physical key with the
    /// logical key it means, of which the engine keeps the modifier keys.
    Pointer {
        #[serde(with = "PointerEventDef")]
        event: PointerEvent,
        held_keys: Vec<(u64, u64)>,
    },
    Focus(#[serde(with = "ViewFocusEventDef")] ViewFocusEvent),
    TextInputEditingValue(#[serde(with = "TextEditingValueDef")] TextEditingValue),
    TextInputAction(#[serde(with = "TextInputActionDef")] TextInputAction),
    TextInputClosed,
    Copy,
    Cut,
    Paste,
    Edit(#[serde(with = "EditCommandDef")] EditCommand),
    CloseRequested,
    PopRoute,
    /// The view's new metrics. On macOS the message carries, as its one port when the window waits
    /// in the resize for the program's picture at the new size, the fence of the window's
    /// transaction that commits that size: WaOS keeps it until it shows that picture, lays its
    /// layers out at that size with it under the fence, and answers [`ToWindow::PictureShown`], so
    /// the window and what it shows change in one frame. With no fence nothing waits, and WaOS lays
    /// its layers out at once, as it does under the fence when it asks the window process for
    /// something the program awaits the answer to, whose arrival ends the window's wait.
    MetricsChanged(#[serde(with = "MetricsDef")] Metrics),
    ScreenChanged(Option<ScreenId>),
    PresenceChanged(#[serde(with = "PresenceDef")] Presence),
    WindowFrameChanged(#[serde(with = "RectDef")] Rect),
    WindowMaximizedChanged(bool),
    WindowFullscreenChanged(bool),
    TitleBarDoubleClickActionChanged(
        #[serde(with = "option_title_bar_action")] Option<TitleBarAction>,
    ),
}

/// The bootstrap name the WaOS whose data folder is `data_folder` is found by: WaOS's bundle id
/// and the SHA-256 of the folder's path, as Chrome names the port of a user data folder, so two
/// WaOS of different data folders never answer each other's window processes.
pub fn bootstrap_name(data_folder: &Path) -> String {
    let digest = Sha256::digest(data_folder.as_os_str().as_encoded_bytes());
    format!("dev.waos.app.windows.{}", hex::encode(digest))
}

/// `message` as the bytes a transport carries.
pub fn encode(message: &impl Serialize) -> Vec<u8> {
    postcard::to_stdvec(message).expect("a message is written into memory")
}

/// The message `bytes` carry; why not, for bytes of another protocol or none.
pub fn decode<M: DeserializeOwned>(bytes: &[u8]) -> Result<M, postcard::Error> {
    postcard::from_bytes(bytes)
}

/// Written without the text a report or a key carries, which may be a password: messages are
/// traced.
impl Debug for FromWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FromWindow::Hello(hello) => f.debug_tuple("Hello").field(hello).finish(),
            FromWindow::WindowOpened { window, opened } => f
                .debug_struct("WindowOpened")
                .field("window", window)
                .field("opened", opened)
                .finish(),
            FromWindow::WindowRefused { window, error } => f
                .debug_struct("WindowRefused")
                .field("window", window)
                .field("error", error)
                .finish(),
            FromWindow::View { window, report } => f
                .debug_struct("View")
                .field("window", window)
                .field("report", report)
                .finish(),
            FromWindow::Key { window, key, event } => f
                .debug_struct("Key")
                .field("window", window)
                .field("key", key)
                .field("kind", &event.kind)
                .finish_non_exhaustive(),
            FromWindow::WindowClosed { window } => f
                .debug_struct("WindowClosed")
                .field("window", window)
                .finish(),
            FromWindow::Opened(request) => f.debug_tuple("Opened").field(request).finish(),
            FromWindow::Prompt { prompt, report } => f
                .debug_struct("Prompt")
                .field("prompt", prompt)
                .field("report", report)
                .finish(),
            FromWindow::MenuBarChosen(id) => f.debug_tuple("MenuBarChosen").field(id).finish(),
            FromWindow::ContextMenuClosed { menu, chosen } => f
                .debug_struct("ContextMenuClosed")
                .field("menu", menu)
                .field("chosen", chosen)
                .finish(),
            FromWindow::QuitRequested => f.write_str("QuitRequested"),
            FromWindow::FileDialogClosed { dialog, picked } => f
                .debug_struct("FileDialogClosed")
                .field("dialog", dialog)
                .field("picked", picked)
                .finish(),
            FromWindow::NotificationPosted { post, posted } => f
                .debug_struct("NotificationPosted")
                .field("post", post)
                .field("posted", posted)
                .finish(),
            FromWindow::NotificationActivated(activation) => f
                .debug_tuple("NotificationActivated")
                .field(&TracedActivation(activation))
                .finish(),
            FromWindow::NotificationPermission { state } => f
                .debug_struct("NotificationPermission")
                .field("state", state)
                .finish(),
            FromWindow::NotificationPermissionAnswered { request, state } => f
                .debug_struct("NotificationPermissionAnswered")
                .field("request", request)
                .field("state", state)
                .finish(),
        }
    }
}

/// Written without what the user typed in reply, which may be anything.
impl Debug for LaunchReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchReason::ProgramRunning => f.write_str("ProgramRunning"),
            LaunchReason::UserOpened => f.write_str("UserOpened"),
            LaunchReason::NotificationActivated(activation) => f
                .debug_tuple("NotificationActivated")
                .field(&TracedActivation(activation))
                .finish(),
        }
    }
}

/// A click on a notification as a message traces it: its id and the action chosen, without the
/// reply typed.
struct TracedActivation<'a>(&'a Activation);

impl Debug for TracedActivation<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Activation")
            .field("id", &self.0.id)
            .field("action", &self.0.action)
            .finish_non_exhaustive()
    }
}

impl Debug for ViewReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ViewReport::Pointer { event, .. } => f
                .debug_struct("Pointer")
                .field("change", &event.change)
                .finish_non_exhaustive(),
            ViewReport::Focus(event) => f.debug_tuple("Focus").field(event).finish(),
            ViewReport::TextInputEditingValue(value) => f
                .debug_struct("TextInputEditingValue")
                .field("selection", &value.selection)
                .field("composing", &value.composing)
                .finish_non_exhaustive(),
            ViewReport::TextInputAction(action) => {
                f.debug_tuple("TextInputAction").field(action).finish()
            }
            ViewReport::TextInputClosed => f.write_str("TextInputClosed"),
            ViewReport::Copy => f.write_str("Copy"),
            ViewReport::Cut => f.write_str("Cut"),
            ViewReport::Paste => f.write_str("Paste"),
            ViewReport::Edit(command) => f.debug_tuple("Edit").field(command).finish(),
            ViewReport::CloseRequested => f.write_str("CloseRequested"),
            ViewReport::PopRoute => f.write_str("PopRoute"),
            ViewReport::MetricsChanged(metrics) => {
                f.debug_tuple("MetricsChanged").field(metrics).finish()
            }
            ViewReport::ScreenChanged(screen) => {
                f.debug_tuple("ScreenChanged").field(screen).finish()
            }
            ViewReport::PresenceChanged(presence) => {
                f.debug_tuple("PresenceChanged").field(presence).finish()
            }
            ViewReport::WindowFrameChanged(frame) => {
                f.debug_tuple("WindowFrameChanged").field(frame).finish()
            }
            ViewReport::WindowMaximizedChanged(maximized) => f
                .debug_tuple("WindowMaximizedChanged")
                .field(maximized)
                .finish(),
            ViewReport::WindowFullscreenChanged(fullscreen) => f
                .debug_tuple("WindowFullscreenChanged")
                .field(fullscreen)
                .finish(),
            ViewReport::TitleBarDoubleClickActionChanged(action) => f
                .debug_tuple("TitleBarDoubleClickActionChanged")
                .field(action)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests;
