//! The sockets a program has open, as the system holds them: its TCP connections ([`Connection`]),
//! made, accepted, or made by the engine for its HTTP (`network::http`), which reads and writes
//! them through tokio's traits; its listeners ([`Listener`]); and its UDP sockets
//! ([`Datagrams`]). Each is a use the program makes now (`access::active_uses`), from when the
//! system's socket is made until it closes, its record what the inspector reads: its addresses,
//! the names they were looked up by, and the bytes it moved, counted as the system answers each
//! read and write, and read only as the record is.
//!
//! A count costs one relaxed atomic add beside the system call it counts, on counters only that
//! socket's own reads and writes touch, so a program streaming at full speed pays nothing it
//! could see (measured: the CPU per GiB is within 3% with the counting and without).
//!
//! The invariant: a socket's use ends exactly as the system's socket closes, when the last of
//! what shares it drops, and every byte the system took from or handed to the program through it
//! is counted once, and nothing else is.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::access::{ActiveUse, ActiveUseKind, TcpOrigin};
use crate::network::ProgramNetwork;

/// The bytes a socket moved for the program.
#[derive(Debug, Default)]
pub(crate) struct Counts {
    /// Taken by the system from the program, to send.
    sent: AtomicU64,
    /// Handed by the system to the program, received.
    received: AtomicU64,
}

impl Counts {
    pub(crate) fn add_sent(&self, bytes: usize) {
        self.sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub(crate) fn add_received(&self, bytes: usize) {
        self.received.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// The bytes sent so far.
    pub(crate) fn sent(&self) -> u64 {
        self.sent.load(Ordering::Relaxed)
    }

    /// The bytes received so far.
    pub(crate) fn received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }
}

/// A TCP connection: the system's socket, what it moved, and its use, shared by the program's
/// socket and its two streams, and closed as the last of them drops.
pub(crate) struct Connection {
    stream: tokio::net::TcpStream,
    counts: Arc<Counts>,
    _active: ActiveUse,
}

impl Connection {
    /// `stream`, connected to `remote` as `origin` says, a use of the program whose network is
    /// `network`, with the names its lookups answered `remote` for.
    pub(crate) fn new(
        stream: tokio::net::TcpStream,
        network: &ProgramNetwork,
        origin: TcpOrigin,
        remote: SocketAddr,
    ) -> Connection {
        let counts = Arc::new(Counts::default());
        let active = network.access().hold(ActiveUseKind::TcpConnection {
            origin,
            local: stream.local_addr().ok(),
            remote,
            names: network.names_of(remote.ip()),
            counts: Arc::clone(&counts),
        });
        Connection {
            stream,
            counts,
            _active: active,
        }
    }

    /// A connection to `address`, which the engine makes for the program's HTTP: its writes sent
    /// at once, as a request's are.
    pub(crate) async fn dial(
        address: SocketAddr,
        network: &ProgramNetwork,
    ) -> io::Result<Connection> {
        let stream = tokio::net::TcpStream::connect(address).await?;
        stream.set_nodelay(true)?;
        Ok(Connection::new(
            stream,
            network,
            TcpOrigin::HttpRequest,
            address,
        ))
    }

    /// The system's socket, for its addresses and options.
    pub(crate) fn stream(&self) -> &tokio::net::TcpStream {
        &self.stream
    }

    #[cfg(test)]
    pub(crate) fn counts(&self) -> &Counts {
        &self.counts
    }

    /// Reads into `buffer` once the system has bytes, answering how many, 0 once the peer is done
    /// sending, each counted.
    pub(crate) fn poll_read(
        &self,
        cx: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            match self.stream.try_read(buffer) {
                Ok(read) => {
                    self.counts.add_received(read);
                    return Poll::Ready(Ok(read));
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::task::ready!(self.stream.poll_read_ready(cx))?;
                }
                Err(error) => return Poll::Ready(Err(error)),
            }
        }
    }

    /// Writes what the system takes of `buffer` once it takes any, answering how much, each byte
    /// counted.
    pub(crate) fn poll_write(
        &self,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            match self.stream.try_write(buffer) {
                Ok(written) => {
                    self.counts.add_sent(written);
                    return Poll::Ready(Ok(written));
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::task::ready!(self.stream.poll_write_ready(cx))?;
                }
                Err(error) => return Poll::Ready(Err(error)),
            }
        }
    }
}

impl AsyncRead for Connection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let read = std::task::ready!(Connection::poll_read(
            &self,
            cx,
            buffer.initialize_unfilled()
        ))?;
        buffer.advance(read);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Connection::poll_write(&self, cx, buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

/// A TCP socket listening: the system's listener and its use, shared by the program's socket and
/// its stream of accepted connections, and closed as the last of them drops.
pub(crate) struct Listener {
    listener: tokio::net::TcpListener,
    _active: ActiveUse,
}

impl Listener {
    /// `listener`, a use of the program whose network is `network`, at the address it listens at.
    pub(crate) fn new(
        listener: tokio::net::TcpListener,
        network: &ProgramNetwork,
    ) -> io::Result<Listener> {
        let address = listener.local_addr()?;
        let active = network
            .access()
            .hold(ActiveUseKind::TcpListener { address });
        Ok(Listener {
            listener,
            _active: active,
        })
    }

    /// The system's listener.
    pub(crate) fn listener(&self) -> &tokio::net::TcpListener {
        &self.listener
    }
}

/// A UDP socket: the system's socket, what it moved, and its use, shared by the program's socket
/// and the sends and receives it started, and closed as the last of them drops.
pub(crate) struct Datagrams {
    socket: tokio::net::UdpSocket,
    counts: Counts,
    _active: ActiveUse,
}

impl Datagrams {
    /// `socket`, a use of the program whose network is `network`, whose record reads it as it is
    /// read.
    pub(crate) fn new(socket: tokio::net::UdpSocket, network: &ProgramNetwork) -> Arc<Datagrams> {
        Arc::new_cyclic(|datagrams| Datagrams {
            socket,
            counts: Counts::default(),
            _active: network.access().hold(ActiveUseKind::UdpSocket {
                socket: datagrams.clone(),
            }),
        })
    }

    /// The system's socket, for its addresses and options.
    pub(crate) fn socket(&self) -> &tokio::net::UdpSocket {
        &self.socket
    }

    pub(crate) fn counts(&self) -> &Counts {
        &self.counts
    }

    /// Sends `data` where the socket is connected, counted.
    pub(crate) async fn send(&self, data: &[u8]) -> io::Result<()> {
        let sent = self.socket.send(data).await?;
        self.counts.add_sent(sent);
        Ok(())
    }

    /// Sends `data` to `to`, counted.
    pub(crate) async fn send_to(&self, data: &[u8], to: SocketAddr) -> io::Result<()> {
        let sent = self.socket.send_to(data, to).await?;
        self.counts.add_sent(sent);
        Ok(())
    }

    /// Counts a datagram of `length` bytes handed to the program.
    pub(crate) fn handed_over(&self, length: usize) {
        self.counts.add_received(length);
    }
}
