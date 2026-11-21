//! The named pipe a WaOS listens on, on Windows: where it is, connecting to it, and listening on
//! it. Windows has no folder of the user's own for it, so the pipe is named after the user and
//! guarded as the Unix socket's folder guards it (the design's decision 30): the pipe's security
//! descriptor makes the user its owner and gives the user alone access, so no other user may
//! connect, or read what WaOS writes, as the default descriptor lets everyone; and `waos`
//! talks only to a pipe the user owns, since any user may make a pipe of any free name, and one
//! made before WaOS starts would otherwise receive the program.
//!
//! The invariants a later editor will break:
//! - A WaOS makes the pipe's first instance as the first (`FILE_FLAG_FIRST_PIPE_INSTANCE`), so it
//!   never joins a pipe another process made under the name; while another WaOS has it, a second
//!   one is refused. The pipe goes when its last instance closes: nothing is left to clear.
//! - One instance always waits for the next connection: the next is made as soon as one connects,
//!   before it is handed out, so a `waos` finds the pipe busy for a moment at most.
//! - The pipe takes no client from another machine (`PIPE_REJECT_REMOTE_CLIENTS`, tokio's
//!   default), and a client lets the server identify it but never act as it (tokio's default
//!   `SECURITY_IDENTIFICATION`).

use std::ffi::OsStr;
use std::io::{self, ErrorKind};
use std::os::windows::io::AsHandle;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY};

use crate::connection::Connection;
use crate::os::windows::{UserOnly, owner_sid, user_sid};

/// The stream of a connection `waos` made: a named pipe's client end.
pub type ClientStream = NamedPipeClient;

/// The stream of a connection a WaOS accepted: an instance of its named pipe.
pub type ServerStream = NamedPipeServer;

/// How long a `waos` waits for the pipe while every instance is busy, which is the moment
/// between a connection and the next instance at most.
const BUSY_WAIT: Duration = Duration::from_secs(2);
const RETRY_EVERY: Duration = Duration::from_millis(20);

/// Where WaOS listens on this machine: `\\.\pipe\waos-dev-<SID>`, named after the user by their
/// SID, so each user's WaOS has a pipe of its own. A process that cannot read its own user, which
/// the system always lets it, gets a name with none, at which binding and connecting fail, as
/// they read the user too.
pub fn listener_path() -> PathBuf {
    let user = user_sid().unwrap_or_default();
    PathBuf::from(format!(r"\\.\pipe\waos-dev-{user}"))
}

impl Connection<ClientStream> {
    /// Connects to the WaOS listening at `path`: `NotFound` when none is, and `PermissionDenied`
    /// when the pipe there is another user's.
    pub async fn connect(path: &Path) -> io::Result<Connection<ClientStream>> {
        let pipe = open_when_free(path.as_os_str()).await?;
        refuse_another_users(&pipe, path)?;
        Ok(Connection::new(pipe))
    }
}

/// The pipe at `path`, opened once an instance is free, or the system's answer.
async fn open_when_free(path: &OsStr) -> io::Result<NamedPipeClient> {
    let deadline = Instant::now() + BUSY_WAIT;
    loop {
        match ClientOptions::new().open(path) {
            Err(error) if is_busy(&error) && Instant::now() < deadline => {}
            opened => return opened,
        }
        tokio::time::sleep(RETRY_EVERY).await;
    }
}

fn is_busy(error: &io::Error) -> bool {
    error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
}

/// Refuses the pipe `pipe`, opened at `path`, unless this user owns it, as the WaOS's own does.
fn refuse_another_users(pipe: &NamedPipeClient, path: &Path) -> io::Result<()> {
    if owner_sid(pipe.as_handle())? == user_sid()? {
        return Ok(());
    }
    Err(io::Error::new(
        ErrorKind::PermissionDenied,
        format!(
            "the pipe at {} is another user's, not a WaOS of yours",
            path.display()
        ),
    ))
}

/// Where a WaOS accepts `waos`'s connections: the pipe's instance waiting for the next one.
/// Dropping it closes that instance; the pipe goes with the last one.
pub struct Listener {
    waiting: NamedPipeServer,
    security: UserOnly,
    path: PathBuf,
}

impl std::fmt::Debug for Listener {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Listener")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Listener {
    /// Listens at `path`, the pipe's first instance, for this user alone. While another WaOS has
    /// the pipe, or any process made one of its name, this one is refused with `AddrInUse`.
    pub async fn bind(path: &Path) -> io::Result<Listener> {
        let security = UserOnly::new(&user_sid()?)?;
        let waiting = security
            .create_pipe(
                ServerOptions::new().first_pipe_instance(true),
                path.as_os_str(),
            )
            .map_err(|error| refused_as_second(error, path))?;
        Ok(Listener {
            waiting,
            security,
            path: path.to_owned(),
        })
    }

    /// The next connection: one run. The next instance is made before it is handed out.
    pub async fn accept(&mut self) -> io::Result<Connection<ServerStream>> {
        self.waiting.connect().await?;
        let next = self
            .security
            .create_pipe(&ServerOptions::new(), self.path.as_os_str())?;
        Ok(Connection::new(std::mem::replace(&mut self.waiting, next)))
    }
}

/// `error`, the system's refusal of a first instance, as `AddrInUse` where the name is taken.
fn refused_as_second(error: io::Error, path: &Path) -> io::Error {
    if error.raw_os_error() != Some(ERROR_ACCESS_DENIED as i32) {
        return error;
    }
    io::Error::new(
        ErrorKind::AddrInUse,
        format!(
            "another WaOS answers at {}, or another program made a pipe of its name",
            path.display()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::windows::{as_the_system_writes_it, owner_and_access_text};

    #[test]
    fn the_pipe_is_named_after_the_user() {
        let path = listener_path();
        let user = user_sid().unwrap();
        assert!(user.starts_with("S-1-"), "{user}");
        assert_eq!(path, PathBuf::from(format!(r"\\.\pipe\waos-dev-{user}")));
    }

    #[tokio::test]
    async fn the_user_owns_the_pipe_and_alone_may_open_it() {
        let path = PathBuf::from(format!(
            r"\\.\pipe\waos-dev-test-{}-security",
            std::process::id()
        ));
        let listener = Listener::bind(&path).await.unwrap();
        let user = user_sid().unwrap();
        let security = owner_and_access_text(listener.waiting.as_handle()).unwrap();
        // `FA`, every right a pipe has, is what `GA` comes to on one.
        let expected = as_the_system_writes_it(&format!("O:{user}D:P(A;;FA;;;{user})")).unwrap();
        assert_eq!(security, expected);
    }
}
