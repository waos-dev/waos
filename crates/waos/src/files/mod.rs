//! A program's files, served as `wasi:filesystem` from its mounts. The program is given one
//! folder, its root `/`, and every path resolves through its mount table ([`mounts`]): to the
//! longest mount containing it, then beneath that mount's folder on the machine. A folder above
//! a mount lists only what is mounted under it, and a path outside every mount is not found, so a
//! program cannot probe what the machine holds. The mounts are its own `/data` and `/cache`, its
//! `/tmp`, emptied when it ends (`temporary_folders`), its grants' folders, read live, and the files the user handed it, each at its own path, what it
//! picked kept among its grants as well ([`Picks`]): a mount added or removed applies at the
//! program's next call, and a handle already open stays valid. A path stays beneath its mount's
//! folder by the system's one call where it has one, and by a walk of its folders elsewhere
//! ([`beneath`]); every call is made on the program's own thread, and answers when the system
//! does. On Windows they are served by the copy's Windows half, every path walked, and a status
//! read through a handle (`windows`); a path beneath the mounts holding a name Windows reads
//! otherwise than WASI is refused before it reaches the system ([`names`]).
//!
//! The calls are WASI's, whichever its version: the handles ([`Descriptor`]) and where a path
//! leads ([`Target`]). What reaches a folder the program holds by its path, as a watch of it does,
//! takes the path the system gives of its handle ([`HeldFolder`]). [`p3`] serves them as `wasi:filesystem@0.3`, which Rust's `std` calls on
//! `wasm32-wasip3`, whose descriptor a machine's root is too (`machines`).
//!
//! Ported from `wasmtime-wasi` 49.0.3 (`src/filesystem.rs`, `src/filesystem/unix.rs`,
//! `src/p2/filesystem.rs` and `src/p2/host/filesystem.rs`) and 50.0.1 (`src/filesystem/primitives`,
//! [`beneath`], whose module doc names its files), by the Wasmtime Project Developers, under the
//! Apache License 2.0 with the LLVM exception, which `files/LICENSE` holds. What differs: a
//! mount table in place of preopened folders, each call made in place rather than on a pool of
//! blocking threads, a path kept beneath its folder by Apple's flags as well, and a read-only mount
//! refusing a change with WASI's `read-only`.
//!
//! A machine the user gave the program (`machines`) is given whole, beside the mounts: its root
//! is a folder of the machine, writable, with no table above it ([`folder_of_machine`]), whose
//! paths resolve with the system's ordinary calls, as they do for any program on the machine,
//! a symlink holding an absolute path and `..` above `/` among them (the processes proposal's
//! decision 8). On Windows this computer's root is the folder of its drives, `/c` being `C:\`,
//! and every path of this computer given to a program, or taken from it, is named as
//! [`machine_paths`] names it.
//!
//! The invariant: nothing of the machine is reached from a mount but beneath its folder, through
//! [`beneath`]; only a machine's root, and what is opened from it, resolves paths unconfined; a
//! folder above a mount is the table's, never the machine's.

mod beneath;
mod descriptor;
mod errors;
mod held_folder;
pub mod machine_paths;
mod mounts;
mod names;
pub(crate) mod p3;
mod picks;
mod status;
mod target;
#[cfg(windows)]
mod windows;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use waos_capabilities::{ProgramFolders, ProgramGrants};
use wasmtime::component::{HasData, ResourceTable};

pub(crate) use descriptor::Descriptor;
use descriptor::{DirectoryEntry, OpenMode, Opened};
pub(crate) use errors::ErrorCode;
pub(crate) use held_folder::HeldFolder;
pub(crate) use mounts::HandedFiles;
use mounts::{LiveMounts, ROOT};
pub(crate) use picks::Picks;
use status::{DescriptorStat, MetadataHashValue};
use target::Target;

/// The files of a machine the user gave a program, from `folder`: the folder, open, which the
/// program may change, every path from it resolving as the machine resolves any path. On Windows
/// this computer's root, `/`, is the folder of its drives ([`machine_paths`]).
pub(crate) fn folder_of_machine(folder: &Path) -> io::Result<Descriptor> {
    #[cfg(windows)]
    if folder == Path::new(machine_paths::ROOT) {
        return Ok(Descriptor::Drives);
    }
    let opened = Opened::on_machine(
        mounts::open_folder(folder)?,
        folder.to_path_buf(),
        OpenMode {
            read: true,
            write: true,
        },
    );
    Ok(Descriptor::Dir(opened))
}

/// What the files read and write in a program's store, whichever WASI version calls them.
pub(crate) struct FilesCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) files: &'a mut ProgramFiles,
}

