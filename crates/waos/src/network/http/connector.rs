//! How a program's request is connected, a connection of the program's own (`sockets`) under
//! each, so its bytes are counted as its sockets' are. Straight to its host ([`Way::Direct`]): an
//! address as itself, a host pinned to what the program's lookup answered as that answer
//! (`pins`), any other name through the program's checked lookup, which answers only the
//! addresses it may reach; the first tried at once and each next beside it after RFC 8305's
//! connection attempt delay, the first to connect winning. Through the proxy in effect for it
//! ([`Way::Proxied`]): the proxy's host resolved unchecked, since WaOS chose it rather than the
//! program; then an HTTPS request tunnelled through an HTTP proxy with `CONNECT`, any request
//! through a SOCKS 5 proxy asked for its host by name, and a plain HTTP request sent to an HTTP
//! proxy whole. An HTTPS request is in TLS end to end, checked as the system checks a server's
//! certificate; a proxy named `https` is reached in TLS too.
//!
//! The invariant: a connection made for a request goes to an address the program's network
//! answered for the request's host, or to the proxy a request was routed through; a name nothing
//! named is never resolved unchecked.

use std::error::Error as StdError;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::stream::{FuturesUnordered, StreamExt};
use http::Uri;
use hyper_util::client::legacy::connect::proxy::{SocksV5, Tunnel};
use hyper_util::rt::TokioIo;
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use tower_service::Service;
use wasmtime_wasi::runtime::spawn_blocking;

use super::pins::Pins;
use super::proxies::Proxies;
use super::wire::{Stream, Wire};
use crate::access::Cause;
use crate::network::sockets::Connection;
use crate::network::{LookupError, ProgramNetwork, resolve_unchecked};

/// What a connection fails with, as hyper takes it.
type BoxError = Box<dyn StdError + Send + Sync>;

/// A connection being made, as hyper takes one.
type Connecting<T> = Pin<Box<dyn Future<Output = Result<T, BoxError>> + Send>>;

/// What connects a client's requests: the program's network, whose connections they are; the
/// way they go; and the TLS they speak.
#[derive(Clone)]
pub(super) struct Connector {
    network: ProgramNetwork,
    way: Way,
    tls: Arc<ClientConfig>,
}

/// The way a client's requests go.
#[derive(Clone)]
enum Way {
    /// Straight to each request's host, at the addresses the program's network answers.
    Direct { pins: Arc<Pins> },
    /// Through the proxy in effect for each request.
    Proxied { proxies: Arc<Proxies> },
}

/// What connects to a proxy, which hyper-util's tunnels are given.
#[derive(Clone)]
struct ProxyDialer {
    network: ProgramNetwork,
    tls: Arc<ClientConfig>,
}

impl Connector {
    /// Straight to each request's host, a pinned one at its pin.
    pub(super) fn direct(
        network: ProgramNetwork,
        pins: Arc<Pins>,
        tls: Arc<ClientConfig>,
    ) -> Connector {
        Connector {
            network,
            way: Way::Direct { pins },
            tls,
        }
    }

    /// Through the proxies of `proxies`.
    pub(super) fn proxied(
        network: ProgramNetwork,
        proxies: Arc<Proxies>,
        tls: Arc<ClientConfig>,
    ) -> Connector {
        Connector {
            network,
            way: Way::Proxied { proxies },
            tls,
        }
    }

    async fn connect(self, uri: Uri) -> Result<Wire, BoxError> {
        match &self.way {
            Way::Direct { pins } => {
                let host = host_of(&uri)?;
                let addresses = addresses_of(&self.network, pins, host).await?;
                let connection = dial(&addresses, port_of(&uri), &self.network).await?;
                let stream = secured(&self.tls, Box::new(connection), &uri).await?;
                Ok(Wire::new(stream, false))
            }
            Way::Proxied { proxies } => self.through_proxy(proxies, uri).await,
        }
    }

