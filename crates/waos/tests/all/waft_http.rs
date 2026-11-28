//! `examples/waft-http`, a Waft app over `wasdk-adapter-waft`'s HTTP client, under the test shell
//! and against servers on this computer: a response's trailers come through `Body::trailers`
//! after its body has ended, a request's trailers are sent after its streamed or held body, a 307
//! follows with a body held in memory and is answered to the app where the body streamed, a 301,
//! 302 or 303 of a `POST` becomes a `GET` without the body, the redirect limit is told from a
//! redirect whose body can never be sent again, credentials stay within their origin, and a `GET`
//! or `HEAD` with no body sends no `Content-Length: 0`. It is a debug build of Waft, slow to start,
//! and draws, so it runs where the engine makes hardware buffers, on Apple's systems.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use tempfile::TempDir;
use waos::{
    AppInstance, ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramOutput, ProgramView, WaOS,
};
use waos_capabilities::ProgramFolders;

use crate::reaching::{TestGrants, any};
use crate::test_shell::{RecordedOutput, TestShell, config, within};

#[test]
fn trailers_go_both_ways_and_a_307_keeps_a_held_body_but_not_a_streamed_one() {
    let shell = TestShell::new();
    let engine = WaOS::new(config(), shell.shell(), shell.platform()).unwrap();
    let (port, _other, flags) = serve();
    let folders = TempDir::new().unwrap();
    let data = folders.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("port"), port.to_string()).unwrap();
    let output = Arc::new(RecordedOutput::default());
    let grants = TestGrants::new(any(), true);
    let program = engine
        .load(
            Path::new(waos_test_programs::WAFT_HTTP),
            &waos_test_programs::code_of(waos_test_programs::WAFT_HTTP),
        )
        .unwrap();
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&output) as Arc<dyn ProgramOutput>),
        grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
        folders: Some(ProgramFolders {
            data,
            cache: folders.path().join("cache"),
        }),
        ..waos_local::LocalCapabilities::new().environment()
    };
    let running = engine.run(&program, environment).unwrap();
    let view = within(shell.headless.view(0))
        .and_then(|shown| shown.view())
        .expect("waft-http asks for its window");
    output.wait_for("response trailers: x-checksum=7 after 5 bytes");
    output.wait_for("response trailers again: same");
    output.wait_for("request trailers: x-checksum=7");
    output.wait_for("held trailers: x-checksum=7");
    output.wait_for("redirect held trailers: x-checksum=7");
    output.wait_for("redirect in memory: payload");
    output.wait_for("redirect stream: 307 Temporary Redirect");
    output.wait_for("301 POST: GET 0");
    output.wait_for("302 POST: GET 0");
    output.wait_for("303 POST: GET 0");
    output.wait_for("too many: too many redirects");
    output.wait_for("limit stream: 307 Temporary Redirect");
    output.wait_for("credentials: authorization: none");
    output.wait_for("head: 200 OK");
    assert!(
        !flags.content_length_on_get.load(Ordering::SeqCst),
        "a GET with no body sends no Content-Length: 0"
    );
    assert!(
        !flags.content_length_on_head.load(Ordering::SeqCst),
        "a HEAD with no body sends no Content-Length: 0"
    );
    end(&running, &view, &output);
}

/// Closes the window, which ends the program.
fn end(running: &AppInstance, view: &ProgramView, output: &RecordedOutput) {
    view.close_requested();
    let end = within(running.ended());
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "{end:?}: {:#?}",
        output.lines()
    );
}

/// What a server watched for in the requests it answered.
#[derive(Default)]
struct Flags {
    /// Whether a `GET` with no body was sent a `Content-Length: 0`.
    content_length_on_get: AtomicBool,
    /// Whether a `HEAD` with no body was sent a `Content-Length: 0`.
    content_length_on_head: AtomicBool,
}

/// What came with a request: its body and its trailer fields.
type Received = (Vec<u8>, Option<Vec<(String, String)>>);

/// A server on this computer, at the port it answers, and another origin at the second port:
/// `/trailers` streams five bytes and trailers; `/echo-trailers` says the trailers the request
/// sent after its body; `/redirect` and `/redirect-trailers` answer a 307 to `/echo` and
/// `/echo-trailers`; `/moved301`, `/moved302` and `/see303` answer their redirect to `/method`,
/// which says the method and how many bytes came; `/loop` redirects to itself; `/redirect-other`
/// sends the request to `/echo-headers` on the other origin, which says whether `authorization`
/// arrived; and `/head` answers a `HEAD` with no content. Each connection is one request.
fn serve() -> (u16, u16, Arc<Flags>) {
    let (main, port) = listen();
    let (other, other_port) = listen();
    let flags = Arc::new(Flags::default());
    let watched = Arc::clone(&flags);
    thread::spawn(move || {
        for stream in main.incoming().flatten() {
            let watched = Arc::clone(&watched);
            thread::spawn(move || answer(stream, &watched, other_port));
        }
    });
    let watched = Arc::clone(&flags);
    thread::spawn(move || {
        for stream in other.incoming().flatten() {
            let watched = Arc::clone(&watched);
            thread::spawn(move || answer(stream, &watched, port));
        }
    });
    (port, other_port, flags)
}

/// A listener on this computer, and the port it answers.
fn listen() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

