//! Writing one of the package manager's small files whole.

use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

/// Writes `contents` to `path` whole: to a new file beside it, on disk, then renamed over it, so a
/// reader, or a crash, finds the old contents or the new, never part of either.
pub(crate) fn write_whole(path: &Path, contents: &[u8]) -> io::Result<()> {
    let folder = path
        .parent()
        .expect("a file of the package manager is in one of its folders");
    let mut file = NamedTempFile::new_in(folder)?;
    file.write_all(contents)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}
