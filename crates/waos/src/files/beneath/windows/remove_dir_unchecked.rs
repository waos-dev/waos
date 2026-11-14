use super::changes_at_folder::remove_dir_at;
use std::path::Path;
use std::{fs, io};

/// *Unsandboxed* function similar to `remove_dir`, but which does not perform
/// sandboxing.
///
/// WaOS: made on `start`'s handle (`changes_at_folder`), not through its path.
pub(crate) fn remove_dir_unchecked(start: &fs::File, path: &Path) -> io::Result<()> {
    remove_dir_at(start, path)
}
