//! Writing a file of the program's own safely: whole, into a draft beside it first, then renamed
//! onto it, which replaces it at once, so a file is never left half written and a write that
//! fails leaves it as it was.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Writes `contents` to a draft beside `path`, then renames it onto `path`; the draft goes when
/// the rename fails.
pub(crate) fn write_safely(path: &Path, contents: &[u8]) -> io::Result<()> {
    let draft = draft_of(path);
    fs::write(&draft, contents)?;
    fs::rename(&draft, path).inspect_err(|_| {
        // Nothing reads the draft; one left behind is only clutter.
        let _ = fs::remove_file(&draft);
    })
}

/// Where the draft of `path` is written.
fn draft_of(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".saving");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_replaced_whole_and_its_draft_goes() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("feeds.json");
        fs::write(&path, "[]").unwrap();

        write_safely(&path, b"[1]").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "[1]");
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 1, "no draft");
    }

    #[test]
    fn a_write_that_fails_leaves_no_draft() {
        let folder = tempfile::tempdir().unwrap();
        let missing = folder.path().join("gone").join("feeds.json");
        assert!(write_safely(&missing, b"[]").is_err());
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 0);
    }
}
