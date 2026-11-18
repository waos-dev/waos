//! Waft's HTTP client over `wasdk::http`, WASI 0.3's `wasi:http/client`: a request's method, URI
//! and repeated headers, a body streamed both ways, the three timeouts, the response's status,
//! headers, body and trailers, and WASI's error codes passed through, with `TooManyRedirects` the
//! one case this client adds (the design's decisions 9 to 11).
//!
//! WASI follows no redirect, so one is followed here as `SendOptions::redirects` asks, by fetch's
//! rules, which a browser's client and so Waft's on the web follow: a redirect with a location is
//! sent again at that URI, resolved against the request's, up to the limit. A 303 of anything but
//! a `GET` or a `HEAD`, and a 301 or a 302 of a `POST`, become a `GET` without the body and its
//! headers; every other redirect keeps the method and the body, which is sent again only when
//! [`Body::try_clone`] answered `Some` before the first hop, since a stream already read cannot be
//! read again. A redirect whose body cannot be sent again is answered to the app, as Waft's
//! `RedirectPolicy::Follow` says. A redirect to another origin drops the credentials the request
//! carried.
//!
//! A request body's trailers go to WASI's outgoing trailers future once its bytes have ended, read
//! through [`Body::trailers`]; a response's trailers are read from WASI's future once its body has
//! ended, and asking again answers the same fields. HTTP/1.1 sends a chunked request's trailers
//! only when its header names them in `Trailer`, so a request whose body carries them sets that
//! header, as the test program does.
//!
//! The invariant a later editor will break: never await the transmission future `Request::new`
//! answers, nor write or await the trailers future, before `client::send`: both report on a send
//! that has not begun, and waiting for them waits forever. The body's task writes the trailers
//! once the body has ended, which the host reads only as it sends.

use std::fmt;
use std::time::Duration;

use http::header::{self, HeaderName, HeaderValue, LOCATION};
use http::{HeaderMap, Method, Request, Response, StatusCode, Uri};
use url::Url;
use waft_embedder::{
    Body, BodyReadFuture, BodyStream, BodyTrailersFuture, HttpClient, HttpError, HttpFuture,
    RedirectPolicy, SendOptions,
};
use wasdk::http::client;
use wasdk::http::types::{
    self, ErrorCode, FieldSizePayload, Fields, Method as WitMethod, Request as WitRequest,
    RequestOptions, Scheme, Trailers,
};
use wasdk::{
    FutureReader, FutureWriter, StreamReader, StreamResult, StreamWriter, wit_future, wit_stream,
};

/// Waft's HTTP client, as a value: every request is WASI's own, so the client keeps nothing.
pub(crate) struct WasdkHttp;

impl HttpClient for WasdkHttp {
    fn send(&self, request: Request<Body>, options: SendOptions) -> HttpFuture {
        Box::pin(send(request, options))
    }
}

/// The headers that describe a body, which a redirect that drops the body drops with it: fetch's
/// request-body headers and the length.
const BODY_HEADERS: [HeaderName; 5] = [
    header::CONTENT_ENCODING,
    header::CONTENT_LANGUAGE,
    header::CONTENT_LOCATION,
    header::CONTENT_LENGTH,
    header::CONTENT_TYPE,
];

/// The headers that carry credentials, which a redirect to another origin drops, as browsers and
/// reqwest do.
const CREDENTIAL_HEADERS: [HeaderName; 4] = [
    header::AUTHORIZATION,
    header::COOKIE,
    header::PROXY_AUTHORIZATION,
    header::WWW_AUTHENTICATE,
];

