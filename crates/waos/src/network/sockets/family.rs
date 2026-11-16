//! Which addresses a socket of each family may use, as WASI's sockets say.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::network::host::bindings::wasi::sockets::types::IpAddressFamily;

/// Whether `address` is of `family`. An IPv6 socket takes no IPv4-mapped address, since it is
/// IPv6 only, nor an IPv4-compatible one, deprecated since 2006 and handled unevenly by systems.
pub(super) fn is_of_family(address: IpAddr, family: IpAddressFamily) -> bool {
    match (family, address) {
        (IpAddressFamily::Ipv4, IpAddr::V4(_)) => true,
        (IpAddressFamily::Ipv6, IpAddr::V6(ipv6)) => {
            !is_deprecated_ipv4_compatible(ipv6) && ipv6.to_ipv4_mapped().is_none()
        }
        _ => false,
    }
}

fn is_deprecated_ipv4_compatible(address: Ipv6Addr) -> bool {
    matches!(address.segments(), [0, 0, 0, 0, 0, 0, _, _])
        && address != Ipv6Addr::UNSPECIFIED
        && address != Ipv6Addr::LOCALHOST
}

/// Whether `address` names a peer: a specified address and a port.
pub(super) fn is_remote(address: SocketAddr) -> bool {
    !address.ip().to_canonical().is_unspecified() && address.port() != 0
}

/// Whether `address` is one host's, neither multicast nor broadcast.
pub(super) fn is_unicast(address: IpAddr) -> bool {
    match address.to_canonical() {
        IpAddr::V4(ipv4) => !ipv4.is_multicast() && !ipv4.is_broadcast(),
        IpAddr::V6(ipv6) => !ipv6.is_multicast(),
    }
}

/// The wildcard address of `family`, port 0: what the system binds a socket bound implicitly to.
pub(super) fn unspecified(family: IpAddressFamily) -> SocketAddr {
    let ip = match family {
        IpAddressFamily::Ipv4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddressFamily::Ipv6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    };
    SocketAddr::new(ip, 0)
}
