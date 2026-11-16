//! A program's HTTP requests, `wasi:http/client@0.3.0`'s, through `wasmtime-wasi-http`'s hook
//! (decision 13 of the permissions proposal): each checked by its URL's host as a lookup and a
//! connection are checked, then sent with hyper's client, over connections of the program's own
//! (`sockets`), so their bytes are counted as its sockets' are ([`connector`]), through the
//! system's proxy, in TLS checked as the system checks a server's certificate, and following no
//! redirect, so a redirect's target is a request of the program's own, checked the same way. An
//! engine set up with [`HttpProxy::None`], as the tests' are, sends each request straight to its
//! host, whatever proxy the machine has set.
//!
//! A host that is an address is judged by the address rules before the request is sent. A host
//! that is a name the domain rule refuses is refused then too. With no proxy in effect, any other
//! name is resolved as its connection is made, through the program's own checked lookup, which
//! answers only the addresses the program may reach, so the connection goes to exactly those,
//! resolved once: a name whose answer changes between a check and a connection cannot lead one to
//! this computer or the local network without the local-network switch. With a proxy in effect,
//! WaOS looks the host up itself first and picks the way by where it lands ([`route`], decision
//! 14): this computer and the local network directly, at the addresses that lookup answered
//! ([`pins`]), the internet through the proxy, which looks the name up again.
//!
//! Each request is decided and told through the program's access by its host's check
//! ([`check_host`]), the lookups it causes told apart as its own, and held from then until its
//! response's body ends or is let go, or the request fails.
//!
//! The invariant: a request is sent only after its host passed the program's network, read as
//! the grants are when it is sent, and its connection goes only to addresses the same network
//! answered, or to the proxy for a host it answered only internet addresses for.

mod bodies;
mod connector;
mod pins;
mod proxies;
mod route;
mod wire;

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use http::header::PROXY_AUTHORIZATION;
use http::{Method, Uri};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper_util::client::legacy::Client;
use hyper_util::client::proxy::matcher::Intercept;
use hyper_util::rt::{TokioExecutor, TokioTimer};
use wasmtime_wasi_http::{Error, RequestOptions, WasiBody, WasiHttpHooks};

use super::{Lookup, LookupError, ProgramNetwork};
use crate::access::{ActiveUse, ActiveUseKind, Asked, Happened, Refusal};
use bodies::BetweenBytes;
use connector::{Connector, tls_config};
use pins::Pins;
use proxies::Proxies;
use route::{Route, route};

/// How long a request waits where the program set no time, as `wasmtime-wasi-http` waits.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// Which proxy programs' requests go through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpProxy {
    /// The system's proxy settings, with the environment's over them (`proxies`): how an app
    /// sends.
    System,
    /// None: each request goes straight to its host, whatever proxy the machine has set, so a
    /// test's requests reach what it serves on this computer, and only that.
    None,
    /// Every request through a proxy of `scheme` a test serves on this computer at `port`,
    /// standing in for the system's, whatever proxy the machine has set.
    #[cfg(test)]
    Test { scheme: &'static str, port: u16 },
}

impl HttpProxy {
    /// The proxies a request may go through, read now.
    fn proxies(self) -> Proxies {
        match self {
            HttpProxy::System => Proxies::system(),
            HttpProxy::None => Proxies::none(),
            #[cfg(test)]
            HttpProxy::Test { scheme, port } => Proxies::listed(proxies::ProxySettings::through(
                &format!("{scheme}://127.0.0.1:{port}"),
            )),
        }
    }
}

/// A program's HTTP: its network, which each request's host is checked by, the proxy requests go
/// through, and what sends what passes, made at the first request.
pub(crate) struct ProgramHttp {
    network: ProgramNetwork,
    proxy: HttpProxy,
    sender: Option<Sender>,
}

/// What sends a program's requests: its network, which picks each request's way past a proxy;
/// the proxies; the hosts pinned to what the network answered for them; a client straight to
/// each host and one through the proxies.
#[derive(Clone)]
struct Sender {
    network: ProgramNetwork,
    proxies: Arc<Proxies>,
    pins: Arc<Pins>,
    direct: Client<Connector, WasiBody>,
    proxied: Client<Connector, WasiBody>,
}

/// What a request sends back: the response, and what reports an error of the request's own after
/// the response came, which hyper reports through the response.
type Sent = (
    http::Response<WasiBody>,
    Box<dyn Future<Output = Result<(), Error>> + Send>,
);

