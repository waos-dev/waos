//! Which addresses are this computer's and the local network's, which the local-network switch
//! guards (decision 12 of the permissions proposal): loopback, the private ranges, link-local
//! and multicast addresses, and those that stand for this computer or every machine on the
//! network, the unspecified address and the broadcast address. Every other address is the
//! internet's, which the network switch guards.
//!
//! The invariant: an IPv4 address written as IPv6 (`::ffff:a.b.c.d`) is judged as the IPv4
//! address it is, so the form a program writes it in changes nothing.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Whether `address` is this computer's or the local network's.
pub(crate) fn is_local(address: IpAddr) -> bool {
    match address.to_canonical() {
        IpAddr::V4(address) => is_local_v4(address),
        IpAddr::V6(address) => is_local_v6(address),
    }
}

fn is_local_v4(address: Ipv4Addr) -> bool {
    address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_multicast()
        || address.is_unspecified()
        || address.is_broadcast()
}

fn is_local_v6(address: Ipv6Addr) -> bool {
    address.is_loopback()
        || address.is_unique_local()
        || address.is_unicast_link_local()
        || address.is_multicast()
        || address.is_unspecified()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_computer_and_the_local_network_are_local_and_the_rest_is_not() {
        let local = [
            "127.0.0.1",
            "127.8.9.10",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.1",
            "169.254.10.20",
            "224.0.0.251",
            "239.255.255.250",
            "0.0.0.0",
            "255.255.255.255",
            "::1",
            "::",
            "fd12:3456::1",
            "fe80::1",
            "ff02::fb",
            "::ffff:192.168.1.1",
            "::ffff:127.0.0.1",
        ];
        for address in local {
            assert!(is_local(address.parse().unwrap()), "{address}");
        }
        let internet = [
            "93.184.216.34",
            "8.8.8.8",
            "172.32.0.1",
            "100.64.0.1",
            "2606:2800:220:1:248:1893:25c8:1946",
            "::ffff:8.8.8.8",
        ];
        for address in internet {
            assert!(!is_local(address.parse().unwrap()), "{address}");
        }
    }
}
