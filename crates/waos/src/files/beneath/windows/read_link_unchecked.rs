use super::changes_at_folder::read_link_at;
use std::path::{Path, PathBuf};
use std::{fs, io};

/// *Unsandboxed* function similar to `read_link`, but which does not perform
/// sandboxing.
///
/// WaOS: read on `start`'s handle (`changes_at_folder`), not through its path.
pub(crate) fn read_link_unchecked(
    start: &fs::File,
    path: &Path,
    _reuse: PathBuf,
) -> io::Result<PathBuf> {
    read_link_at(start, path)
}
