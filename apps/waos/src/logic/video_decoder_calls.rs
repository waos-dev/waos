//! Where the programs' video decoders go: each opening, each chunk and each asking for a frame to
//! the main thread, where Waft's decoders are (`state::video_decoders`), and each answer back in
//! the program's terms (`video_values`). The decoders are a GPU's, the one a program's device is
//! on, so an opening and a question of what a stream's support is carry the GPU they are for; a
//! decoder once open is reached by its key alone. A frame is asked for only as the engine asks,
//! which it does once the program has room for one, so none waits between Waft and the engine.
//!
//! The invariant: a decoder's calls reach the main thread after its opening, which answers before
//! the engine holds the decoder, and its removal last, sent as the engine lets go of it; and a
//! call whose answer cannot come, as WaOS quits, is answered at once, so no program waits for it.

use std::future::pending;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use waft::ui;
use waft::ui::video_decoders as decoders;
use waos::bindings::wasdk::media::video_decoders::{
    DecoderError, VideoDecoderOptions, VideoStreamDescription,
};
use waos::bindings::wasdk::media::video_streams::{EncodedChunk, VideoStreamSetup};
use waos::{
    DecodedFrame, GpuId, OpenedVideoDecoder, ShellGpus, ShellVideoDecoder, ShellVideoDecoders,
    VideoDecodeSupportFuture, VideoDecoderFuture, VideoDecoderOpenFuture,
};

use super::video_values::{
    decoded_frame, program_error, program_kind, program_support, waft_chunk, waft_description,
    waft_gpu, waft_options, waft_setup,
};

/// The id the app gives each decoder a program opens, which its calls carry.
pub(crate) type DecoderKey = u64;

/// Where a decoder's answer to a chunk, a drain or a setup goes.
type Answer = oneshot::Sender<Result<(), decoders::DecoderError>>;

/// One call of the programs' video decoders, on its way to the main thread, with where its
/// answer goes.
pub(crate) enum DecoderCall {
    /// What the system says of decoding a stream on the GPU `gpu` names, the system's default
    /// where none.
    Support {
        gpu: Option<ui::GpuId>,
        description: decoders::VideoStreamDescription,
        answer: oneshot::Sender<decoders::VideoDecodeSupport>,
    },
    /// A program opens a decoder of the GPU `gpu` names, the system's default where none, kept
    /// under `decoder` once it opened; `ended` is told why, should it end by itself.
    Open {
        gpu: Option<ui::GpuId>,
        decoder: DecoderKey,
        setup: decoders::VideoStreamSetup,
        options: decoders::VideoDecoderOptions,
        answer: oneshot::Sender<Result<decoders::CodecKind, decoders::DecoderError>>,
        ended: oneshot::Sender<decoders::DecoderError>,
    },
    Decode {
        decoder: DecoderKey,
        chunk: decoders::EncodedChunk,
        answer: Answer,
    },
    /// The decoder's next frame; none once it has ended.
    NextFrame {
        decoder: DecoderKey,
        answer: oneshot::Sender<Option<Arc<dyn ui::VideoFrame>>>,
    },
    Drain {
        decoder: DecoderKey,
        answer: Answer,
    },
    Reset(DecoderKey),
    Configure {
        decoder: DecoderKey,
        setup: decoders::VideoStreamSetup,
        answer: Answer,
    },
    /// The program let one of its decoders go.
    Remove(DecoderKey),
}

impl DecoderCall {
    /// Answers that WaOS is quitting, whatever it asks.
    fn fail(self) {
        // A program that ended meanwhile hears nothing.
        match self {
            DecoderCall::Support { answer, .. } => drop(answer.send(no_support())),
            DecoderCall::Open { answer, .. } => drop(answer.send(Err(quitting()))),
            DecoderCall::Decode { answer, .. }
            | DecoderCall::Drain { answer, .. }
            | DecoderCall::Configure { answer, .. } => drop(answer.send(Err(quitting()))),
            DecoderCall::NextFrame { answer, .. } => drop(answer.send(None)),
            DecoderCall::Reset(_) | DecoderCall::Remove(_) => {}
        }
    }
}

/// The video decoders of every GPU, as the engine reaches them: every call to the main thread,
/// each decoder under a key of its own whichever GPU's it is.
pub(crate) struct GpusOnRequest {
    calls: Calls,
    last_decoder: Arc<AtomicU64>,
}

impl GpusOnRequest {
    /// The GPUs' decoders, and where their calls arrive on the main thread.
    pub(crate) fn new() -> (GpusOnRequest, UnboundedReceiver<DecoderCall>) {
        let (calls, arriving) = mpsc::unbounded();
        let gpus = GpusOnRequest {
            calls: Calls(calls),
            last_decoder: Arc::default(),
        };
        (gpus, arriving)
    }
}

impl ShellGpus for GpusOnRequest {
    /// Decoders whose every call carries `gpu`; a GPU without decoders answers that it decodes
    /// nothing, and opens no decoder.
    fn video_decoders(&self, gpu: Option<GpuId>) -> Option<Arc<dyn ShellVideoDecoders>> {
        Some(Arc::new(DecodersOnRequest {
            gpu: gpu.map(waft_gpu),
            calls: self.calls.clone(),
            last_decoder: Arc::clone(&self.last_decoder),
        }))
    }
}