/// Sends `request`, following its redirects as `options` says.
async fn send(request: Request<Body>, options: SendOptions) -> Result<Response<Body>, HttpError> {
    let (parts, mut body) = request.into_parts();
    let (mut method, mut uri, mut headers) = (parts.method, parts.uri, parts.headers);
    let mut followed = 0;
    loop {
        // A redirect that keeps the method sends the body again, which only a body held in memory
        // can do: keep that copy before this hop streams the body away.
        let resend = match options.redirects {
            RedirectPolicy::Follow { .. } => body.try_clone(),
            RedirectPolicy::Never => None,
        };
        let response = send_once(&method, &uri, &headers, body, &options).await?;
        let Some(location) = redirect(&response) else {
            return Ok(response);
        };
        let max = match options.redirects {
            RedirectPolicy::Never => return Ok(response),
            RedirectPolicy::Follow { max } => max,
        };
        if followed >= max {
            return Err(HttpError::TooManyRedirects);
        }
        let Some(next) = next_hop(response.status(), &method, resend) else {
            return Ok(response);
        };
        let target = location_target(&uri, &location)?;
        drop(response);
        if !same_origin(&uri, &target) {
            drop_headers(&mut headers, &CREDENTIAL_HEADERS);
        }
        body = match next {
            Next::AsGet => {
                method = Method::GET;
                drop_headers(&mut headers, &BODY_HEADERS);
                Body::empty()
            }
            Next::Same(resend) => resend,
        };
        uri = target;
        followed += 1;
    }
}

/// What the next hop of a followed redirect sends.
#[derive(Debug)]
enum Next {
    /// A `GET` without the body.
    AsGet,
    /// The same method and this body.
    Same(Body),
}

/// What the hop after a redirect of `status` sends, from `resend`, the copy of the body kept to
/// send again: `None` where the body cannot be sent again, so the redirect is the app's to answer.
fn next_hop(status: StatusCode, method: &Method, resend: Option<Body>) -> Option<Next> {
    match redirected(status, method) {
        Redirected::AsGet => Some(Next::AsGet),
        Redirected::Same => resend.map(Next::Same),
    }
}

/// Sends one hop of a request, its body streamed, and answers the response with its body.
async fn send_once(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: Body,
    options: &SendOptions,
) -> Result<Response<Body>, HttpError> {
    let fields = fields_of(headers)?;
    // RFC 9110 §8.6: no `Content-Length: 0` on a request that carries no content.
    if !headers.contains_key(header::CONTENT_LENGTH)
        && let Some(length) = body.length()
        && !(length == 0 && method == Method::GET)
    {
        fields
            .append(
                header::CONTENT_LENGTH.as_str(),
                length.to_string().as_bytes(),
            )
            .map_err(header_error)?;
    }
    let (contents, trailers) = contents_of(body);
    let (request, _sent) = WitRequest::new(
        fields,
        Some(contents),
        trailers,
        Some(request_options(options)?),
    );
    set_method(&request, method)?;
    set_uri(&request, uri)?;
    let response = client::send(request).await.map_err(http_error)?;
    response_of(response)
}

/// What a redirect does to a request's method and body.
#[derive(Debug, PartialEq, Eq)]
enum Redirected {
    /// Sent again as a `GET`, without the body.
    AsGet,
    /// Sent again with the same method and body.
    Same,
}

/// What a redirect with `status` does to a request of `method`, by fetch's rules: 303 makes
/// anything but a `GET` or a `HEAD` a `GET`, 301 and 302 make a `POST` one, and every other
/// redirect keeps both.
fn redirected(status: StatusCode, method: &Method) -> Redirected {
    let get_or_head = method == Method::GET || method == Method::HEAD;
    match status.as_u16() {
        303 if !get_or_head => Redirected::AsGet,
        301 | 302 if method == Method::POST => Redirected::AsGet,
        _ => Redirected::Same,
    }
}

/// Takes `names` out of `headers`.
fn drop_headers(headers: &mut HeaderMap, names: &[HeaderName]) {
    for name in names {
        headers.remove(name);
    }
}

/// Whether `from` and `to` are of one origin: the same scheme, host and port.
fn same_origin(from: &Uri, to: &Uri) -> bool {
    let origin = |uri: &Uri| {
        (
            uri.scheme_str().map(str::to_ascii_lowercase),
            uri.host().map(str::to_ascii_lowercase),
            uri.port_u16(),
        )
    };
    origin(from) == origin(to)
}

