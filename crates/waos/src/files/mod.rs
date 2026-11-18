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
//! leads ([`Target`]). Every path the program names is led through the mounts by one function,
//! [`ProgramFiles::target`], and each call is told through the program's access as the use it is:
//! an open and a change decided, the table's refusal being the verdict and the place it found the
//! record ([`ProgramFiles::open`], [`ProgramFiles::change`]); a status, a symlink read or a hash
//! only noted ([`ProgramFiles::named`]); a handle's first write noted once; and each file opened
//! kept among the program's open files while it is held. What reaches a folder the program holds by its path, as a watch of it does,
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
mod handle_path;
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

use waos_capabilities::ProgramFolders;
use wasmtime::component::{HasData, ResourceTable};

use crate::access::{
    Asked, Cause, FileChange, Noted, ProgramAccess, Refused, Resolution, Resolved,
};
pub(crate) use descriptor::{Descriptor, OpenMode};
use descriptor::{DirectoryEntry, Opened};
pub(crate) use errors::ErrorCode;
pub(crate) use handle_path::path_of;
pub(crate) use held_folder::HeldFolder;
#[cfg(test)]
pub(crate) use mounts::open_folder;
pub(crate) use mounts::{HandedFiles, MountKind, MountOf, MountTable};
use mounts::{LiveMounts, ROOT};
pub(crate) use picks::Picks;
use status::{DescriptorStat, MetadataHashValue};
use target::{Change, OpenRequest, Target};

/// The files of a machine the user gave a program, from `folder`: the folder, open, which the
/// program may change, every path from it resolving as the machine resolves any path, kept among
/// the open files of the program `access` is of. On Windows this computer's root, `/`, is the
/// folder of its drives ([`machine_paths`]), which no handle holds.
pub(crate) fn folder_of_machine(folder: &Path, access: &ProgramAccess) -> io::Result<Descriptor> {
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
    Ok(Descriptor::Dir(used_as_open(opened, access)))
}

