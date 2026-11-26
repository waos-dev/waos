//! The one place a wgpu call that `wasdk:gpu` leaves out becomes a panic, as a call wgpu itself
//! refuses panics. What is left out, and why, is in the crate documentation.

use std::fmt::Display;

/// `what` is wgpu's and not `wasdk:gpu`'s.
#[track_caller]
pub(crate) fn left_out(what: impl Display) -> ! {
    panic!("wasdk-wgpu: {what} is left out of wasdk:gpu (see the crate documentation)")
}
