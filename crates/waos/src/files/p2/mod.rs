//! A program's files served as `wasi:filesystem@0.2`, which Rust's `std` calls on
//! `wasm32-wasip2`: its descriptors and directory-entry streams are the files' own handles, its
//! one preopened folder is the root, and a file's streams are `wasi:io`'s, read and written in
//! place. Its bindings are generated from `wasi-p2/`; `wasi:io` and `wasi:clocks` are named from
//! `wasmtime-wasi-io` and `wasmtime-wasi`, which serve them.
//!
//! The invariant: this module only translates. What a call does is the files' ([`super`]); here
//! are WASI 0.2's types for it, its streams, and its handles in the program's table.

mod convert;
mod host;
mod streams;

use wasmtime::component::{HasData, Linker, ResourceTable};

use super::ProgramFiles;
use super::errors::ErrorCode;

pub(crate) mod bindings {
    wasmtime::component::bindgen!({
        path: "wasi-p2",
        world: "waos:files/files",
        require_store_data_send: true,
        imports: { default: tracing | trappable },
        trappable_error_type: {
            "wasi:filesystem/types.error-code" => crate::files::p2::FsError,
        },
        with: {
            "wasi:io": wasmtime_wasi_io::bindings::wasi::io,
            "wasi:clocks/wall-clock": wasmtime_wasi::p2::bindings::clocks::wall_clock,
            "wasi:filesystem/types.descriptor": crate::files::descriptor::Descriptor,
            "wasi:filesystem/types.directory-entry-stream": crate::files::p2::host::DirectoryEntries,
        },
    });
}

use bindings::wasi::filesystem::{preopens, types};

/// What the files read and write in a program's store.
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

pub(crate) fn add_to_linker<T: FilesCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    types::add_to_linker::<_, HasFiles>(linker, T::files_ctx)?;
    preopens::add_to_linker::<_, HasFiles>(linker, T::files_ctx)
}

/// What a `wasi:filesystem@0.2` call fails with: WASI's error code for the program, or a trap.
#[derive(Debug)]
pub(crate) enum FsError {
    Code(types::ErrorCode),
    Trap(wasmtime::Error),
}

/// What a call answers.
type FsResult<T> = Result<T, FsError>;

impl From<ErrorCode> for FsError {
    fn from(code: ErrorCode) -> FsError {
        FsError::Code(code.into())
    }
}

impl From<wasmtime::component::ResourceTableError> for FsError {
    /// A handle the program does not hold, which traps.
    fn from(error: wasmtime::component::ResourceTableError) -> FsError {
        FsError::Trap(error.into())
    }
}
