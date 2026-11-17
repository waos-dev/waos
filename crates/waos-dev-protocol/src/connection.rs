//! One connection, as frames over a stream: each message as postcard encodes it, behind a
//! four-byte little-endian count of its bytes, so the reader knows how much to wait for before
//! it allocates anything, and refuses a frame over [`MAX_FRAME_BYTES`] on the count alone. The
//! first frame each way is the protocol's number, [`PROTOCOL`], outside the messages' encoding,
//! so two sides of different builds find out before either reads a message it would misread.
//!
//! The invariants a later editor will break:
//! - After `send` or `receive` fails, the connection is done. A frame cut short, or one refused,
//!   leaves the stream somewhere inside a frame, and nothing says where the next one begins; the
//!   side that saw the error closes.
//! - The protocol frame's shape never changes: a count of four, then the number, little-endian.
//!   A side of any build reads it, and reads nothing else until the numbers agree.

use std::io::{self, ErrorKind};

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};

/// The protocol's number, which the first frame each way says: the messages and their encoding
/// as this build of the crate has them. Raised with any change a side of the build before would
/// misread, which, with postcard, is any change to a message but a variant or a field added at
/// the end.
pub const PROTOCOL: u32 = 8;

/// The largest frame either side accepts. A component is megabytes, a debug build tens of
/// them; a count beyond this is a stream that is not ours, refused before a byte of it is read.
pub const MAX_FRAME_BYTES: usize = 256 << 20;

const PREFIX_BYTES: usize = 4;

/// The protocol frame's body: the number's four bytes.
const PROTOCOL_BYTES: usize = 4;

/// One connection: length-prefixed postcard frames over `S`, the stream that carries them on
/// this machine ([`ClientStream`](crate::ClientStream) and [`ServerStream`](crate::ServerStream)).
/// Split into its halves, each is a `Connection` that only sends or only receives, so a WaOS
/// sends a program's output on one task while another watches for the connection closing.
#[derive(Debug)]
pub struct Connection<S> {
    stream: S,
}

impl<S> Connection<S> {
    pub fn new(stream: S) -> Connection<S> {
        Connection { stream }
    }
}

