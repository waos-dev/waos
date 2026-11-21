//! One connection, as frames over a stream: each message as CBOR encodes it, behind a four-byte
//! little-endian count of its bytes, so the reader knows how much to wait for before it allocates
//! anything, and refuses a frame over [`MAX_FRAME_BYTES`] on the count alone. CBOR names each
//! variant and field, so a message added or a field added is read by a side that knows it, and
//! ignored by one that does not. The first frame each way is the [`Hello`], which says the build
//! and the protocol's major, so the two halves find out before either reads a request it would
//! misread.
//!
//! The invariants a later editor will break:
//! - After `send` or `receive` fails, the connection is done. A frame cut short, or one refused,
//!   leaves the stream somewhere inside a frame, and nothing says where the next one begins; the
//!   side that saw the error closes.
//! - The hello's shape is frozen: a field is added with `#[serde(default)]`, and only a real break
//!   raises the major.

use std::io::{self, ErrorKind};

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};

use crate::hello::{Hello, MAJOR};

/// The largest frame either side accepts. A run's component goes over in parts far under this,
/// [`CHUNK_BYTES`](crate::CHUNK_BYTES) at most, and a program's output and a window's picture are
/// megabytes; a count beyond this is a stream that is not ours, refused before a byte of it is
/// read.
pub const MAX_FRAME_BYTES: usize = 256 << 20;

const PREFIX_BYTES: usize = 4;
const LEGACY_PROTOCOL_BYTES: usize = 4;
/// Number answered to a legacy 0.2 `waos` (whose protocols stopped at 10) so its mismatch
/// message reads this WaOS as newer and tells its user to update `waos`.
const LEGACY_REPLY_PROTOCOL: u32 = 11;

/// Whether a connection's stream stays on this machine, as the socket and the pipe do, or crosses
/// the network, where this machine's paths mean nothing, so `Open` is refused there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locality {
    ThisMachine,
    Network,
}

/// One connection: length-prefixed CBOR frames over `S`, the stream that carries them on this
/// machine ([`ClientStream`](crate::ClientStream) and [`ServerStream`](crate::ServerStream)).
/// Split into its halves, each is a `Connection` that only sends or only receives, so a WaOS
/// sends a program's output on one task while another watches for the connection closing.
#[derive(Debug)]
pub struct Connection<S> {
    stream: S,
    locality: Locality,
}

impl<S> Connection<S> {
    /// A connection over `stream`, which stays on this machine.
    pub fn new(stream: S) -> Connection<S> {
        Connection {
            stream,
            locality: Locality::ThisMachine,
        }
    }

    /// A connection over `stream`, which is `locality`'s: a network transport says
    /// [`Locality::Network`], and nothing else may send `Open` over it.
    pub fn over(stream: S, locality: Locality) -> Connection<S> {
        Connection { stream, locality }
    }

    /// Whether the connection stays on this machine.
    pub fn locality(&self) -> Locality {
        self.locality
    }
}

impl<S: AsyncRead + AsyncWrite> Connection<S> {
    /// The two halves, to receive on one task while another sends.
    pub fn into_split(self) -> (Connection<ReadHalf<S>>, Connection<WriteHalf<S>>) {
        let (reading, writing) = tokio::io::split(self.stream);
        (
            Connection::over(reading, self.locality),
            Connection::over(writing, self.locality),
        )
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Connection<S> {
    /// The client's first frame each way: says `ours`, and reads the WaOS's hello before anything
    /// else. An other side that closed before saying one is `UnexpectedEof`, or `BrokenPipe` when
    /// it was gone before this side's frame went out; an older WaOS speaking the 4-byte protocol
    /// number of WaOS 0.2 is `Unsupported` naming WaOS 0.2 and how to update.
    pub async fn hello(&mut self, ours: &Hello) -> io::Result<Hello> {
        self.send(ours).await?;
        let Some(len) = self.read_prefix().await? else {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "the other side closed before saying its hello",
            ));
        };
        if len == LEGACY_PROTOCOL_BYTES {
            let mut number = [0; LEGACY_PROTOCOL_BYTES];
            self.stream.read_exact(&mut number).await?;
            return Err(io::Error::new(
                ErrorKind::Unsupported,
                "this WaOS speaks the dev protocol of WaOS 0.2; update WaOS from https://waos.app",
            ));
        }
        let mut body = vec![0; len];
        self.stream.read_exact(&mut body).await?;
        ciborium::from_reader(body.as_slice()).map_err(invalid_data)
    }

