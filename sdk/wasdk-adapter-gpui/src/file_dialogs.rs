//! GPUI's path prompts as the system's file dialogs.
//!
//! What the user picks is mounted for the program at its own path the moment they confirm, so the
//! paths GPUI gets back open through `std::fs` as they are. A pick is writable: an editor saves
//! what it opens. GPUI asks for files or folders, not both at once (`can_select_mixed_files_and_dirs`
//! answers false), as wasdk's dialogs offer one or the other.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use futures::channel::oneshot;
use gpui::{ForegroundExecutor, PathPromptOptions};
use wasdk::file_dialogs::{self, FileDialogs, FolderOptions, OpenOptions, SaveOptions};

pub(crate) fn prompt_for_paths(
    executor: &ForegroundExecutor,
    options: PathPromptOptions,
) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
    let (sender, receiver) = oneshot::channel();
    let Some(dialogs) = file_dialogs::get_file_dialogs() else {
        let _ = sender.send(Err(anyhow!("WaOS has no file dialogs here")));
        return receiver;
    };
    executor
        .spawn(async move {
            let _ = sender.send(picked_paths(&dialogs, options).await);
        })
        .detach();
    receiver
}

pub(crate) fn prompt_for_new_path(
    executor: &ForegroundExecutor,
    directory: &Path,
    suggested_name: Option<&str>,
) -> oneshot::Receiver<Result<Option<PathBuf>>> {
    let (sender, receiver) = oneshot::channel();
    let Some(dialogs) = file_dialogs::get_file_dialogs() else {
        let _ = sender.send(Err(anyhow!("WaOS has no file dialogs here")));
        return receiver;
    };
    let options = SaveOptions::new();
    options.initial_folder(&directory.to_string_lossy());
    if let Some(name) = suggested_name {
        options.suggested_name(name);
    }
    executor
        .spawn(async move {
            let picked = dialogs
                .save_file(None, options)
                .await
                .map_err(|error| anyhow!("the save dialog failed: {error:?}"))
                .map(|path| path.map(PathBuf::from));
            let _ = sender.send(picked);
        })
        .detach();
    receiver
}

async fn picked_paths(
    dialogs: &FileDialogs,
    options: PathPromptOptions,
) -> Result<Option<Vec<PathBuf>>> {
    let paths = if options.directories {
        let folder = FolderOptions::new();
        folder.writable();
        if let Some(prompt) = &options.prompt {
            folder.confirm_label(prompt);
        }
        dialogs
            .open_folder(None, folder)
            .await
            .map(|path| path.into_iter().collect::<Vec<_>>())
    } else {
        let open = OpenOptions::new();
        open.writable();
        if let Some(prompt) = &options.prompt {
            open.confirm_label(prompt);
        }
        if options.multiple {
            dialogs.open_files(None, open).await
        } else {
            dialogs
                .open_file(None, open)
                .await
                .map(|path| path.into_iter().collect::<Vec<_>>())
        }
    }
    .map_err(|error| anyhow!("the open dialog failed: {error:?}"))?;
    Ok((!paths.is_empty()).then(|| paths.into_iter().map(PathBuf::from).collect()))
}
