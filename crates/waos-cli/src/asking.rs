//! One request to the WaOS on this machine about a program: for `waos ui screenshot`, `tap`,
//! `press`, `drag`, `type`, `key` and `scroll`, connected to as it runs, never started, since only
//! a program a run of `waos run` runs is read or driven (the design's decision 60); for
//! `waos settings` and `set`, on a connection the caller made. Each side says its hello
//! first: a different protocol's major, or a request WaOS does not serve, is refused before it is
//! sent, with both builds named and which one to update.
//!
//! The invariant: [`ask`] never starts WaOS. With none answering, there is no run to ask about.

use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use waos_dev_protocol::{ClientStream, Connection, Hello, Request};

use crate::connecting::{WaOSStarter, nobody_answers};

/// This build, as its hello names it: "waos 0.4.1".
pub const BUILD: &str = concat!("waos ", env!("CARGO_PKG_VERSION"));

/// What the WaOS running answers `request`, its hello read and checked first.
pub async fn ask<A: DeserializeOwned>(request: &Request) -> Result<A> {
    answer(connect().await?, request).await
}

/// What the WaOS running, or the one started when none answers, answers `request`, its hello read
/// and checked first: the settings are set before the run that needs them.
pub async fn ask_starting<A: DeserializeOwned>(
    request: &Request,
    starter: &dyn WaOSStarter,
) -> Result<A> {
    answer(crate::connecting::connect_or_start(starter).await?, request).await
}

/// What the WaOS at the other end of `connection` answers `request`, its hello read and checked
/// first.
pub async fn answer<A: DeserializeOwned>(
    connection: Connection<ClientStream>,
    request: &Request,
) -> Result<A> {
    let mut connection = greeted(connection, request).await?;
    connection.send(request).await.context("sending to WaOS")?;
    connection
        .receive::<A>()
        .await
        .context("reading from WaOS")?
        .context("WaOS closed the connection without an answer")
}

/// `connection` with WaOS's hello read and checked against `request`, as this build's: a mismatch
/// is the error, in the words naming both builds and which one to update.
pub async fn greeted(
    mut connection: Connection<ClientStream>,
    request: &Request,
) -> Result<Connection<ClientStream>> {
    let ours = Hello::cargo_waos(BUILD);
    let theirs = connection
        .hello(&ours)
        .await
        .context("the WaOS that answered")?;
    theirs
        .accepts(&ours, request)
        .map_err(|mismatch| anyhow!("{mismatch}"))?;
    Ok(connection)
}

/// A connection to the WaOS running, never started.
pub async fn connect() -> Result<Connection<ClientStream>> {
    let socket = waos_dev_protocol::listener_path();
    match Connection::connect(&socket).await {
        Ok(connection) => Ok(connection),
        Err(error) if nobody_answers(&error) => {
            bail!(
                "no WaOS answers at {}: run the program with `waos run` first",
                socket.display()
            )
        }
        Err(error) => {
            Err(error).with_context(|| format!("connecting to WaOS at {}", socket.display()))
        }
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
        let client = Connection::connect(&socket).await.unwrap();
        let error = answer::<ResizeAnswer>(client, &resize()).await.unwrap_err();
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
        let client = Connection::connect(&socket).await.unwrap();
        let error = answer::<ResizeAnswer>(client, &resize()).await.unwrap_err();
        let said = error.to_string();
        assert!(said.contains("WaOS 0.4.0"), "{said}");
        assert!(said.contains(BUILD), "{said}");
        assert!(said.contains("waos.app/install.sh"), "{said}");
        assert_eq!(served.await.unwrap(), None);
    }
}
