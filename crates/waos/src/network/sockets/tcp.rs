//! A TCP socket, from its creation through binding, connecting or listening, to its close, and
//! the streams its connection is read and written through, which go on after the socket's handle
//! is dropped. A connection the program makes is checked as the connect starts; one its listener
//! accepts is checked as it is accepted, and reset unseen if refused.

use std::future::Future;
use std::mem;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use rustix::fd::{AsFd, BorrowedFd};
use rustix::io::Errno;
use rustix::net::sockopt;
use wasmtime_wasi::runtime::with_ambient_tokio_runtime;

use super::family::{is_of_family, is_remote, is_unicast, unspecified};
use super::open_sockets::{Connection, Listener};
use super::options::{self, ListenerOptions, clamp_keep_alive_count, clamp_keep_alive_time};
use super::refused_unless;
use crate::access::TcpOrigin;
use crate::network::ProgramNetwork;
use crate::network::host::bindings::wasi::sockets::types::{ErrorCode, IpAddressFamily};

/// The listen queue's size until the program sets one: Rust's standard library's.
const DEFAULT_BACKLOG: u32 = 128;

/// A program's TCP socket.
pub struct TcpSocket {
    state: TcpState,
    /// The listen queue's size, for the first `listen`.
    listen_backlog_size: u32,
    family: IpAddressFamily,
    /// What checks each address the socket uses.
    network: ProgramNetwork,
    /// The options a listener's accepted connections are given where the system does not.
    listener_options: ListenerOptions,
    /// Whether the socket is bound, which it never stops being once it is: kept, so it is asked
    /// of the system until it is.
    is_bound: bool,
}

/// Where a socket is in binding, listening, accepting and connecting.
enum TcpState {
    /// Made, and perhaps bound; next listening or connecting.
    Default(tokio::net::TcpSocket),
    /// Listening, which it does until it is closed.
    Listening(Arc<Listener>),
    /// Connecting, until the connect's future answers.
    Connecting(Pending<Result<Connection, ErrorCode>>),
    /// Connected, by a connect or an accept, which it stays.
    Connected {
        connection: Arc<Connection>,
        /// The peer, as an accept answered it or the first query that found it: a reset
        /// connection no longer tells it.
        peer: Option<SocketAddr>,
        receive_taken: bool,
        send_taken: bool,
    },
    /// Closed, by what failed.
    Closed(ErrorCode),
}

/// A future polled to its end in one place and its answer taken in another, as a connect is
/// polled by its call and its stream kept by the socket.
enum Pending<T> {
    Waiting(Pin<Box<dyn Future<Output = T> + Send>>),
    Ready(T),
}

impl TcpState {
    fn connected(connection: Connection, peer: Option<SocketAddr>) -> TcpState {
        TcpState::Connected {
            connection: Arc::new(connection),
            peer,
            receive_taken: false,
            send_taken: false,
        }
    }

    fn take(&mut self) -> TcpState {
        mem::replace(self, TcpState::Closed(ErrorCode::Other(None)))
    }
}

impl TcpSocket {
    /// A new socket of `family`, its addresses checked by `network`.
    pub(crate) fn new(
        network: ProgramNetwork,
        family: IpAddressFamily,
    ) -> Result<TcpSocket, ErrorCode> {
        let socket = with_ambient_tokio_runtime(|| socket(family))?;
        Ok(TcpSocket {
            state: TcpState::Default(socket),
            listen_backlog_size: DEFAULT_BACKLOG,
            family,
            network,
            listener_options: ListenerOptions::default(),
            is_bound: false,
        })
    }