/// The URI a response's `Location` names, resolved against the request's as a browser resolves a
/// link: absolute, scheme-relative, path-absolute, relative, or a query alone. The fragment, which
/// is never sent, is left off.
fn location_target(current: &Uri, location: &str) -> Result<Uri, HttpError> {
    let base = Url::parse(&current.to_string()).map_err(|_| HttpError::HttpRequestUriInvalid)?;
    let mut target = base
        .join(location.trim())
        .map_err(|_| HttpError::HttpRequestUriInvalid)?;
    target.set_fragment(None);
    target
        .as_str()
        .parse()
        .map_err(|_| HttpError::HttpRequestUriInvalid)
}

/// The location a response redirects to, where it redirects.
fn redirect(response: &Response<Body>) -> Option<String> {
    let is_redirect = matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308);
    if !is_redirect {
        return None;
    }
    response
        .headers()
        .get(LOCATION)
        .and_then(|location| location.to_str().ok())
        .map(str::to_owned)
}

/// `method` as WASI names it.
fn set_method(request: &WitRequest, method: &Method) -> Result<(), HttpError> {
    request
        .set_method(&wit_method(method))
        .map_err(|()| HttpError::HttpRequestMethodInvalid)
}

/// `method` as WASI's method variant.
fn wit_method(method: &Method) -> WitMethod {
    match method.as_str() {
        "GET" => WitMethod::Get,
        "HEAD" => WitMethod::Head,
        "POST" => WitMethod::Post,
        "PUT" => WitMethod::Put,
        "DELETE" => WitMethod::Delete,
        "CONNECT" => WitMethod::Connect,
        "OPTIONS" => WitMethod::Options,
        "TRACE" => WitMethod::Trace,
        "PATCH" => WitMethod::Patch,
        other => WitMethod::Other(other.to_owned()),
    }
}

/// Sets `uri`'s scheme, authority and path on `request`.
fn set_uri(request: &WitRequest, uri: &Uri) -> Result<(), HttpError> {
    let scheme = match uri.scheme_str() {
        Some("http") => Scheme::Http,
        Some("https") => Scheme::Https,
        Some(other) => Scheme::Other(other.to_owned()),
        None => return Err(HttpError::HttpRequestUriInvalid),
    };
    let authority = uri
        .authority()
        .map(|authority| authority.as_str().to_owned())
        .ok_or(HttpError::HttpRequestUriInvalid)?;
    let path = uri.path_and_query().map_or("/", |path| path.as_str());
    request
        .set_scheme(Some(&scheme))
        .and_then(|()| request.set_authority(Some(&authority)))
        .and_then(|()| request.set_path_with_query(Some(path)))
        .map_err(|()| HttpError::HttpRequestUriInvalid)
}

/// `headers` as WASI's fields, each value as it lies.
fn fields_of(headers: &HeaderMap) -> Result<Fields, HttpError> {
    let fields = Fields::new();
    for (name, value) in headers {
        fields
            .append(name.as_str(), value.as_bytes())
            .map_err(header_error)?;
    }
    Ok(fields)
}

/// `fields` as Waft's headers, each name and value as it came.
fn headers_of(fields: &Fields) -> Result<HeaderMap, HttpError> {
    let mut headers = HeaderMap::new();
    for (name, value) in fields.copy_all() {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| header_error(format!("a header name: {error}")))?;
        let value = HeaderValue::from_bytes(&value)
            .map_err(|error| header_error(format!("a header value: {error}")))?;
        headers.append(name, value);
    }
    Ok(headers)
}

/// The trailer fields a response's future answers, as Waft's headers.
fn trailers_of(trailers: Option<Trailers>) -> Result<Option<HeaderMap>, HttpError> {
    trailers.map(|trailers| headers_of(&trailers)).transpose()
}

/// Why WASI or the `http` crate refused a field, as the failure Waft has for one.
fn header_error(error: impl fmt::Display) -> HttpError {
    HttpError::Internal(Some(format!("a header: {error}")))
}

/// A request body's trailers, as WASI's future of them.
type TrailersReader = FutureReader<Result<Option<Trailers>, ErrorCode>>;