impl ProgramHttp {
    pub(crate) fn new(network: ProgramNetwork, proxy: HttpProxy) -> ProgramHttp {
        ProgramHttp {
            network,
            proxy,
            sender: None,
        }
    }

    /// What requests are sent with: the proxies, the system's certificate check, the program's
    /// network, no redirect followed.
    fn sender(&mut self) -> Result<Sender, Error> {
        if let Some(sender) = &self.sender {
            return Ok(sender.clone());
        }
        let tls = tls_config().map_err(|error| Error::InternalError(Some(error.to_string())))?;
        let proxies = Arc::new(self.proxy.proxies());
        let pins = Arc::new(Pins::default());
        let sender = Sender {
            network: self.network.clone(),
            direct: client(Connector::direct(
                self.network.clone(),
                Arc::clone(&pins),
                Arc::clone(&tls),
            )),
            proxied: client(Connector::proxied(
                self.network.clone(),
                Arc::clone(&proxies),
                tls,
            )),
            proxies,
            pins,
        };
        self.sender = Some(sender.clone());
        Ok(sender)
    }
}

/// A client over `connector`, keeping each connection a while for the next request to its host,
/// as hyper's pool does.
fn client(connector: Connector) -> Client<Connector, WasiBody> {
    Client::builder(TokioExecutor::new())
        .timer(TokioTimer::new())
        .pool_timer(TokioTimer::new())
        .build(connector)
}

impl Sender {
    /// Sends `request` the way its route says, and answers the response once its head came.
    async fn execute(
        &self,
        mut request: http::Request<WasiBody>,
    ) -> Result<http::Response<Incoming>, Error> {
        if !is_sendable(request.uri()) {
            return Err(Error::HttpRequestUriInvalid);
        }
        let route = route(&self.network, &self.proxies, request.uri())
            .await
            .map_err(refusal)?;
        let sent = match route {
            Route::Direct { answered } => {
                let _pinned = answered.map(|answered| self.pins.pin(answered));
                self.direct.request(request).await
            }
            Route::Proxied { proxy } => {
                authorize_to(&proxy, &mut request);
                self.proxied.request(request).await
            }
        };
        sent.map_err(|error| error_of(&error))
    }
}

/// Whether `uri` names what hyper sends: an HTTP or HTTPS URL with a host.
fn is_sendable(uri: &Uri) -> bool {
    matches!(uri.scheme_str(), Some("http" | "https")) && uri.host().is_some()
}

/// Gives `request` the credentials of `proxy` where an HTTP proxy is sent it whole, a plain HTTP
/// request, and the program set none of its own.
fn authorize_to(proxy: &Intercept, request: &mut http::Request<WasiBody>) {
    let to_http_proxy = matches!(proxy.uri().scheme_str(), Some("http" | "https"));
    if request.uri().scheme_str() != Some("http") || !to_http_proxy {
        return;
    }
    if let Some(authorization) = proxy.basic_auth() {
        request
            .headers_mut()
            .entry(PROXY_AUTHORIZATION)
            .or_insert_with(|| authorization.clone());
    }
}

impl WasiHttpHooks for ProgramHttp {
    /// hyper's client writes the `host` header from the URL.
    fn set_host_header(&mut self) -> bool {
        false
    }

    fn send_request(
        &mut self,
        request: http::Request<WasiBody>,
        options: Option<RequestOptions>,
        _response_errors: Box<dyn Future<Output = Result<(), Error>> + Send>,
    ) -> Box<dyn Future<Output = Result<Sent, Error>> + Send> {
        let checked = check_host(&self.network, request.method(), request.uri());
        let sender = self.sender();
        Box::new(async move {
            let in_flight = checked?;
            send(sender?, request, options, in_flight).await
        })
    }
}

/// A request allowed: held in flight from now on, and what its response is told by.
struct InFlight {
    active_use: ActiveUse,
    id: u64,
    sent: Instant,
}

/// Refuses the request `method` sends to `uri` before anything is sent unless its host may be
/// reached, decided and told as the request; and the request in flight from then on otherwise.
fn check_host(network: &ProgramNetwork, method: &Method, uri: &Uri) -> Result<InFlight, Error> {
    let id = network.access.next_http_request_id();
    let asked = Asked::SendHttpRequest { id, method, uri };
    network
        .access
        .decide(asked, |_| host_reachable(network, uri))
        .map_err(refusal)?;
    let request = ActiveUseKind::HttpRequest {
        method: method.clone(),
        uri: uri.clone(),
    };
    Ok(InFlight {
        active_use: network.access.hold(request),
        id,
        sent: Instant::now(),
    })
}

