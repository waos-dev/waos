//! WaOS's shell on iOS: iOS allows WaOS one screen, so each app opens in a tab of WaOS's deck, its
//! file dialogs over a screen, its notifications posted under WaOS's name; nothing lists the
//! installed apps outside WaOS, so the user opens one only in WaOS's library.
//!
//! The invariant an editor could break: the engine's shell here is the parts every system takes
//! from Waft, unchanged; what iOS does differently is said here, and nowhere else.

use std::sync::Arc;

use waft::App;
use waos::{ShellCameras, ShellImageCodecs};

use crate::logic::folders::AppFolders;
use crate::logic::program_identities::Identities;
use crate::state::shells::{BuiltShell, SharedParts};

/// Whether WaOS's simulated camera is there: not built on iOS.
pub(crate) const SIMULATED_CAMERA: bool = false;

/// Whether each app may have a place of its own beside WaOS's, so where apps open is the user's
/// choice: no, iOS allows WaOS one screen.
pub(crate) const APPS_OPEN_AS_CHOSEN: bool = false;

/// WaOS's shell on iOS: `shared`'s, with the apps' prompts in WaOS's own windows.
pub(crate) fn shell(_app: &mut App, shared: SharedParts<'_>) -> BuiltShell {
    BuiltShell::in_own_windows(shared.shell, shared.prompt_windows)
}

/// None: an app's windows are tabs of WaOS's.
pub(crate) fn app_identities(_folders: &AppFolders) -> Option<Identities> {
    None
}

/// None: the simulated camera is not built on iOS.
pub(crate) fn simulated_cameras(
    _codecs: Option<Arc<dyn ShellImageCodecs>>,
) -> Option<Arc<dyn ShellCameras>> {
    None
}
