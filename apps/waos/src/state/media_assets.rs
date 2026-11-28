//! The programs' media assets, over Waft's reading of media: the engine opens an asset from a
//! program's thread, or from the thread its link's host was looked up on, and reads it from the
//! program's; each call is posted to the App's thread (`app_thread_bound`), where the asset is
//! opened on Waft's, kept until the program lets it go, and read as asked, its answer sent back
//! once Waft has read it, in the program's terms, a picture as the platform's own frame. The assets
//! are the engine's shell's only where Waft's platform reads media.
//!
//! The invariant: an asset's calls reach the App's thread in the order made, its opening first and
//! its removal last, posted as the engine lets go of it; and every read is answered once, with what
//! Waft read, with a failure where the asset is not open there, or at once as WaOS quits, so no
//! program waits for an answer that cannot come.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::{FutureExt, TryFutureExt};
use waft::App;
use waft::ui::{self, Platform, PlatformRef};
use waos::bindings::wasdk::media::media_players::{
    MediaMetadata, MediaTrack, PlayerError, PlayerErrorKind, SeekTolerance,
};
use waos::bindings::wasdk::platform::windows::Size;
use waos::{AppInstance, MediaAssetFuture, ShellMediaAsset, ShellMediaAssets, VideoFrame};

use super::app_thread_bound::{AppThreadBound, AppThreadCalls};
use crate::logic::camera_values::waft_size;
use crate::logic::frame_readers::engine_frame;
use crate::logic::player_states::{program_error, program_metadata, program_track, waft_tolerance};

/// The id the app gives each asset a program opens, which its calls carry.
type AssetKey = u64;

/// Waft's reading of media, served to the engine as its shell's.
pub(crate) struct MediaAssets {
    shell: Arc<AssetsOnRequest>,
    calls: AppThreadCalls<OpenAssets>,
}

impl MediaAssets {
    /// Waft's reading of media on `platform`, to serve to the engine; none where it has none.
    pub(crate) fn of(platform: &dyn Platform) -> Option<MediaAssets> {
        platform.media_assets()?;
        let (assets, calls) = AppThreadBound::new();
        let shell = AssetsOnRequest {
            assets,
            last_asset: AtomicU64::default(),
        };
        Some(MediaAssets {
            shell: Arc::new(shell),
            calls,
        })
    }

    /// The assets as the engine's shell has them: each call is posted to the App's thread, where
    /// [`serve`](Self::serve) runs it.
    pub(crate) fn shell(&self) -> Arc<dyn ShellMediaAssets> {
        Arc::clone(&self.shell) as Arc<dyn ShellMediaAssets>
    }

    /// From now on, each asset a program opens is one of Waft's, and is read as the program asks.
    pub(crate) fn serve(self, app: &mut App) {
        let open = OpenAssets {
            platform: app.platform(),
            open: HashMap::new(),
        };
        self.calls.serve(app, open);
    }
}

/// The app's media assets, as the engine reaches them: every call is posted to the App's thread.
struct AssetsOnRequest {
    assets: AppThreadBound<OpenAssets>,
    last_asset: AtomicU64,
}

impl ShellMediaAssets for AssetsOnRequest {
    fn open(&self, program: &AppInstance, url: &str) -> Box<dyn ShellMediaAsset> {
        tracing::info!(
            program = program.name(),
            url,
            "a program opens a media asset"
        );
        let key = self.last_asset.fetch_add(1, Ordering::Relaxed) + 1;
        let url = url.to_owned();
        let _ = self.assets.post(move |assets| assets.open(key, &url));
        Box::new(AssetOnRequest {
            key,
            assets: self.assets.clone(),
        })
    }
}

/// One asset, as the engine holds it: its reads are posted to the App's thread, and its removal
/// as it drops.
struct AssetOnRequest {
    key: AssetKey,
    assets: AppThreadBound<OpenAssets>,
}

impl AssetOnRequest {
    /// Asks `read` of Waft's asset on the App's thread, and answers what it read, in the program's
    /// terms.
    fn read<A: Send + 'static, F: Future<Output = Result<A, ui::PlayerError>> + 'static>(
        &self,
        read: impl FnOnce(&dyn ui::MediaAsset) -> F + Send + 'static,
    ) -> MediaAssetFuture<A> {
        let key = self.key;
        let answered = self.assets.ask(move |assets| {
            let reading = assets.open.get(&key).map(|waft| read(waft.as_ref()));
            async move {
                match reading {
                    Some(reading) => reading.await.map_err(program_error),
                    None => Err(failed("the system has no reading of media")),
                }
            }
        });
        Box::pin(answered.map(|read| read.unwrap_or_else(|_| Err(failed("WaOS is quitting")))))
    }
}

impl ShellMediaAsset for AssetOnRequest {
    fn duration(&self) -> MediaAssetFuture<Option<Duration>> {
        self.read(|waft| waft.duration())
    }

    fn tracks(&self) -> MediaAssetFuture<Vec<MediaTrack>> {
        self.read(|waft| {
            waft.tracks()
                .map_ok(|tracks| tracks.into_iter().map(program_track).collect())
        })
    }

    fn metadata(&self) -> MediaAssetFuture<MediaMetadata> {
        self.read(|waft| waft.metadata().map_ok(program_metadata))
    }

    /// Waft's frame, as the engine's: the platform's own behind it.
    fn frame_at(
        &self,
        time: Duration,
        tolerance: SeekTolerance,
        max_size: Option<Size>,
    ) -> MediaAssetFuture<VideoFrame> {
        let tolerance = waft_tolerance(tolerance);
        let max_size = max_size.map(waft_size);
        let frame = self.read(move |waft| waft.frame_at(time, tolerance, max_size));
        Box::pin(async move {
            engine_frame(frame.await?.as_ref()).ok_or_else(|| PlayerError {
                kind: PlayerErrorKind::Unsupported,
                message: "the system's picture is of a kind WaOS does not hand out".to_owned(),
            })
        })
    }
}

impl Drop for AssetOnRequest {
    fn drop(&mut self) {
        let key = self.key;
        let _ = self.assets.post(move |assets| {
            assets.open.remove(&key);
        });
    }
}

/// Waft's assets the programs opened, by the app's key for each, on the App's thread.
struct OpenAssets {
    platform: PlatformRef,
    open: HashMap<AssetKey, Box<dyn ui::MediaAsset>>,
}

impl OpenAssets {
    /// Opens Waft's asset of `url` as `key`, where Waft's platform reads media.
    fn open(&mut self, key: AssetKey, url: &str) {
        if let Some(assets) = self.platform.media_assets() {
            self.open.insert(key, assets.open(url));
        }
    }
}

/// What a read answers that failed for `message`.
fn failed(message: &str) -> PlayerError {
    PlayerError {
        kind: PlayerErrorKind::Other,
        message: message.to_owned(),
    }
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
        let data = system.storage.folders(&id).unwrap().data;
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
