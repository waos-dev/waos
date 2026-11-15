//! A request connects to the addresses the program's checked lookup answered, resolved once: a
//! name answering one address for its lookup and another afterwards, as a DNS rebinding answers,
//! cannot lead the request to this computer. Through a proxy, the host is looked up first and the
//! way picked by where it lands: a name the domain rule refuses, let through for its local
//! addresses alone, never reaches the proxy with an internet answer; this computer is reached
//! directly, at the address the lookup answered; an internet name the rule allows goes through
//! the proxy, an HTTP one or a SOCKS one, which is asked for the name. A localhost name is this
//! computer, under the local-network switch whatever the rule, with a proxy or without, and is
//! never resolved. The system's resolver
//! cannot be made to change its answer here, so these tests give the network a resolver of their
//! own, and send through no proxy or through one of their own, never the machine's, which would
//! resolve the host itself.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use wasmtime_wasi_http::{Error, RequestOptions, WasiHttpHooks};

use super::{HttpProxy, ProgramHttp};
use crate::network::test_grants::{TestGrants, any, off, only};
use crate::network::{LookupError, ProgramNetwork};

/// The name the tests' resolver answers for.
const NAME: &str = "rebinding.test";

/// This computer.
const LOOPBACK: &str = "127.0.0.1";

/// An address of the internet's reserved for documentation, which nothing answers.
const DOCUMENTATION: &str = "192.0.2.1";

/// What the tests' proxy answers every request with, which a server never does.
const FROM_THE_PROXY: u16 = 203;

#[test]
fn a_request_connects_to_the_address_its_lookup_answered() {
    let server = Server::start();
    let answers = Answers::first_then(LOOPBACK, DOCUMENTATION);
    let network = ProgramNetwork::resolving_with(TestGrants::new(any(), true), answers.resolver());
    let sent = send(network, HttpProxy::None, &format!("{NAME}:{}", server.port));
    assert!(matches!(sent, Ok(200)), "{sent:?}");
    assert_eq!(answers.calls(), 1, "resolved once");
    assert_eq!(server.connections(), 1);
}

#[test]
fn a_name_answering_this_computer_after_its_lookup_does_not_reach_it() {
    let server = Server::start();
    let answers = Answers::first_then(DOCUMENTATION, LOOPBACK);
    let network = ProgramNetwork::resolving_with(TestGrants::new(any(), false), answers.resolver());
    let sent = send(network, HttpProxy::None, &format!("{NAME}:{}", server.port));
    assert!(
        sent.is_err(),
        "the documentation address answers nothing: {sent:?}"
    );
    assert_eq!(answers.calls(), 1, "resolved once");
    assert_eq!(server.connections(), 0, "this computer was not reached");
}

#[test]
fn through_a_proxy_a_refused_name_answering_the_internet_never_reaches_it() {
    let proxy = Server::start_answering(FROM_THE_PROXY);
    let answers = Answers::first_then(DOCUMENTATION, DOCUMENTATION);
    let network = ProgramNetwork::resolving_with(TestGrants::new(off(), true), answers.resolver());
    let sent = send(
        network,
        HttpProxy::Test {
            scheme: "http",
            port: proxy.port,
        },
        NAME,
    );
    assert!(
        matches!(sent, Err(Error::HttpRequestDenied)),
        "let through for its local addresses alone, it has none: {sent:?}"
    );
    assert_eq!(proxy.connections(), 0, "the proxy was not reached");
}

#[test]
fn through_a_proxy_a_name_answering_this_computer_is_reached_directly() {
    let proxy = Server::start_answering(FROM_THE_PROXY);
    let server = Server::start();
    let answers = Answers::first_then(LOOPBACK, DOCUMENTATION);
    let network = ProgramNetwork::resolving_with(TestGrants::new(any(), true), answers.resolver());
    let sent = send(
        network,
        HttpProxy::Test {
            scheme: "http",
            port: proxy.port,
        },
        &format!("{NAME}:{}", server.port),
    );
    assert!(matches!(sent, Ok(200)), "{sent:?}");
    assert_eq!(answers.calls(), 1, "resolved once");
    assert_eq!(server.connections(), 1);
    assert_eq!(proxy.connections(), 0, "the proxy was not reached");
}

#[test]
fn through_a_proxy_an_address_of_this_computer_is_reached_directly() {
    let proxy = Server::start_answering(FROM_THE_PROXY);
    let server = Server::start();
    let answers = Answers::first_then(DOCUMENTATION, DOCUMENTATION);
    let network = ProgramNetwork::resolving_with(TestGrants::new(off(), true), answers.resolver());
    let sent = send(
        network,
        HttpProxy::Test {
            scheme: "http",
            port: proxy.port,
        },
        &format!("{LOOPBACK}:{}", server.port),
    );
    assert!(matches!(sent, Ok(200)), "{sent:?}");
    assert_eq!(answers.calls(), 0, "an address answers itself");
    assert_eq!(server.connections(), 1);
    assert_eq!(proxy.connections(), 0, "the proxy was not reached");
}