/// One GPU's video decoders, as the engine reaches them: every call to the main thread.
struct DecodersOnRequest {
    /// The GPU they are of, the system's default where none.
    gpu: Option<ui::GpuId>,
    calls: Calls,
    last_decoder: Arc<AtomicU64>,
}

impl ShellVideoDecoders for DecodersOnRequest {
    fn support(&self, description: VideoStreamDescription) -> VideoDecodeSupportFuture {
        let description = waft_description(description);
        let (answer, answered) = oneshot::channel();
        self.calls.send(DecoderCall::Support {
            gpu: self.gpu,
            description,
            answer,
        });
        Box::pin(async move { program_support(answered.await.unwrap_or_else(|_| no_support())) })
    }

    fn open(
        &self,
        setup: VideoStreamSetup,
        options: VideoDecoderOptions,
    ) -> VideoDecoderOpenFuture {
        let decoder = self.last_decoder.fetch_add(1, Ordering::Relaxed) + 1;
        let calls = self.calls.clone();
        let (answer, answered) = oneshot::channel();
        let (ended, has_ended) = oneshot::channel();
        self.calls.send(DecoderCall::Open {
            gpu: self.gpu,
            decoder,
            setup: waft_setup(setup),
            options: waft_options(options),
            answer,
            ended,
        });
        Box::pin(async move {
            let kind = answered
                .await
                .unwrap_or_else(|_| Err(quitting()))
                .map_err(program_error)?;
            Ok(OpenedVideoDecoder {
                frames: frames_of(decoder, calls.clone()),
                ended: ended_by_itself(has_ended),
                decoder: Arc::new(DecoderOnRequest { decoder, calls }),
                kind: program_kind(kind),
            })
        })
    }
}

/// A decoder's frames, each asked of the main thread as the engine asks for the next, and ended
/// once Waft's decoder has; a frame of a kind the engine does not take is passed over.
fn frames_of(decoder: DecoderKey, calls: Calls) -> BoxStream<'static, DecodedFrame> {
    stream::unfold(calls, move |calls| async move {
        loop {
            let (answer, answered) = oneshot::channel();
            calls.send(DecoderCall::NextFrame { decoder, answer });
            let frame = answered.await.ok().flatten()?;
            if let Some(decoded) = decoded_frame(&frame) {
                return Some((decoded, calls));
            }
        }
    })
    .boxed()
}

/// Why the decoder ended by itself, as Waft tells; never, for one that does not.
fn ended_by_itself(
    has_ended: oneshot::Receiver<decoders::DecoderError>,
) -> BoxFuture<'static, DecoderError> {
    async move {
        match has_ended.await {
            Ok(reason) => program_error(reason),
            Err(_) => pending().await,
        }
    }
    .boxed()
}

/// One decoder a program opened, as the engine reaches it.
struct DecoderOnRequest {
    decoder: DecoderKey,
    calls: Calls,
}

impl ShellVideoDecoder for DecoderOnRequest {
    fn decode(&self, chunk: EncodedChunk) -> VideoDecoderFuture {
        let (decoder, chunk) = (self.decoder, waft_chunk(chunk));
        self.calls.ask(move |answer| DecoderCall::Decode {
            decoder,
            chunk,
            answer,
        })
    }

    fn drain(&self) -> VideoDecoderFuture {
        let decoder = self.decoder;
        self.calls
            .ask(move |answer| DecoderCall::Drain { decoder, answer })
    }

    fn reset(&self) {
        self.calls.send(DecoderCall::Reset(self.decoder));
    }

    fn configure(&self, setup: VideoStreamSetup) -> VideoDecoderFuture {
        let (decoder, setup) = (self.decoder, waft_setup(setup));
        self.calls.ask(move |answer| DecoderCall::Configure {
            decoder,
            setup,
            answer,
        })
    }
}

impl Drop for DecoderOnRequest {
    fn drop(&mut self) {
        self.calls.send(DecoderCall::Remove(self.decoder));
    }
}

/// Where the calls go: the main thread.
#[derive(Clone)]
struct Calls(UnboundedSender<DecoderCall>);

impl Calls {
    fn send(&self, call: DecoderCall) {
        if let Err(going) = self.0.unbounded_send(call) {
            tracing::info!("WaOS is quitting: a video decoder's call goes nowhere");
            going.into_inner().fail();
        }
    }

    /// Asks the call `call` makes of the answer it carries, and answers what comes back in the
    /// program's terms, or that WaOS is quitting where nothing will.
    fn ask(&self, call: impl FnOnce(Answer) -> DecoderCall) -> VideoDecoderFuture {
        let (answer, answered) = oneshot::channel();
        self.send(call(answer));
        Box::pin(async move {
            answered
                .await
                .unwrap_or_else(|_| Err(quitting()))
                .map_err(program_error)
        })
    }
}

fn quitting() -> decoders::DecoderError {
    decoders::DecoderError::Failed("WaOS is quitting".to_owned())
}

/// What is said of decoding a stream where nothing can ask the system, or the GPU has no
/// decoders: that it decodes none.
pub(crate) fn no_support() -> decoders::VideoDecodeSupport {
    decoders::VideoDecodeSupport {
        hardware: decoders::DecodeSupport::No,
        software: decoders::DecodeSupport::No,
    }
}
