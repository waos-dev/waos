//! WaOS on Android ([`AndroidShell`]): each app's windows are WaOS's own, `waos-program-window`'s,
//! each a task of its own with its own Recents card, unless the user chooses tabs inside WaOS, so
//! where apps open is the user's choice; its file dialogs open over a screen, its notifications
//! are posted under WaOS's name, and WaOS's simulated camera is there. Each installed app has its
//! shortcut, on the home screen and on WaOS's icon's menu (`logic::program_shortcuts`), which the
//! library keeps matching what is installed, for as long as WaOS runs.
//!
//! The invariant an editor could break: a tap on a shortcut that opens inside WaOS is heard only
//! once the library opens apps in the deck's tabs, where the tap opens one, so `follow` comes
//! after the deck is made.

use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use waft::{App, Entity};
use waos::{Shell, ShellCameras, ShellImageCodecs};

use super::library::Library;
use crate::logic::folders::AppFolders;
use crate::logic::program_identities::Identities;
use crate::logic::program_shortcuts::SystemShortcuts;
use crate::logic::simulated_cameras::SimulatedCameras;
use crate::logic::startup::BeforeTheEngine;
use crate::system_shell::{AfterTheEngine, AppPrompts, SharedParts, SystemShell, UserOpen};

/// WaOS on Android: the parts every system takes from Waft, and Android's shortcuts of the
/// installed apps, where Waft's Android host gives them.
pub(crate) struct AndroidShell {
    shell: Shell,
    shortcuts: Option<Rc<dyn SystemShortcuts>>,
}

impl SystemShell for AndroidShell {
    const SIMULATED_CAMERA: bool = true;
    const APPS_OPEN_AS_CHOSEN: bool = true;

    /// WaOS on Android: `shared`'s shell, and the shortcuts of `app`'s platform.
    fn new(
        app: &mut App,
        _folders: &AppFolders,
        _before: &BeforeTheEngine<'_>,
        shared: SharedParts,
    ) -> AndroidShell {
        AndroidShell {
            shell: shared.shell,
            shortcuts: crate::os::system_shortcuts(app.platform().as_ref()),
        }
    }

    /// None: Android knows an app's windows only as WaOS's tasks.
    fn identities(_folders: &AppFolders) -> Option<Identities> {
        None
    }

    fn simulated_cameras(
        codecs: Option<Arc<dyn ShellImageCodecs>>,
    ) -> Option<Arc<dyn ShellCameras>> {
        Some(Arc::new(SimulatedCameras::new(codecs)))
    }

    fn shell(&self) -> Shell {
        self.shell.clone()
    }

    /// None yet: a shortcut's tap that opens inside WaOS goes to the library (`follow`), and one
    /// opening an app's own task, or its card brought back from Recents, is a view the platform
    /// gives (`state::given_views`).
    fn opened_by_the_user(&mut self) -> BoxStream<'static, UserOpen> {
        stream::empty().boxed()
    }

    /// From now on, the installed apps' shortcuts are kept through `after`'s library, and each tap
    /// on one that opens inside WaOS opens its app there, or brings it forward where it runs; the
    /// apps' prompts are in WaOS's own windows.
    fn follow(self, app: &mut App, after: AfterTheEngine<'_>) -> AppPrompts {
        if let Some(shortcuts) = self.shortcuts {
            keep_shortcuts(app, after.library, shortcuts);
        }
        AppPrompts::in_own_windows(after.prompt_windows)
    }
}

/// The installed apps' shortcuts kept in `system` through `library`, and each tap on one that
/// opens inside WaOS opening its app there, or bringing it forward where it runs.
fn keep_shortcuts(app: &mut App, library: &Entity<Library>, system: Rc<dyn SystemShortcuts>) {
    let (taps_to, cx) = (library.downgrade(), app.to_async());
    system.hear_taps(Box::new(move |id| {
        let library = taps_to.clone();
        cx.post(move |app| {
            if let Some(library) = library.upgrade() {
                library.update(app, |library, cx| library.show_program(&id, None, cx));
            }
        });
    }));
    library.update(app, |library, cx| library.keep_shortcuts(system, cx));
}
