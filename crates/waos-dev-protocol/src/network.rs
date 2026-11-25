//! The dev protocol across the network, for a computer reaching another device's WaOS: a
//! WebSocket, each message one binary WebSocket message, so any WebSocket client, proxy or tunnel
//! carries it. `wss://` is taken from any address, its certificate the device's own, which a
//! computer pins by the fingerprint it learned at pairing (`pairing`); `ws://` is taken from this
//! machine alone, where a tunnel or a forwarded port ends.
//!
//! The invariants a later editor will break:
//! - A computer trusts a device's certificate by its fingerprint and nothing else: no name, no
//!   authority, no shortcut. Only pairing connects without one, and pairing checks the fingerprint
//!   it saw against the device's own word for it before keeping it.
//! - `ws://` from another address is refused before a message is read: a token would cross the
//!   network in the clear.

use std::fmt;
use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, ServerConfig, SignatureScheme};
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{self, Message};

use crate::connection::{
    Connection, Frames, Locality, MAX_FRAME_BYTES, ReceiveFrames, SendFrames, TransportFrames,
    invalid_data,
};
use crate::{ClientConnection, ServerConnection};

/// The port a WaOS with developer access listens on, or the first free one after it; an address
/// given without a port means this one.
pub const PORT: u16 = 7351;

/// How many ports after [`PORT`] a WaOS tries when it is taken.
const PORTS_TRIED: u16 = 20;

/// The first byte of a TLS connection's first record, a handshake: what tells `wss://` from
/// `ws://`, whose first byte is the `G` of `GET`.
const TLS_HANDSHAKE: u8 = 0x16;

/// The name the device's certificate is made for; a computer checks the fingerprint, never it.
const CERTIFICATE_NAME: &str = "waos-developer-access";

/// A device's certificate, as a computer recognizes it: the SHA-256 of its DER.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    /// The fingerprint of `certificate`, its DER.
    pub fn of(certificate: &[u8]) -> Fingerprint {
        Fingerprint(Sha256::digest(certificate).into())
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

/// A fingerprint in a message: its 32 bytes.
impl serde::Serialize for Fingerprint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde_bytes::Bytes::new(&self.0).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Fingerprint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Fingerprint, D::Error> {
        let bytes = serde_bytes::ByteBuf::deserialize(deserializer)?;
        <[u8; 32]>::try_from(bytes.as_slice())
            .map(Fingerprint)
            .map_err(|_| serde::de::Error::custom("a fingerprint is 32 bytes"))
    }
}

impl FromStr for Fingerprint {
    type Err = String;

    fn from_str(text: &str) -> Result<Fingerprint, String> {
        let bytes = hex::decode(text).map_err(|error| format!("a fingerprint: {error}"))?;
        let bytes = <[u8; 32]>::try_from(bytes.as_slice())
            .map_err(|_| "a fingerprint is 32 bytes".to_owned())?;
        Ok(Fingerprint(bytes))
    }
}

/// A device's identity for developer access: its key, and the self-signed certificate it
/// answers `wss://` with, made the first time access is turned on and kept from then on, so a
/// computer paired once recognizes it after.
#[derive(Clone)]
pub struct Identity {
    certificate: Vec<u8>,
    key: Vec<u8>,
}

impl Identity {
    /// A new identity: a P-256 key and a certificate for it.
    pub fn make() -> io::Result<Identity> {
        let made = rcgen::generate_simple_self_signed(vec![CERTIFICATE_NAME.to_owned()])
            .map_err(io::Error::other)?;
        Ok(Identity {
            certificate: made.cert.der().to_vec(),
            key: made.signing_key.serialize_der(),
        })
    }

    /// The identity of `certificate`, its DER, and `key`, its PKCS#8 DER, as kept.
    pub fn new(certificate: Vec<u8>, key: Vec<u8>) -> Identity {
        Identity { certificate, key }
    }

    /// The certificate's DER, to keep.
    pub fn certificate(&self) -> &[u8] {
        &self.certificate
    }

    /// The key's PKCS#8 DER, to keep where the user alone reads it.
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of(&self.certificate)
    }

    fn acceptor(&self) -> io::Result<TlsAcceptor> {
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone()));
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(self.certificate.clone())], key)
            .map_err(io::Error::other)?;
        Ok(TlsAcceptor::from(Arc::new(config)))
    }
}

