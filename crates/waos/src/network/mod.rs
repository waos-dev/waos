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
//! it resolves to. TCP and UDP are the engine's own too ([`sockets`]), checked address by address
//! at each use: a local address needs the local-network switch; any other needs the
//! network switch, then its rule, by the names the address was looked up by: any domain allows
//! it, only some allows it when one of its names is among them, so a bare address is refused,
//! and all but some refuses it when one of its names is among them.
//!
//! HTTP is checked the same way ([`http`]): a request's connection resolves its host through the
//! program's checked lookup ([`ProgramNetwork::reachable`]), so the request connects to the
//! addresses that passed the rules, resolved once. Where a proxy would carry a request, the
//! engine runs that lookup itself before picking the way: an address of this computer or the
//! local network is connected to directly, and only an internet one goes through the proxy,
//! which looks the name up again.
//!
//! Each check is told through the program's access as the use it is, the check's own function
//! being the rule, whole: a lookup ([`ProgramNetwork::look_up`], [`ProgramNetwork::reachable`]),
//! with what it was for ([`Cause`]); a connection, an accepted one and a listening socket decided;
//! a datagram noted. A refusal names the setting that refused: `local-network` for this computer
//! and the local network, `network` for the internet while the switch is off, and `domains` for a
//! name or an address the domain rule refuses.
//!
//! The invariant: nothing here holds a verdict. Every check reads the grants as they are, so a
//! switch turned off refuses the next lookup, connection or datagram, while a connection open
//! already carries on.

mod addresses;
pub(crate) mod host;
mod http;
mod localhost;
mod record;
mod sockets;

use std::fmt;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::str::FromStr;
use std::sync::Arc;

use waos_capabilities::DomainRule;

use crate::access::{Asked, Cause, Noted, ProgramAccess, Refusal, Refused};

pub(crate) use addresses::is_local;
pub use http::HttpProxy;
pub(crate) use http::ProgramHttp;
use localhost::localhost_addresses;
use record::AddressRecord;

/// One program's network: its access, where each check is decided and its grants read, the
/// resolver its names are resolved with, and the addresses its lookups answered. Clones share the
/// record.
#[derive(Clone)]
pub(crate) struct ProgramNetwork {
    access: ProgramAccess,
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
    /// The program may not reach the name, as the setting refused it.
    Refused(Refusal),
    /// The text is no host name.
    Invalid,
    /// The system's resolver found no address for it.
    Unresolvable,
}

/// What refuses this computer and the local network.
const LOCAL_NETWORK: Refusal = Refusal::by("local-network");

impl ProgramNetwork {
    /// The network of the program `access` is of, its names resolved by the system's resolver.
    pub(crate) fn new(access: ProgramAccess) -> ProgramNetwork {
        ProgramNetwork::resolving_with(access, Arc::new(system_resolve))
    }

    /// The network of the program `access` is of, its names resolved by `resolver`.
    pub(crate) fn resolving_with(access: ProgramAccess, resolver: Resolver) -> ProgramNetwork {
        ProgramNetwork {
            access,
            resolver,
            record: Arc::default(),
        }
    }

    /// The access of the program whose network this is.
    pub(crate) fn access(&self) -> &ProgramAccess {
        &self.access
    }

    /// The addresses of `name` the program's own lookup answers, as [`plan_lookup`] and
    /// [`resolve`] answer them: decided and told as one use. Blocks while the system resolves.
    ///
    /// [`plan_lookup`]: Self::plan_lookup
    /// [`resolve`]: Self::resolve
    pub(crate) fn look_up(&self, name: &str) -> Result<Vec<IpAddr>, LookupError> {
        let asked = Asked::LookUpName {
            name,
            cause: Cause::Program,
        };
        self.access
            .decide(asked, |_| match self.plan_lookup(name)? {
                Lookup::Known(addresses) => Ok(addresses),
                Lookup::Resolve { name, by_rule } => self.resolve(&name, by_rule),
            })
    }

    /// How a lookup of `name` goes, or why it is refused: an address answers itself; a localhost
    /// name answers this computer's loopback addresses when the local network is allowed; any
    /// other name is resolved when the domain rule allows it, or when the local network is
    /// allowed, for its local addresses alone.
    fn plan_lookup(&self, name: &str) -> Result<Lookup, LookupError> {
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
        if self.access.grants().local_network() {
            Ok(Lookup::Known(loopback))
        } else {
            Err(LookupError::Refused(LOCAL_NETWORK))
        }
    }

    /// The lookup of `name`, which the system resolves: in full when the domain rule allows it,
    /// for its local addresses alone when only the local network is allowed.
    fn plan_system_lookup(&self, name: String) -> Result<Lookup, LookupError> {
        let network = self.access.grants().network();
        let by_rule = network.on && network.domains.allows(&name);
        if by_rule || self.access.grants().local_network() {
            Ok(Lookup::Resolve { name, by_rule })
        } else {
            Err(LookupError::Refused(self.internet_refusal()))
        }
    }

    /// What refuses the internet now: the network switch while it is off, the domain rule
    /// otherwise.
    fn internet_refusal(&self) -> Refusal {
        if self.access.grants().network().on {
            Refusal::by("domains")
        } else {
            Refusal::by("network")
        }
    }

