//! The dev server announced on the local network while it listens there for developer access, so
//! `waos devices` on a computer finds it: a DNS-SD service of `waos_dev_protocol`'s type on the
//! port it listens on, its TXT record the protocol's major, the certificate's fingerprint and the
//! system, announced as the system announces its own services (`os::announce_dev_server`).
//!
//! The invariant a later editor will break: an announcement is for finding only, and the dev
//! server answers its address whether or not one is made, so one that fails is said and nothing
//! else.

use std::io;

use waos_dev_protocol::{Announced, Fingerprint, SERVICE};

use crate::os::{DevServerAnnouncement, announce_dev_server};

/// The dev server's service on the network, there until dropped.
pub(crate) struct Announcement(
    #[expect(dead_code, reason = "kept for its drop")] DevServerAnnouncement,
);

/// Announces the dev server listening on `port`, answering with the certificate `fingerprint`
/// names.
pub(crate) async fn announce(port: u16, fingerprint: Fingerprint) -> io::Result<Announcement> {
    let announced = Announced::new(fingerprint, std::env::consts::OS);
    let txt = announced.entries();
    announce_dev_server(SERVICE, port, &txt)
        .await
        .map(Announcement)
}
