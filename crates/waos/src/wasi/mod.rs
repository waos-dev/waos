//! The WASI a program's `std` imports on `wasm32-wasip2`, served by `wasmtime-wasi` with nothing
//! of this machine in it: its standard output goes to the output chosen for it, or to `tracing`
//! ([`output`]), its monotonic clock is the program's own ([`clock`]), its environment is empty,
//! and its randomness is the system's. Its filesystem is not `wasmtime-wasi`'s: the engine serves
//! it from the program's mounts (`files`). Its TCP and UDP are, Preview 2's, which `std::net`
//! imports, and Preview 3's, which a program awaits, both over the same sockets of `wasmtime-wasi`,
//! each address they use checked by the program's network (`network`), which also serves the name
//! lookup of both itself. Of the rest of Preview 3, only the clocks, which the world imports for
//! the time a view's events are stamped on.
//!
//! The invariant: nothing here reaches outside the engine but through a check of the program's
//! grants. What a program reaches of this machine comes through its environment, which `files`,
//! `network` and the other interfaces serve.

mod clock;
mod output;

use std::sync::Arc;

use waos_capabilities::{OutputStream, ProgramOutput};
use wasmtime::component::Linker;
use wasmtime_wasi::cli::{WasiCli, WasiCliView};
use wasmtime_wasi::clocks::{WasiClocks, WasiClocksView};
use wasmtime_wasi::p2::bindings::{cli, clocks, random, sockets};
use wasmtime_wasi::random::{WasiRandom, WasiRandomView};
use wasmtime_wasi::sockets::{WasiSockets, WasiSocketsView};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};
use wasmtime_wasi_io::IoView;

use crate::clock::Clock;
use crate::network::ProgramNetwork;
use clock::MonotonicClock;
use output::StandardStream;

/// Every WASI interface a program may import that `wasmtime-wasi` serves, one at a time.
pub(crate) fn add_to_linker<T>(linker: &mut Linker<T>) -> wasmtime::Result<()>
where
    T: WasiView + IoView + 'static,
{
    wasmtime_wasi_io::add_to_linker_async(linker)?;
    clocks::wall_clock::add_to_linker::<_, WasiClocks>(linker, T::clocks)?;
    clocks::monotonic_clock::add_to_linker::<_, WasiClocks>(linker, T::clocks)?;
    random::random::add_to_linker::<_, WasiRandom>(linker, T::random)?;
    random::insecure::add_to_linker::<_, WasiRandom>(linker, T::random)?;
    random::insecure_seed::add_to_linker::<_, WasiRandom>(linker, T::random)?;
    cli::exit::add_to_linker::<_, WasiCli>(linker, T::cli)?;
    cli::environment::add_to_linker::<_, WasiCli>(linker, T::cli)?;
    cli::stdin::add_to_linker::<_, WasiCli>(linker, T::cli)?;
    cli::stdout::add_to_linker::<_, WasiCli>(linker, T::cli)?;
    cli::stderr::add_to_linker::<_, WasiCli>(linker, T::cli)?;
    cli::terminal_input::add_to_linker::<_, WasiCli>(linker, T::cli)?;
    cli::terminal_output::add_to_linker::<_, WasiCli>(linker, T::cli)?;
    cli::terminal_stdin::add_to_linker::<_, WasiCli>(linker, T::cli)?;
    cli::terminal_stdout::add_to_linker::<_, WasiCli>(linker, T::cli)?;
    cli::terminal_stderr::add_to_linker::<_, WasiCli>(linker, T::cli)?;
    let network_options = sockets::network::LinkOptions::default();
    sockets::network::add_to_linker::<_, WasiSockets>(linker, &network_options, T::sockets)?;
    sockets::instance_network::add_to_linker::<_, WasiSockets>(linker, T::sockets)?;
    sockets::tcp_create_socket::add_to_linker::<_, WasiSockets>(linker, T::sockets)?;
    sockets::tcp::add_to_linker::<_, WasiSockets>(linker, T::sockets)?;
    sockets::udp_create_socket::add_to_linker::<_, WasiSockets>(linker, T::sockets)?;
    sockets::udp::add_to_linker::<_, WasiSockets>(linker, T::sockets)?;
    wasmtime_wasi::p3::bindings::sockets::types::add_to_linker::<_, WasiSockets>(
        linker,
        T::sockets,
    )?;
    wasmtime_wasi::p3::clocks::add_to_linker(linker)?;
    Ok(())
}

/// The WASI context the program `name` starts with: its standard streams to `output`, or to the
/// log under its name, the time on `clock`, TCP and UDP of both versions to what `network`
/// allows, and, where it has its own folders, its temporary folder in `/cache`.
pub(crate) fn context(
    name: &str,
    clock: Clock,
    output: Option<Arc<dyn ProgramOutput>>,
    network: &ProgramNetwork,
    has_own_folders: bool,
) -> WasiCtx {
    let mut wasi = WasiCtxBuilder::new();
    if has_own_folders {
        // Where `std::env::temp_dir` leads, so a temporary file can be renamed onto a pick (the
        // file dialogs proposal's decision 3).
        wasi.env("TMPDIR", "/cache");
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
