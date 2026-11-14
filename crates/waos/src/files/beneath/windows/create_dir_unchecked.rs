use super::changes_at_folder::create_dir_at;
use std::path::Path;
use std::{fs, io};

/// *Unsandboxed* function similar to `create_dir`, but which does not perform
/// sandboxing.
///
/// WaOS: made on `start`'s handle (`changes_at_folder`), not through its path.
pub(crate) fn create_dir_unchecked(start: &fs::File, path: &Path) -> io::Result<()> {
    create_dir_at(start, path)
}