#[test]
fn through_a_proxy_a_name_the_rule_allows_on_the_internet_goes_through_it() {
    let proxy = Server::start_answering(FROM_THE_PROXY);
    let answers = Answers::first_then(DOCUMENTATION, LOOPBACK);
    let network = ProgramNetwork::resolving_with(TestGrants::new(any(), false), answers.resolver());
    let sent = send(
        network,
        HttpProxy::Test {
            scheme: "http",
            port: proxy.port,
        },
        NAME,
    );
    assert!(matches!(sent, Ok(FROM_THE_PROXY)), "{sent:?}");
    assert_eq!(answers.calls(), 1, "looked up by WaOS before the proxy");
    assert_eq!(proxy.connections(), 1);
    assert_eq!(
        proxy.request_lines(),
        [format!("GET http://{NAME}/ HTTP/1.1")],
        "the proxy is asked for the name"
    );
}

#[test]
fn through_a_socks_proxy_a_name_the_rule_allows_on_the_internet_is_asked_for_by_name() {
    let proxy = SocksProxy::start();
    let answers = Answers::first_then(DOCUMENTATION, LOOPBACK);
    let network = ProgramNetwork::resolving_with(TestGrants::new(any(), false), answers.resolver());
    let sent = send(
        network,
        HttpProxy::Test {
            scheme: "socks5",
            port: proxy.port,
        },
        NAME,
    );
    assert!(matches!(sent, Ok(FROM_THE_PROXY)), "{sent:?}");
    assert_eq!(answers.calls(), 1, "looked up by WaOS before the proxy");
    assert_eq!(
        proxy.targets(),
        [format!("{NAME}:80")],
        "the proxy is asked for the name, not an address"
    );
}

#[test]
fn a_localhost_name_reaches_this_computer_under_the_local_network_whatever_the_rule() {
    let server = Server::start();
    let answers = Answers::first_then(DOCUMENTATION, DOCUMENTATION);
    let network = ProgramNetwork::resolving_with(
        TestGrants::new(only(&["example.com"]), true),
        answers.resolver(),
    );
    let sent = send(
        network,
        HttpProxy::None,
        &format!("app.localhost:{}", server.port),
    );
    assert!(matches!(sent, Ok(200)), "{sent:?}");
    assert_eq!(
        answers.calls(),
        0,
        "answered by the engine, not the resolver"
    );
    assert_eq!(server.connections(), 1);
}

#[test]
fn a_localhost_name_is_refused_with_the_local_network_switch_off() {
    let server = Server::start();
    let answers = Answers::first_then(LOOPBACK, LOOPBACK);
    let network = ProgramNetwork::resolving_with(
        TestGrants::new(only(&["app.localhost"]), false),
        answers.resolver(),
    );
    let sent = send(
        network,
        HttpProxy::None,
        &format!("app.localhost:{}", server.port),
    );
    assert!(matches!(sent, Err(Error::HttpRequestDenied)), "{sent:?}");
    assert_eq!(server.connections(), 0);
}

#[test]
fn through_a_proxy_a_localhost_name_is_reached_directly() {
    let proxy = Server::start_answering(FROM_THE_PROXY);
    let server = Server::start();
    let answers = Answers::first_then(DOCUMENTATION, DOCUMENTATION);
    let network = ProgramNetwork::resolving_with(TestGrants::new(any(), true), answers.resolver());
    let sent = send(
        network,
        HttpProxy::Test {
            scheme: "http",
            port: proxy.port,
        },
        &format!("app.localhost:{}", server.port),
    );
    assert!(matches!(sent, Ok(200)), "{sent:?}");
    assert_eq!(
        answers.calls(),
        0,
        "answered by the engine, not the resolver"
    );
    assert_eq!(server.connections(), 1);
    assert_eq!(proxy.connections(), 0, "the proxy was not reached");
}

/// Sends a request for `http://{authority}/` through `network` and `proxy`, on a runtime as a
/// program's thread has, and answers its status.
fn send(network: ProgramNetwork, proxy: HttpProxy, authority: &str) -> Result<u16, Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut http = ProgramHttp::new(network, proxy);
    let body = Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed_unsync();
    let request = http::Request::get(format!("http://{authority}/"))
        .body(body)
        .unwrap();
    let options = RequestOptions {
        connect_timeout: Some(Duration::from_millis(200)),
        first_byte_timeout: Some(Duration::from_millis(200)),
        between_bytes_timeout: None,
    };
    let sending = http.send_request(request, Some(options), Box::new(async { Ok(()) }));
    let (response, _errors) = runtime.block_on(Pin::from(sending))?;
    Ok(response.status().as_u16())
}

