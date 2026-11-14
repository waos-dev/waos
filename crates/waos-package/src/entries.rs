//! What a package holds: its entries' names, and the most the ones read whole may hold.

/// The entry holding the program's [`Manifest`](crate::Manifest), as TOML.
pub const MANIFEST: &str = "manifest.toml";

/// The entry holding the program, as a component.
pub const COMPONENT: &str = "program.wasm";

/// The entry holding the program's icon, as a PNG.
pub const ICON: &str = "icon.png";

/// The entry holding the publisher's [`PublisherKey`](crate::PublisherKey): Ed25519's 32 bytes.
pub const PUBLISHER_KEY: &str = "publisher.key";

/// The entry holding the signature over the other three: Ed25519's 64 bytes.
pub(crate) const SIGNATURE: &str = "signature";

pub(crate) const MANIFEST_LIMIT: u64 = 64 * 1024;

pub(crate) const ICON_LIMIT: u64 = 8 * 1024 * 1024;

pub(crate) const PUBLISHER_KEY_LENGTH: usize = ed25519_dalek::PUBLIC_KEY_LENGTH;

pub(crate) const SIGNATURE_LENGTH: usize = ed25519_dalek::SIGNATURE_LENGTH;
