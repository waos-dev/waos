//! This computer's cameras as a capability: the provider of the cameras trait of
//! `waos-capabilities` for the machine in front of the user, with each OS's code inside.
//!
//! The invariant: this crate depends on `waos-capabilities` only. It names no shell, no engine and
//! no program's internals; a program gets it when its environment is chosen, at start.
