//! WaOS on one system: the one place that says what WaOS does there. The engine shows apps
//! through the system's shell (showing flows down), and what the user does through the system,
//! opening an app from the Dock or the Start menu, comes up as one stream, which startup connects
//! to what opens apps, so the system and the library open apps in the same place (what the user
//! did flows up). A system's shell never opens apps itself. Each system's shell fills in only what
//! its system does differently from the parts every system takes from Waft ([`SharedParts`]).
//!
//! The invariant an editor could break: [`ThisSystemShell`] is the only choice of a system's
//! shell, and startup the only code that makes one, so no other shared code names a system.

use std::rc::Rc;
use std::sync::Arc;

use futures::channel::oneshot;
use futures::stream::BoxStream;
use waft::{App, Entity};
use waos::{AppInstance, Shell, ShellCameras, ShellImageCodecs};
use waos_package::ProgramId;
use waos_program_window::{PromptPlacement, PromptWindows};
use waos_services::packages::PackageManager;

use crate::logic::app_instances::AppInstances;
use crate::logic::asking_now::AskingNow;
use crate::logic::folders::AppFolders;
use crate::logic::program_identities::Identities;
use crate::logic::program_places::TabsOnRequest;
use crate::logic::startup::BeforeTheEngine;
use crate::state::library::Library;
use crate::ui::every_window::ViewContent;

/// The shell of the system WaOS is built for.
#[cfg(target_os = "macos")]
pub(crate) type ThisSystemShell = crate::state::macos_shell::MacosShell;
#[cfg(windows)]
pub(crate) type ThisSystemShell = crate::state::windows_shell::WindowsShell;
#[cfg(target_os = "linux")]
pub(crate) type ThisSystemShell = crate::state::linux_shell::LinuxShell;
#[cfg(target_os = "android")]
pub(crate) type ThisSystemShell = crate::state::android_shell::AndroidShell;
#[cfg(target_os = "ios")]
pub(crate) type ThisSystemShell = crate::state::ios_shell::IosShell;

/// Whether this system has WaOS's simulated camera; elsewhere an app answered Simulated lists no
/// camera, and its settings page says so.
pub(crate) const SIMULATED_CAMERA: bool = <ThisSystemShell as SystemShell>::SIMULATED_CAMERA;

/// Whether each app may have a place of its own beside WaOS's on this system, as Android gives an
/// app's window a task, so where apps open is the user's choice.
pub(crate) const APPS_OPEN_AS_CHOSEN: bool = <ThisSystemShell as SystemShell>::APPS_OPEN_AS_CHOSEN;

/// WaOS on one system: how it shows apps, lists them, and hears what the user does there.
pub(crate) trait SystemShell: Sized {
    /// Whether this system has WaOS's simulated camera, where the engine takes a camera's frames.
    const SIMULATED_CAMERA: bool;
    /// Whether each app may have a place of its own beside WaOS's, so where apps open is the
    /// user's choice.
    const APPS_OPEN_AS_CHOSEN: bool;

    /// WaOS on this system, made from the domains `before` the engine and the parts every system
    /// takes from Waft, `shared`, its own files among `folders`; it keeps on `app`'s main thread
    /// what must be kept there.
    fn new(
        app: &mut App,
        folders: &AppFolders,
        before: &BeforeTheEngine<'_>,
        shared: SharedParts,
    ) -> Self;
    /// WaOS's simulated cameras, which encode their photos with `codecs`, where this system has
    /// them ([`SystemShell::SIMULATED_CAMERA`]).
    fn simulated_cameras(
        codecs: Option<Arc<dyn ShellImageCodecs>>,
    ) -> Option<Arc<dyn ShellCameras>>;
    /// What the apps' identities are made of, their icons among `folders`, where each app's
    /// windows are an app of its own to the system; none elsewhere.
    fn identities(folders: &AppFolders) -> Option<Identities>;
    /// The parts the engine shows apps through: the engine's `Shell`, unchanged.
    fn shell(&self) -> Shell;
    /// The apps the user opened through the system: the Dock, the Start menu, a phone's shortcut.
    fn opened_by_the_user(&mut self) -> BoxStream<'static, UserOpen>;
    /// From now on, what this system's shell does with the parts of WaOS made after the engine,
    /// `after`; answers where the apps' prompts are shown.
    fn follow(self, app: &mut App, after: AfterTheEngine<'_>) -> AppPrompts;
}

/// The parts every system takes from Waft and WaOS's own, made before any system's shell.
#[allow(
    dead_code,
    reason = "each system's shell reads what its system needs of these"
)]
pub(crate) struct SharedParts {
    /// The engine's shell as every system has it: the apps' views in WaOS's own windows or in
    /// tabs, as each was opened, Waft's parts, and this system's simulated cameras.
    pub(crate) shell: Shell,
    /// Where the apps' views in tabs go.
    pub(crate) tabs: TabsOnRequest,
    /// What the apps' users are asked now, a file dialog among it.
    pub(crate) asking: Arc<AskingNow>,
}

/// The parts of WaOS made after the engine that a system's shell follows.
#[allow(
    dead_code,
    reason = "each system's shell reads what its system needs of these"
)]
pub(crate) struct AfterTheEngine<'a> {
    pub(crate) library: &'a Entity<Library>,
    pub(crate) instances: &'a Arc<AppInstances>,
    pub(crate) packages: &'a Arc<PackageManager>,
    /// The apps' prompts in WaOS's own windows.
    pub(crate) prompt_windows: Rc<PromptWindows>,
}

/// Where the apps' prompts are shown, and what goes in the views of WaOS's own that showing them
/// adds, where it adds any.
pub(crate) struct AppPrompts {
    pub(crate) placement: Rc<dyn PromptPlacement>,
    pub(crate) views: Option<ViewContent>,
}

impl AppPrompts {
    /// The apps' prompts in WaOS's own windows, by `prompt_windows`, which add no views of their
    /// own.
    pub(crate) fn in_own_windows(prompt_windows: Rc<PromptWindows>) -> AppPrompts {
        AppPrompts {
            placement: prompt_windows,
            views: None,
        }
    }
}

/// An app the user opened through the system, and where what opening it answers goes: the run
/// opened, which the system then shows as the app the user opened, or why it could not be opened.
/// An open nobody answers is refused as one WaOS could not open.
pub(crate) struct UserOpen {
    pub(crate) app: ProgramId,
    pub(crate) opened: oneshot::Sender<Result<AppInstance, String>>,
}
