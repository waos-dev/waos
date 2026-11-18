//! A Waft app that sends HTTP through `wasdk-adapter-waft`: it reads a response's trailers, sends
//! a request whose body is streamed with trailers of its own, follows a 307 that resends a body
//! held in memory, and answers a 307 whose body streamed, which cannot be sent again.
//!
//! It reads the port its test's server answers on from `/data/port` and prints each answer as
//! `what: result`, so the engine's test can check them.
//!
//! ```text
//! cargo build -p waft-http --target wasm32-wasip3
//! ```

use http::{HeaderMap, Method, Request};
use waft::{App, Color, ColoredBox, IntoWidget, WindowConfig, run_app_with};
use waft_embedder::{
    Body, BodyReadFuture, BodyStream, BodyTrailersFuture, HttpFuture, SendOptions,
};

#[wasdk::main]
async fn main() {
    wasdk_adapter_waft::run(|app| {
        let window = WindowConfig {
            title: "Waft HTTP".to_owned(),
            size: [320.0, 240.0],
            ..WindowConfig::default()
        };
        run_app_with(
            app,
            ColoredBox::new(Color::from_argb(255, 30, 30, 30)).into_widget(),
            window,
        );
        let port = std::fs::read_to_string("/data/port")
            .expect("the test writes the server's port")
            .trim()
            .to_owned();
        check(app, &port);
    })
    .await;
}

/// Sends the test's four requests and prints each answer.
fn check(app: &mut App, port: &str) {
    let platform = app.platform();
    let Some(client) = platform.http() else {
        println!("http: none");
        return;
    };
    let base = format!("http://127.0.0.1:{port}");
    let response_trailers = client.send(
        request(Method::GET, &format!("{base}/trailers"), Body::empty()),
        SendOptions::new(),
    );
    // HTTP/1.1 sends a chunked request's trailers only when the header names them.
    let request_trailers = client.send(
        request_with_trailers(
            Method::POST,
            &format!("{base}/echo-trailers"),
            Body::from_stream(Chunks::new()).with_trailers(trailers()),
            "x-checksum",
        ),
        SendOptions::new(),
    );
    let held = client.send(
        request(
            Method::POST,
            &format!("{base}/redirect"),
            Body::from(b"payload".to_vec()),
        ),
        SendOptions::new(),
    );
    let streamed = client.send(
        request(
            Method::POST,
            &format!("{base}/redirect"),
            Body::from_stream(Chunks::new()),
        ),
        SendOptions::new(),
    );

    app.spawn(async move |_cx| {
        report_response_trailers(response_trailers).await;
        report_body("request trailers", request_trailers).await;
        report_body("redirect in memory", held).await;
        report_status("redirect stream", streamed).await;
    });
}

/// A request with `method`, `uri` and `body`.
fn request(method: Method, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(body)
        .expect("a well-formed request")
}

/// A request whose body carries trailers, which `names` announce as HTTP/1.1 asks.
fn request_with_trailers(method: Method, uri: &str, body: Body, names: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("trailer", names)
        .body(body)
        .expect("a well-formed request")
}

/// The trailers the request sends after its body.
fn trailers() -> HeaderMap {
    let mut trailers = HeaderMap::new();
    trailers.insert("x-checksum", "7".parse().unwrap());
    trailers
}

/// Says what a response's trailers are, once its body has ended.
async fn report_response_trailers(sending: HttpFuture) {
    match sending.await {
        Ok(response) => {
            let mut body = response.into_body();
            match body.read_all().await {
                Ok(bytes) => match body.trailers().await {
                    Ok(trailers) => {
                        println!(
                            "response trailers: {} after {} bytes",
                            describe(trailers.clone()),
                            bytes.len()
                        );
                        match body.trailers().await {
                            Ok(again) if again == trailers => {
                                println!("response trailers again: same");
                            }
                            Ok(_) => println!("response trailers again: different"),
                            Err(error) => {
                                println!("response trailers again: failed: {error}")
                            }
                        }
                    }
                    Err(error) => println!("response trailers: failed: {error}"),
                },
                Err(error) => println!("response trailers: body failed: {error}"),
            }
        }
        Err(error) => println!("response trailers: failed: {error}"),
    }
}

/// Says what a response's body is.
async fn report_body(what: &str, sending: HttpFuture) {
    match sending.await {
        Ok(response) => match response.into_body().read_all().await {
            Ok(bytes) => println!("{what}: {}", String::from_utf8_lossy(&bytes)),
            Err(error) => println!("{what}: body failed: {error}"),
        },
        Err(error) => println!("{what}: failed: {error}"),
    }
}

/// Says what a response's status is.
async fn report_status(what: &str, sending: HttpFuture) {
    match sending.await {
        Ok(response) => println!("{what}: {}", response.status()),
        Err(error) => println!("{what}: failed: {error}"),
    }
}

/// `trailers` as `name=value`, one pair after another.
fn describe(trailers: Option<HeaderMap>) -> String {
    match trailers {
        None => "none".to_owned(),
        Some(trailers) => trailers
            .iter()
            .map(|(name, value)| format!("{name}={}", value.to_str().unwrap_or("?")))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// A body that streams `payload` in one piece, its length unknown, so it cannot be sent again.
struct Chunks {
    remaining: &'static [u8],
}

impl Chunks {
    fn new() -> Chunks {
        Chunks {
            remaining: b"payload",
        }
    }
}

impl BodyStream for Chunks {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BodyReadFuture<'a> {
        Box::pin(async move {
            let read = self.remaining.len().min(buf.len());
            buf[..read].copy_from_slice(&self.remaining[..read]);
            self.remaining = &self.remaining[read..];
            Ok(read)
        })
    }

    fn trailers(&mut self) -> BodyTrailersFuture<'_> {
        Box::pin(async { Ok(None) })
    }

    fn length(&self) -> Option<u64> {
        None
    }
}
