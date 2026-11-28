//! The WaOS a command goes to, connected to over `waos-dev-protocol`'s connection: at once when one
//! answers, and otherwise, on this machine, started through the caller's [`WaOSStarter`] and
//! waited for, for a run and for a program's settings, which are WaOS's to keep whether or not a
//! run is on.
//!
//! The invariant: a WaOS is started only on this machine, when nothing answers where WaOS listens,
//! never beside one that does.

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use waos_dev_protocol::ClientConnection;

use crate::destination::Destination;
use crate::waos_app;

/// How long a WaOS just started may take to answer, and how often it is tried.
const START_WAIT: Duration = Duration::from_secs(10);
const RETRY_EVERY: Duration = Duration::from_millis(200);

/// How the CLI starts WaOS when nothing answers on the dev socket.
pub trait WaOSStarter: Send + Sync {
    /// Starts WaOS in the background and returns the path to its log file.
    fn start(&self) -> Result<PathBuf>;
}

/// The standalone CLI's starter: launches the installed WaOS app (or headless `waos` on `PATH`
/// when on headless Linux).
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultStarter;

impl WaOSStarter for DefaultStarter {
    fn start(&self) -> Result<PathBuf> {
        waos_app::start()
    }
}

/// The WaOS at `destination`: connected to at once when one answers, and, on this machine,
/// started via `starter` and waited for otherwise.
pub async fn connect_or_start(
    destination: &Destination,
    starter: &dyn WaOSStarter,
) -> Result<ClientConnection> {
    match destination {
        Destination::ThisMachine { socket } => connect_or_start_at(socket, starter).await,
        Destination::Device(_) => destination
            .connect()
            .await
            .with_context(|| format!("connecting to WaOS at {destination}")),
    }
}

/// The WaOS listening at `socket`: connected to at once when one answers, and started via
/// `starter` and waited for otherwise.
async fn connect_or_start_at(socket: &Path, starter: &dyn WaOSStarter) -> Result<ClientConnection> {
    match ClientConnection::connect(socket).await {
        Ok(connection) => return Ok(connection),
        Err(error) if nobody_answers(&error) => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("connecting to WaOS at {}", socket.display()));
        }
    }
    eprintln!("waos: no WaOS answers; starting WaOS");
    let log = starter.start()?;
    eprintln!("waos: WaOS writes its log to {}", log.display());
    let deadline = Instant::now() + START_WAIT;
    loop {
        tokio::time::sleep(RETRY_EVERY).await;
        match ClientConnection::connect(socket).await {
            Ok(connection) => return Ok(connection),
            Err(error) if nobody_answers(&error) && Instant::now() < deadline => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "WaOS was started, but nothing answers at {}",
                        socket.display()
                    )
                });
            }
        }
    }
}

/// Whether `error` says no WaOS listens: no socket file, or one nobody answers on.
pub fn nobody_answers(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::NotFound | ErrorKind::ConnectionRefused
    )
}
