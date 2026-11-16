//! WASI 0.3's addresses as the system's and back.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use super::bindings::wasi::sockets::types::{
    IpAddress, IpSocketAddress, Ipv4SocketAddress, Ipv6SocketAddress,
};

impl From<IpAddr> for IpAddress {
    fn from(address: IpAddr) -> IpAddress {
        match address {
            IpAddr::V4(v4) => IpAddress::Ipv4(v4.octets().into()),
            IpAddr::V6(v6) => IpAddress::Ipv6(segments_of(v6)),
        }
    }
}

impl From<IpSocketAddress> for SocketAddr {
    fn from(address: IpSocketAddress) -> SocketAddr {
        match address {
            IpSocketAddress::Ipv4(v4) => SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::from(<[u8; 4]>::from(v4.address)),
                v4.port,
            )),
            IpSocketAddress::Ipv6(v6) => SocketAddr::V6(SocketAddrV6::new(
                ipv6_of(v6.address),
                v6.port,
                v6.flow_info,
                v6.scope_id,
            )),
        }
    }
}

impl From<SocketAddr> for IpSocketAddress {
    fn from(address: SocketAddr) -> IpSocketAddress {
        match address {
            SocketAddr::V4(v4) => IpSocketAddress::Ipv4(Ipv4SocketAddress {
                address: v4.ip().octets().into(),
                port: v4.port(),
            }),
            SocketAddr::V6(v6) => IpSocketAddress::Ipv6(Ipv6SocketAddress {
                address: segments_of(*v6.ip()),
                port: v6.port(),
                flow_info: v6.flowinfo(),
                scope_id: v6.scope_id(),
            }),
        }
    }
}

type Segments = (u16, u16, u16, u16, u16, u16, u16, u16);

fn segments_of(address: Ipv6Addr) -> Segments {
    let [x0, x1, x2, x3, x4, x5, x6, x7] = address.segments();
    (x0, x1, x2, x3, x4, x5, x6, x7)
}

fn ipv6_of(segments: Segments) -> Ipv6Addr {
    let (x0, x1, x2, x3, x4, x5, x6, x7) = segments;
    Ipv6Addr::new(x0, x1, x2, x3, x4, x5, x6, x7)
}
