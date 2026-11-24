//! `wasdk:media/video-decoders`: the system's video decoders, which a program hands a stream's
//! chunks and reads decoded frames from. The shell opens each decoder and decodes
//! ([`ShellVideoDecoders`], [`ShellVideoDecoder`]); the engine keeps the two rules every system
//! shares, so no shell keeps them again. The interface is served to a program by `host`.
//!
//! The invariants:
//! - A decoder is handed a chunk that is not a key frame only once a key frame has gone in since
//!   it was opened, drained, reset, configured, or told a chunk was damaged. Until then such a
//!   chunk is answered `bad-data` here and never reaches the shell.
//! - A program holds at most the frames it said it would (`frame-limit`). Each frame lent takes a
//!   slot, which its hardware buffer and their textures hold with it (`video_frames`), and the
//!   shell's next frame is asked for only once a slot is free.

pub(crate) mod host;

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use futures::future::{self, BoxFuture, Either};
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::bindings::wasdk::media::video_decoders::{
    DecoderError, VideoDecodeSupport, VideoDecoderOptions, VideoStreamDescription,
};
use crate::bindings::wasdk::media::video_streams::{CodecKind, EncodedChunk, VideoStreamSetup};
use crate::shell::{DecodedFrame, OpenedVideoDecoder, ShellVideoDecoder, ShellVideoDecoders};
use crate::sync::locked;

/// What a chunk that is not a key frame is answered while the decoder waits for one.
const KEY_FRAME_DUE: &str = "the decoder takes a key frame next";

/// `video-decoders.video-decoders`: the shell's decoders, as the program reaches them.
pub struct VideoDecoders(Arc<dyn ShellVideoDecoders>);

impl VideoDecoders {
    pub(crate) fn new(shell: Arc<dyn ShellVideoDecoders>) -> VideoDecoders {
        VideoDecoders(shell)
    }

    pub(crate) fn support(&self, description: &VideoStreamDescription) -> VideoDecodeSupport {
        self.0.support(description)
    }

    /// A decoder for the stream `setup` begins, lending `options.frame_limit` frames at once.
    pub(crate) fn open(
        &self,
        setup: VideoStreamSetup,
        options: VideoDecoderOptions,
    ) -> impl Future<Output = Result<VideoDecoder, DecoderError>> + Send + 'static + use<> {
        let frame_limit = options.frame_limit;
        let opening = self.0.open(setup, options);
        async move { Ok(VideoDecoder::new(opening.await?, frame_limit)) }
    }
}

/// A decoded frame with its slot among the frames its decoder lends at once.
pub(crate) type LentDecodedFrame = (DecodedFrame, OwnedSemaphorePermit);

/// What comes out of a decoder, until the program takes it: its frames, and why it ended.
pub(crate) struct DecoderOutput {
    pub(crate) frames: BoxStream<'static, LentDecodedFrame>,
    pub(crate) ended: BoxFuture<'static, Result<(), DecoderError>>,
}

/// `video-decoders.video-decoder`: a decoder the shell opened for the program.
pub struct VideoDecoder {
    shell: Arc<dyn ShellVideoDecoder>,
    kind: CodecKind,
    /// Whether the next chunk must be a key frame.
    key_frame_due: Arc<AtomicBool>,
    /// What comes out of it, until the program takes it.
    output: Mutex<Option<DecoderOutput>>,
    /// Dropped with the decoder, which resolves `ended` for a decoder the program let go of.
    _dropped: oneshot::Sender<()>,
}

impl VideoDecoder {
    fn new(opened: OpenedVideoDecoder, frame_limit: u32) -> VideoDecoder {
        let (dropped, was_dropped) = oneshot::channel();
        VideoDecoder {
            shell: opened.decoder,
            kind: opened.kind,
            key_frame_due: Arc::new(AtomicBool::new(true)),
            output: Mutex::new(Some(DecoderOutput {
                frames: lent_one_slot_each(opened.frames, frame_limit),
                ended: ended_or_dropped(opened.ended, was_dropped),
            })),
            _dropped: dropped,
        }
    }

