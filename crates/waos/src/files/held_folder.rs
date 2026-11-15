//! Where on the machine a folder the program holds is now, for what reaches it by its path rather
//! than through its handle, as the system's watches of a folder do (`file_watching`): the path the
//! system gives of the folder's handle ([`path_of`]).
//!
//! The invariant: a path is given only while it leads to the folder the handle is on, checked by
//! the folder's identity, so a folder beneath a mount is never named by a path that a rename or a
//! symlink has since made lead elsewhere, outside the mount among them.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::descriptor::Descriptor;
use super::errors::ErrorCode;
use super::handle_path::path_of;
use super::mounts::open_folder;
use super::status::FileStat;
use crate::access::{Resolution, Resolved};

/// A folder the program holds, and the path that leads to it now.
pub(crate) struct HeldFolder {
    handle: Arc<fs::File>,
    path: PathBuf,
}

impl HeldFolder {
    /// The folder `descriptor` is on, at the path that leads to it now: `not-directory` for a
    /// file, `unsupported` for a folder above the mounts, which is the table's and no folder of the
    /// machine, and for the drives of this computer on Windows; `no-entry` for a folder no path
    /// leads to any longer.
    pub(crate) fn of(descriptor: &Descriptor) -> Result<HeldFolder, ErrorCode> {
        let opened = match descriptor {
            Descriptor::Dir(opened) => opened,
            Descriptor::File(_) => return Err(ErrorCode::NotDirectory),
            Descriptor::Above(_) => return Err(ErrorCode::Unsupported),
            #[cfg(windows)]
            Descriptor::Drives => return Err(ErrorCode::Unsupported),
        };
        let handle = Arc::clone(opened.handle());
        let path = path_of(&handle)?;
        let folder = HeldFolder { handle, path };
        if !folder.is_at_its_path() {
            return Err(ErrorCode::NoEntry);
        }
        Ok(folder)
    }

    /// The path that led to the folder when it was taken.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the path still leads to the folder.
    pub(crate) fn is_at_its_path(&self) -> bool {
        let identity = |folder: &fs::File| FileStat::of_file(folder).map(|stat| stat.identity);
        match (open_folder(&self.path), identity(&self.handle)) {
            (Ok(at_path), Ok(held)) => identity(&at_path).is_ok_and(|found| found == held),
            _ => false,
        }
    }
}

impl Resolved for HeldFolder {
    fn resolution(&self) -> Resolution<'_> {
        Resolution::OnMachine(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::super::descriptor::{OpenMode, Opened};
    use super::super::mounts::MountOf;
    use super::*;

    fn held(folder: &Path) -> Descriptor {
        let mode = OpenMode {
            read: true,
            write: false,
        };
        Descriptor::Dir(Opened::new(
            open_folder(folder).unwrap(),
            MountOf::granted(false),
            mode,
        ))
    }

    #[test]
    fn a_folder_held_is_at_the_path_the_system_gives_until_it_moves() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = scratch.path().join("watched");
        fs::create_dir(&folder).unwrap();
        let descriptor = held(&folder);

        let found = HeldFolder::of(&descriptor).unwrap();
        assert_eq!(
            fs::canonicalize(found.path()).unwrap(),
            fs::canonicalize(&folder).unwrap()
        );
        assert!(found.is_at_its_path());

        fs::rename(&folder, scratch.path().join("moved")).unwrap();
        fs::create_dir(&folder).unwrap();
        assert!(!found.is_at_its_path(), "another folder is at the path now");
        let moved = HeldFolder::of(&descriptor).unwrap();
        assert!(moved.path().ends_with("moved"), "{:?}", moved.path());
    }

    #[test]
    fn a_file_and_a_folder_above_the_mounts_are_no_folder_to_reach_by_its_path() {
        let scratch = tempfile::tempdir().unwrap();
        let file = scratch.path().join("file");
        fs::write(&file, "").unwrap();
        let mode = OpenMode {
            read: true,
            write: false,
        };
        let file = Descriptor::File(Opened::new(
            fs::File::open(&file).unwrap(),
            MountOf::granted(false),
            mode,
        ));
        assert!(matches!(
            HeldFolder::of(&file),
            Err(ErrorCode::NotDirectory)
        ));
        let above = Descriptor::Above(PathBuf::from("/"));
        assert!(matches!(
            HeldFolder::of(&above),
            Err(ErrorCode::Unsupported)
        ));
    }
}
