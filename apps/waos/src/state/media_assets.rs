//! The programs' media assets on the main thread, over Waft's reading of media: each opened on
//! Waft's as the engine asks, kept until the program lets it go, and each read asked of it, its
//! answer sent back once Waft has read it, in the program's terms, a picture as Waft's frame. The
//! assets are the engine's shell's only where Waft's platform reads media.
//!
//! The invariant: every read is answered once, with what Waft read, or with a failure where the
//! asset is not open here; so no program waits for an answer that cannot come.

use std::collections::HashMap;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::oneshot;
use waft::App;
use waft::ui::{self, Platform};
use waos::ShellMediaAssets;
use waos::bindings::wasdk::media::media_players::{PlayerError, PlayerErrorKind};

use crate::logic::media_asset_calls::{AssetCall, AssetKey, AssetRead, AssetsOnRequest};
use crate::logic::player_states::{program_error, program_metadata, program_track};

/// Waft's reading of media, served to the engine as its shell's.
pub(crate) struct MediaAssets {
    shell: Arc<AssetsOnRequest>,
    arriving: UnboundedReceiver<AssetCall>,
}

/// Waft's assets the programs opened, by the app's key for each.
type OpenAssets = HashMap<AssetKey, Box<dyn ui::MediaAsset>>;

impl MediaAssets {
    /// Waft's reading of media on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<MediaAssets> {
        platform.media_assets()?;
        let (shell, arriving) = AssetsOnRequest::new();
        Some(MediaAssets {
            shell: Arc::new(shell),
            arriving,
        })
    }

    /// The assets as the engine's shell has them: each call goes to the main thread, where
    /// [`serve`](Self::serve) carries it out.
    pub(crate) fn shell(&self) -> Arc<dyn ShellMediaAssets> {
        Arc::clone(&self.shell) as Arc<dyn ShellMediaAssets>
    }

    /// From now on, each asset a program opens is one of Waft's, and is read as the program asks.
    pub(crate) fn serve(self, app: &mut App) {
        let mut arriving = self.arriving;
        app.spawn(async move |cx| {
            let mut open = OpenAssets::new();
            while let Some(call) = arriving.next().await {
                cx.update(|app| take(app, &mut open, call));
            }
        });
    }
}

/// Carries out `call`, from the engine.
fn take(app: &mut App, open: &mut OpenAssets, call: AssetCall) {
    match call {
        AssetCall::Open { asset, url } => {
            if let Some(assets) = app.platform().media_assets() {
                open.insert(asset, assets.open(&url));
            }
        }
        AssetCall::Read(asset, read) => match open.get(&asset) {
            Some(waft) => answer(app, waft.as_ref(), read),
            None => read.fail(PlayerError {
                kind: PlayerErrorKind::Other,
                message: "the system has no reading of media".to_owned(),
            }),
        },
        AssetCall::Remove(asset) => {
            open.remove(&asset);
        }
    }
}

/// Asks `read` of Waft's asset, and answers it once Waft has read it.
fn answer(app: &mut App, waft: &dyn ui::MediaAsset, read: AssetRead) {
    match read {
        AssetRead::Duration(answer) => answer_once_read(app, waft.duration(), answer, |read| read),
        AssetRead::Tracks(answer) => answer_once_read(app, waft.tracks(), answer, |tracks| {
            tracks.into_iter().map(program_track).collect()
        }),
        AssetRead::Metadata(answer) => {
            answer_once_read(app, waft.metadata(), answer, program_metadata)
        }
        AssetRead::FrameAt {
            time,
            tolerance,
            max_size,
            answer,
        } => answer_once_read(
            app,
            waft.frame_at(time, tolerance, max_size),
            answer,
            |frame| frame,
        ),
    }
}

/// Answers into `answer` what `reading` reads, once Waft has, as `program_value` gives the
/// program its value.
fn answer_once_read<T: 'static, U: 'static>(
    app: &mut App,
    reading: ui::MediaAssetFuture<T>,
    answer: oneshot::Sender<Result<U, PlayerError>>,
    program_value: impl FnOnce(T) -> U + 'static,
) {
    app.spawn(async move |_cx| {
        let read = reading.await.map(program_value).map_err(program_error);
        // A program that ended meanwhile hears nothing.
        let _ = answer.send(read);
    });
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use waft_test::{AssetAnswers, FrameAsked, MockMediaAssets, TestPlatform};
    use waos::ProgramEnd;
    use waos_package::ProgramId;

    use super::*;
    use crate::state::test_system::{Lines, TestSystem, real_path};

    const MEDIA_ASSETS: &str = "dev.example.mediaassets";

    fn answers() -> AssetAnswers {
        AssetAnswers {
            duration: Ok(Some(Duration::from_secs(90))),
            tracks: Ok(vec![ui::MediaTrack {
                id: "track:1".to_owned(),
                kind: ui::MediaTrackKind::Video,
                language: String::new(),
                label: String::new(),
                selected: true,
            }]),
            metadata: Ok(ui::MediaMetadata {
                title: Some("Bip Bop".to_owned()),
                ..ui::MediaMetadata::default()
            }),
            frame_size: Err(ui::PlayerError {
                kind: ui::PlayerErrorKind::Decode,
                message: "a damaged frame".to_owned(),
            }),
        }
    }

    #[test]
    fn a_programs_asset_is_wafts_and_what_waft_reads_reaches_the_program() {
        let assets = MockMediaAssets::new(answers());
        let platform = TestPlatform::new().with_media_assets(Rc::new(assets.clone()));
        let system = TestSystem::new(platform);
        let id: ProgramId = MEDIA_ASSETS.parse().unwrap();
        let data = system.store.folders(&id).unwrap().data;
        std::fs::write(data.join("sources"), "/data/clip.mp4\n").unwrap();
        std::fs::write(data.join("clip.mp4"), "a clip").unwrap();
        let lines = Arc::new(Lines::default());
        let running = system.run_as(waos_test_programs::MEDIA_ASSETS, MEDIA_ASSETS, &lines);

        system.run_tasks_until("the program hears the picture fail", || {
            lines.has("/data/clip.mp4: picture failed (decode: a damaged frame)")
        });
        assert!(lines.has("/data/clip.mp4: length 90s"));
        assert!(lines.has("/data/clip.mp4: tracks video*"));
        assert!(lines.has("/data/clip.mp4: title Bip Bop"));
        let opened = &assets.opened()[0];
        let clip = real_path(&data).join("clip.mp4");
        assert_eq!(
            waft::ui::file_uri_path(&opened.url()).as_deref(),
            Some(clip.as_path()),
            "where the clip is on the machine"
        );
        assert_eq!(
            opened.frames_asked(),
            [FrameAsked {
                time: Duration::from_secs(9),
                tolerance: ui::SeekTolerance::ANY,
                max_size: Some(ui::Size::new(160.0, 160.0)),
            }]
        );
        let end = futures::executor::block_on(running.ended());
        assert!(matches!(end, ProgramEnd::Returned(Ok(()))), "{end:?}");
        system.run_tasks_until("Waft's asset is let go", || opened.is_dropped());
    }

    #[test]
    fn a_platform_without_a_reading_of_media_gives_the_engine_none() {
        assert!(MediaAssets::of(&TestPlatform::new()).is_none());
        let reading =
            TestPlatform::new().with_media_assets(Rc::new(MockMediaAssets::new(answers())));
        assert!(MediaAssets::of(&reading).is_some());
    }
}
