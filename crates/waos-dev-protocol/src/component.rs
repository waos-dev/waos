//! A run's component as an upload over the connection: its `Run` announces the component's
//! length and its SHA-256, and the `Component` frames after it carry the bytes, each at most
//! [`CHUNK_BYTES`], which the receiving side assembles and checks. A component is megabytes, a
//! debug build hundreds of them, over the largest frame a connection accepts whole, so a run's
//! component is the one message that is many.
//!
//! The invariants a later editor will break:
//! - The parts are ordinary frames, so the frame cap stays a guard per frame: a part is far
//!   under it, and the whole component's size is bounded by nothing but what the receiving side
//!   can hold.
//! - The receiver reads parts until the announced length is reached, and answers the whole only
//!   when its digest is the one announced: a component that arrives short, past its length, or
//!   changed on the way is an error naming what did not hold. The errors of a wrong upload are
//!   `InvalidData`, after whole frames, so the connection still speaks and the run can be
//!   refused on it.

use std::io;

use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncRead, AsyncWrite};

use waos_package::ProgramId;

use crate::connection::{Connection, invalid_data};
use crate::messages::ToWaOS;

/// How many bytes of a component each `Component` frame carries, at most: far under the largest
/// frame a connection accepts, so the cap keeps guarding each frame while the whole component is
/// as large as it likes.
pub const CHUNK_BYTES: usize = 8 << 20;

impl<S: AsyncWrite + Unpin> Connection<S> {
    /// Sends a run of `component` as the program `id` with `variables`: the `Run` announcing the
    /// component's length and digest, then the component itself in `Component` frames.
    pub async fn send_run(
        &mut self,
        id: &ProgramId,
        variables: &[(String, String)],
        component: &[u8],
    ) -> io::Result<()> {
        self.send(&ToWaOS::Run {
            id: id.clone(),
            length: component.len() as u64,
            digest: component_digest(component),
            variables: variables.to_vec(),
        })
        .await?;
        for part in component.chunks(CHUNK_BYTES) {
            self.send(&ToWaOS::Component {
                bytes: part.to_vec(),
            })
            .await?;
        }
        Ok(())
    }
}

impl<S: AsyncRead + Unpin> Connection<S> {
    /// The component a `Run` announced as `length` bytes of `digest`: the `Component` frames
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
            match self.receive::<ToWaOS>().await? {
                Some(ToWaOS::Component { bytes }) => {
                    if component.len() as u64 + bytes.len() as u64 > length {
                        return Err(invalid_data(format!(
                            "a component frame past the {length} bytes its run announced"
                        )));
                    }
                    component.extend_from_slice(&bytes);
                }
                Some(_) => {
                    return Err(invalid_data(
                        "a message that is not a component frame, while the component was still being sent",
                    ));
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
                "the component's digest is not the one its run announced",
            ));
        }
        Ok(component)
    }
}

/// The SHA-256 of `component`, which a run's `digest` carries and its receiver checks.
pub fn component_digest(component: &[u8]) -> [u8; 32] {
    let mut digest = [0; 32];
    digest.copy_from_slice(&Sha256::digest(component));
    digest
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use tokio::io::DuplexStream;
    use waos_package::ProgramId;

    use super::*;

    fn pair() -> (Connection<DuplexStream>, Connection<DuplexStream>) {
        let (one, other) = tokio::io::duplex(64);
        (Connection::new(one), Connection::new(other))
    }

    fn id() -> ProgramId {
        "dev.example.triangle".parse().unwrap()
    }

    /// The `Run` at the other end of `receiver`, whose component the test reads.
    async fn announced(
        mut receiver: Connection<DuplexStream>,
    ) -> (Connection<DuplexStream>, u64, [u8; 32]) {
        let ToWaOS::Run { length, digest, .. } =
            receiver.receive::<ToWaOS>().await.unwrap().expect("a run")
        else {
            panic!("a run");
        };
        (receiver, length, digest)
    }

    #[tokio::test]
    async fn a_component_larger_than_one_part_is_sent_and_comes_back_whole() {
        let (mut sender, receiver) = pair();
        // Half over a part's size, so the last part is a short one.
        let component: Vec<u8> = (0..=255u8)
            .cycle()
            .take(CHUNK_BYTES + CHUNK_BYTES / 2)
            .collect();
        let sending = tokio::spawn(async move { sender.send_run(&id(), &[], &component).await });
        let (mut receiver, length, digest) = announced(receiver).await;
        assert_eq!(length, (CHUNK_BYTES + CHUNK_BYTES / 2) as u64);
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
            sender
                .send(&ToWaOS::Run {
                    id: id(),
                    length,
                    digest: [0; 32],
                    variables: Vec::new(),
                })
                .await
                .unwrap();
            sender
                .send(&ToWaOS::Component { bytes: component })
                .await
                .unwrap();
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
    async fn a_part_past_the_length_a_run_announced_is_refused() {
        let (mut sender, receiver) = pair();
        let part = b"eight bytes".to_vec();
        let sending = tokio::spawn(async move {
            sender
                .send(&ToWaOS::Run {
                    id: id(),
                    length: 4,
                    digest: component_digest(b"eight"),
                    variables: Vec::new(),
                })
                .await
                .unwrap();
            sender
                .send(&ToWaOS::Component { bytes: part })
                .await
                .unwrap();
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
