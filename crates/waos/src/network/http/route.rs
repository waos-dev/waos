//! Which way a program's request goes. With no proxy in effect for it, as the proxy settings and
//! their exceptions say, it goes straight to its host, which its connection looks up through the
//! program's checked lookup (`connector`). With one, WaOS looks the host up itself first, through
//! the program's own lookup (the domain rule, the address record, the local-network switch), and
//! picks the way by where the host lands: an address on this computer or the local network is
//! connected to directly, at the addresses that lookup answered, under the local-network switch; an
//! internet address goes through the proxy, under the network switch and its domain rule. A name
//! let through only for its local addresses that has none is refused, as it is without a proxy; a
//! host that is an address goes directly when it is local. A name answering both kinds goes
//! directly to its local addresses alone when the local-network switch lets the program reach them,
//! so a connection lands where it was checked, never on an internet address the proxy was meant to
//! carry; without the switch its local addresses are not answered, and its internet ones go through
//! the proxy.
//!
//! The blind spot that remains: the proxy looks an internet name up again, so a DNS answering it
//! differently the second time, with an address on this computer or the local network, leads the
//! proxy there. macOS's own check of the local network has the same one through a proxy.
//!
//! The invariant: a request a proxy is in effect for reaches the proxy only after the program's
//! lookup answered addresses it may reach, every one of them on the internet.

use std::net::IpAddr;

use http::Uri;
use hyper_util::client::proxy::matcher::Intercept;
use wasmtime_wasi::runtime::spawn_blocking;

use super::proxies::Proxies;
use crate::access::Cause;
use crate::network::addresses::is_local;
use crate::network::{LookupError, ProgramNetwork};

/// The way a request goes.
#[derive(Debug)]
pub(super) enum Route {
    /// Straight to the host. Where a proxy was in effect, at the addresses the program's lookup
    /// answered for it (`answered`); otherwise wherever the program's lookup answers as its
    /// connection is made, as without a proxy.
    Direct { answered: Option<Answered> },
    /// Through `proxy`, which looks the host up again.
    Proxied { proxy: Intercept },
}

/// The addresses the program's lookup answered for a host, as the request names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Answered {
    pub(super) host: String,
    pub(super) addresses: Vec<IpAddr>,
}

/// The way a request for `uri` goes, through the proxies in `proxies`; refused as the program's
/// lookup refuses its host when a proxy is in effect for it. The lookup runs off the calling
/// thread. `uri`'s host is one `check_host` let through.
pub(super) async fn route(
    network: &ProgramNetwork,
    proxies: &Proxies,
    uri: &Uri,
) -> Result<Route, LookupError> {
    let Some(proxy) = proxies.intercept(uri) else {
        return Ok(Route::Direct { answered: None });
    };
    let host = uri.host().ok_or(LookupError::Invalid)?;
    let looked_up = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let network = network.clone();
    let reachable =
        spawn_blocking(move || network.reachable(&looked_up, Cause::HttpRequest)).await?;
    let local: Vec<IpAddr> = reachable
        .into_iter()
        .filter(|address| is_local(*address))
        .collect();
    if local.is_empty() {
        return Ok(Route::Proxied { proxy });
    }
    Ok(Route::Direct {
        answered: Some(Answered {
            host: host.to_owned(),
            addresses: local,
        }),
    })
}
