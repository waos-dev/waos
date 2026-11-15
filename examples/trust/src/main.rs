//! A program that asks the system whether it trusts a server's certificate chain, and what a
//! reader learns `trust` from: it checks the chain its test gave it, a self-signed one, which no
//! system trusts, then a chain with no certificate, and then requests a page over HTTPS from a
//! server presenting that chain, which WaOS refuses with the same check.
//!
//! It prints each result as `what: result`. It expects its test (`crates/waos/tests/all/trust.rs`)
//! to write the chain's one certificate, for `trusted.localhost`, in `/data/chain.der`, and to
//! serve it over TLS on this computer, at the port written in `/data/port`.
//!
//! ```text
//! cargo build -p trust --target wasm32-wasip3
//! ```

use std::fs;

use wasdk::http::client;
use wasdk::http::types::{Fields, Request, Scheme};
use wasdk::trust::{self, TrustError};
use wasdk::wit_future;

/// The name the chain is for.
const SERVER: &str = "trusted.localhost";

#[wasdk::main]
async fn main() {
    let chain = vec![fs::read("/data/chain.der").unwrap()];
    let port: u16 = fs::read_to_string("/data/port")
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let Some(trust) = trust::get_trust() else {
        println!("trust: none on this system");
        return;
    };
    let checked = trust.verify_server(chain, SERVER.to_owned()).await;
    println!("verify the chain: {}", said(checked));
    let checked = trust.verify_server(Vec::new(), SERVER.to_owned()).await;
    println!("verify no chain: {}", said(checked));
    println!(
        "get over https: {}",
        get_over_https(&format!("{SERVER}:{port}")).await
    );
}

/// What a check answered, in a word: the error's text is the system's.
fn said(checked: Result<(), TrustError>) -> &'static str {
    match checked {
        Ok(()) => "trusted",
        Err(TrustError::Untrusted(_)) => "untrusted",
        Err(TrustError::Failed(_)) => "failed",
    }
}

/// Asks `authority` for `/` over HTTPS, and answers its status or the error WASI answers.
async fn get_over_https(authority: &str) -> String {
    let (_no_trailers, trailers) = wit_future::new(|| Ok(None));
    let (request, _sent) = Request::new(Fields::new(), None, trailers, None);
    request.set_scheme(Some(&Scheme::Https)).unwrap();
    request.set_authority(Some(authority)).unwrap();
    request.set_path_with_query(Some("/")).unwrap();
    match client::send(request).await {
        Ok(response) => response.get_status_code().to_string(),
        Err(error) => format!("{error:?}"),
    }
}
