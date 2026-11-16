//! A UDP socket: each datagram checked by the program's network as it is sent to, or received
//! from, its address, a received one the network refuses dropped unseen; connecting checks both,
//! since it only sets where datagrams go and come from.

use std::net::SocketAddr;
use std::sync::Arc;

use rustix::fd::AsFd;
use rustix::io::Errno;
use tracing::debug;
use wasmtime_wasi::runtime::with_ambient_tokio_runtime;

use super::connection::Datagrams;
use super::family::{is_of_family, is_remote, unspecified};
use super::options;
use super::refused_unless;
use crate::network::ProgramNetwork;
use crate::network::host::bindings::wasi::sockets::types::{ErrorCode, IpAddressFamily};

/// The largest datagram a program may send, as UDP's length field can say it; the networks it
/// crosses take far less.
const MAX_DATAGRAM_SIZE: usize = u16::MAX as usize;

/// A program's UDP socket, shared with the sends and receives it has started.
pub struct UdpSocket {
    datagrams: Arc<Datagrams>,
    family: IpAddressFamily,
    /// What checks each address the socket uses.
    network: ProgramNetwork,
    /// Whether the socket is bound, which it never stops being once it is: kept, so each send
    /// and receive need not ask the system.
    is_bound: bool,
    /// Where it is connected, kept so each send need not ask the system.
    remote_address: Option<SocketAddr>,
}

impl UdpSocket {
    /// A new socket of `family`, its addresses checked by `network`, writable once it answers.
    pub(crate) async fn new(
        network: ProgramNetwork,
        family: IpAddressFamily,
    ) -> Result<UdpSocket, ErrorCode> {
        let socket = with_ambient_tokio_runtime(|| socket(family))?;
        // A socket is writable at once, which programs count on; tokio marks it so a moment
        // later (wasmtime issue 12612), so its first send waits for that here.
        socket.writable().await?;
        Ok(UdpSocket {
            datagrams: Arc::new(Datagrams::new(socket)),
            family,
            network,
            is_bound: false,
            remote_address: None,
        })
    }

    pub(crate) fn is_bound(&mut self) -> bool {
        if !self.is_bound {
            self.is_bound = self
                .datagrams
                .socket()
                .local_addr()
                .is_ok_and(|address| address != unspecified(self.family));
        }
        self.is_bound
    }

    pub(crate) fn local_address(&mut self) -> Result<SocketAddr, ErrorCode> {
        if !self.is_bound() {
            return Err(ErrorCode::InvalidState);
        }
        Ok(self.datagrams.socket().local_addr()?)
    }

    pub(crate) fn remote_address(&self) -> Result<SocketAddr, ErrorCode> {
        self.remote_address.ok_or(ErrorCode::InvalidState)
    }

    pub(crate) fn bind(&mut self, address: SocketAddr) -> Result<(), ErrorCode> {
        if self.is_bound() {
            return Err(ErrorCode::InvalidState);
        }
        if !is_of_family(address.ip(), self.family) {
            return Err(ErrorCode::InvalidArgument);
        }
        Ok(bind(self.datagrams.socket(), address)?)
    }

    pub(crate) fn connect(&mut self, address: SocketAddr) -> Result<(), ErrorCode> {
        if !is_of_family(address.ip(), self.family) || !is_remote(address) {
            return Err(ErrorCode::InvalidArgument);
        }
        refused_unless(self.network.allows_send_to(address))?;
        refused_unless(self.network.allows_receive_from(address))?;
        let connected = connect(self.datagrams.socket(), address);
        self.update_remote_address();
        Ok(connected?)
    }

    pub(crate) fn disconnect(&mut self) -> Result<(), ErrorCode> {
        if self.remote_address.is_none() {
            return Err(ErrorCode::InvalidState);
        }
        // Linux may give up the port of a socket bound to the wildcard address as it
        // disconnects, which would read as unbound; it was bound, and stays so.
        self.is_bound = true;
        let disconnected = disconnect(self.datagrams.socket());
        self.update_remote_address();
        Ok(disconnected?)
    }

    /// Keeps where the socket is connected as the system says, after a call that may change it.
    fn update_remote_address(&mut self) {
        self.remote_address = self
            .datagrams
            .socket()
            .peer_addr()
            .ok()
            .filter(|address| *address != unspecified(self.family));
    }

    /// Sends `data` to `address`, or where the socket is connected, which `address` must then be.
    pub(crate) fn send(
        &mut self,
        data: Vec<u8>,
        address: Option<SocketAddr>,
    ) -> impl Future<Output = Result<(), ErrorCode>> + Send + use<> {
        let family = self.family;
        let datagrams = Arc::clone(&self.datagrams);
        let network = self.network.clone();
        let connected = self.remote_address;
        async move {
            if data.len() > MAX_DATAGRAM_SIZE {
                return Err(ErrorCode::DatagramTooLarge);
            }
            let to = match (address, connected) {
                (Some(address), _)
                    if !is_remote(address) || !is_of_family(address.ip(), family) =>
                {
                    return Err(ErrorCode::InvalidArgument);
                }
                (Some(address), Some(connected)) if address != connected => {
                    return Err(ErrorCode::InvalidArgument);
                }
                (Some(address), _) => address,
                (None, Some(connected)) => connected,
                (None, None) => return Err(ErrorCode::InvalidArgument),
            };
            refused_unless(network.allows_send_to(to))?;
            if connected == Some(to) {
                datagrams.send(&data).await?;
            } else {
                datagrams.send_to(&data, to).await?;
            }
            Ok(())
        }
    }