/// The body of a request as WASI's stream and trailers, which a task writes from Waft's body as it
/// reads it.
fn contents_of(body: Body) -> (StreamReader<u8>, TrailersReader) {
    let (contents, reader) = wit_stream::new::<u8>();
    let (trailers, trailers_reader) = wit_future::new(|| Ok(None));
    wasdk::spawn(stream_body(body, contents, trailers));
    (reader, trailers_reader)
}

/// Streams `body` into `contents`, then ends it: the stream closed and the trailers written, the
/// body's own once its bytes have ended, or the body's failure, which has the host abort the
/// request rather than send a body cut short as if whole.
async fn stream_body(
    mut body: Body,
    mut contents: StreamWriter<u8>,
    trailers: FutureWriter<Result<Option<Trailers>, ErrorCode>>,
) {
    let written = write_contents(&mut body, &mut contents).await;
    drop(contents);
    let ending = match written {
        Ok(()) => match body.trailers().await {
            Ok(trailers) => sent_trailers(trailers),
            Err(error) => Err(ErrorCode::InternalError(Some(format!(
                "the request body's trailers: {error}"
            )))),
        },
        Err(error) => Err(ErrorCode::InternalError(Some(error))),
    };
    // The host reads the trailers only once it has sent the body, and is gone if it stopped.
    let _ = trailers.write(ending).await;
}

/// Writes all of `body` into `contents`, or says why it stopped.
async fn write_contents(body: &mut Body, contents: &mut StreamWriter<u8>) -> Result<(), String> {
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let read = body
            .read(&mut chunk)
            .await
            .map_err(|error| format!("the request body failed: {error}"))?;
        if read == 0 {
            return Ok(());
        }
        if !contents.write_all(chunk[..read].to_vec()).await.is_empty() {
            return Err("the host stopped reading the request body".to_owned());
        }
    }
}

/// A request body's trailers as WASI's, or the failure that aborts the request.
fn sent_trailers(trailers: Option<HeaderMap>) -> Result<Option<Trailers>, ErrorCode> {
    trailers
        .map(|trailers| {
            fields_of(&trailers).map_err(|error| {
                ErrorCode::InternalError(Some(format!("the request body's trailers: {error}")))
            })
        })
        .transpose()
}

/// WASI's request options for `options`, each timeout set only where `options` has one, so that
/// none leaves the host's own.
fn request_options(options: &SendOptions) -> Result<RequestOptions, HttpError> {
    let wasi = RequestOptions::new();
    let refused = |error| HttpError::Internal(Some(format!("WASI refuses a timeout: {error:?}")));
    if let Some(timeout) = options.connect_timeout {
        wasi.set_connect_timeout(Some(nanos(timeout)))
            .map_err(refused)?;
    }
    if let Some(timeout) = options.first_byte_timeout {
        wasi.set_first_byte_timeout(Some(nanos(timeout)))
            .map_err(refused)?;
    }
    if let Some(timeout) = options.between_bytes_timeout {
        wasi.set_between_bytes_timeout(Some(nanos(timeout)))
            .map_err(refused)?;
    }
    Ok(wasi)
}

/// `timeout` in the nanoseconds WASI counts.
fn nanos(timeout: Duration) -> u64 {
    u64::try_from(timeout.as_nanos()).unwrap_or(u64::MAX)
}

/// WASI's response as Waft's, its body streaming.
fn response_of(response: types::Response) -> Result<Response<Body>, HttpError> {
    let status = StatusCode::from_u16(response.get_status_code())
        .map_err(|_| HttpError::HttpProtocolError)?;
    let fields = response.get_headers();
    let headers = headers_of(&fields)?;
    let length = headers
        .get(http::header::CONTENT_LENGTH)
        .and_then(|length| length.to_str().ok())
        .and_then(|length| length.parse().ok());
    let (consumed, done) = wit_future::new(|| Ok(()));
    let (stream, trailers) = types::Response::consume_body(response, done);
    let body = Body::from_stream(WasdkBody {
        stream,
        trailers: Some(trailers),
        ended: None,
        length,
        // Held while the body is read, so WASI knows the body is wanted; dropped with the body.
        _consumed: Some(consumed),
    });
    let mut answer = Response::new(body);
    *answer.status_mut() = status;
    *answer.headers_mut() = headers;
    Ok(answer)
}