    fn as_fd(&self) -> Result<BorrowedFd<'_>, ErrorCode> {
        match &self.state {
            TcpState::Default(socket) => Ok(socket.as_fd()),
            TcpState::Connected { connection, .. } => Ok(connection.stream().as_fd()),
            TcpState::Listening(listener) => Ok(listener.listener().as_fd()),
            TcpState::Connecting(_) => Err(ErrorCode::InvalidState),
            TcpState::Closed(error) => Err(error.clone()),
        }
    }

    pub(crate) fn is_bound(&mut self) -> bool {
        if !self.is_bound {
            self.is_bound = match &self.state {
                TcpState::Default(socket) => socket
                    .local_addr()
                    .is_ok_and(|address| address != unspecified(self.family)),
                _ => true,
            };
        }
        self.is_bound
    }

    pub(crate) fn bind(&mut self, address: SocketAddr) -> Result<(), ErrorCode> {
        if self.is_bound() {
            return Err(ErrorCode::InvalidState);
        }
        let TcpState::Default(socket) = &self.state else {
            return Err(ErrorCode::InvalidState);
        };
        if !is_unicast(address.ip()) || !is_of_family(address.ip(), self.family) {
            return Err(ErrorCode::InvalidArgument);
        }
        bind(socket, address)
    }

    /// Starts connecting to `address`, which [`poll_finish_connect`](Self::poll_finish_connect)
    /// finishes. The checks run as the connect does, so a refusal closes the socket as a failed
    /// connect does.
    pub(crate) fn start_connect(&mut self, address: SocketAddr) -> Result<(), ErrorCode> {
        let TcpState::Default(_) = &self.state else {
            return Err(ErrorCode::InvalidState);
        };
        if !is_unicast(address.ip())
            || !is_remote(address)
            || !is_of_family(address.ip(), self.family)
        {
            return Err(ErrorCode::InvalidArgument);
        }
        let network = self.network.clone();
        let TcpState::Default(socket) = self.state.take() else {
            unreachable!("checked above");
        };
        self.state = TcpState::Connecting(Pending::new(async move {
            refused_unless(network.allows_connect(address))?;
            let stream = socket.connect(address).await?;
            Ok(Connection::new(
                stream,
                &network,
                TcpOrigin::Connected,
                address,
            ))
        }));
        Ok(())
    }

    pub(crate) fn poll_finish_connect(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), ErrorCode>> {
        match &mut self.state {
            TcpState::Connecting(connect) => {
                ready!(with_ambient_tokio_runtime(|| connect.poll_ready(cx)));
            }
            TcpState::Connected { .. } => return Poll::Ready(Ok(())),
            TcpState::Closed(error) => return Poll::Ready(Err(error.clone())),
            _ => return Poll::Ready(Err(ErrorCode::InvalidState)),
        }
        let TcpState::Connecting(connect) = self.state.take() else {
            unreachable!("matched above");
        };
        match connect.into_ready() {
            Ok(connection) => {
                self.state = TcpState::connected(connection, None);
                Poll::Ready(Ok(()))
            }
            Err(error) => {
                self.state = TcpState::Closed(error.clone());
                Poll::Ready(Err(error))
            }
        }
    }

    /// Listens, binding the socket first where it is not bound, so every system listens alike:
    /// Linux and macOS bind it as they listen, Windows refuses to.
    pub(crate) fn listen(&mut self) -> Result<TcpListenStream, ErrorCode> {
        let already_bound = self.is_bound();
        let socket = match self.state.take() {
            TcpState::Default(socket) => socket,
            state => {
                self.state = state;
                return Err(ErrorCode::InvalidState);
            }
        };
        let address = match already_bound {
            true => socket.local_addr()?,
            false => unspecified(self.family),
        };
        refused_unless(self.network.allows_listen(address))?;
        if !already_bound {
            bind(&socket, address)?;
        }
        let listener = socket
            .listen(self.listen_backlog_size)
            .map_err(listen_error)?;
        let listener = Arc::new(Listener::new(listener, &self.network)?);
        self.state = TcpState::Listening(Arc::clone(&listener));
        Ok(TcpListenStream {
            listener,
            family: self.family,
            listener_options: self.listener_options.clone(),
            network: self.network.clone(),
            pending_accept: None,
        })
    }

    pub(crate) fn take_send_stream(&mut self) -> Result<TcpSendStream, ErrorCode> {
        match &mut self.state {
            TcpState::Connected {
                connection,
                send_taken,
                ..
            } if !*send_taken => {
                *send_taken = true;
                Ok(TcpSendStream {
                    connection: Arc::clone(connection),
                })
            }
            TcpState::Closed(error) => Err(error.clone()),
            _ => Err(ErrorCode::InvalidState),
        }
    }

    pub(crate) fn take_receive_stream(&mut self) -> Result<TcpReceiveStream, ErrorCode> {
        match &mut self.state {
            TcpState::Connected {
                connection,
                receive_taken,
                ..
            } if !*receive_taken => {
                *receive_taken = true;
                Ok(TcpReceiveStream {
                    connection: Arc::clone(connection),
                })
            }
            TcpState::Closed(error) => Err(error.clone()),
            _ => Err(ErrorCode::InvalidState),
        }
    }

    pub(crate) fn local_address(&mut self) -> Result<SocketAddr, ErrorCode> {
        if !self.is_bound() {
            return Err(ErrorCode::InvalidState);
        }
        match &self.state {
            TcpState::Default(socket) => Ok(socket.local_addr()?),
            TcpState::Connecting(_) => Err(ErrorCode::InvalidState),
            TcpState::Connected { connection, .. } => Ok(connection.stream().local_addr()?),
            TcpState::Listening(listener) => Ok(listener.listener().local_addr()?),
            TcpState::Closed(error) => Err(error.clone()),
        }
    }

    pub(crate) fn remote_address(&mut self) -> Result<SocketAddr, ErrorCode> {
        match &mut self.state {
            TcpState::Connected {
                peer: Some(peer), ..
            } => Ok(*peer),
            TcpState::Connected {
                connection, peer, ..
            } => {
                let address = connection.stream().peer_addr()?;
                *peer = Some(address);
                Ok(address)
            }
            TcpState::Closed(error) => Err(error.clone()),
            _ => Err(ErrorCode::InvalidState),
        }
    }

    pub(crate) fn is_listening(&self) -> bool {
        matches!(self.state, TcpState::Listening(_))
    }

    pub(crate) fn address_family(&self) -> IpAddressFamily {
        self.family
    }

    /// Sets the listen queue's size, clamped as systems clamp it; on a listener, where the system
    /// lets a second `listen` change it.
    pub(crate) fn set_listen_backlog_size(&mut self, value: u64) -> Result<(), ErrorCode> {
        if value == 0 {
            return Err(ErrorCode::InvalidArgument);
        }
        let value = value
            .try_into()
            .unwrap_or(i32::MAX as u32)
            .clamp(1, i32::MAX as u32);
        match &self.state {
            TcpState::Default(_) => {
                self.listen_backlog_size = value;
                Ok(())
            }
            TcpState::Listening(listener) => {
                let backlog = value.try_into().unwrap_or(i32::MAX);
                if rustix::net::listen(listener.listener(), backlog).is_err() {
                    return Err(ErrorCode::NotSupported);
                }
                self.listen_backlog_size = value;
                Ok(())
            }
            TcpState::Closed(error) => Err(error.clone()),
            _ => Err(ErrorCode::InvalidState),
        }
    }

    pub(crate) fn keep_alive_enabled(&self) -> Result<bool, ErrorCode> {
        Ok(sockopt::socket_keepalive(self.as_fd()?)?)
    }

    pub(crate) fn set_keep_alive_enabled(&self, value: bool) -> Result<(), ErrorCode> {
        Ok(sockopt::set_socket_keepalive(self.as_fd()?, value)?)
    }

    pub(crate) fn keep_alive_idle_time(&self) -> Result<u64, ErrorCode> {
        Ok(options::keep_alive_time(sockopt::tcp_keepidle(
            self.as_fd()?,
        )?))
    }

    pub(crate) fn set_keep_alive_idle_time(&mut self, value: u64) -> Result<(), ErrorCode> {
        if value == 0 {
            return Err(ErrorCode::InvalidArgument);
        }
        let value = clamp_keep_alive_time(value);
        sockopt::set_tcp_keepidle(self.as_fd()?, Duration::from_nanos(value))?;
        self.listener_options.set_keep_alive_idle_time(value);
        Ok(())
    }

    pub(crate) fn keep_alive_interval(&self) -> Result<u64, ErrorCode> {
        Ok(options::keep_alive_time(sockopt::tcp_keepintvl(
            self.as_fd()?,
        )?))
    }

    pub(crate) fn set_keep_alive_interval(&self, value: u64) -> Result<(), ErrorCode> {
        if value == 0 {
            return Err(ErrorCode::InvalidArgument);
        }
        let value = clamp_keep_alive_time(value);
        Ok(sockopt::set_tcp_keepintvl(
            self.as_fd()?,
            Duration::from_nanos(value),
        )?)
    }

    pub(crate) fn keep_alive_count(&self) -> Result<u32, ErrorCode> {
        Ok(sockopt::tcp_keepcnt(self.as_fd()?)?)
    }

    pub(crate) fn set_keep_alive_count(&self, value: u32) -> Result<(), ErrorCode> {
        if value == 0 {
            return Err(ErrorCode::InvalidArgument);
        }
        let value = clamp_keep_alive_count(value);
        Ok(sockopt::set_tcp_keepcnt(self.as_fd()?, value)?)
    }

    pub(crate) fn hop_limit(&self) -> Result<u8, ErrorCode> {
        options::unicast_hop_limit(self.as_fd()?, self.family)
    }

    pub(crate) fn set_hop_limit(&mut self, value: u8) -> Result<(), ErrorCode> {
        options::set_unicast_hop_limit(self.as_fd()?, self.family, value)?;
        self.listener_options.set_hop_limit(value);
        Ok(())
    }

    pub(crate) fn receive_buffer_size(&self) -> Result<u64, ErrorCode> {
        options::receive_buffer_size(self.as_fd()?)
    }

    pub(crate) fn set_receive_buffer_size(&mut self, value: u64) -> Result<(), ErrorCode> {
        let size = options::set_receive_buffer_size(self.as_fd()?, value)?;
        self.listener_options.set_receive_buffer_size(size);
        Ok(())
    }

    pub(crate) fn send_buffer_size(&self) -> Result<u64, ErrorCode> {
        options::send_buffer_size(self.as_fd()?)
    }

    pub(crate) fn set_send_buffer_size(&mut self, value: u64) -> Result<(), ErrorCode> {
        let size = options::set_send_buffer_size(self.as_fd()?, value)?;
        self.listener_options.set_send_buffer_size(size);
        Ok(())
    }
}

