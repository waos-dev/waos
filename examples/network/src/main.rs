//! A program that reaches the network through `std::net` and through `wasdk::sockets`, WASI 0.3's
//! sockets, which it awaits, and what a reader learns WaOS's network from: it looks up a name its
//! test's domain rule allows and one it does not, and a name under `localhost`, which is this
//! computer on every system; connects to this computer by name and by address, sends a datagram
//! to an address of the internet, does the same lookups and a connection with an echo through
//! WASI 0.3's sockets, and holds a connection to this computer while its user turns the switch
//! off, then finds a new connection refused and the one it holds still answering.
//!
//! It prints each result as `what: result`, an error by its kind. The names of the rule are under
//! `example`, which no one serves, so a lookup the rule lets through prints `not refused`
//! whatever the system answers. It expects its test
//! (`crates/waos/tests/all/network.rs`) to serve an echo on this computer, at the port written in
//! `/data/port`.
//!
//! ```text
//! cargo build -p network --target wasm32-wasip3
//! ```

use std::fs;
use std::io::{self, BufRead, BufReader, ErrorKind, Write};
use std::net::{TcpStream, ToSocketAddrs, UdpSocket};

use wasdk::monotonic_clock;
use wasdk::sockets::ip_name_lookup;
use wasdk::sockets::types::{
    ErrorCode, IpAddressFamily, IpSocketAddress, Ipv4SocketAddress, TcpSocket,
};
use wasdk::wit_stream;

/// An address of the internet's, reserved for documentation, which nothing answers.
const DOCUMENTATION_ADDRESS: &str = "192.0.2.1:9";

/// How long the program waits between two tries of a new connection, in nanoseconds.
const TRY_EVERY: u64 = 10_000_000;

#[wasdk::main]
async fn main() {
    let port: u16 = fs::read_to_string("/data/port")
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    look_up_by_rule("allowed.example");
    look_up_by_rule("refused.example");
    look_up("app.localhost");
    say(
        "connect localhost",
        TcpStream::connect(("localhost", port)).map(|_| "connected"),
    );
    say(
        "connect 127.0.0.1",
        TcpStream::connect(("127.0.0.1", port)).map(|_| "connected"),
    );
    say("udp to 192.0.2.1", send_datagram());
    for name in ["allowed.example", "refused.example"] {
        println!("await lookup {name}: {}", await_look_up_by_rule(name).await);
    }
    println!(
        "await lookup app.localhost: {}",
        await_look_up("app.localhost").await
    );
    println!("await echo 127.0.0.1: {}", await_echo(port).await);
    match TcpStream::connect(("127.0.0.1", port)) {
        Ok(held) => hold_while_the_switch_goes_off(held, port).await,
        Err(error) => println!("hold: {:?}", error.kind()),
    }
}

/// Prints whether `name` answered addresses, or why not.
fn look_up(name: &str) {
    let found = (name, 80).to_socket_addrs().map(|_| "found");
    say(&format!("lookup {name}"), found);
}

/// Prints whether the lookup of `name` was refused; any other answer is the system's.
fn look_up_by_rule(name: &str) {
    let looked_up = match (name, 80).to_socket_addrs() {
        Err(error) if error.kind() == ErrorKind::PermissionDenied => Err(error),
        _ => Ok("not refused"),
    };
    say(&format!("lookup {name}"), looked_up);
}

/// Sends a datagram to an address of the internet; any answer but a refusal is the system's.
fn send_datagram() -> io::Result<&'static str> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    match socket.send_to(b"hello", DOCUMENTATION_ADDRESS) {
        Err(error) if error.kind() == ErrorKind::PermissionDenied => Err(error),
        _ => Ok("not refused"),
    }
}

/// Whether `name` answered addresses through WASI 0.3's lookup, or why not.
async fn await_look_up(name: &str) -> String {
    match ip_name_lookup::resolve_addresses(name.to_owned()).await {
        Ok(_) => "found".to_owned(),
        Err(error) => format!("{error:?}"),
    }
}

/// Whether the lookup of `name` through WASI 0.3's lookup was refused; any other answer is the
/// system's.
async fn await_look_up_by_rule(name: &str) -> String {
    match ip_name_lookup::resolve_addresses(name.to_owned()).await {
        Err(error @ ip_name_lookup::ErrorCode::AccessDenied) => format!("{error:?}"),
        _ => "not refused".to_owned(),
    }
}

/// Connects to the echo at `port` on this computer through WASI 0.3's sockets, and answers the
/// line it echoes, or why it could not.
async fn await_echo(port: u16) -> String {
    match echoed_through_wasi_sockets(port).await {
        Ok(line) => line,
        Err(error) => format!("{error:?}"),
    }
}

/// The line the echo at `port` answers to `ping`, over a connection of WASI 0.3's sockets.
async fn echoed_through_wasi_sockets(port: u16) -> Result<String, ErrorCode> {
    let socket = TcpSocket::create(IpAddressFamily::Ipv4)?;
    let address = IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port,
        address: (127, 0, 0, 1),
    });
    socket.connect(address).await?;
    let (mut writer, sent) = wit_stream::new::<u8>();
    let _sending = socket.send(sent);
    writer.write_all(b"ping\n".to_vec()).await;
    let (mut received, _receiving) = socket.receive();
    let mut line = Vec::new();
    while let Some(byte) = received.next().await {
        if byte == b'\n' {
            break;
        }
        line.push(byte);
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

/// Holds `held` while new connections are tried until one is refused, then says the held one
/// still echoes.
async fn hold_while_the_switch_goes_off(held: TcpStream, port: u16) {
    println!("hold: connected");
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Err(error) if error.kind() == ErrorKind::PermissionDenied => break,
            _ => monotonic_clock::wait_for(TRY_EVERY).await,
        }
    }
    println!("new connection: refused");
    let echoed = echo(&held, "ping");
    say("held connection", echoed);
}

/// Sends `line` on `stream`, and answers the line that comes back.
fn echo(mut stream: &TcpStream, line: &str) -> io::Result<String> {
    writeln!(stream, "{line}")?;
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer)?;
    Ok(answer.trim_end().to_owned())
}

/// Prints `what: result`, an error by its kind.
fn say(what: &str, result: io::Result<impl std::fmt::Display>) {
    match result {
        Ok(value) => println!("{what}: {value}"),
        Err(error) => println!("{what}: {:?}", error.kind()),
    }
}
