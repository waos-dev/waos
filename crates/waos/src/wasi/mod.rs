//! The WASI a program's `std` imports on `wasm32-wasip3`, Preview 3, served by `wasmtime-wasi`
//! with nothing of this machine in it: its standard output goes to the output chosen for it, or to
//! `tracing` ([`output`]), its monotonic clock is the program's own ([`clock`]), its environment
//! holds `TMPDIR` alone, and its randomness is the system's. Its filesystem is not `wasmtime-wasi`'s: the engine
//! serves it from the program's mounts (`files`), and its sockets from the program's network
//! (`network`). Preview 2 is not served: nothing a program is built with imports it.
//!
//! The invariant: nothing here reaches outside the engine but through a check of the program's
//! grants. What a program reaches of this machine comes through its environment, which `files`,
//! `network` and the other interfaces serve.

mod clock;
mod output;

use std::sync::Arc;

use waos_capabilities::{OutputStream, ProgramOutput};
use wasmtime::component::Linker;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use crate::access::ProgramAccess;
use crate::clock::Clock;
use clock::MonotonicClock;
use output::StandardStream;

/// Every WASI interface a program may import that `wasmtime-wasi` serves.
pub(crate) fn add_to_linker<T: WasiView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    wasmtime_wasi::p3::clocks::add_to_linker(linker)?;
    wasmtime_wasi::p3::random::add_to_linker(linker)?;
    wasmtime_wasi::p3::cli::add_to_linker(linker)?;
    Ok(())
}

/// The WASI context the program `name` starts with: its standard streams to `output`, or to the
/// log under its name, each write told through its `access`, the time on `clock`, and, where it
/// has a temporary folder, `TMPDIR` naming it, `/tmp`.
pub(crate) fn context(
    name: &str,
    clock: Clock,
    output: Option<Arc<dyn ProgramOutput>>,
    access: &ProgramAccess,
    has_temporary_folder: bool,
) -> WasiCtx {
    let mut wasi = WasiCtxBuilder::new();
    if has_temporary_folder {
        // What a program that names its temporary folder reads: `std::env::temp_dir` reads
        // nothing on WASI, and tempfile takes it through `override_temp_dir`.
        wasi.env("TMPDIR", "/tmp");
    }
    wasi.stdout(StandardStream::new(
        name,
        OutputStream::Stdout,
        output.clone(),
        access.clone(),
    ));
    wasi.stderr(StandardStream::new(
        name,
        OutputStream::Stderr,
        output,
        access.clone(),
    ));
    wasi.monotonic_clock(MonotonicClock::new(clock));
    wasi.build()
}
