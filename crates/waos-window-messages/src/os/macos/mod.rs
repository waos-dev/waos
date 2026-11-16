//! The messages over Mach, as WaOS and a program's window process speak them on macOS: rights to
//! ports (`ports`), found by a bootstrap name or carried in a message; and messages of bytes and
//! ports, each received with its sender's audit token (`mach_messages`).
//!
//! The invariant: wrapping Mach and the C library is what this module is for, and it is the one
//! place in the crate where `unsafe` is written.

#![allow(unsafe_code)]

mod mach_messages;
mod ports;

pub use mach_messages::{Arrival, AuditToken, Received};
pub use ports::{ReceiveRight, SendRight};
