//! A publisher's keys, Ed25519 as RFC 8032 defines it: the [`SigningKey`] a package is signed with,
//! which its publisher keeps, and the [`PublisherKey`] the package carries, which an installer
//! remembers and holds every update of the program to.
//!
//! The invariants a later editor will break: a signature is checked with dalek's
//! `verify_strict`, which refuses the forms of a signature or key that would let one signature
//! pass for another message; a signing key never shows its secret through `Debug`; and a
//! publisher key's text is its 32 bytes in hex, which [`PublisherKey::to_hex`] writes and its
//! `FromStr` reads, so the key a catalog pins reads back as written.

use std::fmt;
use std::str::FromStr;

use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use ed25519_dalek::{SECRET_KEY_LENGTH, Signature, Signer, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::PackageError;
use crate::entries::PUBLISHER_KEY_LENGTH;

/// How many bytes of a key's SHA-256 its fingerprint shows.
const FINGERPRINT_BYTES: usize = 8;

/// A publisher's public key, as a package carries it: its 32 bytes, known to be a point of
/// Ed25519's curve.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublisherKey([u8; PUBLISHER_KEY_LENGTH]);

impl PublisherKey {
    /// The key from its 32 bytes, as a package's `publisher.key` holds them.
    pub fn from_bytes(bytes: &[u8]) -> Result<PublisherKey, KeyError> {
        let bytes: [u8; PUBLISHER_KEY_LENGTH] = bytes
            .try_into()
            .map_err(|_| KeyError::PublicKeyLength(bytes.len()))?;
        VerifyingKey::from_bytes(&bytes).map_err(|_| KeyError::NotAPublicKey)?;
        Ok(PublisherKey(bytes))
    }

    pub(crate) fn to_bytes(self) -> [u8; PUBLISHER_KEY_LENGTH] {
        self.0
    }

    /// The key in full, as text: its 32 bytes as 64 lowercase hex digits, which is how a catalog
    /// names the key it pins and what `parse` reads back. People tell keys apart by the
    /// [`fingerprint`](Self::fingerprint).
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// The key in a form short enough for people to tell keys apart by, in the library and in
    /// errors: the first 8 bytes of its SHA-256 as four groups of four hex digits, like
    /// `E744 C079 1320 C328`. Installers compare whole keys; this is only for showing.
    pub fn fingerprint(&self) -> String {
        let digest = Sha256::digest(self.0);
        let groups: Vec<String> = digest[..FINGERPRINT_BYTES]
            .chunks(2)
            .map(hex::encode_upper)
            .collect();
        groups.join(" ")
    }

    /// Refuses `signature` unless this key made it over `message`.
    pub(crate) fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), PackageError> {
        let key = VerifyingKey::from_bytes(&self.0)
            .expect("a publisher key is a point, checked when it was made");
        key.verify_strict(message, signature)
            .map_err(|_| PackageError::SignatureDoesNotMatch)
    }
}

/// The key from its 64 hex digits, as [`PublisherKey::to_hex`] writes them, in either case.
impl FromStr for PublisherKey {
    type Err = KeyError;

    fn from_str(text: &str) -> Result<PublisherKey, KeyError> {
        let bytes = hex::decode(text).map_err(|_| KeyError::NotHexDigits)?;
        PublisherKey::from_bytes(&bytes)
    }
}

impl fmt::Debug for PublisherKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublisherKey({})", self.fingerprint())
    }
}

/// A publisher's private key, which signs every package of theirs. Whoever holds it can publish
/// updates of the publisher's programs; a publisher who loses it can publish none, and gives a
/// program a new id to publish it again.
pub struct SigningKey(ed25519_dalek::SigningKey);

impl SigningKey {
    /// A new key, from the system's random source.
    pub fn generate() -> SigningKey {
        let mut secret = [0; SECRET_KEY_LENGTH];
        getrandom::fill(&mut secret).expect("the system's random source answers");
        SigningKey(ed25519_dalek::SigningKey::from_bytes(&secret))
    }

    /// The key from its text: PKCS#8's PEM, which begins `-----BEGIN PRIVATE KEY-----`, as
    /// [`to_pem`](Self::to_pem) writes it and `openssl genpkey -algorithm ed25519` makes it.
    /// Whitespace around it is ignored.
    pub fn from_pem(text: &str) -> Result<SigningKey, KeyError> {
        ed25519_dalek::SigningKey::from_pkcs8_pem(text.trim())
            .map(SigningKey)
            .map_err(|error| KeyError::NotASigningKey(error.to_string()))
    }

    /// The key as text: PKCS#8's PEM, the private key with its public key, lines ended by line
    /// feeds.
    pub fn to_pem(&self) -> String {
        let pem = self
            .0
            .to_pkcs8_pem(LineEnding::LF)
            .expect("an Ed25519 key encodes as PKCS#8");
        pem.as_str().to_owned()
    }

    /// The public half, which every package this key signs carries.
    pub fn publisher(&self) -> PublisherKey {
        PublisherKey(self.0.verifying_key().to_bytes())
    }

    pub(crate) fn sign(&self, message: &[u8]) -> Signature {
        self.0.sign(message)
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SigningKey(publisher {})",
            self.publisher().fingerprint()
        )
    }
}

