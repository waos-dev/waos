//! Waft's file dialogs over `wasdk:platform/file-dialogs`, one to one: each kind a function, its
//! options set on the WIT's resources, and each pick answered as a path, which WaOS has mounted
//! where the program reads it (the file dialogs proposal's decision 4).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use waft_embedder::{
    FileDialogError, FileDialogFuture, FileType, FolderOptions, OpenOptions, PickedFile,
    PickedFolder, SaveOptions,
};
use wasdk::file_dialogs as platform;

use crate::views::WasdkView;

/// The platform's file dialogs.
pub(crate) struct WasdkFileDialogs(Rc<platform::FileDialogs>);

impl WasdkFileDialogs {
    /// The platform's file dialogs, where it has them.
    pub(crate) fn get() -> Option<WasdkFileDialogs> {
        platform::get_file_dialogs().map(|dialogs| WasdkFileDialogs(Rc::new(dialogs)))
    }

    pub(crate) fn open_file(
        &self,
        parent: Option<Rc<WasdkView>>,
        options: OpenOptions,
    ) -> FileDialogFuture<Option<PickedFile>> {
        let dialogs = Rc::clone(&self.0);
        Box::pin(async move {
            let options = open_options(options);
            let picked = dialogs.open_file(platform_view(&parent), options).await;
            Ok(picked.map_err(framework_error)?.map(picked_path))
        })
    }

    pub(crate) fn open_files(
        &self,
        parent: Option<Rc<WasdkView>>,
        options: OpenOptions,
    ) -> FileDialogFuture<Vec<PickedFile>> {
        let dialogs = Rc::clone(&self.0);
        Box::pin(async move {
            let options = open_options(options);
            let picked = dialogs.open_files(platform_view(&parent), options).await;
            Ok(picked
                .map_err(framework_error)?
                .into_iter()
                .map(picked_path)
                .collect())
        })
    }

    pub(crate) fn open_folder(
        &self,
        parent: Option<Rc<WasdkView>>,
        options: FolderOptions,
    ) -> FileDialogFuture<Option<PickedFolder>> {
        let dialogs = Rc::clone(&self.0);
        Box::pin(async move {
            let set = platform::FolderOptions::new();
            if let Some(folder) = &options.initial_folder {
                set.initial_folder(&path_string(folder));
            }
            if let Some(label) = &options.confirm_label {
                set.confirm_label(label);
            }
            if options.writable {
                set.writable();
            }
            let picked = dialogs.open_folder(platform_view(&parent), set).await;
            Ok(picked.map_err(framework_error)?.map(picked_folder_path))
        })
    }

    pub(crate) fn save_file(
        &self,
        parent: Option<Rc<WasdkView>>,
        options: SaveOptions,
    ) -> FileDialogFuture<Option<PickedFile>> {
        let dialogs = Rc::clone(&self.0);
        Box::pin(async move {
            let set = platform::SaveOptions::new();
            for file_type in &options.types {
                set.add_type(
                    &file_type.label,
                    &file_type.extensions,
                    &file_type.mime_types,
                );
            }
            if let Some(folder) = &options.initial_folder {
                set.initial_folder(&path_string(folder));
            }
            if let Some(name) = &options.suggested_name {
                set.suggested_name(name);
            }
            if let Some(label) = &options.confirm_label {
                set.confirm_label(label);
            }
            let picked = dialogs.save_file(platform_view(&parent), set).await;
            Ok(picked.map_err(framework_error)?.map(picked_path))
        })
    }
}

/// `options`, set on the WIT's resource.
fn open_options(options: OpenOptions) -> platform::OpenOptions {
    let set = platform::OpenOptions::new();
    for FileType {
        label,
        extensions,
        mime_types,
    } in &options.types
    {
        set.add_type(label, extensions, mime_types);
    }
    if let Some(folder) = &options.initial_folder {
        set.initial_folder(&path_string(folder));
    }
    if let Some(label) = &options.confirm_label {
        set.confirm_label(label);
    }
    if options.writable {
        set.writable();
    }
    set
}

fn platform_view(parent: &Option<Rc<WasdkView>>) -> Option<&wasdk::views::View> {
    parent.as_ref().map(|view| view.platform_view())
}

/// A pick, as the path WaOS mounted it at.
fn picked_path(path: String) -> PickedFile {
    PickedFile::Path(PathBuf::from(path))
}

/// A folder picked, as the path WaOS mounted it at.
fn picked_folder_path(path: String) -> PickedFolder {
    PickedFolder::Path(PathBuf::from(path))
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn framework_error(error: platform::DialogError) -> FileDialogError {
    match error {
        platform::DialogError::Failed(reason) => FileDialogError::Failed(reason),
    }
}
