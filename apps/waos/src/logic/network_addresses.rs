//! This device's addresses on the networks it is on, as the Developer card shows them for a
//! computer to reach the dev server at: IPv4 first, then IPv6, neither the loopback nor link-local,
//! whose addresses another machine does not reach without naming an interface.

use std::net::IpAddr;

/// The addresses of this device's interfaces that are up; none where the system does not say,
/// which is said.
pub(crate) fn network_addresses() -> Vec<IpAddr> {
    let interfaces = match if_addrs::get_if_addrs() {
        Ok(interfaces) => interfaces,
        Err(error) => {
            tracing::warn!(%error, "this device's network addresses cannot be read");
            return Vec::new();
        }
    };
    let reached = interfaces
        .iter()
        .filter(|interface| interface.is_oper_up() && !interface.is_loopback())
        .filter(|interface| !interface.is_link_local())
        .map(|interface| interface.ip());
    in_order(reached)
}

/// `addresses` without repeats, IPv4 first.
fn in_order(addresses: impl Iterator<Item = IpAddr>) -> Vec<IpAddr> {
    let mut ordered: Vec<IpAddr> = Vec::new();
    for address in addresses {
        if !ordered.contains(&address) {
            ordered.push(address);
        }
    }
    ordered.sort_by_key(|address| address.is_ipv6());
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_addresses_come_first_each_once() {
        let addresses = ["fd00::1", "192.168.1.20", "fd00::1", "10.0.0.2"]
            .map(|address| address.parse::<IpAddr>().unwrap());
        let ordered = in_order(addresses.into_iter());
        let said: Vec<String> = ordered.iter().map(ToString::to_string).collect();
        assert_eq!(said, ["192.168.1.20", "10.0.0.2", "fd00::1"]);
    }
}