    pub(crate) fn kind(&self) -> CodecKind {
        self.kind
    }

    /// Hands `chunk` to the decoder, or answers `bad-data` here for one that is not the key
    /// frame the decoder waits for.
    pub(crate) fn decode(
        &self,
        chunk: EncodedChunk,
    ) -> impl Future<Output = Result<(), DecoderError>> + Send + 'static + use<> {
        if self.key_frame_due.load(Ordering::Relaxed) && !chunk.key_frame {
            let refused = Err(DecoderError::BadData(KEY_FRAME_DUE.to_owned()));
            return Either::Left(future::ready(refused));
        }
        let key_frame_due = Arc::clone(&self.key_frame_due);
        let decoding = self.shell.decode(chunk);
        Either::Right(async move {
            let decoded = decoding.await;
            match &decoded {
                Ok(()) => key_frame_due.store(false, Ordering::Relaxed),
                Err(DecoderError::BadData(_)) => key_frame_due.store(true, Ordering::Relaxed),
                Err(_) => {}
            }
            decoded
        })
    }

    /// Resolves once every chunk given has come out; a key frame is due after it.
    pub(crate) fn drain(
        &self,
    ) -> impl Future<Output = Result<(), DecoderError>> + Send + 'static + use<> {
        self.then_key_frame_due(self.shell.drain())
    }

    /// Drops what the decoder holds; a key frame is due after it.
    pub(crate) fn reset(&self) {
        self.shell.reset();
        self.key_frame_due.store(true, Ordering::Relaxed);
    }

    /// Another setup from the next chunk on, which is a key frame.
    pub(crate) fn configure(
        &self,
        setup: VideoStreamSetup,
    ) -> impl Future<Output = Result<(), DecoderError>> + Send + 'static + use<> {
        self.then_key_frame_due(self.shell.configure(setup))
    }

    /// What comes out of the decoder, the first time it is asked for.
    pub(crate) fn take_output(&self) -> Option<DecoderOutput> {
        locked(&self.output).take()
    }

    /// `asked` of the shell, after which, where it went well, a key frame is due.
    fn then_key_frame_due(
        &self,
        asked: BoxFuture<'static, Result<(), DecoderError>>,
    ) -> impl Future<Output = Result<(), DecoderError>> + Send + 'static + use<> {
        let key_frame_due = Arc::clone(&self.key_frame_due);
        async move {
            asked.await?;
            key_frame_due.store(true, Ordering::Relaxed);
            Ok(())
        }
    }
}

/// `frames`, each with a slot of the `limit` lent at once: the next is asked of the shell only
/// once a slot is free, so a program holding all of them stops the decoder until it drops one.
fn lent_one_slot_each(
    frames: BoxStream<'static, DecodedFrame>,
    limit: u32,
) -> BoxStream<'static, LentDecodedFrame> {
    let slots = Arc::new(Semaphore::new(limit as usize));
    stream::unfold((frames, slots), |(mut frames, slots)| async move {
        // The slots are never closed, so this fails only as the stream is dropped.
        let slot = Arc::clone(&slots).acquire_owned().await.ok()?;
        let frame = frames.next().await?;
        Some(((frame, slot), (frames, slots)))
    })
    .boxed()
}

/// Why a decoder ended: the shell's reason for one that ended by itself, and nothing for one the
/// program dropped first.
fn ended_or_dropped(
    ended: BoxFuture<'static, DecoderError>,
    was_dropped: oneshot::Receiver<()>,
) -> BoxFuture<'static, Result<(), DecoderError>> {
    async move {
        match future::select(ended, was_dropped).await {
            Either::Left((reason, _)) => Err(reason),
            Either::Right(_) => Ok(()),
        }
    }
    .boxed()
}
