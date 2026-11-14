//! `examples/network` under grants of each test's own, against an echo server on this computer,
//! through `std::net`'s sockets, WASI 0.2's, and through WASI 0.3's, which answer alike: a name the
//! domain rule refuses is not looked up, a bare address of the internet is refused under a rule of
//! only some domains, the network switch does not reach this computer, by name or by address, and
//! the local-network switch does with the network switch off, until it is turned off, when a new
//! connection is refused and the one held carries on. Names under `localhost` are this computer,
//! answered by WaOS on every system; the rule's names are under `example`, which no one serves,
//! so no test needs a resolver of its own or the internet.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use crate::reaching::{Run, TestGrants, any, off, only, port_of};

#[test]
fn a_name_the_rule_refuses_and_a_bare_address_under_only_some_are_refused() {
    let grants = TestGrants::new(only(&["allowed.example"]), false);
    let run = Run::start(waos_test_programs::NETWORK, &grants, &port_of(serve_echo()));
    run.returned();
    assert_eq!(
        run.lines.all(),
        [
            "lookup allowed.example: not refused",
            "lookup refused.example: PermissionDenied",
            "lookup app.localhost: PermissionDenied",
            "connect localhost: PermissionDenied",
            "connect 127.0.0.1: PermissionDenied",
            "udp to 192.0.2.1: PermissionDenied",
            "await lookup allowed.example: not refused",
            "await lookup refused.example: ErrorCode::AccessDenied",
            "await lookup app.localhost: ErrorCode::AccessDenied",
            "await echo 127.0.0.1: ErrorCode::AccessDenied",
            "hold: PermissionDenied",
        ]
    );
}

#[test]
fn the_network_switch_reaches_the_internet_but_not_this_computer() {
    let grants = TestGrants::new(any(), false);
    let run = Run::start(waos_test_programs::NETWORK, &grants, &port_of(serve_echo()));
    run.returned();
    assert_eq!(
        run.lines.all(),
        [
            "lookup allowed.example: not refused",
            "lookup refused.example: not refused",
            "lookup app.localhost: PermissionDenied",
            "connect localhost: PermissionDenied",
            "connect 127.0.0.1: PermissionDenied",
            "udp to 192.0.2.1: not refused",
            "await lookup allowed.example: not refused",
            "await lookup refused.example: not refused",
            "await lookup app.localhost: ErrorCode::AccessDenied",
            "await echo 127.0.0.1: ErrorCode::AccessDenied",
            "hold: PermissionDenied",
        ]
    );
}

#[test]
fn the_local_network_switch_reaches_this_computer_until_it_is_turned_off() {
    let grants = TestGrants::new(off(), true);
    let run = Run::start(waos_test_programs::NETWORK, &grants, &port_of(serve_echo()));
    run.lines.wait_for("hold: connected");
    grants.set_local_network(false);
    run.returned();
    assert_eq!(
        run.lines.all(),
        [
            "lookup allowed.example: PermissionDenied",
            "lookup refused.example: PermissionDenied",
            "lookup app.localhost: found",
            "connect localhost: connected",
            "connect 127.0.0.1: connected",
            "udp to 192.0.2.1: PermissionDenied",
            "await lookup allowed.example: ErrorCode::AccessDenied",
            "await lookup refused.example: ErrorCode::AccessDenied",
            "await lookup app.localhost: found",
            "await echo 127.0.0.1: ping",
            "hold: connected",
            "new connection: refused",
            "held connection: ping",
        ]
    );
}

/// An echo of every line on this computer, at the port it answers.
fn serve_echo() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            thread::spawn(move || echo(stream));
        }
    });
    port
}

fn echo(stream: TcpStream) {
    let mut writer = stream.try_clone().unwrap();
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        if writeln!(writer, "{line}").is_err() {
            return;
        }
    }
}
