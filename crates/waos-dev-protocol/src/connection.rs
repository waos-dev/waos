//! One connection, as messages: each message as CBOR encodes it, carried whole by its frames
//! (`Frames`). Over a stream, as the socket and the pipe are, a frame is the message behind a
//! four-byte little-endian count of its bytes, so the reader knows how much to wait for before it
//! allocates anything, and refuses a frame over [`MAX_FRAME_BYTES`] on the count alone
//! ([`StreamFrames`]). CBOR names each variant and field, so a message added or a field added is
//! read by a side that knows it, and ignored by one that does not. The first message each way is
//! the [`Hello`], which says the build and the protocol's major, so the two halves find out before
//! either reads a request it would misread.
//!
//! The invariants a later editor will break:
//! - After `send` or `receive` fails, the connection is done. A frame cut short, or one refused,
//!   leaves the stream somewhere inside a frame, and nothing says where the next one begins; the
//!   side that saw the error closes.
//! - The hello's shape is frozen: a field is added with `#[serde(default)]`, and only a real break
//!   raises the major.

use std::future::Future;
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

/// What sends a connection's messages: each message's bytes, whole, as one frame.
pub trait SendFrames: Send {
    /// Sends `frame`, one message's bytes, at most [`MAX_FRAME_BYTES`].
    fn send_frame(&mut self, frame: Vec<u8>) -> impl Future<Output = io::Result<()>> + Send;
}

/// What receives a connection's messages: each message's bytes, whole.
pub trait ReceiveFrames: Send {
    /// The next message's bytes, refused over [`MAX_FRAME_BYTES`]; none once the other side
    /// closed between messages.
    fn receive_frame(&mut self) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send;
}

/// What carries a connection's messages both ways, and parts into the half that receives and
/// the half that sends.
pub trait Frames: SendFrames + ReceiveFrames {
    type Receiving: ReceiveFrames;
    type Sending: SendFrames;

    fn split(self) -> (Self::Receiving, Self::Sending);
}

/// Frames over a byte stream: each message behind a four-byte little-endian count of its bytes.
#[derive(Debug)]
pub struct StreamFrames<S>(S);

impl<S> StreamFrames<S> {
    pub fn new(stream: S) -> StreamFrames<S> {
        StreamFrames(stream)
    }
}

/// The frames of a connection that came over either of two transports: the stream on this
/// machine, or the network's.
#[derive(Debug)]
pub enum TransportFrames<L, N> {
    Local(L),
    Network(N),
}

impl<L: SendFrames, N: SendFrames> SendFrames for TransportFrames<L, N> {
    async fn send_frame(&mut self, frame: Vec<u8>) -> io::Result<()> {
        match self {
            TransportFrames::Local(frames) => frames.send_frame(frame).await,
            TransportFrames::Network(frames) => frames.send_frame(frame).await,
        }
    }
}

impl<L: ReceiveFrames, N: ReceiveFrames> ReceiveFrames for TransportFrames<L, N> {
    async fn receive_frame(&mut self) -> io::Result<Option<Vec<u8>>> {
        match self {
            TransportFrames::Local(frames) => frames.receive_frame().await,
            TransportFrames::Network(frames) => frames.receive_frame().await,
        }
    }
}

impl<L: Frames, N: Frames> Frames for TransportFrames<L, N> {
    type Receiving = TransportFrames<L::Receiving, N::Receiving>;
    type Sending = TransportFrames<L::Sending, N::Sending>;

    fn split(self) -> (Self::Receiving, Self::Sending) {
        match self {
            TransportFrames::Local(frames) => {
                let (receiving, sending) = frames.split();
                (
                    TransportFrames::Local(receiving),
                    TransportFrames::Local(sending),
                )
            }
            TransportFrames::Network(frames) => {
                let (receiving, sending) = frames.split();
                (
                    TransportFrames::Network(receiving),
                    TransportFrames::Network(sending),
                )
            }
        }
    }
}