/// A listener's connections, each accepted as the program asks for the next, those the program's
/// network refuses reset unseen.
pub(crate) struct TcpListenStream {
    listener: Arc<Listener>,
    family: IpAddressFamily,
    listener_options: ListenerOptions,
    network: ProgramNetwork,
    /// The next connection allowed, accepted ahead of the program's asking.
    pending_accept: Option<Pending<Result<(tokio::net::TcpStream, SocketAddr), ErrorCode>>>,
}

impl TcpListenStream {
    /// The next connection accepted, as a socket of its own; one that failed as a closed socket.
    pub(crate) fn poll_accept(&mut self, cx: &mut Context<'_>) -> Poll<TcpSocket> {
        ready!(self.poll_ready(cx));
        let accepted = self
            .pending_accept
            .take()
            .expect("ready above")
            .into_ready();
        let state = match accepted {
            Ok((client, peer)) => {
                self.listener_options.apply(self.family, &client);
                let connection = Connection::new(client, &self.network, TcpOrigin::Accepted, peer);
                TcpState::connected(connection, Some(peer))
            }
            Err(error) => TcpState::Closed(error),
        };
        Poll::Ready(TcpSocket {
            state,
            listen_backlog_size: DEFAULT_BACKLOG,
            family: self.family,
            network: self.network.clone(),
            listener_options: ListenerOptions::default(),
            is_bound: true,
        })
    }

