use std::net::Ipv4Addr;

use super::*;
use crate::hello::Hello;

/// A listener on this machine, at a port the system picks, with a new identity.
async fn listener() -> (NetworkListener, Identity) {
    let identity = Identity::make().unwrap();
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let listener = NetworkListener::bind_at(&identity, address).await.unwrap();
    (listener, identity)
}

fn address(listener: &NetworkListener, secure: bool) -> DeviceAddress {
    DeviceAddress {
        host: "127.0.0.1".to_owned(),
        port: listener.port(),
        secure,
    }
}

/// The server's side of one connection: its handshakes and its hello, then the one message it
/// echoes back.
async fn echo_once(mut listener: NetworkListener) -> io::Result<()> {
    let mut connection = listener.accept().await?.handshake().await?;
    assert_eq!(connection.locality(), Locality::Network);
    connection.accept_hello(&Hello::waos("WaOS 0.7.1")).await?;
    let message: Option<Vec<u8>> = connection.receive().await?;
    connection.send(&message).await
}

#[tokio::test]
async fn a_message_goes_over_wss_to_the_certificate_pinned_and_back() {
    let (listener, identity) = listener().await;
    let address = address(&listener, true);
    let server = tokio::spawn(echo_once(listener));
    let (mut client, seen) = connect(&address, Some(identity.fingerprint()))
        .await
        .unwrap();
    assert_eq!(seen, Some(identity.fingerprint()));
    client.hello(&Hello::command("waos 0.7.1")).await.unwrap();
    client.send(&vec![1u8, 2, 3]).await.unwrap();
    let back: Option<Option<Vec<u8>>> = client.receive().await.unwrap();
    assert_eq!(back, Some(Some(vec![1, 2, 3])));
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_certificate_other_than_the_one_pinned_is_refused() {
    let (listener, _identity) = listener().await;
    let address = address(&listener, true);
    tokio::spawn(echo_once(listener));
    let other = Identity::make().unwrap().fingerprint();
    let error = connect(&address, Some(other)).await.unwrap_err();
    assert!(
        error.to_string().contains("not the one paired with"),
        "{error}"
    );
}

#[tokio::test]
async fn with_nothing_pinned_the_certificate_is_taken_and_its_fingerprint_answered() {
    let (listener, identity) = listener().await;
    let address = address(&listener, true);
    tokio::spawn(echo_once(listener));
    let (_client, seen) = connect(&address, None).await.unwrap();
    assert_eq!(seen, Some(identity.fingerprint()));
}

#[tokio::test]
async fn ws_from_this_machine_is_taken() {
    let (listener, _identity) = listener().await;
    let address = address(&listener, false);
    let server = tokio::spawn(echo_once(listener));
    let (mut client, seen) = connect(&address, None).await.unwrap();
    assert_eq!(seen, None);
    client.hello(&Hello::command("waos 0.7.1")).await.unwrap();
    client.send(&vec![9u8]).await.unwrap();
    let back: Option<Option<Vec<u8>>> = client.receive().await.unwrap();
    assert_eq!(back, Some(Some(vec![9])));
    server.await.unwrap().unwrap();
}

#[test]
fn ws_from_elsewhere_is_answered_forbidden_naming_wss() {
    let refused = plain_refused();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert!(refused.body().as_deref().unwrap().contains("wss://"));
}

#[test]
fn an_address_is_wss_on_the_default_port_unless_it_says_otherwise() {
    let parsed = |text: &str| text.parse::<DeviceAddress>().unwrap();
    assert_eq!(
        parsed("192.168.1.23").to_string(),
        "wss://192.168.1.23:7351"
    );
    assert_eq!(
        parsed("phone.local:7400").to_string(),
        "wss://phone.local:7400"
    );
    assert_eq!(
        parsed("ws://127.0.0.1:9000/").to_string(),
        "ws://127.0.0.1:9000"
    );
    assert_eq!(parsed("[fe80::1]:7352").to_string(), "wss://[fe80::1]:7352");
    assert_eq!(parsed("fe80::1").to_string(), "wss://[fe80::1]:7351");
    assert!("http://phone".parse::<DeviceAddress>().is_err());
    assert!("phone:port".parse::<DeviceAddress>().is_err());
}

#[test]
fn a_fingerprint_reads_back_as_it_is_written() {
    let fingerprint = Identity::make().unwrap().fingerprint();
    assert_eq!(
        fingerprint.to_string().parse::<Fingerprint>(),
        Ok(fingerprint)
    );
}
