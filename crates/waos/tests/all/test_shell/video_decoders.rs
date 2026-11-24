//! The system's video decoders as the test shell keeps them: each decoder opened recorded with
//! what it was opened for and what it is asked, in order. A decoder puts a frame out for each
//! chunk it is handed, at once and in the order handed, with the chunk's time; a chunk whose first
//! byte is [`DAMAGED`] it answers as damaged; and a test takes the decoder away as the system
//! would.

use std::future::ready;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::video_frames::HeldFrame;
use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use waos::bindings::wasdk::media::video_decoders::{
    DecodeSupport, DecoderError, VideoDecodeSupport, VideoDecoderOptions, VideoStreamDescription,
};
use waos::bindings::wasdk::media::video_frames::{PixelAspect, PixelRect, VideoColor};
use waos::bindings::wasdk::media::video_streams::{CodecKind, EncodedChunk, VideoStreamSetup};
use waos::{
    DecodedFrame, OpenedVideoDecoder, ShellVideoDecoder, ShellVideoDecoders, VideoDecoderFuture,
    VideoDecoderOpenFuture,
};

/// The first byte of a chunk a test decoder answers as damaged.
pub const DAMAGED: u8 = 0xBD;

/// The part of its 64 by 48 buffer a test decoder says is picture.
pub const VISIBLE: PixelRect = PixelRect {
    x: 2,
    y: 4,
    width: 60,
    height: 40,
};

/// Decoders that keep each one opened.
#[derive(Default)]
pub struct TestVideoDecoders {
    opened: Mutex<Vec<Arc<TestDecoder>>>,
}

impl TestVideoDecoders {
    /// The decoders opened, in order.
    pub fn opened(&self) -> Vec<Arc<TestDecoder>> {
        self.opened.lock().unwrap().clone()
    }
}

impl ShellVideoDecoders for TestVideoDecoders {
    /// As a system that lists its software decoders and says nothing of its hardware until
    /// tried.
    fn support(&self, _description: &VideoStreamDescription) -> VideoDecodeSupport {
        VideoDecodeSupport {
            hardware: DecodeSupport::Unknown,
            software: DecodeSupport::Yes,
        }
    }

    fn open(
        &self,
        setup: VideoStreamSetup,
        options: VideoDecoderOptions,
    ) -> VideoDecoderOpenFuture {
        let (put_out, frames) = mpsc::unbounded();
        let (end, ended) = oneshot::channel();
        let decoder = Arc::new(TestDecoder {
            setup,
            options,
            calls: Mutex::default(),
            put_out: Mutex::new(Some(put_out)),
            end: Mutex::new(Some(end)),
            taken: Arc::default(),
        });
        self.opened.lock().unwrap().push(Arc::clone(&decoder));
        let opened = OpenedVideoDecoder {
            frames: counted(frames, Arc::clone(&decoder.taken)),
            // A decoder never taken away never ends by itself.
            ended: Box::pin(async move {
                match ended.await {
                    Ok(reason) => reason,
                    Err(_) => std::future::pending().await,
                }
            }) as BoxFuture<'static, DecoderError>,
            decoder,
            kind: CodecKind::Software,
        };
        Box::pin(ready(Ok(opened)))
    }
}

/// What a decoder was asked.
#[derive(Clone, Debug, PartialEq)]
pub enum DecoderCall {
    /// A chunk, by its timestamp.
    Decode(i64),
    Drain,
    Reset,
    Configure,
}

/// One decoder a program opened, as the test sees it.
pub struct TestDecoder {
    pub setup: VideoStreamSetup,
    pub options: VideoDecoderOptions,
    calls: Mutex<Vec<DecoderCall>>,
    /// Where its frames go; none once it was taken away.
    put_out: Mutex<Option<mpsc::UnboundedSender<SentFrame>>>,
    end: Mutex<Option<oneshot::Sender<DecoderError>>>,
    /// How many of its frames the engine has taken.
    taken: Arc<AtomicUsize>,
}

impl TestDecoder {
    /// What it was asked, in order.
    pub fn calls(&self) -> Vec<DecoderCall> {
        self.calls.lock().unwrap().clone()
    }

    /// How many of its frames the engine has taken.
    pub fn frames_taken(&self) -> usize {
        self.taken.load(Ordering::Relaxed)
    }

    /// The system takes the decoder away: its frames end, and it ends as lost.
    pub fn lose(&self) {
        self.put_out.lock().unwrap().take();
        if let Some(end) = self.end.lock().unwrap().take() {
            let _ = end.send(DecoderError::Lost);
        }
    }

    fn record(&self, call: DecoderCall) {
        self.calls.lock().unwrap().push(call);
    }

    /// Puts out the picture of `chunk`.
    #[cfg(target_vendor = "apple")]
    fn put_out(&self, chunk: &EncodedChunk) {
        let sent = SentFrame {
            picture: HeldFrame::new(super::video_frames::frame_of([0, 0, 255, 255])),
            timestamp: chunk.timestamp,
            duration: chunk.duration,
        };
        if let Some(put_out) = &*self.put_out.lock().unwrap() {
            let _ = put_out.unbounded_send(sent);
        }
    }

    /// No picture: the test's frames are Apple's pixel buffers.
    #[cfg(not(target_vendor = "apple"))]
    fn put_out(&self, _chunk: &EncodedChunk) {}
}

impl ShellVideoDecoder for TestDecoder {
    fn decode(&self, chunk: EncodedChunk) -> VideoDecoderFuture {
        self.record(DecoderCall::Decode(chunk.timestamp));
        if chunk.data.first() == Some(&DAMAGED) {
            let damaged = DecoderError::BadData("the test's damaged chunk".to_owned());
            return Box::pin(ready(Err(damaged)));
        }
        self.put_out(&chunk);
        Box::pin(ready(Ok(())))
    }

    fn drain(&self) -> VideoDecoderFuture {
        self.record(DecoderCall::Drain);
        Box::pin(ready(Ok(())))
    }

    fn reset(&self) {
        self.record(DecoderCall::Reset);
    }

    fn configure(&self, _setup: VideoStreamSetup) -> VideoDecoderFuture {
        self.record(DecoderCall::Configure);
        Box::pin(ready(Ok(())))
    }
}

/// A frame on its way from a decoder to the engine: its picture, and its chunk's time.
struct SentFrame {
    picture: HeldFrame,
    timestamp: i64,
    duration: Option<u64>,
}

impl SentFrame {
    /// The frame as a decoder hands it over, with what the test's stream says of every picture.
    fn into_decoded(self) -> DecodedFrame {
        DecodedFrame {
            frame: self.picture.into_frame(),
            timestamp: self.timestamp,
            duration: self.duration,
            visible_rect: VISIBLE,
            pixel_aspect: PixelAspect {
                width: 4,
                height: 3,
            },
            color: VideoColor {
                primaries: Some(1),
                transfer: Some(1),
                matrix: Some(0),
                range: None,
                chroma_location: None,
            },
            hdr_metadata: None,
        }
    }
}

/// `frames` as a decoder's, each counted in `taken` as the engine takes it.
fn counted(
    frames: mpsc::UnboundedReceiver<SentFrame>,
    taken: Arc<AtomicUsize>,
) -> BoxStream<'static, DecodedFrame> {
    frames
        .map(move |frame| {
            taken.fetch_add(1, Ordering::Relaxed);
            frame.into_decoded()
        })
        .boxed()
}
