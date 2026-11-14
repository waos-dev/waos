//! A file dialog answered as its user set it among the program's settings, in place of showing it:
//! the paths set, checked as the dialog would have let the user choose them. A path that does not
//! hold now is the dialog's failure, with why, never another path and never the dialog shown: the
//! settings say not to ask.
//!
//! The invariant: what passes here is only what the user could have chosen in that dialog: files
//! that exist to open, a folder that exists, and a file to save in a folder that exists.

use std::path::{Path, PathBuf};

use waos_capabilities::FileDialog;

use crate::bindings::wasdk::platform::file_dialogs::DialogError;

/// `paths`, set as the answer to `dialog`, if each holds now for it; else why not.
pub(super) fn checked(
    dialog: FileDialog,
    paths: Vec<PathBuf>,
) -> Result<Vec<PathBuf>, DialogError> {
    for path in &paths {
        if let Some(why) = refusal(dialog, path) {
            tracing::warn!(?dialog, path = %path.display(), why, "the answer set for a dialog does not hold");
            return Err(DialogError::Failed(format!(
                "WaOS answers this dialog with {}, as the program's settings say, but it {why}",
                path.display()
            )));
        }
    }
    Ok(paths)
}

/// Why `path` cannot answer `dialog` now, in words that follow "it": none when it can. What a
/// settings page checks a path against before it keeps it as the answer.
pub fn refusal(dialog: FileDialog, path: &Path) -> Option<&'static str> {
    if !path.is_absolute() {
        return Some("is not a whole path");
    }
    match dialog {
        FileDialog::OpenFile | FileDialog::OpenFiles if !path.exists() => Some("does not exist"),
        FileDialog::OpenFile | FileDialog::OpenFiles if path.is_dir() => Some("is a folder"),
        FileDialog::OpenFolder if !path.is_dir() => Some("is not a folder"),
        FileDialog::SaveFile if path.is_dir() => Some("is a folder"),
        FileDialog::SaveFile if !path.parent().is_some_and(Path::is_dir) => {
            Some("is in a folder that does not exist")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_set_path_answers_only_the_dialog_that_could_have_chosen_it() {
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("model.glb");
        fs::write(&file, "glTF").unwrap();
        let folder = scratch.path().to_path_buf();
        let missing = scratch.path().join("missing.txt");
        let unsaved = scratch.path().join("new.pdf");
        let holds = |dialog, path: &Path| checked(dialog, vec![path.to_path_buf()]).is_ok();

        assert!(holds(FileDialog::OpenFile, &file));
        assert!(holds(FileDialog::OpenFiles, &file));
        assert!(holds(FileDialog::OpenFolder, &folder));
        assert!(
            holds(FileDialog::SaveFile, &unsaved),
            "a file to save need not exist"
        );
        assert!(
            holds(FileDialog::SaveFile, &file),
            "an existing one is replaced"
        );

        assert!(!holds(FileDialog::OpenFile, &missing));
        assert!(!holds(FileDialog::OpenFile, &folder));
        assert!(!holds(FileDialog::OpenFolder, &file));
        assert!(!holds(FileDialog::SaveFile, &folder));
        assert!(!holds(FileDialog::SaveFile, &missing.join("out.pdf")));
        assert!(!holds(FileDialog::OpenFile, Path::new("model.glb")));
    }

    #[test]
    fn one_path_that_does_not_hold_fails_the_whole_answer_and_says_why() {
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("a.txt");
        fs::write(&file, "a").unwrap();
        let missing = scratch.path().join("b.txt");
        let Err(DialogError::Failed(why)) =
            checked(FileDialog::OpenFiles, vec![file, missing.clone()])
        else {
            panic!("a missing file answered");
        };
        assert!(why.contains(&missing.display().to_string()), "{why}");
        assert!(why.ends_with("does not exist"), "{why}");
    }
}
