//! What a program may reach of the network, read from its grants at each check (decisions 12
//! and 13 of the permissions proposal). The network switch and its domain rule guard the
//! internet; the local-network switch guards this computer and the local network
//! ([`addresses`]), which no name a domain rule could refuse stands in front of.
//!
//! A name is looked up by the engine itself ([`host`], `wasi:sockets/ip-name-lookup`). `localhost` and the names under it are this computer on every system ([`localhost`]):
//! answered by the engine, never by the system's resolver, under the local-network switch as
//! `127.0.0.1` is, whatever the network switch and its domain rule say. Any other name the domain
//! rule refuses answers `access-denied`; one it allows is resolved by the system's resolver, and
//! its addresses are kept with the name ([`record`]). With the network refused and the local
//! network allowed, such a name answers only the addresses of this computer and the local network
//! it resolves to. TCP and UDP, of both versions, are
//! `wasmtime-wasi`'s, checked address by address: a local address needs the local-network switch; any other needs the
//! network switch, then its rule, by the names the address was looked up by: any domain allows
//! it, only some allows it when one of its names is among them, so a bare address is refused,
//! and all but some refuses it when one of its names is among them.
//!
//! HTTP is checked the same way ([`http`]): reqwest resolves a request's host through the
//! program's checked lookup ([`ProgramNetwork::reachable`]), so the request connects to the
//! addresses that passed the rules, resolved once. Where a proxy would carry a request, the
//! engine runs that lookup itself before picking the way: an address of this computer or the
//! local network is connected to directly, and only an internet one goes through the proxy,
//! which looks the name up again.
//!
//! The invariant: nothing here holds a verdict. Every check reads the grants as they are, so a
//! switch turned off refuses the next lookup, connection or datagram, while a connection open
//! already carries on.

mod addresses;
pub(crate) mod host;
mod http;
mod localhost;
mod record;

use std::fmt;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::str::FromStr;
use std::sync::Arc;

use waos_capabilities::{DomainRule, ProgramGrants};
use wasmtime_wasi::sockets::SocketAddrUse;

use addresses::is_local;
pub use http::HttpProxy;
pub(crate) use http::ProgramHttp;
use localhost::localhost_addresses;
use record::AddressRecord;

/// One program's network: its grants, read at each check, the resolver its names are resolved
/// with, and the addresses its lookups answered. Clones share the record.
#[derive(Clone)]
pub(crate) struct ProgramNetwork {
    grants: Arc<dyn ProgramGrants>,
    resolver: Resolver,
    record: Arc<AddressRecord>,
}

/// What resolves a name to its addresses, each once: the system's resolver, which a test
/// replaces with answers of its own.
pub(crate) type Resolver = Arc<dyn Fn(&str) -> Result<Vec<IpAddr>, LookupError> + Send + Sync>;

/// How a lookup goes, decided before the system's resolver is asked.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Lookup {
    /// The name's addresses are known without asking the system: an address answers itself, and
    /// `localhost` or a name under it answers this computer's loopback addresses.
    Known(Vec<IpAddr>),
    /// The system resolves the name, in ASCII and lower case, never a localhost name. `by_rule`
    /// when the domain rule lets the program reach it, and every address is answered and
    /// recorded; otherwise only the local addresses are answered, under the local-network switch.
    Resolve { name: String, by_rule: bool },
}

/// Why a lookup answers no address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LookupError {
    /// The program may not reach the name.
    Refused,
    /// The text is no host name.
    Invalid,
    /// The system's resolver found no address for it.
    Unresolvable,
}

impl ProgramNetwork {
    /// The network of a program with `grants`, its names resolved by the system's resolver.
    pub(crate) fn new(grants: Arc<dyn ProgramGrants>) -> ProgramNetwork {
        ProgramNetwork::resolving_with(grants, Arc::new(system_resolve))
    }

    /// The network of a program with `grants`, its names resolved by `resolver`.
    pub(crate) fn resolving_with(
        grants: Arc<dyn ProgramGrants>,
        resolver: Resolver,
    ) -> ProgramNetwork {
        ProgramNetwork {
            grants,
            resolver,
            record: Arc::default(),
        }
    }

    /// How a lookup of `name` goes, or why it is refused: an address answers itself; a localhost
    /// name answers this computer's loopback addresses when the local network is allowed; any
    /// other name is resolved when the domain rule allows it, or when the local network is
    /// allowed, for its local addresses alone.
    pub(crate) fn plan_lookup(&self, name: &str) -> Result<Lookup, LookupError> {
        let name = match host_of(name)? {
            url::Host::Domain(name) => name,
            url::Host::Ipv4(address) => return Ok(Lookup::Known(vec![address.into()])),
            url::Host::Ipv6(address) => return Ok(Lookup::Known(vec![address.into()])),
        };
        match localhost_addresses(&name) {
            Some(loopback) => self.plan_localhost_lookup(loopback),
            None => self.plan_system_lookup(name),
        }
    }

    /// A localhost name's lookup: its loopback addresses, under the local-network switch alone.
    fn plan_localhost_lookup(&self, loopback: Vec<IpAddr>) -> Result<Lookup, LookupError> {
        if self.grants.local_network() {
            Ok(Lookup::Known(loopback))
        } else {
            Err(LookupError::Refused)
        }
    }

