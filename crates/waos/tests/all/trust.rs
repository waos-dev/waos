//! `examples/trust` with a self-signed chain made for the test, served over TLS on this computer:
//! the system's checker does not trust it, an empty chain fails to be checked, and a request
//! over HTTPS to the server presenting it is refused with `TLS-certificate-error`, the same check
//! made on the program's HTTP. A chain the system trusts is not reached here: no server of the
//! internet is, and no root is added to this machine's (`src/trust/tests.rs` checks a chain up to
//! a root of the test's own instead).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

use crate::reaching::{Run, TestGrants, any};

#[test]
fn a_self_signed_chain_is_untrusted_by_the_check_and_by_https() {
    let certified =
        rcgen::generate_simple_self_signed(vec!["trusted.localhost".to_owned()]).unwrap();
    let chain = certified.cert.der().clone();
    let key = PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
    let port = serve_tls(chain.clone(), key.into());
    let data = [
        ("chain.der", chain.to_vec()),
        ("port", port.to_string().into_bytes()),
    ];
    let grants = TestGrants::new(any(), true);
    let run = Run::start(waos_test_programs::TRUST, &grants, &data);
    run.returned();
    assert_eq!(
        run.lines.all(),
        [
            "verify the chain: untrusted",
            "verify no chain: failed",
            "get over https: ErrorCode::TlsCertificateError",
        ]
    );
}

/// A server on this computer presenting `chain` over TLS, at the port it answers; a client that
/// trusts it is answered `200`.
fn serve_tls(chain: CertificateDer<'static>, key: PrivateKeyDer<'static>) -> u16 {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![chain], key)
        .unwrap();
    let config = Arc::new(config);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let connection = ServerConnection::new(Arc::clone(&config)).unwrap();
            let mut tls = StreamOwned::new(connection, stream);
            let mut request = [0; 1024];
            if tls.read(&mut request).is_ok() {
                let _ = tls.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
            }
        }
    });
    port
}
