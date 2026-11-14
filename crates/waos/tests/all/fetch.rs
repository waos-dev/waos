//! `examples/fetch` under grants of each test's own, against an HTTP server on this computer: a
//! request goes out when its host passes the program's network, by name or by address, and is
//! refused with `HTTP-request-denied` otherwise, a name the domain rule refuses before anything
//! is sent; this computer is reached under the local-network switch whatever its name, and a
//! name the domain rule allows does not reach it without that switch; with both switches off
//! nothing goes out; a redirect comes back to the program, which WaOS does not follow; a body the
//! program streams reaches the server whole and the one echoed comes back as a stream; and a
//! request the program drops before it is answered closes its connection. `nowhere.example` is a
//! name no one serves, so the rule's refusal tells from the failure of a request let out.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use crate::reaching::{Run, TestGrants, any, off, only, port_of};

/// How long `/closed` waits for the connection of `/never` to close.
const CLOSE_WITHIN: Duration = Duration::from_secs(5);

#[test]
fn a_request_goes_to_a_host_the_program_may_reach_and_no_redirect_is_followed() {
    let grants = TestGrants::new(only(&["allowed.localhost"]), true);
    let run = Run::start(waos_test_programs::FETCH, &grants, &port_of(serve_pages()));
    run.returned();
    assert_eq!(
        run.lines.all(),
        [
            "get allowed.localhost/hello: 200 hello",
            "get refused.localhost/hello: 200 hello",
            "get 127.0.0.1/hello: 200 hello",
            "get allowed.localhost/moved: 302 /hello",
            "get nowhere.example/hello: ErrorCode::HttpRequestDenied",
            "post 127.0.0.1/echo: 200 streamed both ways",
            "get 127.0.0.1/never: dropped",
            "get 127.0.0.1/closed: 200 closed",
        ]
    );
}

#[test]
fn a_name_the_rule_allows_does_not_reach_this_computer_without_the_local_network() {
    let grants = TestGrants::new(any(), false);
    let run = Run::start(waos_test_programs::FETCH, &grants, &port_of(serve_pages()));
    run.returned();
    let lines = run.lines.all();
    assert_eq!(
        lines[..4],
        [
            "get allowed.localhost/hello: ErrorCode::HttpRequestDenied",
            "get refused.localhost/hello: ErrorCode::HttpRequestDenied",
            "get 127.0.0.1/hello: ErrorCode::HttpRequestDenied",
            "get allowed.localhost/moved: ErrorCode::HttpRequestDenied",
        ]
    );
    assert!(
        lines[4].starts_with("get nowhere.example/hello: ")
            && !lines[4].ends_with("HttpRequestDenied"),
        "let out, and failed as no one serves it: {}",
        lines[4]
    );
}

#[test]
fn with_both_switches_off_no_request_goes_out() {
    let grants = TestGrants::new(off(), false);
    let run = Run::start(waos_test_programs::FETCH, &grants, &port_of(serve_pages()));
    run.returned();
    assert_eq!(
        run.lines.all(),
        [
            "get allowed.localhost/hello: ErrorCode::HttpRequestDenied",
            "get refused.localhost/hello: ErrorCode::HttpRequestDenied",
            "get 127.0.0.1/hello: ErrorCode::HttpRequestDenied",
            "get allowed.localhost/moved: ErrorCode::HttpRequestDenied",
            "get nowhere.example/hello: ErrorCode::HttpRequestDenied",
            "post 127.0.0.1/echo: ErrorCode::HttpRequestDenied",
            "get 127.0.0.1/never: ErrorCode::HttpRequestDenied",
            "get 127.0.0.1/closed: ErrorCode::HttpRequestDenied",
        ]
    );
}

/// Whether the connection of `/never` was opened, then closed.
#[derive(Default)]
struct Never {
    state: Mutex<NeverState>,
    changed: Condvar,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum NeverState {
    #[default]
    NotAsked,
    Open,
    Closed,
}

impl Never {
    fn set(&self, state: NeverState) {
        *self.state.lock().unwrap() = state;
        self.changed.notify_all();
    }

    /// Whether the connection closed, waiting for it no longer than [`CLOSE_WITHIN`].
    fn closed(&self) -> bool {
        let state = self.state.lock().unwrap();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, CLOSE_WITHIN, |state| *state != NeverState::Closed)
            .unwrap();
        *state == NeverState::Closed
    }
}

/// An HTTP server on this computer, at the port it answers: `/hello` is a page, `/moved` a
/// redirect to it, `/echo` answers the body it was sent, `/never` answers nothing and holds its
/// connection until the client closes it, `/closed` says whether it has, and anything else is not
/// found. Each connection is one request.
fn serve_pages() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let never = Arc::new(Never::default());
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let never = Arc::clone(&never);
            thread::spawn(move || answer(stream, &never));
        }
    });
    port
}

/// Reads one request from `stream` and answers it.
fn answer(stream: TcpStream, never: &Never) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let Some((path, framing)) = request_head(&mut reader) else {
        return;
    };
    let response = match path.as_str() {
        "/hello" => page("200 OK", "hello"),
        "/moved" => {
            "HTTP/1.1 302 Found\r\nlocation: /hello\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                .to_owned()
        }
        "/echo" => match body(&mut reader, framing) {
            Some(body) => page("200 OK", &body),
            None => return,
        },
        "/never" => {
            never.set(NeverState::Open);
            while reader.read(&mut [0; 64]).is_ok_and(|read| read > 0) {}
            never.set(NeverState::Closed);
            return;
        }
        "/closed" => page("200 OK", if never.closed() { "closed" } else { "open" }),
        _ => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned(),
    };
    let _ = (&stream).write_all(response.as_bytes());
}

/// How a request's body is framed.
enum Framing {
    Length(usize),
    Chunked,
}

/// The path of the request whose head `reader` holds, and how its body is framed.
fn request_head(reader: &mut impl BufRead) -> Option<(String, Framing)> {
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let mut framing = Framing::Length(0);
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 {
            return None;
        }
        if header == "\r\n" {
            break;
        }
        let (name, value) = header.split_once(':')?;
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "content-length" => framing = Framing::Length(value.parse().ok()?),
            "transfer-encoding" if value.eq_ignore_ascii_case("chunked") => {
                framing = Framing::Chunked;
            }
            _ => {}
        }
    }
    let path = request_line.split(' ').nth(1)?.to_owned();
    Some((path, framing))
}

/// The body `reader` holds, framed as `framing` says.
fn body(reader: &mut impl BufRead, framing: Framing) -> Option<String> {
    let bytes = match framing {
        Framing::Length(length) => exactly(reader, length)?,
        Framing::Chunked => chunks(reader)?,
    };
    String::from_utf8(bytes).ok()
}

/// A chunked body's chunks, joined.
fn chunks(reader: &mut impl BufRead) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let mut size = String::new();
        reader.read_line(&mut size).ok()?;
        let size = usize::from_str_radix(size.trim(), 16).ok()?;
        let chunk = exactly(reader, size + 2)?;
        if size == 0 {
            return Some(bytes);
        }
        bytes.extend_from_slice(&chunk[..size]);
    }
}

fn exactly(reader: &mut impl Read, length: usize) -> Option<Vec<u8>> {
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

/// A response of `status` whose body is `body`.
fn page(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}
