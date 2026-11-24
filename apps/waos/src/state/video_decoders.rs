//! The programs' video decoders on the main thread, over Waft's: each decoder opened on Waft's
//! as the engine asks, kept once it opened until the program lets it go, each chunk handed to it
//! and each frame asked of it, and each answer sent back once Waft has it. The decoders are the
//! engine's shell's only where Waft's platform has them.
//!
//! The invariant: every call is answered once, with what Waft answered, or with a failure where
//! the decoder is not open here; and a decoder opened for a program that no longer waits for it
//! is let go at once, so none is kept that no program holds.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::oneshot;
use futures::future::{Either, select};
use waft::App;
use waft::ui::Platform;
use waft::ui::video_decoders as decoders;
use waos::ShellVideoDecoders;

use crate::logic::video_decoder_calls::{DecoderCall, DecoderKey, DecodersOnRequest};

/// Waft's video decoders, served to the engine as its shell's.
pub(crate) struct VideoDecoders {
    shell: Arc<DecodersOnRequest>,
    arriving: UnboundedReceiver<DecoderCall>,
}

/// One of Waft's decoders a program opened, with what ends the watch for its end as it goes.
struct OpenDecoder {
    decoder: Arc<dyn decoders::VideoDecoder>,
    _removed: oneshot::Sender<()>,
}

/// Waft's decoders the programs opened, by the app's key for each.
type OpenDecoders = Rc<RefCell<HashMap<DecoderKey, OpenDecoder>>>;

impl VideoDecoders {
    /// Waft's video decoders on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<VideoDecoders> {
        platform.video_decoders()?;
        let (shell, arriving) = DecodersOnRequest::new();
        Some(VideoDecoders {
            shell: Arc::new(shell),
            arriving,
        })
    }

    /// The decoders as the engine's shell has them: each call goes to the main thread, where
    /// [`serve`](Self::serve) carries it out.
    pub(crate) fn shell(&self) -> Arc<dyn ShellVideoDecoders> {
        Arc::clone(&self.shell) as Arc<dyn ShellVideoDecoders>
    }

    /// From now on, each decoder a program opens is one of Waft's, handed the program's chunks.
    pub(crate) fn serve(self, app: &mut App) {
        let mut arriving = self.arriving;
        app.spawn(async move |cx| {
            let open = OpenDecoders::default();
            while let Some(call) = arriving.next().await {
                cx.update(|app| take(app, &open, call));
            }
        });
    }
}

/// Carries out `call`, from the engine.
fn take(app: &mut App, open: &OpenDecoders, call: DecoderCall) {
    match call {
        DecoderCall::Support {
            description,
            answer,
        } => {
            let platform = app.platform();
            let support = platform
                .video_decoders()
                .map(|system| system.support(&description));
            // A program that ended meanwhile hears nothing.
            if let Some(support) = support {
                let _ = answer.send(support);
            }
        }
        DecoderCall::Open {
            decoder,
            setup,
            options,
            answer,
            ended,
        } => {
            let platform = app.platform();
            let opening = match platform.video_decoders() {
                Some(system) => system.open(setup, options),
                None => Box::pin(std::future::ready(Err(not_open()))),
            };
            keep_once_opened(app, open, decoder, opening, answer, ended);
        }
        DecoderCall::Decode {
            decoder,
            chunk,
            answer,
        } => match opened(open, decoder) {
            Some(waft) => answer_once_done(app, waft.decode(chunk), answer),
            None => drop(answer.send(Err(not_open()))),
        },
        DecoderCall::NextFrame { decoder, answer } => match opened(open, decoder) {
            Some(waft) => answer_once_done(app, waft.next_frame(), answer),
            None => drop(answer.send(None)),
        },
        DecoderCall::Drain { decoder, answer } => match opened(open, decoder) {
            Some(waft) => answer_once_done(app, waft.drain(), answer),
            None => drop(answer.send(Err(not_open()))),
        },
        DecoderCall::Reset(decoder) => {
            if let Some(waft) = opened(open, decoder) {
                waft.reset();
            }
        }
        DecoderCall::Configure {
            decoder,
            setup,
            answer,
        } => match opened(open, decoder) {
            Some(waft) => answer_once_done(app, waft.configure(setup), answer),
            None => drop(answer.send(Err(not_open()))),
        },
        DecoderCall::Remove(decoder) => {
            open.borrow_mut().remove(&decoder);
        }
    }
}

/// Waft's decoder open under `decoder`, if it is.
fn opened(open: &OpenDecoders, decoder: DecoderKey) -> Option<Arc<dyn decoders::VideoDecoder>> {
    open.borrow()
        .get(&decoder)
        .map(|open| Arc::clone(&open.decoder))
}

