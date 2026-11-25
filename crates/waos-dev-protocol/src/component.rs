//! An install's component as an upload over the connection: its `Install` announces the
//! component's length and its SHA-256, and the `Component` frames after it carry the bytes, each at most
//! [`CHUNK_BYTES`], which the receiving side assembles and checks. A component is megabytes, a
//! debug build hundreds of them, over the largest frame a connection accepts whole, so an install's
//! component is the one message that is many.
//!
//! The invariants a later editor will break:
//! - The parts are ordinary frames, so the frame cap stays a guard per frame: a part is far
//!   under it, and the whole component's size is bounded by nothing but what the receiving side
//!   can hold.
//! - The receiver reads parts until the announced length is reached, and answers the whole only
//!   when its digest is the one announced: a component that arrives short, past its length, or
//!   changed on the way is an error naming what did not hold. The errors of a wrong upload are
//!   `InvalidData`, after whole frames, so the connection still speaks and the install can
//!   be refused on it.

use std::io;

use crate::connection::{Connection, ReceiveFrames, SendFrames, invalid_data};
use crate::messages::Component;
use sha2::{Digest as _, Sha256};

/// How many bytes of a component each `Component` frame carries, at most: far under the largest
/// frame a connection accepts, so the cap keeps guarding each frame while the whole component is
/// as large as it likes.
pub const CHUNK_BYTES: usize = 8 << 20;

impl<F: SendFrames> Connection<F> {
    /// Sends `component` as the `Component` frames its `Install` announced: the parts in order, each
    /// at most [`CHUNK_BYTES`].
    pub async fn send_component(&mut self, component: &[u8]) -> io::Result<()> {
        for part in component.chunks(CHUNK_BYTES) {
            self.send(&Component {
                bytes: part.to_vec(),
            })
            .await?;
        }
        Ok(())
    }
}

impl<F: ReceiveFrames> Connection<F> {
    /// The component an `Install` announced as `length` bytes of `digest`: the `Component` frames
    /// that follow, assembled and checked. A frame that is not a part of the component, or more
    /// bytes than the length announced, is an error, and so is a digest that does not hold once
    /// the bytes are all there.
    pub async fn receive_component(
        &mut self,
        length: u64,
        digest: &[u8; 32],
    ) -> io::Result<Vec<u8>> {
        // Grown by the parts that arrive, not by the length announced, which nothing has checked:
        // a length of anything preallocates nothing.
        let mut component = Vec::new();
        while (component.len() as u64) < length {
            match self.receive::<Component>().await? {
                Some(Component { bytes }) => {
                    if component.len() as u64 + bytes.len() as u64 > length {
                        return Err(invalid_data(format!(
                            "a component frame past the {length} bytes its install announced"
                        )));
                    }
                    component.extend_from_slice(&bytes);
                }
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the other side closed before the component was whole",
                    ));
                }
            }
        }
        if &component_digest(&component) != digest {
            return Err(invalid_data(
                "the component's digest is not the one its install announced",
            ));
        }
        Ok(component)
    }
}

/// The SHA-256 of `component`, which an install's `digest` carries and its receiver checks.
pub fn component_digest(component: &[u8]) -> [u8; 32] {
    let mut digest = [0; 32];
    digest.copy_from_slice(&Sha256::digest(component));
    digest
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use tokio::io::DuplexStream;

    use super::*;
    use crate::connection::StreamFrames;
    use crate::messages::Install;
    use waos_package::ProgramId;

    type Duplex = Connection<StreamFrames<DuplexStream>>;

    fn pair() -> (Duplex, Duplex) {
        let (one, other) = tokio::io::duplex(64);
        (Connection::new(one), Connection::new(other))
    }

    fn id() -> ProgramId {
        "dev.example.triangle".parse().unwrap()
    }

    fn install(length: u64, digest: [u8; 32]) -> Install {
        Install {
            id: id(),
            kind: waos_package::ProgramKind::App,
            name: "Triangle".into(),
            version: "0.1.0".parse().unwrap(),
            icon: None,
            category: None,
            file_associations: Vec::new(),
            length,
            digest,
        }
    }

    /// The `Install` at the other end of `receiver`, whose component the test reads.
    async fn announced(mut receiver: Duplex) -> (Duplex, u64, [u8; 32]) {
        let install: Install = receiver.receive().await.unwrap().expect("an install");
        (receiver, install.length, install.digest)
    }

    #[tokio::test]
    async fn a_component_larger_than_one_part_is_sent_and_comes_back_whole() {
        let (mut sender, receiver) = pair();
        // Half over a part's size, so the last part is a short one.
        let component: Vec<u8> = (0..=255u8)
            .cycle()
            .take(CHUNK_BYTES + CHUNK_BYTES / 2)
            .collect();
        let sending = tokio::spawn(async move {
            sender
                .send(&install(
                    component.len() as u64,
                    component_digest(&component),
                ))
                .await
                .unwrap();
            sender.send_component(&component).await
        });
        let (mut receiver, length, digest) = announced(receiver).await;
        let received = receiver.receive_component(length, &digest).await.unwrap();
        assert_eq!(received.len(), CHUNK_BYTES + CHUNK_BYTES / 2);
        // The pattern the component was made of, at the ends and where the parts meet.
        for at in [0, CHUNK_BYTES - 1, CHUNK_BYTES, received.len() - 1] {
            assert_eq!(received[at], (at % 256) as u8, "byte {at}");
        }
        sending.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn a_component_whose_digest_is_not_the_one_announced_is_refused() {
        let (mut sender, receiver) = pair();
        let component = b"not the component announced".to_vec();
        let length = component.len() as u64;
        let sending = tokio::spawn(async move {
            sender.send(&install(length, [0; 32])).await.unwrap();
            sender.send_component(&component).await.unwrap();
        });
        let (mut receiver, length, digest) = announced(receiver).await;
        let error = receiver
            .receive_component(length, &digest)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(error.to_string().contains("digest"), "{error}");
        sending.await.unwrap();
    }

    #[tokio::test]
    async fn a_part_past_the_length_an_install_announced_is_refused() {
        let (mut sender, receiver) = pair();
        let sending = tokio::spawn(async move {
            sender
                .send(&install(4, component_digest(b"eight")))
                .await
                .unwrap();
            sender.send_component(b"eight bytes").await.unwrap();
        });
        let (mut receiver, length, digest) = announced(receiver).await;
        let error = receiver
            .receive_component(length, &digest)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(error.to_string().contains("past"), "{error}");
        sending.await.unwrap();
    }
}
