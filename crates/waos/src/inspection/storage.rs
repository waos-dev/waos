//! A program's storage as an inspector reads it: the folders and files it sees at paths of its
//! own, from its own `/data`, `/cache` and `/tmp`, the folders its user granted it and the files
//! handed to it, whether it runs or not; and the entries of any folder inside those, resolved
//! through the program's own mount table, so a path that leaves a mount reads nothing.
//!
//! The invariant: every path read is resolved by [`MountTable`], as the program's own access
//! resolves it; nothing here opens a folder of the machine that the program does not see.

use std::path::PathBuf;
use std::sync::Arc;

use waos_capabilities::{MountKind, ProgramFolders, ProgramGrants};

use super::bindings::wasdk::inspect::storage;
use super::bindings::wasi::clocks::system_clock::Instant;
use crate::files::HandedFiles;
use crate::files::MountTable;

/// What a program sees of this computer's files, as its settings, grants and handed files make it
/// now.
#[derive(Clone)]
pub struct ProgramStorage {
    folders: Option<ProgramFolders>,
    temporary: Option<PathBuf>,
    grants: Arc<dyn ProgramGrants>,
    handed: HandedFiles,
}

impl ProgramStorage {
    /// The storage a program that does not run has: its own `folders`, its `temporary` folder
    /// where it has one, and its `grants`, none granted where `grants` is none. Nothing is
    /// handed to it.
    pub fn of(
        folders: Option<ProgramFolders>,
        temporary: Option<PathBuf>,
        grants: Option<Arc<dyn ProgramGrants>>,
    ) -> ProgramStorage {
        ProgramStorage {
            folders,
            temporary,
            grants: grants.unwrap_or_else(|| Arc::new(crate::permissions::NothingGranted)),
            handed: HandedFiles::default(),
        }
    }

    /// The same, with the files its run handed it.
    pub(crate) fn with_handed(
        folders: Option<ProgramFolders>,
        temporary: Option<PathBuf>,
        grants: Arc<dyn ProgramGrants>,
        handed: HandedFiles,
    ) -> ProgramStorage {
        ProgramStorage {
            folders,
            temporary,
            grants,
            handed,
        }
    }

    /// Its mounts as its grants make them now, as `wasdk:inspect` names them.
    pub fn mounts(&self) -> Vec<storage::Mount> {
        let table = self.table();
        (table.mounts().into_iter())
            .map(|mounted| storage::Mount {
                path: mounted.at.display().to_string(),
                kind: mount_kind(mounted.kind),
                writable: mounted.writable,
                place: mounted.place.display().to_string(),
            })
            .collect()
    }

    /// The entries of `path`, a path of the program, resolved through its mounts, as
    /// `wasdk:inspect` names them.
    pub fn read_folder(&self, path: &str) -> Result<Vec<storage::Entry>, String> {
        let entries = self.table().read_folder(path).map_err(why)?;
        Ok((entries.into_iter())
            .map(|entry| storage::Entry {
                name: entry.name,
                is_folder: entry.is_folder,
                size: entry.size,
                modified: entry.modified.map(|(seconds, nanoseconds)| Instant {
                    seconds,
                    nanoseconds,
                }),
            })
            .collect())
    }

    /// The table its folders, temporary folder, grants and handed files make now.
    fn table(&self) -> MountTable {
        MountTable::of(
            self.folders.as_ref(),
            self.temporary.as_deref(),
            self.grants.as_ref(),
            &self.handed,
        )
    }
}

/// What a mount is to the inspector, as `wasdk:inspect` names it.
fn mount_kind(kind: MountKind) -> storage::MountKind {
    match kind {
        MountKind::OwnData => storage::MountKind::OwnData,
        MountKind::OwnCache => storage::MountKind::OwnCache,
        MountKind::OwnTemporary => storage::MountKind::OwnTemporary,
        MountKind::Granted => storage::MountKind::GrantedFolder,
        MountKind::Picked | MountKind::Pasted => storage::MountKind::HandedFile,
        // A machine's root is no folder a program sees at a path of its own.
        MountKind::Machine => storage::MountKind::GrantedFolder,
    }
}

/// Why a folder was not read, in the words an inspector shows.
fn why(code: crate::files::ErrorCode) -> String {
    use crate::files::ErrorCode;
    match code {
        ErrorCode::NoEntry => "no entry",
        ErrorCode::NotPermitted => "not permitted",
        ErrorCode::NotDirectory => "not a folder",
        ErrorCode::IsDirectory => "a folder",
        ErrorCode::Invalid => "invalid path",
        ErrorCode::Access => "access denied",
        ErrorCode::ReadOnly => "read-only",
        ErrorCode::NameTooLong => "name too long",
        ErrorCode::Loop => "a loop of links",
        ErrorCode::Io => "a system error",
        _ => "the system refused it",
    }
    .to_owned()
}