/// Answers into `answer` where the decoder `opening` opens runs, once Waft has opened it, keeping
/// it under `decoder` while a program waits for it, and tells `ended` why, should it end by
/// itself before the program lets it go.
fn keep_once_opened(
    app: &mut App,
    open: &OpenDecoders,
    decoder: DecoderKey,
    opening: decoders::VideoDecoderOpenFuture,
    answer: oneshot::Sender<Result<decoders::CodecKind, decoders::DecoderError>>,
    ended: oneshot::Sender<decoders::DecoderError>,
) {
    let open = Rc::clone(open);
    app.spawn(async move |_cx| {
        let waft = match opening.await {
            Ok(waft) => waft,
            // A program that ended meanwhile hears nothing.
            Err(error) => return drop(answer.send(Err(error))),
        };
        let (removed, was_removed) = oneshot::channel();
        let has_ended = waft.ended();
        let kind = waft.kind();
        open.borrow_mut().insert(
            decoder,
            OpenDecoder {
                decoder: waft,
                _removed: removed,
            },
        );
        if answer.send(Ok(kind)).is_err() {
            open.borrow_mut().remove(&decoder);
            return;
        }
        if let Either::Left((reason, _)) = select(has_ended, was_removed).await {
            let _ = ended.send(reason);
        }
    });
}

/// Answers into `answer` what `doing` answers, once Waft has.
fn answer_once_done<T: 'static>(
    app: &mut App,
    doing: impl Future<Output = T> + 'static,
    answer: oneshot::Sender<T>,
) {
    app.spawn(async move |_cx| {
        // A program that ended meanwhile hears nothing.
        let _ = answer.send(doing.await);
    });
}

fn not_open() -> decoders::DecoderError {
    decoders::DecoderError::Failed("the decoder is not open in WaOS".to_owned())
}

// The test decodes for real, with VideoToolbox, and reads the frames' planes on the GPU.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::sync::Arc;

    use futures::FutureExt;
    use waft_test::TestPlatform;
    use waos::ProgramEnd;
    use waos_package::ProgramId;

    use crate::state::test_system::{Lines, TestSystem};

    const VIDEO_DECODING: &str = "dev.example.videodecoding";

    /// Twelve pictures of 320 by 240, each decoded up to two ahead of ones shown before it.
    const CLIP: &[u8] = include_bytes!("../../../../examples/video-decoding/clip.h264");

    /// When each of the clip's pictures is shown, in the order they are decoded, counted in
    /// pictures: what `ffprobe` reads of the clip's packets.
    const SHOWN: &str = "0\n3\n1\n2\n5\n4\n8\n6\n7\n11\n9\n10\n";

    /// A program hands the system's decoder a stream it read itself, picture by picture in the
    /// order they are decoded, and reads the decoder's own frames in the order they are shown:
    /// through the engine, to Waft's decoder on the main thread, which is VideoToolbox's, and
    /// back, each frame's planes read on the program's GPU.
    #[test]
    fn a_program_decodes_a_stream_it_reads_with_the_systems_decoder() {
        let decoders = waft_embedder_apple::video_decoders::video_decoders().expect("decoders");
        let platform = TestPlatform::new().with_video_decoders(decoders);
        let system = TestSystem::new(platform);
        let id: ProgramId = VIDEO_DECODING.parse().unwrap();
        let data = system.store.folders(&id).unwrap().data;
        std::fs::write(data.join("clip.h264"), CLIP).unwrap();
        std::fs::write(data.join("shown"), SHOWN).unwrap();
        let lines = Arc::new(Lines::default());
        let running = system.run_as(waos_test_programs::VIDEO_DECODING, VIDEO_DECODING, &lines);

        let ended = || running.ended().now_or_never();
        system.run_tasks_until("the program ends", || ended().is_some());
        let end = ended().expect("ended");
        assert!(
            matches!(end, ProgramEnd::Returned(Ok(()))),
            "{end:?}, having said {:#?}",
            lines.all()
        );
        let mut said = vec![
            "decoding 12 pictures".to_owned(),
            "picture 0: 320x240 in 2 planes".to_owned(),
            // The clip's first pixel is black, which over video's range is these.
            "its first samples: PlaneContent::Luma [16], PlaneContent::Chroma [128, 128]"
                .to_owned(),
        ];
        said.extend((1..12).map(|shown| format!("picture {shown}: 320x240 in 2 planes")));
        assert_eq!(lines.all(), said);
    }
}
