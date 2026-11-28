//! WaOS on Windows ([`WindowsShell`]): an app's windows are WaOS's own, `waos-program-window`'s in
//! WaOS's process, each under the app's identity, its AppUserModelID, so the taskbar groups them
//! as the app (`logic::program_identities`); its context menus and file dialogs open in them, and
//! so do WaOS's prompts for it; its notifications are posted under WaOS's name, and it has no menu
//! bar, Windows having none. Each installed app has its shortcut in the Start menu, which starts
//! `waos open --program <id>` (`logic::program_entries`), for as long as WaOS runs.
//!
//! The invariant an editor could break: the engine's shell here is the parts every system takes
//! from Waft, unchanged; what Windows does differently is said here, and nowhere else.

use std::sync::Arc;

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use waft::App;
use waos::{Shell, ShellCameras, ShellImageCodecs};

use crate::logic::folders::AppFolders;
use crate::logic::program_entries::ProgramEntries;
use crate::logic::program_entries::start_menu::StartMenu;
use crate::logic::program_identities::Identities;
use crate::logic::startup::BeforeTheEngine;
use crate::system_shell::{AfterTheEngine, AppPrompts, SharedParts, SystemShell, UserOpen};

/// WaOS on Windows: the parts every system takes from Waft, and the installed apps' Start menu
/// shortcuts, which follow the package manager on a thread of their own.
pub(crate) struct WindowsShell {
    shell: Shell,
}

impl SystemShell for WindowsShell {
    const SIMULATED_CAMERA: bool = false;
    const APPS_OPEN_AS_CHOSEN: bool = false;

    /// WaOS on Windows: from now on each app `before`'s package manager installs has its Start
    /// menu shortcut among `folders`; the engine's shell is `shared`'s.
    fn new(
        _app: &mut App,
        folders: &AppFolders,
        before: &BeforeTheEngine<'_>,
        shared: SharedParts,
    ) -> WindowsShell {
        let start_menu = StartMenu {
            folder: folders.start_menu.clone(),
        };
        ProgramEntries::follow_installed(Box::new(start_menu), folders, before.packages);
        WindowsShell {
            shell: shared.shell,
        }
    }

    fn identities(folders: &AppFolders) -> Option<Identities> {
        Identities::of_this_waos(folders.program_icons.clone())
    }

    /// None: the simulated camera is not built on Windows.
    fn simulated_cameras(
        _codecs: Option<Arc<dyn ShellImageCodecs>>,
    ) -> Option<Arc<dyn ShellCameras>> {
        None
    }

    fn shell(&self) -> Shell {
        self.shell.clone()
    }

    /// None yet: a Start menu shortcut starts `waos open --program <id>`, a later launch of WaOS,
    /// which the library carries out (`state::library`).
    fn opened_by_the_user(&mut self) -> BoxStream<'static, UserOpen> {
        stream::empty().boxed()
    }

    /// The apps' prompts in WaOS's own windows, over the app's.
    fn follow(self, _app: &mut App, after: AfterTheEngine<'_>) -> AppPrompts {
        AppPrompts::in_own_windows(after.prompt_windows)
    }
}
