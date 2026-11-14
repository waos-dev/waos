use super::changes_at_folder::remove_file_at;
use std::path::Path;
use std::{fs, io};

/// *Unsandboxed* function similar to `remove_file`, but which does not perform
/// sandboxing.
///
/// WaOS: made on `start`'s handle (`changes_at_folder`), not through its path.
pub(crate) fn remove_file_unchecked(start: &fs::File, path: &Path) -> io::Result<()> {
    remove_file_at(start, path)
}
