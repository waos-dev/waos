//! What only a Unix socket has, a file: a stale one a WaOS that died left is replaced, a second
//! WaOS's probe of the first is a connection that asks nothing, and a listener dropped leaves no
//! file. The rest of the transport's behaviour is in `transport.rs`.

#![cfg(unix)]

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use waos_dev_protocol::{Connection, FromWaOS, Listener, ToWaOS};

/// A folder of this test's own, with its socket's path inside; removed with the folder.
struct TestFolder(PathBuf);

impl TestFolder {
    fn new(test: &str) -> TestFolder {
        let folder =
            std::env::temp_dir().join(format!("waos-dev-socket-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        TestFolder(folder)
    }

    fn socket(&self) -> PathBuf {
        self.0.join("dev.sock")
    }
}

impl Drop for TestFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn a_socket_file_a_dead_waos_left_is_replaced() {
    let folder = TestFolder::new("stale");
    let socket = folder.socket();
    fs::create_dir_all(&folder.0).unwrap();
    // Bound and dropped, as a WaOS that died leaves it: the file stays, nobody answers.
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    assert!(socket.exists());

    let mut listener = Listener::bind(&socket).await.unwrap();
    let (client, server) = tokio::join!(Connection::connect(&socket), listener.accept());
    let (mut client, mut server) = (client.unwrap(), server.unwrap());
    let (sent, received) = tokio::join!(server.send(&FromWaOS::Started), client.receive());
    sent.unwrap();
    assert_eq!(received.unwrap(), Some(FromWaOS::Started));
}

#[tokio::test]
async fn a_second_waos_probes_the_first_with_a_connection_that_asks_nothing() {
    let folder = TestFolder::new("second");
    let socket = folder.socket();
    let mut first = Listener::bind(&socket).await.unwrap();
    let (refused, probe) = tokio::join!(Listener::bind(&socket), first.accept());
    assert_eq!(refused.unwrap_err().kind(), ErrorKind::AddrInUse);
    let mut probe = probe.unwrap();
    assert_eq!(
        probe.receive::<ToWaOS>().await.unwrap(),
        None,
        "the probe is a connection that asked nothing"
    );
}

#[tokio::test]
async fn a_listener_dropped_leaves_no_socket_file() {
    let folder = TestFolder::new("dropped");
    let socket = folder.socket();
    drop(Listener::bind(&socket).await.unwrap());
    assert!(!socket.exists());
}
