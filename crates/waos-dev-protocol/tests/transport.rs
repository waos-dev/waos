//! The messages over the transport on this machine, a Unix socket or a named pipe, after the two
//! sides agreed on the protocol; a client of another build refused; a second WaOS refused while
//! the first listens; the connection closing seen as none; and nobody answering once the listener
//! is dropped. What only a socket file has is in `socket.rs`.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use waos_dev_protocol::{
    Asked, ClientStream, Connection, FromWaOS, Input, Listener, MAX_FRAME_BYTES, OutputStream,
    PROTOCOL, Question, RunEnd, ServerStream, ToWaOS, Unreached, WindowPicture, component_digest,
};

/// Where this test's WaOS listens, apart from every other test's and from the user's own WaOS: a
/// socket in a folder of the test's own, removed with it, or a pipe of the test's own name.
struct TestPlace {
    path: PathBuf,
}

impl TestPlace {
    #[cfg(unix)]
    fn new(test: &str) -> TestPlace {
        let folder = std::env::temp_dir().join(format!("waos-dev-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        TestPlace {
            path: folder.join("dev.sock"),
        }
    }

    #[cfg(windows)]
    fn new(test: &str) -> TestPlace {
        TestPlace {
            path: PathBuf::from(format!(
                r"\\.\pipe\waos-dev-test-{}-{test}",
                std::process::id()
            )),
        }
    }
}

#[cfg(unix)]
impl Drop for TestPlace {
    fn drop(&mut self) {
        if let Some(folder) = self.path.parent() {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}

/// A stream to the WaOS at `path` with nothing said on it, as a client of another build opens it.
#[cfg(unix)]
async fn raw_client(path: &Path) -> ClientStream {
    tokio::net::UnixStream::connect(path).await.unwrap()
}

#[cfg(windows)]
async fn raw_client(path: &Path) -> ClientStream {
    tokio::net::windows::named_pipe::ClientOptions::new()
        .open(path)
        .unwrap()
}

/// A listener at `path` and a connection to it, with the listener's end of that connection,
/// the two sides agreed on the protocol.
async fn connected(path: &Path) -> (Listener, Connection<ClientStream>, Connection<ServerStream>) {
    let mut listener = Listener::bind(path).await.unwrap();
    let (client, server) = tokio::join!(Connection::connect(path), listener.accept());
    let (mut client, mut server) = (client.unwrap(), server.unwrap());
    let (agreed, agreed_too) = tokio::join!(client.agree_on_protocol(), server.agree_on_protocol());
    agreed.unwrap();
    agreed_too.unwrap();
    (listener, client, server)
}

/// A client built from another waos-dev-protocol, one that speaks protocol `PROTOCOL + 1`: it sends its
/// number and gets WaOS's back, the one frame every build reads, and WaOS refuses it naming
/// both numbers and closes.
#[tokio::test]
async fn a_client_of_another_build_is_refused_with_both_numbers() {
    let place = TestPlace::new("other-build");
    let mut listener = Listener::bind(&place.path).await.unwrap();
    let (client, server) = tokio::join!(raw_client(&place.path), listener.accept());
    let (mut client, mut server) = (client, server.unwrap());
    let theirs = PROTOCOL + 1;
    let mut their_frame = 4u32.to_le_bytes().to_vec();
    their_frame.extend_from_slice(&theirs.to_le_bytes());
    client.write_all(&their_frame).await.unwrap();

    let refused = server.agree_on_protocol().await.unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::Unsupported);
    assert!(
        refused.to_string().contains(&format!("protocol {theirs}"))
            && refused
                .to_string()
                .contains(&format!("protocol {PROTOCOL}")),
        "{refused}"
    );
    drop(server);

    let mut ours = [0; 8];
    client.read_exact(&mut ours).await.unwrap();
    assert_eq!(&ours[..4], 4u32.to_le_bytes());
    assert_eq!(u32::from_le_bytes(ours[4..].try_into().unwrap()), PROTOCOL);
    assert_eq!(read_after_close(&mut client).await, 0, "WaOS closed");
}

/// What a read gets once the other side closed: nothing, which a pipe says as a broken pipe.
async fn read_after_close(client: &mut ClientStream) -> usize {
    match client.read(&mut [0; 1]).await {
        Ok(read) => read,
        Err(error) if error.kind() == ErrorKind::BrokenPipe => 0,
        Err(error) => panic!("{error}"),
    }
}

fn every_from_waos() -> Vec<FromWaOS> {
    vec![
        FromWaOS::Started,
        FromWaOS::Output {
            stream: OutputStream::Stdout,
            bytes: b"hello, ".to_vec(),
        },
        FromWaOS::Output {
            stream: OutputStream::Stderr,
            bytes: b"a warning\n".to_vec(),
        },
        FromWaOS::Ended(RunEnd::Returned { success: true }),
        FromWaOS::Ended(RunEnd::Returned { success: false }),
        FromWaOS::Ended(RunEnd::Failed("unreachable".into())),
        FromWaOS::Ended(RunEnd::Stopped),
        FromWaOS::Ended(RunEnd::Replaced),
        FromWaOS::Refused("not a component".into()),
        FromWaOS::Opened,
        FromWaOS::Screenshots(vec![
            WindowPicture {
                window: 1,
                title: "Notes".into(),
                width: 1600,
                height: 1200,
                in_sight: true,
                png: Some((0..=255).cycle().take(1 << 20).collect()),
                asking: None,
            },
            WindowPicture {
                window: 2,
                title: String::new(),
                width: 800,
                height: 600,
                in_sight: false,
                png: None,
                asking: Some(Asked::Question(Question::Camera)),
            },
        ]),
        FromWaOS::InputDone {
            in_sight: true,
            new_picture: true,
        },
        FromWaOS::InputDone {
            in_sight: false,
            new_picture: false,
        },
        FromWaOS::Unreached(Unreached::NotRunByCargoWaos),
        FromWaOS::Unreached(Unreached::NoWindow {
            window: 3,
            windows: 1,
        }),
        FromWaOS::Unreached(Unreached::OutsideWindow {
            width: 1600,
            height: 1200,
            x: 1600.0,
            y: 0.0,
            scale: 2.0,
        }),
        FromWaOS::Unreached(Unreached::NotEditing),
        FromWaOS::Unreached(Unreached::UnknownKey("hyper+q".into())),
    ]
}

fn every_to_waos() -> Vec<ToWaOS> {
    let id: waos_package::ProgramId = "dev.example.triangle".parse().unwrap();
    vec![
        ToWaOS::Run {
            id: id.clone(),
            length: 3 << 20,
            digest: [0xab; 32],
            variables: vec![("RUST_LOG".to_owned(), "debug".to_owned())],
        },
        ToWaOS::Component {
            bytes: (0..=255).cycle().take(1 << 16).collect(),
        },
        ToWaOS::Open {
            id: id.clone(),
            files: vec!["/Users/me/notes.md".into()],
        },
        ToWaOS::Screenshot { id: id.clone() },
        ToWaOS::Input {
            id: id.clone(),
            window: 1,
            input: Input::Tap { x: 1.5, y: 2.5 },
        },
        ToWaOS::Input {
            id: id.clone(),
            window: 1,
            input: Input::Text("héllo".into()),
        },
        ToWaOS::Input {
            id: id.clone(),
            window: 2,
            input: Input::Key("cmd+shift+z".into()),
        },
        ToWaOS::Input {
            id,
            window: 1,
            input: Input::Scroll {
                x: 3.0,
                y: 4.0,
                dx: -5.0,
                dy: 6.0,
            },
        },
    ]
}

#[tokio::test]
async fn every_message_comes_back_as_sent() {
    let place = TestPlace::new("round-trip");
    let (_listener, mut client, mut server) = connected(&place.path).await;

    for message in every_to_waos() {
        let (sent, received) = tokio::join!(client.send(&message), server.receive::<ToWaOS>());
        sent.unwrap();
        assert_eq!(received.unwrap(), Some(message));
    }

    for message in every_from_waos() {
        let (sent, received) = tokio::join!(server.send(&message), client.receive::<FromWaOS>());
        sent.unwrap();
        assert_eq!(received.unwrap(), Some(message));
    }
}

/// A run's component larger than one frame goes over the transport as parts and comes back
/// whole, checked by its length and digest: a debug build is hundreds of megabytes, over the
/// frame a connection accepts whole.
#[tokio::test]
async fn a_component_larger_than_one_frame_round_trips() {
    let place = TestPlace::new("big-component");
    let (_listener, mut client, mut server) = connected(&place.path).await;
    let id: waos_package::ProgramId = "dev.example.triangle".parse().unwrap();
    let length = (MAX_FRAME_BYTES + 1) as u64;
    let sending = tokio::spawn(async move {
        let component: Vec<u8> = (0..=255u8).cycle().take(MAX_FRAME_BYTES + 1).collect();
        client.send_run(&id, &[], &component).await.unwrap();
    });
    let ToWaOS::Run {
        length: announced,
        digest,
        ..
    } = server.receive::<ToWaOS>().await.unwrap().expect("a run")
    else {
        panic!("a run");
    };
    assert_eq!(announced, length);
    let component = server.receive_component(announced, &digest).await.unwrap();
    assert_eq!(component.len() as u64, length);
    assert_eq!(component_digest(&component), digest, "the whole of it");
    // The pattern the component was made of, where the parts meet and at its end.
    let end = (MAX_FRAME_BYTES + 1) - 1;
    for at in [0, end / 2, end] {
        assert_eq!(component[at], (at % 256) as u8, "byte {at}");
    }
    sending.await.unwrap();
}

#[tokio::test]
async fn the_halves_send_while_they_watch_for_the_close() {
    let place = TestPlace::new("halves");
    let (_listener, mut client, server) = connected(&place.path).await;
    let (mut server_in, mut server_out) = server.into_split();

    let watching = tokio::spawn(async move { server_in.receive::<ToWaOS>().await });
    server_out.send(&FromWaOS::Started).await.unwrap();
    assert_eq!(
        client.receive::<FromWaOS>().await.unwrap(),
        Some(FromWaOS::Started)
    );
    drop(client);
    assert_eq!(watching.await.unwrap().unwrap(), None, "the close is seen");
}

#[tokio::test]
async fn the_other_side_closing_is_none() {
    let place = TestPlace::new("closing");
    let (_listener, client, mut server) = connected(&place.path).await;
    drop(client);
    assert_eq!(server.receive::<ToWaOS>().await.unwrap(), None);
}

#[tokio::test]
async fn one_listener_serves_one_connection_after_another() {
    let place = TestPlace::new("one-after-another");
    let (mut listener, mut first, mut first_served) = connected(&place.path).await;
    let (second, second_served) = tokio::join!(Connection::connect(&place.path), listener.accept());
    let (mut second, mut second_served) = (second.unwrap(), second_served.unwrap());
    let (agreed, agreed_too) = tokio::join!(
        second.agree_on_protocol(),
        second_served.agree_on_protocol()
    );
    agreed.unwrap();
    agreed_too.unwrap();
    for (client, served) in [
        (&mut first, &mut first_served),
        (&mut second, &mut second_served),
    ] {
        let (sent, received) = tokio::join!(served.send(&FromWaOS::Opened), client.receive());
        sent.unwrap();
        assert_eq!(received.unwrap(), Some(FromWaOS::Opened));
    }
}

#[tokio::test]
async fn a_second_waos_is_refused_while_the_first_listens() {
    let place = TestPlace::new("second");
    let _first = Listener::bind(&place.path).await.unwrap();
    let refused = Listener::bind(&place.path).await.unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::AddrInUse);
}

#[tokio::test]
async fn nobody_answers_once_the_listener_is_dropped() {
    let place = TestPlace::new("dropped");
    drop(Listener::bind(&place.path).await.unwrap());
    assert_eq!(
        Connection::connect(&place.path).await.unwrap_err().kind(),
        ErrorKind::NotFound
    );
}