    /// A connection for `uri` through the proxy `proxies` have in effect for it.
    async fn through_proxy(&self, proxies: &Proxies, uri: Uri) -> Result<Wire, BoxError> {
        let intercept = proxies
            .intercept(&uri)
            .ok_or("no proxy is in effect for the request")?;
        let proxy = intercept.uri().clone();
        let dialer = ProxyDialer {
            network: self.network.clone(),
            tls: Arc::clone(&self.tls),
        };
        match proxy.scheme_str() {
            Some("socks5" | "socks5h") => {
                let mut socks = SocksV5::new(proxy, dialer);
                if let Some((user, password)) = intercept.raw_auth() {
                    socks = socks.with_auth(user.to_owned(), password.to_owned());
                }
                // hyper-util takes a destination without a port as HTTPS's.
                let tunnelled = socks.call(with_port(&uri)?).await?;
                let stream = secured(&self.tls, tunnelled.into_inner(), &uri).await?;
                Ok(Wire::new(stream, false))
            }
            Some("http" | "https") if is_https(&uri) => {
                let mut tunnel = Tunnel::new(proxy, dialer);
                if let Some(authorization) = intercept.basic_auth() {
                    tunnel = tunnel.with_auth(authorization.clone());
                }
                let tunnelled = tunnel.call(uri.clone()).await?;
                let stream = secured(&self.tls, tunnelled.into_inner(), &uri).await?;
                Ok(Wire::new(stream, false))
            }
            Some("http" | "https") => Ok(Wire::new(dialer.dial(proxy).await?, true)),
            _ => Err(format!("a proxy WaOS does not speak to: {proxy}").into()),
        }
    }
}

impl Service<Uri> for Connector {
    type Response = Wire;
    type Error = BoxError;
    type Future = Connecting<Wire>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        Box::pin(self.clone().connect(uri))
    }
}

impl ProxyDialer {
    /// A connection to the proxy at `proxy`, its host resolved as the system resolves it, in TLS
    /// for a proxy named `https`.
    async fn dial(self, proxy: Uri) -> Result<Box<dyn Stream>, BoxError> {
        let host = host_of(&proxy)?.to_owned();
        let addresses = spawn_blocking(move || resolve_unchecked(&host)).await?;
        let connection = dial(&addresses, port_of(&proxy), &self.network).await?;
        secured(&self.tls, Box::new(connection), &proxy).await
    }
}

impl Service<Uri> for ProxyDialer {
    type Response = TokioIo<Box<dyn Stream>>;
    type Error = BoxError;
    type Future = Connecting<TokioIo<Box<dyn Stream>>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, proxy: Uri) -> Self::Future {
        let dialing = self.clone().dial(proxy);
        Box::pin(async move { Ok(TokioIo::new(dialing.await?)) })
    }
}

/// The addresses a request for `host` connects to: an address as itself, a pinned host as its
/// pin, any other name as the program's lookup answers it, only those it may reach.
async fn addresses_of(
    network: &ProgramNetwork,
    pins: &Pins,
    host: &str,
) -> Result<Vec<IpAddr>, LookupError> {
    if let Ok(address) = bare(host).parse::<IpAddr>() {
        return Ok(vec![address]);
    }
    if let Some(pinned) = pins.addresses_of(host) {
        return Ok(pinned);
    }
    let network = network.clone();
    let name = host.to_owned();
    spawn_blocking(move || network.reachable(&name, Cause::HttpRequest)).await
}

/// RFC 8305's "Connection Attempt Delay": how long an attempt has alone before the next is made
/// beside it.
const ATTEMPT_DELAY: Duration = Duration::from_millis(250);

/// A connection of the program whose network is `network` to the first of `addresses` that
/// answers at `port`; the last one's failure where none does.
async fn dial(
    addresses: &[IpAddr],
    port: u16,
    network: &ProgramNetwork,
) -> std::io::Result<Connection> {
    first_connected(addresses, |address| {
        Connection::dial(SocketAddr::new(address, port), network)
    })
    .await
}