impl<S: AsyncWrite + Unpin + Send> SendFrames for StreamFrames<S> {
    async fn send_frame(&mut self, frame: Vec<u8>) -> io::Result<()> {
        // Within the limit, so within a `u32`.
        let count = (accepted(frame.len())? as u32).to_le_bytes();
        self.0.write_all(&count).await?;
        self.0.write_all(&frame).await
    }
}

impl<S: AsyncRead + Unpin + Send> ReceiveFrames for StreamFrames<S> {
    async fn receive_frame(&mut self) -> io::Result<Option<Vec<u8>>> {
        let Some(len) = self.read_prefix().await? else {
            return Ok(None);
        };
        let mut body = vec![0; len];
        self.0.read_exact(&mut body).await?;
        Ok(Some(body))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> Frames for StreamFrames<S> {
    type Receiving = StreamFrames<ReadHalf<S>>;
    type Sending = StreamFrames<WriteHalf<S>>;

    fn split(self) -> (Self::Receiving, Self::Sending) {
        let (reading, writing) = tokio::io::split(self.0);
        (StreamFrames(reading), StreamFrames(writing))
    }
}

impl<S: AsyncRead + Unpin> StreamFrames<S> {
    /// The next frame's byte count, checked against the limit; none when the other side closed
    /// between frames.
    async fn read_prefix(&mut self) -> io::Result<Option<usize>> {
        let mut prefix = [0; PREFIX_BYTES];
        let mut filled = 0;
        while filled < PREFIX_BYTES {
            match self.0.read(&mut prefix[filled..]).await? {
                0 if filled == 0 => return Ok(None),
                0 => return Err(cut_short()),
                read => filled += read,
            }
        }
        accepted(u32::from_le_bytes(prefix) as usize).map(Some)
    }
}

/// One connection: CBOR messages carried by `F`, its frames, over a stream on this machine
/// ([`StreamFrames`] of [`ClientStream`](crate::ClientStream) and
/// [`ServerStream`](crate::ServerStream)). Split into its halves, each is a `Connection` that
/// only sends or only receives, so a WaOS sends a program's output on one task while another
/// watches for the connection closing.
#[derive(Debug)]
pub struct Connection<F> {
    frames: F,
    locality: Locality,
}

impl<S> Connection<StreamFrames<S>> {
    /// A connection over `stream`, which stays on this machine.
    pub fn new(stream: S) -> Connection<StreamFrames<S>> {
        Connection::over(stream, Locality::ThisMachine)
    }

    /// A connection over `stream`, which is `locality`'s: a network transport says
    /// [`Locality::Network`], and nothing else may send `Open` over it.
    pub fn over(stream: S, locality: Locality) -> Connection<StreamFrames<S>> {
        Connection {
            frames: StreamFrames(stream),
            locality,
        }
    }
}

impl<F> Connection<F> {
    /// A connection whose messages `frames` carry, which is `locality`'s.
    pub fn carried(frames: F, locality: Locality) -> Connection<F> {
        Connection { frames, locality }
    }

    /// Whether the connection stays on this machine.
    pub fn locality(&self) -> Locality {
        self.locality
    }
}

impl<F: Frames> Connection<F> {
    /// The two halves, to receive on one task while another sends.
    pub fn into_split(self) -> (Connection<F::Receiving>, Connection<F::Sending>) {
        let (receiving, sending) = self.frames.split();
        (
            Connection {
                frames: receiving,
                locality: self.locality,
            },
            Connection {
                frames: sending,
                locality: self.locality,
            },
        )
    }

    /// The client's first message each way: says `ours`, and reads the WaOS's hello before
    /// anything else. An other side that closed before saying one is `UnexpectedEof`, or
    /// `BrokenPipe` when it was gone before this side's message went out; an older WaOS speaking
    /// the 4-byte protocol number of WaOS 0.2 is `Unsupported` naming WaOS 0.2 and how to update.
    pub async fn hello(&mut self, ours: &Hello) -> io::Result<Hello> {
        self.send(ours).await?;
        let Some(frame) = self.frames.receive_frame().await? else {
            return Err(closed_before_hello());
        };
        if frame.len() == LEGACY_PROTOCOL_BYTES {
            return Err(io::Error::new(
                ErrorKind::Unsupported,
                "this WaOS speaks the dev protocol of WaOS 0.2; update WaOS from https://waos.app",
            ));
        }
        ciborium::from_reader(frame.as_slice()).map_err(invalid_data)
    }

    /// WaOS's side of the hello: reads the client's first message first, and answers with `ours`;
    /// when a `waos` of WaOS 0.2 sends its 4-byte protocol number, answers a 4-byte number
    /// frame of our own so the old client prints its mismatch naming both numbers, and returns
    /// `Unsupported`.
    pub async fn accept_hello(&mut self, ours: &Hello) -> io::Result<Hello> {
        let Some(frame) = self.frames.receive_frame().await? else {
            return Err(closed_before_hello());
        };
        if let Ok(number) = <[u8; LEGACY_PROTOCOL_BYTES]>::try_from(frame.as_slice()) {
            let theirs = u32::from_le_bytes(number);
            let reply = LEGACY_REPLY_PROTOCOL.to_le_bytes().to_vec();
            let _ = self.frames.send_frame(reply).await;
            return Err(io::Error::new(
                ErrorKind::Unsupported,
                format!(
                    "a waos of WaOS 0.2 speaks legacy dev protocol {theirs}; this WaOS speaks {MAJOR}"
                ),
            ));
        }
        let theirs: Hello = ciborium::from_reader(frame.as_slice()).map_err(invalid_data)?;
        self.send(ours).await?;
        Ok(theirs)
    }
}

impl<F: SendFrames> Connection<F> {
    /// Sends `message` as one frame; refused, unsent, when it is over the limit.
    pub async fn send(&mut self, message: &impl Serialize) -> io::Result<()> {
        let frame = encoded(message)?;
        self.frames.send_frame(frame).await
    }
}

impl<F: ReceiveFrames> Connection<F> {
    /// The next message; none once the other side closed between frames.
    pub async fn receive<M: DeserializeOwned>(&mut self) -> io::Result<Option<M>> {
        let Some(frame) = self.frames.receive_frame().await? else {
            return Ok(None);
        };
        ciborium::from_reader(frame.as_slice())
            .map(Some)
            .map_err(invalid_data)
    }
}

fn closed_before_hello() -> io::Error {
    io::Error::new(
        ErrorKind::UnexpectedEof,
        "the other side closed before saying its hello",
    )
}

/// `message` as CBOR, checked against the limit.
fn encoded(message: &impl Serialize) -> io::Result<Vec<u8>> {
    let mut frame = Vec::new();
    ciborium::into_writer(message, &mut frame).map_err(invalid_data)?;
    accepted(frame.len())?;
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

    fn pair() -> (
        Connection<StreamFrames<DuplexStream>>,
        Connection<StreamFrames<DuplexStream>>,
    ) {
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
            legacy_server.frames.0.write_all(&frame).await.unwrap();
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
        legacy_client.frames.0.write_all(&frame).await.unwrap();
        let error = server
            .accept_hello(&Hello::waos("WaOS 0.3.0"))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert!(error.to_string().contains("10"), "{error}");

        let mut reply = [0u8; 8];
        legacy_client.frames.0.read_exact(&mut reply).await.unwrap();
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

    #[tokio::test]
    async fn the_count_is_four_bytes_little_endian_before_the_message() {
        let (mut sender, mut receiver) = pair();
        tokio::spawn(async move {
            sender
                .send(&Component {
                    bytes: vec![1, 2, 3],
                })
                .await
        });
        let mut frame = [0; 15];
        receiver.frames.0.read_exact(&mut frame).await.unwrap();
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
            .frames
            .0
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
        sender.frames.0.write_all(&[8, 0]).await.unwrap();
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
