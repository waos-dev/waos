//! The messages over the transport on this machine, a Unix socket or a named pipe, after the two
//! sides said their hellos; a client of another protocol major named with both builds; a second
//! WaOS refused while the first listens; the connection closing seen as none; and nobody
//! answering once the listener is dropped. What only a socket file has is in `socket.rs`.

use std::io::ErrorKind;
use std::path::Path;

use waos_dev_protocol::{
    Asked, CHUNK_BYTES, ClientStream, Connection, Hello, Input, InputAnswer, InputDone,
    InspectAnswer, Listener, MAJOR, MAX_FRAME_BYTES, Mismatch, OpenAnswer, OutputStream, Question,
    Request, ResizeAnswer, Resized, Run, RunEnd, RunEvent, ScreenshotAnswer, ServerStream,
    SettingRow, SettingsAnswer, TestPlace, Upload, WindowId, WindowPicture, component_digest,
};

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

fn cargo_waos_hello() -> Hello {
    Hello::cargo_waos("cargo-waos 0.3.0")
}

fn waos_hello() -> Hello {
    Hello::waos("WaOS 0.3.0")
}

/// A listener at `path` and a connection to it, with the listener's end of that connection,
/// the two sides having said their hellos.
async fn connected(path: &Path) -> (Listener, Connection<ClientStream>, Connection<ServerStream>) {
    let mut listener = Listener::bind(path).await.unwrap();
    let (client, server) = tokio::join!(Connection::connect(path), listener.accept());
    let (mut client, mut server) = (client.unwrap(), server.unwrap());
    let (ours, theirs) = (cargo_waos_hello(), waos_hello());
    let (theirs, theirs_too) = tokio::join!(client.hello(&ours), server.accept_hello(&theirs));
    theirs.unwrap();
    theirs_too.unwrap();
    (listener, client, server)
}

/// A `cargo waos` of another build, one whose hello says protocol `MAJOR + 1`: the two exchange
/// hellos, and the mismatch names both builds.
#[tokio::test]
async fn a_client_of_another_major_is_named_with_both_builds() {
    let place = TestPlace::new("other-major");
    let mut listener = Listener::bind(place.path()).await.unwrap();
    let (client, server) = tokio::join!(raw_client(place.path()), listener.accept());
    let (mut client, mut server) = (Connection::new(client), server.unwrap());
    let mut newer = cargo_waos_hello();
    newer.major = MAJOR + 1;
    newer.build = "cargo-waos 0.4.0".to_owned();
    let ours = waos_hello();
    let (theirs, theirs_too) = tokio::join!(client.hello(&newer), server.accept_hello(&ours));
    let theirs = theirs.unwrap();
    assert_eq!(theirs.major, MAJOR);
    assert_eq!(theirs_too.unwrap(), newer);
    let mismatch = theirs.accepts(&newer, &screenshot()).unwrap_err();
    assert!(matches!(mismatch, Mismatch::Major { .. }));
    assert!(
        mismatch.to_string().contains("cargo-waos 0.4.0")
            && mismatch.to_string().contains("WaOS 0.3.0"),
        "{mismatch}"
    );
    drop(server);
    assert_eq!(
        client.receive::<Request>().await.unwrap(),
        None,
        "WaOS closed"
    );
}

fn screenshot() -> Request {
    Request::Screenshot {
        id: "dev.example.notes".parse().unwrap(),
        window: None,
    }
}

fn every_request() -> Vec<Request> {
    let id: waos_package::ProgramId = "dev.example.triangle".parse().unwrap();
    vec![
        Request::Run(Run {
            id: id.clone(),
            args: vec!["--flag".to_owned()],
            variables: vec![("RUST_LOG".to_owned(), "debug".to_owned())],
            length: 3 << 20,
            digest: [0xab; 32],
        }),
        Request::Screenshot {
            id: id.clone(),
            window: Some(WindowId(2)),
        },
        Request::Input {
            id: id.clone(),
            window: Some(WindowId(1)),
            input: Input::Tap { x: 1.5, y: 2.5 },
        },
        Request::Input {
            id: id.clone(),
            window: None,
            input: Input::Key("cmd+shift+z".into()),
        },
        Request::Resize {
            id: id.clone(),
            window: Some(WindowId(1)),
            width: 390.0,
            height: 844.0,
        },
        Request::Settings { id: id.clone() },
        Request::Inspect {
            id: id.clone(),
            asked: waos_dev_protocol::Inspection::ActiveUses,
        },
        Request::Open {
            id: id.clone(),
            files: vec!["/Users/me/notes.md".into()],
        },
    ]
}