/// Why bytes or text are not a publisher's key. A message reads after the name of what held
/// them: "publisher.key: it is 31 bytes; …".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// Not the 32 bytes of an Ed25519 public key: this many.
    PublicKeyLength(usize),
    /// 32 bytes that are no Ed25519 public key.
    NotAPublicKey,
    /// Text that is not a key's bytes in hex.
    NotHexDigits,
    /// Not an Ed25519 private key in PKCS#8's PEM, in the decoder's words.
    NotASigningKey(String),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyError::PublicKeyLength(length) => write!(
                f,
                "it is {length} bytes; an Ed25519 public key is {PUBLISHER_KEY_LENGTH}"
            ),
            KeyError::NotAPublicKey => f.write_str("it is not an Ed25519 public key"),
            KeyError::NotHexDigits => write!(
                f,
                "it is not a publisher key's {PUBLISHER_KEY_LENGTH} bytes as {} hex digits",
                PUBLISHER_KEY_LENGTH * 2
            ),
            KeyError::NotASigningKey(reason) => write!(
                f,
                "it is not an Ed25519 private key in PKCS#8's PEM, which begins \"-----BEGIN PRIVATE KEY-----\": {reason}"
            ),
        }
    }
}

impl std::error::Error for KeyError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 8410's example of an Ed25519 private key, section 10.3, in the form OpenSSL writes.
    const RFC_8410_KEY: &str = "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEINTuctv5E1hK1bbY8fdp+K06/nwoy/HU++CXqI9EdVhC
-----END PRIVATE KEY-----
";

    /// Its public key, as the RFC gives it.
    const RFC_8410_PUBLIC_KEY: &str =
        "19bf44096984cdfe8541bac167dc3b96c85086aa30b6b6cb0c5c38ad703166e1";

    #[test]
    fn a_key_reads_back_from_the_pem_it_writes() {
        let key = SigningKey::generate();
        let pem = key.to_pem();
        assert!(pem.starts_with("-----BEGIN PRIVATE KEY-----\n"), "{pem}");
        assert_eq!(
            SigningKey::from_pem(&pem).unwrap().publisher(),
            key.publisher()
        );
        let without_its_last_line_feed = pem.trim_end();
        assert_eq!(
            SigningKey::from_pem(without_its_last_line_feed)
                .unwrap()
                .publisher(),
            key.publisher()
        );
    }

    #[test]
    fn the_pem_openssl_writes_is_read() {
        let key = SigningKey::from_pem(RFC_8410_KEY).unwrap();
        assert_eq!(hex::encode(key.publisher().to_bytes()), RFC_8410_PUBLIC_KEY);
    }

    #[test]
    fn text_that_is_not_a_private_key_is_refused() {
        for text in [
            "",
            "a key",
            RFC_8410_KEY.replace("PRIVATE", "PUBLIC").as_str(),
        ] {
            let error = SigningKey::from_pem(text).unwrap_err();
            assert!(matches!(error, KeyError::NotASigningKey(_)), "{error}");
        }
    }

    #[test]
    fn generated_keys_differ() {
        assert_ne!(
            SigningKey::generate().publisher(),
            SigningKey::generate().publisher()
        );
    }

    #[test]
    fn a_public_key_reads_from_its_32_bytes_and_no_other_length() {
        let publisher = SigningKey::generate().publisher();
        let bytes = publisher.to_bytes();
        assert_eq!(PublisherKey::from_bytes(&bytes).unwrap(), publisher);
        assert_eq!(
            PublisherKey::from_bytes(&bytes[..31]).unwrap_err(),
            KeyError::PublicKeyLength(31)
        );
        assert_eq!(
            PublisherKey::from_bytes(&[bytes.as_slice(), &[0]].concat()).unwrap_err(),
            KeyError::PublicKeyLength(33)
        );
    }

    #[test]
    fn a_public_key_reads_back_from_its_hex() {
        let publisher = SigningKey::from_pem(RFC_8410_KEY).unwrap().publisher();
        assert_eq!(publisher.to_hex(), RFC_8410_PUBLIC_KEY);
        assert_eq!(
            RFC_8410_PUBLIC_KEY.parse::<PublisherKey>().unwrap(),
            publisher
        );
        assert_eq!(
            RFC_8410_PUBLIC_KEY
                .to_uppercase()
                .parse::<PublisherKey>()
                .unwrap(),
            publisher
        );
    }

    #[test]
    fn text_that_is_not_a_public_keys_hex_is_refused() {
        assert_eq!(
            "not hex".parse::<PublisherKey>().unwrap_err(),
            KeyError::NotHexDigits
        );
        assert_eq!(
            RFC_8410_PUBLIC_KEY[..62]
                .parse::<PublisherKey>()
                .unwrap_err(),
            KeyError::PublicKeyLength(31)
        );
    }

    #[test]
    fn a_fingerprint_is_the_start_of_the_keys_sha256_in_four_groups() {
        let publisher = SigningKey::from_pem(RFC_8410_KEY).unwrap().publisher();
        assert_eq!(publisher.fingerprint(), "E744 C079 1320 C328");
    }

    #[test]
    fn a_signing_key_shows_its_publisher_never_its_secret() {
        let key = SigningKey::from_pem(RFC_8410_KEY).unwrap();
        assert_eq!(
            format!("{key:?}"),
            "SigningKey(publisher E744 C079 1320 C328)"
        );
    }
}