/// Where a device's WaOS is reached across the network: `host` or `host:port` (`wss://`), or a
/// `wss://` or `ws://` URL; [`PORT`] when none is given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceAddress {
    pub host: String,
    pub port: u16,
    pub secure: bool,
}

impl FromStr for DeviceAddress {
    type Err = String;

    fn from_str(text: &str) -> Result<DeviceAddress, String> {
        let (secure, rest) = match text.split_once("://") {
            Some(("wss", rest)) => (true, rest),
            Some(("ws", rest)) => (false, rest),
            Some((scheme, _)) => {
                return Err(format!(
                    "{scheme}:// is not one: wss:// or ws:// reaches a WaOS"
                ));
            }
            None => (true, text),
        };
        let rest = rest.trim_end_matches('/');
        let (host, port) = split_port(rest)?;
        if host.is_empty() {
            return Err(format!("{text} names no host"));
        }
        Ok(DeviceAddress {
            host: host.to_owned(),
            port: port.unwrap_or(PORT),
            secure,
        })
    }
}

impl fmt::Display for DeviceAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = if self.secure { "wss" } else { "ws" };
        if self.host.contains(':') {
            write!(formatter, "{scheme}://[{}]:{}", self.host, self.port)
        } else {
            write!(formatter, "{scheme}://{}:{}", self.host, self.port)
        }
    }
}

/// `text`'s host and its port, if it has one: `[v6]:port`, `host:port`, or a bare host or v6.
fn split_port(text: &str) -> Result<(&str, Option<u16>), String> {
    let port = |port: &str| {
        port.parse::<u16>()
            .map_err(|_| format!("{port} is not a port"))
    };
    if let Some(bracketed) = text.strip_prefix('[') {
        let (host, after) = bracketed
            .split_once(']')
            .ok_or_else(|| format!("{text} opens a [ it does not close"))?;
        return match after.strip_prefix(':') {
            Some(after) => Ok((host, Some(port(after)?))),
            None if after.is_empty() => Ok((host, None)),
            None => Err(format!("{text} has more after its address")),
        };
    }
    match text.matches(':').count() {
        0 => Ok((text, None)),
        1 => {
            let (host, after) = text.split_once(':').expect("one colon");
            Ok((host, Some(port(after)?)))
        }
        // An IPv6 address without brackets, which names no port.
        _ => Ok((text, None)),
    }
}

/// A TCP stream, as it is or inside TLS: what a WebSocket runs over.
#[derive(Debug)]
pub enum NetworkStream<T> {
    Plain(TcpStream),
    Tls(Box<T>),
}

impl<T: AsyncRead + Unpin> AsyncRead for NetworkStream<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            NetworkStream::Plain(stream) => Pin::new(stream).poll_read(context, buf),
            NetworkStream::Tls(stream) => Pin::new(stream.as_mut()).poll_read(context, buf),
        }
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for NetworkStream<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            NetworkStream::Plain(stream) => Pin::new(stream).poll_write(context, buf),
            NetworkStream::Tls(stream) => Pin::new(stream.as_mut()).poll_write(context, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            NetworkStream::Plain(stream) => Pin::new(stream).poll_flush(context),
            NetworkStream::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(context),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            NetworkStream::Plain(stream) => Pin::new(stream).poll_shutdown(context),
            NetworkStream::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(context),
        }
    }
}

/// The stream of a connection a WaOS accepted across the network.
pub type ServerNetworkStream = NetworkStream<tokio_rustls::server::TlsStream<TcpStream>>;

/// The stream of a connection `waos` made across the network.
pub type ClientNetworkStream = NetworkStream<tokio_rustls::client::TlsStream<TcpStream>>;

/// Frames as WebSocket messages: each message one binary WebSocket message.
#[derive(Debug)]
pub struct WebSocketFrames<S>(WebSocketStream<S>);

/// The half of [`WebSocketFrames`] that receives.
#[derive(Debug)]
pub struct WebSocketReceiving<S>(SplitStream<WebSocketStream<S>>);

/// The half of [`WebSocketFrames`] that sends.
#[derive(Debug)]
pub struct WebSocketSending<S>(SplitSink<WebSocketStream<S>, Message>);