    /// Ready once a connection the network allows was accepted, or accepting failed.
    pub(crate) fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.pending_accept.is_none() {
            let listener = Arc::clone(&self.listener);
            let network = self.network.clone();
            self.pending_accept = Some(Pending::new(async move {
                loop {
                    let (client, peer) = accept(listener.listener()).await?;
                    if network.allows_accept(peer) {
                        return Ok((client, peer));
                    }
                    reset(client);
                }
            }));
        }
        let pending = self.pending_accept.as_mut().expect("set above");
        with_ambient_tokio_runtime(|| pending.poll_ready(cx).map(|_| ()))
    }
}

/// The sending half of a connection, which shuts the connection down for writing as it drops.
pub(crate) struct TcpSendStream {
    connection: Arc<Connection>,
}

impl TcpSendStream {
    pub(crate) fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.connection.stream().poll_write_ready(cx).map(|_| ())
    }

    pub(crate) fn poll_write(
        &mut self,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, ErrorCode>> {
        self.connection
            .poll_write(cx, buffer)
            .map_err(|error| write_error(&error))
    }
}

impl Drop for TcpSendStream {
    fn drop(&mut self) {
        _ = rustix::net::shutdown(self.connection.stream(), rustix::net::Shutdown::Write);
    }
}

/// The receiving half of a connection, which shuts the connection down for reading as it drops.
pub(crate) struct TcpReceiveStream {
    connection: Arc<Connection>,
}

