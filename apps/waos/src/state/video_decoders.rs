//! The programs' video decoders on the main thread, over Waft's: the engine's calls arrive here
//! and are carried out on the decoders of the GPU each names (`logic::open_video_decoders`), the
//! one a program's device is on, which Waft's platform hands out by its id and keeps, so a
//! decoded picture opens where it is drawn. Each wait for an answer is a task of the app's. The
//! engine's shell has GPUs only where Waft's platform has decoders at all, as its default GPU
//! does; a GPU without them answers that it decodes nothing.

use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use waft::App;
use waft::ui::Platform;
use waos::ShellGpus;

use crate::logic::open_video_decoders::OpenVideoDecoders;
use crate::logic::video_decoder_calls::{DecoderCall, GpusOnRequest};

/// Waft's video decoders of every GPU, served to the engine as its shell's GPUs.
pub(crate) struct VideoDecoders {
    shell: Arc<GpusOnRequest>,
    arriving: UnboundedReceiver<DecoderCall>,
}

impl VideoDecoders {
    /// Waft's video decoders on `platform`, to serve to the engine; none where its default GPU has
    /// none, as a platform that decodes no video.
    pub(crate) fn of(platform: &dyn Platform) -> Option<VideoDecoders> {
        platform.gpu(None)?.video_decoders()?;
        let (shell, arriving) = GpusOnRequest::new();
        Some(VideoDecoders {
            shell: Arc::new(shell),
            arriving,
        })
    }

    /// The GPUs as the engine's shell has them: each of their decoders' calls goes to the main
    /// thread, where [`serve`](Self::serve) carries it out.
    pub(crate) fn shell(&self) -> Arc<dyn ShellGpus> {
        Arc::clone(&self.shell) as Arc<dyn ShellGpus>
    }

    /// From now on, each decoder a program opens is one of Waft's, of the GPU its device is on,
    /// handed the program's chunks.
    pub(crate) fn serve(self, app: &mut App) {
        let mut arriving = self.arriving;
        app.spawn(async move |cx| {
            let open = OpenVideoDecoders::default();
            while let Some(call) = arriving.next().await {
                cx.update(|app| take(app, &open, call));
            }
        });
    }
}

/// Carries out `call`, from the engine, on Waft's decoders of the GPU it names.
fn take(app: &mut App, open: &OpenVideoDecoders, call: DecoderCall) {
    let platform = app.platform();
    let decoders_of = |gpu| platform.gpu(gpu)?.video_decoders();
    let mut start_task = |task| {
        app.spawn(async move |_cx| task.await);
    };
    open.take(&decoders_of, &mut start_task, call);
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
        let data = system.storage.folders(&id).unwrap().data;
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
            "and in memory: PlaneContent::Luma [16], PlaneContent::Chroma [128, 128]".to_owned(),
        ];
        said.extend((1..12).map(|shown| format!("picture {shown}: 320x240 in 2 planes")));
        assert_eq!(lines.all(), said);
    }
}
