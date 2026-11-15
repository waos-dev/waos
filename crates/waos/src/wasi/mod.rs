//! The WASI a program's `std` imports on `wasm32-wasip3`, Preview 3, served by `wasmtime-wasi`
//! with nothing of this machine in it: its standard output goes to the output chosen for it, or to
//! `tracing` ([`output`]), its monotonic clock is the program's own ([`clock`]), its environment
//! holds `TMPDIR` alone, and its randomness is the system's. Its filesystem is not `wasmtime-wasi`'s: the engine
//! serves it from the program's mounts (`files`). Its TCP and UDP, which `std::net` and a program's
//! own awaits both use, are `wasmtime-wasi`'s sockets, each address they use checked by the
//! program's network (`network`), which also serves the name lookup itself. Preview 2 is not
//! served: nothing a program is built with imports it.
//!
//! The invariant: nothing here reaches outside the engine but through a check of the program's
//! grants. What a program reaches of this machine comes through its environment, which `files`,
//! `network` and the other interfaces serve.

mod clock;
mod output;

use std::sync::Arc;

use waos_capabilities::{OutputStream, ProgramOutput};
use wasmtime::component::Linker;
use wasmtime_wasi::sockets::{WasiSockets, WasiSocketsView};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use crate::clock::Clock;
use crate::network::ProgramNetwork;
use clock::MonotonicClock;
use output::StandardStream;

/// Every WASI interface a program may import that `wasmtime-wasi` serves.
pub(crate) fn add_to_linker<T: WasiView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    wasmtime_wasi::p3::clocks::add_to_linker(linker)?;
    wasmtime_wasi::p3::random::add_to_linker(linker)?;
    wasmtime_wasi::p3::cli::add_to_linker(linker)?;
    // The sockets' types alone: their name lookup is the network's own (`network`).
    wasmtime_wasi::p3::bindings::sockets::types::add_to_linker::<_, WasiSockets>(
        linker,
        T::sockets,
    )?;
    Ok(())
}

/// The WASI context the program `name` starts with: its standard streams to `output`, or to the
/// log under its name, the time on `clock`, TCP and UDP of both versions to what `network`
/// allows, and, where it has a temporary folder, `TMPDIR` naming it, `/tmp`.
pub(crate) fn context(
    name: &str,
    clock: Clock,
    output: Option<Arc<dyn ProgramOutput>>,
    network: &ProgramNetwork,
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
    ));
    wasi.stderr(StandardStream::new(name, OutputStream::Stderr, output));
    wasi.monotonic_clock(MonotonicClock::new(clock));
    let network = network.clone();
    wasi.allow_tcp(true)
        .allow_udp(true)
        .socket_addr_check(move |address, use_| {
            Box::pin(std::future::ready(network.allows_socket(address, use_)))
        });
    wasi.build()
}
