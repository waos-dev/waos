//! WaOS's shell on Android: each app's windows are WaOS's own, `waos-program-window`'s, each a
//! task of its own with its own Recents card, unless the user chooses tabs inside WaOS, so where
//! apps open is the user's choice; its file dialogs open over a screen, its notifications are
//! posted under WaOS's name, and WaOS's simulated camera is there. Each installed app has its
//! launcher shortcut, on the home screen and on WaOS's icon's menu (`logic::program_shortcuts`),
//! which the library keeps matching what is installed and opens through app management when the
//! user taps one, for as long as WaOS runs.
//!
//! The invariant an editor could break: the shell hands the shortcuts up as it is made, and
//! startup connects them to the library once it opens apps in the deck's tabs, where a tap may
//! open one; nothing here reaches the library.

use std::sync::Arc;

use waft::App;
use waos::{ShellCameras, ShellImageCodecs};

use crate::logic::folders::AppFolders;
use crate::logic::program_identities::Identities;
use crate::logic::simulated_cameras::SimulatedCameras;
use crate::state::shells::{BuiltShell, SharedParts};

/// Whether WaOS's simulated camera is there: yes.
pub(crate) const SIMULATED_CAMERA: bool = true;

/// Whether each app may have a place of its own beside WaOS's, so where apps open is the user's
/// choice: yes, an app's window may be a task of its own.
pub(crate) const APPS_OPEN_AS_CHOSEN: bool = true;

/// WaOS's shell on Android: `shared`'s, with the apps' prompts in WaOS's own windows, and the
/// launcher shortcuts of `app`'s platform, where Waft's Android host gives them.
pub(crate) fn shell(app: &mut App, shared: SharedParts<'_>) -> BuiltShell {
    BuiltShell {
        shortcuts: crate::os::system_shortcuts(app.platform().as_ref()),
        ..BuiltShell::in_own_windows(shared.shell, shared.prompt_windows)
    }
}

/// None: Android knows an app's windows only as WaOS's tasks.
pub(crate) fn app_identities(_folders: &AppFolders) -> Option<Identities> {
    None
}

/// WaOS's simulated cameras, which encode their photos with `codecs`.
pub(crate) fn simulated_cameras(
    codecs: Option<Arc<dyn ShellImageCodecs>>,
) -> Option<Arc<dyn ShellCameras>> {
    Some(Arc::new(SimulatedCameras::new(codecs)))
}
