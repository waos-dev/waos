//! The devices whose WaOS announces developer access on the local network, as `waos devices`
//! lists them and `waos pair <name>` finds one: browsed for over mDNS a moment, each by its name,
//! its address and what its announcement says.
//!
//! The invariant a later editor will break: what an announcement says finds a device and nothing
//! more; trusting one is pairing's, by the fingerprint the device proves.

use std::net::IpAddr;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use waos_dev_protocol::{Announced, DeviceAddress, SERVICE};

/// How long a browse listens for announcements.
pub const BROWSE: Duration = Duration::from_secs(2);

/// A device found on the local network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundDevice {
    /// Its name, as it announces itself.
    pub name: String,
    pub address: DeviceAddress,
    pub announced: Announced,
}

/// The devices announcing developer access, heard within `wait`; `stop` ends the browse early once
/// it holds for what was found.
pub fn browse(wait: Duration, stop: impl Fn(&[FoundDevice]) -> bool) -> Result<Vec<FoundDevice>> {
    let daemon = ServiceDaemon::new().context("listening for devices on the local network")?;
    let events = daemon
        .browse(&format!("{SERVICE}.local."))
        .context("listening for devices on the local network")?;
    let deadline = Instant::now() + wait;
    let mut found: Vec<FoundDevice> = Vec::new();
    while !stop(&found) {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        let Ok(event) = events.recv_timeout(left) else {
            break;
        };
        if let ServiceEvent::ServiceResolved(service) = event
            && let Some(device) = found_device(&service)
            && !found.iter().any(|known| known.name == device.name)
        {
            found.push(device);
        }
    }
    let _ = daemon.shutdown();
    Ok(found)
}

/// The device `service` announces; none where its record is no WaOS's or names no address.
fn found_device(service: &mdns_sd::ResolvedService) -> Option<FoundDevice> {
    let announced = Announced::read(|key| service.get_property_val_str(key))?;
    let addresses: Vec<IpAddr> = service
        .get_addresses()
        .iter()
        .map(|address| address.to_ip_addr())
        .collect();
    let ip = reachable_address(&addresses)?;
    Some(FoundDevice {
        name: mdns_sd::instance_name_unescaped(service.get_fullname()),
        address: DeviceAddress {
            host: ip.to_string(),
            port: service.get_port(),
            secure: true,
        },
        announced,
    })
}

/// The address of `addresses` another machine reaches best: an IPv4 one not the loopback, as a
/// link-local IPv6 one needs its interface named; else any.
fn reachable_address(addresses: &[IpAddr]) -> Option<IpAddr> {
    let elsewhere = |ip: &&IpAddr| !ip.is_loopback();
    (addresses.iter().filter(elsewhere).find(|ip| ip.is_ipv4()))
        .or_else(|| addresses.iter().find(elsewhere))
        .or_else(|| addresses.first())
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_taken_is_one_another_machine_reaches() {
        let ips =
            |all: &[&str]| -> Vec<IpAddr> { all.iter().map(|ip| ip.parse().unwrap()).collect() };
        let taken = |all: &[&str]| reachable_address(&ips(all)).map(|ip| ip.to_string());
        assert_eq!(
            taken(&["127.0.0.1", "fe80::1", "192.168.1.20"]).as_deref(),
            Some("192.168.1.20")
        );
        assert_eq!(taken(&["127.0.0.1", "fe80::1"]).as_deref(), Some("fe80::1"));
        assert_eq!(taken(&["127.0.0.1"]).as_deref(), Some("127.0.0.1"));
        assert_eq!(taken(&[]), None);
    }
}
