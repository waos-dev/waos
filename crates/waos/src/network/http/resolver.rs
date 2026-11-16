//! The names reqwest resolves for a program's requests: each through the program's own checked
//! lookup, so a connection goes to the addresses that passed the rules, resolved once and
//! recorded as `ip-name-lookup` records them, and never to an answer the name gave afterwards. A
//! host the program's lookup answered already, to pick a request's way past a proxy (`route`), is
//! pinned to that answer while the request is sent, so it is not looked up a second time. The one
//! other name reqwest resolves is the proxy a request is sent through, WaOS's way out rather than
//! a host the program named, which is resolved unchecked: a localhost name as this computer, as
//! the program's lookup answers it, any other as the system resolves it.
//!
//! The invariant: a name is resolved unchecked only when a request was routed through a proxy of
//! that name, and answered without a lookup only while a request holds the answer the program's
//! lookup gave for it, so a name nothing named is checked, never let through.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use http::Uri;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use wasmtime_wasi::runtime::spawn_blocking;

use super::route::Answered;
use crate::access::Cause;
use crate::network::{ProgramNetwork, resolve_unchecked};
use crate::sync::locked;

/// A program's resolver for reqwest: its network, the proxies its requests went through, and the
/// hosts pinned to what its lookup answered.
pub(super) struct ProgramResolver {
    network: ProgramNetwork,
    /// The host of each proxy a request was sent through.
    proxy_hosts: Mutex<HashSet<String>>,
    /// The addresses each host is answered with while requests routed straight to it are sent.
    pinned: Mutex<HashMap<String, Pin>>,
}

/// What a host is pinned to: the addresses the program's lookup answered last, and how many
/// requests being sent hold the pin.
struct Pin {
    addresses: Vec<IpAddr>,
    holders: usize,
}

/// A request's hold on a host's pin, until this is dropped: the last hold dropped unpins the
/// host.
pub(super) struct Pinned {
    resolver: Arc<ProgramResolver>,
    host: String,
}

impl ProgramResolver {
    pub(super) fn new(network: ProgramNetwork) -> ProgramResolver {
        ProgramResolver {
            network,
            proxy_hosts: Mutex::default(),
            pinned: Mutex::default(),
        }
    }

    /// Notes that a request goes through `proxy`, before reqwest resolves the proxy's host.
    pub(super) fn note_proxy(&self, proxy: &Uri) {
        if let Some(host) = proxy.host() {
            locked(&self.proxy_hosts).insert(host.to_owned());
        }
    }

    /// Answers `answered`'s host with its addresses until every pin of it held is dropped: a
    /// newer answer replaces the addresses of a host pinned already.
    pub(super) fn pin(self: &Arc<Self>, answered: Answered) -> Pinned {
        let Answered { host, addresses } = answered;
        let mut pinned = locked(&self.pinned);
        let pin = pinned.entry(host.clone()).or_insert(Pin {
            addresses: Vec::new(),
            holders: 0,
        });
        pin.addresses = addresses;
        pin.holders += 1;
        Pinned {
            resolver: Arc::clone(self),
            host,
        }
    }

    fn is_proxy(&self, name: &str) -> bool {
        locked(&self.proxy_hosts).contains(name)
    }

    fn pinned_addresses(&self, name: &str) -> Option<Vec<IpAddr>> {
        locked(&self.pinned)
            .get(name)
            .map(|pin| pin.addresses.clone())
    }
}

impl Drop for Pinned {
    fn drop(&mut self) {
        let mut pinned = locked(&self.resolver.pinned);
        let Some(pin) = pinned.get_mut(&self.host) else {
            return;
        };
        pin.holders -= 1;
        if pin.holders == 0 {
            pinned.remove(&self.host);
        }
    }
}

impl Resolve for ProgramResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let name = name.as_str().to_owned();
        if let Some(addresses) = self.pinned_addresses(&name) {
            return Box::pin(async move { Ok(socket_addresses(addresses)) });
        }
        let resolving = if self.is_proxy(&name) {
            spawn_blocking(move || resolve_unchecked(&name))
        } else {
            let network = self.network.clone();
            spawn_blocking(move || network.reachable(&name, Cause::HttpRequest))
        };
        Box::pin(async move {
            let addresses = resolving.await?;
            Ok(socket_addresses(addresses))
        })
    }
}

/// `addresses`, as reqwest takes them: the port is the request's.
fn socket_addresses(addresses: Vec<IpAddr>) -> Addrs {
    Box::new(
        addresses
            .into_iter()
            .map(|address| SocketAddr::new(address, 0)),
    )
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::access::ProgramAccess;
    use crate::network::test_grants::{TestGrants, any};

    const HOST: &str = "pinned.test";

    /// A resolver whose lookups of any name answer `192.0.2.1` and are counted.
    fn resolver(lookups: &Arc<AtomicUsize>) -> Arc<ProgramResolver> {
        let counted = Arc::clone(lookups);
        let network = ProgramNetwork::resolving_with(
            ProgramAccess::new(TestGrants::new(any(), true)),
            Arc::new(move |_: &str| {
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(vec![address("192.0.2.1")])
            }),
        );
        Arc::new(ProgramResolver::new(network))
    }

    fn address(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn pinned(resolver: &Arc<ProgramResolver>, to: &str) -> Pinned {
        resolver.pin(Answered {
            host: HOST.to_owned(),
            addresses: vec![address(to)],
        })
    }

    /// What `resolver` answers for the host, on a runtime as a program's thread has.
    fn resolved(resolver: &Arc<ProgramResolver>) -> Vec<IpAddr> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let resolving = resolver.resolve(Name::from_str(HOST).unwrap());
        let addresses = runtime.block_on(resolving).unwrap();
        addresses.map(|address| address.ip()).collect()
    }

    #[test]
    fn a_host_stays_pinned_until_the_last_request_holding_it_ends() {
        let lookups = Arc::new(AtomicUsize::new(0));
        let resolver = resolver(&lookups);
        let first = pinned(&resolver, "127.0.0.1");
        let second = pinned(&resolver, "127.0.0.2");
        assert_eq!(
            resolved(&resolver),
            [address("127.0.0.2")],
            "the newer answer"
        );
        drop(first);
        assert_eq!(resolved(&resolver), [address("127.0.0.2")], "still held");
        assert_eq!(lookups.load(Ordering::SeqCst), 0);
        drop(second);
        assert_eq!(
            resolved(&resolver),
            [address("192.0.2.1")],
            "looked up again"
        );
        assert_eq!(lookups.load(Ordering::SeqCst), 1);
    }
}
