//! The code serving `wasi:sockets/ip-name-lookup` to a program, Preview 2's, which Rust's
//! `std::net` calls before it connects to a name ([`p2`]), and Preview 3's, which a program
//! awaits ([`p3`]): both the lookup the program's network allows, the system's resolver asked on a
//! thread of tokio's for blocking work. The rest of `wasi:sockets`, of both versions, is
//! `wasmtime-wasi`'s, linked by `wasi`, each address checked by the same network.
//!
//! The invariant: the two answer alike, each from [`ProgramNetwork::plan_lookup`] and
//! [`ProgramNetwork::resolve`], so a name one refuses the other refuses too.

mod p2;
mod p3;

use wasmtime::component::{HasData, Linker, ResourceTable};

use super::ProgramNetwork;
pub use p2::ResolveAddressStream;

/// What the name lookup reads and writes in a program's store.
pub(crate) struct NetworkCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
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
    p2::add_to_linker(linker)?;
    p3::add_to_linker(linker)
}
