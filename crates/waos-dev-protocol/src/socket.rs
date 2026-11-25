//! The Unix socket a WaOS listens on, on this machine: where it is, connecting to it, and
//! listening on it. The socket is in the user's own folder, made for the user alone, so the
//! system's file permissions decide who may connect (the design's decision 30).
//!
//! The invariants a later editor will break:
//! - A WaOS removes a socket file only when nobody answers on it: its own on the way out, or
//!   one a WaOS that died left behind. While another WaOS answers, a second one is refused.
//! - The folder is made for the user alone, and the socket file too, so the path's permissions
//!   are what keep other users out, whatever the system does with a socket file's mode.

use std::fs::{self, DirBuilder, Permissions};
use std::io::{self, ErrorKind};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

use crate::connection::{Connection, Locality, StreamFrames, TransportFrames};
use crate::{ClientConnection, ServerConnection};

/// The stream of a connection `waos` made: a Unix socket's.
pub type ClientStream = UnixStream;

/// The stream of a connection a WaOS accepted: a Unix socket's, as the other end's.
pub type ServerStream = UnixStream;

/// Where WaOS listens on this machine, in the user's own folder: on Apple's systems
/// `~/Library/Application Support/WaOS/dev.sock`; elsewhere `$XDG_RUNTIME_DIR/waos/dev.sock`,
/// the folder a system keeps for the user's sockets, or `~/.local/share/waos/dev.sock` when
/// there is none. macOS allows a socket path 103 bytes long, so a user's name may be 50 bytes;
/// binding or connecting to a longer path fails with the system's message.
pub fn listener_path() -> PathBuf {
    user_folder().join(SOCKET_FILE)
}

const SOCKET_FILE: &str = "dev.sock";

#[cfg(target_vendor = "apple")]
fn user_folder() -> PathBuf {
    home().join("Library/Application Support/WaOS")
}

#[cfg(not(target_vendor = "apple"))]
fn user_folder() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime) if !runtime.is_empty() => PathBuf::from(runtime).join("waos"),
        _ => home().join(".local/share/waos"),
    }
}

/// The user's home, or the root when the system cannot say: a path there fails to bind or
/// connect with the system's message, which names the path.
fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

impl ClientConnection {
    /// Connects to the WaOS listening at `path`: `NotFound` or `ConnectionRefused` when none is.
    pub async fn connect(path: &Path) -> io::Result<ClientConnection> {
        Ok(local(UnixStream::connect(path).await?))
    }
}

/// Where a WaOS accepts `waos`'s connections. Dropping it removes its socket file, so a
/// WaOS that quit leaves none; one that died leaves a file the next WaOS replaces.
#[derive(Debug)]
pub struct Listener {
    listener: UnixListener,
    path: PathBuf,
}

impl Listener {
    /// Listens at `path`, making its folder for the user alone. A socket file left there by a
    /// WaOS that died is replaced; while another WaOS answers on it, this one is refused with
    /// `AddrInUse`.
    pub async fn bind(path: &Path) -> io::Result<Listener> {
        make_private_folder(path)?;
        clear_stale_socket(path).await?;
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, Permissions::from_mode(0o600))?;
        Ok(Listener {
            listener,
            path: path.to_owned(),
        })
    }

    /// The next connection: one run.
    pub async fn accept(&mut self) -> io::Result<ServerConnection> {
        let (stream, _) = self.listener.accept().await?;
        Ok(local(stream))
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        // A file already gone is what this wants.
        let _ = fs::remove_file(&self.path);
    }
}

/// Makes the socket's folder, for the user alone, where it is missing.
fn make_private_folder(socket: &Path) -> io::Result<()> {
    let Some(folder) = socket.parent() else {
        return Ok(());
    };
    DirBuilder::new().recursive(true).mode(0o700).create(folder)
}

/// Removes the socket file at `path` when nobody answers on it; refuses while a WaOS does. The
/// probe is a connection that closes at once, which that WaOS sees as a run that asked nothing.
async fn clear_stale_socket(path: &Path) -> io::Result<()> {
    match UnixStream::connect(path).await {
        Ok(_answering) => Err(io::Error::new(
            ErrorKind::AddrInUse,
            format!("another WaOS answers at {}", path.display()),
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == ErrorKind::ConnectionRefused => {
            tracing::info!(path = %path.display(), "replacing the socket file a WaOS that died left");
            fs::remove_file(path)
        }
        Err(error) => Err(error),
    }
}

/// A connection over `stream`, which stays on this machine.
fn local<S, N>(stream: S) -> Connection<TransportFrames<StreamFrames<S>, N>> {
    Connection::carried(
        TransportFrames::Local(StreamFrames::new(stream)),
        Locality::ThisMachine,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_is_in_the_user_folder_under_its_name() {
        let path = listener_path();
        #[cfg(target_vendor = "apple")]
        assert!(path.starts_with(home()), "{}", path.display());
        #[cfg(not(target_vendor = "apple"))]
        {
            let kept_for_the_user = match std::env::var_os("XDG_RUNTIME_DIR") {
                Some(runtime) if !runtime.is_empty() => PathBuf::from(runtime),
                _ => home().join(".local/share"),
            };
            assert_eq!(
                path.parent(),
                Some(kept_for_the_user.join("waos").as_path()),
                "{}",
                path.display()
            );
        }
        assert!(path.ends_with(SOCKET_FILE), "{}", path.display());
    }
}