/// A resolver answering one address for its first lookup and another for every one after.
#[derive(Clone)]
struct Answers {
    first: IpAddr,
    then: IpAddr,
    calls: Arc<AtomicUsize>,
}

impl Answers {
    fn first_then(first: &str, then: &str) -> Answers {
        Answers {
            first: first.parse().unwrap(),
            then: then.parse().unwrap(),
            calls: Arc::default(),
        }
    }

    fn resolver(&self) -> crate::network::Resolver {
        let answers = self.clone();
        Arc::new(move |name: &str| {
            if name != NAME {
                return Err(LookupError::Unresolvable);
            }
            let call = answers.calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![if call == 0 {
                answers.first
            } else {
                answers.then
            }])
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

/// An HTTP server on this computer answering each request with one status, counting its
/// connections and keeping each request's first line: a server, or a proxy standing in for the
/// system's, which is asked for a whole URL.
struct Server {
    port: u16,
    connections: Arc<AtomicUsize>,
    request_lines: Arc<Mutex<Vec<String>>>,
}

impl Server {
    /// A server answering `200`.
    fn start() -> Server {
        Server::start_answering(200)
    }

    fn start_answering(status: u16) -> Server {
        let listener = TcpListener::bind((LOOPBACK, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let connections = Arc::new(AtomicUsize::new(0));
        let request_lines = Arc::new(Mutex::new(Vec::new()));
        let (counted, kept) = (Arc::clone(&connections), Arc::clone(&request_lines));
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                counted.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() {
                    kept.lock().unwrap().push(line.trim_end().to_owned());
                }
                while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
                    line.clear();
                }
                let response = format!(
                    "HTTP/1.1 {status} Answered\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                );
                let _ = (&stream).write_all(response.as_bytes());
            }
        });
        Server {
            port,
            connections,
            request_lines,
        }
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    fn request_lines(&self) -> Vec<String> {
        self.request_lines.lock().unwrap().clone()
    }
}

/// A SOCKS 5 proxy on this computer, without authentication, which keeps the target of each
/// connection it is asked for and answers the request sent through it with
/// [`FROM_THE_PROXY`].
struct SocksProxy {
    port: u16,
    targets: Arc<Mutex<Vec<String>>>,
}

impl SocksProxy {
    fn start() -> SocksProxy {
        let listener = TcpListener::bind((LOOPBACK, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let targets = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&targets);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if let Ok(target) = connect(&stream) {
                    kept.lock().unwrap().push(target);
                    answer_through(&stream);
                }
            }
        });
        SocksProxy { port, targets }
    }

    fn targets(&self) -> Vec<String> {
        self.targets.lock().unwrap().clone()
    }
}

/// Agrees on no authentication and takes the connection request on `stream`, answering that it
/// is made; answers its target, a name or an address, with its port.
fn connect(mut stream: &TcpStream) -> std::io::Result<String> {
    let [_version, methods] = read_bytes::<2>(stream)?;
    read_vec(stream, methods.into())?;
    stream.write_all(&[5, 0])?;
    let [_version, _connect, _reserved, kind] = read_bytes::<4>(stream)?;
    let host = match kind {
        1 => IpAddr::from(read_bytes::<4>(stream)?).to_string(),
        3 => {
            let [length] = read_bytes::<1>(stream)?;
            String::from_utf8_lossy(&read_vec(stream, length.into())?).into_owned()
        }
        4 => IpAddr::from(read_bytes::<16>(stream)?).to_string(),
        _ => return Err(std::io::ErrorKind::InvalidData.into()),
    };
    let port = u16::from_be_bytes(read_bytes::<2>(stream)?);
    stream.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0])?;
    Ok(format!("{host}:{port}"))
}

/// Reads the request's head sent through the proxy and answers it.
fn answer_through(stream: &TcpStream) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
        line.clear();
    }
    let response = format!(
        "HTTP/1.1 {FROM_THE_PROXY} Answered\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    );
    let _ = (&*stream).write_all(response.as_bytes());
}

fn read_bytes<const N: usize>(mut stream: &TcpStream) -> std::io::Result<[u8; N]> {
    let mut bytes = [0; N];
    stream.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_vec(mut stream: &TcpStream, length: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    Ok(bytes)
}
