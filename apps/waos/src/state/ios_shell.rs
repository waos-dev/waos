//! WaOS on iOS ([`IosShell`]): iOS allows WaOS one screen, so each app opens in a tab of WaOS's
//! deck, its file dialogs over a screen, its notifications posted under WaOS's name; nothing lists
//! the installed apps outside WaOS.
//!
//! The invariant an editor could break: the engine's shell here is the parts every system takes
//! from Waft, unchanged; what iOS does differently is said here, and nowhere else.

use std::sync::Arc;

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use waft::App;
use waos::{Shell, ShellCameras, ShellImageCodecs};

use crate::logic::folders::AppFolders;
use crate::logic::program_identities::Identities;
use crate::logic::startup::BeforeTheEngine;
use crate::system_shell::{AfterTheEngine, AppPrompts, SharedParts, SystemShell, UserOpen};

/// WaOS on iOS: the parts every system takes from Waft.
pub(crate) struct IosShell {
    shell: Shell,
}

impl SystemShell for IosShell {
    const SIMULATED_CAMERA: bool = false;
    const APPS_OPEN_AS_CHOSEN: bool = false;

    fn new(
        _app: &mut App,
        _folders: &AppFolders,
        _before: &BeforeTheEngine<'_>,
        shared: SharedParts,
    ) -> IosShell {
        IosShell {
            shell: shared.shell,
        }
    }

    /// None: an app's windows are tabs of WaOS's.
    fn identities(_folders: &AppFolders) -> Option<Identities> {
        None
    }

    /// None: the simulated camera is not built on iOS.
    fn simulated_cameras(
        _codecs: Option<Arc<dyn ShellImageCodecs>>,
    ) -> Option<Arc<dyn ShellCameras>> {
        None
    }

    fn shell(&self) -> Shell {
        self.shell.clone()
    }

    /// None: iOS lists no app of WaOS's, so the user opens one only in WaOS.
    fn opened_by_the_user(&mut self) -> BoxStream<'static, UserOpen> {
        stream::empty().boxed()
    }

    /// The apps' prompts in WaOS's own windows.
    fn follow(self, _app: &mut App, after: AfterTheEngine<'_>) -> AppPrompts {
        AppPrompts::in_own_windows(after.prompt_windows)
    }
}