/// WASI's error code as Waft's, case for case.
fn http_error(error: ErrorCode) -> HttpError {
    match error {
        ErrorCode::DnsTimeout => HttpError::DnsTimeout,
        ErrorCode::DnsError(payload) => HttpError::DnsError(waft_embedder::DnsErrorPayload {
            rcode: payload.rcode,
            info_code: payload.info_code,
        }),
        ErrorCode::DestinationNotFound => HttpError::DestinationNotFound,
        ErrorCode::DestinationUnavailable => HttpError::DestinationUnavailable,
        ErrorCode::DestinationIpProhibited => HttpError::DestinationIpProhibited,
        ErrorCode::DestinationIpUnroutable => HttpError::DestinationIpUnroutable,
        ErrorCode::ConnectionRefused => HttpError::ConnectionRefused,
        ErrorCode::ConnectionTerminated => HttpError::ConnectionTerminated,
        ErrorCode::ConnectionTimeout => HttpError::ConnectionTimeout,
        ErrorCode::ConnectionReadTimeout => HttpError::ConnectionReadTimeout,
        ErrorCode::ConnectionWriteTimeout => HttpError::ConnectionWriteTimeout,
        ErrorCode::ConnectionLimitReached => HttpError::ConnectionLimitReached,
        ErrorCode::TlsProtocolError => HttpError::TlsProtocolError,
        ErrorCode::TlsCertificateError => HttpError::TlsCertificateError,
        ErrorCode::TlsAlertReceived(payload) => {
            HttpError::TlsAlertReceived(waft_embedder::TlsAlertReceivedPayload {
                alert_id: payload.alert_id,
                alert_message: payload.alert_message,
            })
        }
        ErrorCode::HttpRequestDenied => HttpError::HttpRequestDenied,
        ErrorCode::HttpRequestLengthRequired => HttpError::HttpRequestLengthRequired,
        ErrorCode::HttpRequestBodySize(limit) => HttpError::HttpRequestBodySize(limit),
        ErrorCode::HttpRequestMethodInvalid => HttpError::HttpRequestMethodInvalid,
        ErrorCode::HttpRequestUriInvalid => HttpError::HttpRequestUriInvalid,
        ErrorCode::HttpRequestUriTooLong => HttpError::HttpRequestUriTooLong,
        ErrorCode::HttpRequestHeaderSectionSize(limit) => {
            HttpError::HttpRequestHeaderSectionSize(limit)
        }
        ErrorCode::HttpRequestHeaderSize(field) => {
            HttpError::HttpRequestHeaderSize(field.map(field_size))
        }
        ErrorCode::HttpRequestTrailerSectionSize(limit) => {
            HttpError::HttpRequestTrailerSectionSize(limit)
        }
        ErrorCode::HttpRequestTrailerSize(field) => {
            HttpError::HttpRequestTrailerSize(field_size(field))
        }
        ErrorCode::HttpResponseIncomplete => HttpError::HttpResponseIncomplete,
        ErrorCode::HttpResponseHeaderSectionSize(limit) => {
            HttpError::HttpResponseHeaderSectionSize(limit)
        }
        ErrorCode::HttpResponseHeaderSize(field) => {
            HttpError::HttpResponseHeaderSize(field_size(field))
        }
        ErrorCode::HttpResponseBodySize(limit) => HttpError::HttpResponseBodySize(limit),
        ErrorCode::HttpResponseTrailerSectionSize(limit) => {
            HttpError::HttpResponseTrailerSectionSize(limit)
        }
        ErrorCode::HttpResponseTrailerSize(field) => {
            HttpError::HttpResponseTrailerSize(field_size(field))
        }
        ErrorCode::HttpResponseTransferCoding(coding) => {
            HttpError::HttpResponseTransferCoding(coding)
        }
        ErrorCode::HttpResponseContentCoding(coding) => {
            HttpError::HttpResponseContentCoding(coding)
        }
        ErrorCode::HttpResponseTimeout => HttpError::HttpResponseTimeout,
        ErrorCode::HttpUpgradeFailed => HttpError::HttpUpgradeFailed,
        ErrorCode::HttpProtocolError => HttpError::HttpProtocolError,
        ErrorCode::LoopDetected => HttpError::LoopDetected,
        ErrorCode::ConfigurationError => HttpError::ConfigurationError,
        ErrorCode::InternalError(reason) => HttpError::Internal(reason),
    }
}

