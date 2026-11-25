//! Where the dev server takes connections from the network: a listener bound while developer
//! access is on and dropped once it is turned off, saying to developer access whether it listens
//! and on which port.
//!
//! The invariant a later editor will break: a connection from the network is served only while
//! access is on, so turning it off closes the listener before anything else is accepted.

use std::io;

use tokio::sync::watch;
use waos_dev_protocol::{Incoming, NetworkListener};

use super::developer_access::{DeveloperAccess, Listening, ListeningPlace};

/// The dev server's listener for the network, following developer access.
pub(crate) struct NetworkDoor {
    access: DeveloperAccess,
    turned: watch::Receiver<bool>,
    listener: Option<NetworkListener>,
    /// Whether access was on when it was last followed; none before the first time.
    followed: Option<bool>,
}

impl NetworkDoor {
    pub(crate) fn new(access: DeveloperAccess) -> NetworkDoor {
        let turned = access.turned();
        NetworkDoor {
            access,
            turned,
            listener: None,
            followed: None,
        }
    }

    /// The next connection from the network, its handshakes still to come; waits while access is
    /// off. Cancelling it loses nothing: a listener half bound is bound again.
    pub(crate) async fn next(&mut self) -> io::Result<Incoming> {
        loop {
            let on = *self.turned.borrow_and_update();
            if self.followed != Some(on) {
                self.follow(on).await;
            }
            let Some(listener) = &mut self.listener else {
                // The access outlives the door, so its sender never goes.
                let _ = self.turned.changed().await;
                continue;
            };
            tokio::select! {
                accepted = listener.accept() => return accepted,
                _ = self.turned.changed() => {}
            }
        }
    }

    /// Listens, or stops listening, as access is `on`, and says so.
    async fn follow(&mut self, on: bool) {
        self.listener = None;
        let listening = if on {
            match self.bind().await {
                Ok(listener) => {
                    let port = listener.port();
                    tracing::info!(port, "listening for developer access from the network");
                    self.listener = Some(listener);
                    Listening::On { port }
                }
                Err(error) => {
                    tracing::warn!(%error, "developer access cannot listen for the network");
                    Listening::Failed(error.to_string())
                }
            }
        } else {
            Listening::Not
        };
        self.access.listens(listening);
        self.followed = Some(on);
    }

    async fn bind(&self) -> io::Result<NetworkListener> {
        let identity = self.access.identity()?;
        match self.access.place() {
            ListeningPlace::EveryAddress => NetworkListener::bind(&identity).await,
            #[cfg(test)]
            ListeningPlace::Loopback => {
                let loopback = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0));
                NetworkListener::bind_at(&identity, loopback).await
            }
        }
    }
}
