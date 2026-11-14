//! Where the files and folders the user picks for a program are mounted: each at its own path, for
//! the program's run at once, and among its mounts through its grants, which keep it until the
//! user removes it; and what the user picked ahead, the answer set for a dialog among the
//! program's settings.
//!
//! The invariant: a pick is mounted before its path is answered to the program, so the program
//! never has a path it cannot reach; and that path, like the mount kept among its grants, is the
//! pick's path as a program names it (`machine_paths`), never the machine's where they differ.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use waos_capabilities::{FileDialog, Mount, ProgramGrants};

use super::{HandedFiles, machine_paths};

/// Where a program's picks are mounted. Every clone mounts into the same files.
#[derive(Clone)]
pub(crate) struct Picks {
    handed: HandedFiles,
    grants: Arc<dyn ProgramGrants>,
}

impl Picks {
    pub(crate) fn new(handed: HandedFiles, grants: Arc<dyn ProgramGrants>) -> Picks {
        Picks { handed, grants }
    }

    /// Mounts `host`, which the user picked, at its own path, writable as `writable` says, and
    /// answers the path the program sees it at; none, with a warning, for a path a program has no
    /// name for, such as a share's on Windows.
    pub(crate) fn mount(&self, host: &Path, writable: bool) -> Option<String> {
        let Some(own) = machine_paths::in_program(host) else {
            tracing::warn!(host = %host.display(), "a pick has no path a program names; it is not mounted");
            return None;
        };
        self.handed.hand_pick(host, writable);
        self.grants.keep_picked(Mount {
            host: host.to_owned(),
            at: own.clone(),
            writable,
        });
        Some(own.to_string_lossy().into_owned())
    }

    /// The paths on the machine the user set as the answer to `dialog`, as the grants hold them
    /// now; none to show the dialog.
    pub(crate) fn set_answer(&self, dialog: FileDialog) -> Option<Vec<PathBuf>> {
        self.grants.file_dialog_answer(dialog)
    }
}
