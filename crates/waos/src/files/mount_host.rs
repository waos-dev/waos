//! The code serving `wasdk:platform/mounts` to a program: a folder it holds, a machine's root among
//! them, mounted at a path of its own files for as long as it keeps the `mounted`, so what reaches
//! files by their paths reaches it there. The mount is one of the program's handed files'
//! ([`HandedFiles::mount`]), which the mount table takes up at the program's next call.
//!
//! The invariant: a program mounts only a folder it holds, as writable as its descriptor is and of
//! the kind its folder is, so mounting gives it nothing it did not have.

use std::path::Path;

use wasmtime::component::{HasData, Linker, Resource, ResourceTable};

use super::{Descriptor, ErrorCode, HandedFiles, HeldFolder};
use crate::bindings::wasdk::platform::mounts::{self, HostMounted};
use crate::files::p3::bindings::wasi::filesystem::types::ErrorCode as WasiErrorCode;

/// A folder the program mounted, until it drops this.
pub struct ProgramMount {
    /// The number the program's handed files know it by.
    number: u64,
}

/// What the mounts interface reads and writes in a program's store.
pub(crate) struct MountsCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) handed: &'a HandedFiles,
}

pub(crate) trait MountsCtxView: Send {
    fn mounts_ctx(&mut self) -> MountsCtx<'_>;
}

struct HasMounts;

impl HasData for HasMounts {
    type Data<'a> = MountsCtx<'a>;
}

pub(crate) fn add_to_linker<T: MountsCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    mounts::add_to_linker::<_, HasMounts>(linker, T::mounts_ctx)
}

impl mounts::Host for MountsCtx<'_> {
    /// A descriptor the program does not hold traps, as every file call's does.
    fn mount(
        &mut self,
        folder: Resource<Descriptor>,
        at: String,
    ) -> wasmtime::Result<Result<Resource<ProgramMount>, WasiErrorCode>> {
        let mounted = mount_held(self.table.get(&folder)?, self.handed, Path::new(&at));
        Ok(match mounted {
            Ok(number) => Ok(self.table.push(ProgramMount { number })?),
            Err(code) => Err(code.into()),
        })
    }
}

impl HostMounted for MountsCtx<'_> {
    fn drop(&mut self, mounted: Resource<ProgramMount>) -> wasmtime::Result<()> {
        let mounted = self.table.delete(mounted)?;
        self.handed.unmount(mounted.number);
        Ok(())
    }
}

/// Mounts the folder `descriptor` is on at `at` among `handed`: `not-directory` for a file and
/// `unsupported` for a folder above the mounts, which is no folder of the machine.
fn mount_held(descriptor: &Descriptor, handed: &HandedFiles, at: &Path) -> Result<u64, ErrorCode> {
    let held = HeldFolder::of(descriptor)?;
    let Descriptor::Dir(opened) = descriptor else {
        return Err(ErrorCode::NotDirectory);
    };
    let writable = descriptor.flags().mutate_directory;
    let kind = opened.mount().kind;
    tracing::info!(folder = %held.path().display(), at = %at.display(), writable, "a folder mounted");
    handed.mount(held.path(), at, writable, kind)
}
