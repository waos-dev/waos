//! Where the files and folders the user picks for a program, or opens it with, are mounted: each
//! at its own path, kept among its grants, which mount it until the user removes it there, or,
//! where the grants keep nothing, mounted for the program's run alone; each pick told through the
//! program's access (`access`); and what the user picked ahead, the answer set for a dialog among
//! the program's settings.
//!
//! The invariant: a pick is mounted in one place, the grants where they keep it, so taking it back
//! there unmounts it at the program's next call; and it is mounted before its path is answered to
//! the program, so the program never has a path it cannot reach. That path, like the mount kept
//! among its grants, is the pick's path as a program names it (`machine_paths`), never the
//! machine's where they differ.

use std::path::Path;
use std::sync::Arc;

use waos_capabilities::{Mount, ProgramGrants};

use super::{HandedFiles, machine_paths};
use crate::access::{Happened, ProgramAccess};

/// Where a program's picks are mounted. Every clone mounts into the same files.
#[derive(Clone)]
pub(crate) struct Picks {
    handed: HandedFiles,
    /// Where each pick is told and kept.
    access: ProgramAccess,
}

impl Picks {
    pub(crate) fn new(handed: HandedFiles, access: ProgramAccess) -> Picks {
        Picks { handed, access }
    }

    /// Mounts `host`, which the user picked, at its own path, writable as `writable` says: among
    /// the grants where they keep it, for the run otherwise. Answers the path the program sees it
    /// at; none, with a warning, for a path a program has no name for, such as a share's on
    /// Windows. Whether the grants keep it is read back from them once they are asked to.
    pub(crate) fn mount(&self, host: &Path, writable: bool) -> Option<String> {
        let Some(own) = machine_paths::in_program(host) else {
            tracing::warn!(host = %host.display(), "a pick has no path a program names; it is not mounted");
            return None;
        };
        let pick = Mount {
            host: host.to_owned(),
            at: own,
            writable,
        };
        let grants = self.access.grants();
        grants.keep_picked(pick.clone());
        let kept = grants.file_system().mounts.contains(&pick);
        self.handed.hand_pick(&pick, kept);
        self.access.tell(Happened::PickMounted { on_machine: host });
        Some(pick.at.to_string_lossy().into_owned())
    }

    /// A shared handle on the program's grants, for waiting dialogs to re-check stored answers.
    pub(crate) fn grants(&self) -> Arc<dyn ProgramGrants> {
        self.access.clone_grants()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::access::Told;
    use crate::permissions::NothingGranted;

    #[test]
    fn a_pick_mounted_is_told_where_it_is_on_the_machine() {
        let access = ProgramAccess::new(Arc::new(NothingGranted));
        let picks = Picks::new(HandedFiles::default(), access.clone());
        let host = std::env::temp_dir();
        assert!(picks.mount(&host, false).is_some());
        assert_eq!(
            access.told(),
            [Told::Happened(format!(
                "PickMounted {{ on_machine: {host:?} }}"
            ))]
        );
    }
}
