//! WaOS on Linux ([`LinuxShell`]): an app's windows are WaOS's own, `waos-program-window`'s in
//! WaOS's process, each under the app's identity, its app id, so the desktop groups them
//! as the app (`logic::program_identities`); its context menus and file dialogs open in them, and
//! so do WaOS's prompts for it; its notifications are posted under WaOS's name, and it has no menu
//! bar, Linux desktops having none. Each installed app has its desktop entry, which starts
//! `waos open --program <id>` (`logic::program_entries`), for as long as WaOS runs.
//!
//! The invariant an editor could break: the engine's shell here is the parts every system takes
//! from Waft, unchanged; what Linux does differently is said here, and nowhere else.

use std::sync::Arc;

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use waft::App;
use waos::{Shell, ShellCameras, ShellImageCodecs};

use crate::logic::folders::AppFolders;
use crate::logic::program_entries::ProgramEntries;
use crate::logic::program_entries::desktop_entries::DesktopEntries;
use crate::logic::program_identities::Identities;
use crate::logic::startup::BeforeTheEngine;
use crate::system_shell::{AfterTheEngine, AppPrompts, SharedParts, SystemShell, UserOpen};

/// WaOS on Linux: the parts every system takes from Waft, and the installed apps' desktop
/// entries, which follow the package manager on a thread of their own.
pub(crate) struct LinuxShell {
    shell: Shell,
}

impl SystemShell for LinuxShell {
    const SIMULATED_CAMERA: bool = false;
    const APPS_OPEN_AS_CHOSEN: bool = false;

    /// WaOS on Linux: from now on each app `before`'s package manager installs has its desktop
    /// entry among `folders`; the engine's shell is `shared`'s.
    fn new(
        _app: &mut App,
        folders: &AppFolders,
        before: &BeforeTheEngine<'_>,
        shared: SharedParts,
    ) -> LinuxShell {
        let desktop_entries = DesktopEntries {
            applications: folders.applications.clone(),
        };
        ProgramEntries::follow_installed(Box::new(desktop_entries), folders, before.packages);
        LinuxShell {
            shell: shared.shell,
        }
    }

    fn identities(folders: &AppFolders) -> Option<Identities> {
        Identities::of_this_waos(folders.program_icons.clone())
    }

    /// None: the simulated camera is not built on Linux.
    fn simulated_cameras(
        _codecs: Option<Arc<dyn ShellImageCodecs>>,
    ) -> Option<Arc<dyn ShellCameras>> {
        None
    }

    fn shell(&self) -> Shell {
        self.shell.clone()
    }

    /// None yet: a desktop entry starts `waos open --program <id>`, a later launch of WaOS,
    /// which the library carries out (`state::library`).
    fn opened_by_the_user(&mut self) -> BoxStream<'static, UserOpen> {
        stream::empty().boxed()
    }

    /// The apps' prompts in WaOS's own windows, over the app's.
    fn follow(self, _app: &mut App, after: AfterTheEngine<'_>) -> AppPrompts {
        AppPrompts::in_own_windows(after.prompt_windows)
    }
}
