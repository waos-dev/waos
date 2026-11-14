use super::changes_at_folder::rename_at;
use std::path::Path;
use std::{fs, io};

/// *Unsandboxed* function similar to `rename`, but which does not perform
/// sandboxing.
///
/// WaOS: made on the folders' handles (`changes_at_folder`), not through their paths.
pub(crate) fn rename_unchecked(
    old_start: &fs::File,
    old_path: &Path,
    new_start: &fs::File,
    new_path: &Path,
) -> io::Result<()> {
    rename_at(old_start, old_path, new_start, new_path)
}
