//! Each app's own folders, `data` and `cache`, which it sees at `/data` and `/cache`, in its
//! folder under `programs/`, beside its installed versions, which are the package manager's: made
//! at its first run, installed or run in development, and removed with the app's folder, whole,
//! when the package manager removes it.

use std::fs;
use std::path::Path;

use waos_capabilities::ProgramFolders;

use crate::store::StoreError;

/// The app's own folders, in its folder: what it keeps, and what it keeps that may be thrown
/// away.
const DATA: &str = "data";
const CACHE: &str = "cache";

/// The app's own folders in `program`, its folder, made where they are not yet.
pub(crate) fn own_folders(program: &Path) -> Result<ProgramFolders, StoreError> {
    let folders = ProgramFolders {
        data: program.join(DATA),
        cache: program.join(CACHE),
    };
    for folder in [&folders.data, &folders.cache] {
        fs::create_dir_all(folder).map_err(StoreError::io_at(folder))?;
    }
    Ok(folders)
}
