//! Waft's HTTP client over `wasdk::http`, WASI 0.3's `wasi:http/client`: a request's method, URI
//! and repeated headers, a body streamed both ways, the three timeouts, the response's status,
//! headers, body and trailers, and WASI's error codes passed through, with `TooManyRedirects` the
//! one case this client adds (the design's decisions 9 to 11).
//!
//! WASI follows no redirect, so one is followed here as `SendOptions::redirects` asks: a redirect
//! with a location is sent again at that URI, up to the limit. A `GET` or a `HEAD`, which carries
//! no body here, follows every kind; another method follows 303, 301 and 302 as a `GET` without
//! its body, and 307 and 308 fail, since a body already streamed cannot be sent again.
//!
//! The invariant a later editor will break: never await the transmission future `Request::new`
//! answers, nor write or await the trailers future, before `client::send`: both report on a send
//! that has not begun, and waiting for them waits forever. Dropping them is how a request with
//! nothing to follow is sent.

use std::fmt;
use std::time::Duration;

use http::header::{HeaderName, HeaderValue, LOCATION};
use http::{HeaderMap, Method, Request, Response, StatusCode, Uri};
use waft_embedder::{
    Body, BodyReadFuture, BodyStream, BodyTrailersFuture, HttpClient, HttpError, HttpFuture,
    RedirectPolicy, SendOptions,
};
use wasdk::http::client;
use wasdk::http::types::{
    self, ErrorCode, FieldSizePayload, Fields, Method as WitMethod, Request as WitRequest,
    RequestOptions, Scheme, Trailers,
};
use wasdk::{FutureReader, FutureWriter, StreamReader, StreamResult, wit_future, wit_stream};

/// Waft's HTTP client, as a value: every request is WASI's own, so the client keeps nothing.
pub(crate) struct WasdkHttp;

impl HttpClient for WasdkHttp {
    fn send(&self, request: Request<Body>, options: SendOptions) -> HttpFuture {
        Box::pin(send(request, options))
    }
}

/// Sends `request`, following its redirects as `options` says.
async fn send(request: Request<Body>, options: SendOptions) -> Result<Response<Body>, HttpError> {
    let (parts, body) = request.into_parts();
    let (mut method, mut uri, headers, mut body) = (parts.method, parts.uri, parts.headers, body);
    let mut followed = 0;
    loop {
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
        let target = location_target(&uri, &location)?;
        let status = response.status();
        drop(response);
        let (next_method, next_body) = next_hop(status, &method)?;
        method = next_method;
        uri = target;
        body = next_body;
        followed += 1;
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
    let contents = contents_of(body);
    let (no_trailers, trailers) = wit_future::new(|| Ok(None));
    // Dropped before the send, which reads it as it goes: a trailer writer held across it would
    // leave the request closed only once this call answered.
    drop(no_trailers);
    let (request, _sent) =
        WitRequest::new(fields, contents, trailers, Some(request_options(options)?));
    set_method(&request, method)?;
    set_uri(&request, uri)?;
    let response = client::send(request).await.map_err(http_error)?;
    response_of(response)
}

/// The response's URI per a redirect's method rules: 303 always a `GET`, 301 and 302 a `GET` where
/// the request was not one, and 307 and 308 the same method, which a body cannot be sent again for.
fn next_hop(status: StatusCode, method: &Method) -> Result<(Method, Body), HttpError> {
    let head_or_get = method == Method::GET || method == Method::HEAD;
    match status.as_u16() {
        307 | 308 if !head_or_get => Err(HttpError::Internal(Some(
            "a redirect of a request with a body cannot send the body again".to_owned(),
        ))),
        301..=303 if !head_or_get => Ok((Method::GET, Body::empty())),
        _ => Ok((method.clone(), Body::empty())),
    }
}

/// The URI a response's `Location` names, against the request's.
fn location_target(current: &Uri, location: &str) -> Result<Uri, HttpError> {
    let location = location.trim();
    if location.starts_with("http://") || location.starts_with("https://") {
        return location
            .parse()
            .map_err(|_| HttpError::HttpRequestUriInvalid);
    }
    let scheme = current
        .scheme_str()
        .ok_or(HttpError::HttpRequestUriInvalid)?;
    let authority = current
        .authority()
        .map(|authority| authority.as_str())
        .ok_or(HttpError::HttpRequestUriInvalid)?;
    let target = if location.starts_with('/') {
        format!("{scheme}://{authority}{location}")
    } else {
        let path = current.path();
        let folder = &path[..path.rfind('/').map_or(0, |slash| slash + 1)];
        format!("{scheme}://{authority}{folder}{location}")
    };
    target.parse().map_err(|_| HttpError::HttpRequestUriInvalid)
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

/// The body of a request as WASI's stream, which a task writes from Waft's body as it reads it.
fn contents_of(body: Body) -> Option<StreamReader<u8>> {
    let mut body = body;
    let (mut writer, reader) = wit_stream::new::<u8>();
    wasdk::spawn(async move {
        let mut chunk = vec![0; 64 * 1024];
        loop {
            match body.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(read) => {
                    if !writer.write_all(chunk[..read].to_vec()).await.is_empty() {
                        return;
                    }
                }
            }
        }
    });
    Some(reader)
}

/// WASI's request options for `options`, each timeout as `None` where none.
fn request_options(options: &SendOptions) -> Result<RequestOptions, HttpError> {
    let wasi = RequestOptions::new();
    for set in [
        wasi.set_connect_timeout(options.connect_timeout.map(nanos)),
        wasi.set_first_byte_timeout(options.first_byte_timeout.map(nanos)),
        wasi.set_between_bytes_timeout(options.between_bytes_timeout.map(nanos)),
    ] {
        set.map_err(|error| {
            HttpError::Internal(Some(format!("WASI refuses a timeout: {error:?}")))
        })?;
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
            match &ended {
                Ok(_) => {
                    self.ended = Some(Ok(None));
                    Ok(0)
                }
                Err(error) => {
                    self.ended = Some(Err(error.clone()));
                    Err(error.clone())
                }
            }
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
    }

    #[test]
    fn a_redirect_of_a_body_keeps_its_method_only_where_it_may() {
        let (method, _) = next_hop(StatusCode::SEE_OTHER, &Method::POST).unwrap();
        assert_eq!(method, Method::GET);
        let (method, _) = next_hop(StatusCode::TEMPORARY_REDIRECT, &Method::GET).unwrap();
        assert_eq!(method, Method::GET);
        assert!(
            next_hop(StatusCode::TEMPORARY_REDIRECT, &Method::POST).is_err(),
            "a body cannot be sent again"
        );
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