impl<S: AsyncRead + AsyncWrite + Unpin + Send> SendFrames for WebSocketFrames<S> {
    async fn send_frame(&mut self, frame: Vec<u8>) -> io::Result<()> {
        self.0
            .send(Message::Binary(frame.into()))
            .await
            .map_err(io_error)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> ReceiveFrames for WebSocketFrames<S> {
    async fn receive_frame(&mut self) -> io::Result<Option<Vec<u8>>> {
        next_frame(&mut self.0).await
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> Frames for WebSocketFrames<S> {
    type Receiving = WebSocketReceiving<S>;
    type Sending = WebSocketSending<S>;

    fn split(self) -> (WebSocketReceiving<S>, WebSocketSending<S>) {
        let (sending, receiving) = self.0.split();
        (WebSocketReceiving(receiving), WebSocketSending(sending))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> ReceiveFrames for WebSocketReceiving<S> {
    async fn receive_frame(&mut self) -> io::Result<Option<Vec<u8>>> {
        next_frame(&mut self.0).await
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> SendFrames for WebSocketSending<S> {
    async fn send_frame(&mut self, frame: Vec<u8>) -> io::Result<()> {
        self.0
            .send(Message::Binary(frame.into()))
            .await
            .map_err(io_error)
    }
}

/// The next binary message's bytes from `messages`; none once the other side closed. A ping or a
/// pong is the WebSocket's own and passes; text is not this protocol's.
async fn next_frame(
    messages: &mut (impl futures::Stream<Item = tungstenite::Result<Message>> + Unpin),
) -> io::Result<Option<Vec<u8>>> {
    loop {
        match messages.next().await {
            None | Some(Ok(Message::Close(_))) => return Ok(None),
            Some(Ok(Message::Binary(bytes))) => return Ok(Some(bytes.into())),
            Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
            Some(Ok(Message::Text(_))) => {
                return Err(invalid_data(
                    "a text message, where the protocol's are binary",
                ));
            }
            Some(Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed)) => {
                return Ok(None);
            }
            Some(Err(error)) => return Err(io_error(error)),
        }
    }
}

fn io_error(error: tungstenite::Error) -> io::Error {
    match error {
        tungstenite::Error::Io(error) => error,
        tungstenite::Error::Capacity(error) => invalid_data(error.to_string()),
        other => io::Error::other(other),
    }
}

/// The WebSocket's limits: a message as large as a frame of the protocol, and no larger.
fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES))
}

/// Where a WaOS with developer access takes connections from the network.
pub struct NetworkListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
    port: u16,
}

impl NetworkListener {
    /// Listens on every address, on [`PORT`] or the first free port after it, answering
    /// `wss://` with `identity`'s certificate.
    pub async fn bind(identity: &Identity) -> io::Result<NetworkListener> {
        let mut last = None;
        for port in PORT..PORT + PORTS_TRIED {
            let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
            match NetworkListener::bind_at(identity, address).await {
                Err(error) if error.kind() == ErrorKind::AddrInUse => last = Some(error),
                bound => return bound,
            }
        }
        Err(last.unwrap_or_else(|| ErrorKind::AddrInUse.into()))
    }

    /// Listens at `address` alone, as a test listens on this machine's loopback; port 0 is any
    /// free port.
    pub async fn bind_at(identity: &Identity, address: SocketAddr) -> io::Result<NetworkListener> {
        let acceptor = identity.acceptor()?;
        let listener = TcpListener::bind(address).await?;
        let port = listener.local_addr()?.port();
        Ok(NetworkListener {
            listener,
            acceptor,
            port,
        })
    }

    /// The port it listens on, which it announces.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The next connection, its handshakes still to come, which the caller runs on a task of
    /// the connection's own, so a slow peer holds up no other.
    pub async fn accept(&mut self) -> io::Result<Incoming> {
        let (stream, peer) = self.listener.accept().await?;
        Ok(Incoming {
            stream,
            peer,
            acceptor: self.acceptor.clone(),
        })
    }
}

/// A connection from the network, accepted, its TLS and WebSocket handshakes still to come.
pub struct Incoming {
    stream: TcpStream,
    peer: SocketAddr,
    acceptor: TlsAcceptor,
}

impl Incoming {
    /// The address it came from.
    pub fn peer(&self) -> SocketAddr {
        self.peer
    }

    /// The connection, its handshakes done: `wss://` from anywhere, `ws://` from this machine
    /// alone, refused from elsewhere with an answer naming `wss://`.
    #[expect(
        clippy::result_large_err,
        reason = "tungstenite's callback refuses with its own response, as large as it is"
    )]
    pub async fn handshake(self) -> io::Result<ServerConnection> {
        let mut first = [0; 1];
        let peeked = self.stream.peek(&mut first).await?;
        let stream = if peeked == 1 && first[0] == TLS_HANDSHAKE {
            let tls = self.acceptor.accept(self.stream).await?;
            NetworkStream::Tls(Box::new(tls))
        } else {
            NetworkStream::Plain(self.stream)
        };
        let secure = matches!(stream, NetworkStream::Tls(_));
        let from_this_machine = self.peer.ip().is_loopback();
        let refuse_plain = !secure && !from_this_machine;
        let socket = tokio_tungstenite::accept_hdr_async_with_config(
            stream,
            |_request: &Request, response: Response| {
                if refuse_plain {
                    return Err(plain_refused());
                }
                Ok(response)
            },
            Some(config()),
        )
        .await
        .map_err(io_error)?;
        Ok(across_network(socket))
    }
}

/// A connection over `socket`, as each side holds one.
fn across_network<L, S>(
    socket: WebSocketStream<S>,
) -> Connection<TransportFrames<L, WebSocketFrames<S>>> {
    Connection::carried(
        TransportFrames::Network(WebSocketFrames(socket)),
        Locality::Network,
    )
}

/// The answer to `ws://` from another machine: refused, naming what to use.
fn plain_refused() -> ErrorResponse {
    let mut response = ErrorResponse::new(Some(
        "this WaOS takes ws:// from this machine alone; connect with wss://".to_owned(),
    ));
    *response.status_mut() = StatusCode::FORBIDDEN;
    response
}

/// A connection to the WaOS at `address`, trusting the certificate `pinned` names; with none, as
/// pairing does, any certificate, whose fingerprint is answered for pairing to check. `ws://`
/// has no certificate, so none is answered.
pub async fn connect(
    address: &DeviceAddress,
    pinned: Option<Fingerprint>,
) -> io::Result<(ClientConnection, Option<Fingerprint>)> {
    let tcp = TcpStream::connect((address.host.as_str(), address.port)).await?;
    tcp.set_nodelay(true)?;
    let (stream, seen) = if address.secure {
        let verifier = Arc::new(PinnedCertificate::new(pinned));
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::clone(&verifier) as _)
            .with_no_client_auth();
        let name = ServerName::try_from(CERTIFICATE_NAME).expect("a name");
        let tls = TlsConnector::from(Arc::new(config))
            .connect(name, tcp)
            .await?;
        (NetworkStream::Tls(Box::new(tls)), verifier.seen())
    } else {
        (NetworkStream::Plain(tcp), None)
    };
    let url = address.to_string();
    let (socket, _response) =
        tokio_tungstenite::client_async_with_config(url, stream, Some(config()))
            .await
            .map_err(io_error)?;
    Ok((across_network(socket), seen))
}

/// A device's certificate, trusted by its fingerprint alone: the one pinned, or, with none, any,
/// whose fingerprint it keeps for pairing to check.
#[derive(Debug)]
struct PinnedCertificate {
    pinned: Option<Fingerprint>,
    seen: Mutex<Option<Fingerprint>>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl PinnedCertificate {
    fn new(pinned: Option<Fingerprint>) -> PinnedCertificate {
        let provider = CryptoProvider::get_default()
            .cloned()
            .unwrap_or_else(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()));
        PinnedCertificate {
            pinned,
            seen: Mutex::new(None),
            algorithms: provider.signature_verification_algorithms,
        }
    }

    fn seen(&self) -> Option<Fingerprint> {
        *self.seen.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ServerCertVerifier for PinnedCertificate {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fingerprint = Fingerprint::of(end_entity);
        *self.seen.lock().unwrap_or_else(PoisonError::into_inner) = Some(fingerprint);
        match self.pinned {
            Some(pinned) if pinned != fingerprint => Err(rustls::Error::General(format!(
                "the device's certificate is not the one paired with (it is {fingerprint}); pair again with `waos pair`"
            ))),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, certificate, signature, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, certificate, signature, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests;
