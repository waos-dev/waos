//! The WIT's copies. `wit/` is the one place the WIT is edited; each crate that generates
//! bindings from it carries a copy of its own, so the crate builds from what it publishes, and the
//! repository checks out on Windows, where Git makes a symbolic link a plain file (the design's
//! decision 57, as wasmtime's crates carry theirs, copied by its `ci/vendor-wit.sh`).
//!
//! The invariant a later editor will break: [`COPIES`] names every copy. A crate whose
//! `generate!` or `bindgen!` reads WIT taken from `wit/` is listed here, and nowhere edits its
//! copy: `cargo xtask sync-wit` replaces each one whole, and `tests/wit_copies.rs` names every
//! file of a copy that differs from `wit/`.

use crate::folder_copy::FolderCopy;

/// Every copy of the WIT, each a crate's that generates bindings from it.
pub const COPIES: [FolderCopy; 3] = [
    // The SDK's, which `wit_bindgen::generate!` makes the program's API of.
    FolderCopy {
        source: "wit",
        destination: "sdk/wasdk/wit",
        left_out: &[],
    },
    // The engine's, which `wasmtime::component::bindgen!` makes the host's traits of.
    FolderCopy {
        source: "wit",
        destination: "crates/waos/wit",
        left_out: &[],
    },
    // The WebGPU host's `wasi:webgpu`, whose bindings the engine's reuse for that package, so
    // the two are generated from the same WIT.
    FolderCopy {
        source: "wit/deps/wasi-webgpu-0.3.0-rc.2",
        destination: "crates/waos-webgpu-host/wit/deps/wasi-webgpu-0.3.0-rc.2",
        left_out: &[],
    },
];
