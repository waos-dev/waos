//! WaOS's shell on Linux: an app's windows are WaOS's own, `waos-program-window`'s in WaOS's
//! process, each under the app's identity, its app id, so the desktop groups them as the
//! app (`logic::program_identities`); its context menus and file dialogs open in them, and so do
//! WaOS's prompts for it; its notifications are posted under WaOS's name, and it has no menu bar,
//! Linux desktops having none. Each installed app, not a tool, has its desktop entry, which
//! starts `waos open --program <id>` (`logic::program_entries`), a later launch of WaOS that the
//! library opens through app management, for as long as WaOS runs.
//!
//! The invariant an editor could break: the engine's shell here is the parts every system takes
//! from Waft, unchanged; what Linux does differently is said here, and nowhere else.

use std::sync::Arc;

use waft::App;
use waos::{ShellCameras, ShellImageCodecs};

use crate::logic::folders::AppFolders;
use crate::logic::program_entries::ProgramEntries;
use crate::logic::program_entries::desktop_entries::DesktopEntries;
use crate::logic::program_identities::Identities;
use crate::state::shells::{BuiltShell, SharedParts};

/// Whether WaOS's simulated camera is there: not built on Linux.
pub(crate) const SIMULATED_CAMERA: bool = false;

/// Whether each app may have a place of its own beside WaOS's, so where apps open is the user's
/// choice: no, an app's windows are WaOS's own.
pub(crate) const APPS_OPEN_AS_CHOSEN: bool = false;

/// WaOS's shell on Linux: from now on each app `shared`'s package manager installs has its
/// desktop entry among `shared`'s folders; the engine's shell is `shared`'s, with the apps'
/// prompts in WaOS's own windows, over the app's.
pub(crate) fn shell(_app: &mut App, shared: SharedParts<'_>) -> BuiltShell {
    let desktop_entries = DesktopEntries {
        applications: shared.folders.applications.clone(),
    };
    ProgramEntries::follow_installed(Box::new(desktop_entries), shared.folders, &shared.packages);
    BuiltShell::in_own_windows(shared.shell, shared.prompt_windows)
}

/// What each app's windows carry as the app's identity, their icons among `folders`.
pub(crate) fn app_identities(folders: &AppFolders) -> Option<Identities> {
    Identities::of_this_waos(folders.program_icons.clone())
}

/// None: the simulated camera is not built on Linux.
pub(crate) fn simulated_cameras(
    _codecs: Option<Arc<dyn ShellImageCodecs>>,
) -> Option<Arc<dyn ShellCameras>> {
    None
}
