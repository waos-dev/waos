//! A program's file dialogs in Waft's terms: what a dialog offers, the engine's options, as
//! Waft's, and what Waft's dialog answers, as the paths on the machine the user picked, which the
//! engine mounts.
//!
//! The invariant: a pick is answered by its path on the machine or not at all. A pick the system
//! gives without a path, a handle or a stream, which the desktop systems never give, fails the
//! dialog, since the engine mounts paths alone.

use std::path::PathBuf;
use std::pin::Pin;

use waft::ui::{self, FileDialogError, FileDialogFuture, PickedFile, PickedFolder};
use waos::bindings::wasdk::platform::file_dialogs::DialogError;
use waos::{FileType, FolderOptions, OpenOptions, SaveOptions};

/// What a dialog answers in the program's terms, once the user has chosen or cancelled: the paths
/// picked, none when the user cancelled.
pub type PickedPaths = Pin<Box<dyn Future<Output = Result<Vec<PathBuf>, DialogError>>>>;

pub fn waft_open_options(options: OpenOptions) -> ui::OpenOptions {
    ui::OpenOptions {
        types: options.types.into_iter().map(waft_file_type).collect(),
        initial_folder: options.initial_folder,
        confirm_label: options.confirm_label,
        writable: options.writable,
    }
}

pub fn waft_folder_options(options: FolderOptions) -> ui::FolderOptions {
    ui::FolderOptions {
        initial_folder: options.initial_folder,
        confirm_label: options.confirm_label,
        writable: options.writable,
    }
}

pub fn waft_save_options(options: SaveOptions) -> ui::SaveOptions {
    ui::SaveOptions {
        types: options.types.into_iter().map(waft_file_type).collect(),
        initial_folder: options.initial_folder,
        suggested_name: options.suggested_name,
        confirm_label: options.confirm_label,
    }
}

fn waft_file_type(file_type: FileType) -> ui::FileType {
    ui::FileType {
        label: file_type.label,
        extensions: file_type.extensions,
        mime_types: file_type.mime_types,
    }
}

/// The paths of the files `picked` answers, one or several.
pub fn picked_file_paths<T>(picked: FileDialogFuture<T>) -> PickedPaths
where
    T: IntoIterator<Item = PickedFile> + 'static,
{
    paths_of(picked, file_path)
}

/// The path of the folder `picked` answers.
pub fn picked_folder_path(picked: FileDialogFuture<Option<PickedFolder>>) -> PickedPaths {
    paths_of(picked, folder_path)
}

/// The path of each pick `picked` answers, read by `path_of`; a pick without one fails.
fn paths_of<T, P>(picked: FileDialogFuture<T>, path_of: fn(P) -> Option<PathBuf>) -> PickedPaths
where
    T: IntoIterator<Item = P> + 'static,
    P: 'static,
{
    Box::pin(async move {
        let picks = picked
            .await
            .map_err(|FileDialogError::Failed(reason)| DialogError::Failed(reason))?;
        picks
            .into_iter()
            .map(|pick| {
                path_of(pick).ok_or_else(|| {
                    DialogError::Failed("the system gave a pick without a path".to_owned())
                })
            })
            .collect()
    })
}

/// A file picked's path; none for a handle or a stream.
fn file_path(pick: PickedFile) -> Option<PathBuf> {
    match pick {
        PickedFile::Path(path) => Some(path),
        PickedFile::Handle(_) | PickedFile::Stream(_) => None,
    }
}

/// A folder picked's path; none for a handle.
fn folder_path(pick: PickedFolder) -> Option<PathBuf> {
    match pick {
        PickedFolder::Path(path) => Some(path),
        PickedFolder::Handle(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::future::ready;

    use futures::FutureExt;
    use waft_test::MemoryFile;

    use super::*;

    fn text_type() -> FileType {
        FileType {
            label: "Text".to_owned(),
            extensions: vec!["txt".to_owned()],
            mime_types: vec!["text/plain".to_owned()],
        }
    }

    #[test]
    fn what_a_dialog_offers_is_offered_as_wafts() {
        let open = waft_open_options(OpenOptions {
            types: vec![text_type()],
            initial_folder: Some(PathBuf::from("/Users/someone/Notes")),
            confirm_label: Some("Read".to_owned()),
            writable: true,
        });
        assert_eq!(open.types[0].extensions, ["txt"]);
        assert_eq!(open.types[0].mime_types, ["text/plain"]);
        assert_eq!(open.confirm_label.as_deref(), Some("Read"));
        assert!(open.writable);
        let save = waft_save_options(SaveOptions {
            types: vec![text_type()],
            suggested_name: Some("draft.txt".to_owned()),
            ..SaveOptions::default()
        });
        assert_eq!(save.types[0].label, "Text");
        assert_eq!(save.suggested_name.as_deref(), Some("draft.txt"));
        let folder = waft_folder_options(FolderOptions {
            writable: true,
            ..FolderOptions::default()
        });
        assert!(folder.writable);
    }

    #[test]
    fn picks_are_answered_by_their_paths_and_a_failure_as_the_programs() {
        let picks = vec![
            PickedFile::Path(PathBuf::from("/a.txt")),
            PickedFile::Path(PathBuf::from("/b.txt")),
        ];
        let picked = picked_file_paths(Box::pin(ready(Ok(picks))));
        assert!(matches!(
            picked.now_or_never(),
            Some(Ok(paths)) if paths == [PathBuf::from("/a.txt"), PathBuf::from("/b.txt")]
        ));
        let folder = Some(PickedFolder::Path(PathBuf::from("/pictures")));
        let picked = picked_folder_path(Box::pin(ready(Ok(folder))));
        assert!(matches!(
            picked.now_or_never(),
            Some(Ok(paths)) if paths == [PathBuf::from("/pictures")]
        ));
        let failed: FileDialogFuture<Option<PickedFile>> =
            Box::pin(ready(Err(FileDialogError::Failed("no panel".to_owned()))));
        assert!(matches!(
            picked_file_paths(failed).now_or_never(),
            Some(Err(DialogError::Failed(reason))) if reason == "no panel"
        ));
    }

    #[test]
    fn a_pick_without_a_path_fails_the_dialog() {
        let handle = MemoryFile::new("note.txt", "a line").picked();
        let picked = picked_file_paths(Box::pin(ready(Ok(Some(handle)))));
        assert!(matches!(
            picked.now_or_never(),
            Some(Err(DialogError::Failed(_)))
        ));
    }
}
