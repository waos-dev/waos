//! The WaOS on this machine, connected to over `waos-dev-protocol`'s connection: at once when one
//! answers, and otherwise the installed WaOS started (`waos_app`) and waited for, for a run and for
//! a program's settings, which are WaOS's to keep whether or not a run is on.
//!
//! The invariant: a WaOS is started only when nothing answers where WaOS listens, never beside
//! one that does.

use std::io::{self, ErrorKind};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use waos_dev_protocol::{ClientStream, Connection};

use crate::waos_app;

/// How long a WaOS just started may take to answer, and how often it is tried.
const START_WAIT: Duration = Duration::from_secs(10);
const RETRY_EVERY: Duration = Duration::from_millis(200);

/// The WaOS on this machine: connected to at once when one answers, and started and waited for
/// otherwise.
pub async fn connect_or_start() -> Result<Connection<ClientStream>> {
    let socket = waos_dev_protocol::listener_path();
    match Connection::connect(&socket).await {
        Ok(connection) => return Ok(connection),
        Err(error) if nobody_answers(&error) => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("connecting to WaOS at {}", socket.display()));
        }
    }
    eprintln!("cargo waos: no WaOS answers; starting the installed WaOS");
    let log = waos_app::start()?;
    eprintln!("cargo waos: WaOS writes its log to {}", log.display());
    let deadline = Instant::now() + START_WAIT;
    loop {
        tokio::time::sleep(RETRY_EVERY).await;
        match Connection::connect(&socket).await {
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
