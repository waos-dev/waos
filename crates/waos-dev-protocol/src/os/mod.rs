//! What the transport needs of each system beyond tokio: on Windows, the security of the named
//! pipe (`windows`). A Unix socket needs nothing beyond the standard library's file permissions.

#[cfg(windows)]
pub(crate) mod windows;
