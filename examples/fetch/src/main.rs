//! A program that makes HTTP requests through WASI 0.3's HTTP client, and what a reader learns
//! WaOS's HTTP from: it asks a server on this computer for a page by two names and by address,
//! and for a page that has moved, which WaOS tells it of rather than following; it asks a host no
//! one serves, `nowhere.example`, which its test's rule may refuse before anything is sent; it
//! sends a body as a stream and reads the one echoed back the same way; and it gives up on a
//! request the server never answers by dropping it, then asks whether the server saw it go.
//!
//! It prints each request as `get host/path: status body`, the `location` a redirect names in
//! place of its body, or the error WASI answers. It expects its test
//! (`crates/waos/tests/all/fetch.rs`) to serve `/hello`, `/moved`, `/echo`, `/never` and
//! `/closed` on this computer, at the port written in `/data/port`.
//!
//! ```text
//! cargo build -p fetch --target wasm32-wasip3
//! ```

use std::fs;
use std::pin::pin;

use futures::future::{Either, select};
use wasdk::http::client;
use wasdk::http::types::{ErrorCode, Fields, Method, Request, Response, Scheme, Trailers};
use wasdk::{FutureReader, StreamReader, monotonic_clock, wit_future, wit_stream};

/// What the program sends to be echoed, in the two parts it writes.
const SENT: [&str; 2] = ["streamed ", "both ways"];

/// How long the program waits for an answer that never comes before it drops the request, in
/// nanoseconds.
const GIVE_UP_AFTER: u64 = 200_000_000;

#[wasdk::main]
async fn main() {
    let port: u16 = fs::read_to_string("/data/port")
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    for (host, path) in [
        ("allowed.localhost", "/hello"),
        ("refused.localhost", "/hello"),
        ("127.0.0.1", "/hello"),
        ("allowed.localhost", "/moved"),
        ("nowhere.example", "/hello"),
    ] {
        let said = said(get(&format!("{host}:{port}"), path).await);
        println!("get {host}{path}: {said}");
    }
    let authority = format!("127.0.0.1:{port}");
    println!("post 127.0.0.1/echo: {}", said(echo(&authority).await));
    println!("get 127.0.0.1/never: {}", give_up(&authority).await);
    println!(
        "get 127.0.0.1/closed: {}",
        said(get(&authority, "/closed").await)
    );
}

/// Asks `authority` for `path` over HTTP, and answers the status and what came with it.
async fn get(authority: &str, path: &str) -> Result<String, ErrorCode> {
    let response = client::send(request(authority, path, None)).await?;
    told(response).await
}

/// Sends [`SENT`] to `authority`'s `/echo` as a stream, and answers what came back.
async fn echo(authority: &str) -> Result<String, ErrorCode> {
    let request = request(authority, "/echo", Some(stream_of(&SENT)));
    request.set_method(&Method::Post).unwrap();
    let response = client::send(request).await?;
    told(response).await
}

/// Asks `authority` for `/never`, which the server never answers, and drops the request once
/// [`GIVE_UP_AFTER`] has passed.
async fn give_up(authority: &str) -> String {
    let sent = pin!(client::send(request(authority, "/never", None)));
    let waited = pin!(monotonic_clock::wait_for(GIVE_UP_AFTER));
    match select(sent, waited).await {
        Either::Left((answered, _)) => said(answered.map(|response| response.get_status_code())),
        Either::Right(_) => "dropped".to_owned(),
    }
}

/// A request for `path` at `authority` over HTTP, with `contents` as its body.
fn request(authority: &str, path: &str, contents: Option<StreamReader<u8>>) -> Request {
    let (request, _sent) = Request::new(Fields::new(), contents, no_trailers(), None);
    request.set_scheme(Some(&Scheme::Http)).unwrap();
    request.set_authority(Some(authority)).unwrap();
    request.set_path_with_query(Some(path)).unwrap();
    request
}

/// A request's trailers when it sends none: a future that resolves to none.
fn no_trailers() -> FutureReader<Result<Option<Trailers>, ErrorCode>> {
    let (_none, trailers) = wit_future::new(|| Ok(None));
    trailers
}

/// `parts`, written one after the other into a stream the program gives.
fn stream_of(parts: &[&str]) -> StreamReader<u8> {
    let (mut writer, reader) = wit_stream::new::<u8>();
    let parts: Vec<Vec<u8>> = parts.iter().map(|part| part.as_bytes().to_vec()).collect();
    wasdk::spawn(async move {
        for part in parts {
            // What a write leaves unwritten, the reader is gone; writing again would trap.
            if !writer.write_all(part).await.is_empty() {
                return;
            }
        }
    });
    reader
}

/// The status of `response` and what came with it: where it redirects to, or its body.
async fn told(response: Response) -> Result<String, ErrorCode> {
    let status = response.get_status_code();
    let told = match location(&response) {
        Some(location) => location,
        None => body(response).await,
    };
    Ok(format!("{status} {told}"))
}

/// Where a redirect sends the program, if the response is one.
fn location(response: &Response) -> Option<String> {
    let headers = response.get_headers();
    let location = headers.get("location").into_iter().next()?;
    Some(String::from_utf8_lossy(&location).into_owned())
}

/// The whole of the response's body, as text.
async fn body(response: Response) -> String {
    let (_read, read_result) = wit_future::new(|| Ok(()));
    let (contents, _trailers) = Response::consume_body(response, read_result);
    let bytes = contents.collect().await;
    String::from_utf8_lossy(&bytes).into_owned()
}

/// What a request answered, or the error WASI answered, as printed.
fn said(answered: Result<impl ToString, ErrorCode>) -> String {
    match answered {
        Ok(said) => said.to_string(),
        Err(error) => format!("{error:?}"),
    }
}