/// Reads one request from `stream` and answers it.
fn answer(stream: TcpStream, flags: &Flags, other_port: u16) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let Some(request) = head(&mut reader) else {
        return;
    };
    if matches!(&request.framing, Framing::Length(0)) {
        match request.method.as_str() {
            "GET" => flags.content_length_on_get.store(true, Ordering::SeqCst),
            "HEAD" => flags.content_length_on_head.store(true, Ordering::SeqCst),
            _ => {}
        }
    }
    let Some((body, trailers)) = read_body(&mut reader, &request.framing) else {
        return;
    };
    match request.path.as_str() {
        "/trailers" => chunked(&stream, b"hello", &[("x-checksum", "7")]),
        "/echo-trailers" => page(&stream, "200 OK", trailers_text(&trailers).as_bytes()),
        "/redirect" => moved(&stream, "307 Temporary Redirect", "/echo"),
        "/redirect-trailers" => moved(&stream, "307 Temporary Redirect", "/echo-trailers"),
        "/echo" => page(&stream, "200 OK", &body),
        "/moved301" => moved(&stream, "301 Moved Permanently", "/method"),
        "/moved302" => moved(&stream, "302 Found", "/method"),
        "/see303" => moved(&stream, "303 See Other", "/method"),
        "/method" => page(
            &stream,
            "200 OK",
            format!("{} {}", request.method, body.len()).as_bytes(),
        ),
        "/loop" => moved(&stream, "302 Found", "/loop"),
        "/redirect-other" => moved(
            &stream,
            "307 Temporary Redirect",
            &format!("http://127.0.0.1:{other_port}/echo-headers"),
        ),
        "/echo-headers" => page(
            &stream,
            "200 OK",
            format!(
                "authorization: {}",
                request.authorization.as_deref().unwrap_or("none")
            )
            .as_bytes(),
        ),
        "/head" => page(&stream, "200 OK", b""),
        _ => page(&stream, "404 Not Found", b""),
    }
}

/// A request's method and path, how its body is framed, and its credentials.
struct Head {
    method: String,
    path: String,
    framing: Framing,
    authorization: Option<String>,
}

/// How a request's body is framed.
enum Framing {
    /// No body.
    None,
    /// `Length` bytes.
    Length(usize),
    /// Chunks, then trailers.
    Chunked,
}

/// Reads one request's method, path, framing and credentials.
fn head(reader: &mut BufReader<TcpStream>) -> Option<Head> {
    let mut request = String::new();
    if reader.read_line(&mut request).ok()? == 0 {
        return None;
    }
    let method = request.split_whitespace().next()?.to_owned();
    let path = request.split_whitespace().nth(1)?.to_owned();
    let (mut length, mut chunked, mut authorization) = (None, false, None);
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = value.parse().ok(),
            "transfer-encoding" if value.eq_ignore_ascii_case("chunked") => chunked = true,
            "authorization" => authorization = Some(value.to_owned()),
            _ => {}
        }
    }
    let framing = match (chunked, length) {
        (true, _) => Framing::Chunked,
        (false, Some(length)) => Framing::Length(length),
        (false, None) => Framing::None,
    };
    Some(Head {
        method,
        path,
        framing,
        authorization,
    })
}

/// The body of a request and its trailers, or `None` when the connection ended early.
fn read_body(reader: &mut BufReader<TcpStream>, framing: &Framing) -> Option<Received> {
    match framing {
        Framing::None => Some((Vec::new(), None)),
        Framing::Length(length) => {
            let mut body = vec![0; *length];
            reader.read_exact(&mut body).ok()?;
            Some((body, None))
        }
        Framing::Chunked => {
            let mut body = Vec::new();
            let mut trailers = Vec::new();
            loop {
                let mut size_line = String::new();
                if reader.read_line(&mut size_line).ok()? == 0 {
                    return None;
                }
                let size_text = size_line.trim_end_matches(['\r', '\n']);
                let size_text = size_text.split(';').next().unwrap_or("").trim();
                let size = usize::from_str_radix(size_text, 16).ok()?;
                if size == 0 {
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).ok()? == 0 {
                            return None;
                        }
                        let line = line.trim_end_matches(['\r', '\n']);
                        if line.is_empty() {
                            break;
                        }
                        if let Some((name, value)) = line.split_once(':') {
                            trailers
                                .push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
                        }
                    }
                    return Some((body, Some(trailers)));
                }
                let start = body.len();
                body.resize(start + size, 0);
                reader.read_exact(&mut body[start..]).ok()?;
                let mut crlf = String::new();
                reader.read_line(&mut crlf).ok()?;
            }
        }
    }
}

/// `trailers` as `name=value`, one pair after another.
fn trailers_text(trailers: &Option<Vec<(String, String)>>) -> String {
    trailers
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Answers with `body` and its length.
fn page(stream: &TcpStream, status: &str, body: &[u8]) {
    let mut stream = stream;
    let head = format!(
        "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

/// Answers with `body` in chunks and `trailers` after them.
fn chunked(stream: &TcpStream, body: &[u8], trailers: &[(&str, &str)]) {
    let mut stream = stream;
    let names = trailers
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ");
    let mut response = format!(
        "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\ntrailer: {names}\r\nconnection: close\r\n\r\n"
    );
    response.push_str(&format!("{:x}\r\n", body.len()));
    response.push_str(&String::from_utf8_lossy(body));
    response.push_str("\r\n0\r\n");
    for (name, value) in trailers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    let _ = stream.write_all(response.as_bytes());
}

/// Answers with a redirect to `location`.
fn moved(stream: &TcpStream, status: &str, location: &str) {
    let mut stream = stream;
    let response = format!(
        "HTTP/1.1 {status}\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    );
    let _ = stream.write_all(response.as_bytes());
}
