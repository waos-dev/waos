//! A program's file dialogs in Waft's terms: what a dialog offers, the engine's options, as
//! Waft's, and what Waft's dialog answers, as the paths on the machine the user picked, which the
//! engine mounts.
//!
//! The invariant: a pick is answered by its path on the machine or not at all, since the engine
//! mounts paths alone. A pick the system gives without a path, Android's handle or stream, which
//! the desktop systems never give, is answered by where it is on the phone's own storage
//! (`phone_storage`), and fails the dialog, saying why, where it is not there. Whether WaOS may
//! read a path is not asked here: the engine asks the system before it mounts a pick.

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;

use waft::ui::{self, FileDialogError, FileDialogFuture, PickedFile, PickedFolder};
use waos::bindings::wasdk::platform::file_dialogs::DialogError;
use waos::{FileType, FolderOptions, OpenOptions, SaveOptions};

use crate::logic::phone_storage::path_on_phone_storage;

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

/// The paths of the files `picked` answers, one or several, a pick without a path where it is on
/// the phone's storage, shared at `phone_storage`; none where the system's picks are paths, as on
/// a desktop.
pub fn picked_file_paths<T>(
    picked: FileDialogFuture<T>,
    phone_storage: Option<PathBuf>,
) -> PickedPaths
where
    T: IntoIterator<Item = PickedFile> + 'static,
{
    paths_of(
        picked,
        Rc::new(move |pick| file_path(pick, phone_storage.as_deref())),
    )
}

/// The path of the folder `picked` answers, a pick without a path where it is on the phone's
/// storage, shared at `phone_storage`; none where the system's picks are paths.
pub fn picked_folder_path(
    picked: FileDialogFuture<Option<PickedFolder>>,
    phone_storage: Option<PathBuf>,
) -> PickedPaths {
    paths_of(
        picked,
        Rc::new(move |pick| folder_path(pick, phone_storage.as_deref())),
    )
}

/// The path of each pick `picked` answers, read by `path_of`; a pick without one fails, saying why.
fn paths_of<T, P>(
    picked: FileDialogFuture<T>,
    path_of: Rc<dyn Fn(P) -> Result<PathBuf, String>>,
) -> PickedPaths
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
            .map(|pick| path_of(pick).map_err(DialogError::Failed))
            .collect()
    })
}

/// A file picked's path: its own, or, for a handle or a stream, where it is on the phone's
/// storage.
fn file_path(pick: PickedFile, phone_storage: Option<&Path>) -> Result<PathBuf, String> {
    match pick {
        PickedFile::Path(path) => Ok(path),
        PickedFile::Handle(_) | PickedFile::Stream(_) => path_of_url(&pick.url(), phone_storage),
    }
}

/// A folder picked's path: its own, or, for a handle, where it is on the phone's storage.
fn folder_path(pick: PickedFolder, phone_storage: Option<&Path>) -> Result<PathBuf, String> {
    match pick {
        PickedFolder::Path(path) => Ok(path),
        PickedFolder::Handle(_) => match pick.url() {
            Some(url) => path_of_url(&url, phone_storage),
            None => Err(WITHOUT_PATH.to_owned()),
        },
    }
}

const WITHOUT_PATH: &str = "the system gave a pick without a path";

/// Where the pick the system names by `url` is on the phone's storage, shared at `phone_storage`.
fn path_of_url(url: &str, phone_storage: Option<&Path>) -> Result<PathBuf, String> {
    let shared = phone_storage.ok_or_else(|| WITHOUT_PATH.to_owned())?;
    path_on_phone_storage(url, shared)
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
        let picked = picked_file_paths(Box::pin(ready(Ok(picks))), None);
        assert!(matches!(
            picked.now_or_never(),
            Some(Ok(paths)) if paths == [PathBuf::from("/a.txt"), PathBuf::from("/b.txt")]
        ));
        let folder = Some(PickedFolder::Path(PathBuf::from("/pictures")));
        let picked = picked_folder_path(Box::pin(ready(Ok(folder))), None);
        assert!(matches!(
            picked.now_or_never(),
            Some(Ok(paths)) if paths == [PathBuf::from("/pictures")]
        ));
        let failed: FileDialogFuture<Option<PickedFile>> =
            Box::pin(ready(Err(FileDialogError::Failed("no panel".to_owned()))));
        assert!(matches!(
            picked_file_paths(failed, None).now_or_never(),
            Some(Err(DialogError::Failed(reason))) if reason == "no panel"
        ));
    }

    #[test]
    fn a_pick_not_on_the_phone_s_storage_fails_the_dialog() {
        let handle = MemoryFile::new("note.txt", "a line").picked();
        let picked = picked_file_paths(Box::pin(ready(Ok(Some(handle)))), phone_storage());
        assert!(matches!(
            picked.now_or_never(),
            Some(Err(DialogError::Failed(reason))) if reason.contains("phone's own storage")
        ));
    }

    #[test]
    fn a_pick_on_the_phone_s_storage_is_answered_by_its_path() {
        let folder = Some(PickedFolder::Handle(Box::new(DocumentFolder)));
        let picked = picked_folder_path(Box::pin(ready(Ok(folder))), phone_storage());
        assert!(matches!(
            picked.now_or_never(),
            Some(Ok(paths)) if paths == [PathBuf::from("/storage/emulated/0/Music")]
        ));
        let folder = Some(PickedFolder::Handle(Box::new(DocumentFolder)));
        let on_a_desktop = picked_folder_path(Box::pin(ready(Ok(folder))), None);
        assert!(matches!(
            on_a_desktop.now_or_never(),
            Some(Err(DialogError::Failed(reason))) if reason.contains("without a path")
        ));
    }

    /// Where a phone's shared storage is.
    fn phone_storage() -> Option<PathBuf> {
        Some(PathBuf::from("/storage/emulated/0"))
    }

    /// Android's pick of the shared storage's Music folder, as its URL names it.
    struct DocumentFolder;

    impl ui::FolderHandle for DocumentFolder {
        fn name(&self) -> &str {
            "Music"
        }

        fn url(&self) -> Option<String> {
            Some(
                "content://com.android.externalstorage.documents/tree/primary%3AMusic/document/primary%3AMusic"
                    .to_owned(),
            )
        }

        fn entries(&self) -> ui::FolderEntriesFuture {
            Box::pin(ready(Ok(Vec::new())))
        }

        fn open_file(&self, _name: &str) -> ui::FileOpenFuture<PickedFile> {
            Box::pin(ready(Err(std::io::ErrorKind::NotFound.into())))
        }

        fn open_folder(&self, _name: &str) -> ui::FileOpenFuture<PickedFolder> {
            Box::pin(ready(Err(std::io::ErrorKind::NotFound.into())))
        }

        fn create_file(&self, _name: &str) -> Option<ui::FileOpenFuture<PickedFile>> {
            None
        }

        fn create_folder(&self, _name: &str) -> Option<ui::FileOpenFuture<PickedFolder>> {
            None
        }
    }
}
