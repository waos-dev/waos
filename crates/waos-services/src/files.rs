//! Writing a small file whole, as every domain that keeps files does. Not a domain: a helper of
//! the crate's, which holds no state.

use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

/// Writes `contents` to `path` whole: to a new file beside it, on disk, then renamed over it, so a
/// reader, or a crash, finds the old contents or the new, never part of either.
pub(crate) fn write_whole(path: &Path, contents: &[u8]) -> io::Result<()> {
    let folder = path.parent().expect("a file is written in a folder");
    let mut file = NamedTempFile::new_in(folder)?;
    file.write_all(contents)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}