impl TcpReceiveStream {
    pub(crate) fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.connection.stream().poll_read_ready(cx).map(|_| ())
    }

    /// Reads into `buffer`, answering how much was read, 0 once the peer is done sending.
    pub(crate) fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<Result<usize, ErrorCode>> {
        if buffer.is_empty() {
            return Poll::Ready(Ok(0));
        }
        self.connection
            .poll_read(cx, buffer)
            .map_err(ErrorCode::from)
    }
}

impl Drop for TcpReceiveStream {
    fn drop(&mut self) {
        _ = rustix::net::shutdown(self.connection.stream(), rustix::net::Shutdown::Read);
    }
}

impl<T> Pending<T> {
    fn new(future: impl Future<Output = T> + Send + 'static) -> Pending<T> {
        Pending::Waiting(Box::pin(future))
    }

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<&mut T> {
        if let Pending::Waiting(future) = self {
            *self = Pending::Ready(ready!(future.as_mut().poll(cx)));
        }
        match self {
            Pending::Ready(answer) => Poll::Ready(answer),
            Pending::Waiting(_) => unreachable!("answered above"),
        }
    }

    fn into_ready(self) -> T {
        match self {
            Pending::Ready(answer) => answer,
            Pending::Waiting(_) => panic!("a future taken before it answered"),
        }
    }
}

/// A non-blocking socket of `family`; an IPv6 one IPv6 only, as WASI's sockets have it.
fn socket(family: IpAddressFamily) -> std::io::Result<tokio::net::TcpSocket> {
    match family {
        IpAddressFamily::Ipv4 => tokio::net::TcpSocket::new_v4(),
        IpAddressFamily::Ipv6 => {
            let socket = tokio::net::TcpSocket::new_v6()?;
            sockopt::set_ipv6_v6only(&socket, true)?;
            Ok(socket)
        }
    }
}

/// Binds `socket` to `address`, a recent socket's `TIME_WAIT` on it no hindrance, as WASI's
/// sockets ask: `SO_REUSEADDR` everywhere but Windows, where it is the default and the option
/// means something else.
fn bind(socket: &tokio::net::TcpSocket, address: SocketAddr) -> Result<(), ErrorCode> {
    #[cfg(not(windows))]
    {
        _ = sockopt::set_socket_reuseaddr(socket, true);
    }
    socket
        .bind(address)
        .map_err(|error| match Errno::from_io_error(&error) {
            // A family the checks above let through.
            Some(Errno::AFNOSUPPORT) => ErrorCode::InvalidArgument,
            // Windows' answer when the ephemeral ports run out.
            #[cfg(windows)]
            Some(Errno::NOBUFS) => ErrorCode::AddressInUse,
            _ => error.into(),
        })
}

/// What a `listen` that failed answers: Windows' documented `EMFILE`, though no file is opened,
/// as `ENOBUFS`.
fn listen_error(error: std::io::Error) -> std::io::Error {
    match Errno::from_io_error(&error) {
        #[cfg(windows)]
        Some(Errno::MFILE) => Errno::NOBUFS.into(),
        _ => error,
    }
}

