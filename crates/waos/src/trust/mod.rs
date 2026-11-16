//! `wasdk:platform/trust`: the system's check of a server's certificate chain, for a program
//! that brings its own TLS, made with `rustls-platform-verifier`, which asks the system's own
//! checker: its roots, those the user or an organization added, revocation and its rules. HTTPS
//! through `wasi:http` is checked the same way, with the same verifier (`network::http`). The
//! interface is served to a program by [`host`].
//!
//! The invariant: a chain is trusted only by the system's answer. Nothing here adds a root, so
//! a program trusts what the machine's own apps trust, and no more.

pub(crate) mod host;

use std::sync::Arc;

use rustls::CertificateError;
use rustls::client::danger::ServerCertVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls_platform_verifier::Verifier;

use crate::bindings::wasdk::platform::trust::TrustError;

/// The system's checker, as a program's `trust` handle holds it.
pub struct Trust {
    verifier: Arc<Verifier>,
}

impl Trust {
    /// The system's checker; none where the system has none the verifier can reach.
    pub(crate) fn system() -> Option<Trust> {
        match Verifier::new(provider()) {
            Ok(verifier) => Some(Trust {
                verifier: Arc::new(verifier),
            }),
            Err(error) => {
                tracing::warn!(%error, "the system's certificate checker is not there");
                None
            }
        }
    }

    /// Whether the system trusts `chain`, leaf first, for `server_name`. Blocks while the system
    /// checks, which may ask the network whether a certificate was revoked.
    pub(crate) fn verify(&self, chain: &[Vec<u8>], server_name: &str) -> Result<(), TrustError> {
        let (leaf, intermediates) = chain
            .split_first()
            .ok_or_else(|| TrustError::Failed("the chain holds no certificate".to_owned()))?;
        let server_name = ServerName::try_from(server_name)
            .map_err(|error| TrustError::Failed(error.to_string()))?;
        let intermediates: Vec<CertificateDer<'_>> = intermediates
            .iter()
            .map(|certificate| CertificateDer::from(certificate.as_slice()))
            .collect();
        let checked = self.verifier.verify_server_cert(
            &CertificateDer::from(leaf.as_slice()),
            &intermediates,
            &server_name,
            &[],
            UnixTime::now(),
        );
        match checked {
            Ok(_) => Ok(()),
            Err(rustls::Error::InvalidCertificate(CertificateError::BadEncoding)) => Err(
                TrustError::Failed("the chain holds what is no certificate".to_owned()),
            ),
            Err(rustls::Error::InvalidCertificate(error)) => {
                Err(TrustError::Untrusted(error.to_string()))
            }
            Err(error) => Err(TrustError::Failed(error.to_string())),
        }
    }
}

/// The cryptography the checker verifies signatures with, and the program's HTTP speaks TLS with
/// (`network::http`), so a chain is checked as the program's HTTPS checks it.
pub(crate) fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

#[cfg(test)]
mod tests;
