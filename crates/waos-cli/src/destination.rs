//! Which WaOS a command goes to: the one on this machine, at its socket, or, with `--device` or
//! `WAOS_DEVICE`, a device's this computer is paired with, across the network over `wss://` to
//! the certificate it pinned, each hello carrying the device's token.
//!
//! The invariant a later editor will break: a device is reached only once paired, by the
//! fingerprint and token `devices.toml` keeps, so no command reaches one it does not recognize;
//! and WaOS is started only on this machine.

use std::fmt;
use std::io;
use std::path::PathBuf;

use anyhow::{Result, bail};
use waos_dev_protocol::{ClientConnection, Hello, connect_across_network};

use crate::asking::BUILD;
use crate::devices::{Devices, PairedDevice};

/// Which WaOS a command goes to.
pub enum Destination {
    /// The WaOS on this machine, listening at `socket`.
    ThisMachine { socket: PathBuf },
    /// A device's WaOS, across the network.
    Device(PairedDevice),
}

impl Destination {
    /// The WaOS on this machine, where it listens.
    pub fn this_machine() -> Destination {
        Destination::ThisMachine {
            socket: waos_dev_protocol::listener_path(),
        }
    }

    /// The WaOS `device` names, by a paired device's name or address, as `--device` gives it; the
    /// one on this machine with none.
    pub fn named(device: Option<&str>) -> Result<Destination> {
        let Some(named) = device else {
            return Ok(Destination::this_machine());
        };
        match Devices::of_this_user()?.find(named)? {
            Some(device) => Ok(Destination::Device(device)),
            None => bail!(
                "this computer is not paired with {named}: turn on Developer access in its WaOS's settings, then pair with `waos pair {named}`"
            ),
        }
    }

    /// A connection to it, its hellos still to come.
    pub async fn connect(&self) -> io::Result<ClientConnection> {
        match self {
            Destination::ThisMachine { socket } => ClientConnection::connect(socket).await,
            Destination::Device(device) => {
                let (connection, _) =
                    connect_across_network(&device.address, Some(device.fingerprint)).await?;
                Ok(connection)
            }
        }
    }

    /// This build's hello to it, with the device's token across the network.
    pub fn hello(&self) -> Hello {
        let mut hello = Hello::command(BUILD);
        if let Destination::Device(device) = self {
            hello.token = Some(device.token.clone());
        }
        hello
    }
}

impl fmt::Display for Destination {
    /// Where it is, as an error names it: the socket, or the device and its address.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Destination::ThisMachine { socket } => write!(formatter, "{}", socket.display()),
            Destination::Device(device) => {
                write!(formatter, "{} ({})", device.name, device.address)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use waos_dev_protocol::{Fingerprint, Identity, NetworkListener, Request, Token};

    use super::*;
    use crate::asking;

    /// A device of the test's own, on the loopback, answering one connection's hello: the
    /// device's address, its fingerprint, and the token the hello carried, once it came.
    async fn device() -> (String, Fingerprint, tokio::task::JoinHandle<Option<Token>>) {
        let identity = Identity::make().unwrap();
        let loopback = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let mut listener = NetworkListener::bind_at(&identity, loopback).await.unwrap();
        let address = format!("wss://127.0.0.1:{}", listener.port());
        let served = tokio::spawn(async move {
            let incoming = listener.accept().await.unwrap();
            let mut connection = incoming.handshake().await.ok()?;
            let theirs = connection
                .accept_hello(&Hello::waos("WaOS test"))
                .await
                .ok()?;
            theirs.token
        });
        (address, identity.fingerprint(), served)
    }

    fn paired(address: &str, fingerprint: Fingerprint) -> Destination {
        Destination::Device(PairedDevice {
            name: "test".to_owned(),
            address: address.parse().unwrap(),
            fingerprint,
            token: Token::new("the device's token"),
        })
    }

    #[tokio::test]
    async fn a_device_is_reached_at_its_pinned_certificate_with_its_token() {
        let (address, fingerprint, served) = device().await;
        let destination = paired(&address, fingerprint);
        let connection = destination.connect().await.unwrap();
        asking::greeted(connection, &destination, &Request::ServingPrograms)
            .await
            .unwrap();
        assert_eq!(
            served.await.unwrap(),
            Some(Token::new("the device's token"))
        );
    }

    #[tokio::test]
    async fn a_device_answering_with_another_certificate_is_refused() {
        let (address, _, served) = device().await;
        let destination = paired(&address, Fingerprint([7; 32]));
        let refused = destination.connect().await.unwrap_err();
        assert!(refused.to_string().contains("waos pair"), "{refused}");
        assert_eq!(served.await.unwrap(), None, "no hello reached it");
    }
}