    /// The addresses the system resolves `name` to, as [`Lookup::Resolve`] says: all of them,
    /// recorded with the name, or the local ones alone, a name that has none being refused as
    /// the rule refuses it, whether it resolves or not. Blocks while the system resolves.
    fn resolve(&self, name: &str, by_rule: bool) -> Result<Vec<IpAddr>, LookupError> {
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
            Err(LookupError::Refused(self.internet_refusal()))
        } else {
            Ok(local)
        }
    }

    /// The addresses of `host` a connection of the program may go to, made for `cause`: an
    /// address, if the program may reach it; a name, looked up as
    /// [`plan_lookup`](Self::plan_lookup) and [`resolve`](Self::resolve) look it up, and only the
    /// addresses it answered that the program may reach. None of them is refused. Decided and told
    /// as one use. Blocks while the system resolves.
    pub(crate) fn reachable(&self, host: &str, cause: Cause) -> Result<Vec<IpAddr>, LookupError> {
        let asked = Asked::LookUpName { name: host, cause };
        self.access.decide(asked, |_| self.reachable_by_rules(host))
    }

    fn reachable_by_rules(&self, host: &str) -> Result<Vec<IpAddr>, LookupError> {
        let addresses = match self.plan_lookup(host)? {
            Lookup::Known(addresses) => addresses,
            Lookup::Resolve { name, by_rule } => self.resolve(&name, by_rule)?,
        };
        let mut refused = None;
        let mut reachable = Vec::with_capacity(addresses.len());
        for address in addresses {
            match self.reach(address) {
                Ok(()) => reachable.push(address),
                Err(refusal) => refused = refused.or(Some(refusal)),
            }
        }
        match refused {
            Some(refusal) if reachable.is_empty() => Err(LookupError::Refused(refusal)),
            _ if reachable.is_empty() => Err(LookupError::Refused(self.internet_refusal())),
            _ => Ok(reachable),
        }
    }

    /// Whether the program may listen at `address`, which is reached from this computer and the
    /// local network, so under the local-network switch: decided and told. Its sockets ask it as
    /// they listen ([`sockets`]); a bind, which reaches nothing, asks nothing.
    pub(crate) fn allows_listen(&self, address: SocketAddr) -> bool {
        let listened = || match self.access.grants().local_network() {
            true => Ok(()),
            false => Err(LOCAL_NETWORK),
        };
        self.access
            .decide(Asked::ListenTcp { address }, |_| listened())
            .is_ok()
    }

    /// Whether the program may connect to `address`: decided and told, with the names the
    /// address was looked up by.
    pub(crate) fn allows_connect(&self, address: SocketAddr) -> bool {
        self.access
            .decide(Asked::ConnectTcp { address }, |_| self.named(address))
            .is_ok()
    }

    /// Whether the program may take a connection from `address`, which its listener accepted:
    /// decided and told, with the names the address was looked up by.
    pub(crate) fn allows_accept(&self, address: SocketAddr) -> bool {
        self.access
            .decide(Asked::AcceptTcp { address }, |_| self.named(address))
            .is_ok()
    }

    /// Whether the program may send a datagram to `address`: noted, as each datagram is checked
    /// on its own.
    pub(crate) fn allows_send_to(&self, address: SocketAddr) -> bool {
        self.allows_datagram(address, true)
    }

    /// Whether the program may be handed a datagram from `address`: noted, as each datagram is
    /// checked on its own.
    pub(crate) fn allows_receive_from(&self, address: SocketAddr) -> bool {
        self.allows_datagram(address, false)
    }

    fn allows_datagram(&self, address: SocketAddr, sent: bool) -> bool {
        let refusal = self.reach(address.ip()).err();
        self.access.note(Noted::UdpDatagram {
            address,
            sent,
            refusal,
        });
        refusal.is_none()
    }

    /// The names `address` was looked up by, if the program may reach it: what a connection
    /// resolved to.
    fn named(&self, address: SocketAddr) -> Result<Vec<String>, Refusal> {
        self.reach(address.ip())
            .map(|()| self.record.names_of(address.ip()))
    }

    /// Whether the program may reach `address`, or what refuses it: a local one under the
    /// local-network switch, any other under the network switch and its rule, by the names the
    /// address was looked up by.
    fn reach(&self, address: IpAddr) -> Result<(), Refusal> {
        if is_local(address) {
            return match self.access.grants().local_network() {
                true => Ok(()),
                false => Err(LOCAL_NETWORK),
            };
        }
        let network = self.access.grants().network();
        if !network.on {
            Err(Refusal::by("network"))
        } else if self.rule_allows(&network.domains, address) {
            Ok(())
        } else {
            Err(Refusal::by("domains"))
        }
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
            LookupError::Refused(_) => "the program may not reach this host",
            LookupError::Invalid => "this is no host name",
            LookupError::Unresolvable => "no address was found for this name",
        })
    }
}

impl std::error::Error for LookupError {}

impl Refused for LookupError {
    /// The setting of a refusal; none for a name that is no host, or one that has no address.
    fn setting(&self) -> Option<&'static str> {
        match self {
            LookupError::Refused(refusal) => Some(refusal.setting),
            LookupError::Invalid | LookupError::Unresolvable => None,
        }
    }
}

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
