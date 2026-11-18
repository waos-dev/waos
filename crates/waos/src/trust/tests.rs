//! The system's checker on chains made here: a self-signed one, which no system trusts, and one
//! signed by an authority of the test's own, which the checker is told to trust beside the
//! system's roots, as a root the user added would be: what a trusted chain needs is a leaf for
//! the name, then the chain up to a root the system trusts. A chain up to the system's own roots
//! is not checked here: no server of the internet is reached, and no root is added to this
//! machine's.

use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::CertificateDer;
use rustls_platform_verifier::Verifier;

use super::{Trust, provider};
use crate::bindings::wasdk::platform::trust::TrustError;

const NAME: &str = "trusted.localhost";

#[test]
fn a_self_signed_chain_is_untrusted() {
    let leaf = rcgen::generate_simple_self_signed(vec![NAME.to_owned()]).unwrap();
    let trust = Trust::system().expect("this system has a checker");
    let checked = trust.verify(&[leaf.cert.der().to_vec()], NAME);
    assert!(
        matches!(checked, Err(TrustError::Untrusted(_))),
        "{checked:?}"
    );
}

#[test]
fn a_chain_up_to_a_trusted_root_for_its_name_is_trusted_and_for_another_is_not() {
    let authority = authority();
    let leaf = leaf_signed_by(&authority);
    let trust = trusting(authority.der().clone());
    let trusted = trust.verify(std::slice::from_ref(&leaf), NAME);
    assert!(trusted.is_ok(), "{trusted:?}");
    let other = trust.verify(&[leaf], "other.localhost");
    assert!(matches!(other, Err(TrustError::Untrusted(_))), "{other:?}");
}

#[test]
fn what_is_no_chain_or_no_name_fails_to_be_checked() {
    let trust = Trust::system().expect("this system has a checker");
    let empty = trust.verify(&[], NAME);
    assert!(matches!(empty, Err(TrustError::Failed(_))), "{empty:?}");
    let garbage = trust.verify(&[b"not a certificate".to_vec()], NAME);
    assert!(matches!(garbage, Err(TrustError::Failed(_))), "{garbage:?}");
    let leaf = rcgen::generate_simple_self_signed(vec![NAME.to_owned()]).unwrap();
    let nameless = trust.verify(&[leaf.cert.der().to_vec()], "not a name!");
    assert!(
        matches!(nameless, Err(TrustError::Failed(_))),
        "{nameless:?}"
    );
}

/// An authority of the test's own, which signs servers' certificates.
fn authority() -> CertifiedIssuer<'static, KeyPair> {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    // Windows' checker asks that a certificate that signs have the use, as it is absent here.
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap()
}

/// A server's certificate for [`NAME`], signed by `authority`.
fn leaf_signed_by(authority: &CertifiedIssuer<'static, KeyPair>) -> Vec<u8> {
    let mut params = CertificateParams::new(vec![NAME.to_owned()]).unwrap();
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    let key = KeyPair::generate().unwrap();
    params.signed_by(&key, authority).unwrap().der().to_vec()
}

/// The system's checker, trusting `root` beside the system's own roots.
fn trusting(root: CertificateDer<'static>) -> Trust {
    let verifier = Verifier::new_with_extra_roots([root], provider()).unwrap();
    Trust {
        verifier: Arc::new(verifier),
    }
}
