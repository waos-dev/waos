//! `wasdk:platform/file-dialogs`: the platform's file dialogs, a function per kind, each pick
//! mounted at its own path the moment it is answered.
//!
//! The shell shows the dialog and answers the paths on the machine the user chose ([`ShellFileDialogs`]),
//! unless the user set the dialog's answer among the program's settings: then those paths are the
//! answer, checked as the dialog would have let them be chosen (`set_answers`), and nothing is shown;
//! the engine mounts each where the program sees it, at its own path, writable where the options
//! asked or the dialog saves, and keeps it among the program's mounts through its grants
//! (`files::Picks`), until the user removes it. The interface is served to a program by `host`.
//!
//! The invariant: a path answered to the program is mounted before the program has it, so the
//! program never reads a pick it cannot reach.

pub(crate) mod host;
pub(crate) mod set_answers;

use std::path::PathBuf;
use std::sync::Arc;

use crate::shell::ShellFileDialogs;

/// A kind of file a dialog offers, as Flutter's `XTypeGroup`: its label, its extensions and its
/// MIME types.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileType {
    pub label: String,
    pub extensions: Vec<String>,
    pub mime_types: Vec<String>,
}

/// What an open dialog offers; `file-dialogs.open-options`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenOptions {
    /// The kinds of file offered; none for any file.
    pub types: Vec<FileType>,
    /// The folder the dialog opens in, on the machine.
    pub initial_folder: Option<PathBuf>,
    pub confirm_label: Option<String>,
    /// Whether the pick may be written, not only read.
    pub writable: bool,
}

/// What a folder dialog offers; `file-dialogs.folder-options`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderOptions {
    pub initial_folder: Option<PathBuf>,
    pub confirm_label: Option<String>,
    /// Whether what the folder holds may be written, not only read.
    pub writable: bool,
}

/// What a save dialog offers; `file-dialogs.save-options`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveOptions {
    pub types: Vec<FileType>,
    pub initial_folder: Option<PathBuf>,
    /// The file's name the dialog starts with.
    pub suggested_name: Option<String>,
    pub confirm_label: Option<String>,
}

/// `file-dialogs.file-dialogs`: the shell's dialogs, as the program reaches them.
pub struct FileDialogs(Arc<dyn ShellFileDialogs>);
