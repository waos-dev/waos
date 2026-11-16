//! The code serving `wasi:sockets@0.3.0` to a program, which Rust's `std::net` calls and a program
//! awaits: its name lookup ([`lookup`]), the lookup the program's network allows, the system's
//! resolver asked on a thread of tokio's for blocking work; and its TCP and UDP sockets ([`tcp`],
//! [`udp`]), the network's own ([`super::sockets`]), a connection's bytes moved through
//! component-model streams ([`streams`]). Its bindings are generated from the engine's copy of
//! `wit/`, whose `wasi-sockets-0.3.0` is the one `wasmtime-wasi` 50 serves; `wasi:clocks` is named
//! from `wasmtime-wasi`, which serves it.
//!
//! The sockets' part is ported from `wasmtime-wasi` 50.0.1 (`src/p3/sockets/host/types/tcp.rs`,
//! `src/p3/sockets/host/types/udp.rs` and `src/p3/sockets/conv.rs`), by the Wasmtime Project
//! Developers, under the Apache License 2.0 with the LLVM exception, which `network/LICENSE`
//! holds. What differs is a socket's checks, which [`super::sockets`] says.
//!
//! The invariant: this module only translates. What a call does is the sockets' and the
//! network's; here are WASI 0.3's types for it, its streams, and its handles in the program's
//! table. A lookup answers from [`ProgramNetwork::plan_lookup`] and [`ProgramNetwork::resolve`],
//! as the network's checks of an address do, so a name the network refuses is refused however
//! the program asks.

mod convert;
mod lookup;
mod streams;
mod tcp;
mod udp;

use wasmtime::component::{HasData, Linker, ResourceTable};

use super::ProgramNetwork;

pub(crate) mod bindings {
    wasmtime::component::bindgen!({
        inline: "
            package waos:network@0.3.0;

            /// What a program's `std` imports for its network on `wasm32-wasip3`, which WaOS
            /// serves itself (`crates/waos/src/network`).
            world network {
                import wasi:sockets/types@0.3.0;
                import wasi:sockets/ip-name-lookup@0.3.0;
            }
        ",
        path: "wit",
        require_store_data_send: true,
        imports: {
            "wasi:sockets/types@0.3.0.[method]tcp-socket.listen": store | tracing | trappable,
            "wasi:sockets/types@0.3.0.[method]tcp-socket.send": store | tracing | trappable,
            "wasi:sockets/types@0.3.0.[method]tcp-socket.receive": store | tracing | trappable,
            "wasi:sockets/types@0.3.0.[static]udp-socket.create": async | tracing | trappable,
            default: tracing | trappable,
        },
        trappable_error_type: {
            "wasi:sockets/types@0.3.0.error-code" => crate::network::host::SocketError,
        },
        with: {
            "wasi:clocks": wasmtime_wasi::p3::bindings::clocks,
            "wasi:sockets/types@0.3.0.tcp-socket": crate::network::sockets::TcpSocket,
            "wasi:sockets/types@0.3.0.udp-socket": crate::network::sockets::UdpSocket,
        },
    });
}

use bindings::wasi::sockets::types::ErrorCode;

/// What a `wasi:sockets` call fails with: WASI's error code for the program, or a trap.
pub type SocketError = wasmtime_wasi::TrappableError<ErrorCode>;

/// What a call answers.
type SocketResult<T> = Result<T, SocketError>;

/// What the network's interfaces read and write in a program's store: its network, and the table
/// its sockets' handles are in.
pub(crate) struct NetworkCtx<'a> {
    pub(crate) network: &'a ProgramNetwork,
    pub(crate) table: &'a mut ResourceTable,
}

pub(crate) trait NetworkCtxView: Send {
    fn network_ctx(&mut self) -> NetworkCtx<'_>;
}

struct HasNetwork;

impl HasData for HasNetwork {
    type Data<'a> = NetworkCtx<'a>;
}

pub(crate) fn add_to_linker<T: NetworkCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    bindings::wasi::sockets::types::add_to_linker::<_, HasNetwork>(linker, T::network_ctx)?;
    bindings::wasi::sockets::ip_name_lookup::add_to_linker::<_, HasNetwork>(linker, T::network_ctx)
}

impl bindings::wasi::sockets::types::Host for NetworkCtx<'_> {
    fn convert_error_code(&mut self, error: SocketError) -> wasmtime::Result<ErrorCode> {
        error.downcast()
    }
}