/// WASI's field size as Waft's.
fn field_size(field: FieldSizePayload) -> waft_embedder::FieldSizePayload {
    waft_embedder::FieldSizePayload {
        field_name: field.field_name,
        field_size: field.field_size,
    }
}

/// A response body WASI streams, read as Waft reads one.
struct WasdkBody {
    stream: StreamReader<u8>,
    /// WASI's trailers, until the body has ended; none once read.
    trailers: Option<FutureReader<Result<Option<Trailers>, ErrorCode>>>,
    /// How the body ended, once it has: its trailers, or why it stopped.
    ended: Option<Result<Option<HeaderMap>, HttpError>>,
    length: Option<u64>,
    /// The app's word that it is done with the body, which WASI reads: held until the body goes.
    _consumed: Option<FutureWriter<Result<(), ErrorCode>>>,
}

impl WasdkBody {
    /// Reads WASI's trailers future, which says why a body stopped short.
    async fn read_trailers(&mut self) -> Result<Option<HeaderMap>, HttpError> {
        match self.trailers.take() {
            Some(trailers) => {
                let trailers = trailers.await.map_err(http_error)?;
                trailers_of(trailers)
            }
            None => Ok(None),
        }
    }
}

impl BodyStream for WasdkBody {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BodyReadFuture<'a> {
        Box::pin(async move {
            if let Some(ended) = &self.ended {
                return match ended {
                    Ok(_) => Ok(0),
                    Err(error) => Err(error.clone()),
                };
            }
            if buf.is_empty() {
                return Ok(0);
            }
            let (result, data) = self.stream.read(Vec::with_capacity(buf.len())).await;
            let read = match result {
                StreamResult::Complete(read) => read,
                StreamResult::Dropped | StreamResult::Cancelled => 0,
            };
            if read > 0 {
                let read = read.min(buf.len());
                buf[..read].copy_from_slice(&data[..read]);
                return Ok(read);
            }
            let ended = self.read_trailers().await;
            self.ended = Some(ended.clone());
            ended.map(|_| 0)
        })
    }

    fn trailers(&mut self) -> BodyTrailersFuture<'_> {
        Box::pin(async move {
            if let Some(ended) = &self.ended {
                return ended.clone();
            }
            let trailers = self.read_trailers().await;
            self.ended = Some(trailers.clone());
            trailers
        })
    }

    fn length(&self) -> Option<u64> {
        self.length
    }
}

#[cfg(test)]
mod tests {
    use wasdk::http::types::DnsErrorPayload;

    use super::*;

    #[test]
    fn a_method_says_what_it_is_and_what_it_is_not() {
        assert!(matches!(wit_method(&Method::GET), WitMethod::Get));
        assert!(matches!(wit_method(&Method::POST), WitMethod::Post));
        assert!(matches!(
            wit_method(&Method::from_bytes(b"BREW").unwrap()),
            WitMethod::Other(other) if other == "BREW"
        ));
    }

    #[test]
    fn a_location_is_taken_against_the_request_it_answers() {
        let current: Uri = "https://example.com/a/b?q=1".parse().unwrap();
        assert_eq!(
            location_target(&current, "/c").unwrap(),
            "https://example.com/c".parse::<Uri>().unwrap()
        );
        assert_eq!(
            location_target(&current, "d").unwrap(),
            "https://example.com/a/d".parse::<Uri>().unwrap()
        );
        assert_eq!(
            location_target(&current, "http://other.test/e").unwrap(),
            "http://other.test/e".parse::<Uri>().unwrap()
        );
        assert_eq!(
            location_target(&current, "//other.test/f").unwrap(),
            "https://other.test/f".parse::<Uri>().unwrap()
        );
        assert_eq!(
            location_target(&current, "?q=2").unwrap(),
            "https://example.com/a/b?q=2".parse::<Uri>().unwrap()
        );
        assert_eq!(
            location_target(&current, "../g#part").unwrap(),
            "https://example.com/g".parse::<Uri>().unwrap()
        );
    }