    /// The lookup of `name`, which the system resolves: in full when the domain rule allows it,
    /// for its local addresses alone when only the local network is allowed.
    fn plan_system_lookup(&self, name: String) -> Result<Lookup, LookupError> {
        let network = self.grants.network();
        let by_rule = network.on && network.domains.allows(&name);
        if by_rule || self.grants.local_network() {
            Ok(Lookup::Resolve { name, by_rule })
        } else {
            Err(LookupError::Refused)
        }
    }

    /// The addresses the system resolves `name` to, as [`Lookup::Resolve`] says: all of them,
    /// recorded with the name, or the local ones alone, a name that has none being refused as
    /// the rule refuses it, whether it resolves or not. Blocks while the system resolves.
    pub(crate) fn resolve(&self, name: &str, by_rule: bool) -> Result<Vec<IpAddr>, LookupError> {
        if by_rule {
            let addresses = (self.resolver)(name)?;
            self.record.keep(name, &addresses);
            return Ok(addresses);
        }
        let local: Vec<IpAddr> = (self.resolver)(name)
            .unwrap_or_default()
            .into_iter()
            .filter(|address| is_local(*address))
            .collect();
        if local.is_empty() {
            Err(LookupError::Refused)
        } else {
            Ok(local)
        }
    }

    /// The addresses of `host` a connection of the program may go to: an address, if the
    /// program may reach it; a name, looked up as [`plan_lookup`](Self::plan_lookup) and
    /// [`resolve`](Self::resolve) look it up, and only the addresses it answered that the
    /// program may reach. None of them is refused. Blocks while the system resolves.
    pub(crate) fn reachable(&self, host: &str) -> Result<Vec<IpAddr>, LookupError> {
        let addresses = match self.plan_lookup(host)? {
            Lookup::Known(addresses) => addresses,
            Lookup::Resolve { name, by_rule } => self.resolve(&name, by_rule)?,
        };
        let reachable: Vec<IpAddr> = addresses
            .into_iter()
            .filter(|address| self.may_reach(*address))
            .collect();
        if reachable.is_empty() {
            Err(LookupError::Refused)
        } else {
            Ok(reachable)
        }
    }

    /// Whether the program may use `address` as `use_` says, `wasmtime-wasi`'s check of every
    /// socket address: binding a port reaches nothing; listening is reached from this computer
    /// and the local network; connecting, accepting, sending and receiving reach the address.
    pub(crate) fn allows_socket(&self, address: SocketAddr, use_: SocketAddrUse) -> bool {
        match use_ {
            SocketAddrUse::TcpBind | SocketAddrUse::UdpBind => true,
            SocketAddrUse::TcpListen => self.grants.local_network(),
            SocketAddrUse::TcpConnect
            | SocketAddrUse::TcpAccept
            | SocketAddrUse::UdpSend
            | SocketAddrUse::UdpReceive => self.may_reach(address.ip()),
        }
    }

    /// Whether the program may reach `address`: a local one under the local-network switch, any
    /// other under the network switch and its rule, by the names the address was looked up by.
    pub(crate) fn may_reach(&self, address: IpAddr) -> bool {
        if is_local(address) {
            return self.grants.local_network();
        }
        let network = self.grants.network();
        network.on && self.rule_allows(&network.domains, address)
    }

    /// Whether `rule` allows `address`, which it judges by the names the address was looked up by.
    fn rule_allows(&self, rule: &DomainRule, address: IpAddr) -> bool {
        let names = self.record.names_of(address);
        match rule {
            DomainRule::Any => true,
            DomainRule::Only(_) => names.iter().any(|name| rule.allows(name)),
            DomainRule::AnyBut(_) => names.iter().all(|name| rule.allows(name)),
        }
    }
}

/// `name` as a host: an address, or a domain in ASCII and lower case. A bare IPv6 address, which
/// a URL writes in brackets, is taken too.
fn host_of(name: &str) -> Result<url::Host, LookupError> {
    match url::Host::parse(name) {
        Ok(host) => Ok(host),
        Err(_) => std::net::Ipv6Addr::from_str(name)
            .map(url::Host::Ipv6)
            .map_err(|_| LookupError::Invalid),
    }
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LookupError::Refused => "the program may not reach this host",
            LookupError::Invalid => "this is no host name",
            LookupError::Unresolvable => "no address was found for this name",
        })
    }
}

impl std::error::Error for LookupError {}

/// The addresses of `name`, resolved for WaOS's own way out rather than for a host the program
/// named, so unchecked: a localhost name's loopback addresses, or what the system's resolver
/// answers.
pub(crate) fn resolve_unchecked(name: &str) -> Result<Vec<IpAddr>, LookupError> {
    match localhost_addresses(name) {
        Some(loopback) => Ok(loopback),
        None => system_resolve(name),
    }
}

/// The addresses the system's resolver answers for `name`, each once, in its order.
pub(crate) fn system_resolve(name: &str) -> Result<Vec<IpAddr>, LookupError> {
    let answered = (name, 0)
        .to_socket_addrs()
        .map_err(|_| LookupError::Unresolvable)?;
    let mut addresses: Vec<IpAddr> = Vec::new();
    for address in answered.map(|address| address.ip().to_canonical()) {
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    Ok(addresses)
}

#[cfg(test)]
pub(crate) mod test_grants;
#[cfg(test)]
mod tests;
