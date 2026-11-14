//! One question to the WaOS on this machine about a program: for `cargo waos screenshot`, `tap`,
//! `press`, `drag`, `type`, `key` and `scroll`, connected to as it runs, never started, since only
//! a program a run of `cargo waos run` runs is read or driven (the design's decision 60); for
//! `cargo waos settings` and `set`, on a connection the caller made. Each the protocol agreed, the
//! question sent, and its one answer read.
//!
//! The invariant: [`ask`] never starts WaOS. With none answering, there is no run to ask about.

use anyhow::{Context, Result, bail};
use waos_dev_protocol::{ClientStream, Connection, FromWaOS, ToWaOS};

use crate::connecting::nobody_answers;

/// What the WaOS running answers `question`.
pub async fn ask(question: &ToWaOS) -> Result<FromWaOS> {
    let socket = waos_dev_protocol::listener_path();
    let connection = match Connection::connect(&socket).await {
        Ok(connection) => connection,
        Err(error) if nobody_answers(&error) => {
            bail!(
                "no WaOS answers at {}: run the program with `cargo waos run` first",
                socket.display()
            )
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("connecting to WaOS at {}", socket.display()));
        }
    };
    ask_on(connection, question).await
}

/// What the WaOS at the other end of `connection` answers `question`.
pub async fn ask_on(
    mut connection: Connection<ClientStream>,
    question: &ToWaOS,
) -> Result<FromWaOS> {
    connection
        .agree_on_protocol()
        .await
        .context("the WaOS that answered")?;
    connection.send(question).await.context("sending to WaOS")?;
    connection
        .receive::<FromWaOS>()
        .await
        .context("reading from WaOS")?
        .context("WaOS closed the connection without an answer")
}