/// Whether `uri`'s host may be reached: addresses known without the system, such as an address,
/// by the address rules, a name the system resolves unless the domain rule and the local network
/// both refuse it. The addresses the system resolves a name to are judged as the request's
/// connection is made.
fn host_reachable(network: &ProgramNetwork, uri: &Uri) -> Result<(), LookupError> {
    let host = uri.host().ok_or(LookupError::Invalid)?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match network.plan_lookup(host)? {
        Lookup::Known(addresses) => {
            let mut refused = Refusal::by("network");
            for address in addresses {
                match network.reach(address) {
                    Ok(()) => return Ok(()),
                    Err(refusal) => refused = refusal,
                }
            }
            Err(LookupError::Refused(refused))
        }
        Lookup::Resolve { .. } => Ok(()),
    }
}

/// What a lookup's refusal is to a request.
fn refusal(error: LookupError) -> Error {
    match error {
        LookupError::Refused(_) => Error::HttpRequestDenied,
        LookupError::Invalid => Error::HttpRequestUriInvalid,
        LookupError::Unresolvable => Error::DnsError {
            rcode: None,
            info_code: None,
        },
    }
}

/// Sends `request` with `sender`, within the times `options` sets: its lookup, the connection
/// and the response's head within the first two together, each part of its body within the
/// third.
async fn send(
    sender: Sender,
    request: http::Request<WasiBody>,
    options: Option<RequestOptions>,
    in_flight: InFlight,
) -> Result<Sent, Error> {
    let options = options.unwrap_or_default();
    let until_head = options.connect_timeout.unwrap_or(DEFAULT_TIMEOUT)
        + options.first_byte_timeout.unwrap_or(DEFAULT_TIMEOUT);
    let between_bytes = options.between_bytes_timeout.unwrap_or(DEFAULT_TIMEOUT);
    let response = tokio::time::timeout(until_head, sender.execute(request))
        .await
        .map_err(|_| Error::ConnectionTimeout)??;
    sender.network.access.tell(Happened::HttpResponded {
        request_id: in_flight.id,
        status: response.status().as_u16(),
        time_to_first_byte: in_flight.sent.elapsed(),
    });
    let active_use = in_flight.active_use;
    let response =
        response.map(|body| BetweenBytes::new(body, between_bytes, active_use).boxed_unsync());
    Ok((response, Box::new(async { Ok(()) })))
}

/// WASI's error for what hyper's client reports, by the first of its causes that tells: the
/// program's lookup refusing the host, the certificate, the TLS exchange, a refused or timed-out
/// connection; any other failure to connect as an unavailable destination.
fn error_of(error: &(dyn std::error::Error + 'static)) -> Error {
    let mut cause = Some(error);
    while let Some(current) = cause {
        if let Some(told) = told_by(current) {
            return told;
        }
        cause = next_cause(current);
    }
    let connecting = error
        .downcast_ref::<hyper_util::client::legacy::Error>()
        .is_some_and(hyper_util::client::legacy::Error::is_connect);
    if connecting {
        Error::DestinationUnavailable
    } else {
        Error::InternalError(Some(error.to_string()))
    }
}

/// WASI's error for `cause`, where it is one that tells.
fn told_by(cause: &(dyn std::error::Error + 'static)) -> Option<Error> {
    if let Some(lookup) = cause.downcast_ref::<LookupError>() {
        return Some(refusal(*lookup));
    }
    if let Some(tls) = cause.downcast_ref::<rustls::Error>() {
        return Some(match tls {
            rustls::Error::InvalidCertificate(_) => Error::TlsCertificateError,
            _ => Error::TlsProtocolError,
        });
    }
    match cause.downcast_ref::<std::io::Error>()?.kind() {
        std::io::ErrorKind::ConnectionRefused => Some(Error::ConnectionRefused),
        std::io::ErrorKind::TimedOut => Some(Error::ConnectionTimeout),
        _ => None,
    }
}

/// The cause beneath `cause`: for an I/O error, the error it carries, which its own `source`
/// passes over.
fn next_cause<'a>(
    cause: &'a (dyn std::error::Error + 'static),
) -> Option<&'a (dyn std::error::Error + 'static)> {
    let carried = cause
        .downcast_ref::<std::io::Error>()
        .and_then(std::io::Error::get_ref);
    match carried {
        Some(carried) => Some(carried),
        None => cause.source(),
    }
}

#[cfg(test)]
mod tests;
