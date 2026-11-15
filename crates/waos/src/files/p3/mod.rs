//! A program's files served as `wasi:filesystem@0.3`, which Rust's `std` calls on
//! `wasm32-wasip3`: its descriptors are the files' own handles, its one
//! preopened folder is the root, and a file's contents and a folder's entries are component-model
//! streams, each read and write made in place on the program's thread. Its bindings are generated
//! from the engine's copy of `wit/`, whose `wasi-filesystem-0.3.0` is the one `wasmtime-wasi` 50
//! serves; `wasi:clocks` is named from `wasmtime-wasi`, which serves it.
//!
//! Ported from `wasmtime-wasi` 50.0.1 (`src/p3/filesystem/host.rs`), by the Wasmtime Project
//! Developers, under the Apache License 2.0 with the LLVM exception, which `files/LICENSE` holds.
//! What differs is what [`super`] says of the files, and the streams read and written in place
//! rather than on a pool of blocking threads.
//!
//! The invariant: this module only translates. What a call does is the files' ([`super`]); here
//! are WASI 0.3's types for it, its streams, and its handles in the program's table.

mod convert;
mod host;
mod streams;

use wasmtime::component::Linker;

use super::errors::ErrorCode;
use super::{FilesCtxView, HasFiles};

pub(crate) mod bindings {
    wasmtime::component::bindgen!({
        inline: "
            package waos:files@0.3.0;

            /// What a program's `std` imports for its files on `wasm32-wasip3`, which WaOS serves
            /// itself (`crates/waos/src/files`).
            world files {
                import wasi:filesystem/types@0.3.0;
                import wasi:filesystem/preopens@0.3.0;
            }
        ",
        path: "wit",
        require_store_data_send: true,
        imports: {
            "wasi:filesystem/types@0.3.0.[method]descriptor.read-via-stream": store | tracing | trappable,
            "wasi:filesystem/types@0.3.0.[method]descriptor.write-via-stream": store | tracing | trappable,
            "wasi:filesystem/types@0.3.0.[method]descriptor.append-via-stream": store | tracing | trappable,
            "wasi:filesystem/types@0.3.0.[method]descriptor.read-directory": store | tracing | trappable,
            default: tracing | trappable,
        },
        trappable_error_type: {
            "wasi:filesystem/types@0.3.0.error-code" => crate::files::p3::FsError,
        },
        with: {
            "wasi:clocks": wasmtime_wasi::p3::bindings::clocks,
            "wasi:filesystem/types@0.3.0.descriptor": crate::files::descriptor::Descriptor,
        },
    });
}

use bindings::wasi::filesystem::{preopens, types};

pub(crate) fn add_to_linker<T: FilesCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    types::add_to_linker::<_, HasFiles>(linker, T::files_ctx)?;
    preopens::add_to_linker::<_, HasFiles>(linker, T::files_ctx)
}

/// What a `wasi:filesystem@0.3` call fails with: WASI's error code for the program, or a trap.
/// Public, as the bindings' traits that answer with it are; the crate does not export it.
#[derive(Debug)]
pub enum FsError {
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