/// The first of `addresses` that `connect` answers for, each next tried beside the last after
/// [`ATTEMPT_DELAY`] rather than once it fails, as every client tries a name's IPv6 and IPv4
/// addresses (RFC 8305, "Happy Eyeballs v2"); the last failure where none answers.
async fn first_connected<T, F, Fut>(addresses: &[IpAddr], connect: F) -> std::io::Result<T>
where
    F: Fn(IpAddr) -> Fut,
    Fut: Future<Output = std::io::Result<T>>,
{
    let mut connecting = FuturesUnordered::new();
    let mut next = 0;
    let mut started = Instant::now();
    let mut failed = std::io::Error::from(std::io::ErrorKind::AddrNotAvailable);
    loop {
        if next < addresses.len() {
            connecting.push(connect(addresses[next]));
            next += 1;
            started = Instant::now();
        }
        if connecting.is_empty() {
            return Err(failed);
        }
        let next_attempt = tokio::time::sleep(ATTEMPT_DELAY.saturating_sub(started.elapsed()));
        tokio::select! {
            Some(result) = connecting.next() => match result {
                Ok(connected) => return Ok(connected),
                Err(error) => failed = error,
            },
            () = next_attempt, if next < addresses.len() => {}
        }
    }
}

/// `stream`, in TLS with `uri`'s host where `uri` is HTTPS's.
async fn secured(
    tls: &Arc<ClientConfig>,
    stream: Box<dyn Stream>,
    uri: &Uri,
) -> Result<Box<dyn Stream>, BoxError> {
    if !is_https(uri) {
        return Ok(stream);
    }
    let name = ServerName::try_from(bare(host_of(uri)?).to_owned())?;
    let connector = tokio_rustls::TlsConnector::from(Arc::clone(tls));
    Ok(Box::new(connector.connect(name, stream).await?))
}

/// The TLS a program's HTTPS speaks: TLS 1.2 or 1.3, HTTP/1.1, each server's certificate checked
/// as the system checks it.
pub(super) fn tls_config() -> Result<Arc<ClientConfig>, rustls::Error> {
    let provider = crate::trust::provider();
    let verifier = rustls_platform_verifier::Verifier::new(Arc::clone(&provider))?;
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

fn host_of(uri: &Uri) -> Result<&str, LookupError> {
    uri.host().ok_or(LookupError::Invalid)
}

/// `host` without the brackets a URL writes an IPv6 address in.
fn bare(host: &str) -> &str {
    host.trim_start_matches('[').trim_end_matches(']')
}

fn is_https(uri: &Uri) -> bool {
    uri.scheme_str() == Some("https")
}

/// The port `uri` names, or its scheme's: 443 for HTTPS, 1080 for SOCKS, 80 otherwise.
fn port_of(uri: &Uri) -> u16 {
    uri.port_u16().unwrap_or(match uri.scheme_str() {
        Some("https") => 443,
        Some("socks5" | "socks5h") => 1080,
        _ => 80,
    })
}

/// `uri`'s scheme and host, with its port written out.
fn with_port(uri: &Uri) -> Result<Uri, http::Error> {
    let scheme = uri.scheme_str().unwrap_or("http");
    let host = uri.host().unwrap_or_default();
    Ok(Uri::try_from(format!(
        "{scheme}://{host}:{}",
        port_of(uri)
    ))?)
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    /// The second address is reached while the first is still trying, not after it fails.
    #[tokio::test]
    async fn an_address_that_never_answers_does_not_hold_the_next_back() {
        let addresses = [
            IpAddr::from(Ipv4Addr::new(192, 0, 2, 1)),
            IpAddr::from(Ipv4Addr::LOCALHOST),
        ];
        let started = Instant::now();
        let connected = first_connected(&addresses, |address| async move {
            if address.is_loopback() {
                Ok(address)
            } else {
                std::future::pending().await
            }
        })
        .await
        .expect("this computer answers");
        assert_eq!(connected, IpAddr::from(Ipv4Addr::LOCALHOST));
        assert!(
            started.elapsed() >= ATTEMPT_DELAY,
            "the next waited its turn: {:?}",
            started.elapsed()
        );
        assert!(
            started.elapsed() < ATTEMPT_DELAY * 4,
            "the next did not wait for the first to fail: {:?}",
            started.elapsed()
        );
    }
}
