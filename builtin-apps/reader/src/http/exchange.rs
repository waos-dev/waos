//! One request and its answer over `wasdk::http`, WASI 0.3's HTTP client, which WaOS sends with
//! the system's check of the server's certificate and through the system's proxy, following no
//! redirect: a `GET` of an address with headers, answered with the status, the headers and the
//! body, read no further than a limit, all by a deadline.
//!
//! The invariant a later editor will break: the deadline ends the wait by dropping the exchange,
//! which cancels the request and its body in WaOS; nothing here blocks the program.

use std::future::Future;
use std::pin::pin;
use std::time::Instant;

use futures::future::{Either, select};
use url::Url;
use wasdk::http::client;
use wasdk::http::types::{ErrorCode, Fields, Request, RequestOptions, Response, Scheme};
use wasdk::{StreamReader, StreamResult, monotonic_clock, wit_future};

use super::HttpError;

/// How long a connection may take, and how long the server may leave between two parts of its
/// answer, in nanoseconds.
const CONNECT_WITHIN: u64 = 15_000_000_000;
const ANSWER_WITHIN: u64 = 20_000_000_000;

/// The most of a body read at once, in bytes.
const READ_AT_ONCE: usize = 64 * 1024;

/// A server's answer to one request.
pub(super) struct Reply {
    pub(super) status: u16,
    /// The headers, their names in lower case.
    pub(super) headers: Vec<(String, String)>,
    /// The body; empty for an answer that is not a success.
    pub(super) body: Vec<u8>,
}

impl Reply {
    pub(super) fn header(&self, name: &str) -> Option<&str> {
        let found = self.headers.iter().find(|(header, _)| header == name);
        found.map(|(_, value)| value.as_str())
    }
}

/// Asks for `address` with `headers`, and answers what the server said, its body read where it
/// succeeded, no further than `limit` bytes, all by `deadline`.
pub(super) async fn get(
    address: &Url,
    headers: &[(&str, &str)],
    limit: usize,
    deadline: Instant,
) -> Result<Reply, HttpError> {
    let request = request_for(address, headers)?;
    by_deadline(exchange(request, limit), deadline).await
}

/// Sends `request`, and answers the reply, its body read where it succeeded, no further than
/// `limit` bytes.
async fn exchange(request: Request, limit: usize) -> Result<Reply, HttpError> {
    let response = client::send(request)
        .await
        .map_err(|error| failure(&error))?;
    let status = response.get_status_code();
    let headers = headers_of(&response);
    let body = if (200..=299).contains(&status) {
        body_of(response, limit).await?
    } else {
        Vec::new()
    };
    Ok(Reply {
        status,
        headers,
        body,
    })
}

/// What `work` answers, or `TimedOut` once `deadline` passed, `work` dropped then.
async fn by_deadline<T>(
    work: impl Future<Output = Result<T, HttpError>>,
    deadline: Instant,
) -> Result<T, HttpError> {
    let left = deadline.saturating_duration_since(Instant::now());
    let left = u64::try_from(left.as_nanos()).unwrap_or(u64::MAX);
    match select(pin!(work), pin!(monotonic_clock::wait_for(left))).await {
        Either::Left((done, _)) => done,
        Either::Right(_) => Err(HttpError::TimedOut),
    }
}

fn request_for(address: &Url, headers: &[(&str, &str)]) -> Result<Request, HttpError> {
    let fields = Fields::new();
    for (name, value) in headers {
        fields
            .append(name, value.as_bytes())
            .map_err(|_| HttpError::BadAddress)?;
    }
    let scheme = match address.scheme() {
        "https" => Scheme::Https,
        "http" => Scheme::Http,
        _ => return Err(HttpError::BadAddress),
    };
    let authority = address.host_str().map(|host| match address.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    });
    let path = match address.query() {
        Some(query) => format!("{}?{query}", address.path()),
        None => address.path().to_owned(),
    };
    let (_no_trailers, trailers) = wit_future::new(|| Ok(None));
    let (request, _sent) = Request::new(fields, None, trailers, Some(patient_options()));
    let set = request.set_scheme(Some(&scheme)).is_ok()
        && request.set_authority(authority.as_deref()).is_ok()
        && request.set_path_with_query(Some(&path)).is_ok();
    if set && authority.is_some() {
        Ok(request)
    } else {
        Err(HttpError::BadAddress)
    }
}

/// The timeouts WaOS keeps for the request, so that a server that says nothing ends it.
fn patient_options() -> RequestOptions {
    let options = RequestOptions::new();
    // A host that keeps no such timeout refuses to set it, and the deadline still ends the wait.
    let _ = options.set_connect_timeout(Some(CONNECT_WITHIN));
    let _ = options.set_first_byte_timeout(Some(ANSWER_WITHIN));
    let _ = options.set_between_bytes_timeout(Some(ANSWER_WITHIN));
    options
}

fn headers_of(response: &Response) -> Vec<(String, String)> {
    let named = |(name, value): (String, Vec<u8>)| {
        let value = String::from_utf8_lossy(&value).trim().to_owned();
        (name.to_ascii_lowercase(), value)
    };
    response
        .get_headers()
        .copy_all()
        .into_iter()
        .map(named)
        .collect()
}

/// The whole of `response`'s body; an error where it is longer than `limit` bytes, or where the
/// server's answer broke off.
async fn body_of(response: Response, limit: usize) -> Result<Vec<u8>, HttpError> {
    let (_read, read_result) = wit_future::new(|| Ok(()));
    let (contents, trailers) = Response::consume_body(response, read_result);
    let bytes = read_to_end(contents, limit).await?;
    trailers.await.map_err(|error| failure(&error))?;
    Ok(bytes)
}

/// What `contents` holds until its writer is gone; an error where it is longer than `limit`
/// bytes.
async fn read_to_end(mut contents: StreamReader<u8>, limit: usize) -> Result<Vec<u8>, HttpError> {
    let mut bytes = Vec::new();
    loop {
        bytes.reserve(READ_AT_ONCE);
        let (result, read) = contents.read(bytes).await;
        bytes = read;
        if bytes.len() > limit {
            return Err(HttpError::TooLarge);
        }
        if !matches!(result, StreamResult::Complete(_)) {
            return Ok(bytes);
        }
    }
}

/// Why WASI's client did not answer, as the app tells its user.
fn failure(error: &ErrorCode) -> HttpError {
    match error {
        ErrorCode::HttpRequestDenied => HttpError::NotAllowed,
        ErrorCode::DnsTimeout
        | ErrorCode::DnsError(_)
        | ErrorCode::DestinationNotFound
        | ErrorCode::DestinationUnavailable
        | ErrorCode::DestinationIpUnroutable
        | ErrorCode::ConnectionRefused
        | ErrorCode::ConnectionTerminated
        | ErrorCode::ConnectionLimitReached => HttpError::Unreachable,
        ErrorCode::ConnectionTimeout
        | ErrorCode::ConnectionReadTimeout
        | ErrorCode::ConnectionWriteTimeout
        | ErrorCode::HttpResponseTimeout => HttpError::TimedOut,
        ErrorCode::TlsProtocolError
        | ErrorCode::TlsCertificateError
        | ErrorCode::TlsAlertReceived(_) => HttpError::NotSecure,
        other => unnamed(&format!("{other:?}")),
    }
}

/// A failure WaOS gives no name the app could tell its user: `said` goes to the log, where whoever
/// runs the program reads it.
fn unnamed(said: &str) -> HttpError {
    eprintln!("a request failed: {said}");
    HttpError::Failed
}
