//! What a package's signature signs: a first line naming the form, then a line for each signed
//! entry, in this order: its SHA-256 as 64 lowercase hex digits, two spaces, and its name. Every
//! line ends with a line feed.
//!
//! ```text
//! wapk-signature-1
//! <SHA-256 of manifest.toml>  manifest.toml
//! <SHA-256 of program.wasm>  program.wasm
//! <SHA-256 of icon.png>  icon.png
//! ```
//!
//! The lines after the first are what `shasum -a 256 manifest.toml program.wasm icon.png` prints
//! in a folder the package was unzipped into, so a person can check a package by hand.
//!
//! The invariant a later editor will break: the message is a function of the three entries'
//! bytes alone, never of how the zip holds them, so packing the entries again keeps the signature
//! (the design's decision 44). A change to the form is a new first line, which an installer must
//! know before a package signed that way opens.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::entries::{COMPONENT, ICON, MANIFEST};

/// The first line of the message: what it is, and which form of it.
const FORM: &str = "wapk-signature-1";

/// The SHA-256 of each entry the signature covers.
pub(crate) struct SignedDigests {
    pub(crate) manifest: [u8; 32],
    pub(crate) component: [u8; 32],
    pub(crate) icon: [u8; 32],
}

impl SignedDigests {
    /// The message the publisher's key signs.
    pub(crate) fn message(&self) -> String {
        let mut message = format!("{FORM}\n");
        for (entry, digest) in [
            (MANIFEST, &self.manifest),
            (COMPONENT, &self.component),
            (ICON, &self.icon),
        ] {
            writeln!(message, "{}  {entry}", hex::encode(digest)).expect("a String takes text");
        }
        message
    }
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_is_the_form_then_each_entrys_sha256_and_name() {
        let digests = SignedDigests {
            manifest: sha256(b"manifest"),
            component: sha256(b"component"),
            icon: sha256(b"icon"),
        };
        assert_eq!(
            digests.message(),
            "wapk-signature-1\n\
             05b3abf2579a5eb66403cd78be557fd860633a1fe2103c7642030defe32c657f  manifest.toml\n\
             6985ca1f4daa5a584a28eae043a239cb96689af1337ea13afb63e00c2bf512fa  program.wasm\n\
             c2d4b446a44ce54fab8e01150e24dd24f3d850c7c14dcfe31f6321341dd86874  icon.png\n"
        );
    }
}
