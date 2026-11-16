//! What a program's TCP and UDP sockets do, which [`super::host`] serves as `wasi:sockets@0.3`:
//! each socket the system's own, made non-blocking on tokio, and each address it uses checked by
//! the program's network at that use ([`ProgramNetwork::allows_socket`]) before the call that
//! reaches it: a bind, a connection, a listener, an accepted connection, a datagram sent or
//! received.
//!
//! Ported from `wasmtime-wasi` 50.0.1 (`src/sockets/mod.rs`, `src/sockets/tcp.rs` and
//! `src/sockets/udp.rs`), by the Wasmtime Project Developers, under the Apache License 2.0 with
//! the LLVM exception, which `network/LICENSE` holds. What differs: Preview 3 alone, its WIT
//! types used throughout; the program's network asked directly at each use, in place of the
//! embedder's check of every address; and TCP and UDP always served, the network deciding what
//! they reach.
//!
//! The invariant: no call that reaches an address is made before the program's network allowed
//! that address for that use, read from the grants as they are then; a refusal answers
//! `access-denied`, and a refused accepted connection or datagram is dropped before the program
//! sees it.

mod errors;
mod family;
mod options;
mod tcp;
mod udp;

use std::net::SocketAddr;

use wasmtime_wasi::sockets::SocketAddrUse;

use super::ProgramNetwork;
use super::host::bindings::wasi::sockets::types::ErrorCode;
pub use tcp::TcpSocket;
pub(crate) use tcp::{TcpListenStream, TcpReceiveStream, TcpSendStream};
pub use udp::UdpSocket;

/// Refuses `use_` of `address`, as `access-denied`, unless the program's network allows it.
fn check(
    network: &ProgramNetwork,
    address: SocketAddr,
    use_: SocketAddrUse,
) -> Result<(), ErrorCode> {
    if network.allows_socket(address, use_) {
        Ok(())
    } else {
        Err(ErrorCode::AccessDenied)
    }
}