    /// WaOS's side of the hello: reads the client's first frame first, and answers with `ours`;
    /// when a `waos` of WaOS 0.2 sends its 4-byte protocol number, answers a 4-byte number
    /// frame of our own so the old client prints its mismatch naming both numbers, and returns
    /// `Unsupported`.
    pub async fn accept_hello(&mut self, ours: &Hello) -> io::Result<Hello> {
        let Some(len) = self.read_prefix().await? else {
            return Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "the other side closed before saying its hello",
            ));
        };
        if len == LEGACY_PROTOCOL_BYTES {
            let mut number = [0; LEGACY_PROTOCOL_BYTES];
            self.stream.read_exact(&mut number).await?;
            let theirs = u32::from_le_bytes(number);
            let mut frame = Vec::with_capacity(PREFIX_BYTES + LEGACY_PROTOCOL_BYTES);
            frame.extend_from_slice(&(LEGACY_PROTOCOL_BYTES as u32).to_le_bytes());
            frame.extend_from_slice(&LEGACY_REPLY_PROTOCOL.to_le_bytes());
            let _ = self.stream.write_all(&frame).await;
            return Err(io::Error::new(
                ErrorKind::Unsupported,
                format!(
                    "a waos of WaOS 0.2 speaks legacy dev protocol {theirs}; this WaOS speaks {MAJOR}"
                ),
            ));
        }
        let mut body = vec![0; len];
        self.stream.read_exact(&mut body).await?;
        let theirs: Hello = ciborium::from_reader(body.as_slice()).map_err(invalid_data)?;
        self.send(ours).await?;
        Ok(theirs)
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
        ciborium::from_reader(body.as_slice())
            .map(Some)
            .map_err(invalid_data)
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

/// `message` as one frame: its byte count, then its CBOR.
fn frame(message: &impl Serialize) -> io::Result<Vec<u8>> {
    let mut frame = vec![0; PREFIX_BYTES];
    ciborium::into_writer(message, &mut frame).map_err(invalid_data)?;
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

pub(crate) fn invalid_data(
    error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::DuplexStream;

    use crate::messages::Component;

    fn pair() -> (Connection<DuplexStream>, Connection<DuplexStream>) {
        let (one, other) = tokio::io::duplex(64);
        (Connection::new(one), Connection::new(other))
    }

    fn hello() -> Hello {
        Hello::command("waos 0.3.0")
    }

    #[tokio::test]
    async fn a_message_comes_back_as_sent() {
        let (mut sender, mut receiver) = pair();
        tokio::spawn(async move { sender.send(&(7u32, "seven".to_string())).await });
        let received: Option<(u32, String)> = receiver.receive().await.unwrap();
        assert_eq!(received, Some((7, "seven".into())));
    }

    #[tokio::test]
    async fn two_sides_exchange_their_hellos_and_go_on_to_the_messages() {
        let (mut one, mut other) = pair();
        let ours = hello();
        let server_hello = Hello::waos("WaOS 0.3.0");
        let (theirs, theirs_too) =
            tokio::join!(one.hello(&ours), other.accept_hello(&server_hello));
        assert_eq!(theirs.unwrap(), server_hello);
        assert_eq!(theirs_too.unwrap(), ours);
        tokio::spawn(async move { one.send(&7u32).await });
        assert_eq!(other.receive::<u32>().await.unwrap(), Some(7));
    }

    #[tokio::test]
    async fn a_new_client_meeting_a_legacy_server_names_waos_0_2_and_how_to_update() {
        let (mut client, mut legacy_server) = pair();
        tokio::spawn(async move {
            let mut frame = Vec::new();
            frame.extend_from_slice(&4u32.to_le_bytes());
            frame.extend_from_slice(&10u32.to_le_bytes());
            legacy_server.stream.write_all(&frame).await.unwrap();
        });
        let error = client.hello(&hello()).await.unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert_eq!(
            error.to_string(),
            "this WaOS speaks the dev protocol of WaOS 0.2; update WaOS from https://waos.app"
        );
    }

    #[tokio::test]
    async fn a_new_server_meeting_a_legacy_client_answers_a_legacy_number_frame() {
        let (mut legacy_client, mut server) = pair();
        let mut frame = Vec::new();
        frame.extend_from_slice(&4u32.to_le_bytes());
        frame.extend_from_slice(&10u32.to_le_bytes());
        legacy_client.stream.write_all(&frame).await.unwrap();
        let error = server
            .accept_hello(&Hello::waos("WaOS 0.3.0"))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert!(error.to_string().contains("10"), "{error}");

        let mut reply = [0u8; 8];
        legacy_client.stream.read_exact(&mut reply).await.unwrap();
        assert_eq!(u32::from_le_bytes(reply[..4].try_into().unwrap()), 4);
        assert_eq!(
            u32::from_le_bytes(reply[4..].try_into().unwrap()),
            LEGACY_REPLY_PROTOCOL
        );
    }

    #[tokio::test]
    async fn closing_before_the_hello_is_an_error() {
        let (one, mut other) = pair();
        drop(one);
        let error = other.hello(&hello()).await.unwrap_err();
        assert!(
            matches!(
                error.kind(),
                ErrorKind::UnexpectedEof | ErrorKind::BrokenPipe
            ),
            "{error}"
        );
    }

    #[test]
    fn the_count_is_four_bytes_little_endian_before_the_message() {
        let frame = frame(&Component {
            bytes: vec![1, 2, 3],
        })
        .unwrap();
        assert_eq!(&frame[..4], &11u32.to_le_bytes());
        // CBOR writes the struct as a map, and its bytes as a byte string: `{"bytes": h'010203'}`.
        assert_eq!(
            &frame[4..],
            &[0xa1, 0x65, b'b', b'y', b't', b'e', b's', 0x43, 1, 2, 3]
        );
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

    #[test]
    fn a_split_connection_keeps_the_locality_it_was_given() {
        let (one, _other) = tokio::io::duplex(64);
        let network = Connection::over(one, Locality::Network);
        let (reading, writing) = network.into_split();
        assert_eq!(reading.locality(), Locality::Network);
        assert_eq!(writing.locality(), Locality::Network);
    }
}
