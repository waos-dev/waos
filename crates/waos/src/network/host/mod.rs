//! The code serving `wasi:sockets/ip-name-lookup@0.3.0` to a program, which Rust's `std::net`
//! calls before it connects to a name and a program awaits ([`p3`]): the lookup the program's
//! network allows, the system's resolver asked on a thread of tokio's for blocking work. The rest
//! of `wasi:sockets` is `wasmtime-wasi`'s, linked by `wasi`, each address checked by the same
//! network.
//!
//! The invariant: a lookup answers from [`ProgramNetwork::plan_lookup`] and
//! [`ProgramNetwork::resolve`], as the network's checks of an address do, so a name the network
//! refuses is refused however the program asks.

mod p3;

use wasmtime::component::{HasData, Linker};

use super::ProgramNetwork;

/// What the name lookup reads and writes in a program's store.
pub(crate) struct NetworkCtx<'a> {
    pub(crate) network: &'a ProgramNetwork,
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
    p3::add_to_linker(linker)
}