impl<S: AsyncRead + AsyncWrite> Connection<S> {
    /// The two halves, to receive on one task while another sends.
    pub fn into_split(self) -> (Connection<ReadHalf<S>>, Connection<WriteHalf<S>>) {
        let (reading, writing) = tokio::io::split(self.stream);
        (Connection::new(reading), Connection::new(writing))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Connection<S> {
    /// The first frame each way: says this build's [`PROTOCOL`], and reads the other side's
    /// before anything else. When they differ the connection is done, and the error, of kind
    /// `Unsupported`, names both numbers; `cargo waos` prints it. An other side that closed
    /// before saying its number is `UnexpectedEof`, or `BrokenPipe` when it was gone before this
    /// side's frame went out.
    pub async fn agree_on_protocol(&mut self) -> io::Result<()> {
        self.agree_on(PROTOCOL).await
    }

    async fn agree_on(&mut self, ours: u32) -> io::Result<()> {
        self.stream.write_all(&protocol_frame(ours)).await?;
        let theirs = self.read_protocol().await?;
        if theirs != ours {
            return Err(io::Error::new(
                ErrorKind::Unsupported,
                format!(
                    "the other side speaks protocol {theirs} and this side protocol {ours}: cargo waos and the WaOS it reaches must be built from the same waos-dev-protocol"
                ),
            ));
        }
        Ok(())
    }

    /// The other side's protocol number, from its first frame.
    async fn read_protocol(&mut self) -> io::Result<u32> {
        let len = self.read_prefix().await?.ok_or_else(|| {
            io::Error::new(
                ErrorKind::UnexpectedEof,
                "the other side closed before saying its protocol",
            )
        })?;
        if len != PROTOCOL_BYTES {
            return Err(invalid_data(format!(
                "the first frame is {len} bytes long, not a protocol number"
            )));
        }
        let mut number = [0; PROTOCOL_BYTES];
        self.stream.read_exact(&mut number).await?;
        Ok(u32::from_le_bytes(number))
    }
}

impl<S: AsyncWrite + Unpin> Connection<S> {
    /// Sends `message` as one frame; refused, unsent, when it is over the limit.
    pub async fn send(&mut self, message: &impl Serialize) -> io::Result<()> {
        let frame = frame(message)?;
        self.stream.write_all(&frame).await
    }
}

impl<S: AsyncRead + Unpin> Connection<S> {
    /// The next message; none once the other side closed between frames.
    pub async fn receive<M: DeserializeOwned>(&mut self) -> io::Result<Option<M>> {
        let Some(len) = self.read_prefix().await? else {
            return Ok(None);
        };
        let mut body = vec![0; len];
        self.stream.read_exact(&mut body).await?;
        postcard::from_bytes(&body).map(Some).map_err(invalid_data)
    }

    /// The next frame's byte count, checked against the limit; none when the other side closed
    /// between frames.
    async fn read_prefix(&mut self) -> io::Result<Option<usize>> {
        let mut prefix = [0; PREFIX_BYTES];
        let mut filled = 0;
        while filled < PREFIX_BYTES {
            match self.stream.read(&mut prefix[filled..]).await? {
                0 if filled == 0 => return Ok(None),
                0 => return Err(cut_short()),
                read => filled += read,
            }
        }
        accepted(u32::from_le_bytes(prefix) as usize).map(Some)
    }
}

/// The protocol frame: a count of four, then `protocol` little-endian.
fn protocol_frame(protocol: u32) -> [u8; PREFIX_BYTES + PROTOCOL_BYTES] {
    let mut frame = [0; PREFIX_BYTES + PROTOCOL_BYTES];
    frame[..PREFIX_BYTES].copy_from_slice(&(PROTOCOL_BYTES as u32).to_le_bytes());
    frame[PREFIX_BYTES..].copy_from_slice(&protocol.to_le_bytes());
    frame
}

/// `message` as one frame: its byte count, then its bytes.
fn frame(message: &impl Serialize) -> io::Result<Vec<u8>> {
    let mut frame = postcard::to_extend(message, vec![0; PREFIX_BYTES]).map_err(invalid_data)?;
    let len = accepted(frame.len() - PREFIX_BYTES)?;
    // Within the limit, so within a `u32`.
    frame[..PREFIX_BYTES].copy_from_slice(&(len as u32).to_le_bytes());
    Ok(frame)
}

/// `len` when a frame that long is accepted; the error names the limit otherwise.
fn accepted(len: usize) -> io::Result<usize> {
    if len > MAX_FRAME_BYTES {
        return Err(invalid_data(format!(
            "a frame of {len} bytes, over the {MAX_FRAME_BYTES} a connection accepts"
        )));
    }
    Ok(len)
}

fn cut_short() -> io::Error {
    io::Error::new(
        ErrorKind::UnexpectedEof,
        "the other side closed inside a frame",
    )
}

fn invalid_data(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::DuplexStream;

    fn pair() -> (Connection<DuplexStream>, Connection<DuplexStream>) {
        let (one, other) = tokio::io::duplex(64);
        (Connection::new(one), Connection::new(other))
    }

    #[tokio::test]
    async fn a_message_comes_back_as_sent() {
        let (mut sender, mut receiver) = pair();
        tokio::spawn(async move { sender.send(&(7u32, "seven".to_string())).await });
        let received: Option<(u32, String)> = receiver.receive().await.unwrap();
        assert_eq!(received, Some((7, "seven".into())));
    }

    #[tokio::test]
    async fn two_sides_of_one_build_agree_and_go_on_to_the_messages() {
        let (mut one, mut other) = pair();
        let (agreed, agreed_too) = tokio::join!(one.agree_on_protocol(), other.agree_on_protocol());
        agreed.unwrap();
        agreed_too.unwrap();
        tokio::spawn(async move { one.send(&7u32).await });
        assert_eq!(other.receive::<u32>().await.unwrap(), Some(7));
    }

    #[test]
    fn the_protocol_frame_is_a_count_of_four_then_the_number() {
        assert_eq!(protocol_frame(1), [4, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(protocol_frame(258), [4, 0, 0, 0, 2, 1, 0, 0]);
    }

    #[tokio::test]
    async fn two_sides_of_different_builds_each_refuse_naming_both_numbers() {
        let (mut one, mut other) = pair();
        let (refused, refused_too) = tokio::join!(one.agree_on(1), other.agree_on(2));
        let (refused, refused_too) = (refused.unwrap_err(), refused_too.unwrap_err());
        assert_eq!(refused.kind(), ErrorKind::Unsupported);
        assert_eq!(refused_too.kind(), ErrorKind::Unsupported);
        assert!(
            refused.to_string().contains("protocol 2")
                && refused.to_string().contains("protocol 1"),
            "{refused}"
        );
        assert!(
            refused_too.to_string().contains("protocol 1")
                && refused_too.to_string().contains("protocol 2"),
            "{refused_too}"
        );
    }

    #[tokio::test]
    async fn closing_before_the_protocol_frame_is_an_error() {
        let (one, mut other) = pair();
        drop(one);
        let error = other.agree_on_protocol().await.unwrap_err();
        assert!(
            matches!(
                error.kind(),
                ErrorKind::UnexpectedEof | ErrorKind::BrokenPipe
            ),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_first_frame_that_is_not_a_number_is_refused() {
        let (mut one, mut other) = pair();
        tokio::spawn(async move { one.send(&"a message before the protocol").await });
        let error = other.agree_on_protocol().await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn the_count_is_four_bytes_little_endian_before_the_message() {
        let frame = frame(&vec![1u8, 2, 3]).unwrap();
        // postcard writes a `Vec` as its length, then its bytes.
        assert_eq!(frame, [4, 0, 0, 0, 3, 1, 2, 3]);
    }

    #[tokio::test]
    async fn a_count_over_the_limit_is_refused_before_a_body_is_read() {
        let (mut sender, mut receiver) = pair();
        sender
            .stream
            .write_all(&u32::MAX.to_le_bytes())
            .await
            .unwrap();
        // No body follows: a receive that tried to read one would wait here forever.
        let error = receiver.receive::<Vec<u8>>().await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(
            error.to_string().contains(&MAX_FRAME_BYTES.to_string()),
            "the error names the limit: {error}"
        );
    }

    #[test]
    fn a_frame_at_the_limit_is_accepted_and_one_over_it_refused() {
        assert_eq!(accepted(MAX_FRAME_BYTES).unwrap(), MAX_FRAME_BYTES);
        assert_eq!(
            accepted(MAX_FRAME_BYTES + 1).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn closing_between_frames_is_none() {
        let (sender, mut receiver) = pair();
        drop(sender);
        assert_eq!(receiver.receive::<u32>().await.unwrap(), None);
    }

    #[tokio::test]
    async fn closing_inside_a_frame_is_an_error() {
        let (mut sender, mut receiver) = pair();
        sender.stream.write_all(&[8, 0]).await.unwrap();
        drop(sender);
        let error = receiver.receive::<u32>().await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
    }
}
