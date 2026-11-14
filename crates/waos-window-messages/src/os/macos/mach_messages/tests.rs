use super::*;

const PROTOCOL: u32 = 7;

/// A port of the test's own, and a send right to it.
fn port() -> (ReceiveRight, SendRight) {
    let port = ReceiveRight::new().expect("a port");
    let right = port.make_send().expect("a send right");
    (port, right)
}

fn message(arrival: Arrival) -> Received {
    match arrival {
        Arrival::Message(received) => received,
        Arrival::NoSenders => panic!("a message, not the last sender gone"),
    }
}

#[test]
fn bytes_cross_with_the_senders_audit_token() {
    let (port, right) = port();
    right.send(PROTOCOL, b"hello", Vec::new()).unwrap();
    let received = message(port.receive(PROTOCOL).unwrap());
    assert_eq!(received.bytes, b"hello");
    assert!(received.ports.is_empty());
    assert_eq!(received.sender.pid(), std::process::id() as i32);
}

#[test]
fn a_port_carried_is_the_receivers_to_send_to() {
    let (first, to_first) = port();
    let (second, to_second) = port();
    to_first
        .send(PROTOCOL, b"reply here", vec![to_second])
        .unwrap();
    let mut received = message(first.receive(PROTOCOL).unwrap());
    assert_eq!(received.ports.len(), 1);
    let reply_to = received.ports.remove(0);
    reply_to.send(PROTOCOL, b"a reply", Vec::new()).unwrap();
    assert_eq!(message(second.receive(PROTOCOL).unwrap()).bytes, b"a reply");
}

#[test]
fn many_bytes_go_out_of_line_and_arrive_whole() {
    let (port, right) = port();
    let bytes: Vec<u8> = (0..INLINE_LIMIT * 3 + 5).map(|index| index as u8).collect();
    right.send(PROTOCOL, &bytes, Vec::new()).unwrap();
    assert_eq!(message(port.receive(PROTOCOL).unwrap()).bytes, bytes);
}

#[test]
fn bytes_of_no_whole_word_arrive_as_they_were() {
    let (port, right) = port();
    for length in 0..9 {
        let bytes: Vec<u8> = (0..length).map(|index| 200 + index as u8).collect();
        right.send(PROTOCOL, &bytes, Vec::new()).unwrap();
        assert_eq!(message(port.receive(PROTOCOL).unwrap()).bytes, bytes);
    }
}

#[test]
fn the_port_hears_when_its_last_sender_is_gone() {
    let (port, right) = port();
    port.notify_when_no_senders().unwrap();
    right.send(PROTOCOL, b"last words", Vec::new()).unwrap();
    drop(right);
    assert_eq!(
        message(port.receive(PROTOCOL).unwrap()).bytes,
        b"last words"
    );
    assert!(matches!(
        port.receive(PROTOCOL).unwrap(),
        Arrival::NoSenders
    ));
}

#[test]
fn a_message_of_another_protocol_is_refused() {
    let (port, right) = port();
    let (_, carried) = self::port();
    right
        .send(PROTOCOL + 1, b"elsewhere", vec![carried])
        .unwrap();
    let refused = port.receive(PROTOCOL).unwrap_err();
    assert_eq!(refused.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn a_full_queue_fails_at_once_rather_than_waiting() {
    let (_port, right) = port();
    let sent = (0..2000)
        .map(|_| right.send(PROTOCOL, b"x", Vec::new()))
        .position(|sent| sent.is_err());
    assert!(sent.is_some(), "the queue has a limit");
}

#[test]
fn a_name_checked_in_is_found_by_looking_it_up() {
    let name = format!("dev.waos.tests.window-messages.{}", std::process::id());
    let port = ReceiveRight::check_in(&name).unwrap();
    let right = SendRight::look_up(&name).unwrap();
    right.send(PROTOCOL, b"found", Vec::new()).unwrap();
    assert_eq!(message(port.receive(PROTOCOL).unwrap()).bytes, b"found");
}