pub(crate) trait FilesCtxView: Send {
    fn files_ctx(&mut self) -> FilesCtx<'_>;
}

struct HasFiles;

impl HasData for HasFiles {
    type Data<'a> = FilesCtx<'a>;
}

/// One program's files: its mounts, as its grants hold them now.
pub(crate) struct ProgramFiles {
    mounts: LiveMounts,
}

impl ProgramFiles {
    /// The files of a program with its own `folders` and its `temporary` folder, where it has
    /// them, `grants`, and the files the user hands it, `handed`.
    pub(crate) fn new(
        folders: Option<&ProgramFolders>,
        temporary: Option<&Path>,
        grants: Arc<dyn ProgramGrants>,
        handed: HandedFiles,
    ) -> ProgramFiles {
        ProgramFiles {
            mounts: LiveMounts::new(folders, temporary, grants, handed),
        }
    }

    /// Where on the machine `path`, an absolute path inside one of the program's mounts, is; none
    /// for a path outside every mount, where nothing is, or one a symlink leads out of its mount.
    pub(crate) fn machine_path(&mut self, path: &Path) -> Option<PathBuf> {
        self.mounts.now().machine_path(path)
    }

    /// The one folder the program is given: its root, `/`.
    pub(crate) fn root() -> Descriptor {
        Descriptor::Above(PathBuf::from(ROOT))
    }

    /// Where `path`, given with `descriptor`, leads: through the mounts from a folder above
    /// them, or beneath a folder opened beneath one, a name the system would read otherwise
    /// refused ([`names`]); or, from a machine's root, as the machine resolves it.
    pub(crate) fn target(
        &mut self,
        descriptor: &Descriptor,
        path: &str,
    ) -> Result<Target, ErrorCode> {
        match descriptor {
            Descriptor::Above(folder) => {
                names::check(path)?;
                self.mounts.now().resolve(folder, Path::new(path))
            }
            #[cfg(windows)]
            Descriptor::Drives => Ok(Target::in_drives(path)),
            Descriptor::Dir(folder) if folder.is_on_machine() => Ok(folder.beneath(path)),
            Descriptor::Dir(folder) => {
                names::check(path)?;
                Ok(folder.beneath(path))
            }
            Descriptor::File(_) => Err(ErrorCode::NotDirectory),
        }
    }

    /// The status of what `descriptor` is on.
    pub(crate) fn status(&mut self, descriptor: &Descriptor) -> Result<DescriptorStat, ErrorCode> {
        match descriptor {
            Descriptor::Above(_) => self.target(descriptor, ".")?.status(true),
            #[cfg(windows)]
            Descriptor::Drives => Ok(DescriptorStat::folder_above()),
            Descriptor::Dir(opened) | Descriptor::File(opened) => Ok(opened.file_status()?.status),
        }
    }

    /// The hash of what `descriptor` is on.
    pub(crate) fn metadata_hash(
        &mut self,
        descriptor: &Descriptor,
    ) -> Result<MetadataHashValue, ErrorCode> {
        match descriptor {
            Descriptor::Above(_) => self.target(descriptor, ".")?.metadata_hash(true),
            #[cfg(windows)]
            Descriptor::Drives => Ok(MetadataHashValue::of_folder_above(Path::new(
                machine_paths::ROOT,
            ))),
            Descriptor::Dir(opened) | Descriptor::File(opened) => {
                Ok(MetadataHashValue::of_file(opened.file_status()?.identity))
            }
        }
    }

    /// The entries of the folder `descriptor` is on: what the system lists, or for a folder
    /// above the mounts, the next name of each mount under it, each a folder.
    pub(crate) fn read_directory(
        &mut self,
        descriptor: &Descriptor,
    ) -> Result<Vec<Result<DirectoryEntry, ErrorCode>>, ErrorCode> {
        match descriptor {
            Descriptor::Dir(folder) => folder.read_directory(),
            Descriptor::File(_) => Err(ErrorCode::NotDirectory),
            #[cfg(windows)]
            Descriptor::Drives => Ok(machine_paths::drives()
                .into_iter()
                .map(|name| {
                    Ok(DirectoryEntry {
                        type_: status::DescriptorType::Directory,
                        name,
                    })
                })
                .collect()),
            Descriptor::Above(path) => {
                let table = self.mounts.now();
                match table.resolve(path, Path::new("."))? {
                    Target::Above(folder) => Ok(table.entries_under(&folder)),
                    target => target.open_folder()?.read_directory(),
                }
            }
        }
    }
}