    #[test]
    fn a_redirect_changes_the_method_as_fetch_does() {
        assert_eq!(
            redirected(StatusCode::SEE_OTHER, &Method::POST),
            Redirected::AsGet
        );
        assert_eq!(
            redirected(StatusCode::SEE_OTHER, &Method::PUT),
            Redirected::AsGet
        );
        assert_eq!(
            redirected(StatusCode::SEE_OTHER, &Method::HEAD),
            Redirected::Same
        );
        assert_eq!(
            redirected(StatusCode::FOUND, &Method::POST),
            Redirected::AsGet
        );
        assert_eq!(
            redirected(StatusCode::MOVED_PERMANENTLY, &Method::DELETE),
            Redirected::Same
        );
        assert_eq!(
            redirected(StatusCode::TEMPORARY_REDIRECT, &Method::POST),
            Redirected::Same
        );
        assert_eq!(
            redirected(StatusCode::PERMANENT_REDIRECT, &Method::GET),
            Redirected::Same
        );
    }

    /// A body that streams, so it cannot be sent again: the copy a redirect that keeps the body
    /// needs.
    struct Stream;

    impl BodyStream for Stream {
        fn read<'a>(&'a mut self, _buf: &'a mut [u8]) -> BodyReadFuture<'a> {
            Box::pin(std::future::ready(Ok(0)))
        }

        fn trailers(&mut self) -> BodyTrailersFuture<'_> {
            Box::pin(std::future::ready(Ok(None)))
        }

        fn length(&self) -> Option<u64> {
            None
        }
    }

    #[test]
    fn a_redirect_that_keeps_the_body_needs_one_held_in_memory() {
        let held = Body::from(b"a body".to_vec());
        assert!(matches!(
            next_hop(
                StatusCode::TEMPORARY_REDIRECT,
                &Method::POST,
                held.try_clone()
            ),
            Some(Next::Same(_))
        ));
        assert!(
            next_hop(
                StatusCode::PERMANENT_REDIRECT,
                &Method::POST,
                Body::from_stream(Stream).try_clone()
            )
            .is_none(),
            "a streamed body is gone, so the redirect is the app's"
        );
        assert!(
            matches!(
                next_hop(StatusCode::SEE_OTHER, &Method::POST, None),
                Some(Next::AsGet)
            ),
            "a GET needs no body"
        );
    }

    #[test]
    fn credentials_stay_within_their_origin() {
        let from: Uri = "https://example.com/a".parse().unwrap();
        assert!(same_origin(
            &from,
            &"https://EXAMPLE.com/b".parse().unwrap()
        ));
        assert!(!same_origin(
            &from,
            &"http://example.com/a".parse().unwrap()
        ));
        assert!(!same_origin(
            &from,
            &"https://example.com:8443/a".parse().unwrap()
        ));
        assert!(!same_origin(
            &from,
            &"https://other.test/a".parse().unwrap()
        ));
    }

    #[test]
    fn an_error_code_keeps_its_case_and_payload() {
        assert!(matches!(
            http_error(ErrorCode::DnsTimeout),
            HttpError::DnsTimeout
        ));
        assert!(matches!(
            http_error(ErrorCode::DnsError(DnsErrorPayload {
                rcode: Some("NXDOMAIN".to_owned()),
                info_code: Some(3),
            })),
            HttpError::DnsError(payload) if payload.rcode.as_deref() == Some("NXDOMAIN")
        ));
        assert!(matches!(
            http_error(ErrorCode::InternalError(Some("CORS".to_owned()))),
            HttpError::Internal(Some(reason)) if reason == "CORS"
        ));
    }
}