/// `opened`, kept among the files `access`'s program has open while any copy of it is held.
fn used_as_open(opened: Opened, access: &ProgramAccess) -> Opened {
    let open_use = access.opened(opened.handle(), opened.mount().kind, opened.open_mode());
    opened.used_as(open_use)
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

/// One program's files: its mounts, as its grants hold them now, and where each use of them is
/// decided and told.
pub(crate) struct ProgramFiles {
    mounts: LiveMounts,
    access: ProgramAccess,
}

/// Why the mounts did not let a path call go on: what the program is answered, and whether the
/// mounts refused it, as a path outside them or a change a mount does not allow, or it failed.
pub(crate) struct FilesRefusal {
    code: ErrorCode,
    by_the_mounts: bool,
}

impl ProgramFiles {
    /// The files of a program with its own `folders` and its `temporary` folder, where it has
    /// them, the grants `access` reads, and the files the user hands it, `handed`.
    pub(crate) fn new(
        folders: Option<&ProgramFolders>,
        temporary: Option<&Path>,
        access: ProgramAccess,
        handed: HandedFiles,
    ) -> ProgramFiles {
        ProgramFiles {
            mounts: LiveMounts::new(folders, temporary, access.clone(), handed),
            access,
        }
    }

    /// Opens what `path`, given with `from`, names, as `request` asks: decided by the mounts and
    /// told as one use, and kept among the program's open files while it is held.
    pub(crate) fn open(
        &mut self,
        from: &Descriptor,
        path: &str,
        request: &OpenRequest,
    ) -> Result<Descriptor, ErrorCode> {
        let access = self.access.clone();
        let asked = Asked::OpenFile {
            path,
            mode: request.open_mode(),
            cause: Cause::Program,
        };
        let target = access.decide(asked, |_| self.permitted_open(from, path, request))?;
        Ok(match target.open(request)? {
            Descriptor::Dir(opened) => Descriptor::Dir(used_as_open(opened, &access)),
            Descriptor::File(opened) => Descriptor::File(used_as_open(opened, &access)),
            other => other,
        })
    }

    /// Where `path` leads from `from`, if the mounts let it be opened as `request` asks.
    fn permitted_open(
        &mut self,
        from: &Descriptor,
        path: &str,
        request: &OpenRequest,
    ) -> Result<Target, FilesRefusal> {
        request.check().map_err(FilesRefusal::failed)?;
        let target = self.target(from, path).map_err(FilesRefusal::failed)?;
        match target.open_refusal(request) {
            Some(code) => Err(FilesRefusal::by_the_mounts(code)),
            None => Ok(target),
        }
    }

    /// Where `change` at `path`, given with `from`, is made: decided by the mounts and told as one
    /// use. A rename and a link name a second path, from `to_from`; use
    /// [`change_between`](Self::change_between) for them.
    pub(crate) fn change(
        &mut self,
        from: &Descriptor,
        path: &str,
        change: FileChange<'_>,
    ) -> Result<Target, ErrorCode> {
        let access = self.access.clone();
        let kind = change_of(&change);
        let asked = Asked::ChangeFile { path, change };
        Ok(access.decide(asked, |_| self.permitted_change(from, path, kind))?)
    }

    /// Where a rename or a link moves or links what `path`, given with `from`, names, and where it
    /// puts it, at the path `change` names, given with `to_from`: decided by the mounts, both
    /// places, and told as one use.
    pub(crate) fn change_between(
        &mut self,
        from: &Descriptor,
        path: &str,
        to_from: &Descriptor,
        change: FileChange<'_>,
    ) -> Result<(Target, Target), ErrorCode> {
        let access = self.access.clone();
        let (kinds, to) = match change {
            FileChange::Rename { to } => ((Change::Remove, Change::Replace), to),
            FileChange::HardLink { to } => ((Change::Modify, Change::Create), to),
            _ => return Err(ErrorCode::Invalid),
        };
        let asked = Asked::ChangeFile { path, change };
        let places = access.decide(asked, |_| {
            let from = self.permitted_change(from, path, kinds.0)?;
            let to = self.permitted_change(to_from, to, kinds.1)?;
            Ok::<_, FilesRefusal>(Places(from, to))
        })?;
        Ok((places.0, places.1))
    }

    /// Where `path` leads from `from`, if the mounts let `change` be made there.
    fn permitted_change(
        &mut self,
        from: &Descriptor,
        path: &str,
        change: Change,
    ) -> Result<Target, FilesRefusal> {
        let target = self.target(from, path).map_err(FilesRefusal::failed)?;
        match target.refusal_for(change) {
            Some(code) => Err(FilesRefusal::by_the_mounts(code)),
            None => Ok(target),
        }
    }

    /// Where `path`, given with `from`, leads, for a status, a symlink read or a hash: the same
    /// resolution, noted, not decided.
    pub(crate) fn named(&mut self, from: &Descriptor, path: &str) -> Result<Target, ErrorCode> {
        let target = self.target(from, path);
        let resolution = match &target {
            Ok(target) => target.resolution(),
            Err(_) => Resolution::Nothing,
        };
        self.access.note(Noted::MetadataRead { path, resolution });
        target
    }

    /// Notes the first write through `opened`, once per handle.
    pub(crate) fn writing(&self, opened: &Opened) {
        if opened.first_write() {
            self.access.note(Noted::FirstWrite {
                mount: opened.mount(),
                file: opened.handle(),
            });
        }
    }

    /// Where on the machine `path`, an absolute path inside one of the program's mounts, is, for
    /// what `cause` names to open on the program's behalf: decided and told as an open; none for a
    /// path outside every mount, where nothing is, or one a symlink leads out of its mount.
    pub(crate) fn machine_path(&mut self, path: &Path, cause: Cause) -> Option<PathBuf> {
        let access = self.access.clone();
        let asked = Asked::OpenFile {
            path: &path.to_string_lossy(),
            mode: OpenMode {
                read: true,
                write: false,
            },
            cause,
        };
        access
            .decide(asked, |_| self.mounts.now().machine_path(path))
            .ok()
    }

    /// The one folder the program is given: its root, `/`.
    pub(crate) fn root() -> Descriptor {
        Descriptor::Above(PathBuf::from(ROOT))
    }

    /// Where `path`, given with `descriptor`, leads: through the mounts from a folder above
    /// them, or beneath a folder opened beneath one, a name the system would read otherwise
    /// refused ([`names`]); or, from a machine's root, as the machine resolves it. The one place a
    /// path the program names is led through the mounts.
    fn target(&mut self, descriptor: &Descriptor, path: &str) -> Result<Target, ErrorCode> {
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

/// What a change is to the module's table of refusals.
fn change_of(change: &FileChange<'_>) -> Change {
    match change {
        FileChange::CreateDirectory | FileChange::Symlink { .. } => Change::Create,
        FileChange::RemoveDirectory | FileChange::Remove => Change::Remove,
        FileChange::SetTimes => Change::Modify,
        FileChange::Rename { .. } => Change::Remove,
        FileChange::HardLink { .. } => Change::Modify,
    }
}

/// The two places a rename or a link names: what is moved or linked, and where it goes.
struct Places(Target, Target);

impl Resolved for Places {
    fn resolution(&self) -> Resolution<'_> {
        Resolution::Both(Box::new((self.0.resolution(), self.1.resolution())))
    }
}

impl FilesRefusal {
    fn by_the_mounts(code: ErrorCode) -> FilesRefusal {
        FilesRefusal {
            code,
            by_the_mounts: true,
        }
    }

    fn failed(code: ErrorCode) -> FilesRefusal {
        FilesRefusal {
            code,
            by_the_mounts: false,
        }
    }
}

impl Refused for FilesRefusal {
    fn setting(&self) -> Option<&'static str> {
        self.by_the_mounts.then_some("folders")
    }
}

impl From<FilesRefusal> for ErrorCode {
    fn from(refusal: FilesRefusal) -> ErrorCode {
        refusal.code
    }
}

impl Refused for ErrorCode {
    /// A system's error, which no setting decides.
    fn setting(&self) -> Option<&'static str> {
        None
    }
}

#[cfg(test)]
mod tests;
