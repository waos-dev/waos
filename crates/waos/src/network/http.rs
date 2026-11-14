//! A program's HTTP requests, `wasi:http/client@0.3.0`'s, through `wasmtime-wasi-http`'s hook
//! (decision 13 of the permissions proposal): each checked by its URL's host as a lookup and a
//! connection are checked, then sent with reqwest, which goes through the system's proxy and
//! checks the server's certificate as the system does, and follows no redirect, so a redirect's
//! target is a request of the program's own, checked the same way. An engine set up with
//! [`HttpProxy::None`], as the tests' are, sends each request straight to its host, whatever
//! proxy the machine has set.
//!
//! A host that is an address is judged by the address rules before the request is sent. A host
//! that is a name the domain rule refuses is refused then too. With no proxy in effect, any other
//! name is resolved by reqwest through the program's own checked lookup ([`resolver`]), which
//! answers only the addresses the program may reach, so the connection goes to exactly those,
//! resolved once: a name whose answer changes between a check and a connection cannot lead one to
//! this computer or the local network without the local-network switch. With a proxy in effect,
//! WaOS looks the host up itself first and picks the way by where it lands ([`route`], decision
//! 14): this computer and the local network directly, at the addresses that lookup answered, the
//! internet through the proxy, which looks the name up again.
//!
//! The invariant: a request reaches reqwest only after its host passed the program's network,
//! read as the grants are when it is sent, and reqwest connects only to addresses the same
//! network answered, or to the proxy for a host it answered only internet addresses for.

mod bodies;
mod proxies;
mod resolver;
mod route;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use http::Uri;
use http_body_util::BodyExt;
use wasmtime_wasi_http::{Error, RequestOptions, WasiBody, WasiHttpHooks};

use super::{Lookup, LookupError, ProgramNetwork};
use bodies::{BetweenBytes, SyncBody};
use proxies::Proxies;
use resolver::ProgramResolver;
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
/// the proxies; a client straight to each host and one through the proxies, both resolving
/// through the program's resolver.
#[derive(Clone)]
struct Sender {
    network: ProgramNetwork,
    proxies: Arc<Proxies>,
    direct: reqwest::Client,
    proxied: reqwest::Client,
    resolver: Arc<ProgramResolver>,
}

/// What a request sends back: the response, and what reports an error of the request's own after
/// the response came, which reqwest reports through the response.
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
    /// resolver, no redirect followed.
    fn sender(&mut self) -> Result<Sender, Error> {
        if let Some(sender) = &self.sender {
            return Ok(sender.clone());
        }
        let resolver = Arc::new(ProgramResolver::new(self.network.clone()));
        let proxies = self.proxy.proxies();
        let sender = Sender {
            network: self.network.clone(),
            direct: client(reqwest::Client::builder().no_proxy(), &resolver)?,
            proxied: client(
                proxies.sending_through(reqwest::Client::builder()),
                &resolver,
            )?,
            proxies: Arc::new(proxies),
            resolver,
        };
        self.sender = Some(sender.clone());
        Ok(sender)
    }
}

/// `builder`'s client, resolving through `resolver` and following no redirect.
fn client(
    builder: reqwest::ClientBuilder,
    resolver: &Arc<ProgramResolver>,
) -> Result<reqwest::Client, Error> {
    builder
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(Arc::clone(resolver))
        .build()
        .map_err(|error| Error::InternalError(Some(error.to_string())))
}

impl Sender {
    /// Sends `request` the way its route says, and answers the response once its head came.
    async fn execute(&self, request: http::Request<WasiBody>) -> Result<reqwest::Response, Error> {
        let route = route(&self.network, &self.proxies, request.uri())
            .await
            .map_err(refusal)?;
        let request = request.map(|body| reqwest::Body::wrap(SyncBody::new(body)));
        let request =
            reqwest::Request::try_from(request).map_err(|_| Error::HttpRequestUriInvalid)?;
        let sent = match route {
            Route::Direct { answered } => {
                let _pinned = answered.map(|answered| self.resolver.pin(answered));
                self.direct.execute(request).await
            }
            Route::Proxied { proxy } => {
                self.resolver.note_proxy(&proxy);
                self.proxied.execute(request).await
            }
        };
        sent.map_err(|error| error_of(&error))
    }
}

impl WasiHttpHooks for ProgramHttp {
    /// reqwest writes the `host` header from the URL.
    fn set_host_header(&mut self) -> bool {
        false
    }

    fn send_request(
        &mut self,
        request: http::Request<WasiBody>,
        options: Option<RequestOptions>,
        _response_errors: Box<dyn Future<Output = Result<(), Error>> + Send>,
    ) -> Box<dyn Future<Output = Result<Sent, Error>> + Send> {
        let checked = check_host(&self.network, request.uri());
        let sender = self.sender();
        Box::new(async move {
            checked?;
            send(sender?, request, options).await
        })
    }
}

/// Refuses `uri` before anything is sent unless its host may be reached: addresses known
/// without the system, such as an address, by the address rules, a name the system resolves
/// unless the domain rule and the local network both refuse it. The addresses the system
/// resolves a name to are judged when reqwest resolves it.
fn check_host(network: &ProgramNetwork, uri: &Uri) -> Result<(), Error> {
    let host = uri.host().ok_or(Error::HttpRequestUriInvalid)?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    match network.plan_lookup(host).map_err(refusal)? {
        Lookup::Known(addresses) if !addresses.iter().any(|known| network.may_reach(*known)) => {
            Err(Error::HttpRequestDenied)
        }
        Lookup::Known(_) | Lookup::Resolve { .. } => Ok(()),
    }
}

/// What a lookup's refusal is to a request.
fn refusal(error: LookupError) -> Error {
    match error {
        LookupError::Refused => Error::HttpRequestDenied,
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
) -> Result<Sent, Error> {
    let options = options.unwrap_or_default();
    let until_head = options.connect_timeout.unwrap_or(DEFAULT_TIMEOUT)
        + options.first_byte_timeout.unwrap_or(DEFAULT_TIMEOUT);
    let between_bytes = options.between_bytes_timeout.unwrap_or(DEFAULT_TIMEOUT);
    let response = tokio::time::timeout(until_head, sender.execute(request))
        .await
        .map_err(|_| Error::ConnectionTimeout)??;
    let response = http::Response::from(response)
        .map(|body| BetweenBytes::new(body, between_bytes).boxed_unsync());
    Ok((response, Box::new(async { Ok(()) })))
}

/// WASI's error for what reqwest reports, by the first of its causes that tells: the program's
/// resolver refusing the host, the certificate, the TLS exchange, a refused or timed-out
/// connection.
fn error_of(error: &reqwest::Error) -> Error {
    if error.is_timeout() {
        return Error::ConnectionTimeout;
    }
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = cause {
        if let Some(told) = told_by(current) {
            return told;
        }
        cause = next_cause(current);
    }
    if error.is_connect() {
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