    /// The next datagram from an address the program's network allows, and that address.
    pub(crate) fn receive(
        &mut self,
    ) -> impl Future<Output = Result<(Vec<u8>, SocketAddr), ErrorCode>> + Send + use<> {
        let datagrams = Arc::clone(&self.datagrams);
        let network = self.network.clone();
        let is_bound = self.is_bound();
        async move {
            if !is_bound {
                return Err(ErrorCode::InvalidState);
            }
            loop {
                let mut data = vec![0; MAX_DATAGRAM_SIZE];
                let (length, from) = datagrams.socket().recv_from(&mut data).await?;
                data.truncate(length);
                if network.allows_receive_from(from) {
                    datagrams.handed_over(length);
                    return Ok((data, from));
                }
            }
        }
    }

    pub(crate) fn address_family(&self) -> IpAddressFamily {
        self.family
    }

    pub(crate) fn unicast_hop_limit(&self) -> Result<u8, ErrorCode> {
        options::unicast_hop_limit(self.datagrams.socket(), self.family)
    }

    pub(crate) fn set_unicast_hop_limit(&self, value: u8) -> Result<(), ErrorCode> {
        options::set_unicast_hop_limit(self.datagrams.socket(), self.family, value)
    }

    pub(crate) fn receive_buffer_size(&self) -> Result<u64, ErrorCode> {
        options::receive_buffer_size(self.datagrams.socket())
    }

    pub(crate) fn set_receive_buffer_size(&self, value: u64) -> Result<(), ErrorCode> {
        options::set_receive_buffer_size(self.datagrams.socket(), value).map(|_| ())
    }

    pub(crate) fn send_buffer_size(&self) -> Result<u64, ErrorCode> {
        options::send_buffer_size(self.datagrams.socket())
    }

    pub(crate) fn set_send_buffer_size(&self, value: u64) -> Result<(), ErrorCode> {
        options::set_send_buffer_size(self.datagrams.socket(), value).map(|_| ())
    }
}

/// A non-blocking UDP socket of `family`, closed on `exec`; an IPv6 one IPv6 only, as WASI's
/// sockets have it.
fn socket(family: IpAddressFamily) -> std::io::Result<tokio::net::UdpSocket> {
    // The standard library calls `WSAStartup` before its first socket; this makes it.
    #[cfg(windows)]
    {
        static STARTED: std::sync::Once = std::sync::Once::new();
        STARTED.call_once(|| {
            let _ = std::net::TcpStream::connect(std::net::SocketAddrV4::new(
                std::net::Ipv4Addr::UNSPECIFIED,
                0,
            ));
        });
    }
    #[cfg(not(any(windows, target_vendor = "apple")))]
    let flags = rustix::net::SocketFlags::CLOEXEC | rustix::net::SocketFlags::NONBLOCK;
    #[cfg(any(windows, target_vendor = "apple"))]
    let flags = rustix::net::SocketFlags::empty();
    let socket = rustix::net::socket_with(
        match family {
            IpAddressFamily::Ipv4 => rustix::net::AddressFamily::INET,
            IpAddressFamily::Ipv6 => rustix::net::AddressFamily::INET6,
        },
        rustix::net::SocketType::DGRAM,
        flags,
        None,
    )?;
    #[cfg(target_vendor = "apple")]
    rustix::io::ioctl_fioclex(&socket)?;
    #[cfg(any(windows, target_vendor = "apple"))]
    rustix::io::ioctl_fionbio(&socket, true)?;
    if family == IpAddressFamily::Ipv6 {
        rustix::net::sockopt::set_ipv6_v6only(&socket, true)?;
    }
    tokio::net::UdpSocket::try_from(std::net::UdpSocket::from(socket))
}

fn bind(socket: impl AsFd, address: SocketAddr) -> Result<(), Errno> {
    rustix::net::bind(socket, &address).map_err(|error| match error {
        // Windows' answer when the ephemeral ports run out.
        #[cfg(windows)]
        Errno::NOBUFS => Errno::ADDRINUSE,
        // A family the checks above let through.
        Errno::AFNOSUPPORT => Errno::INVAL,
        _ => error,
    })
}

/// Connects `socket` to `address`. Most systems move a connected socket's local address to the
/// interface its new peer is reached through; Linux refuses, so there it is disconnected and
/// connected again.
fn connect(socket: impl AsFd, address: SocketAddr) -> Result<(), Errno> {
    match rustix::net::connect(socket.as_fd(), &address) {
        #[cfg(target_os = "linux")]
        Err(Errno::INVAL) => {
            _ = disconnect(socket.as_fd());
            rustix::net::connect(socket.as_fd(), &address)
        }
        // A family the checks above let through.
        Err(Errno::AFNOSUPPORT) => Err(Errno::INVAL),
        // Only a non-blocking TCP connect answers this.
        Err(Errno::INPROGRESS) => {
            debug!("UDP connect returned EINPROGRESS, which should never happen");
            Ok(())
        }
        connected => connected,
    }
}

/// Disconnects `socket`: macOS answers `EINVAL` or `EAFNOSUPPORT` though it disconnected.
fn disconnect(socket: impl AsFd) -> Result<(), Errno> {
    match rustix::net::connect_unspec(socket) {
        #[cfg(target_os = "macos")]
        Err(Errno::INVAL | Errno::AFNOSUPPORT) => Ok(()),
        disconnected => disconnected,
    }
}
