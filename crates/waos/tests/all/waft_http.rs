//! `examples/waft-http`, a Waft app over `wasdk-adapter-waft`'s HTTP client, under the test
//! shell and against a server on this computer: a response's trailers come through `Body::trailers`
//! after its body has ended, a request's trailers are sent after its streamed body, a 307 follows
//! with a body held in memory, and a 307 whose body streamed is answered to the app because the
//! body cannot be sent again. It is a debug build of Waft, slow to start, and draws, so it runs
//! where the engine makes hardware buffers, on Apple's systems.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use tempfile::TempDir;
use waos::{
    ProgramEnd, ProgramEnvironment, ProgramGrants, ProgramOutput, ProgramView, RunningProgram,
};
use waos_capabilities::ProgramFolders;

use crate::reaching::{TestGrants, any};
use crate::shell::{self, Lines, WITHIN};

#[test]
fn trailers_go_both_ways_and_a_307_keeps_a_held_body_but_not_a_streamed_one() {
    let (engine, _shell, arrivals) = shell::engine();
    let (port, content_length_on_get) = serve();
    let folders = TempDir::new().unwrap();
    let data = folders.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("port"), port.to_string()).unwrap();
    let lines = Arc::new(Lines::default());
    let grants = TestGrants::new(any(), true);
    let program = shell::load(&engine, waos_test_programs::WAFT_HTTP);
    let environment = ProgramEnvironment {
        output: Some(Arc::clone(&lines) as Arc<dyn ProgramOutput>),
        grants: Some(Arc::clone(&grants) as Arc<dyn ProgramGrants>),
        folders: Some(ProgramFolders {
            data,
            cache: folders.path().join("cache"),
        }),
        ..waos_local::LocalCapabilities::new().environment()
    };
    let (running, _given) = engine.run(&program, environment, Vec::new()).unwrap();
    let view = arrivals
        .recv_timeout(WITHIN)
        .expect("waft-http asks for its window");
    lines.wait_for("response trailers: x-checksum=7 after 5 bytes");
    lines.wait_for("response trailers again: same");
    lines.wait_for("request trailers: x-checksum=7");
    lines.wait_for("redirect in memory: payload");
    lines.wait_for("redirect stream: 307 Temporary Redirect");
    assert!(
        !content_length_on_get.load(Ordering::SeqCst),
        "a GET with no body sends no Content-Length: 0"
    );
    end(&running, &view, &lines);
}

/// Closes the window, which ends the program.
fn end(running: &RunningProgram, view: &ProgramView, lines: &Lines) {
    view.close_requested();
    let end = shell::wait_for_end(running);
    assert!(
        matches!(end, ProgramEnd::Returned(Ok(()))),
        "{end:?}: {:#?}",
        lines.all()
    );
}

/// What came with a request: its body and its trailer fields.
type Received = (Vec<u8>, Option<Vec<(String, String)>>);

/// A server on this computer, at the port it answers. `/trailers` streams five bytes and
/// trailers, `/echo-trailers` says the trailers the request sent after its body, `/redirect`
/// answers a 307 to `/echo`, and `/echo` answers the body it was sent. Each connection is one
/// request.
fn serve() -> (u16, Arc<AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // Whether a `GET` with no body was sent a `Content-Length: 0`, which it must not be.
    let content_length_on_get = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&content_length_on_get);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let seen = Arc::clone(&seen);
            thread::spawn(move || answer(stream, &seen));
        }
    });
    (port, content_length_on_get)
}

/// Reads one request from `stream` and answers it.
fn answer(stream: TcpStream, content_length_on_get: &AtomicBool) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let Some((method, path, framing)) = head(&mut reader) else {
        return;
    };
    if method == "GET" && matches!(framing, Framing::Length(0)) {
        content_length_on_get.store(true, Ordering::SeqCst);
    }
    let Some((body, trailers)) = read_body(&mut reader, framing) else {
        return;
    };
    match path.as_str() {
        "/trailers" => chunked(&stream, b"hello", &[("x-checksum", "7")]),
        "/echo-trailers" => {
            let text = trailers
                .unwrap_or_default()
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join(", ");
            page(&stream, "200 OK", text.as_bytes());
        }
        "/redirect" => {
            let response = "HTTP/1.1 307 Temporary Redirect\r\nlocation: /echo\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
            let mut writer = &stream;
            let _ = writer.write_all(response.as_bytes());
        }
        "/echo" => page(&stream, "200 OK", &body),
        _ => page(&stream, "404 Not Found", b""),
    }
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

/// The method and path a request asks for, and how its body is framed.
fn head(reader: &mut BufReader<TcpStream>) -> Option<(String, String, Framing)> {
    let mut request = String::new();
    if reader.read_line(&mut request).ok()? == 0 {
        return None;
    }
    let method = request.split_whitespace().next()?.to_owned();
    let path = request.split_whitespace().nth(1)?.to_owned();
    let (mut length, mut chunked) = (None, false);
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
            _ => {}
        }
    }
    let framing = match (chunked, length) {
        (true, _) => Framing::Chunked,
        (false, Some(length)) => Framing::Length(length),
        (false, None) => Framing::None,
    };
    Some((method, path, framing))
}

/// The body of a request and its trailers, or `None` when the connection ended early.
fn read_body(reader: &mut BufReader<TcpStream>, framing: Framing) -> Option<Received> {
    match framing {
        Framing::None => Some((Vec::new(), None)),
        Framing::Length(length) => {
            let mut body = vec![0; length];
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
