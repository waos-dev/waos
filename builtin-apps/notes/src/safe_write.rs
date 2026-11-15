//! Writing a file safely, as `file-dialogs` asks: whole, into a draft first, then renamed onto the
//! file, which replaces it at once, so the file is never left half written and a write that fails
//! leaves it as it was.
//!
//! The invariant a later editor will break: the draft is written in a folder on the file's volume
//! that the program may create files in, never beside a file the user picked, whose folder WaOS
//! does not mount.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Writes `contents` to a draft in `drafts`, then renames it onto `path`; the draft goes when the
/// rename fails.
pub(crate) fn write_safely(path: &Path, contents: &str, drafts: &Path) -> io::Result<()> {
    let draft = drafts.join(draft_name(path));
    fs::write(&draft, contents)?;
    fs::rename(&draft, path).inspect_err(|_| {
        // Nothing else of the program's reads the draft; one left behind is only clutter.
        let _ = fs::remove_file(&draft);
    })
}

/// The program's temporary folder, in its `/cache`, as `TMPDIR` names it: Rust's
/// `std::env::temp_dir` panics on `wasm32-wasip3`.
pub(crate) fn temporary_folder() -> PathBuf {
    std::env::var_os("TMPDIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

/// The name a draft of `path` is written under.
fn draft_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    format!("{name}.saving")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_replaced_whole_and_its_draft_goes() {
        let folder = tempfile::tempdir().unwrap();
        let (path, drafts) = (folder.path().join("shopping.txt"), folder.path());
        fs::write(&path, "eggs").unwrap();

        write_safely(&path, "eggs, milk", drafts).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "eggs, milk");
        assert!(!drafts.join("shopping.txt.saving").exists());
    }

    #[test]
    fn a_write_that_cannot_be_renamed_leaves_the_file_as_it_was() {
        let folder = tempfile::tempdir().unwrap();
        let missing = folder.path().join("gone").join("shopping.txt");

        assert!(write_safely(&missing, "eggs", folder.path()).is_err());
        assert_eq!(
            fs::read_dir(folder.path()).unwrap().count(),
            0,
            "no draft left"
        );
    }
}
