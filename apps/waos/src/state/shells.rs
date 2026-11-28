//! Each system's shell: how WaOS shows apps there. The contract is the engine's `Shell`; each
//! system's module builds it from that system's owners with one function, `shell`, and says what
//! is that system's own (whether it has WaOS's simulated camera, whether apps open where the user
//! chose, what identity an app's windows carry). Every system starts from the parts it takes from
//! Waft and WaOS's own ([`SharedParts`]) and fills in only what it does differently. A shell is
//! handed what it needs when it is made, app management among it, so it opens what the user opens
//! through it by asking app management, then brings the app forward; what it must tell WaOS later
//! goes up through a channel startup made first, and startup connects.
//!
//! The invariant an editor could break: [`this_system`] is the only choice of a system's module,
//! and startup the only code that builds a shell, so no other shared code names a system.

use std::rc::Rc;
use std::sync::Arc;

use waos::Shell;
use waos_program_window::{PromptPlacement, PromptWindows};
use waos_services::packages::PackageManager;

use crate::logic::app_management::AppManagement;
use crate::logic::asking_now::AskingNow;
use crate::logic::folders::AppFolders;
use crate::logic::notification_permissions::NotificationPermissions;
use crate::logic::program_places::{ProgramPlaces, TabsOnRequest};
use crate::logic::program_shortcuts::SystemShortcuts;
use crate::ui::every_window::ViewContent;

#[cfg(target_os = "android")]
pub(crate) use crate::state::android_shell as this_system;
#[cfg(target_os = "ios")]
pub(crate) use crate::state::ios_shell as this_system;
#[cfg(target_os = "linux")]
pub(crate) use crate::state::linux_shell as this_system;
/// The module of the system WaOS is built for.
#[cfg(target_os = "macos")]
pub(crate) use crate::state::macos_shell as this_system;
#[cfg(windows)]
pub(crate) use crate::state::windows_shell as this_system;

/// What every system's shell is built from: the parts every system takes from Waft and WaOS's
/// own, and what the shell is handed to keep.
#[allow(
    dead_code,
    reason = "each system's shell reads what its system needs of these"
)]
pub(crate) struct SharedParts<'a> {
    /// The engine's shell as every system has it: the apps' views in WaOS's own windows or in
    /// tabs, as each was opened, Waft's parts, and this system's simulated cameras.
    pub(crate) shell: Shell,
    /// Where each app's views go, a tab or windows.
    pub(crate) places: ProgramPlaces,
    /// Where the apps' views in tabs go.
    pub(crate) tabs: TabsOnRequest,
    /// What the apps' users are asked now, a file dialog among it.
    pub(crate) asking: Arc<AskingNow>,
    /// Where WaOS keeps its files, the system's own among them.
    pub(crate) folders: &'a AppFolders,
    /// What is installed, which the system lists.
    pub(crate) packages: Arc<PackageManager>,
    /// What opens an app the user opens through the system, and tells which apps run.
    pub(crate) app_management: Arc<dyn AppManagement>,
    /// The apps' prompts in WaOS's own windows.
    pub(crate) prompt_windows: Rc<PromptWindows>,
    /// Where the shell tells what the system says of each app's notifications.
    pub(crate) notification_permissions: NotificationPermissions,
}

/// A system's shell, as its module builds it: the engine's `Shell`, where the apps' prompts are
/// shown, and the system's shortcuts of the installed apps, where it has them, which the library
/// keeps matching what is installed and whose taps it opens.
pub(crate) struct BuiltShell {
    pub(crate) shell: Shell,
    pub(crate) prompts: AppPrompts,
    pub(crate) shortcuts: Option<Rc<dyn SystemShortcuts>>,
}

impl BuiltShell {
    /// `shell`, the apps' prompts in `prompt_windows`, and no shortcuts.
    pub(crate) fn in_own_windows(shell: Shell, prompt_windows: Rc<PromptWindows>) -> BuiltShell {
        BuiltShell {
            shell,
            prompts: AppPrompts::in_own_windows(prompt_windows),
            shortcuts: None,
        }
    }
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
