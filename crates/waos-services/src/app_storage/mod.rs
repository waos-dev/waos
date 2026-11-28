//! App storage: each app's own folders, `data` and `cache`, which it sees at `/data` and `/cache`
//! ([`AppStorage`]), kept in its folder under `programs/` in WaOS's store directory, beside its
//! installed versions, which are the package manager's.
//!
//! The invariant a later editor will break: an app's own folders are made at its first run,
//! installed or run in development, and an install of its id keeps them; they go only with the
//! app's folder, which the package manager removes whole, so nothing else removes what an app
//! kept.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use waos_capabilities::ProgramFolders;
use waos_package::ProgramId;

/// The folder of the apps' folders, in the directory.
const PROGRAMS: &str = "programs";

/// The app's own folders, in its folder: what it keeps, and what it keeps that may be thrown
/// away.
const DATA: &str = "data";
const CACHE: &str = "cache";

/// Each app's own folders, in WaOS's store directory. Clones are the same.
#[derive(Clone)]
pub struct AppStorage {
    programs: PathBuf,
}

impl AppStorage {
    /// The apps' own folders in `directory`, WaOS's store directory.
    pub fn new(directory: &Path) -> AppStorage {
        AppStorage {
            programs: directory.join(PROGRAMS),
        }
    }

    /// The app `id`'s own folders, made where they are not yet: at its first run.
    pub fn folders(&self, id: &ProgramId) -> io::Result<ProgramFolders> {
        let program = self.programs.join(id.as_str());
        let folders = ProgramFolders {
            data: program.join(DATA),
            cache: program.join(CACHE),
        };
        for folder in [&folders.data, &folders.cache] {
            fs::create_dir_all(folder).map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", folder.display()))
            })?;
        }
        Ok(folders)
    }

    /// Whether the app `id` has a folder its own folders are kept in: it is installed, or ran.
    pub fn keeps_folders_of(&self, id: &ProgramId) -> bool {
        self.programs.join(id.as_str()).is_dir()
    }
}