#[tokio::test]
async fn every_request_comes_back_as_sent() {
    let place = TestPlace::new("round-trip");
    let (_listener, mut client, mut server) = connected(place.path()).await;

    for request in every_request() {
        let (sent, received) = tokio::join!(client.send(&request), server.receive::<Request>());
        sent.unwrap();
        assert_eq!(received.unwrap(), Some(request));
    }
}

#[tokio::test]
async fn every_answer_comes_back_as_sent() {
    let place = TestPlace::new("answers");
    let (_listener, mut client, mut server) = connected(place.path()).await;

    let picture = WindowPicture {
        window: WindowId(1),
        title: "Notes".into(),
        width: 1600,
        height: 1200,
        in_sight: true,
        png: Some((0..=255).cycle().take(1 << 20).collect()),
        asking: Some(Asked::Question(Question::Camera)),
    };
    let screenshots = ScreenshotAnswer::Pictures(vec![picture]);
    let (sent, received) = tokio::join!(
        server.send(&screenshots),
        client.receive::<ScreenshotAnswer>()
    );
    sent.unwrap();
    assert_eq!(received.unwrap(), Some(screenshots));
    let input = tokio::join!(
        server.send(&InputAnswer::Done(InputDone {
            in_sight: false,
            new_picture: true,
        })),
        client.receive::<InputAnswer>()
    );
    input.0.unwrap();
    assert_eq!(
        input.1.unwrap(),
        Some(InputAnswer::Done(InputDone {
            in_sight: false,
            new_picture: true,
        }))
    );
    let resize = tokio::join!(
        server.send(&ResizeAnswer::Done(Resized {
            width: 780,
            height: 1689,
            scale: 2.0,
            new_picture: true,
        })),
        client.receive::<ResizeAnswer>()
    );
    resize.0.unwrap();
    let settings = SettingsAnswer::Rows(vec![SettingRow {
        key: "camera".into(),
        values: vec!["allow".into()],
        follows_global: false,
        offered: Vec::new(),
    }]);
    let (sent, received) = tokio::join!(server.send(&settings), client.receive::<SettingsAnswer>());
    sent.unwrap();
    assert_eq!(received.unwrap(), Some(settings));
    let inspected = InspectAnswer::Inspected(vec![r#"{"a":1}"#.into()]);
    let (sent, received) = tokio::join!(server.send(&inspected), client.receive::<InspectAnswer>());
    sent.unwrap();
    assert_eq!(received.unwrap(), Some(inspected));
    let opened = tokio::join!(
        server.send(&OpenAnswer::Opened),
        client.receive::<OpenAnswer>()
    );
    opened.0.unwrap();
    let have = tokio::join!(server.send(&Upload::Have), client.receive::<Upload>());
    have.0.unwrap();
    assert_eq!(have.1.unwrap(), Some(Upload::Have));
    let event = RunEvent::Output {
        stream: OutputStream::Stderr,
        bytes: b"a warning\n".to_vec(),
    };
    let (sent, received) = tokio::join!(server.send(&event), client.receive::<RunEvent>());
    sent.unwrap();
    assert_eq!(received.unwrap(), Some(event));
    let ended = RunEvent::Ended(RunEnd::Stopped);
    let (sent, received) = tokio::join!(server.send(&ended), client.receive::<RunEvent>());
    sent.unwrap();
    assert_eq!(received.unwrap(), Some(ended));
}

#[tokio::test]
async fn an_unknown_answer_from_a_newer_waos_reads_as_unknown_and_leaves_the_connection_whole() {
    #[derive(serde::Serialize)]
    #[serde(tag = "kind", content = "data")]
    enum NewerRunEvent {
        Progress { percent: u8 },
    }

    let place = TestPlace::new("newer-event");
    let (_listener, mut client, mut server) = connected(place.path()).await;
    let (sent, received) = tokio::join!(
        server.send(&NewerRunEvent::Progress { percent: 42 }),
        client.receive::<RunEvent>()
    );
    sent.unwrap();
    assert_eq!(received.unwrap(), Some(RunEvent::Unknown));
    let (sent, received) = tokio::join!(
        server.send(&RunEvent::Started),
        client.receive::<RunEvent>()
    );
    sent.unwrap();
    assert_eq!(received.unwrap(), Some(RunEvent::Started));
}

#[tokio::test]
async fn the_halves_send_while_they_watch_for_the_close() {
    let place = TestPlace::new("halves");
    let (_listener, mut client, server) = connected(place.path()).await;
    let (mut server_in, mut server_out) = server.into_split();

    let watching = tokio::spawn(async move { server_in.receive::<Request>().await });
    server_out.send(&RunEvent::Started).await.unwrap();
    assert_eq!(
        client.receive::<RunEvent>().await.unwrap(),
        Some(RunEvent::Started)
    );
    drop(client);
    assert_eq!(watching.await.unwrap().unwrap(), None, "the close is seen");
}

#[tokio::test]
async fn the_other_side_closing_is_none() {
    let place = TestPlace::new("closing");
    let (_listener, client, mut server) = connected(place.path()).await;
    drop(client);
    assert_eq!(server.receive::<Request>().await.unwrap(), None);
}

#[tokio::test]
async fn one_listener_serves_one_connection_after_another() {
    let place = TestPlace::new("one-after-another");
    let (mut listener, mut first, mut first_served) = connected(place.path()).await;
    let (second, second_served) =
        tokio::join!(Connection::connect(place.path()), listener.accept());
    let (mut second, mut second_served) = (second.unwrap(), second_served.unwrap());
    let (ours, theirs) = (cargo_waos_hello(), waos_hello());
    let (theirs, theirs_too) =
        tokio::join!(second.hello(&ours), second_served.accept_hello(&theirs));
    theirs.unwrap();
    theirs_too.unwrap();
    for (client, served) in [
        (&mut first, &mut first_served),
        (&mut second, &mut second_served),
    ] {
        let (sent, received) = tokio::join!(served.send(&OpenAnswer::Opened), client.receive());
        sent.unwrap();
        assert_eq!(received.unwrap(), Some(OpenAnswer::Opened));
    }
}

#[tokio::test]
async fn a_second_waos_is_refused_while_the_first_listens() {
    let place = TestPlace::new("second");
    let _first = Listener::bind(place.path()).await.unwrap();
    let refused = Listener::bind(place.path()).await.unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::AddrInUse);
}

#[tokio::test]
async fn nobody_answers_once_the_listener_is_dropped() {
    let place = TestPlace::new("dropped");
    drop(Listener::bind(place.path()).await.unwrap());
    assert_eq!(
        Connection::connect(place.path()).await.unwrap_err().kind(),
        ErrorKind::NotFound
    );
}

/// A run's component larger than one frame goes over the transport as parts and comes back
/// whole, checked by its length and digest: a debug build is hundreds of megabytes, over the
/// frame a connection accepts whole.
#[tokio::test]
async fn a_component_larger_than_one_frame_round_trips() {
    let place = TestPlace::new("big-component");
    let (_listener, mut client, mut server) = connected(place.path()).await;
    let id: waos_package::ProgramId = "dev.example.triangle".parse().unwrap();
    let length = (MAX_FRAME_BYTES + 1) as u64;
    let sending = tokio::spawn(async move {
        let component: Vec<u8> = (0..=255u8).cycle().take(MAX_FRAME_BYTES + 1).collect();
        let run = Run {
            id,
            args: Vec::new(),
            variables: Vec::new(),
            length: component.len() as u64,
            digest: component_digest(&component),
        };
        client.send(&Request::Run(run)).await.unwrap();
        client.send_component(&component).await.unwrap();
    });
    let Request::Run(run) = server.receive::<Request>().await.unwrap().expect("a run") else {
        panic!("a run");
    };
    assert_eq!(run.length, length);
    assert_eq!(run.length, CHUNK_BYTES as u64 * 32 + 1);
    let component = server
        .receive_component(run.length, &run.digest)
        .await
        .unwrap();
    assert_eq!(component.len() as u64, length);
    assert_eq!(component_digest(&component), run.digest, "the whole of it");
    let end = (MAX_FRAME_BYTES + 1) - 1;
    for at in [0, end / 2, end] {
        assert_eq!(component[at], (at % 256) as u8, "byte {at}");
    }
    sending.await.unwrap();
}
