//! What a program's TCP and UDP sockets do, which [`super::host`] serves as `wasi:sockets@0.3`:
//! each socket the system's own, made non-blocking on tokio, and each address it uses checked by
//! the program's network at that use before the call that reaches it: a connection
//! ([`super::ProgramNetwork::allows_connect`]), a listener, an accepted connection, a datagram
//! sent or received. A bind reaches nothing, and asks nothing. Each socket open is a use the
//! program makes now, which counts the bytes it moves ([`open_sockets`]).
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
mod open_sockets;
mod options;
mod tcp;
mod udp;

use super::host::bindings::wasi::sockets::types::ErrorCode;
pub(crate) use open_sockets::{Connection, Counts, Datagrams};
pub use tcp::TcpSocket;
pub(crate) use tcp::{TcpListenStream, TcpReceiveStream, TcpSendStream};
pub use udp::UdpSocket;

/// What a use the program's network refused answers: `access-denied`.
fn refused_unless(allowed: bool) -> Result<(), ErrorCode> {
    if allowed {
        Ok(())
    } else {
        Err(ErrorCode::AccessDenied)
    }
}
