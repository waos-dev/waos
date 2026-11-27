//! One request to the WaOS on this machine about a program: for `waos ui screenshot`, `tap`,
//! `press`, `drag`, `type`, `key` and `scroll`, `waos attach` and `waos stop`, connected to as it
//! runs, never started, since only a program running is read, driven or stopped; for `waos
//! settings` and `set`, on a connection the caller made. Each side says its hello first: a
//! different protocol's major, or a request WaOS does not serve, is refused before it is sent, with
//! both builds named and which one to update.
//!
//! The invariant: [`ask`] never starts WaOS. With none answering, no program runs to ask about.

use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use waos_dev_protocol::{ClientConnection, Hello, Request};

use crate::connecting::{WaOSStarter, nobody_answers};
use crate::destination::Destination;

/// This build, as its hello names it: "waos 0.4.1".
pub const BUILD: &str = concat!("waos ", env!("CARGO_PKG_VERSION"));

/// What the WaOS running at `destination` answers `request`, its hello read and checked first.
pub async fn ask<A: DeserializeOwned>(destination: &Destination, request: &Request) -> Result<A> {
    answer(connect(destination).await?, destination, request).await
}

/// What the WaOS running at `destination`, or the one started on this machine when none answers,
/// answers `request`, its hello read and checked first: the settings are set before the run that
/// needs them.
pub async fn ask_starting<A: DeserializeOwned>(
    destination: &Destination,
    request: &Request,
    starter: &dyn WaOSStarter,
) -> Result<A> {
    let connection = crate::connecting::connect_or_start(destination, starter).await?;
    answer(connection, destination, request).await
}

/// What the WaOS at the other end of `connection`, at `destination`, answers `request`, its hello
/// read and checked first.
pub async fn answer<A: DeserializeOwned>(
    connection: ClientConnection,
    destination: &Destination,
    request: &Request,
) -> Result<A> {
    let mut connection = greeted(connection, destination, request).await?;
    connection.send(request).await.context("sending to WaOS")?;
    connection
        .receive::<A>()
        .await
        .context("reading from WaOS")?
        .context("WaOS closed the connection without an answer")
}

/// `connection`, to `destination`, with WaOS's hello read and checked against `request`, as this
/// build's: a mismatch is the error, in the words naming both builds and which one to update.
pub async fn greeted(
    connection: ClientConnection,
    destination: &Destination,
    request: &Request,
) -> Result<ClientConnection> {
    greeted_with(connection, &destination.hello(), request).await
}

/// `connection` with our hello, `ours`, said and WaOS's read and checked against `request`: a
/// mismatch is the error, in the words naming both builds and which one to update.
pub async fn greeted_with(
    mut connection: ClientConnection,
    ours: &Hello,
    request: &Request,
) -> Result<ClientConnection> {
    let theirs = connection
        .hello(ours)
        .await
        .context("the WaOS that answered")?;
    theirs
        .accepts(ours, request)
        .map_err(|mismatch| anyhow!("{mismatch}"))?;
    Ok(connection)
}

/// What a request to a WaOS came to: its answer, or nobody answering.
pub enum Unreached<A> {
    Answered(A),
    NobodyAnswers,
}

/// What the WaOS running at `destination` answers `request`, its hello read and checked first; or
/// that none answers, which for a list of what runs there is an answer of its own: nothing runs.
pub async fn ask_if_answered<A: DeserializeOwned>(
    destination: &Destination,
    request: &Request,
) -> Result<Unreached<A>> {
    match destination.connect().await {
        Ok(connection) => Ok(Unreached::Answered(
            answer(connection, destination, request).await?,
        )),
        Err(error) if nobody_answers(&error) => Ok(Unreached::NobodyAnswers),
        Err(error) => Err(error).with_context(|| format!("connecting to WaOS at {destination}")),
    }
}

/// A connection to the WaOS running at `destination`, never started.
pub async fn connect(destination: &Destination) -> Result<ClientConnection> {
    match destination.connect().await {
        Ok(connection) => Ok(connection),
        Err(error) if nobody_answers(&error) => match destination {
            Destination::ThisMachine { .. } => bail!(
                "no WaOS answers at {destination}: start the program with `waos run` or `waos launch` first"
            ),
            Destination::Device(_) => bail!(
                "no WaOS answers at {destination}: is WaOS open there, with Developer access on in its settings?"
            ),
        },
        Err(error) => Err(error).with_context(|| format!("connecting to WaOS at {destination}")),
    }
}

#[cfg(test)]
mod tests {
    use waos_dev_protocol::{Listener, MAJOR, ResizeAnswer, TestPlace};

    use super::*;

    fn resize() -> Request {
        Request::Resize {
            id: "dev.example.notes".parse().unwrap(),
            window: None,
            width: 390.0,
            height: 844.0,
        }
    }

    #[tokio::test]
    async fn a_request_not_in_serves_is_refused_before_it_is_sent() {
        let place = TestPlace::new("asking-1");
        let socket = place.path().to_owned();
        let mut listener = Listener::bind(&socket).await.unwrap();
        let served = tokio::spawn(async move {
            let mut server = listener.accept().await.unwrap();
            let older = Hello {
                serves: vec!["run".to_owned()],
                ..Hello::waos("WaOS 0.3.0")
            };
            server.accept_hello(&older).await.unwrap();
            server.receive::<Request>().await.unwrap()
        });
        let client = ClientConnection::connect(&socket).await.unwrap();
        let destination = Destination::ThisMachine { socket };
        let error = answer::<ResizeAnswer>(client, &destination, &resize())
            .await
            .unwrap_err();
        let said = error.to_string();
        assert!(said.contains("WaOS 0.3.0"), "{said}");
        assert!(said.contains("resize"), "{said}");
        assert!(said.contains("https://waos.app"), "{said}");
        assert_eq!(
            served.await.unwrap(),
            None,
            "the request was refused before anything was sent"
        );
    }

    #[tokio::test]
    async fn a_different_major_is_refused_before_anything_is_sent() {
        let place = TestPlace::new("asking-2");
        let socket = place.path().to_owned();
        let mut listener = Listener::bind(&socket).await.unwrap();
        let served = tokio::spawn(async move {
            let mut server = listener.accept().await.unwrap();
            let newer = Hello {
                major: MAJOR + 1,
                ..Hello::waos("WaOS 0.4.0")
            };
            server.accept_hello(&newer).await.unwrap();
            server.receive::<Request>().await.unwrap()
        });
        let client = ClientConnection::connect(&socket).await.unwrap();
        let destination = Destination::ThisMachine { socket };
        let error = answer::<ResizeAnswer>(client, &destination, &resize())
            .await
            .unwrap_err();
        let said = error.to_string();
        assert!(said.contains("WaOS 0.4.0"), "{said}");
        assert!(said.contains(BUILD), "{said}");
        assert!(said.contains("waos.app/install.sh"), "{said}");
        assert_eq!(served.await.unwrap(), None);
    }
}