/// The next connection `listener` accepts, its errors made the same on every system: Windows'
/// `EINPROGRESS`, which WASI's sockets have no word for, as `EINTR`, and the network errors Linux
/// alone answers for a connection that failed before it was accepted as `ECONNABORTED`.
async fn accept(
    listener: &tokio::net::TcpListener,
) -> Result<(tokio::net::TcpStream, SocketAddr), ErrorCode> {
    listener.accept().await.map_err(|error| {
        let error = match Errno::from_io_error(&error) {
            #[cfg(windows)]
            Some(Errno::INPROGRESS) => Errno::INTR.into(),
            #[cfg(target_os = "linux")]
            Some(
                Errno::CONNRESET
                | Errno::NETRESET
                | Errno::HOSTUNREACH
                | Errno::HOSTDOWN
                | Errno::NETDOWN
                | Errno::NETUNREACH
                | Errno::PROTO
                | Errno::NOPROTOOPT
                | Errno::NONET
                | Errno::OPNOTSUPP,
            ) => Errno::CONNABORTED.into(),
            _ => error,
        };
        ErrorCode::from(error)
    })
}

/// What a write that failed answers: a connection the peer closed as `connection-broken`.
fn write_error(error: &std::io::Error) -> ErrorCode {
    match Errno::from_io_error(error) {
        #[cfg(windows)]
        Some(Errno::SHUTDOWN | Errno::CONNABORTED) => ErrorCode::ConnectionBroken,
        #[cfg(not(windows))]
        Some(Errno::PIPE) => ErrorCode::ConnectionBroken,
        _ => error.into(),
    }
}

