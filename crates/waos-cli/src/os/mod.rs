//! What `waos` needs of each system beyond the standard library: on Windows, a folder and a
//! file for the user alone and the registry's record of where WaOS is installed (`windows`).

#[cfg(windows)]
pub(crate) mod windows;
