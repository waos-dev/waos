//! A connection of a program's, made or accepted: the system's socket, and the bytes it moved,
//! counted as the system answers each read and write. The program's TCP streams read and write
//! it ([`super::tcp`]); a datagram socket counts its datagrams the same way ([`Datagrams`]).
//!
//! A count costs one relaxed atomic add beside the system call it counts, on counters only that
//! connection's own reads and writes touch, so a program streaming at full speed pays nothing it
//! could see (`../experiments/2026-11-16-waos-socket-counting`).
//!
//! The invariant: every byte the system took from or handed to the program through the socket is
//! counted once, and nothing else is.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use tracing::debug;

/// The bytes a socket moved for the program.
#[derive(Default)]
pub(crate) struct Counts {
    /// Taken by the system from the program, to send.
    sent: AtomicU64,
    /// Handed by the system to the program, received.
    received: AtomicU64,
}

impl Counts {
    fn add_sent(&self, bytes: usize) {
        self.sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    fn add_received(&self, bytes: usize) {
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

/// A TCP connection: the system's socket and what it moved, shared by the program's socket and
/// its two streams, and closed as the last of them drops.
pub(crate) struct Connection {
    // TODO(connection table): the connection's entry in what the program holds
    // (`access::holdings`), held here, so the entry ends as the system's socket closes.
    stream: tokio::net::TcpStream,
    counts: Counts,
}

impl Connection {
    pub(crate) fn new(stream: tokio::net::TcpStream) -> Connection {
        Connection {
            stream,
            counts: Counts::default(),
        }
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

impl Drop for Connection {
    fn drop(&mut self) {
        debug!(
            peer = ?self.stream.peer_addr().ok(),
            sent = self.counts.sent(),
            received = self.counts.received(),
            "a program's connection closed"
        );
    }
}

/// A UDP socket: the system's socket and what it moved, shared by the program's socket and the
/// sends and receives it started.
pub(crate) struct Datagrams {
    // TODO(connection table): the socket's entry in what the program holds (`access::holdings`),
    // held here, so it ends as the system's socket closes.
    socket: tokio::net::UdpSocket,
    counts: Counts,
}

impl Datagrams {
    pub(crate) fn new(socket: tokio::net::UdpSocket) -> Datagrams {
        Datagrams {
            socket,
            counts: Counts::default(),
        }
    }

    /// The system's socket, for its addresses and options.
    pub(crate) fn socket(&self) -> &tokio::net::UdpSocket {
        &self.socket
    }

    /// Sends `data` where the socket is connected, counted.
    pub(crate) async fn send(&self, data: &[u8]) -> io::Result<()> {
        let sent = self.socket.send(data).await?;
        self.counts.add_sent(sent);
        Ok(())
    }

    /// Sends `data` to `to`, counted.
    pub(crate) async fn send_to(&self, data: &[u8], to: std::net::SocketAddr) -> io::Result<()> {
        let sent = self.socket.send_to(data, to).await?;
        self.counts.add_sent(sent);
        Ok(())
    }

    /// Counts a datagram of `length` bytes handed to the program.
    pub(crate) fn handed_over(&self, length: usize) {
        self.counts.add_received(length);
    }
}

impl Drop for Datagrams {
    fn drop(&mut self) {
        debug!(
            local = ?self.socket.local_addr().ok(),
            sent = self.counts.sent(),
            received = self.counts.received(),
            "a program's UDP socket closed"
        );
    }
}