/// Drops `socket` with a reset, so its peer learns at once it was refused.
fn reset(socket: tokio::net::TcpStream) {
    _ = socket.set_zero_linger();
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use std::future::poll_fn;

    use super::*;
    use crate::access::{ActiveUseKind, ProgramAccess};
    use crate::network::test_grants::{TestGrants, any};

    /// A network that reaches this computer.
    fn local_network() -> ProgramNetwork {
        ProgramNetwork::new(ProgramAccess::new(TestGrants::new(any(), true)))
    }

    #[tokio::test]
    async fn an_accepted_connection_keeps_its_peer_after_a_reset() {
        let mut socket = TcpSocket::new(local_network(), IpAddressFamily::Ipv4).unwrap();
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let mut listener = socket.listen().unwrap();
        let client = tokio::net::TcpStream::connect(socket.local_address().unwrap())
            .await
            .unwrap();
        let peer = client.local_addr().unwrap();
        // The accept made, its answer left for `poll_accept`.
        poll_fn(|cx| listener.poll_ready(cx)).await;
        client.set_zero_linger().unwrap();
        drop(client);

        let mut accepted = poll_fn(|cx| listener.poll_accept(cx)).await;
        let mut input = accepted.take_receive_stream().unwrap();
        let mut byte = [0];
        let read = tokio::time::timeout(
            Duration::from_secs(5),
            poll_fn(|cx| input.poll_read(cx, &mut byte)),
        )
        .await
        .unwrap();
        assert!(matches!(read, Err(ErrorCode::ConnectionReset)));
        let TcpState::Connected { connection, .. } = &accepted.state else {
            panic!("an accepted connection");
        };
        assert!(
            connection.stream().peer_addr().is_err(),
            "the system no longer tells it"
        );
        assert_eq!(accepted.remote_address().unwrap(), peer);
    }

    #[tokio::test]
    async fn a_connection_made_keeps_its_peer_after_a_reset() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let peer = listener.local_addr().unwrap();
        let mut client = TcpSocket::new(local_network(), IpAddressFamily::Ipv4).unwrap();
        client.start_connect(peer).unwrap();
        poll_fn(|cx| client.poll_finish_connect(cx)).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        assert_eq!(
            client.remote_address().unwrap(),
            peer,
            "asked while healthy"
        );
        server.set_zero_linger().unwrap();
        drop(server);

        let mut input = client.take_receive_stream().unwrap();
        let mut byte = [0];
        let read = tokio::time::timeout(
            Duration::from_secs(5),
            poll_fn(|cx| input.poll_read(cx, &mut byte)),
        )
        .await
        .unwrap();
        assert!(matches!(read, Err(ErrorCode::ConnectionReset)));
        let TcpState::Connected { connection, .. } = &client.state else {
            panic!("a connection made");
        };
        assert!(
            connection.stream().peer_addr().is_err(),
            "the system no longer tells it"
        );
        assert_eq!(client.remote_address().unwrap(), peer);
    }

    #[tokio::test]
    async fn a_connection_the_network_refuses_closes_the_socket_as_access_denied() {
        let refusing = ProgramNetwork::new(ProgramAccess::new(TestGrants::new(any(), false)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpSocket::new(refusing, IpAddressFamily::Ipv4).unwrap();
        client
            .start_connect(listener.local_addr().unwrap())
            .unwrap();
        let connected = poll_fn(|cx| client.poll_finish_connect(cx)).await;
        assert!(matches!(connected, Err(ErrorCode::AccessDenied)));
        assert!(matches!(
            client.state,
            TcpState::Closed(ErrorCode::AccessDenied)
        ));
    }

    #[tokio::test]
    async fn a_connection_counts_each_byte_it_sends_and_receives() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpSocket::new(local_network(), IpAddressFamily::Ipv4).unwrap();
        client
            .start_connect(listener.local_addr().unwrap())
            .unwrap();
        poll_fn(|cx| client.poll_finish_connect(cx)).await.unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        let mut output = client.take_send_stream().unwrap();
        let mut input = client.take_receive_stream().unwrap();

        let written = poll_fn(|cx| output.poll_write(cx, b"ping")).await.unwrap();
        let mut heard = [0; 4];
        tokio::io::AsyncReadExt::read_exact(&mut server, &mut heard)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut server, b"pong!")
            .await
            .unwrap();
        let mut answer = [0; 8];
        let mut read = 0;
        while read < 5 {
            read += poll_fn(|cx| input.poll_read(cx, &mut answer[read..]))
                .await
                .unwrap();
        }

        let TcpState::Connected { connection, .. } = &client.state else {
            panic!("a connection made");
        };
        assert_eq!(written, 4);
        assert_eq!(connection.counts().sent(), 4);
        assert_eq!(connection.counts().received(), 5);
    }

    /// The program's sockets, as what it uses now names them.
    fn sockets(network: &ProgramNetwork) -> Vec<String> {
        network
            .access()
            .uses_but_open_files()
            .iter()
            .map(|kind| match kind {
                ActiveUseKind::TcpConnection { origin, remote, .. } => {
                    format!("{origin:?} to {}", remote.port())
                }
                ActiveUseKind::TcpListener { address } => {
                    format!("listening at {}", address.port())
                }
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[tokio::test]
    async fn each_listener_and_connection_is_a_use_until_its_socket_closes() {
        let network = local_network();
        let mut socket = TcpSocket::new(network.clone(), IpAddressFamily::Ipv4).unwrap();
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let mut listener = socket.listen().unwrap();
        let port = socket.local_address().unwrap().port();
        let mut client = TcpSocket::new(network.clone(), IpAddressFamily::Ipv4).unwrap();
        client
            .start_connect(socket.local_address().unwrap())
            .unwrap();
        poll_fn(|cx| client.poll_finish_connect(cx)).await.unwrap();
        let mut accepted = poll_fn(|cx| listener.poll_accept(cx)).await;
        let client_port = client.local_address().unwrap().port();
        assert_eq!(
            sockets(&network),
            [
                format!("listening at {port}"),
                format!("Connected to {port}"),
                format!("Accepted to {client_port}"),
            ]
        );

        let input = client.take_receive_stream().unwrap();
        drop(client);
        assert_eq!(sockets(&network).len(), 3, "its stream still holds it");
        drop(input);
        drop(accepted.take_send_stream().unwrap());
        drop(accepted);
        assert_eq!(sockets(&network), [format!("listening at {port}")]);
        drop(socket);
        assert_eq!(
            sockets(&network).len(),
            1,
            "its stream of connections still holds it"
        );
        drop(listener);
        assert!(sockets(&network).is_empty());
    }
}
