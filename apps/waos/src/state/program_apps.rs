//! Where WaOS's prompts for the apps are shown, on every system ([`ProgramPrompts`]); and,
//! everywhere but macOS, each installed app's entry where the system lists its apps, on Windows
//! and Linux, following the package manager. On macOS an app's windows, menus, dialogs,
//! notifications and prompts are its window process's (`state::macos_shell`); elsewhere an app's
//! windows are WaOS's own, `waos-program-window`'s in WaOS's process, its context menus and file
//! dialogs open in them, and so are its prompts', `PromptWindows`; its notifications are WaOS's,
//! posted under WaOS's name; an app has no menu bar there, those desktops having none.
//!
//! The invariant a later editor will break: an entry names only an app the package manager has
//! installed, matched again at each change it announces.

#[cfg(not(target_os = "macos"))]
pub(crate) use other::*;

use std::rc::Rc;

use waos_program_window::PromptPlacement;

use crate::ui::every_window::ViewContent;

/// Where the programs' prompts are shown, and what goes in the views of WaOS's own that showing
/// them adds, where it adds any.
pub(crate) struct ProgramPrompts {
    pub(crate) placement: Rc<dyn PromptPlacement>,
    pub(crate) views: Option<ViewContent>,
}

#[cfg(not(target_os = "macos"))]
mod other {
    use std::rc::Rc;
    use std::sync::Arc;

    use waos_program_window::PromptWindows;
    use waos_services::packages::PackageManager;

    use super::ProgramPrompts;
    use crate::logic::folders::AppFolders;
    #[cfg(any(windows, target_os = "linux"))]
    use crate::logic::program_entries::ProgramEntries;
    #[cfg(any(windows, target_os = "linux"))]
    use crate::logic::program_identities::Identities;

    /// No program has an app of its own here, beyond its windows' identity; each installed one's
    /// entry where the system lists its apps, on Windows and Linux, over the store.
    pub(crate) struct Started {
        entries: Option<KeptEntries>,
    }

    /// The installed programs' entries, and the store they follow.
    struct KeptEntries {
        #[cfg(any(windows, target_os = "linux"))]
        entries: ProgramEntries,
        #[cfg(any(windows, target_os = "linux"))]
        packages: Arc<PackageManager>,
    }

    /// The installed programs' entries over `packages`, among `folders`, where the system lists apps
    /// by entries WaOS writes.
    pub(crate) fn start(folders: &AppFolders, packages: &Arc<PackageManager>) -> Started {
        Started {
            entries: kept_entries(folders, packages),
        }
    }

    #[cfg(any(windows, target_os = "linux"))]
    fn kept_entries(folders: &AppFolders, packages: &Arc<PackageManager>) -> Option<KeptEntries> {
        let identities = Identities::of_this_waos(folders.program_icons.clone())?;
        Some(KeptEntries {
            entries: ProgramEntries::of_this_system(folders, identities),
            packages: Arc::clone(packages),
        })
    }

    /// None: Android lists an app by its package, which WaOS's programs have none of.
    #[cfg(not(any(windows, target_os = "linux")))]
    fn kept_entries(_folders: &AppFolders, _packages: &Arc<PackageManager>) -> Option<KeptEntries> {
        None
    }

    /// From now on, the installed programs' entries in `kept` match the package manager's record,
    /// now and at each change it announces.
    #[cfg(any(windows, target_os = "linux"))]
    fn follow_installed(kept: KeptEntries) {
        if let Err(error) = kept.entries.follow(kept.packages) {
            tracing::warn!(%error, "the programs' entries do not follow the store");
        }
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    fn follow_installed(_kept: KeptEntries) {}

    /// From now on, the installed programs' entries match what the package manager has installed; answers
    /// where the programs' prompts are shown: in WaOS's own windows, by `own_windows`.
    pub(crate) fn follow(started: Started, own_windows: Rc<PromptWindows>) -> ProgramPrompts {
        if let Some(kept) = started.entries {
            follow_installed(kept);
        }
        ProgramPrompts {
            placement: own_windows,
            views: None,
        }
    }
}
